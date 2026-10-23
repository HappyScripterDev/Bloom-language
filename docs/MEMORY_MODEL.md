# Wie Bloom Speicher verwaltet

Stand: 0.15.0. Bloom zählt Referenzen. Jeder Wert weiß, wie viele Stellen
ihn gerade benutzen, und gibt sich selbst frei, wenn keiner mehr da ist.
Kein GC, der irgendwann aufräumt, und kein `free()` im Anwendungscode.

Betroffen sind:

- `string` – unveränderliche UTF-8-Bytes,
- `[T]` – Arrays (Referenzwerte, stabiler Header mit `rc`),
- `Slice<T>` – Views auf Array-Bereiche (zählen den Header mit),
- Structs und Enums, die solche Werte (rekursiv) enthalten.

`int`, `float`, `bool`, `char` und Typen ohne verwaltete Felder brauchen
nichts davon und erzeugen auch keinen Heap-Verkehr.

Das Modell ist **nicht threadsicher**. Es gibt ja noch keine Threads, und
die Zähler sind einfache `size_t`-Inkremente ohne Atomik. Wenn Threads
kommen, muss das hier neu gedacht werden (atomare Zähler und Regeln für
geteilte veränderbare Arrays). Das ist ein eigenes Projekt, nichts, was
man mal eben mit einbaut.

## 1. Laufzeit-Repräsentation

### Strings

```c
typedef struct { size_t rc; char *data; size_t len; } bloom_str;
```

- Heap-String: **eine** Allokation `[Header | Bytes | '\0']`; `data` zeigt
  auf die Bytes direkt hinter dem Header, der Header ist über
  `data - sizeof(bloom_str)` erreichbar. `rc` beginnt bei 1.
- String-Literale werden als **statische** `bloom_str`-Werte emittiert
  (`rc == SIZE_MAX` = unsterblich). `clone`/`drop` sind für sie No-Ops;
  sie werden nie freigegeben und nie kopiert.
- `bloom_str_clone(s)` erhöht `rc` (außer bei statischen Strings).
- `bloom_str_drop(&s)` verringert `rc`; bei 0 wird der Block freigegeben.
  Die Funktion ist idempotent (nullt danach `data`/`len`).

### Arrays

```c
typedef struct bloom_arr_X_s { size_t rc; T *data; size_t len; size_t cap; size_t gen; } *bloom_arr_X;
```

- **Stabiler Header:** Der Header wird getrennt vom Elementpuffer allokiert
  und bewegt sich nie. Wachstum (`reserve`/`realloc`) ändert nur `data`;
  der Header (und alles, was auf ihn zeigt) bleibt gültig.
- Referenzsemantik bleibt erhalten: Kopien teilen den Header; `push`,
  `pop`, `insert`, `remove`, `set` wirken über alle Kopien.
- `clone`/`drop` verwalten den Header. Beim letzten `drop` werden die
  Elemente freigegeben (bei `[string]` rekursiv per `bloom_str_drop`),
  danach Puffer und Header.
- `gen` ist die **Layout-Generation**: `insert`, `remove` und ein
  erfolgreiches `pop` erhöhen sie (Elementpositionen verschieben sich);
  `push`, `set` und fehlgeschlagene Operationen nicht.
- Element-Besitz: `push`/`insert`/`set` **klonen** das Element (das Array
  erhält eine Referenz); `pop`/`remove` **transferieren** das Element
  (Besitz wandert zum Ergebnis).
- `slice(a, start, end)` erzeugt eine **Kopie** der Elemente (siehe
  Abschnitt 6).

### Slice-Views (`Slice<T>`)

```c
typedef struct { bloom_arr_X hdr; int64_t start; size_t len; size_t gen; } bloom_slice_X;
```

- Ein View speichert **keinen** rohen Pufferzeiger, sondern die
  Header-Referenz, den Startoffset, die View-Länge und die beim Erzeugen
  gültige Layout-Generation.
- `slice_view` erhöht `hdr->rc`: Der Header (und damit der Puffer) lebt,
  solange ein Array-Besitzer **oder** ein View ihn referenziert. `clone`
  erhöht, `drop` verringert und gibt beim letzten Besitzer frei (über
  dieselbe Header-Freigabe wie Arrays, inklusive Element-Drops).
- Jeder Elementzugriff (`get`/`set`) und `len` prüfen:
  Header vorhanden, `hdr->gen == view.gen` (sonst „invalidated“),
  View-Index in `[0, len)` und `start + i < hdr->len`. Der Zugriff liest
  stets `hdr->data[start + i]` — nach einer Reallokation also den
  **aktuellen** Puffer.
