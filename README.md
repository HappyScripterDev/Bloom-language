# Bloom

Bloom ist eine statisch typisierte, kompilierte Programmiersprache. Sie verbindet
die Klarheit und Systemnähe von C mit einer modernen, gut lesbaren Syntax und
orientiert sich langfristig an der Ausdruckskraft von C++ – ohne deren
historischen Ballast zu übernehmen.

**Aktueller Stand: Version 0.15.0.** Der Compiler ist vollständig implementiert:
Lexer, Parser, Typprüfung, Codegenerierung nach C und native Kompilierung mit
MSVC, Clang, GCC oder tcc. Heap-Speicher wird per **Referenzzählung**
deterministisch freigegeben; **Slice-Views** erlauben referenzierende
Array-Ausschnitte ohne Kopien. Seit 0.11.0 gibt es einen **TLS-Client**
(`std.tls`, Windows Schannel) und **HTTPS** inklusive
**Chunked-Transfer-Encoding**; seit 0.12.0 kommen **`std.json`** (Parser,
Wertmodell, Serializer) und **`std.path`** (lexikalische
Pfadoperationen) hinzu; seit 0.13.0 **`std.web`** — ein synchroner
REST-Router mit Pfad-/Query-Parametern und JSON-Hilfen; seit 0.14.0
**`std.html`** – sichere HTML-Escapierung und eine kleine Template-Engine
mit Bedingungen, Schleifen und automatischer Escapierung; seit 0.15.0
**Vorlagen aus Dateien** (`html::template_file`/`template_file_from`),
**binäre HTTP-Antworten** über `std.fs::Bytes` (`http::write_response_bytes`)
und **statische Dateien** mit Wurzelordner-Schutz (`web::static_file`).

```bloom
struct Pair<A, B> {
    first: A,
    second: B
}

enum Shape {
    Circle(float),
    Square(float)
}

impl Shape {
    fn circle(radius: float) -> Shape {
        return Shape::Circle(radius);
    }

    fn area(self) -> float {
        return match self {
            Shape::Circle(radius) => 3.14159 * radius * radius,
            Shape::Square(side) => side * side
        };
    }
}

fn first_of<T>(values: [T]) -> Option<T> {
    if len(values) == 0 {
        return Option::None;
    }
    return Option::Some(values[0]);
}

fn main() -> int {
    let s = Shape::circle(2.0);
    println(str(s.area()));

    let p = Pair { first: 1, second: "one" };
    println(str(p.first));

    match first_of([10, 20]) {
        Option::Some(v) => println("first: " + str(v)),
        Option::None => println("empty")
    };

    match 95 {
        n if n >= 90 => println("A"),
        n if n >= 50 => println("B"),
        _ => println("F")
    };

    return 0;
}
```

## Was Bloom 0.15.0 kann

- Native `.exe`-Programme unter Windows (und native Programme auf anderen
  Plattformen mit vorhandenem C-Compiler)
- Eigene Dateiendung `.blm`, eigene Grammatik, eigener Compiler, eigene Laufzeit
- Statische Typisierung mit `int`, `float`, `bool`, `string`, `char`, Arrays
- **Automatische Speicherverwaltung per Referenzzählung** für Strings,
  Arrays, Slice-Views und Structs/Enums mit verwalteten Feldern –
  deterministische Freigabe an allen Kontrollfluss-Ausgängen (auch
  `?`-Fehlerpfad); nachweisbar leak-frei über `BLOOM_MEM_REPORT=1`
- **Slice-Views (`Slice<T>`)**: `slice_view(a, start, end)` erzeugt einen
  referenzierenden View auf einen Array-Bereich (keine Kopie). Views
  bleiben bei `push`/Reallokation gültig, überleben das Ende der
  Array-Variable und werden von `insert`/`remove`/`pop` kontrolliert
  invalidiert (Laufzeitfehler statt Speicherzugriffsfehler). `slice`
  behält die Kopiesemantik
