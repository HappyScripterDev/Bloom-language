# Wie der Compiler funktioniert

Bloom ist ein Rust-Programm ohne externe Crates. Es nimmt eine `.blm`-Datei,
macht daraus C-Code und gibt den einem normalen C-Compiler, der die `.exe`
baut. Warum dieser Umweg? Weil auf jedem Rechner irgendein C-Compiler
rumliegt und man sich dann nicht für jede Plattform einen eigenen Backend
bauen muss. Der Preis ist, dass die Fehlermeldungen des C-Compilers
manchmal durchscheinen, aber damit kann ich leben.

```text
 .blm-Quelltext
      │
      ▼
 ┌─────────┐   ┌────────┐   ┌──────────┐   ┌────────────┐   ┌──────────┐
 │  Lexer  │ → │ Parser │ → │ Typprüfung│ → │ Codegen C  │ → │ C-Toolchain │
 └─────────┘   └────────┘   └──────────┘   └────────────┘   └──────────┘
      │              │             │
      └──────────────┴─────────────┴──→ Diagnosesystem (Datei, Zeile, Spalte, Code)
```

## Module

| Datei | Verantwortung |
| --- | --- |
| `src/main.rs` | Einstiegspunkt, Moduldeklarationen |
| `src/cli.rs` | Kommandozeile: `build`, `run`, `check`, `init`, `fmt`, Hilfe, Version |
| `src/token.rs` | Tokenarten, Positionen (`Pos`, `Span`) |
| `src/lexer.rs` | Tokenisierung inkl. Literale, Kommentare, Positionsverfolgung |
| `src/ast.rs` | Syntaxbaum-Datentypen (inkl. Enums, Patterns, `impl`) |
| `src/parser.rs` | Recursive-Descent-Parser mit Fehlererholung |
| `src/types.rs` | Typmodell (`int`, `float`, `bool`, `string`, `char`, `void`, `[T]`, Structs, Enums, Typparameter) |
| `src/modules.rs` | Modul-Loader: Dateiauflösung, Modulgraph, Deduplizierung, Import-Kanten |
| `src/check.rs` | Namensauflösung (modulübergreifend), Typprüfung, Methoden, Enums, Patterns, Konstantenauswertung |
| `src/matchcheck.rs` | Exhaustiveness-/Unerreichbarkeitsprüfung (Matrix-Verfahren) inkl. Gegenbeispielen |
| `src/diagnostics.rs` | Diagnoseformat, Fehlercodes, Quelltext-Ausschnitte, Mehrdatei-Rendering |
| `src/codegen.rs` | C-Codegenerator (alle Module, Methoden, Enums, Match) |
| `src/toolchain.rs` | Erkennung und Aufruf von MSVC/Clang/GCC/tcc |
| `src/compiler.rs` | Pipeline-Steuerung, temporäre Verzeichnisse, Fehlerabbildung |
| `src/project.rs` | `bloom.toml`-Lesen und `bloom init` |
| `src/fmt.rs` | Tokenbasierter Quellcode-Formatter (`bloom fmt`) inkl. Kommentarerhalt |
| `runtime/bloom_rt.c` | C-Kernlaufzeit (wird per `include_str!` eingebettet) |
| `runtime/bloom_fs.c` | `std::fs`-Implementierung (nach den Typdefinitionen emittiert) |
| `runtime/bloom_process.c` | `std::process`-Implementierung (CreateProcessW/fork) |
| `runtime/bloom_time.c` | `std::time`-Implementierung (Uhren, Sleep, UTC-Formatierung) |
| `runtime/bloom_win32.c` | `std::win32`-Wrapper (nur Windows; UTF-16-Konvertierung) |
| `runtime/bloom_net.c` | `std::net`-Implementierung (Winsock/POSIX, Handle-Registry) |
| `runtime/bloom_tls.c` | `std::tls`-Implementierung (Windows Schannel, Zertifikatsvalidierung) |
| `runtime/bloom_http.c` | `std::http`-Implementierung (Client/Server/Parser, HTTPS, Chunked, Limits) |
| `runtime/bloom_path.c` | `std::path`-Implementierung (lexikalische Pfadoperationen, Host-Semantik) |
| `runtime/bloom_web.c` | `std::web`-Implementierung (Router, Matching, Prozentdekodierung, JSON-Hilfen) |
| `runtime/bloom_html.c` | `std::html`-Implementierung (HTML-Escaping, iterativer Template-Parser und -Renderer) |
| `runtime/bloom_json.c` | `std::json`-Implementierung (RFC-8259-Parser, refcountetes Wertmodell, Serializer) |
| `tests/e2e.rs` | Integrationstests über die echte CLI |

## Virtuelle Standardmodule

`std::math`, `std::fs`, `std::process`, `std::time`, `std::win32`,
`std::net`, `std::tls`, `std::http`, `std::path` und `std::json` haben **keine
Quelldatei**. Der Modul-Loader erkennt die Pfade (`builtin_module`) und legt
ein leeres Modul mit `BuiltinModule`-Marker an. Der Checker registriert
dafür:

- `FnSig`-Einträge mit kanonischen Namen (`std::fs::read_text`), C-Namen
  (`bloom_fs_read_text`) und konkreten Signaturen (u. a.
  `Result<T, string>`),
- den Struct `std::process::ProcessOutput` (Felder `exit_code`, `stdout`,
  `stderr`) und den Struct `std::json::JsonValue` (Feld `id` vom
  internen Typ `Type::Json`) und die Structs `std::web::Router` (Feld `id`
vom internen Typ `Type::Web`) sowie `std::web::Match` (Felder `status`,
`name`, `allow` und `params: std::json::JsonValue`), `std::fs::Bytes`
(Feld `id` vom internen Typ `Type::Bytes`), `std::html::Template`
(Feld `id` vom internen Typ `Type::Html`) und `std::web::FormData`
(Feld `id` vom internen Typ `Type::Form`).

Codegen überspringt Prototypen für `std::*`-Funktionen, emittiert die
C-Implementierung nur für tatsächlich geladene Module (nach den
Typdefinitionen, damit die monomorphisierten `Result`-/Struct-Typen
verfügbar sind) und ruft die Helfer direkt auf — Argumente werden dabei
**geliehen** (die Helfer übernehmen keinen Besitz), managed Ergebnisse
werden zu besessenen Temporären. Die `Result`-Typen der Stdlib werden über
die Instanziierungs-Sammlung garantiert emittiert; `std::http` registriert
zusätzlich die `std::net`- und `std::tls`-Signaturen (und `std::tls` die
`std::net`-Signaturen), weil sein Runtime-Code diese Helfer und deren
`Result`-Typen nutzt. Die Emission erfolgt in Abhängigkeitsreihenfolge
net → tls → http; `std::tls` verlinkt automatisch `ws2_32`, `secur32`
und `crypt32`, `std::net`/`std::http` weiterhin `ws2_32`.

