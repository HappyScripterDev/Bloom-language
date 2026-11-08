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

Stand: **0.22.0**. Getestet wird auf Windows (MSVC). Linux und macOS
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

## Was in 0.22 dazugekommen ist

- Native Layout-Engine: `row`/`column`-Container, Padding, Abstand,
  feste und flexible Größen (Gewichte), Alignment, Verschachtelung bis
  Tiefe 32 und automatische Neuberechnung bei `WM_SIZE` und
  `WM_DPICHANGED`. Layouts sind fenstergebunden (IDs ab 10000), keine
  separaten Handles nötig.
- Drei neue native Controls: ComboBox (`combo_*`), mehrzeiliges
  Textfeld (`text_area_create`, Zeilenenden werden LF/CRLF-normalisiert)
  und ListBox (`list_*`) mit Hinzufügen, Entfernen, Anzahl, Auswahl und
  ausgewähltem Text. Ungültige Indizes liefern `Err`.
- Menüs: Menüleiste, Popup-Menüs, Einträge, Trennlinien und
  Aktivieren/Deaktivieren. Menü-IDs liegen bei 1000+ und kollidieren
  nicht mit Controls; Menü-Klicks erscheinen als `menu`-Events.
- Dateidialoge: `open_file_dialog` und `save_file_dialog` über die
  nativen Windows-Dialoge (Unicode-Pfade, Filter, Abbruch als
  `Option::None`). `std::fs`/`std::path` bleiben für den eigentlichen
  Dateizugriff zuständig.
- Tastatur: Tab/Shift+Tab über die native Windows-Navigation
  (`IsDialogMessage`), deaktivierte und versteckte Controls werden
  übersprungen; `focused_control` liefert die aktuelle ID.
- Robustere Ereignisse: Das Schließereignis geht nie verloren, wird
  genau einmal geliefert und kommt nach bereits eingereihten Events.
  Überlauf der 64-Platz-Warteschlange verwirft das älteste Ereignis und
  zählt in `events_dropped`. Programmgesteuerte Textänderungen erzeugen
  weiterhin keine `input`-Events.
- DPI: `dpi_scale` für getestete Umrechnung; Layouts werden nach
  Größen-/DPI-Änderungen neu angewendet. Die Grenzen (keine
  automatische Control-Skalierung, keine echte Multi-Monitor-Prüfung)
  sind dokumentiert.
- Interne Test-Hooks: `debug_event_flood` und `debug_cross_thread`
  (nicht Teil der stabilen API) für Queue- und Thread-Tests.
- Beispiele: `desktop_layout_stack`, `desktop_resize`,
  `desktop_combo_box`, `desktop_text_area`, `desktop_list_box`,
  `desktop_menu`, `desktop_file_dialog`, `desktop_keyboard`,
  `desktop_dpi`, `desktop_gallery`.
- Behoben: Layout-Reihenfolge bei Spalten, geleakte Auswahl-Strings,
  doppeltes Freigeben von Menüs/Layouts beim Schließen.

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
cargo test                    # 538 Tests: Unit + echte kompilierte Programme
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
```

## Lizenz

MIT. Mach damit, was du willst.

Die eingebettete SQLite-Amalgamation (`runtime/sqlite3.c`/`.h`) ist
Public Domain. Version, Quelle und Prüfsumme stehen in
[runtime/sqlite3_NOTICE.md](runtime/sqlite3_NOTICE.md).

Die eingebettete Argon2-Referenzimplementierung (`runtime/argon2/`)
steht unter CC0 1.0 oder Apache 2.0 (nach Wahl). Die Lizenztexte und
Hinweise stehen in [runtime/argon2_NOTICE.md](runtime/argon2_NOTICE.md).