- **Generische Structs, Funktionen und Methoden** (`Pair<A, B>`, `fn first<T>(...)`,
  `impl<T> Box<T>`, auch `Slice<T>` als Parameter/Rückgabe) mit
  Typinferenz, Turbofish (`identity::<int>(x)`) und Monomorphisierung
- **Methoden und `impl`-Blöcke** für Structs und Enums, inklusive assoziierter
  Funktionen (`Shape::circle(2.0)`) und generischer `impl`-Blöcke
- **Enums** mit Nutzdaten (positionell/benannt), auch generisch
- **Pattern Matching** (`match`) mit statischer Exhaustiveness-Prüfung,
  Literal-Patterns, Guards (`n if n > 0`) und **Oder-Patterns** (`1 | 2 | 3`)
- **`Option<T>` und `Result<T, E>`** als eingebaute generische Enums
- **`?`-Operator** für `Result` und `Option` (Fehlerweitergabe)
- **`panic(msg)`** für absichtliche fatale Fehler (Exit 70 mit Quellort)
- **String-Bibliothek**: `contains`, `starts_with`, `ends_with`, `trim`,
  `replace`, `to_upper`, `to_lower` (ASCII), `split`, `join`
- **Arrays**: `push`, `pop`, `insert`, `remove`, `slice` (Kopie),
  Grenzprüfung, Referenzsemantik, **stabiler Header**
- **`std.math`** (`use std::math;`): `sqrt`, `pow`, `floor`, `ceil`,
  `round`, `abs`, `min`, `max`, `clamp` und `_int`-Varianten
- **`std.fs`** (`use std::fs;`): `read_text`, `write_text`, `append_text`,
  `exists`, `is_file`, `is_dir`, `file_size`, `create_dir_all`,
  `remove_file`, `remove_dir`, `copy_file`, `rename`, `current_dir`,
  `join_path`, `read_dir` – alle Fehler über `Result`, Unicode-Pfade
  unter Windows (UTF-16-Wide-APIs); zusätzlich **`read_text_limited`**
  (Größenlimit vor dem Lesen, UTF-8-Validierung) und **`read_bytes`**
  (binäre Inhalte als verwaltetes `std.fs::Bytes` mit `bytes_len`,
  eingebettete NUL-Bytes bleiben erhalten, Obergrenze 8 MiB)
- **`std.process`** (`use std::process;`): `run`/`run_in` starten
  Programme ohne Shell mit getrennten Argumenten und liefern
  `ProcessOutput { exit_code, stdout, stderr }` (Startfehler ≠
  Exit-Code ≠ 0; kein Deadlock bei großen Ausgaben)
- **`std.time`** (`use std::time;`): `unix_seconds`, `unix_millis`,
  `monotonic_millis`, `sleep_ms`, `format_utc` (ISO-8601, zeitzonenfrei)
- **`bloom fmt`**: tokenbasierter, kommentar- und literalerhaltender
  Formatter mit `--check`, Verzeichnismodus, Idempotenz und sicherem
  Schreiben
- **FFI:** `extern "C"`/`extern "system"`-Deklarationen mit ABI-Typen
  (`c_int`, `c_uint`, `c_long`, `c_ulong`, `c_longlong`, `c_ulonglong`,
  `f32`, `rawptr`), expliziten Konvertierungen, `unsafe`-Grenze und
  Linker-Integration (`--link`, `[build] link_libraries`)
- **`std.win32`** (`use std::win32;`): `process_id`, `tick_count_ms`,
  `last_error`, `get_computer_name`, `sleep_ms`, `message_box`
- **`std.net`** (`use std::net;`): blockierendes TCP mit DNS, Listenern
  (Port 0), `tcp_connect`/`accept`/`read`/`write_all`/`close`,
  Timeouts und interner Handle-Registry (keine rohen OS-Deskriptoren)