## FFI (`extern` + `unsafe`)

`extern`-Blöcke werden in einer eigenen Phase (`collect_extern_functions`)
registriert: ABI-Validierung (nur `"C"`/`"system"`, E1071), FFI-Typ-Prüfung
(nur `Type::Ffi`-Skalare/`rawptr`, E1072), Duplikat- und Symbolprüfung. Die
`FnSig` trägt `is_extern`/`system_abi`; der `c_name` ist der rohe
Symbolname (kein `bf_`-Mangling). Codegen emittiert `extern`-Prototypen
(mit `BLOOM_SYSTEM_ABI` = `__stdcall` unter 32-bit Windows) und keine
Bodies. `unsafe`-Blöcke erhöhen `unsafe_depth` im Checker; externe Aufrufe
außerhalb sind E1070. Die Link-Argumente laufen über `toolchain::LinkItem`
(validierte Namen oder bestehende Dateien) — MSVC über die Build-Batchdatei
(Argumente werden auf sichere Zeichen geprüft und bei Bedarf gequotet),
GNU-artige Compiler als getrennte `Command`-Argumente.

## Netzwerk-Handles (`std::net`)

`bloom_net.c` führt eine Tabelle fester Größe (65536 Slots). Ein Bloom-
Handle packt Slot-Index (20 Bit) und Generation (Rest); `tcp_close` erhöht
die Generation und gibt den Slot frei, sodass alte Handles nie wieder
gültig werden. Listener-, Verbindungs- und TLS-Slots sind getrennt
typisiert (`tcp_*`-Funktionen lehnen TLS-Handles ab und umgekehrt).
`BLOOM_MEM_REPORT=1` zählt offene Sockets (`live sockets`), damit
Leck-Tests auch Verbindungen abdecken. Winsock wird einmalig initialisiert;
stdout wird bei nicht-Konsole pro Zeile geflusht, damit Serverprogramme
ihren Port vor dem Blockieren ausgeben.

## TLS (`std::tls`)

`bloom_tls.c` nutzt Windows Schannel (keine eigene Kryptografie) mit
`SCH_CREDENTIALS`, `SCH_CRED_MANUAL_CRED_VALIDATION` und
`SCH_USE_STRONG_CRYPTO`; TLS 1.2 ist das Minimum, TLS 1.3 wird bei
vorhandener SDK-/OS-Unterstützung aktiviert. Die Zertifikatsvalidierung
erfolgt explizit: System-Trust über `CertGetCertificateChain` +
`CertVerifyCertificateChainPolicy(CERT_CHAIN_POLICY_SSL, hostname)`,
eigene CA über Kette zum Anker (`CertCompareCertificate`), Zeitgültigkeit
(`CertVerifyTimeValidity`), ServerAuth-EKU und Hostname (SAN-Decoding via
`CryptDecodeObjectEx`, `*.`-Wildcards, iPAddress-SANs; CN nur ohne SAN).
Der Handshake ist zeitlich begrenzt (nicht-blockierender Connect +
`SO_RCVTIMEO`); TLS-1.3-Post-Handshake-Nachrichten werden über
`InitializeSecurityContext` verarbeitet, ein Verbindungsende ohne
`close_notify` ist ein Fehler. Ohne Windows liefern alle Einstiegspunkte
einen klaren Fehler.

## HTTP (`std::http`)

`bloom_http.c` nutzt eine kleine Transport-Abstraktion (`tcp` oder `tls`)
über den Net-/TLS-Helfern (keine zweite Socket-Implementierung). Der
Parser arbeitet auf einem wachsenden Puffer mit erzwungenen Limits (Zeile
8192, Header 16384, Body 4194304 Bytes), dekodiert
`Transfer-Encoding: chunked` (Antworten und Anfragen, Trailer validiert
und verworfen) und lehnt mehrdeutiges Framing (Transfer-Encoding neben
Content-Length, doppelte/ungültige `Content-Length`), fehlende
Host-Header (HTTP/1.1) und unbekannte Transfer-Encodings explizit ab.
`HEAD`, `204` und `304` haben nie einen Body; 1xx-Interim-Antworten
werden verworfen (`101` abgelehnt). `https://` verbindet über `std::tls`
mit voller Zertifikatsvalidierung; `*_with_ca`-Varianten erlauben eine
eigene CA. Client und Server teilen sich die Header-Parsing-Helfer;
`header_get` vergleicht case-insensitiv.

## JSON-Handles (`std::json`) und die Referenzzählung

JSON-Werte sind Bäume, die sich mit den Mitteln der Sprache nicht direkt
als Wertetyp ausdrücken lassen (rekursive Enums existieren nicht).
`std::json` verwendet deshalb dasselbe Muster wie andere Ressourcen: ein
**Handle** auf einen Laufzeitbaum. Anders als Sockets ist das Handle aber
**verwaltet**: `Type::Json` ist ein interner, managed Grundtyp
(`bloom_json_handle` = `int64_t`), dessen Clone `bloom_json_retain` und
dessen Drop `bloom_json_release` aufruft. Der öffentliche Struct
`std::json::JsonValue { id: Type::Json }` erbt diese Verwaltung, und die
generierten Clone-/Drop-Helfer der `Result`-/`Option`-Instanzen reichen
sie weiter — dadurch gibt es kein manuelles `free`, keine Leaks auf
Fehlerpfaden und keine Zyklen (der Parser erzeugt keine, `set`/`push`
lehnen sie ab).

Technische Details:

- `Type::Json` liefert `c_type` = `bloom_json_handle`; der Typ taucht nur
  innerhalb des Builtin-Structs auf und ist aus Quellcode nicht
  konstruierbar (Feldzugriff `v.id` ist als intern dokumentiert).
- Die Composite-Helfer werden vor der Stdlib-Runtime emittiert; deshalb
  deklariert der Codegenerator `bloom_json_retain`/`bloom_json_release`
  vorab, sobald `std::json` geladen ist.
- `bloom_json_*`-Aufrufe laufen über den Borrowed-Args-Pfad
  (`emit_stdlib_call`): Argumente werden geliehen, verwaltete Ergebnisse
  als besessene Temporäre behandelt.
