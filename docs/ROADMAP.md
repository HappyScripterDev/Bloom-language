# Roadmap

Hier steht, was in welcher Version reingekommen ist und was noch fehlt.
Die alten Versionen liest wahrscheinlich niemand mehr, aber ich lasse sie
drin, weil man so sieht, wie das Ding gewachsen ist.

**implementiert** heißt: ist drin und hat Tests. **geplant** heißt: steht
auf der Liste, angefasst habe ich es noch nicht.

## Version 0.1.0 – MVP (implementiert)

- Lexer, Parser, AST, Namensauflösung, Typprüfung, Konstantenauswertung
- `let`/`var`/`const`, Basistypen, Operatoren, Kontrollfluss, Funktionen
- C-Codegenerierung + Toolchain-Erkennung (MSVC/Clang/GCC/tcc)
- CLI (`build`, `run`, `check`, `init`, `--help`, `--version`), `bloom.toml`
- Diagnosesystem mit Codes, Positionen und Hilfetexten

## Version 0.2.0 – Structs (implementiert)

- `struct`-Deklarationen, Struct-Literale, Feldzugriff/-zuweisung
- Structs als Parameter/Rückgabewerte, verschachtelte Structs
- Wertsemantik, rekursive Structs abgelehnt (E1024), Arrays von Structs abgelehnt
- Arrays als echte Referenzwerte (Header-Pointer)

## Version 0.3.0 – Methoden und impl-Blöcke (implementiert)

- `impl`-Blöcke, mehrere Methoden, Methodenketten
- Empfänger `self` (Wertkopie, lesend) und `var self` (In-Place-Mutation,
  erfordert veränderliche Wurzel)
- Methoden als Parameter-/Rückgabetypen, Methoden rufen Methoden auf
- Vorwärtsdeklarationen im C-Code (Aufrufe vor Definition)
- Diagnosen E1030–E1033

## Version 0.4.0 – Enums, Pattern Matching, Option/Result (implementiert)

- `enum` mit Unit-, positionellen und benannten Varianten, auch generisch
- `Enum::Variant`-Konstruktion, Enums in Struct-Feldern, als Parameter/Rückgabe
- `match` mit Bindungen, Wildcards, verschachtelten Patterns
- Statische Exhaustiveness (Matrix-Verfahren) mit Gegenbeispielen (E1041),
  Unerreichbarkeitsprüfung (E1042)
- `Option<T>` und `Result<T, E>` als eingebaute generische Enums
- Tagged-Union-C-Repräsentation, monomorphisierte Instanziierungen
- Rekursive Enums abgelehnt (E1047), Arrays von Enums abgelehnt (E1015)

## Version 0.5.0 – Module und Projektstruktur (implementiert)

- `use`-Imports (Module und einzelne Symbole), Untermodule
  (`net::client`, `util/mod.blm`), Import relativ zur Datei und zum
  Einstiegsverzeichnis
- Modulgraph mit Deduplizierung, zyklische Importe definiert erlaubt,
  Einstiegsmodul nicht importierbar
- `pub`-Sichtbarkeit für Funktionen, Structs, Enums, Methoden;
  private Symbole E1051, fehlende Module E1050, unbekannte Symbole E1055,
  doppelte Imports E1052
- Qualifizierte Pfade in Ausdrücken und Typen
- C-Codegenerierung über alle Module in einer Übersetzungseinheit mit
  kollisionsfreiem Namensschema
- Mehrdatei-Projekte über die bestehende CLI (`bloom run`/`build`/`check`)

## Version 0.6.0 – Sprache vertiefen (implementiert)

- **Generische Structs, Funktionen und Methoden** (`Pair<A, B>`,
  `fn identity<T>(...)`, `impl<T> Box<T>`) mit Inferenz aus Argumenten,
  erwartetem Rückgabetyp und Turbofish (`identity::<int>(x)`);
  Monomorphisierung über die bestehende Enum-Infrastruktur, transitive
  Instanziierung generischer Aufrufe
- **Methoden auf Enums** und **assoziierte Funktionen** (`Shape::circle(2.0)`,
  `impl<T> Option<T>`); Namenskonflikt mit Varianten ist E1030;
  `var self` auf Enums abgelehnt (E1033)
- **`?`-Operator** für `Result<T, E>` mit Einmal-Auswertung und
  Fehlertyp-Prüfung (E1060/E1061)
- **Pattern-Ausbau:** Literal-Patterns (`0`, `-3`, `true`, `'a'`, `"text"`)
  und Guards (`n if n > 0 => ...`); `bool` gilt als endlicher Typ,
  Guards zählen nicht für die Exhaustiveness
- **Arrays:** `pop` (→ `Option<T>`), `insert` mit Grenzprüfung
- **String-Bibliothek:** `contains`, `starts_with`, `ends_with`, `trim`,
  `replace`, `to_upper`/`to_lower` (ASCII), `split`
- **Speichermodell:** Analyse mit Bewertung aller Optionen und Empfehlung
  (Referenzzählung) in `docs/MEMORY_MODEL.md`; Umsetzung für 0.7 geplant

Aus 0.6.0 vertagt (weiterhin geplant):

- Array von Arrays/Structs/Enums, `remove`, Slices
- `join`, `std.math`
- Oder-Patterns (`A | B`), Range-Patterns, `?` für `Option`
- `panic`-Funktion in der Sprache
- Re-Exports (`pub use`), Modul-Aliasse, `super`-Pfade

## Version 0.7.0 – Speicherverwaltung, Collections, Standardbibliothek (implementiert)

