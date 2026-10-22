# Erste Schritte mit Bloom

Hier geht es von einem leeren Rechner bis zum laufenden Programm.
Wenn etwas nicht klappt, schau am Ende in die Fehlersuche. Die Beispiele
sind alle aus `examples/` und laufen wirklich, du kannst sie also
kopieren und rumprobieren.

## 1. Was du brauchst

Zwei Dinge:

1. **Rust**, um den Compiler zu bauen.
   Windows: <https://www.rust-lang.org/tools/install>, einfach
   `rustup-init.exe` starten. Linux/macOS:
   `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`.
2. **Einen C-Compiler**, weil Bloom nach C übersetzt und den Rest dem
   C-Compiler überlässt.

Auf Windows reicht eins davon:

- **Visual Studio Build Tools** mit „Desktop development with C++“
  (<https://visualstudio.microsoft.com/visual-cpp-build-tools/>).
  Bloom findet das über `vswhere.exe` von allein.
- **Clang** (<https://releases.llvm.org/>), `clang.exe` muss im PATH
  liegen.
- **MinGW-w64 GCC**, z. B. von <https://winlibs.com/>.
- **tcc** (<https://bellard.org/tcc/>), klein und schnell, aber nicht
  bei jedem Kram dabei.

Auf Linux/macOS tut es `gcc` oder `clang`.

Kurz prüfen:

```bash
rustc --version
cargo --version
cl
clang --version
gcc --version
```

Bloom probiert der Reihe nach `BLOOM_CC`, MSVC, Clang, GCC und tcc.
Wenn du einen bestimmten willst:

```bat
set BLOOM_CC=clang
set BLOOM_CC=C:\pfad\zu\cl.exe
```

```bash
export BLOOM_CC=clang
```

## 2. Compiler bauen

```bash
cd Bloom
cargo build --release
```

Danach liegt der Compiler unter `target\release\bloom.exe`. Wenn du
willst, pack `target\release` in den PATH, dann geht `bloom` überall.

## 3. Hallo Welt

`hello.blm`:

```bloom
fn main() -> int {
    println("Hello from Bloom!");
    return 0;
}
```

```bash
bloom run hello.blm
```

```text
Hello from Bloom!
```

Was dabei passiert: Bloom liest den Quelltext, prüft Typen, schreibt
C-Code, ruft den C-Compiler auf und startet am Ende die `.exe`. Das
dauert beim ersten Mal ein oder zwei Sekunden, danach merkt man es
kaum.

## 4. Bauen und prüfen

```bash
bloom build hello.blm                  # hello.exe neben der Quelldatei
bloom build hello.blm -o C:\tmp\hi.exe
bloom build hello.blm --emit-c build\hello.c   # C-Code angucken
bloom check hello.blm                  # nur prüfen, nichts bauen
```

`check` ist praktisch, wenn du nur wissen willst, ob alles stimmt:

```text
error[E1002]: incompatible types: expected `int`, found `string`
  --> main.blm:2:20
  |
2 |     let age: int = "hello";
  |                    ^^^^^^^ expected `int`, found `string`
  |
  = help: the declared type is `int`
```

## 5. Ein Projekt anlegen

```bash
bloom init HelloWorld
cd HelloWorld
bloom run
```

Das legt das hier an:

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

Im Projektordner findet `bloom run` und `bloom build` den Einstieg
selbst. Gebaut wird nach `build/HelloWorld.exe`.

## 6. Variablen und Schleifen

```bloom
fn main() -> int {
    let name = "Bloom";     // let = nicht änderbar
    var counter = 0;        // var = änderbar

    while counter < 3 {
        println(name + " " + str(counter));
        counter = counter + 1;
    }

    for i in 1..4 {         // 1, 2, 3 (Ende exklusiv)
        if i == 2 {
            continue;
        }
        println(str(i));
    }
    return 0;
}
```

`str(...)` macht aus Zahlen und Booleans Text, damit man sie mit
Strings zusammenkleben kann. Mehr Beispiele: `examples/variables.blm`,
`examples/loops.blm`.

## 7. Structs

```bloom
struct Point {
    x: int,
    y: int
}

fn main() -> int {
    var p = Point { x: 10, y: 20 };
    println(str(p.x));
    p.x = 100;
    println(str(p.x));
    return 0;
}
```

Ein paar Regeln, die immer wieder auftauchen:

- Beim Erzeugen müssen alle Felder genau einmal angegeben werden.
- Felder schreibst du nur, wenn die Variable `var` ist.
- Structs werden kopiert. `var b = a;` gibt dir eine unabhängige Kopie.
- Structs dürfen andere Structs enthalten, aber nicht sich selbst.

Beispiel: `examples/structs.blm`.

## 8. Methoden

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
}

fn main() -> int {
    var c = Counter { n: 0 };
    c.inc();
    c.inc();
    println(str(c.get()));   // 2
    return 0;
}
```

`self` liest nur, `var self` darf den Empfänger verändern. Für
`var self` brauchst du eine `var`-Variable, auf einer `let`-Variable
oder einem Zwischenergebnis geht es nicht. Beispiel:
`examples/methods.blm`.

## 9. Enums und match

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
```

`match` muss alle Varianten abdecken, sonst meckert der Compiler
(E1041). Guards und Oder-Patterns gehen auch:

```bloom
fn grade(score: int) -> string {
    return match score {
        0 => "zero",
        n if n >= 90 => "A",
        n if n >= 50 => "B",
        _ => "F"
    };
}

fn classify(n: int) -> string {
    return match n {
        1 | 2 | 3 => "small",
        4 | 5 => "medium",
        _ => "large"
    };
}
```

`Option` und `Result` sind eingebaute Enums:

```bloom
fn divide(a: int, b: int) -> Result<int, string> {
    if b == 0 {
        return Result::Err("division by zero");
    }
    return Result::Ok(a / b);
}

fn half(a: int) -> Result<int, string> {
    let q = divide(a, 2)?;    // bei Err sofort weiterreichen
    return Result::Ok(q);
}
```

Beispiele: `examples/enums.blm`, `examples/option_result.blm`,
`examples/result_try.blm`.

## 10. Generics

```bloom
struct Box<T> {
    value: T
}

fn identity<T>(value: T) -> T {
    return value;
}

fn main() -> int {
    let b = Box { value: 42 };    // T wird erkannt
    println(str(b.value));
    println(identity("text"));    // T = string
    println(str(identity::<int>(7)));  // explizit, wenn es nicht klar ist
    return 0;
}
```

Läuft wie in Rust, nur ohne den ganzen Trait-Kram. Beispiele:
`examples/generics.blm`, `examples/enum_methods.blm`.

## 11. Strings und Arrays

```bloom
fn main() -> int {
    let s = "  Hello, World  ";
    let clean = trim(s);

    println(str(contains(clean, "World")));
    println(str(starts_with(clean, "Hello")));
    println(replace(clean, "World", "Bloom"));
    println(to_upper(clean));

    let words = split(clean, ", ");
    println(words[1]);

    var xs: [int] = [1, 2, 3];
    push(xs, 4);
    let last = pop(xs);         // Option<int>
    insert(xs, 0, 10);
    let removed = remove(xs, 1); // Option<int>
    let part = slice(xs, 0, 2);  // Kopie
    println(join(words, " | "));
    return 0;
}
```

Arrays haben `push`, `pop`, `insert`, `remove` und `slice`. `slice`
kopiert, `slice_view` nicht (siehe unten). Beispiele:
`examples/strings.blm`, `examples/arrays.blm`,
`examples/collections.blm`.

## 12. Speicher

Strings, Arrays und alles, was die drin hat, wird per
**Referenzzählung** verwaltet. Du musst nichts freigeben und nichts
kopieren, der Compiler hängt an jedem Ausgang aus einem Block die
passenden Zähler-Anpassungen dran. Auch beim `?`-Fehlerpfad.

Prüfen kannst du das so:

```bash
set BLOOM_MEM_REPORT=1
bloom run examples/strings.blm
```

Am Ende steht auf stderr `live allocations: 0`. Steht da was anderes,
ist irgendwo ein Leck. Details und Grenzen stehen in
`docs/MEMORY_MODEL.md`.

Slice-Views sind der einzige Ort, an dem man aufpassen muss:

```bloom
var values = [10, 20, 30, 40, 50];
let view = slice_view(values, 1, 4);   // 20, 30, 40 (ohne Kopie)
println(str(len(view)));
println(str(view[1]));

values[2] = 99;      // sieht man durch den View
println(str(view[1]));

var writable = slice_view(values, 0, 3);
writable[0] = 7;     // schreibt ins Original
```

`push` macht Views nicht kaputt. `insert`, `remove` und ein
erfolgreiches `pop` invalidieren sie aber, danach gibt es einen
Laufzeitfehler statt eines Absturzes. Beispiel: `examples/slices.blm`.

## 13. Dateien, Prozesse, Zeit

```bloom
use std::fs;
use std::process;
use std::time;

fn run() -> Result<int, string> {
    fs::write_text("demo.txt", "Hallo\n")?;
    print(fs::read_text("demo.txt")?);
    fs::remove_file("demo.txt")?;

    // Programm und Argumente getrennt, keine Shell dazwischen
    let out = process::run("cmd.exe", ["/C", "echo", "fertig"])?;
    println(out.stdout);

    println(time::format_utc(time::unix_seconds()));
    let t0 = time::monotonic_millis();
    time::sleep_ms(10);
    println(str(time::monotonic_millis() - t0));
    return Result::Ok(0);
}

fn fail(e: string) -> int {
    println("FEHLER: " + e);
    return 1;
}

fn main() -> int {
    return match run() {
        Result::Ok(code) => code,
        Result::Err(e) => fail(e)
    };
}
```

`?` geht nur in Funktionen, die selbst `Result` zurückgeben. `main`
muss `int` liefern, deshalb steckt die eigentliche Arbeit in `run()`.
In den nächsten Abschnitten lasse ich `main` weg, damit es nicht
ständig gleich aussieht.

Alles, was schiefgehen kann, kommt als `Result` zurück, es gibt keine
Exceptions. Beispiele: `examples/file_io.blm`,
`examples/process.blm`, `examples/time.blm`.

## 14. JSON und Pfade

```bloom
use std::json;
use std::path;

fn run() -> Result<int, string> {
    let doc = json::parse("{\"name\": \"Ada\", \"age\": 36}")?;
    println(json::get_string(doc, "name")?);

    let cfg = json::make_object();
    json::set(cfg, "debug", json::make_bool(true));
    println(json::stringify(cfg)?);

    println(path::join("C:\\projekte", "src\\main.blm"));
    return Result::Ok(0);
}
```

JSON-Werte werden wie alles andere automatisch freigegeben. `std::path`
arbeitet nur mit dem Text des Pfads, es schaut nicht auf die Platte.
Echte Dateizugriffe laufen über `std::fs`. Beispiele:
`examples/json_parse.blm`, `examples/json_create.blm`,
`examples/path_operations.blm`.

## 15. HTML-Vorlagen

```bloom
use std::html;
use std::json;

fn run() -> Result<int, string> {
    let data = json::make_object();
    json::set(data, "title", json::make_string("Benutzer"));

    let users = json::make_array();
    let a = json::make_object();
    json::set(a, "name", json::make_string("Ada <admin>"));
    json::push(users, a);
    json::set(data, "users", users);

    let src = "<h1>{{title}}</h1><ul>{{#each users}}<li>{{this.name}}</li>{{/each}}</ul>";
    let t = html::template(src)?;
    println(html::render(t, data)?);
    return Result::Ok(0);
}
```

`{{...}}` wird automatisch escaped, `{{#if}}`/`{{else}}` und
`{{#each}}` gibt es auch. Was nicht passt (fehlender Wert, `null`,
Array da wo Text hin soll), ist ein Fehler und kein stiller leerer
String. `<script>` und `<style>` lehnt der Parser ab.

Seit 0.15 kannst du Vorlagen auch aus Dateien laden:

```bloom
let t = html::template_file("templates/seite.html")?;          // direkt
let t2 = html::template_file_from("templates", "seite.html")?; // mit Wurzelordner
```

Die Datei darf maximal 64 KiB groß und muss gültiges UTF-8 sein.
`template_file_from` nimmt einen relativen Pfad, verhindert `..` und
prüft nach dem Öffnen, ob die Datei wirklich im Ordner liegt. Auch
wenn ein Symlink oder eine Junction woanders hinzeigt: gibt Ärger.

Über HTTP schicken: `html::render_page(conn, 200, t, data)`.

## 16. HTTP, Web-Routing und statische Dateien

Kleiner Server, der JSON zurückgibt:

```bloom
use std::web;
use std::http;
use std::net;

fn run() -> Result<int, string> {
    let router = web::router();
    web::route(router, "GET", "/users/:id", "user")?;

    let listener = net::tcp_listen("127.0.0.1", 8080, 8)?;
    let conn = net::tcp_accept(listener)?;
    let request = http::read_request(conn)?;

    let m = web::resolve_request(router, request)?;
    if m.name == "user" {
        web::json_response(conn, 200, m.params)?;
    } else {
        http::write_response(conn, 404, "text/plain; charset=utf-8", "nope")?;
    }
    net::tcp_close(conn);
    return Result::Ok(0);
}
```

Dateien aus einem Ordner ausliefern geht ohne Router:

```bloom
match web::static_file(conn, "public", request) {
    Result::Ok(status) => println("gesendet: " + str(status)),
    Result::Err(e) => println("Schreibfehler: " + e)
};
```

`static_file` macht GET und HEAD, rät den Content-Type aus der Endung,
schickt `nosniff` mit und antwortet mit 404 wenn die Datei fehlt, 400
wenn jemand mit `..` oder Symlinks aus dem Ordner rauswill und 500 bei
Lesefehlern oder Dateien über 8 MiB.

Wenn du Binärdaten schicken willst (Bilder, ZIPs, whatever):

```bloom
use std::fs;
use std::http;

fn send_logo(conn: int) -> Result<bool, string> {
    let bytes = fs::read_bytes("logo.png")?;
    return http::write_response_bytes(conn, 200, "image/png", "", bytes);
}
```

`write_response_bytes_head` macht das Gleiche, lässt aber den Body weg,
für HEAD-Requests.

Beispiele: `examples/web_router.blm`, `examples/web_json_api.blm`,
`examples/web_static_files.blm`, `examples/web_html_site.blm`.

## 16a. Formulare

Formulardaten kommen als `application/x-www-form-urlencoded` an, also
Text wie `q=bloom+forms&lang=de&lang=en`.

```bloom
use std::web;

fn run() -> Result<int, string> {
    let form = web::form_body("q=bloom+forms&lang=de&lang=en")?;

    // Ein Wert, der genau einmal vorkommt
    let q = web::form_get(form, "q")?;
    println("Suche: " + q);

    // Mehrere Werte (Checkbox-Gruppe)
    let languages = web::form_values(form, "lang");
    println("Sprachen: " + join(languages, ", "));

    // Erster Wert oder None
    match web::form_first(form, "q") {
        Option::Some(v) => println("erster Treffer: " + v),
        Option::None => println("kein q")
    };

    // Existiert das Feld überhaupt?
    println(str(web::form_has(form, "q")));
    println(str(web::form_count(form)));
    return Result::Ok(0);
}
```

Wichtig beim Auslesen:

- `form_get` liefert `Err`, wenn das Feld fehlt **oder** mehrfach
  vorkommt. Wer den ersten von mehreren Werten will, nimmt
  `form_first` und sagt damit, was er meint.
- Ein vorhandenes Feld mit leerem Wert (`leer=`) ist `Ok("")`, ein
  fehlendes Feld ist `Err`. Das ist ein Unterschied.
- `form_values` gibt alle Werte in Request-Reihenfolge zurück,
  `form_names` die verschiedenen Namen in der Reihenfolge des ersten
  Auftretens.

In einem Server nimmst du `form_body_request`, dann prüft Bloom auch
den Content-Type:

```bloom
match web::form_body_request(request) {
    Result::Ok(form) => handle(form),
    Result::Err(e) => send_error(conn, 400, e)
};
```

Um Formulardaten wieder zu verschicken oder zu speichern, gibt es
`web::form_encode`. Es kodiert Leerzeichen als `+` und alles andere
Nötige als `%XX`; Parse → Encode → Parse ergibt dieselben Einträge in
derselben Reihenfolge.

Limits: Body 1 MiB, 4096 Einträge, Name 256 Bytes, Wert 64 KiB.
Ungültige Prozentkodierung wie `%`, `%0` oder `%GG` und ungültiges
UTF-8 sind Fehler. Der Parser repariert nichts stillschweigend.

Und ganz wichtig: Dekodieren ist **keine** Validierung und keine
Autorisierung. Formularwerte sind Eingaben von außen. Prüfe sie selbst,
und lass sie durch die Template-Engine escapen, wenn du sie ausgibst.
CSRF-Schutz, Sessions und Login musst du selbst bauen.

Beispiele: `examples/form_parse.blm`,
`examples/form_multiple_values.blm`, `examples/form_encode.blm`,
`examples/web_form_server.blm`, `examples/web_form_errors.blm`.

## 16b. Cookies, Sessions und CSRF

Cookies lesen:

```bloom
match web::cookies(request) {
    Result::Ok(cookies) => println("Cookies: " + join(web::cookie_names(cookies), ", ")),
    Result::Err(e) => println("Cookie-Fehler: " + e)
};
```

`cookie_get` gibt `Err`, wenn ein Cookie fehlt oder mehrfach vorkommt.
Mehrfach ist Absicht: zwei Cookies mit demselben Namen sind mehrdeutig
und werden nicht stillschweigend zu einem Wert zusammengefasst.
`cookie_first` nimmt bewusst den ersten.

Cookie setzen:

```bloom
let options = web::CookieOptions {
    path: "/",
    max_age: 3600,
    same_site: "Lax",
    http_only: true,
    insecure_http: false   // false = Secure ist gesetzt
};
let line = web::set_cookie("theme", "dark", options)?;
// line ist "Set-Cookie: theme=dark; Path=/; Max-Age=3600; HttpOnly; Secure; SameSite=Lax"
```

`insecure_http: true` lässt `Secure` weg und ist nur für lokale
HTTP-Entwicklung gedacht. Löschen geht mit `delete_cookie(name, path,
insecure_http)` und muss denselben Pfad verwenden wie beim Setzen.

Sessions:

```bloom
fn run() -> Result<int, string> {
    let store = web::session_store(1000, 3600)?;  // max. 1000 Sessions, 1 Stunde
    let session = web::session_new(store)?;
    let id = web::session_id(session);
    println(id);                                   // 43 Zeichen Base64url

    // Später, aus einer Anfrage:
    let cookies = web::cookies(request)?;
    let resumed = web::session_load(store, cookies, "sid")?;
    let data = web::session_data(resumed);
    json::set(data, "user", json::make_string("ada"))?;
    println(json::get_string(data, "user")?);
    return Result::Ok(0);
}
```

Die ID kommt aus 32 echten Zufallsbytes (Windows `BCryptGenRandom`,
POSIX `/dev/urandom`). Geht die Zufallsquelle nicht, gibt es einen
Fehler, keinen schwächeren Ersatz. `session_load` scheitert bei
fehlenden, kaputten, unbekannten und abgelaufenen IDs; wenn du dann
eine neue Session willst, rufst du selbst `session_new` auf. So weißt
du immer, ob eine Session wiederverwendet wurde.

Sessions leben nur im Speicher und sind nach einem Neustart weg.
Ablauf ist absolut (nicht gleitend) und wird mit der monotonen Uhr
berechnet. Ist der Store voll, schlägt `session_new` fehl, statt eine
aktive Session zu verdrängen.

Den Cookie für die Session baut `session_cookie(session, "sid", 3600)`
(Secure, HttpOnly, SameSite=Lax, Path=/). Für HTTP-Entwicklung gibt es
`session_cookie_insecure`.

CSRF:

```bloom
let token = web::csrf_token(session)?;   // im Formular als hidden field
// beim Speichern:
if !web::csrf_check(session, form_token) {
    // 403
}
web::csrf_rotate(session);               // nach Erfolg neuen Token
```

Der Token ist 43 Zeichen Zufall, gehört zu genau dieser Session und
wird in konstanter Zeit verglichen. `csrf_check` ist **keine**
Anmeldung und schützt nur, wenn du es bei jedem verändernden Request
selbst aufrufst. `SameSite` hilft zusätzlich, ersetzt die Prüfung aber
nicht. Mehrere `Set-Cookie`-Header schickst du mit
`web::cookie_headers([...])` oder als einzelne Zeilen im Header-Block
von `write_response_full`.

Plattformen: Getestet ist die Zufallsquelle unter Windows. Der
POSIX-Pfad (`/dev/urandom`) ist implementiert, aber in diesem Repo
nicht getestet.

Beispiele: `examples/web_cookies.blm`, `examples/web_session_basic.blm`,
`examples/web_session_expiry.blm`, `examples/web_csrf_form.blm`,
`examples/web_session_errors.blm`.

## 17. HTTPS

```bloom
use std::http;

fn main() -> int {
    match http::get("https://example.com/") {
        Result::Ok(r) => println(str(r.status)),
        Result::Err(e) => println("Fehler: " + e)
    };
    return 0;
}
```

Das Zertifikat wird immer geprüft, einen „unsicher“-Schalter gibt es
nicht. Für eine eigene CA: `http::get_with_ca(url, "meine-ca.der")`.
Roh-TLS ohne HTTP geht über `use std::tls;` mit `tls_connect`,
`tls_read`, `tls_write_all` und so weiter. Beispiel:
`examples/https_client.blm`.

## 18. Module

`src/main.blm`:

```bloom
use shapes;

fn main() -> int {
    let r = shapes::Rectangle { width: 3, height: 4 };
    println(str(shapes::area(r)));
    return 0;
}
```

`src/shapes.blm`:

```bloom
pub struct Rectangle {
    width: int,
    height: int
}

pub fn area(r: Rectangle) -> int {
    return r.width * r.height;
}
```

`use shapes;` sucht `shapes.blm` (oder `shapes/mod.blm`) neben der
Datei, die importiert. Nur `pub`-Sachen sieht man von außen.
`pub use` re-exportiert Symbole weiter. Volles Beispiel:
`examples/modules_project/`.

## 19. C und Windows

```bloom
extern "system" {
    fn GetCurrentProcessId() -> c_uint;
}

fn main() -> int {
    unsafe {
        println(str(int(GetCurrentProcessId())));
    }
    return 0;
}
```

Externe Aufrufe gehen nur in `unsafe`-Blöcken. Bibliotheken hängst du
mit `--link` an oder in `bloom.toml`:

```toml
[build]
link_libraries = ["user32"]
```

Ein paar Windows-Funktionen sind schon fertig eingepackt:
`use std::win32;` mit `process_id`, `message_box`, `get_computer_name`
und so weiter. Beispiele: `examples/ffi_basic.blm`,
`examples/win32_info.blm`.

## 20. Formatieren

```bash
bloom fmt src/main.blm    # eine Datei
bloom fmt src/            # alles rekursiv
bloom fmt --check src/    # nur prüfen, Exit 1 bei Unterschieden
```

Der Formatter lässt Kommentare und Strings in Ruhe, ist idempotent und
schreibt bei Syntaxfehlern gar nichts.

## Fehlersuche

**`error[E3002]: kein C-Compiler gefunden`**
Kein MSVC/Clang/GCC/tcc gefunden. Installieren (Schritt 1) oder
`BLOOM_CC` setzen.

**`error[E3003]: native Kompilierung fehlgeschlagen`**
Der C-Compiler hat gemeckert. Seine Ausgabe steht direkt darunter. Wenn
der Fehler im generierten C-Code liegt, ist das ein Bloom-Bug und
gehört gemeldet.

**Umlaute sehen in der Konsole komisch aus**
Bloom stellt die Windows-Konsole auf UTF-8 um. In alten Terminals
bleibt es manchmal trotzdem kaputt, im Programm selbst sind die Bytes
aber immer UTF-8.

**`cannot find 'x' in this scope`**
Variablen müssen vor der Benutzung deklariert sein. Funktionen dürfen
dagegen überall in der Datei stehen.

**Der `?`-Operator funktioniert nicht**
Die Funktion muss dann selbst `Result<...>` oder `Option<...>`
zurückgeben. In `fn main() -> int` geht `?` nicht.