- Jeder JSON-Knoten zählt in `bloom_live_allocs`; Leck-Tests decken damit
  auch Bäume ab.
- **Referenzzählung geteilter Teilbäume:** Das Freigeben eines Baums
  dekrementiert die Zähler der Kinder und gibt nur Knoten frei, deren
  Zähler 0 erreicht (iterativ, refcount-treu). Ein in mehreren Objekten
  gespeicherter oder von Bloom-Variablen gehaltener Teilbaum bleibt am
  Leben. Ein früherer 0.12.0-Fehler (bedingungsloses Freigeben aller
  Kinder) wurde in 0.13.0 behoben und ist durch
  `json_shared_subtree_release_natively` abgesichert.

## Web-Router (`std::web`)

`bloom_web.c` ist eine synchrone Routing-Schicht über den vorhandenen
String-/HTTP-Helfern (keine zweite HTTP-Runtime): ein referenzgezählter
Router (`Type::Web`, gleiche Bauart wie `Type::Json`) hält registrierte
Routen (Methode, normalisiertes Muster, Anwendungsname, segmentweise
zerlegt). `resolve` teilt das Ziel zuerst an `?`, segmentiert den Pfad an
`/` und dekodiert erst danach pro Segment (kodierte Trennzeichen erzeugen
keine neuen Routen); Query-Parameter werden formartig dekodiert (erster
Wert gewinnt). Die spezifischste passende Route gewinnt (fest vor
`:param`); Registrierung lehnt doppelte Muster, Namen und gleiche Shapes
ab, daher ist der Gewinner eindeutig. `Match.params` ist ein
`std::json`-Objekt; `json_response(_with)` serialisiert über
`bloom_json_stringify` und schreibt über `bloom_http_write_response_full`
(Header-Validierung, 204/304 ohne Body). Die Emission erfolgt nach der
JSON-Runtime, die Signaturregistrierung lädt zusätzlich
`std::json`, `std::http`, `std::net` und `std::tls`.

## Templates (`std::html`)

`bloom_html.c` enthält den Escaper (`escape`, Ausgabe höchstens
6 × Eingabe + 1) und eine kleine Template-Engine. Der Parser ist
**iterativ**: er zerlegt die Quelle in eine flache Instruktionsliste
(Text, `{{pfad}}`, `{{#if}}`, `{{else}}`, `{{#each}}`, Endmarker) und
patcht Sprungziele über einen Begrenzten Block-Stack (max. 32 Ebenen),
also kein C-Stack-Wachstum durch Verschachtelung. Das geparste Template
ist ein referenzgezählter Handle (`Type::Html`, Bauart wie
`Type::Json`/`Type::Web`); `render` läuft in einer Schleife über die
Instruktionen mit einem Schleifen-Stack (max. 32) und zählt Iterationen
global (65536). Werte werden direkt aus den JSON-Knoten gelesen (keine
Kopien): Strings escaped in den Ausgabepuffer, Zahlen über Literal bzw.
`bloom_json_format_double`, Booleans als Text; `null`, Arrays und
Objekte sind Fehler. Parserfehler tragen Byte-/Zeilen-/Spaltenposition,
Renderfehler den Pfad. `<script>`/`<style>` werden beim Parsen
abgelehnt. `render_page(_with)` rendert vollständig vor dem Schreiben
und nutzt `bloom_http_write_response_full` mit
`text/html; charset=utf-8`. Emission nach `std::json`/`std::http`
(html zuletzt); die Signaturregistrierung lädt `json`, `http`, `net`,
`tls` und `fs`. `template_file`/`template_file_from` lesen über die
`std::fs`-Helfer (Größenlimit vor dem Lesen, UTF-8-Prüfung) und nutzen
denselben Parser; `template_file_from` öffnet mit Wurzelordner-Schutz
(kanonischer Pfadvergleich nach dem Öffnen).

## Dateien (`std::fs::Bytes`, statische Inhalte)

`bloom_fs.c` enthält neben den Text-/Verzeichnisfunktionen einen
refcounteten Byte-Puffer (`bloom_fs_bytes`, `Type::Bytes`, Bauart wie
`Type::Json`): `read_bytes` liest exakt die Dateigröße (max. 8 MiB,
NUL-Bytes bleiben erhalten), `bytes_len` liest nur das Feld.
`bloom_fs_open_confined` öffnet eine Datei und prüft **auf dem
geöffneten Handle** (Windows `GetFinalPathNameByHandleW` plus
`CompareStringOrdinal` gegen den Wurzelordner-Handle; POSIX
`realpath` plus Geräte-/Inode-Vergleich), ob der aufgelöste Pfad im
Wurzelordner liegt — Symlinks/Junctions werden dadurch mitgeprüft.
`bloom_web_static_file` verbindet das mit `bloom_http_write_response_bytes`
(exakte Byte-Länge, `_head`-Variante für `HEAD`); die MIME-Tabelle ist
fest, unbekannte Endungen liefern `application/octet-stream`. Emission
von `std::fs` erfolgt vor `std::http`/`std::web`/`std::html`, die diese
Helfer und den `Bytes`-Struct in ihren Signaturen verwenden.

## Formulare (`std::web::FormData`)

Der Formular-Code lebt in `bloom_web.c`, weil dort schon Request-Daten
und Prozentdekodierung liegen. Ein `bloom_web_form` ist ein
refcountetes Handle (`Type::Form`, Bauart wie `Type::Web`) auf ein
Array von `bloom_form_entry { name: bloom_str; value: bloom_str }`.
Der Parser arbeitet auf dem Body-Puffer: Eintragsgrenzen werden
zuerst gesucht, dann Name und Wert mit **derselben**
`bloom_web_decode`-Funktion wie die Query-Parameter dekodiert
(`+` nur hier als Leerzeichen, `%` genau einmal, UTF-8-Validierung,
Ausgabepuffer höchstens Eingabelänge plus eins). Die Einträge behalten
ihre Reihenfolge, Duplikate werden nicht zusammengefasst. Limits
werden beim Zählen geprüft (1 MiB Body, 4096 Einträge, 256 Byte Name,
64 KiB Wert), bevor eine weitere Kopie angelegt wird. `form_body_request`
liest den Content-Type über `bloom_http_header_get`, ignoriert
Parameter außer `charset` und lehnt fremde Medientypen sowie
Nicht-UTF-8-Charsets ab. Der Encoder schreibt in einen vorab
berechneten Puffer (reservierte Bytes als `%XX`, Leerzeichen als `+`)
und verweigert Ergebnisse über 1 MiB, damit die Ausgabe wieder
parsebar bleibt. Alle Funktionen nehmen das Handle geliehen; `Ok`-Werte
geben einen neuen Besitzwert zurück, Fehlerpfade geben alle Teilwerte
frei. Testseitig prüfen `form_body_parse_natively`,
`form_encode_roundtrip_natively`, `web_form_server_natively` und
`web_form_errors_natively` Parsen, Limits, Round-trip und den echten
HTTP-Weg inklusive Leak-Zählern.

