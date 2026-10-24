# Bloom

Bloom ist eine kleine, kompilierte Programmiersprache. Sie wird nach C
übersetzt und dann von einem ganz normalen C-Compiler zu einer echten
`.exe` gebaut. Von außen erinnert sie an Rust und C, aber alles ist
absichtlich klein: keine Makros, keine Header, kein Build-System, das
man erst studieren muss.

Die Idee dahinter ist simpel. Ich wollte eine Sprache, die man an einem
Nachmittag lernen kann, mit der man aber trotzdem echte Programme
schreiben kann: Dateien lesen, im Netzwerk reden, JSON verarbeiten,
HTML-Seiten ausliefern. Alles davon ist schon eingebaut und braucht
keine externen Bibliotheken.

Stand: **0.21.0**. Getestet wird auf Windows (MSVC). Linux und macOS
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

## Was in 0.21 dazugekommen ist

- `std.desktop`: echte native Windows-Fenster und -Steuerelemente. Fenster
  mit Titel und Größe, Labels, Buttons, einzeilige Texteingaben, Checkboxen,
  Sichtbarkeit, Aktivierung, Fokus, Position und Größe.
- Ereignisse ohne Callback-Zwang: `next_event` blockiert ohne Busy-Wait
  und liefert `click`, `check`, `input` und `close` mit Control-ID und
  aktuellen Werten. Ein privater Weckmechanismus verhindert verlorene
  Ereignisse nach Cross-Thread-`SendMessage`; die Warteschlange ist pro
  Fenster begrenzt.
- Fenster und Ressourcen: `Window` ist ein verwalteter Handle mit
  Referenzzählung. Schließen ist idempotent, Controls werden nach dem
  Schließen abgelehnt, der letzte Handle zerstört das native Fenster.
  Bis zu 64 Fenster und 512 Controls pro Fenster.
- Unicode: alle Texte laufen über UTF-8 ↔ UTF-16 mit Validierung
  (Umlaute, Emoji, Japanisch getestet). Steuerzeichen und NUL-Bytes in
  Texten werden abgelehnt.
- DPI: Per-Monitor-v2-Awareness beim ersten Fenster (mit Fallback),
  öffentliche Koordinaten und Größen in logischen 96-DPI-Einheiten;
  `WM_DPICHANGED` passt die Fenstergröße an. Controls werden nicht
  automatisch umskaliert.
- Windows-Build: Programme mit `std::desktop` linken standardmäßig als
  GUI-Anwendung (`/SUBSYSTEM:WINDOWS /ENTRY:mainCRTStartup`, kein
  Konsolenfenster). `bloom build ... --console` erzwingt die Konsole,
  z. B. für Tests. Normale Programme bleiben unverändert.
- Beispiele: `desktop_hello`, `desktop_button`, `desktop_input`,
  `desktop_checkbox`, `desktop_counter`, `desktop_layout`,
  `desktop_unicode`, `desktop_errors`.

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
- Keine Menüs, Toolbars, Tabellen-/Tree-Controls, Drag-and-drop,
  eigenen Zeichenflächen, Direct3D, Themes oder Designer.
- Keine Threads: Fenster und Controls gehören dem Thread, der sie
  erzeugt hat; alle Desktop-Aufrufe müssen dort laufen.
- Keine automatische Layout-Engine und keine automatische
  Control-Skalierung bei DPI-Wechseln oder Fenstergrößenänderung.
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
cargo test                    # 529 Tests: Unit + echte kompilierte Programme
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
```

## Lizenz

MIT. Mach damit, was du willst.

Die eingebettete SQLite-Amalgamation (`runtime/sqlite3.c`/`.h`) ist
Public Domain. Version, Quelle und Prüfsumme stehen in
[runtime/sqlite3_NOTICE.md](runtime/sqlite3_NOTICE.md).

Die eingebettete Argon2-Referenzimplementierung (`runtime/argon2/`)
steht unter CC0 1.0 oder Apache 2.0 (nach Wahl). Die Lizenztexte und
Hinweise stehen in [runtime/argon2_NOTICE.md](runtime/argon2_NOTICE.md).