- **Referenzzählung** für Strings und Array-Header: deterministische
  Freigabe, Element-Besitz (`push`/`insert`/`set` klonen, `pop`/`remove`
  transferieren), generierte Clone-/Drop-Helfer für Structs/Enums mit
  verwalteten Feldern, Cleanup an allen Ausgängen (`return`, `break`,
  `continue`, `?`-Fehlerpfad), Leak-Nachweis über `BLOOM_MEM_REPORT`
  (siehe `docs/MEMORY_MODEL.md`)
- **Collections:** `remove(a, i)` → `Option<T>`, `slice(a, start, end)`
  (Kopiesemantik; Views dokumentiert vertagt)
- **Standardbibliothek:** `join([string], sep)`, virtuelles Modul
  `std.math` (`sqrt`, `pow`, `floor`, `ceil`, `round`, `abs`, `min`,
  `max`, `clamp` + `_int`-Varianten) mit `use std::math;` und
  Symbol-Imports
- **Sprachfunktionen:** `?`-Operator für `Option<T>`, `panic(msg)`
  (divergierend, Exit 70), Oder-Patterns (`A | B`, inkl. Bindungs- und
  Unerreichbarkeitsprüfung), `pub use`-Re-Exports (transitiv,
  zyklussicher)
- 385 Tests (294 Unit + 91 E2E), darunter native Leak-Tests

Vertagt (weiterhin geplant):

- Slice-Views, `remove`-Varianten über Index-Rückgabe hinaus
- Modul-Re-Exports (`pub use geometry;`), Modul-Aliasse, `super`-Pfade
- `join` für Nicht-String-Typen, `std.json`, weitere `std`-Module

## Version 0.8.0 – Slice-Views und Runtime-Härtung (implementiert)

- **Stabiler Array-Header**: Header (`rc`, `data`, `len`, `cap`, `gen`)
  bleibt beim Wachstum stehen; nur der Elementpuffer wird reallokiert
- **`Slice<T>`** als eigener Typ (Variablen, Parameter, Rückgaben,
  Struct-Felder, Enum-Nutzlasten, generische Funktionen `Slice<T>`) mit
  Runtime-Repräsentation `{hdr, start, len, gen}`
- **`slice_view(a, start, end)`** erzeugt einen echten View (keine Kopie);
  `slice` behält die Kopiesemantik
- **Referenzzählung für Views**: der Header lebt, solange ein Array oder
  ein View ihn hält; Views überleben Scope-Ende und Neuzuweisung des
  Arrays; Reallokation durch `push` macht sie nicht ungültig
- **Generationenbasierte Invalidierung**: `insert`/`remove`/erfolgreiches
  `pop` invalidieren alle Views (kontrollierter Laufzeitfehler mit Quelle,
  Exit 70); `push`/`set`/fehlgeschlagene Operationen nicht
- **Lesen und Schreiben über Views** mit Grenz- und Gültigkeitsprüfung;
  Schreiben verlangt eine `var`-View-Bindung
- **`var self`-Aliasanalyse**: Das 0.7-Restrisiko ist mit dem heutigen
  Typsystem nicht konstruierbar (Struct-Globals sind verboten, E1014;
  Guard-Test vorhanden) — dokumentiert
- 399 Tests (301 Unit + 98 E2E), darunter native View-, Reallokations-,
  Invalidierungs- und Leak-Tests

Vertagt (weiterhin geplant):

- Feineres View-Tracking (Invalidierung nur betroffener Bereiche)
- Views auf Arrays von Structs/Enums (setzt solche Arrays voraus)
- Threads/atomare Zähler (eigenständiges Architekturprojekt)

## Version 0.9.0 – Standardbibliotheken und Formatter (implementiert)

- **`std::fs`** (15 Funktionen, siehe Spezifikation): Dateien lesen,
  schreiben, anhängen, kopieren, umbenennen, löschen; Verzeichnisse
  anlegen/auflisten/entfernen; Pfadverbindung. Alle Fehler über `Result`
  mit Operation + Pfad + Grund. Unter Windows UTF-16-Wide-APIs, daher
  Unicode- und Leerzeichen-Pfade nativ getestet. POSIX-Variante vorhanden
  (auf Windows nicht testbar).
- **`std::process`**: `run`/`run_in` ohne Shell, getrennte Argumente mit
  MSVC-Quoting (keine Shell-String-Interpolation),
  `ProcessOutput { exit_code, stdout, stderr }`; Startfehler ≠ Exit-Code
  ≠ 0; stdout/stderr nebenläufig per Polling gelesen (kein Deadlock,
  große Ausgaben getestet); POSIX-Variante (untested).
- **`std::time`**: `unix_seconds`/`unix_millis` (konsistente Systemuhr),
  `monotonic_millis` (QueryPerformanceCounter/CLOCK_MONOTONIC),
  `sleep_ms` (0 = No-op, negativ = Laufzeitfehler, große Werte in
  Blöcken), `format_utc` (ISO 8601, zeitzonenfrei, Jahre außerhalb
  0000–9999 = Laufzeitfehler).
- **`bloom fmt`**: tokenbasierter, kommentar- und literalerhaltender
  Formatter; `--check`; Datei- und Verzeichnismodus; idempotent;
  Parser-Verifikation vor dem Schreiben; sicherer Schreibvorgang über
  temporäre Datei; CRLF/BOM bleiben erhalten.
- Neue Beispiele `file_io.blm`, `process.blm`, `time.blm`; 423 Tests
  (311 Unit + 112 E2E) inklusive nativer Modul-, Fehler-, Deadlock-,
  Unicode-Pfad-, Leak- und Formatter-Korpus-Tests.

