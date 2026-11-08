# Bloom

Bloom ist auf **Server-Software, Webanwendungen und Automatisierung**
ausgerichtet und wächst um native Windows-Desktopprogramme. Die Syntax
ist an Rust angelehnt (Typen, `Result`, Pattern-Matching), die
Bedienung bewusst einfach wie bei Python, und der Compiler erzeugt
nativen C-Code mit C-ähnlicher Laufzeit. Alles, was für Server und
Automatisierung nötig ist, soll ohne externe Laufzeit oder
Paketmanager funktionieren: Dateien, Prozesse, HTTP/TLS, JSON,
SQLite, Authentifizierung, Sessions und native GUI.

Die Idee dahinter ist simpel. Ich wollte eine Sprache, die man an einem
Nachmittag lernen kann, mit der man aber trotzdem echte Programme
schreiben kann: Dateien lesen, im Netzwerk reden, JSON verarbeiten,
HTML-Seiten ausliefern. Alles davon ist schon eingebaut und braucht
keine externen Bibliotheken.

Stand: **0.23.0**. Getestet wird auf Windows (MSVC). Linux und macOS
sind im Code vorgesehen, aber ich entwickle und teste fast nur auf
Windows. `std::desktop` ist ausschließlich unter Windows verfügbar.

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

    return 0;
}
```

## Was schon geht

- Eigener Compiler in Rust: Lexer, Parser, Typprüfung, C-Code und
  Aufruf des C-Compilers. Kein LLVM, keine externen Abhängigkeiten.
- Statische Typen, Structs, Enums mit Daten, Generics, Methoden,
  Pattern Matching, `Option`/`Result`, `?`-Operator.
- Speicher wird automatisch per Referenzzählung verwaltet. Kein GC,
  kein manuelles `free`. Ob wirklich nichts übrig bleibt, kann man mit
  `BLOOM_MEM_REPORT=1` nachschauen.
- Slice-Views: `slice_view` zeigt auf einen Array-Bereich, ohne zu
  kopieren. Wird der Array umgebaut, fliegen die Views kontrolliert
  raus, statt dass irgendwo Müll gelesen wird.
- Standardbibliothek ohne Ballast: `std.math`, `std.fs`, `std.process`,
  `std.time`, `std.net`, `std.http` (Client und Server, mit HTTPS),
  `std.json`, `std.path`, `std.web` (Router, Formulare, Cookies, Sessions,
  CSRF), `std.html`, `std.db` (eingebettetes SQLite) und `std.auth`
  (Argon2id, Konten, Rollen, Session-Login).
- `bloom fmt` räumt den Quelltext auf, `bloom check` prüft ohne zu
  bauen, `bloom init` legt ein Projekt an.
- FFI zu C und ein paar Windows-Funktionen, falls man doch mal ans
  System muss.

## Schnellstart

Du brauchst Rust und irgendeinen C-Compiler.

```bash
cd Bloom
cargo build --release
```

Ergebnis: `target\release\bloom.exe`.

```bash
bloom run examples/hello.blm
```

Wenn Bloom keinen C-Compiler findet, sagt es dir, was fehlt. Welchen
es nimmt, kann man mit `BLOOM_CC` erzwingen (`set BLOOM_CC=clang`).
Mehr dazu steht in [docs/GETTING_STARTED.md](docs/GETTING_STARTED.md).

## Was in 0.23 dazugekommen ist

- `std.env`: `args`, `get`, `has`, `set`, `remove`, `current_dir`,
  `set_current_dir`, `executable_path`, `os`, `arch` und `process_id`.
  Fehlende und leere Variablen bleiben unterscheidbar (leere Werte sind
  unter Windows prozesslokal, da Windows sie nicht speichert).
- `std.process` erweitert: `spawn`, `wait`, `try_wait`, `kill`,
  `run_options` mit Arbeitsverzeichnis, Timeout (max. 24 h) und
  Ausgabelimit (Standard 8 MiB, max. 64 MiB). Programme werden direkt
  über `CreateProcessW` gestartet, Argumente einzeln mit den
  MSVC-Quoting-Regeln übergeben – keine implizite Shell, keine
  Verkettung von Benutzereingaben. stdout/stderr werden parallel
  gelesen (kein Pipe-Deadlock); Timeout beendet den Kindprozess.
- `std.cli`: Command/Parsed-Handles, Flags, Optionen, Positions-
  argumente, Subcommands, Defaults, Pflichtfelder, `--name=value`,
  `--`, kurze Flags, Hilfe als String. Der Parser beendet den Prozess
  nie selbst; Fehler sind konkrete `Err`-Werte.
- `std.collections`: `List`, `Map` und `Set` für Strings mit
  Push/Pop/Get/Set/Insert/Remove/Clear/Length/Capacity (Liste),
  Insert/Get/Has/Remove/Keys/Values/Length/Clear (Map) und
  Insert/Has/Remove/Values/Length/Clear (Set). Alle Werte werden
  referenzgezählt kopiert und freigegeben; Schlüssel sind nicht leer,
  maximal 1 Mio. Einträge. **Noch nicht generisch** (nur Strings).
- `std.io`: explizite Textausgabe auf stdout/stderr, Flush,
  `read_line` (EOF getrennt von Leerzeile) und `read_stdin` mit Limit;
  ungültiges UTF-8 wird gemeldet.
- Beispiele: `env_info`, `cli_basic`, `cli_subcommands`,
  `process_run`, `process_output`, `process_timeout`,
  `collections_list`, `collections_map`, `collections_set`,
  `automation_build`, `automation_project`, `bloom_task_runner`.
- Behoben: leere Umgebungswerte, Auswahl-Leck im Map/Set-Lookup,
  CLI-Defaults werden beim Parsen materialisiert.
## Was (noch) nicht geht

Damit hier keine falschen Erwartungen entstehen:

- Keine Threads, keine Closures, keine Iteratoren.
- Keine Vererbung, keine Interfaces, keine Makros.
- Kein Paketmanager, kein Language Server, kein Debugger.
- Kein HTTP/2, keine WebSockets, kein UDP.
- TLS gibt es nur als Client unter Windows; einen HTTPS-Server gibt es
  nicht (TLS-Terminierung davor).
- `std::desktop` gibt es nur unter Windows; andere Systeme liefern einen
  klaren Fehler. MacOS/Linux-GUIs sind weder implementiert noch getestet.
- Keine Toolbars, Tree-Views, Tabellen-Controls, Drag-and-drop,
  eigenen Zeichenflächen, Direct3D, Themes oder GUI-Designer. Menüs
  gibt es als einfache Menüleiste mit Popups und Trennlinien.
- Keine Threads: Fenster und Controls gehören dem Thread, der sie
  erzeugt hat; alle Desktop-Aufrufe müssen dort laufen.
- Keine automatische Control-Skalierung bei DPI-Wechseln; Layouts
  werden neu berechnet, manuell positionierte Controls bleiben
  pixelgenau. `Enter` löst keinen Standardbutton aus (Space auf dem
  fokussierten Button funktioniert). Screenreader-/Multi-Monitor-Tests
  fehlen; Dateidialog-Erfolgspfade wurden manuell geprüft, automatisiert
  ist der Abbruchpfad.
- Collections sind in 0.23 auf Strings beschränkt; generische
  `List<T>`/`Map<K,V>`/`Set<T>` folgen erst mit einem ausgebauten
  Genericsystem.
- Neue Prozess-APIs (`spawn`/`wait`/`try_wait`/`kill`/`run_options`)
  sind unter Windows implementiert; POSIX nutzt weiterhin `run`/`run_in`.
  Environment-Overrides und stdin-Umleitung fehlen in 0.23. Timeouts
  beenden den direkten Kindprozess, keine Job-Objekte (kein Baum-Kill).
- CLI unterstützt eine Subcommand-Ebene; Optionen vor dem Subcommand
  werden nicht unterstützt. `--help`/`--version` sind explizit zu
  registrieren.
- Leere Umgebungswerte werden unter Windows prozesslokal simuliert und
  nicht an Kindprozesse vererbt; Umgebungsänderungen sind nicht
  thread-sicher.
- `std::io` arbeitet mit UTF-8-Text; Konsole-Codepages und rohe
  Byte-Ströme auf stdin werden nicht automatisch konvertiert.
- Kein OAuth/OIDC, keine MFA, keine E-Mail-Verifikation, kein
  Passwort-Reset per E-Mail, kein JWT.
- Die Sprache ist 0.x. Ich ändere APIs auch mal, wenn es sein muss.

## Doku

- [docs/GETTING_STARTED.md](docs/GETTING_STARTED.md) – von null bis zum
  laufenden Programm, mit Beispielen für alles Wichtige
- [docs/LANGUAGE_SPEC.md](docs/LANGUAGE_SPEC.md) – die kompletten
  Sprachregeln, wenn du es genau wissen willst
- [docs/MEMORY_MODEL.md](docs/MEMORY_MODEL.md) – wie die
  Referenzzählung funktioniert und wo ihre Grenzen sind
- [docs/COMPILER_ARCHITECTURE.md](docs/COMPILER_ARCHITECTURE.md) – wie
  der Compiler von innen aussieht
- [docs/ROADMAP.md](docs/ROADMAP.md) – was fertig ist und was als
  Nächstes kommt

## Selbst entwickeln

```bash
cargo test                    # 545 Tests: Unit + echte kompilierte Programme
cargo clippy --all-targets
cargo fmt
```

Die Integrationstests kompilieren kleine `.blm`-Programme, führen sie
aus und schauen sich die Ausgabe an. Wenn kein C-Compiler da ist,
werden die betroffenen Tests übersprungen statt rot zu werden.

Beispiele liegen alle in `examples/` und lassen sich direkt starten:

```bash
bloom run examples/json_parse.blm
bloom run examples/web_json_api.blm
bloom run examples/template_file.blm
bloom run examples/form_parse.blm
bloom run examples/web_form_server.blm
bloom run examples/web_cookies.blm
bloom run examples/db_prepared.blm
bloom run examples/auth_password.blm
bloom run examples/web_csrf_form.blm
bloom run examples/ui_hello.blm
bloom run examples/desktop_hello.blm
bloom run examples/desktop_gallery.blm
bloom run examples/cli_basic.blm -- --verbose --target linux datei.txt
bloom run examples/process_output.blm
```

## Lizenz

MIT. Mach damit, was du willst.

Die eingebettete SQLite-Amalgamation (`runtime/sqlite3.c`/`.h`) ist
Public Domain. Version, Quelle und Prüfsumme stehen in
[runtime/sqlite3_NOTICE.md](runtime/sqlite3_NOTICE.md).

Die eingebettete Argon2-Referenzimplementierung (`runtime/argon2/`)
steht unter CC0 1.0 oder Apache 2.0 (nach Wahl). Die Lizenztexte und
Hinweise stehen in [runtime/argon2_NOTICE.md](runtime/argon2_NOTICE.md).
