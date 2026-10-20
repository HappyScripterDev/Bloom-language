# Erste Schritte mit Bloom

Diese Anleitung führt vom leeren Rechner zum laufenden Bloom-Programm.
Sie beschreibt den tatsächlichen Stand von Bloom 0.12.0.

## 1. Rust installieren

Der Bloom-Compiler ist in Rust geschrieben.

- Windows: <https://www.rust-lang.org/tools/install> (rustup-init.exe ausführen)
- Linux/macOS: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`

Prüfen:

```bash
rustc --version
cargo --version
```

## 2. Einen C-Compiler einrichten

Bloom 0.5 erzeugt C-Code und übersetzt ihn mit einem vorhandenen C-Compiler
zu einem nativen Programm.

### Windows

Empfohlen (eine Option genügt):

1. **Visual Studio Build Tools** mit dem Paket „Desktop development with C++“:
   <https://visualstudio.microsoft.com/visual-cpp-build-tools/>
   Bloom findet die Installation automatisch über `vswhere.exe`.
2. **Clang** (LLVM): <https://releases.llvm.org/> – Installation, die
   `clang.exe` in den PATH aufnimmt.
3. **MinGW-w64 GCC**: z. B. über <https://winlibs.com/> – `gcc.exe` muss im PATH liegen.
4. **Tiny C Compiler (tcc)**: <https://bellard.org/tcc/>

Prüfen (je nach gewähltem Compiler):

```bat
cl
clang --version
gcc --version
tcc -v
```

### Linux / macOS

```bash
gcc --version      # oder clang --version
```

### Auswahl erzwingen

Bloom probiert in dieser Reihenfolge: `BLOOM_CC`, MSVC, Clang, GCC, tcc.

```bat
set BLOOM_CC=clang
set BLOOM_CC=C:\pfad\zu\cl.exe
```

```bash
export BLOOM_CC=clang
```

Ohne gefundenen C-Compiler bricht `bloom build` mit Fehlercode E3002 ab und
zeigt Einrichtungshinweise an.

## 3. Den Compiler bauen

```bash
cd Bloom
cargo build --release
```

Ergebnis: `target\release\bloom.exe` (Windows) bzw. `target/release/bloom`.

Tipp: Das Verzeichnis `target/release` kann in den PATH aufgenommen werden,
damit `bloom` überall verfügbar ist.

## 4. Das erste Programm

`hello.blm`:

```bloom
fn main() -> int {
    println("Hello from Bloom!");
    return 0;
}
```

Ausführen:

```bash
bloom run hello.blm
```

Ausgabe:

```text
Hello from Bloom!
```

Was dabei passiert:

1. Der Lexer zerlegt den Quelltext in Tokens.
2. Der Parser baut daraus einen Syntaxbaum (AST).
3. Die Typprüfung prüft alle Namen, Typen und Rückgabewerte.
4. Der Codegenerator erzeugt C-Code (inklusive der eingebetteten Laufzeit).
5. Ein C-Compiler (MSVC/Clang/GCC/tcc) erzeugt die native `.exe`.
6. Das Programm wird gestartet.

## 5. Natives Programm bauen

```bash
bloom build hello.blm
```

Erzeugt `hello.exe` neben der Quelldatei. Mit Zielangabe:

```bash
bloom build hello.blm -o C:\projekte\hello.exe
```

Den erzeugten C-Code ansehen:

```bash
bloom build hello.blm --emit-c build\hello.c
```

## 6. Nur prüfen

```bash
bloom check hello.blm
```

Führt Lexer, Parser und Typprüfung aus, erzeugt aber kein Programm.
Fehlerhafte Programme liefern eine Meldung wie:

```text
error[E1002]: incompatible types: expected `int`, found `string`
  --> main.blm:2:20
  |
2 |     let age: int = "hello";
  |                    ^^^^^^^ expected `int`, found `string`
  |
  = help: the declared type is `int`
```

## 7. Ein Projekt anlegen

```bash
bloom init HelloWorld
cd HelloWorld
bloom run
bloom build
```

Struktur:

```text
HelloWorld/
├── bloom.toml
├── src/
│   └── main.blm
└── README.md
```

`bloom.toml`:

```toml
[package]
name = "HelloWorld"
version = "0.2.0"
entry = "src/main.blm"
```

Im Projektverzeichnis liest `bloom build` und `bloom run` den Einstiegspunkt
aus `bloom.toml`. `bloom build` legt das Programm unter `build/HelloWorld.exe` ab.

## 8. Eingaben

`input("...")` liest eine Zeile von der Konsole:

```bloom
fn main() -> int {
    let name = input("Wie heißt du? ");
    println("Hallo, " + name + "!");
    return 0;
}
```

Ein vollständiges Beispiel mit Zahlen: `examples/calculator.blm`.

## 9. Structs

Structs bündeln mehrere Werte unter einem Namen:

```bloom
struct Point {
    x: int,
    y: int
}