Vertagt (weiterhin geplant):

- Umgebungsvariablen, stdin-Weitergabe, Timeouts, Streaming für Prozesse
- Symlink-Garantien, `std.path`, weitere `std`-Module (`std.json`)
- Threads/atomare Zähler (eigenständiges Architekturprojekt)

## Version 0.10.0 – FFI, Windows-API, Netzwerk und HTTP (implementiert)

- **FFI:** `extern "C"`/`extern "system"`-Blöcke mit ABI-Typen
  (`c_int`/`c_uint`/`c_long`/`c_ulong`/`c_longlong`/`c_ulonglong`/`f32`/
  `rawptr`), expliziten Konvertierungen, `unsafe`-Grenze (E1070), klaren
  Deklarationsfehlern (E1071/E1072) und Linker-Integration
  (`--link`, `[build] link_libraries`, Auto-Link der Stdlib-Bibliotheken)
- **`std.win32`:** Prozess-ID, Tick-Count, Last-Error, Computername
  (UTF-16→UTF-8), Sleep und Meldungsbox (nicht automatisiert getestet)
- **`std.net`:** blockierendes TCP mit DNS, Listenern (Port 0,
  `local_port`), Lese-/Schreib-Timeouts, UTF-8-Prüfung empfangener Daten
  und interner Handle-Registry (Slot + Generation; doppeltes Schließen und
  fremde Handles sind Fehler)
- **`std.http`:** HTTP/1.0/1.1-Client (`get`/`post`/`request`) und
  -Server (`read_request`/`write_response`/`header_get`) mit erzwungenen
  Limits (Zeile 8 KiB, Header 16 KiB, Body 4 MiB), Ablehnung von
  Transfer-Encoding/doppelten Content-Length/fehlendem Host; **kein TLS**
  (HTTPS wird klar abgelehnt)
- 447 Tests (318 Unit + 129 E2E), darunter native FFI-, Linker-,
  Loopback-, Timeout-, HTTP-Client/Server- und Leak-Tests

Vertagt (weiterhin geplant):

- TLS/HTTPS, UDP, Chunked Transfer Encoding, Cookies, Redirects, Proxys
- C-Structs by value, Callbacks, variadische Funktionen, Pointer-Arithmetik
- Threads/Async-I/O (eigenständiges Architekturprojekt)

## Version 0.11.0 – TLS, HTTPS und Chunked (implementiert)

- **`std.tls`** (`use std::tls;`, Windows): TLS-Client über Schannel mit
  `tls_connect`, `tls_connect_with_ca`, `tls_connect_ex`, `tls_read`,
  `tls_write_all`, `tls_close`, `tls_set_timeout`; TLS 1.2 als Minimum,
  TLS 1.3 wenn verfügbar, SNI, begrenzter Handshake-Timeout, manuelle
  Zertifikatsvalidierung (System-Trust oder eigene CA-Datei in PEM/DER,
  Hostname-/EKU-/Zeitprüfung, Wildcards, iPAddress-SANs), transparente
  TLS-1.3-Post-Handshake-Verarbeitung, sauberes `close_notify`-Handling
  (fehlendes close_notify ist ein Fehler) — kein unsicherer Modus, keine
  eigene Kryptografie; auf Nicht-Windows klare Fehler
- **`std.http` mit HTTPS:** `https://` verbindet über `std::tls` mit
  voller Zertifikatsvalidierung; `get_with_ca`, `post_with_ca`,
  `request_with_ca` für eigene CAs; es gibt keinen Weg, die Prüfung
  abzuschalten
- **Chunked Transfer Encoding:** Dekodierung für Antworten (Client) und
  Anfragen (Server) inklusive Chunk-Extensions (akzeptiert/ignoriert),
  Trailer-Validierung (verworfen) und erzwungenen Limits (Chunk 4 MiB,
  Trailer 32); mehrdeutiges Framing (TE + Content-Length, doppelte
  Längen, unbekannte Encodings) wird abgelehnt (Request-Smuggling-Schutz)
- **HTTP-Härtung:** 1xx-Interim-Antworten werden verworfen (max. 5, `101`
  abgelehnt), `HEAD`/`204`/`304` haben nie einen Body (irreführendes
  `Content-Length` wird ignoriert), Chunked-Antworten über TLS,
  Transport-Abstraktion (TCP/TLS) im Client und Server
- **Ressourcen-Nachweis:** `BLOOM_MEM_REPORT=1` zählt zusätzlich offene
  Sockets (`live sockets`); TLS-Handles teilen sich die Net-Registry
- 456 Tests (318 Unit + 138 E2E), darunter native TLS-Handshake-,
  Trust-/Hostname-Fehler-, Timeout-, HTTPS-, Chunked- und Leak-/Socket-
  Tests gegen PowerShell-SslStream- und Node/OpenSSL-Testserver

Vertagt (weiterhin geplant):

- TLS-Server, UDP, Cookies, Redirects, Proxys, CRL/OCSP-Prüfung
- C-Structs by value, Callbacks, variadische Funktionen, Pointer-Arithmetik
- Threads/Async-I/O (eigenständiges Architekturprojekt)

## Version 0.12.0 – JSON und Pfade (implementiert)