## Cookies, Sessions und CSRF (`std::web`)

Alle drei liegen in `bloom_web.c`, weil sie dieselbe Request-/
Header-Infrastruktur benutzen. Drei verwaltete Handle-Typen:
`Type::Cookies` (`bloom_web_cookies`, Paarliste), `Type::SessionStore`
(`bloom_web_session_store`, Array aus Session-Zeigern plus Kapazität
und TTL) und `Type::Session` (`bloom_web_session`, ID, CSRF-Token,
JSON-Objekt, Ablaufzeit). Der Store hält eine Referenz pro Session,
jeder Bloom-Handle eine weitere; `session_destroy` gibt nur die
Store-Referenz frei, bestehende Handles bleiben gültig, bis ihr Scope
endet. `session_load` retained die gefundene Session und gibt sie als
neuen Besitzwert zurück.

Sessions bekommen 32 Zufallsbytes aus `bloom_web_random_bytes`:
Windows ruft `BCryptGenRandom` (Link gegen `bcrypt.lib` über
`auto_links` in `compiler.rs`, nur Windows), POSIX liest
`/dev/urandom`. Es gibt bewusst keinen Userspace-Fallback. Für die
Fehlerpfad-Tests kennt die Runtime den internen Hook
`BLOOM_WEB_TEST_FAIL_RANDOM`; er ist kein Teil der öffentlichen API.
IDs und CSRF-Tokens werden mit einer kleinen Base64url-Implementierung
ohne Padding auf 43 Zeichen gebracht. Der Ablauf nutzt
`bloom_time_monotonic_millis` aus `std::time`; deshalb registriert
`std::web` zusätzlich die `std::time`-Signaturen (sonst fehlt die
Uhr im generierten C). `csrf_check` vergleicht gleich lange Tokens
byteweise mit XOR-Akkumulation.

`Set-Cookie`-Zeilen werden als normalisierte Strings erzeugt
(`Set-Cookie: name=value; …`), nie durch freies Zusammensetzen in
Bloom-Code. `std::http` parst den `headers`-Block und schreibt jede
Zeile einzeln — mehrere `Set-Cookie`-Header bleiben dadurch getrennte
Header. Tests: `web_cookie_api_natively`, `web_session_api_natively`,
`web_session_random_failure_natively`, `web_cookies_e2e`,
`web_session_basic_e2e`, `web_session_expiry_e2e`,
`web_session_errors_e2e`, `web_csrf_form_e2e`.

## Datenbank (`std::db`, SQLite)

SQLite wird als **Amalgamation** mitgeliefert
(`runtime/sqlite3.c`/`.h`, Version 3.54.1, Public Domain,
`runtime/sqlite3_NOTICE.md`). Der Compiler bettet beide Dateien per
`include_str!` ein; bei einem Programm mit `std::db` schreibt er sie in
das temporäre Build-Verzeichnis neben `program.c` und übergibt
`sqlite3.c` als **zweite Übersetzungseinheit** an den C-Compiler
(`Toolchain::compile(..., extra_sources)`). Nur dann werden
`SQLITE_THREADSAFE=0`, `SQLITE_OMIT_LOAD_EXTENSION` und `SQLITE_DQS=0`
definiert. Es gibt keine zusätzliche Link-Bibliothek: die Amalgamation
bringt ihre Windows-Abhängigkeiten selbst mit, Bloom linkt weiterhin
nur die bisherigen Bibliotheken. `--emit-c` kopiert `sqlite3.c`/`.h`
ebenfalls neben die ausgegebene C-Datei, damit sie sich manuell
übersetzen lässt.

`bloom_db.c` (emittiert direkt nach `bloom_fs.c`, weil BLOBs die
`std::fs::Bytes`-Helfer nutzen) hält zwei verwaltete Handles:
`bloom_db` (Referenz auf `sqlite3*`) und `bloom_db_stmt` (Statement,
Zustand, Besitzreferenz auf `bloom_db`). Ein Statement erhöht den
Zähler der Datenbank beim Präparieren und gibt ihn beim Finalisieren
frei; dadurch lebt die Verbindung, solange irgendein Bloom-Handle sie
braucht. Die Datenbank schließt erst bei Zähler 0
(`sqlite3_close_v2`, rollt eine offene Transaktion zurück).

Der Binder akzeptiert nur den Zustand `fresh` und Indizes
1..`bind_parameter_count`; Spaltenzugriffe nur im Zustand `row` und
0-basiert. `prepare` verlangt genau eine Anweisung (Rest auf reine
Leerzeichen/Kommentare geprüft) und lehnt eingebettete NUL-Bytes ab.
Fehler werden als `<op>: <sqlite3_errmsg> (SQLite code N)` formatiert;
der SQL-Text wird nicht wiederholt. Emission der Runtime nur bei
geladenem `std::db`; die Proto-typdeklarationen für Retain/Release
stehen im Composite-Helper-Block. `bloom_db_`/`bloom_stmt_` sind in
der Borrowed-Args-Liste von `emit_stdlib_call` eingetragen (sonst
würde der Codegen Argumente klonen, die die Runtime nur leiht).
Getestet durch `db_api_natively`, `db_file_natively`,
`db_web_app_e2e` und `db_examples_natively`.

## Formatter (`src/fmt.rs`)

`bloom fmt` nutzt `lex_with_comments` (der Compiler-Pfad `lex` verwirft
Kommentare weiterhin). Der Formatter gibt jedes Token aus seinem exakten
Quelltext-Slice aus und fügt kanonische Zwischenräume ein; Voranalysen
markieren Inline-Braces (Daten-Braces ohne `;`/`=>`, Layout der Quelle
bleibt erhalten) und Generic-Regionen (`<`/`>` per Quell-Adjazenz). Vor
dem Schreiben wird der Output erneut mit dem echten Parser geparst; bei
Fehlern wird nichts geschrieben. Geschrieben wird über eine temporäre
Datei im Zielverzeichnis mit Umbenennungs-Fallback.

## Pipeline im Detail