- **`std.http`** (`use std::http;`): HTTP/1.0- und HTTP/1.1-Client und
  -Server (`get`, `post`, `request`, `read_request`, `write_response`,
  `header_get`) mit Größenlimits, strikter Validierung,
  **Chunked-Transfer-Encoding** (Dekodierung für Antworten und Anfragen,
  Trailer werden validiert und verworfen) und **HTTPS** über `std.tls`
  (Zertifikat wird immer geprüft; `get_with_ca`, `post_with_ca`,
  `request_with_ca` für eigene CA-Dateien)
- **`std.tls`** (`use std::tls;`, Windows): TLS-Client über Schannel
  (`tls_connect`, `tls_connect_with_ca`, `tls_connect_ex`,
  `tls_read`/`tls_write_all`/`tls_close`/`tls_set_timeout`) mit
  TLS 1.2 als Minimum, TLS 1.3 wenn verfügbar, SNI, manueller
  Zertifikatsvalidierung (System-Trust oder eigene CA, Hostnamen- und
  EKU-Prüfung, Gültigkeitszeitraum), begrenztem Handshake-Timeout,
  sauberem Close-Notify-Handling und klaren Fehlern — ohne unsicheren
  Modus und ohne eigene Kryptografie
- **`std.json`** (`use std::json;`): RFC-8259-Parser, refcountetes
  Wertmodell (automatisch über die Bloom-Referenzzählung verwaltet),
  kompakter und lesbarer Serializer (`parse`, `stringify`,
  `stringify_pretty`, `get`/`has`, `get_string`/`get_int`/`get_float`/
  `get_bool`, `as_string`/`as_int`/`as_float`/`as_bool`, `array_get`/
  `array_length`, `object_keys`, `set`/`push`, Konstruktoren
  `null`/`make_bool`/`make_int`/`make_float`/`make_string`/`make_array`/
  `make_object`, `type_of`) mit Unicode-Escapes inkl. Surrogatpaaren,
  verlustfreien Zahlen-Literalen, Positionsangaben in Fehlern, klaren
  Größen-/Tiefenlimits und Zyklus-Schutz