- **`std.json`** (`use std::json;`): RFC-8259-Parser mit allen sechs
  Grundtypen, Unicode-Escapes inklusive Surrogatpaaren, verlustfreien
  Zahlen-Literalen (Originaltext bleibt erhalten), Fehlern mit
  Byte-/Zeilen-/Spaltenposition, doppelte Schlüssel werden abgelehnt,
  Einfügereihenfolge bleibt erhalten; kompakter und lesbarer Serializer
  (kürzeste round-trip-fähige Gleitkommadarstellung, `NaN`/`inf` nie);
  Accessoren (`get`/`has`/`get_*`/`as_*`), Arrays (`array_get`,
  `array_length`), `object_keys`, Mutation (`set` ersetzt an Ort und
  Stelle, `push`), Konstruktoren und `type_of`
- **Verwaltete JSON-Handles:** `json::JsonValue` ist ein von der
  Bloom-Referenzzählung verwalteter Handle auf einen refcounteten Baum —
  kein manuelles `free`, keine Leaks auf Fehlerpfaden, keine Zyklen
  (`set`/`push` lehnen sie ab), iterative Freigabe tiefer Bäume, jeder
  Knoten zählt in `live allocations`
- **JSON-Limits:** Eingabe 8 MiB, Tiefe 128, String 4 MiB, Zahlen-Token
  128 Zeichen, je 1 048 576 Elemente/Felder, 2 097 152 Werte;
  Serialisierung bis Tiefe 512
- **`std.path`** (`use std::path;`): rein lexikalische Operationen
  (`join`, `parent`, `file_name`, `extension`, `stem`, `is_absolute`,
  `normalize`, `with_extension`) mit Host-Plattform-Semantik; Windows
  erkennt Laufwerks-/UNC-Präfixe, `normalize` arbeitet Go-artig, klare
  Fehler für leere Pfade, `\\?\`-Präfixe und unvollständige UNC-Pfade;
  keine Dateisystemauflösung (Symlinks/Junctions/Kanonisierung bleiben
  ausdrücklich außen vor)
- **Beispiele:** `json_parse`, `json_create`, `json_pretty`,
  `json_file_config` (fs+json+path), `path_operations`
- 464 Tests (318 Unit + 146 E2E), darunter exakte lexikalische
  Pfadtests, Parser-/Accessor-/Serializer-/Roundtrip-/Limit-Tests,
  Datei-Integration und Leak-Nachweise für alle neuen Pfade

Vertagt (weiterhin geplant):

- JSON-Komfortschicht über HTTP (Routing, Templates), `std.json`-Streaming
- TLS-Server, UDP, Cookies, Redirects, Proxys, CRL/OCSP-Prüfung
- C-Structs by value, Callbacks, variadische Funktionen, Pointer-Arithmetik
- Threads/Async-I/O (eigenständiges Architekturprojekt)

## Version 0.13.0 – REST- und Web-Grundlagen (implementiert)

- **`std.web`** (`use std::web;`): synchroner Router auf der bestehenden
  HTTP-Infrastruktur — `route` (Methode, Muster, Anwendungsname),
  `resolve`/`resolve_request` mit Status 200/404/405 (inkl.
  deduplizierter `Allow`-Liste), Pfadparametern (`/users/:id`) und
  Query-Parametern als `std.json`-Objekt; Priorität fester Segmente vor
  Parametern, Ablehnung doppelter Muster/Namen/gleicher Shapes;
  Prozentdekodierung erst nach der Segmentierung (`%2F` erzeugt keine
  Routen), formartige Query-Dekodierung (`+` = Leerzeichen, erster Wert
  gewinnt); klare `Err`-Fälle (kaputte Kodierung, ungültiges UTF-8,
  Limits: 4096 Routen, 256 Segmente/Parameter, 64-KiB-Ziel); keine
  Callbacks — Anwendungen verzweigen über `Match.name`; Routing ist
  ausdrücklich keine Autorisierung
- **JSON-Hilfen:** `json_body`/`json_body_request` (klare Fehler, leere
  Bodies), `json_response`/`json_response_with` (application/json,
  Zusatz-Header) über die bestehende `std::json`-Serialisierung und den
  bestehenden HTTP-Schreibpfad
- **`http::write_response_full`:** validierte Zusatz-Header (keine
  CR/LF-Injection, Framing-Header gesperrt), `204`/`304` ohne Body
- **Kritischer Fix aus 0.12.0:** Die JSON-Freigabe behandelte geteilte
  Teilbäume falsch (bedingungslose Kindfreigabe) — ein in einem Reply
  gespeicherter, anderswo weiterbenutzter Wert wurde vorzeitig
  freigegeben (Heap-Korruption ab der zweiten Iteration). Die Freigabe
  ist jetzt refcount-treu; Regressionstest
  `json_shared_subtree_release_natively`
- **Verwaltete Werte:** `Router` als `Type::Web`-Handle (Referenzzählung
  wie `JsonValue`), `Match.params` als mitverwalteter JSON-Wert
- **Beispiele:** `web_router`, `web_path_params`, `web_json_api`
  (echter REST-Server), `web_errors`, `web_client_json`
- 470 Tests (318 Unit + 152 E2E), darunter exakte Routing-/Parameter-/
  Fehlererwartungen, ein voller REST-Server-Roundtrip (10 echte
  HTTP-Anfragen plus Client-Beispiel) und der Shared-Subtree-Regressionstest

Vertagt (weiterhin geplant):

- Templates/JSON-API-Komfort, statische Inhalte, Middleware
- GUI-Anbindung (aufbauend auf `std::win32`)
- WebSocket, JSON-Streaming, Datenbankabstraktion

## Version 0.14.0 – HTML-Ausgabe und Templates (implementiert)

- **`std.html`** (`use std::html;`): `escape` maskiert `&`, `<`, `>`,
  `"`, `'` (UTF-8-sicher, ohne Abschneiden, überlaufgeprüfte
  Puffergröße); **Template-Engine** mit `{{pfad}}` (verschachtelte
  Pfade, automatisch escaped), `{{#if}}`/`{{else}}`/`{{/if}}`
  (strikt boolesch) und `{{#each}}`/`{{/each}}` (Arrays, `{{this}}`,
  verschachtelbar); Parser einmal (`template`), beliebig oft rendern
  (`render`), verwaltete `Template`-Handles (`Type::Html`,
  Referenzzählung)