- Schreiben (`set`) folgt dem Element-Besitz der Arrays: alten Wert
  freigeben, neuen klonen. `len`/`cap` und die Generation ändern sich nicht.
- Invalidierte Views greifen nie auf den Puffer zu: Sie melden einen
  Laufzeitfehler mit Quelle (Exit 70). Der Fehlerpfad kann dabei keine
  Elemente freigeben, die dem View nicht mehr gehören (der View hält nur
  den Header).

### Structs und Enums

Der Codegenerator emittiert pro verwalteter Instanziierung Hilfsfunktionen:

```c
static bloom_struct_User bloom_clone_User(bloom_struct_User v);   /* klont jedes verwaltete Feld */
static void bloom_drop_User(bloom_struct_User *p);                /* gibt Felder frei */
```

Bei Enums wird nur die **aktive Variante** behandelt (`switch (tag)`);
nach dem Drop wird `tag = -1` gesetzt (idempotent, doppelte Freigabe
unmöglich). Für Structs/Enums ohne verwaltete Felder werden keine
Hilfsfunktionen erzeugt und keine Zähleroperationen ausgeführt.

## 2. Ownership-Regeln des generierten Codes

Das Modell ist eine **Besitzdisziplin pro Variablen-Slot und pro
Statement** (keine benutzersichtbare Borrow-Syntax):

1. **Slots besitzen**: Jede Bloom-Variable (auch Parameter), jedes
   Struct-Feld, jedes Array-Element, jede aktive Enum-Nutzlast und jeder
   Slice-View hält genau eine Referenz.
2. **Store = Clone**: Beim Speichern in einen Slot wird der Wert geklont
   (`bloom_str_clone` usw.). Beim Überschreiben wird der alte Wert zuerst
   freigegeben.
3. **Statement-Temporäre**: Ausdrücke, die neue verwaltete Werte liefern
   (Konkatenation, `trim`, `split`, Funktionsergebnisse, Literale …),
   materialisieren ein besessenes Temporär, das am Ende der Anweisung
   freigegeben wird — es sei denn, es wird in einen Slot transferiert
   (dann wandert der Besitz ohne Zähleränderung).
4. **Parameter sind besessen**: Der Aufrufer klont Argumente für
   Parameter; die Funktion gibt ihre Parameter am Ende frei. Der
   `self`-Empfänger ist **geliehen** (wird nicht geklont und nicht
   freigegeben); `var self` erhält einen Zeiger.
5. **Rückgaben sind besessen**: `return` klont geliehene Werte, bevor
   lokale Slots freigegeben werden; Besitz-Temporäre werden transferiert.
6. **Cleanup an Ausgängen**: Beim Verlassen eines Blocks werden dessen
   Slots in umgekehrter Reihenfolge freigegeben; `return`, `break`,
   `continue` und der Fehlerpfad von `?` geben die betroffenen
   (verschachtelten) Scopes frei. Die Drops werden pro Pfad emittiert
   (keine doppelte Freigabe, da Pfade sich ausschließen).
7. **`?`-Fehlerpfad**: klont die Fehlernutzlast in einen Rückgabe-Slot,
   gibt Statement-Temporäre und alle Funktions-Slots frei und kehrt
   zurück.

Die Hilfsfunktionen sind so gebaut, dass NULL/`{0}`-Werte No-Ops sind;
`drop` ist idempotent. Es gibt keine ungeprüfte Zählerarithmetik, die zu
ungültigem Speicher führen könnte.

## 3. Fehlerfälle und Invarianten

- **Allokationsfehler** (OOM) führen zu einem definierten Laufzeitfehler
  (Exit 70), nicht zu undefiniertem Verhalten.
- **Kapazitätsänderungen** (`reserve`/`realloc`) ändern den Header nicht;
  Kopien bleiben gültig.
- **Doppelte Freigabe**: strukturell ausgeschlossen (jeder Pfad gibt jeden
  Slot genau einmal frei); `drop`-Funktionen sind zusätzlich idempotent.
- **Leere Werte**: `bloom_str{0}`, `NULL`-Arrays und leere Elementpuffer
  sind gültige Zustände; Drop-Operationen prüfen auf NULL.
- **Statische Literale**: unsterblich, `clone`/`drop` No-Ops.

## 4. Instrumentierung und Tests

Die Laufzeit führt einen globalen Zähler offener Allokationen
(`bloom_live_allocs`) und offener Sockets (`bloom_live_sockets`, inklusive
TLS-Verbindungen); `bloom_mem_report()` gibt beide am Programmende auf
stderr aus, wenn `BLOOM_MEM_REPORT=1` gesetzt ist:

```text
live allocations: 0, live sockets: 0
```

Die Integrationstests nutzen das für echte Leak-Nachweise: Programme mit
Schleifen, frühen Rückgaben, `?`-Propagation, Structs/Enums mit
verwalteten Feldern, `var self`-Mutation, Globals, großen Sammlungen und
Slice-Views (Kopien, Rückgaben, Reallokationen, verwaltete Elemente)
müssen mit `live allocations: 0` enden. Dasselbe gilt für die
Standardbibliothek: Strings und Arrays aus `std::fs` (Dateiinhalte,
Verzeichnislisten) und `std::process` (`ProcessOutput` mit stdout/stderr)
sind frische Besitzwerte, die als `Result`-Nutzlast in das bestehende
Modell wandern (Ergebnis-`Ok`/`Err` trägt genau eine Referenz; die
generierten Clone-/Drop-Helfer der `Result`-Instanzen geben sie frei).
Auch `format_utc` liefert einen frischen String. Netzwerk- und
HTTP-Ergebnisse (`ProcessOutput`-artige Structs, `HttpRequest`,
`HttpResponse`, `Result`-Nutzlasten) sind frische Besitzwerte und laufen
durch dieselben generierten Clone-/Drop-Helfer. **Socket- und TLS-Handles
sind keine Besitzwerte**: Sie sind Runtime-IDs ohne Referenzzählung;
`tcp_close` bzw. `tls_close` sind ausdrücklich erforderlich, und die
Registry lehnt unbekannte oder bereits geschlossene Handles ab (kein
automatischer Cleanup, kein Doppel-Free möglich); `live sockets` weist
nicht geschlossene Verbindungen in den Tests nach. **JSON-Werte
(`std::json`) sind dagegen Besitzwerte**: Das Handle lebt in
`std::json::JsonValue` und wird von derselben Referenzzählung verwaltet —
Kopieren hält den Baum, das Scope-Ende gibt ihn frei (kein manuelles
`free`). Teilwerte aus `get`/`array_get` sind mitgezählte Referenzen ohne
tiefe Kopie; Zyklen sind ausgeschlossen (`set`/`push` lehnen sie ab),
tiefe Bäume werden iterativ freigegeben, und jeder JSON-Knoten zählt in
`live allocations`. **Geteilte Teilbäume sind refcount-treu:** Beim
Freigeben eines Baums werden die Zähler der Kinder dekrementiert und nur
Knoten mit Zähler 0 freigegeben — ein geparster Wert, der zusätzlich in
einem anderen Objekt steckt oder von einer Bloom-Variablen gehalten
wird, bleibt gültig (Regressionstest
`json_shared_subtree_release_natively`; ein 0.12.0-Fehler hier ist in
0.13.0 behoben). **Der Web-Router (`std::web::Router`) ist ebenfalls ein
verwalteter Besitzwert** (gleiche Mechanik wie `JsonValue`, `Type::Web`),
und `Match.params` ist ein mitverwalteter `json::JsonValue`. **Auch das
geparste Template (`std::html::Template`, `Type::Html`) ist ein
verwalteter Besitzwert**: es hält die Instruktionsliste, wird beim
Kopieren retained und am Scope-Ende freigegeben; ein fehlgeschlagenes
Parsen liefert `Err` ohne Template, ein fehlgeschlagenes Rendern gibt
den Ausgabepuffer frei. `render_page` rendert vollständig vor dem
Schreiben, daher entstehen bei Renderfehlern keine halben Antworten.
**Byte-Puffer (`std::fs::Bytes`, `Type::Bytes`) sind ebenfalls
verwaltete Besitzwerte**: `fs::read_bytes` liefert ein refcountetes
Handle auf die exakten Dateibytes (auch NUL-Bytes), Kopieren retained,
das Scope-Ende gibt Puffer und Handle frei; `http::write_response_bytes`
liest den Puffer nur und übernimmt keinen Besitz. Die Kontrolle über
den Puffer liegt ausschließlich in der Laufzeit, daher kann Bloom-Code
keine Länge fälschen. **Formulardaten (`std::web::FormData`,
`Type::Form`) folgen demselben Muster**: das Handle besitzt ein Array
aus Name/Wert-Paaren, jedes Paar hält je eine Referenz auf seine
Strings. Kopieren retained das Handle, das Scope-Ende gibt alle Paare
und die Strings frei; ein Parsefehler gibt das halbfertige Formular
samt bereits gelesener Einträge frei. Ein `Ok` von `form_get`,
`form_first` oder `form_values` liefert geklonte Strings bzw. ein
eigenes Array, nie eine Referenz in das Formular hinein. **Cookies
(`Type::Cookies`), Session-Stores (`Type::SessionStore`) und Sessions
(`Type::Session`) sind ebenfalls verwaltete Besitzwerte.** Ein
`Cookies`-Handle hält seine Name/Wert-Strings. Ein Session-Store hält
pro Session eine Referenz; ein `Session`-Handle hält eine weitere und
schützt die Daten (ID, CSRF-Token, JSON-Objekt) auch dann, wenn die
Session aus dem Store gelöscht oder durch Ablauf verworfen wurde.
`session_data` liefert eine retained Referenz auf dasselbe JSON-Objekt:
Mutationen sind für alle Handles sichtbar, und das Objekt lebt, solange
irgendein Handle es hält. `session_destroy` und `session_expire` geben
nur die Store-Referenz frei; Freigabe erfolgt erst, wenn der letzte
Handle den Scope verlässt. `session_new` legt zwei Referenzen an (Store
plus Rückgabewert), `session_load` erhöht den Zähler um eins.
**Datenbank-Handles (`std::db`, `Type::Db`/`Type::Stmt`) sind ebenfalls
referenzgezählt.** Ein `Statement` hält eine Besitzreferenz auf seine
`Database`; die SQLite-Verbindung wird erst geschlossen, wenn sowohl
das Bloom-`Database`-Handle als auch alle daraus abgeleiteten
Statements freigegeben sind. Ein Statement darf daher die ursprüngliche
`Database`-Variable überleben (getestet: `detached_select`). Beim
Freigeben eines Statements wird `sqlite3_finalize` aufgerufen, beim
Freigeben der letzten Datenbankreferenz `sqlite3_close_v2` (eine offene
Transaktion wird dabei zurückgerollt). Fehler beim Präparieren, Binden
oder Ausführen geben die beteiligten Teilressourcen sofort frei;
abgelehnte SQL-Texte (mehrere Anweisungen, NUL-Bytes) finalisieren das
bereits erzeugte Statement. Alle DB-Tests prüfen
`live allocations: 0`.
Alle Stdlib-Tests
laufen unter `BLOOM_MEM_REPORT=1`. Getestet wird nativ mit MSVC
(Windows). Valgrind/ASan sind auf dieser Toolchain nicht verfügbar und
wurden nicht verwendet; die Zähler-Instrumentierung plus die
Idempotenz-/NULL-Invarianten sind der Ersatz. Prozesse, die wegen eines
absichtlichen Laufzeitfehlers (z. B. invalidierter View oder
`format_utc`-Bereichsfehler) mit Exit 70 enden, durchlaufen keinen
regulären Cleanup und werden deshalb nicht mit dem Leak-Kriterium
bewertet — der Runtime-Fehlerpfad ist separat getestet.