fn main() -> int {
    var p = Point {
        x: 10,
        y: 20
    };

    println(p.x);
    println(p.y);

    p.x = 100;
    println(p.x);

    return 0;
}
```

Wichtige Regeln:

- Beim Erzeugen müssen **alle** Felder genau einmal angegeben werden.
- Felder werden mit `.` gelesen und geschrieben (`p.x`).
- Feldzuweisungen sind nur erlaubt, wenn die Variable mit `var` deklariert wurde.
- Structs haben **Wertsemantik**: `var b = a;` erzeugt eine unabhängige Kopie.
  Eine Änderung an `b.x` verändert `a.x` nicht.
- Structs können als Funktionsparameter und Rückgabewerte verwendet werden.
- Structs dürfen andere Structs als Felder enthalten (nicht sich selbst).
- Ein vollständiges Beispiel: `examples/structs.blm`.

Nicht unterstützt in 0.5: Arrays von Structs/Enums, rekursive Structs/Enums,
Vergleiche (`==`) zwischen Structs/Enums, direktes `println(p)` (einzelne
Felder ausgeben), Structs als globale Variablen.

## 10. Methoden

Methoden werden in `impl`-Blöcken definiert:

```bloom
struct Counter {
    n: int
}

impl Counter {
    fn get(self) -> int {
        return self.n;
    }

    fn inc(var self) -> void {
        self.n = self.n + 1;
    }

    fn plus(self, k: int) -> Counter {
        return Counter { n: self.n + k };
    }
}

fn main() -> int {
    var c = Counter { n: 0 };
    c.inc();
    c.inc();
    println(c.get());          // 2
    println(c.plus(10).get()); // 12
    return 0;
}
```

Regeln:

- `self` liest den Empfänger (Wertkopie); `var self` verändert ihn in place.
- `var self`-Methoden verlangen eine veränderliche Variable (`var`);
  auf `let`-Werten oder Temporären sind sie Fehler.
- Methodenketten sind erlaubt; Methoden dürfen sich gegenseitig aufrufen.
- Vollständiges Beispiel: `examples/methods.blm`.

## 11. Enums und Match

```bloom
enum Message {
    Quit,
    Move { x: int, y: int },
    Write(string)
}

fn size(m: Message) -> int {
    return match m {
        Message::Quit => 0,
        Message::Move { x, y } => x + y,
        Message::Write(text) => len(text)
    };
}

fn main() -> int {
    println(size(Message::Move { x: 3, y: 4 }));  // 7
    println(size(Message::Write("hello")));       // 5
    return 0;
}
```

- `match` muss **alle** Varianten abdecken (oder `_`); sonst Fehler E1041.
- Bindungen (`text`, `x`) sind im Arm lokal.
- Literal-Patterns und Guards sind erlaubt:

```bloom
fn grade(score: int) -> string {
    return match score {
        0 => "zero",
        n if n >= 90 => "A",
        n if n >= 50 => "B",
        _ => "F"
    };
}
```

- `Option<T>` und `Result<T, E>` sind eingebaute Enums:

```bloom
fn divide(a: int, b: int) -> Result<int, string> {
    if b == 0 {
        return Result::Err("division by zero");
    }
    return Result::Ok(a / b);
}
```

- Der `?`-Operator reicht Fehler automatisch weiter:

```bloom
fn half(a: int) -> Result<int, string> {
    let q = divide(a, 2)?;   // bei Err sofort return Err(...)
    return Result::Ok(q);
}
```

Beispiele: `examples/enums.blm`, `examples/option_result.blm`,
`examples/result_try.blm`.

## 11a. Generics und Methoden auf Enums

```bloom
struct Box<T> {
    value: T
}

impl<T> Box<T> {
    fn get(self) -> T {
        return self.value;
    }
}

fn identity<T>(value: T) -> T {
    return value;
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
            Shape::Circle(r) => 3.14159 * r * r,
            Shape::Square(s) => s * s
        };
    }
}