- **Strikte Wertregeln:** Strings/Zahlen/Booleans werden eingesetzt;
  `null`, Arrays, Objekte, fehlende Pfade und falsche Typen sind
  kontrollierte Fehler (keine Debug-Ausgaben, keine stillen
  Leerstrings); Parserfehler mit Byte-/Zeilen-/Spaltenposition;
  `<script>`/`<style>` werden abgelehnt (keine JS/CSS-Kontexte)
- **Ressourcenlimits:** Quelle 64 KiB, 16384 Teile, Verschachtelung 32,
  Ausgabe 1 MiB, 65536 Iterationen (auch verschachtelt), Pfad 256 B;
  iterativer Parser/Renderer (kein C-Stack-Wachstum)
- **HTTP-Integration:** `render_page`/`render_page_with` rendern vor
  dem Schreiben (keine halben Antworten) und senden über
  `write_response_full` mit `text/html; charset=utf-8`, validierten
  Zusatz-Headern und 204/304 ohne Body
- **Beispiele:** `html_escape`, `template_basic`, `template_conditions`,
  `template_lists`, `template_errors`, `web_html_page` (echter
  HTML-Server)
- 477 Tests (318 Unit + 159 E2E), darunter Escaping-/Parser-/Render-/
  Limit-/Server-Tests mit Leak-Prüfung (auch auf Fehlerpfaden)

Vertagt (weiterhin geplant):

- Template-Includes, Makros, ausführbare Ausdrücke, Raw-HTML
- Kontextabhängige URL-/JavaScript-Escapierung
- GUI-Anbindung (aufbauend auf `std::win32`)
- WebSocket, JSON-Streaming, Datenbankabstraktion

## Version 0.15.0 – Dateien, Bytes und statische Inhalte (implementiert)

- **`std.fs`-Erweiterung:** `read_text_limited(path, limit)` prüft die
  Dateigröße **vor** dem Lesen (UTF-8-Validierung wie `read_text`);
  `read_bytes` liefert binäre Inhalte als refcountetes
  `std.fs::Bytes`-Handle (`Type::Bytes`, Bauart wie `JsonValue`),
  `bytes_len` die Länge; NUL-Bytes bleiben erhalten, Obergrenze 8 MiB
- **Binäre HTTP-Antworten:** `http::write_response_bytes` und
  `write_response_bytes_head` senden einen `Bytes`-Puffer mit exakter
  Byte-Länge (kein UTF-8-Zwang); `_head` liefert dieselben Kopfzeilen
  ohne Body; `204`/`304` bleiben bodylos; Content-Type/Zusatz-Header
  werden wie bisher validiert (CR/LF vor dem Schreiben abgelehnt)
- **Vorlagen aus Dateien:** `html::template_file(path)` und
  `html::template_file_from(root, relative)` laden und parsen mit
  demselben Parser/Limit (64 KiB); der relative Pfad wird validiert
  (`..`, absolute Pfade, Laufwerks-/UNC-Präfixe sind Fehler) und die
  geöffnete Datei gegen den Wurzelordner geprüft (Symlinks/Junctions
  inklusive); UTF-8 wird vor dem Parsen geprüft; kein Neuladen
- **`web::static_file(connection, root, request)`:** `GET`/`HEAD`
  (sonst `405` mit `Allow: GET, HEAD`); Pfad einmal prozentdekodiert,
  kodierte Trenner/`..`/absolute Pfade → `400`; fehlende Dateien `404`,
  Lesefehler und > 8 MiB → `500`; MIME-Tabelle nach Endung (unbekannt:
  `application/octet-stream`) plus `X-Content-Type-Options: nosniff`;
  vollständig gepuffert (kein Streaming/Range/ETag), kein
  `index.html`-Fallback
- **Beispiele:** `template_file`, `web_static_files`, `web_html_site`,
  `web_static_errors`
- 486 Tests (318 Unit + 168 E2E), darunter Binär-/MIME-/HEAD-/404-/405-/
  Traversal-/Junction-/Limit-/Leak-Tests

Vertagt (weiterhin geplant):

- Streaming/Range-Requests, ETag/Last-Modified, Verzeichnis-Indizes,
  automatisches Neuladen von Dateien
- Template-Includes, Makros, ausführbare Ausdrücke, Raw-HTML
- GUI-Anbindung (aufbauend auf `std::win32`)
- WebSocket, JSON-Streaming, Datenbankabstraktion

## Version 0.16.0 – Formulare (implementiert)

- **`std.web`-Formulare:** `form_body(body)` parst
  `application/x-www-form-urlencoded`; `form_body_request(request)`
  prüft zusätzlich den Content-Type (Parameter erlaubt, `charset` muss
  UTF-8 sein, fehlender/fremder Typ ist ein Fehler). Leerer Body ist
  ein gültiges leeres Formular
- **Datenmodell:** verwaltetes `FormData`-Handle (`Type::Form`) mit
  geordneten Name/Wert-Paaren; wiederholte Feldnamen bleiben erhalten
  (Checkbox-Gruppen, Mehrfachauswahlen), leere Namen und leere Werte
  sind zulässig und von „Feld fehlt“ unterscheidbar