## 5. Grenzen des Modells

- **`slice` kopiert, `slice_view` referenziert**: Beide bleiben bestehen;
  `slice` hat Kopiesemantik, `slice_view` liefert einen Header-zählenden
  View mit den oben beschriebenen Invalidierungsregeln.
- **Invalidierung ist konservativ**: `insert`/`remove`/`pop` invalidieren
  *alle* Views des Headers (Generation), auch wenn ein konkreter Bereich
  unverändert bliebe. Das ist bewusst einfach und sicher; feineres
  View-Tracking ist einer späteren Version vorbehalten.
- **`var self`-Aliasing (geprüft in 0.8)**: Der Empfänger ist die
  Speicherstelle der aufrufenden Variablen. Andere Funktionen können
  darauf nicht zugreifen: keine Closures/Referenztypen, und Structs sind
  als Globals verboten (E1014, Guard-Test in `src/check.rs`). Der
  Empfänger kann während des Aufrufs daher weder freigegeben noch
  verändert werden — das 0.7-Restrisiko ist mit dem heutigen Typsystem
  nicht konstruierbar. Bei künftigen Referenztypen/Global-Structs muss
  die Analyse wiederholt werden.
- **Rekursive Typen**: weiterhin verboten; damit sind Zyklen im
  Objektgraph unmöglich und RC braucht keine Zyklus-Erkennung.
- **`panic` gibt nichts frei**: `panic` beendet den Prozess direkt
  (Exit 70); laufende Cleanup-Pfade werden nicht durchlaufen. Das ist
  dokumentiert und beabsichtigt.
- **Nicht threadsicher** (siehe oben).
- **Statische Literale** belegen Programmspeicher (kein Heap).