- **`std.path`** (`use std::path;`): rein lexikalische Pfadoperationen
  (kein Dateisystemzugriff) mit Host-Plattform-Semantik —
  `join`, `parent`, `file_name`, `extension`, `stem`, `is_absolute`,
  `normalize` (Go-artiges lexikalisches Aufräumen) und `with_extension`;
  Windows erkennt Laufwerks- und UNC-Präfixe, `\\?\`-Präfixe werden
  klar abgelehnt
- **`std.web`** (`use std::web;`): synchroner REST-Router auf der
  bestehenden HTTP-Infrastruktur — Routen mit Methode, Pfadmuster
  (`/users/:id`) und Anwendungsnamen registrieren (`route`), Anfragen
  auflösen (`resolve`, `resolve_request`) mit Status 200/404/405
  (inklusive `Allow`-Liste), Pfadparametern und Query-Parametern als
  `std.json`-Objekt, Prozentkodierung mit sauberer Trennung von
  Segmentierung und Dekodierung, Priorität fester Segmente vor
  `:param`, Ablehnung doppelter/mehrdeutiger Routen; dazu
  `json_body`/`json_body_request` (Anfrage-JSON mit klaren Fehlern) und
  `json_response`/`json_response_with` (JSON-Antworten mit
  `application/json` und optionalen Zusatz-Headern). Keine Callbacks:
  Anwendungen verzweigen über den Routennamen; Routing ist keine
  Autorisierung
- **`web::static_file`**: statische Dateien aus einem Wurzelordner
  ausliefern (nur `GET`/`HEAD`, sonst `405` mit `Allow: GET, HEAD`) —
  Pfad wird genau einmal prozentdekodiert, kodierte Trenner, `..`,
  absolute Pfade und Laufwerks-/UNC-Präfixe werden abgelehnt (`400`);
  der geöffnete Dateipfad wird nach Auflösung von Symlinks/Junctions
  gegen den Wurzelordner geprüft, Ausbrüche ergeben `400`; fehlende
  Dateien/Ordner `404`, andere Lesefehler und Dateien über 8 MiB `500`;
  `Content-Type` aus der Endung (unbekannt:
  `application/octet-stream`) plus `X-Content-Type-Options: nosniff`,
  Antworten vollständig gepuffert (kein Streaming, keine Range-Requests,
  kein Index-Fallback)
- **`http::write_response_full`**: Antworten mit validierten
  Zusatz-Headern (Framing-Header bleiben unter Kontrolle der Bibliothek,
  CR/LF-Injection ausgeschlossen); `204`/`304` liefern nie einen Body
- **`http::write_response_bytes`/`write_response_bytes_head`**: binäre
  Antworten über `std.fs::Bytes` mit exakter Byte-Länge (NUL-Bytes
  bleiben erhalten); die `_head`-Variante sendet dieselben Kopfzeilen
  inklusive echtem `Content-Length` ohne Body; `204`/`304` liefern nie
  einen Body
- **`std.html`** (`use std::html;`): `escape` (maskiert `&`, `<`, `>`,
  `"`, `'`), eine kleine Template-Engine (`template` parst einmal,
  `render` rendert beliebig oft) mit `{{pfad}}`-Platzhaltern
  (automatisch HTML-escaped, verschachtelte Pfade, `this` in `#each`),
  `{{#if}}`/`{{else}}`/`{{/if}}` und `{{#each}}`/`{{/each}}`;
  strikte Typregeln (Strings, Zahlen und Booleans werden eingesetzt;
  `null`, Arrays und Objekte sind Fehler wie fehlende Pfade),
  Parserfehler mit Byte-/Zeilen-/Spaltenposition, `<script>`/`<style>`
  werden abgelehnt, Ressourcenlimits (Quelle 64 KiB, 16384 Teile,
  Verschachtelung 32, Ausgabe 1 MiB, 65536 Iterationen); verwaltete
  `Template`-Handles (Referenzzählung), `render_page`/
  `render_page_with` senden mit `text/html; charset=utf-8` über die
  bestehende HTTP-Response-Infrastruktur. Escaping schützt HTML-Text
  und quotierte Attribute — **nicht** JavaScript-, CSS- oder
  URL-Kontexte; Raw-HTML gibt es nicht. **`template_file`** lädt eine
  Vorlage aus einer Datei (max. 64 KiB, gültiges UTF-8, derselbe Parser
  und dieselben Limits wie `template`); **`template_file_from`** löst
  einen relativen Pfad gegen einen Wurzelordner auf und prüft nach dem
  Öffnen, dass die Datei (inklusive aufgelöster Symlinks/Junctions)
  innerhalb des Ordners liegt (`..`, absolute Pfade und Ausbrüche werden
  abgelehnt)
- **Module und Imports** (`use`), Untermodule, `pub`-Sichtbarkeit,
  **`pub use`-Re-Exports**, zyklische Importe, Deduplizierung,
  qualifizierte Pfade
- `let`/`var`/`const` mit Typinferenz, Funktionen mit Rekursion
- `if`/`else if`/`else`, `while`, `for i in a..b`, `break`, `continue`
- Operatoren mit definierter Semantik; UTF-8-Strings
- Aussagekräftige Fehlermeldungen mit Datei, Zeile, Spalte, Fehlercode, Hilfe
- CLI mit `build`, `run`, `check`, `init`, `--help`, `--version`; Projekte
  mit `bloom.toml`

Nicht enthalten (siehe `docs/ROADMAP.md`): Klassen/Vererbung, Threads,
Closures, Iteratoren, rekursive Datentypen, Paketmanager,
TLS-Server, HTTP/2, GUI-Integration. Das Speichermodell (inkl.
Invalidierungsregeln und Grenzen) ist in `docs/MEMORY_MODEL.md` dokumentiert.