- **Zugriff:** `form_count`, `form_has`, `form_get` (genau ein Wert,
  sonst `Err`), `form_first` (`Option`), `form_values` (alle Werte in
  Reihenfolge), `form_names` (verschiedene Namen in
  Erstauftritts-Reihenfolge)
- **Serialisierung:** `form_encode` schreibt `name=value&...` in
  Eintragsreihenfolge, Leerzeichen als `+`, reservierte Bytes als
  `%XX`; Parse → Encode → Parse erhält Werte und Reihenfolge
- **Dekodierung:** `+` als Leerzeichen (nur Formulare), Prozentescapes
  genau einmal, UTF-8-Validierung; `%`, `%0`, `%GG` und ungültiges
  UTF-8 sind Fehler und werden nicht abgeschnitten oder repariert
- **Limits:** Body 1 MiB, 4096 Einträge, Name 256 Bytes, Wert 64 KiB
  (nach Dekodierung); zusätzlich gilt das bestehende
  `read_request`-Limit von 4 MiB
- **Beispiele:** `form_parse`, `form_multiple_values`, `form_encode`,
  `web_form_server`, `web_form_errors`
- 492 Tests (320 Unit + 172 E2E), darunter Parse-/Limit-/Round-trip-/
  Content-Type-/Echter-HTTP-Client-und-Server-/Leak-Tests

Vertagt (weiterhin geplant):

- Multipart-Formulare und Datei-Uploads, automatische
  Formularvalidierung (Sessions und CSRF-Token kamen in 0.17)
- Streaming/Range-Requests, ETag/Last-Modified, Verzeichnis-Indizes,
  automatisches Neuladen von Dateien
- Template-Includes, Makros, ausführbare Ausdrücke, Raw-HTML
- GUI-Anbindung (aufbauend auf `std::win32`)
- WebSocket, JSON-Streaming, Datenbankabstraktion

## Version 0.17.0 – Cookies, Sessions und CSRF (implementiert)

- **Cookies lesen:** `cookies(request)` liest alle `Cookie`-Header;
  `cookie_count`, `cookie_has`, `cookie_get` (genau ein Wert),
  `cookie_first`, `cookie_names`. Werte bleiben wörtlich (keine
  Prozentdekodierung, kein Unquoting), ungültige Paare werden
  ignoriert, doppelte Namen sind mehrdeutig (`cookie_get` → `Err`).
  Limits: 64 Cookies, Name 64 Bytes, Wert 4096 Bytes
- **`Set-Cookie` erzeugen:** strukturierte `CookieOptions`
  (`path`, `max_age`, `same_site`, `http_only`, `insecure_http`);
  `set_cookie`/`delete_cookie` prüfen Namen, Wert, Pfad und Attribute,
  lehnen CR/LF und ungültige SameSite-Werte ab; `Secure` ist Standard,
  `SameSite=None` verlangt `Secure`; kein Domain-Scope; mehrere Zeilen
  über `cookie_headers` bzw. getrennte Header bleiben getrennt
- **Sessions:** In-Memory-Store mit Kapazität (1..65535) und absoluter
  TTL (1..31536000 s), monotone Uhr; `session_new` erzeugt
  32-Zufallsbytes-IDs (Base64url, 43 Zeichen) über `BCryptGenRandom`
  bzw. `/dev/urandom`; `session_load` lehnt fehlende, doppelte,
  malformed, unbekannte und abgelaufene IDs ab; `session_data` liefert
  das gemeinsame JSON-Objekt; `session_destroy`, `session_expire`,
  `session_count`; volle Stores lehnen neue Sessions ab; keine
  Verdrängung, kein Persistieren, keine Verteilung
- **CSRF:** `csrf_token` (32 Zufallsbytes, 43 Zeichen, pro Session),
  `csrf_check` (konstantzeitlicher Vergleich), `csrf_rotate`,
  `csrf_clear`; Token liegt getrennt von den Anwendungsdaten
- **Session-Cookies:** `session_cookie` (Secure, HttpOnly, SameSite=Lax,
  Path=/, Max-Age) und `session_cookie_insecure` für lokale Entwicklung
- **Beispiele:** `web_cookies`, `web_session_basic`,
  `web_session_expiry`, `web_csrf_form`, `web_session_errors`
- 502 Tests (321 Unit + 181 E2E), darunter Cookie-Parsing/Injection,
  Session-Lebenszyklus, Kapazität, Ablauf, Zufalls-Fehlerpfad,
  CSRF-Bindung und ein vollständiger HTTP-E2E mit Bloom-Client und
  Bloom-Server (Cookie setzen, erneut senden, Session ändern,
  CSRF prüfen)

Vertagt (weiterhin geplant):

- Persistente oder verteilte Sessions, Benutzerkonten, Login- und
  Rollen-Systeme, JWT/OAuth, Multipart-Uploads
- Streaming/Range-Requests, ETag/Last-Modified, automatisches Neuladen
- Template-Includes, Makros, ausführbare Ausdrücke, Raw-HTML
- GUI-Anbindung (aufbauend auf `std::win32`)
- WebSocket, JSON-Streaming, Datenbankabstraktion

## Version 0.18.0 – SQLite und persistente Anwendungen (implementiert)

- **Eingebettetes SQLite 3.54.1:** Amalgamation unter
  `runtime/sqlite3.c`/`.h` (Public Domain, Herkunft und SHA3-256 in
  `runtime/sqlite3_NOTICE.md`), vom Compiler eingebettet und bei
  `std::db`-Programmen als zweite Übersetzungseinheit mit
  `SQLITE_THREADSAFE=0`, `SQLITE_OMIT_LOAD_EXTENSION`, `SQLITE_DQS=0`
  kompiliert. Keine Installation, kein Server, keine Downloads.