### 0. Modul-Loader (`modules.rs`)

Der Loader beginnt bei der Einstiegsdatei und lädt rekursiv alle über `use`
erreichten Module:

1. **Dateiauflösung:** `use a::b;` sucht zuerst relativ zum Verzeichnis der
   importierenden Datei (`a/b.blm`, `a/b/mod.blm`), dann relativ zum
   Einstiegsverzeichnis (für Untermodule).
2. **Modulgraph:** Module werden über ihren kanonischen Pfad (relativ zum
   Einstiegsverzeichnis, z. B. `net::client`) identifiziert und über den
   kanonisierten Dateipfad dedupliziert. Zyklische Importe sind erlaubt: ein
   bereits geladenes Modul wird wiederverwendet.
3. **Import-Kanten:** Jeder `use` wird zu einer Kante `Modul` oder `Symbol`
   (mit kanonischem Zielnamen); die Sichtbarkeitsprüfung erfolgt später im
   Checker.
4. **ID-Eindeutigkeit:** Expr- und Decl-IDs werden über `parse_with_offset`
   modulübergreifend fortlaufend vergeben, damit die globalen Tabellen des
   Checkers kollisionsfrei bleiben.

### 1. Lexer (`lexer.rs`)

Der Lexer läuft zeichenweise (Unicode-skalarwertbasiert) über die Quelle und
erzeugt `Vec<Token>` mit `Span` (Byte-Offset, Zeile, Spalte). Er behandelt:

- Zeilen- und Blockkommentare (nicht verschachtelt)
- Ganzzahl-Literale in vier Basen mit Unterstrichen, Überlaufprüfung
- Gleitkomma-Literale mit Endlichkeitsprüfung
- Strings/Chars mit Escape-Sequenzen inkl. `\u{...}`
- Fehlererholung: unbekannte Zeichen erzeugen eine Diagnose und werden
  übersprungen, damit mehrere Fehler gemeldet werden können

Ist die Tokenliste fehlerfrei, folgt der Parser; andernfalls bricht die
Pipeline nach der Diagnoseausgabe ab.

### 2. Parser (`parser.rs`)

Recursive-Descent mit Präzedenzkaskade (`assignment` → … → `primary`) gemäß der
Grammatik in `docs/LANGUAGE_SPEC.md`. Jeder Ausdruck erhält eine eindeutige
`ExprId`, jede Deklaration eine `DeclId`; beides wird später für die
Typzuordnung und den Codegenerator genutzt.

Fehlererholung: bei Syntaxfehlern wird zur nächsten Anweisungsgrenze
(`;`, `}` oder Statement-Schlüsselwort) synchronisiert, sodass mehrere Fehler
gemeldet werden. Nicht unterstützte Konstrukte (`use`, `class`, …) erzeugen
gezielte E1015-Diagnosen. Bei Parse-Fehlern wird die Typprüfung übersprungen.

### 3. Typprüfung (`check.rs`)

Mehrere Phasen mit Bereichsstapel, jeweils über alle Module des Modulgraphen:

1. **Enum-/Struct-Registrierung:** Namen werden kanonisch registriert
   (`modul::Name`, Einstiegsmodul ohne Präfix), Duplikate, Reserved- und
   `__`-Namen abgelehnt. Danach werden Feld-/Nutzdatentypen aufgelöst
   (Vorwärtsreferenzen möglich, Typparameter erlaubt); eine DFS-Analyse
   erkennt zyklische Wertlayouts (E1024/E1047).
2. **Funktions- und Methodensammelphase:** Signaturen aller Funktionen und
   `impl`-Methoden (Empfänger, Parameter, Rückgabetyp, Sichtbarkeit,
   Typparameter). Impl-Ziele werden über lokale/importierte Namen aufgelöst;
   Methoden-/Feld-Kollisionen sind E1030, assoziierte Funktionen
   (Empfänger `None`) sind erlaubt. `impl`-Blöcke auf `Option`/`Result` sind
   in jedem Modul zulässig.
3. **Import-Validierung:** Jede Import-Kante wird gegen die Symboltabellen
   geprüft (Existenz E1055, Sichtbarkeit E1051, Duplikate E1052).
4. **Globale Deklarationen und Funktionskörper:** Namen werden über
   Bereichsstapel und Modul-Importe aufgelöst; jede Variable erhält einen
   eindeutigen C-Namen (`v_<name>_<n>`). Struct-Literale, Varianten-
   konstruktionen, Methodenaufrufe, Feldzuweisungen, `match`-Ausdrücke
   (inkl. Literal-Patterns und Guards) und `?`-Operatoren werden vollständig
   typgeprüft.
5. **Pattern-Prüfung:** Bindungen, Varianten-/Feld-/Literal-Patterns, Typen,
   Guards (`bool`); danach Exhaustiveness und Unerreichbarkeit über
   `matchcheck.rs` (Guards zählen nicht zur Abdeckung).
6. **Rückgabeanalyse** je Funktion/Methode; **`main`-Prüfung** im
   Einstiegsmodul; **Instanziierung** (monomorph, topologisch, mit
   Rekursions-/Tiefenprüfung) für generische Structs, Enums, Funktionen und
   Methoden.

Generische Funktionen/Methoden werden über ein Worklist-Verfahren
instanziiert: Aufrufe tragen ihre konkreten Typargumente (`type_args`);
deren Körper werden nach weiteren Aufrufen durchsucht, bis die Menge stabil
ist (transitive Hülle). Instanziierungen mit noch offenen Typparametern
werden übersprungen. Inferenz bindet Typparameter aus Argumenttypen,
erwartetem Rückgabetyp und Turbofish (`identity::<int>(x)`).

Ergebnis ist ein `CheckedProgram` mit Typtabelle (`ExprId → Type`),
Variablentabelle, Namens-/Aufruf-/Varianten-/Konstantenauflösung, Funktions-
und Methodensignaturen (inkl. `body` und `type_params`), Struct- und
Enum-Tabellen, Pattern-Bindungstabellen sowie den geordneten
Instanziierungen (`instantiations`, `fn_instantiations`,
`method_instantiations`).

Zur Fehlerreduktion führt der Checker eine Menge „fehlerhafter Ausdrücke“
(`error_exprs`): Folgefehler wie `unbekannt.x` melden nur die Ursache
(E1003) und nicht zusätzlich „int hat keine Felder“.

Beim Typechecking wird `int` implizit zu `float` erweitert (Coercion), sonst
muss der Typ exakt passen. Konstantenauswertung läuft auf denselben
Semantikregeln wie die Laufzeit (u. a. wickelnde Ganzzahlarithmetik).