## Voraussetzungen

1. **Rust** (1.70 oder neuer) zum Bauen des Compilers
2. **Ein C-Compiler als Backend**: MSVC (Build Tools), Clang, GCC oder tcc.
   Die Erkennung ist automatisch; `BLOOM_CC` erzwingt einen Compiler.

## Compiler bauen

```bash
cd Bloom
cargo build --release
```

Ergebnis: `target\release\bloom.exe` (Windows) bzw. `target/release/bloom`.

## Erste Schritte

```bash
bloom run examples/hello.blm        # kompilieren und ausführen
bloom build examples/structs.blm    # erzeugt examples/structs.exe
bloom check examples/enums.blm      # nur prüfen

cd examples/modules_project         # Mehrdatei-Projekt mit Modulen
bloom run
```

Den generierten C-Code ansehen:

```bash
bloom build examples/hello.blm --emit-c build/hello.c
```

## CLI-Übersicht

| Kommando | Bedeutung |
| --- | --- |
| `bloom build [datei.blm] [-o ausgabe] [--emit-c pfad]` | Natives Programm erzeugen |
| `bloom run [datei.blm]` | Kompilieren und ausführen |
| `bloom check [datei.blm]` | Lexer, Parser und Typprüfung ausführen |
| `bloom build/run --link <lib\|datei>` | Zusaetzliche Bibliothek/Objektdatei linken |
| `bloom fmt [--check] <datei\|ordner>` | Quellcode formatieren bzw. Format pruefen |
| `bloom init <name>` | Neues Projekt anlegen |
| `bloom --version`, `bloom --help` | Version bzw. Hilfe |

Ohne Dateiangabe sucht `bloom` im aktuellen Verzeichnis nach `bloom.toml` und
verwendet den dort konfigurierten Einstiegspunkt. Beim Kompilieren werden alle
über `use` erreichten Module automatisch geladen.

## Projektstruktur

```text
Bloom/
├── Cargo.toml
├── README.md
├── LICENSE
├── docs/
│   ├── GETTING_STARTED.md
│   ├── LANGUAGE_SPEC.md
│   ├── COMPILER_ARCHITECTURE.md
│   ├── MEMORY_MODEL.md
│   └── ROADMAP.md
├── examples/              # lauffähige Beispiele (auch known_errors/)
│   └── modules_project/   # Mehrdatei-Projekt mit Modulen
├── runtime/bloom_rt.c     # C-Laufzeitbibliothek (wird eingebettet)
├── src/                   # Compiler in Rust
└── tests/e2e.rs           # Integrationstests (kompilieren und führen aus)
```

## Dokumentation

- [docs/GETTING_STARTED.md](docs/GETTING_STARTED.md) – Installation und erstes Programm
- [docs/LANGUAGE_SPEC.md](docs/LANGUAGE_SPEC.md) – Grammatik und Sprachsemantik
- [docs/COMPILER_ARCHITECTURE.md](docs/COMPILER_ARCHITECTURE.md) – Aufbau des Compilers
- [docs/MEMORY_MODEL.md](docs/MEMORY_MODEL.md) – Speicherverwaltung: Analyse und Empfehlung
- [docs/ROADMAP.md](docs/ROADMAP.md) – implementierte und geplante Funktionen

## Tests

```bash
cargo test        # Unit-Tests + Integrationstests (486 Tests)
cargo clippy --all-targets
cargo fmt
```

Die Integrationstests kompilieren echte `.blm`-Programme (auch Mehrdatei-
Projekte) und führen sie aus. Fehlt ein C-Compiler, werden die betroffenen
Tests mit einem Hinweis übersprungen.

## Lizenz

MIT, siehe [LICENSE](LICENSE).