- **`std::db`-API:** `open`, `open_memory`, `busy_timeout`, `prepare`,
  `bind_null/int/float/bool/text/bytes`, `step`, `execute`, `reset`,
  `column_count/name/index/type/int/float/bool/text/bytes`, `changes`,
  `last_insert_rowid`, `begin`/`commit`/`rollback`.
- **Semantik:** Bind-Parameter 1-basiert, Spalten 0-basiert; genau eine
  SQL-Anweisung pro `prepare`; klare Statement-Zustände
  (`fresh`/`row`/`done`/`error`, Reset löscht Bindungen); strikte
  Spaltentypen (INTEGER/REAL/TEXT/BLOB/NULL ohne stille Konvertierung);
  mehrdeutige Spaltennamen sind Fehler; ungebundene Parameter sind NULL.
- **Transaktionen:** explizit, verschachteltes BEGIN ist ein Fehler;
  offene Transaktionen werden beim Schließen zurückgerollt.
- **Ownership:** Statements halten ihre Datenbank am Leben
  (Referenzzählung), `sqlite3_finalize`/`close_v2` erst beim letzten
  Handle; Fehlerpfade geben Teilressourcen frei; alle Tests leakfrei.
- **Windows:** Dateipfade mit Leerzeichen/Unicode/absoluten Pfaden,
  Verzeichnis- und fehlende Elternpfade als Fehler, schreibgeschützte
  Dateien öffnen (Schreiben scheitert mit SQLITE_READONLY),
  Busy-Timeout begrenzt.
- **Web-Integration:** `web_database_app` speichert Formularwerte über
  gebundene Parameter in SQLite, liest sie zurück und rendert sie
  escaped über `std::html`.
- **Sessions:** `web::session_registered` unterscheidet explizit
  „Handle lebt“ von „Session ist im Store gültig“; kein Wiedereinsetzen
  eines verworfenen Handles.
- **Beispiele:** `db_open`, `db_prepared`, `db_query`,
  `db_transactions`, `db_blob`, `db_errors`, `web_database_app`
- 508 Tests (322 Unit + 186 E2E), darunter Typ-/Zustands-/SQL-Sicherheits-
  /Busy-/Dateipfad-/Leak-Tests und ein HTTP-E2E mit Formular, Datenbank
  und HTML-Antwort

Vertagt (weiterhin geplant):

- PostgreSQL/MySQL, ORMs, Schema-Migrationen, Connection-Pools,
  asynchrone Datenbankzugriffe, Savepoints
- Persistente oder verteilte Sessions, Benutzerkonten, JWT/OAuth,
  Multipart-Uploads
- Streaming/Range-Requests, ETag/Last-Modified, automatisches Neuladen
- Template-Includes, Makros, ausführbare Ausdrücke, Raw-HTML
- GUI-Anbindung (aufbauend auf `std::win32`)
- WebSocket, JSON-Streaming

## Version 0.19.0 – Authentifizierung und Autorisierung (implementiert)

- **Argon2id** (Referenzimplementierung 20190702, CC0/Apache-2.0,
  eingebettet unter `runtime/argon2/`): `password_hash`,
  `password_verify`, `password_needs_rehash`. Parameter m=64 MiB, t=3,
  p=1, Salt 16 Bytes, Tag 32 Bytes, PHC-Format. Verifikation prüft den
  Hashstring vorab (nur Argon2id v19, m≤256 MiB, t≤10, p≤4); gemessen
  ca. 115–120 ms pro Hash auf einem Ryzen 7 5800X (Windows 11, MSVC).
- **Konten in SQLite:** eigenes Schema mit Metatabelle/Version 1,
  `setup` idempotent und transaktional, unbekannte/fremde Schemata
  werden abgelehnt. `register_user`, `authenticate`, `user_find`,
  `user_get`, `user_disable`/`user_enable`, `password_change`.
  Normalisierte ASCII-Benutzernamen mit UNIQUE-Constraint; unbekannter
  Name und falsches Passwort liefern dieselbe Meldung (mit
  Dummy-Argon2-Aufwand).
- **Login-Sessions:** `web::session_regenerate` (neue ID, neuer
  CSRF-Token, gleiche Daten/Expiry), `auth::login` rotiert nach
  erfolgreicher Verifikation; Sessiondaten enthalten nur Benutzer-ID
  und `auth_version`; `current_user` prüft Registrierung, Konto,
  Aktivstatus und Version; `logout` zerstört die Serversession.
  Passwortwechsel/Deaktivierung erhöhen `auth_version` und machen
  ältere Sessions ungültig.
- **Rollen:** `role_create` (idempotent), `role_delete`, `role_assign`,
  `role_remove`, `roles_of`, `has_role`; kein Standard-Admin,
  unbekannte Rollen sind keine Freigabe.
- **Login-Limiter:** festes Zeitfenster, monotone Uhr, begrenzte
  Eintragszahl (Verdrängung des ältesten Fensters), explizite
  Schlüssel, pro Prozess.
- **Bugfix:** `web::delete_cookie` sendet jetzt `Max-Age=0` (vorher
  fehlte das Attribut); Regressionstest angepasst.
- **Beispiele:** `auth_password`, `auth_users`, `auth_sessions`,
  `auth_limiter`, `web_auth_app` (vollständige Anmeldung mit
  Registrierung, Login, Rollen, Admin-Bereich, Logout,
  Passwortwechsel und Deaktivierung)