### 3a. Exhaustiveness (`matchcheck.rs`)

Patterns werden zu Konstruktor-/Wildcard-Patterns normalisiert; darauf laufen
zwei Algorithmen aus der Literatur (Maranget):

- **Exhaustiveness:** Für jeden Enum-Typ wird die Pattern-Matrix pro Variante
  spezialisiert und rekursiv geprüft. Fehlende Fälle liefern ein konkretes
  Gegenbeispiel-Pattern (z. B. `Option::Some(Option::None)`), das in der
  Diagnose (E1041) ausgegeben wird. `bool` wird als endlicher Typ behandelt
  (`true`/`false`), Literal-Patterns werden mit ihren Werten verglichen.
- **Unerreichbarkeit:** Ein Arm ist „useful“, wenn er mindestens einen Wert
  matcht, den frühere Arme nicht matchen; sonst E1042. Arme mit Guard werden
  von beiden Analysen ausgenommen (sie decken nichts garantiert ab).

### 4. Codegenerator (`codegen.rs`)

Der Generator erzeugt eine einzelne, eigenständige C-Datei aus allen Modulen:

1. die eingebettete Laufzeit (`runtime/bloom_rt.c`),
2. Typ-Typedefs (Structs und Enum-Instanziierungen) in gemeinsamer
   topologischer Reihenfolge,
3. C-Prototypen für alle Funktionen und Methoden (Vorwärtsaufrufe),
4. globale Variablen (file-scope),
5. alle Funktions- und Methodendefinitionen (je konkreter Instanziierung
   eine Spezialisierung),
6. das `main`-Gerüst, das die Laufzeit initialisiert, globale Konstanten
   setzt und die `main`-Funktion des Einstiegsmoduls aufruft.

Generische Funktionen/Methoden werden monomorphisiert: Der C-Name wird aus
Basisname und Typparametern gemangelt (`bf_identity_i64`). Beim Emittieren
einer Spezialisierung hält der Generator eine Substitutionsmap
(`current_subst: Typparameter → konkreter Typ`); `expr_type`/`var_type`
wenden sie auf alle Typen an, sodass auch Varianten-Konstruktionen in
generischen Körpern die richtigen monomorphen C-Typen erhalten.

**Ownership-Codegenerierung (Referenzzählung).** Der Generator führt eine
Besitzdisziplin pro Variablen-Slot und Statement durch (Details:
`docs/MEMORY_MODEL.md`):

- `scopes`: Stapel von Scopes mit verwalteten Slots; Blockende emittiert
  Drops in umgekehrter Reihenfolge.
- `stmt_temps`: besessene Temporäre der aktuellen Anweisung; sie werden am
  Statement-Ende freigegeben, sofern sie nicht per `take_owned` in einen
  Slot transferiert wurden (Transfer ohne Zähleränderung).
- Speichern in Slots klont (`clone_text`), Überschreiben gibt den alten
  Wert frei (`drop_line`).
- `return`, `break`, `continue` und der `?`-Fehlerpfad emittieren Drops für
  die jeweils betroffenen Scopes; die Pfade schließen sich aus, daher keine
  doppelte Freigabe. Die Drop-Funktionen sind zusätzlich idempotent
  (NULL-Prüfung, `tag = -1` bei Enums).
- Parameter sind besessen (Aufrufer klont Argumente), Rückgaben besessen,
  `self`-Empfänger geliehen.
- Literale Strings sind statische, unsterbliche `bloom_str`-Werte
  (`rc == SIZE_MAX`); `clone`/`drop` sind No-Ops.
- Für Structs/Enums mit verwalteten Feldern werden `bloom_clone_*`- und
  `bloom_drop_*`-Helfer in topologischer Reihenfolge emittiert; Enums
  behandeln nur die aktive Variante.

Namensräume sind kollisionsfrei: Bloom-Funktionen `bf_*`, lokale Variablen
`v_*`, Struct-Felder `f_*`, Struct-Typen `bloom_struct_*`, Enums
`bloom_enum_<Name>[_<Typargumente>]`, generierte Temporäre `t_*`, Laufzeit
`bloom_*`. Symbole des Einstiegsmoduls behalten kurze Namen; Symbole anderer
Module werden mit dem kanonischen Modulnamen präfixiert
(`bf_geometry_distance`). Doppelte Unterstriche in Bezeichnern sind
reserviert (E1056), wodurch Kollisionen ausgeschlossen sind.

Wichtige Übersetzungsmuster:

| Bloom | C |
| --- | --- |
| `int`-Arithmetik | `bloom_iadd/isub/imul/ineg` (definiertes Wickeln), `bloom_idiv/imod` (mit Nullprüfung) |
| `float`-Arithmetik | native C-Operatoren |
| gemischte Numerik | `(double)`-Cast des `int`-Operanden |
| String-Konkatenation | `bloom_str_concat` |
| String-Vergleich | `bloom_str_eq` |
| Array-Literal | `bloom_arr_<suffix>_new()` + `_push`-Aufrufe über Prelude-Zeilen |
| `a[i]` lesen | `bloom_arr_<suffix>_get(a, i, datei, zeile)` (Grenzprüfung) |
| `a[i] = v` | `bloom_arr_<suffix>_set(a, i, v, datei, zeile)` |
| `slice_view(a, s, e)` | `bloom_slice_<suffix>_new(a, s, e, datei, zeile)` (Header-Retain + Generation) |
| `view[i]` / `view[i] = v` | `bloom_slice_<suffix>_get/_set(view, i, datei, zeile)` (View-Grenzen + Generation geprüft) |
| `s[i]` | `bloom_str_char_at(s, i, datei, zeile)` (Skalarwert, Grenzprüfung) |
| `println(x)` | typabhängige Auswahl `bloom_println_<i64/f64/bool/str/char>` |
| `input(prompt)` | `bloom_input` |
| `push(a, v)` | `bloom_arr_<suffix>_push(a, v)` |
| `pop(a)` | Prelude: `Option`-Temporär + `len`-Prüfung, sonst letztes Element |
| `insert(a, i, v)` | `bloom_arr_<suffix>_insert(a, i, v, datei, zeile)` |
| `contains`/`starts_with`/`ends_with` | `bloom_str_contains/_starts_with/_ends_with` |
| `trim`/`replace`/`to_upper`/`to_lower` | `bloom_str_trim/_replace/_to_upper/_to_lower` |
| `split(s, sep)` | `bloom_str_split(s, sep, datei, zeile)` |
| `?` auf `Result` | Tag-Prüfung: bei `Err` `return <fehler>`-Temporär, sonst Payload |
| `for i in a..b` | `int64_t t = b; for (int64_t i = a; i < t; i++)` |
| `struct Point { x, y }` | `typedef struct { ... } bloom_struct_Point;` (topologisch sortiert) |
| `struct Box<T>` / `Box<int>` | `bloom_struct_Box_i64` (monomorph, kollisionsfrei) |
| `Point { x: 1, y: 2 }` | Temporär + Feldzuweisungen über Prelude-Zeilen |
| `p.x` / `p.x = v` | `((v_p_0).f_x)` bzw. `((v_p_0).f_x = v)` |
| `fn m(self)` / `fn m(var self)` | `bf_T_m(T self)` / `bf_T_m(T *self)` (Aufruf mit `&lvalue`) |
| `p.m()` | `bf_T_m(...)` mit Empfänger als erstem Argument |
| `T::assoc(...)` | `bf_T_assoc(...)` (kein Empfängerargument) |
| `enum E { A, B(int) }` | `typedef struct { int32_t tag; union { char m_A; struct { int64_t _0; } m_B; } data; } bloom_enum_E;` |
| `E::B(5)` | Temporär, `tag = 1`, `data.m_B._0 = 5` |
| `match` | `if/else`-Kette über `tag`/Vergleiche; Bindungs-Extraktion aus `data`; Ergebnis-Temporär |
| `n if n > 0` (Guard) | Bindungs-Temporär wird in der `if`-Bedingung zugewiesen, Guard folgt in derselben Bedingung |
| `Option<int>` | `bloom_enum_Option_i64` (monomorphisiert, kollisionsfrei) |
| Struct-Kopie/Parameter/Rückgabe | native C-Struct-Kopie (Wertsemantik) |
| Array-Wert | Zeiger auf Laufzeit-Header (Referenzsemantik, Kopien teilen) |