fn main() -> int {
    let b = Box { value: 42 };        // T = int (Inferenz)
    println(b.get());
    println(identity("text"));        // T = string
    println(identity::<int>(7));      // Turbofish
    println(str(Shape::circle(2.0).area()));
    return 0;
}
```

- Typparameter werden aus Argumenten, erwartetem Rückgabetyp oder explizit
  (`::<...>`) bestimmt; nicht ableitbare Fälle sind E1045.
- `Type::funktion(...)` ruft assoziierte Funktionen (ohne `self`) auf.
- Beispiele: `examples/generics.blm`, `examples/enum_methods.blm`.

## 11b. Text- und Array-Funktionen

```bloom
fn main() -> int {
    let s = "  Hello, World  ";
    let clean = trim(s);
    println(contains(clean, "World"));   // true
    println(starts_with(clean, "Hello")); // true
    println(replace(clean, "World", "Bloom"));
    println(to_upper(clean));            // HELLO, WORLD
    let words = split(clean, ", ");      // [string]
    println(words[1]);

    var xs: [int] = [1, 2, 3];
    let last = pop(xs);                  // Option<int>
    insert(xs, 0, 10);
    let removed = remove(xs, 1);         // Option<int>
    let part = slice(xs, 0, 2);          // Kopie des Bereichs
    println(join(words, " | "));         // join über [string]
    return 0;
}
```

Beispiele: `examples/text_tools.blm`, `examples/collections.blm`.

## 11c. Fehlerbehandlung, `panic` und `std.math`

```bloom
use std::math;

fn first(o: Option<int>) -> Option<int> {
    let v = o?;                          // None wird weitergereicht
    return Option::Some(v + 1);
}

fn check(n: int) -> int {
    if n < 0 {
        panic("negative input");         // Exit 70 mit Meldung + Zeile
    }
    return n;
}

fn main() -> int {
    println(str(math::sqrt(81.0)));      // 9.0
    println(match first(Option::None) {
        Option::Some(v) => str(v),
        Option::None => "none"
    });
    println(check(5));
    return 0;
}
```

## 11d. Oder-Patterns

```bloom
fn classify(n: int) -> string {
    return match n {
        1 | 2 | 3 => "small",
        4 | 5 => "medium",
        _ => "large"
    };
}
```

Alle Alternativen müssen dieselben Bindungen einführen (oder keine).

## 11e. Speicherverwaltung

Strings, Arrays, Slice-Views und Structs/Enums mit verwalteten Feldern
werden per **Referenzzählung** verwaltet und deterministisch freigegeben.
Du musst nichts tun: Speichern, Überschreiben, Blöcke, `return`, `break`,
`continue` und der `?`-Fehlerpfad werden vom Compiler korrekt behandelt.
Mit `BLOOM_MEM_REPORT=1` gibt das Programm am Ende die Zahl offener
Allokationen auf stderr aus (`live allocations: 0` = kein Leak). Details:
`docs/MEMORY_MODEL.md`.

## 11f. Slice-Views

Ein Slice-View zeigt auf einen Bereich eines Arrays, ohne Elemente zu
kopieren:

```bloom
var values = [10, 20, 30, 40, 50];
let view = slice_view(values, 1, 4);   // [1, 4): 20, 30, 40
println(len(view));                    // 3
println(view[1]);                      // 30

values[2] = 99;                        // sichtbar ueber den View
println(view[1]);                      // 99

var writable = slice_view(values, 0, 3);
writable[0] = 7;                       // schreibt ins Array
```

- `slice_view` erzeugt einen View; `slice` erzeugt weiterhin eine Kopie.
- Views bleiben gueltig, wenn das Array waechst (`push`).
- `insert`, `remove` und ein erfolgreiches `pop` **invalidieren** alle
  Views; ein spaeterer Zugriff meldet einen Laufzeitfehler.
- Schreiben ueber einen View verlangt `var view = ...` (wie bei Arrays).

Beispiel: `examples/slices.blm`.

## 11g. Dateien, Prozesse und Zeit

```bloom
use std::fs;
use std::process;
use std::time;

fn main() -> int {
    // Dateien: alles ueber Result
    fs::write_text("demo.txt", "Hallo\n")?;
    let text = fs::read_text("demo.txt")?;
    print(text);
    fs::remove_file("demo.txt")?;

    // Prozesse: Programm und Argumente getrennt, keine Shell
    let out = process::run("cmd.exe", ["/C", "echo", "fertig"])?;
    println(out.stdout);
    println("exit: " + str(out.exit_code));

    // Zeit
    println(time::format_utc(time::unix_seconds()));
    let t0 = time::monotonic_millis();
    time::sleep_ms(10);
    println("gedauert (ms): " + str(time::monotonic_millis() - t0));
    return 0;
}
```

Beispiele: `examples/file_io.blm`, `examples/process.blm`,
`examples/time.blm`.

## 11h. Quellcode formatieren

```bash
bloom fmt src/main.blm      # formatiert (sicherer Schreibvorgang)
bloom fmt src/              # alle .blm-Dateien rekursiv
bloom fmt --check src/      # nur pruefen; Exit-Code 1 bei Abweichungen
```

Der Formatter erhaelt alle Kommentare und Literale, ist idempotent und
schreibt bei Syntaxfehlern nichts.

## 11i. FFI, Windows-API, Netzwerk, HTTP und TLS

```bloom
extern "system" {
    fn GetCurrentProcessId() -> c_uint;
}