- 516 Tests (323 Unit + 193 E2E), darunter Argon2-KAT-Vektor,
  Hash-Grenzen, Schema-Fehler, SQL-Injection-Versuche, Session-Rotation,
  Invalidierung, CSRF-Pflicht bei Login/Logout/Passwortwechsel,
  Limiter-Grenzen und ein vollständiger HTTP-E2E mit Bloom-Client

Vertagt (weiterhin geplant):

- OAuth/OIDC, externe Login-Anbieter, MFA/Passkeys,
  E-Mail-Verifikation, Passwort-Reset per E-Mail, JWT als
  Session-Ersatz, verteilte Session-Stores, ABAC/Policy-Sprachen,
  automatische Rollenvererbung, Admin-Dashboard
- PostgreSQL/MySQL, ORMs, Schema-Migrationen, Connection-Pools
- Streaming/Range-Requests, ETag/Last-Modified, automatisches Neuladen
- Template-Includes, Makros, ausführbare Ausdrücke, Raw-HTML
- GUI-Anbindung (aufbauend auf `std::win32`)
- WebSocket, JSON-Streaming

## Version 0.20.0 – UI-Komponenten und Themes (implementiert)

- **`std.ui`** baut HTML aus festen Bausteinen: Dokument, Container,
  Stack, Row, Grid, Section, Divider, Überschriften, Text, Karten,
  Alerts, Badges, Buttons, Links, Navigation, Breadcrumbs, Formulare
  (Label, Input, Textarea, Select, Checkbox, Radio, Field mit Hilfe/
  Fehler, Submit), Tabellen, Avatar, Progress, Skeleton, Pagination und
  Empty State.
- **Sicherheit:** kein Roh-HTML; Text und Attributwerte über den
  bestehenden Escaper; URL-Allowlist (`http(s)`, ein führender `/`,
  relativ, Fragment); validierte IDs, Feldnamen, Klassen, Tokens und
  Größen; Limits (8 MiB Ausgabe, 20000 Knoten, Tiefe 64, 1 MiB Text,
  4096 Bytes Attributwert).
- **Themes:** `light`, `dark`, `system` über `data-theme` und
  `prefers-color-scheme`; Stylesheet (rund 14 KB) beim Bauen
  eingebettet, als String oder mit Cache-Header auslieferbar;
  `prefers-reduced-motion` und ein 720-px-Breakpoint sind enthalten.
- **Integration:** `send_page`/`send_stylesheet` für `std::http`;
  Formulare bleiben mit `std::web`/`std::auth` kombinierbar; CSRF bleibt
  Aufgabe der Anwendung.
- **Behoben:** doppelte Tabellenzellen, geleakte Description-Meta,
  hängende Stack-Tags, geleakte Varianten-/Pagination-Strings,
  Checkbox-/Radio-Mutatoren trafen den Wrapper statt das Eingabefeld.
- **Beispiele:** `ui_hello`, `ui_layouts`, `ui_components`, `ui_forms`,
  `ui_themes`, `ui_table`, `ui_dashboard`, `ui_login_page`,
  `ui_gallery`
- 523 Tests (323 Unit + 200 E2E), darunter Escaping, URL-/Token-
  Ablehnung, Komponenten-/ARIA-Prüfungen, Theme-/Stylesheet-Tests,
  HTTP-Auslieferung mit Headern und alle neun Beispiele unter
  `BLOOM_MEM_REPORT=1`

Vertagt (weiterhin geplant):

- Native Windows-Desktopfenster und -Controls (eigenes Modul)
- Clientseitige Interaktivität (JavaScript), freies CSS/HTML,
  Formularvalidierung im Browser
- Weitere Komponenten: Menüs, Tabs, Modals, Charts, Tooltips
- Streaming/Range-Requests, ETag/Last-Modified, automatisches Neuladen
- Template-Includes, Makros, ausführbare Ausdrücke, Raw-HTML
- WebSocket, JSON-Streaming

## Version 1.0.0+ – Ökosystem (geplant)

- Paketmanager, Language Server (LSP), Debug-Informationen
- Optimierungen, zusätzliche Backends (LLVM IR, WebAssembly)
- Threads/Nebenläufigkeit mit sicherem Modell
- Closures, Iteratoren, Pattern-Matching-Ausbau

## Offene Designfragen

- **Speicherverwaltung:** Referenzzählung inklusive Slice-Views ist in 0.8
  umgesetzt (siehe `docs/MEMORY_MODEL.md`); offen: Threadsicherheit (atomare
  Zähler), feineres View-Tracking, `var self`-Garantien bei künftigen
  Referenztypen (heute nicht konstruierbar, Guard-Test vorhanden).
- **Fehlerbehandlung:** `?` für `Result` und `Option` sowie `panic` sind
  umgesetzt; offen: `panic` mit sauberem Cleanup (aktuell direkter Exit).
- **Objektmodell:** Structs + Methoden + Komposition; Vererbung nur, wenn sie
  sich sauber einfügt.
- **Referenzempfänger:** `&self`/`&mut self` erst mit einem sicheren
  Referenzmodell.
- **Rekursion:** braucht Referenztypen; RC macht rekursive Strukturen
  möglich, erfordert dann aber Zyklusbehandlung (Weak-Referenzen o. Ä.).

Begründung der Priorisierung: Zuerst vollständige, getestete Sprachkern-
Funktionen (Methoden, Enums, Module), dann Sammlungen und Systemnähe, dann
Web und Ökosystem – nach dem Leitsatz „kleinster vollständiger Compiler vor
großer Zahl unvollständiger Features“.