Ausdrücke, die Hilfsanweisungen benötigen (Array-Literale, Struct-/
Varianten-Literale, Index-Zuweisungen, `match`), füllen einen „Prelude“-
Puffer, der vor der jeweiligen Anweisung ausgegeben wird. Bei
`while`-Bedingungen wandert der Prelude in den Schleifenkörper.

String-Literale werden byteweise als C-Stringliterale emitett; Nicht-ASCII
und Steuerzeichen werden als dreistellige Oktal-Escapes geschrieben. Das ist
unabhängig von Quellcodepage und Compiler-Flags korrekt.

### 5. C-Toolchain (`toolchain.rs`)

Erkennungsreihenfolge:

1. `BLOOM_CC` (Pfad oder Name; `cl` → MSVC-Suche)
2. MSVC über `vswhere.exe` (Requirement `Microsoft.VisualStudio.Component.VC.Tools.x86.x64`),
   Aufruf über eine temporäre `.bat`, die `vcvars64.bat` lädt und dann `cl`
   mit `/std:c17 /utf-8 /O2` ausführt
3. `clang` im PATH
4. `gcc` im PATH (`-O2 -lm`)
5. `tcc` im PATH

Kompiliert wird immer in einem temporären Arbeitsverzeichnis mit einfachen
Dateinamen (`program.c` → `program.exe`); das Ergebnis wird anschließend an
den Zielort kopiert. Dadurch sind Pfade mit Leerzeichen/Sonderzeichen
unkritisch, und es werden nie Shell-Strings aus Benutzereingaben gebaut.
MXVC wird über `cmd /C` mit einer zuvor geschriebenen Batchdatei aufgerufen
(keine Shell-Interpolation von Nutzerdaten).

Bei Fehlern wird die Ausgabe des C-Compilers vollständig angezeigt
(`E3003`), damit zwischen Bloom-Fehlern und C-Toolchain-Problemen
unterschieden werden kann.

### 6. Laufzeitbibliothek (`runtime/bloom_rt.c`)

Wird als Text in jede erzeugte C-Datei eingebettet (über
`include_str!("../runtime/bloom_rt.c")`). Bestandteile:

- `bloom_str`: Referenzgezählter UTF-8-String (Header `{rc, data, len}`,
  eine Allokation; statische Literale mit `rc == SIZE_MAX`); Clone-/Drop-
  Funktionen, String-Funktionen (`contains`, `starts_with`, `ends_with`,
  `trim`, `replace`, `to_upper`/`to_lower` (ASCII), `split`, `join`)
- Array-Typen als **Referenzwerte** mit Referenzzählung: `bloom_arr_<suffix>_s`
  ist der **stabile** Header (`rc`, `data`, `len`, `cap`, `gen`),
  `bloom_arr_<suffix>` ein Zeiger darauf. Kopien teilen den Header; `push`,
  `pop`, `insert`, `remove`, `set` und `slice` arbeiten auf dem Header.
  Element-Besitz: `push`/`insert`/`set` klonen, `pop`/`remove`
  transferieren (Makro-Parameter `CLONE_ELEM`/`DROP_ELEM`). Wachstum per
  Verdopplung (nur der Datenpuffer wird reallokiert), Grenzprüfung mit
  Datei/Zeile. `gen` wird von `insert`/`remove`/erfolgreichem `pop` erhöht.
- Slice-Views: `bloom_slice_<suffix>` = `{ hdr, start, len, gen }`.
  `_new` prüft die Grenzen und erhöht `hdr->rc`; `_clone`/`_drop` verwalten
  den Header; `_get`/`_set`/`_len` prüfen View-Grenzen und Generation
  (invalidierte Views → `bloom_panicf`). Zugriffe gehen immer über
  `hdr->data` und bleiben daher nach Reallokation gültig.
- `bloom_live_allocs` (Instrumentierung) + `bloom_mem_report()` für
  `BLOOM_MEM_REPORT=1` (Leak-Nachweis in Tests)
- `bloom_panic_msg` für `panic(msg)` (Exit 70 mit Quelle)
- Mathematik-Helfer (`bloom_iabs`, `bloom_i64_min/max/clamp`,
  `bloom_f64_clamp`, `bloom_math_round`)
- UTF-8-Encode/Decode mit Bereichsvalidierung (Overlongs, Surrogate, > U+10FFFF)
- definierte Ganzzahlarithmetik (`bloom_iadd` usw. über `uint64_t`-Umwege,
  dadurch kein UB durch Signed Overflow)
- Konvertierungen mit Bereichsprüfung (`bloom_f64_to_i64`,
  `bloom_str_to_i64/f64`, `bloom_char_from_i64`)