fn main() -> int {
    unsafe {
        println(str(int(GetCurrentProcessId())));   // FFI nur in unsafe
    }
    return 0;
}
```

```bloom
use std::win32;
use std::net;
use std::http;

fn main() -> int {
    println(str(win32::process_id()));

    let listener = match net::tcp_listen("127.0.0.1", 0, 4) {
        Result::Ok(h) => h,
        Result::Err(e) => return 1
    };
    println(str(match net::local_port(listener) {
        Result::Ok(p) => p,
        Result::Err(e) => 0
    }));

    match http::get("http://127.0.0.1:8080/") {
        Result::Ok(r) => println(str(r.status) + " " + r.body),
        Result::Err(e) => println(e)
    };
    return 0;
}
```

Bibliotheken linken: `bloom run --link user32 prog.blm` oder in
`bloom.toml` unter `[build] link_libraries = ["user32"]`.

Beispiele: `examples/ffi_basic.blm`, `examples/win32_info.blm`,
`examples/tcp_client.blm`, `examples/tcp_server.blm`,
`examples/http_get.blm`, `examples/http_server.blm`.

### HTTPS mit `std.tls` (Windows)

```bloom
use std::http;

fn main() -> int {
    // System-Trust (Windows-Zertifikatsspeicher); das Zertifikat wird
    // immer geprüft, es gibt keinen unsicheren Modus.
    match http::get("https://example.com/") {
        Result::Ok(r) => println(str(r.status) + " " + r.body),
        Result::Err(e) => println("Fehler: " + e)
    };

    // Eigene CA-Datei (PEM oder DER, erstes Zertifikat):
    match http::get_with_ca("https://127.0.0.1:8443/", "meine-ca.der") {
        Result::Ok(r) => println(str(r.status)),
        Result::Err(e) => println("Fehler: " + e)
    };
    return 0;
}
```

Roh-TLS ohne HTTP geht über `use std::tls;`:
`tls_connect`, `tls_connect_with_ca`, `tls_connect_ex`,
`tls_read`, `tls_write_all`, `tls_close`, `tls_set_timeout`.
Chunked-Antworten (`Transfer-Encoding: chunked`) und -Anfragen werden
automatisch dekodiert. Beispiel: `examples/https_client.blm`.

### JSON und Pfade (`std.json`, `std.path`)

```bloom
use std::json;
use std::path;

fn main() -> int {
    // JSON parsen, Felder lesen, hübsch ausgeben
    let doc = match json::parse("{\"name\": \"Ada\", \"age\": 36}") {
        Result::Ok(v) => v,
        Result::Err(e) => {
            println("FEHLER: " + e);
            return 1;
        }
    };
    println(json::get_string(doc, "name"));
    match json::stringify_pretty(doc) {
        Result::Ok(text) => println(text),
        Result::Err(e) => println("FEHLER: " + e)
    };

    // Neue Werte konstruieren (das Handle wird automatisch freigegeben)
    let cfg = json::make_object();
    json::set(cfg, "debug", json::make_bool(true));
    let list = json::make_array();
    json::push(list, json::make_string("a"));
    json::set(cfg, "paths", list);
    match json::stringify(cfg) {
        Result::Ok(text) => println(text),
        Result::Err(e) => println("FEHLER: " + e)
    };

    // Pfade: rein lexikalisch, ohne Dateisystemzugriff
    let file = path::join("C:\\projekte", "src\\main.blm");
    println(file);
    match path::normalize("C:\\projekte\\src\\..\\tests") {
        Result::Ok(v) => println(v),
        Result::Err(e) => println("FEHLER: " + e)
    };
    return 0;
}
```

`std::json` arbeitet mit referenzgezählten Werten: Kopieren hält sie am
Leben, der Gültigkeitsbereich gibt sie frei — kein manuelles Aufräumen.
Fehler beim Parsen enthalten Byte-Position, Zeile und Spalte.
`std::path` löst nur lexikalisch auf (keine Symlinks, keine
Existenzprüfung); Dateisystemzugriffe laufen weiter über `std::fs`.
Beispiele: `examples/json_parse.blm`, `examples/json_create.blm`,
`examples/json_pretty.blm`, `examples/json_file_config.blm`,
`examples/path_operations.blm`.

### HTML und Templates (`std.html`)

```bloom
use std::html;
use std::json;

fn main() -> int {
    println(html::escape("<b>Tom & Jerry</b>"));

    let data = json::make_object();
    json::set(data, "title", json::make_string("Benutzer"));
    let users = json::make_array();
    let a = json::make_object();
    json::set(a, "name", json::make_string("Ada <admin>"));
    json::push(users, a);
    json::set(data, "users", users);

    let src = "<h1>{{title}}</h1><ul>{{#each users}}<li>{{this.name}}</li>{{/each}}</ul>";
    match html::template(src) {
        Result::Ok(t) => match html::render(t, data) {
            Result::Ok(page) => println(page),
            Result::Err(e) => println("FEHLER: " + e)
        },
        Result::Err(e) => println("FEHLER: " + e)
    };
    return 0;
}
```

Platzhalter werden **automatisch HTML-escaped**; `{{#if …}}`/`{{else}}`
und `{{#each …}}` (mit `{{this}}`) ergänzen Bedingungen und Schleifen.
Fehlende Werte, `null`, Arrays/Objekte sowie kaputte Vorlagen sind
kontrollierte Fehler; `<script>`/`<style>` werden abgelehnt. Escaping
schützt HTML-Text und quotierte Attribute, **nicht** JavaScript-,
CSS- oder URL-Kontexte. Über HTTP senden: `html::render_page(conn, 200,
t, data)` (setzt `Content-Type: text/html; charset=utf-8`).
Beispiele: `examples/html_escape.blm`, `examples/template_basic.blm`,
`examples/template_conditions.blm`, `examples/template_lists.blm`,
`examples/template_errors.blm`, `examples/web_html_page.blm`.

## 12. Module und mehrere Dateien

`src/main.blm`:

```bloom
use shapes;

fn main() -> int {
    let r = shapes::Rectangle { width: 3, height: 4 };
    println(shapes::area(r));
    return 0;
}
```

`src/shapes.blm`:

```bloom
pub struct Rectangle {
    width: int,
    height: int
}

impl Rectangle {
    pub fn area(self) -> int {
        return self.width * self.height;
    }
}

pub fn area(r: Rectangle) -> int {
    return r.area();
}
```

- `use shapes;` lädt `shapes.blm` (oder `shapes/mod.blm`) relativ zur
  importierenden Datei, sonst relativ zum Einstiegsverzeichnis.
- `use shapes::Rectangle;` importiert ein einzelnes Symbol.
- Nur `pub`-Symbole sind außerhalb ihres Moduls sichtbar.
- `pub use geometry::Point;` re-exportiert ein Symbol aus einem anderen
  Modul, sodass andere es über dieses Modul importieren können.
- Vollständiges Projekt: `examples/modules_project/` (mit `bloom.toml`).

## 13. Nächste Schritte

- `docs/LANGUAGE_SPEC.md` – alle Sprachregeln im Detail
- `docs/MEMORY_MODEL.md` – Speicherverwaltung heute und Empfehlung
- `examples/` – lauffähige Programme (Schleifen, Arrays, Strings, Generics …)
- `docs/COMPILER_ARCHITECTURE.md` – wie der Compiler intern arbeitet

## Fehlersuche

**„error[E3002]: kein C-Compiler gefunden“**
Kein MSVC/Clang/GCC/tcc gefunden. Compiler installieren (siehe Schritt 2) oder
`BLOOM_CC` setzen.

**„error[E3003]: native Kompilierung fehlgeschlagen“**
Der C-Compiler wurde aufgerufen, hat aber Fehler gemeldet. Die Ausgabe des
C-Compilers wird mit angezeigt. Handelt es sich um einen Compilerfehler in
generiertem Code, ist das ein Bloom-Fehler und sollte gemeldet werden.

**Umlaute/Kyrillisch erscheinen falsch (Konsole)**
Die Bloom-Laufzeit stellt die Windows-Konsole auf UTF-8 (CP 65001) um. In sehr
alten Terminals kann die Darstellung trotzdem eingeschränkt sein; die Bytes im
Programm sind immer UTF-8.

**„cannot find `x` in this scope“**
Variablen müssen vor ihrer Verwendung deklariert werden. Funktionen dürfen
dagegen an beliebiger Stelle im File stehen und sich gegenseitig aufrufen.