- Ein-/Ausgabe inklusive Konsolen-UTF-8 auf Windows (`SetConsoleOutputCP`)
- `bloom_panicf` mit Exit-Code 70 und Datei/Zeilenangabe

Structs benötigen keine Laufzeitfunktionen: Sie werden als C-Structs mit
nativer Kopfsemantik dargestellt.

## Fehlerbehandlung im Compiler

- Alle erwartbaren Fehler sind Werte (`Diagnostic`, `BloomError`), keine
  Panics; ein fehlerhaftes Bloom-Programm führt nie zu einem Rust-Stacktrace.
- Diagnosen sammeln sich, werden sortiert nach Entstehung ausgegeben (max. 100)
  und enthalten Datei, Zeile, Spalte, Quelltextausschnitt, Label und Hilfe.
- Der Compiler endet mit definierten Exit-Codes (siehe Sprachspezifikation
  Kapitel 11).

## Tests

- **Unit-Tests** je Modul (Lexer, Parser, Typprüfung, Codegen, Projektdatei,
  Toolchain-Erkennung, Modul-Loader, Exhaustiveness, Slice-Views,
  Formatter, FFI): 318 Tests, laufen ohne C-Compiler.
- **Integrationstests** (`tests/e2e.rs`): rufen die echte `bloom`-Binärdatei auf,
  kompilieren die Beispielprogramme (inklusive Mehrdatei-Projekten mit
  Modulen/Re-Exports und den 0.9-Features Referenzzählung, `remove`/`slice`,
  Slice-Views (Reallokation, Invalidierung, Lebensdauer, Kopien, Container),
  `join`, `std.math`, `std::fs`/`std::process`/`std::time` (inkl.
  Unicode-Pfade, Startfehler, Exit-Codes, Deadlock-Test, Zeitfehler),
  `bloom fmt` (Kommentarerhalt, Idempotenz, Beispiel-Korpus, sichere
  Schreibvorgänge), `Option?`, `panic`, Oder-Patterns, `pub use`), führen
  sie aus und prüfen Ausgabe, Exit-Codes und — über `BLOOM_MEM_REPORT` —
  dass keine Allokationen überleben. Fehlerfälle prüfen Diagnosen und
  Exit-Codes (E1020–E1072). Zusätzlich: FFI (native Testbibliothek,
  Linker-Fehler, Link-Konfiguration), `std::win32`, Loopback-TCP mit
  Timeouts und abgelehnten Verbindungen, HTTP-Client/-Server gegen
  Rust-Peers sowie HTTP-Fehler-/Limit-Pfade. TLS/HTTPS wird gegen einen
  PowerShell-SslStream-Testserver (Schannel) und einen Node/OpenSSL-Server
  geprüft (Handshake, Trust-/Hostname-Fehler, Timeout, close_notify,
  Chunked über TLS); Chunked-Fälle nutzen Rust-TCP-Peers. `std::path`
  wird lexikalisch mit exakten Erwartungswerten geprüft (inkl.
  Fehlerfällen und Idempotenz-Annahmen), `std::json` mit
  Parser-/Accessor-/Serializer-/Roundtrip-/Limit-Tests sowie
  Datei-Integration (9-MiB- und 4,5-MiB-Eingaben als Limitfälle).
  `std::web` wird über exakte Routing-/Parameter-/Fehlererwartungen, den
  vollen REST-Server (10 echte HTTP-Anfragen gegen das Beispiel) und
  einen Shared-Subtree-Regressionstest für die JSON-Referenzzählung
  abgedeckt; `std::html` über Escaping-/Parser-/Render-/Limit-/
  Server-Tests (Content-Type, Zusatz-Header, Escaping gefährlicher
  Eingaben, 405/404) und die Beispiele. Fehlt
  ein C-Compiler, überspringen sich die betroffenen Tests mit Hinweis.

## Bewusste Entwurfsentscheidungen

| Entscheidung | Begründung |
| --- | --- |
| C als erstes Backend | schnelle, breit verfügbare native Codegenerierung; menschenlesbares Zwischenprodukt; später austauschbar, da der AST die Quelle ist |
| Ganzzahlen wickeln definiert | kein UB; Verhalten ist dokumentiert und testbar |
| Kein impliziter `float`→`int`-Weg | verhindert stille Präzisionsverluste |
| Arrays als Header-Zeiger (echte Referenzsemantik) | Kopien teilen Länge **und** Puffer; `push`/`pop`/`insert` sind über alle Kopien sichtbar |
| Structs als C-Structs (Wertsemantik) | native Kopfsemantik von C; kein Laufzeit-Overhead, keine Sonderfälle |
| `var self` als C-Zeiger statt Borrow-Semantik | In-Place-Mutation ohne benutzersichtbare Referenzen; Mutabilitätsregeln bleiben statisch prüfbar |
| Enums als Tagged Union mit Monomorphisierung | keine Laufzeit-Typtags für Generics nötig; alle Typen zur Compile-Zeit aufgelöst |
| Monomorphisierung auch für generische Structs/Funktionen/Methoden | dieselbe Infrastruktur wie bei Enums; keine Boxed-Werte, volle Wertsemantik, keine Laufzeitkosten |
| Exhaustiveness per Matrix-Verfahren | präzise Gegenbeispiele in Diagnosen; erweitert um Literale (`bool` endlich) und Guards (von der Abdeckung ausgenommen) |
| Guard-Bindungen werden in der `if`-Bedingung zugewiesen | Guard-Ausdrücke können Pattern-Bindungen lesen, ohne den `if/else`-Aufbau zu verlassen |
| Module als Dateigraphen (keine Textverkettung) | echte Symboltabellen pro Modul, Sichtbarkeit, Deduplizierung, Zyklen definiert |
| Kanonische Namen mit Modulpräfix in C | kollisionsfrei durch reservierte `__`-Bezeichner; Einstiegsmodul bleibt kurz |
| Rekursive Structs/Enums werden abgelehnt | Werte sind inline; Rekursion braucht Referenzen (geplant mit dem Speichermodell) |
| Kein Freigeben von Heap-Speicher (0.6) | bewusste Einschränkung; Analyse und Migrationspfad in `docs/MEMORY_MODEL.md` |
| Eingebettete Laufzeit statt externer Bibliothek | erzeugte Programme sind eigenständig, kein Bloom-Installationspfad nötig |
| Keine externen Rust-Crates | einfache Builds, keine Supply-Chain-Abhängigkeiten |
