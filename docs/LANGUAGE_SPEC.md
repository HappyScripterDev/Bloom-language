# Bloom Sprachspezifikation

Diese Datei beschreibt die Sprache so, wie der Compiler sie wirklich
versteht. Was hier nicht steht, gibt es nicht. Wenn du wissen willst, was
noch geplant ist, schau in `docs/ROADMAP.md`.

Quelldateien enden auf `.blm` und sind UTF-8 (ein BOM am Anfang stört
nicht).

## 1. Lexikalische Struktur

### 1.1 Kommentare

```bloom
// Zeilenkommentar bis Zeilenende
/* Blockkommentar (nicht verschachtelt) */
```

### 1.2 Bezeichner

Ein Bezeichner beginnt mit einem Unicode-Buchstaben oder `_` und besteht aus
Unicode-Buchstaben, Ziffern und `_`. Bezeichner mit doppeltem Unterstrich
(`__`) sind für die Codegenerierung reserviert und werden abgelehnt (E1056).

### 1.3 Schlüsselwörter

```text
fn  let  var  const  if  else  while  for  in  return  break  continue
true  false  struct  enum  impl  pub  use  match  self
```

`int`, `float`, `bool`, `string`, `char` sind keine Schlüsselwörter, sondern
Typnamen und eingebaute Konvertierungsfunktionen. `class` und `import` werden
erkannt, aber als nicht unterstützt gemeldet.

### 1.4 Zahl-Literale

- Ganzzahlen dezimal (`42`), hexadezimal (`0xFF`), binär (`0b1010`), oktal (`0o17`)
- Unterstriche als Zifferntrenner (`1_000_000`)
- Gleitkommazahlen: `1.5`, `2.0`, `1.5e-3`, `2E6`
- 64-Bit (`int`) bzw. IEEE-754-Doppel (`float`); Literale außerhalb des
  Wertebereichs sind Fehler (E0003)
- Hinweis: `-9223372036854775808` kann nicht als Literal geschrieben werden;
  schreibe stattdessen `-9223372036854775807 - 1`

### 1.5 String- und Zeichen-Literale

```bloom
"Hello\nWorld"
'A'
'\u{2603}'
```

Escape-Sequenzen: `\n` `\t` `\r` `\0` `\\` `\"` `\'` `\u{HHHHHH}` (1–6
Hexziffern, gültiger Unicode-Skalarwert). Strings dürfen nicht über Zeilen
hinweg gehen.

### 1.6 Operatoren und Satzzeichen

```text
( ) { } [ ] , ; : :: . -> .. => 
+ - * / % = == != ! < <= > >= && ||
```

## 2. Grammatik (EBNF)

```ebnf
program      = { item } ;
item         = function | global_decl | struct_decl | enum_decl | impl_block | use_decl ;
use_decl     = "use" path ";" ;
path         = IDENT { "::" IDENT } ;
struct_decl  = [ "pub" ] "struct" IDENT "{" struct_field { "," struct_field } [ "," ] "}" ;
struct_field = IDENT ":" type ;
enum_decl    = [ "pub" ] "enum" IDENT [ "<" IDENT { "," IDENT } ">" ] "{" variant { "," variant } [ "," ] "}" ;
variant      = IDENT [ "(" type { "," type } ")" | "{" struct_field { "," struct_field } [ "," ] "}" ] ;
impl_block   = "impl" path "{" { method } "}" ;
method       = [ "pub" ] "fn" IDENT "(" receiver [ "," params ] ")" [ "->" type ] block ;
receiver     = "self" | "var" "self" ;
function     = [ "pub" ] "fn" IDENT "(" [ params ] ")" [ "->" type ] block ;
params       = param { "," param } ;
param        = IDENT ":" type ;
type         = "int" | "float" | "bool" | "string" | "char" | "void"
             | path [ "<" type { "," type } ">" ]
             | "[" type "]" ;
global_decl  = ( "let" | "var" | "const" ) IDENT [ ":" type ] "=" expr ";" ;
block        = "{" { statement } "}" ;
statement    = var_decl
             | "if" expr block [ "else" ( if_statement | block ) ]
             | "while" expr block
             | "for" IDENT "in" expr ".." expr block
             | "return" [ expr ] ";"
             | "break" ";"
             | "continue" ";"
             | block
             | expr ";" ;
if_statement = "if" expr block [ "else" ( if_statement | block ) ] ;
var_decl     = ( "let" | "var" | "const" ) IDENT [ ":" type ] "=" expr ";" ;
expr         = assignment ;
assignment   = logic_or [ "=" assignment ] ;
logic_or     = logic_and { "||" logic_and } ;
logic_and    = equality { "&&" equality } ;
equality     = comparison { ( "==" | "!=" ) comparison } ;
comparison   = term { ( "<" | "<=" | ">" | ">=" ) term } ;
term         = factor { ( "+" | "-" ) factor } ;
factor       = unary { ( "*" | "/" | "%" ) unary } ;
unary        = ( "-" | "!" ) unary | postfix ;
postfix      = primary { call | index | field } ;
call         = "(" [ expr { "," expr } ] ")" ;
index        = "[" expr "]" ;
field        = "." IDENT ;
primary      = INT | FLOAT | STRING | CHAR | "true" | "false"
             | "self"
             | IDENT
             | path                                          (* Enum::Variant, Konstante, ... *)
             | IDENT "{" [ struct_init { "," struct_init } [ "," ] ] "}"
             | path "{" [ struct_init { "," struct_init } [ "," ] ] "}"
             | match_expr
             | "(" expr ")"
             | "[" [ expr { "," expr } [ "," ] ] "]" ;
match_expr   = "match" expr_no_struct "{" arm { "," arm } [ "," ] "}" ;
arm          = pattern "=>" expr ;
pattern      = "_" | IDENT | path [ "(" [ pattern { "," pattern } ] ")" 
             | "{" [ named_pattern { "," named_pattern } [ "," ] ] "}" ] ;
named_pattern = IDENT [ ":" pattern ] ;
struct_init  = IDENT ":" expr ;
```

Deklarationen (lokal wie global) **müssen** einen Initialisierer besitzen.
Methoden beginnen mit einem Empfänger (`self` oder `var self`).

### 2.1 Struct-Literale in Bedingungen

In den Köpfen von `if`, `while`, `for` und in `match`-Werten wird `IDENT {`
nicht als Struct-Literal gelesen. Dort muss geklammert werden:

```bloom
if (Point { x: 1, y: 2 }).x > 0 { ... }
match (Point { x: 1, y: 2 }).x { ... }
```

### 2.2 Match-Arme

`match` ist ein Ausdruck. Arme sind `Pattern => Ausdruck` und durch Kommas
getrennt. Blöcke als Arm-Körper gibt es nicht. Als Statement benötigt ein
`match` kein abschließendes `;`, wenn es die letzte Anweisung eines Blocks ist.

## 3. Typen

| Typ | Bedeutung | Backend |
| --- | --- | --- |
| `int` | 64-Bit-Ganzzahl, Zweierkomplement | `int64_t` |
| `float` | IEEE-754-Doppel | `double` |
| `bool` | `true`/`false` | `bool` |
| `string` | unveränderliche UTF-8-Bytesequenz | Laufzeitstruktur |
| `char` | Unicode-Skalarwert | `uint32_t` |
| `void` | kein Wert (nur Rückgabetyp) | `void` |
| `[T]` | dynamisches Array | Referenz auf Laufzeit-Header |
| `Slice<T>` | View auf einen Array-Bereich | Header-Referenz + Offset/Länge/Generation |
| Struct-Name (ggf. `<...>`) | benannter Werttyp | C-`struct` (Wertkopie) |
| Enum-Name (ggf. `<...>`) | Tagged Union | C-Struct mit Tag + Union |
| `T` (in generischen Deklarationen) | Typparameter | wird monomorphisiert |

Regeln:

- `T` in `[T]` und `Slice<T>` muss ein Basistyp sein. Verschachtelte Arrays,
  Arrays von Structs und Arrays von Enums sind nicht unterstützt (E1015).
- `void` nur als Rückgabetyp (E1016).
- Generische Typen gibt es für Structs und Enums (z. B. `Box<int>`,
  `Pair<int, string>`, `Option<int>`); Typparameter werden bei der
  Verwendung monomorphisiert.
- `Option<T>` und `Result<T, E>` sind eingebaute Enums:

  ```bloom
  Option<T> = Some(T) | None
  Result<T, E> = Ok(T) | Err(E)
  ```

  Sie werden wie benutzerdefinierte Enums geprüft und gematcht. Eine
  Neudefinition der Namen `Option`/`Result` ist ein Fehler.

## 4. Deklarationen und Gültigkeitsbereiche

```bloom
let name = "Bloom";       // unveränderlich
var counter: int = 0;     // veränderlich
const MAX: int = 100;     // Compile-Time-Konstante
```

- `let` ist unveränderlich (E1006), `var` veränderlich, `const` benötigt
  einen konstanten Ausdruck (E1014).
- Typinferenz: Ohne Angabe wird der Typ aus dem Initialisierer abgeleitet;
  leere Array-Literale brauchen eine Annotation.
- Shadowing in inneren Bereichen ist erlaubt; Duplikate im selben Bereich
  sind E1010.
- Funktionen, Structs und Enums dürfen vorwärts referenziert werden
  (auch über Modulgrenzen). Globale Deklarationen müssen vor ihrer
  Verwendung stehen und konstante Initialisierer haben; Struct-/Enum-Werte
  sind dort nicht erlaubt.

### Konstante Ausdrücke

Literale, Verweise auf lokale Konstanten, `-`/`!`, arithmetische/logische
Operatoren und String-Konkatenation. Keine Funktionsaufrufe, Indizierungen,
Struct-/Enum-Literale (E1014).

## 5. Structs

```bloom
struct Point {
    x: int,
    y: int
}
```

- Felder Komma-getrennt, Komma vor `}` erlaubt; mindestens ein Feld.
- Feldtypen: Basistypen, Arrays von Basistypen, andere Structs, Enums.
- Rekursive Structs (direkt oder über Enums) sind verboten (E1024).
- Struct-Literale: `Point { x: 1, y: 2 }` – alle Felder genau einmal
  (E1020 unbekannt, E1021 doppelt, E1022 fehlend).
- Feldzugriff `p.x`, auch verkettet (`o.inner.value`); unbekannte Felder
  E1020, Nicht-Structs E1023.
- Feldzuweisung `p.x = 5` nur über `var`-Wurzeln (E1006); temporäre Ziele
  sind E1017.
- **Wertsemantik**: Zuweisung, Parameter und Rückgabe kopieren den Wert.
- Array-Felder folgen der Referenzsemantik von Arrays.
- Structs können nicht mit `==` verglichen oder direkt ausgegeben werden
  (E1012/E1019); Felder einzeln vergleichen/ausgeben.

### Generische Structs

```bloom
struct Pair<A, B> {
    first: A,
    second: B
}

struct Box<T> {
    value: T
}
```

- Typparameter nach dem Namen (`<A, B>`); Felder können Typparameter,
  Basistypen, andere generische Structs/Enums und Arrays verwenden.
- Instanziierung über Annotation (`let b: Box<int> = ...;`), erwarteten Typ
  (Rückgabetyp, Parameter) oder Feldwerte im Literal
  (`Pair { first: 1, second: "one" }` → `Pair<int, string>`).
- Nicht ableitbare Parameter sind E1045; falsche Anzahl von Typargumenten
  ist E1016; rekursive Instanziierungen wie `W<W<T>>` sind E1024.
- Jede Kombination konkreter Typargumente wird einmal als eigenständiger
  C-Struct monomorphisiert; Wertsemantik bleibt erhalten.

## 6. Methoden und impl-Blöcke

```bloom
impl Point {
    fn magnitude_squared(self) -> int {
        return self.x * self.x + self.y * self.y;
    }

    fn translated(self, dx: int, dy: int) -> Point {
        return Point { x: self.x + dx, y: self.y + dy };
    }

    fn translate(var self, dx: int, dy: int) -> void {
        self.x = self.x + dx;
        self.y = self.y + dy;
    }
}
```

- `impl`-Blöcke sind nur auf der obersten Ebene erlaubt und müssen im selben
  Modul wie der Struct stehen. Mehrere `impl`-Blöcke pro Struct sind erlaubt.
- Methoden beginnen mit einem Empfänger:
  - `self` – Empfänger als **Wertkopie**, unveränderlich (nur lesen).
  - `var self` – Empfänger wird **in place** verändert; der Aufruf verlangt
    eine veränderliche Wurzel (`var`-Bindung, ggf. über Felder/Indizes).
- Aufruf: `p.magnitude_squared()`, `p.translated(1, 2)`,
  `o.inner.get()`; verkettbar (`p.translated(1, 2).magnitude_squared()`).
- Aufrufe funktionieren auf Werten, `var`-Variablen, Feldern, Funktions-
  ergebnissen (bei `self`) – nicht aber auf temporären Werten bei `var self`
  (E1032) und nicht auf unveränderlichen Bindungen (E1032).
- Methoden heißen wie Felder dürfen nicht mit Feldnamen kollidieren (E1030);
  doppelte Methoden sind E1030, unbekannte Methoden E1031.
- Methoden können andere Methoden desselben Typs aufrufen, auch vor deren
  Definition. Methoden dürfen `pub` sein (siehe Module).
- Es gibt keine Referenzempfänger (`&self`); `var self` ist die einzige
  Mutationsform und wird als C-Zeiger umgesetzt (keine benutzersichtbaren
  Referenzen, keine Borrow-Regeln).

  **Alias-Sicherheit (0.8):** Der Empfänger ist die Speicherstelle der
  aufrufenden Variablen (lokaler Slot oder Feld/Index davon). Andere
  Funktionen können auf diese Stelle nicht zugreifen (keine Closures,
  keine Referenztypen, und Structs sind als Globals verboten, E1014).
  Während eines `var self`-Aufrufs kann der Empfänger daher weder
  freigegeben noch verändert werden — das in 0.7 dokumentierte Restrisiko
  ist mit dem heutigen Typsystem **nicht konstruierbar**. Wird später
  erlaubt, Structs in Globals zu halten oder Referenztypen einzuführen,
  muss diese Analyse wiederholt werden (Guard-Test in `src/check.rs`).

### Methoden auf Enums und assoziierte Funktionen

`impl`-Blöcke sind auch für Enums erlaubt – einschließlich der eingebauten
`Option<T>`/`Result<T, E>`:

```bloom
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

let s = Shape::circle(2.0);   // assoziierte Funktion
println(str(s.area()));       // Methode
```

- **Assoziierte Funktionen** haben keinen Empfänger und werden über den Typ
  aufgerufen (`Shape::circle(2.0)`). Ein Name, der mit einer Variante
  kollidiert, ist E1030.
- Methoden mit Empfänger funktionieren wie bei Structs (`self`/`var self`).
  `var self` auf Enums wird derzeit abgelehnt (E1033).
- Generische `impl`-Blöcke: `impl<T> Box<T> { fn get(self) -> T { ... } }`.
  Typparameter des Blocks müssen zum Zieltyp passen (E1033/E1016).

## 7. Enums und Pattern Matching

### 7.1 Deklaration

```bloom
enum Direction {
    North,
    South,
    East,
    West
}

enum Message {
    Quit,
    Move { x: int, y: int },
    Write(string)
}

enum Pair<A, B> {
    Both(A, B),
    Empty
}
```

- Mindestens eine Variante; Variantennamen eindeutig (E1043).
- Nutzdaten: positionell `V(T1, T2)`, benannt `V { f: T }` oder keine.
- Erlaubte Nutzdatentypen: Basistypen, Arrays von Basistypen, Structs, andere
  Enum-Instanzen (auch generische). Rekursive Enums sind verboten (E1047).
- Vorwärtsreferenzen zwischen Enums/Structs sind erlaubt.

### 7.2 Konstruktion

```bloom
let d = Direction::North;
let m1 = Message::Quit;
let m2 = Message::Move { x: 3, y: 4 };
let m3 = Message::Write("hello");
let o: Option<int> = Option::Some(42);
let n: Option<int> = Option::None;
let r: Result<int, string> = Result::Ok(5);
```

- `Enum::Variant` erzeugt Unit-Varianten, `Enum::Variant(...)` positionelle,
  `Enum::Variant { ... }` benannte.
- Generische Enums: Typparameter werden aus Nutzdaten und/oder dem erwarteten
  Typ abgeleitet; nicht ableitbare Parameter sind E1045 (Annotation nötig).
- Fehlerfälle: unbekannte Variante (E1040), falsche Anzahl/Typen der
  Nutzdaten (E1005/E1002), falsche Feldnamen (E1020/E1021/E1022),
  Unit-Variante mit Wert bzw. Payload-Variante ohne Wert (E1044/E1005).

### 7.3 Match

```bloom
fn describe(d: Direction) -> string {
    return match d {
        Direction::North => "north",
        Direction::South => "south",
        Direction::East => "east",
        Direction::West => "west"
    };
}
```

- Alle Arme müssen denselben Typ haben (E1002).
- **Exhaustiveness** wird statisch geprüft (Matrix-Verfahren):
  - Fehlende Varianten sind E1041 mit konkreten Gegenbeispiel-Patterns
    (`Color::Blue`, `Option::Some(Option::None)`).
  - Ein Wildcard `_` oder Bindungs-Pattern (`other`) deckt den Rest ab.
  - Unerreichbare Arme sind E1042.
- Patterns: `_`, Bindungen, `Enum::Variant`, `Enum::Variant(p1, ...)`,
  `Enum::Variant { feld }` / `{ feld: pattern }`, beliebig verschachtelt.
- **Literal-Patterns**: `0`, `-3`, `true`, `'a'`, `"text"` – verglichen mit
  dem Wert; der Typ muss zum geprüften Wert passen (E1002/E1044).
  `bool`-Patterns gelten als endlich (`true` + `false` sind exhaustiv).
- **Oder-Patterns**: `p1 | p2 | p3` – der Arm trifft, wenn eine Alternative
  passt. Alle Alternativen müssen **dieselben Bindungen** mit denselben
  Typen einführen (oder keine); sonst E1044. Für Exhaustiveness und
  Unerreichbarkeit wird jede Alternative einzeln ausgewertet; eine
  bereits abgedeckte Alternative ist E1042.
- **Guards**: `pattern if bedingung => ...` – die Bedingung muss `bool` sein
  und darf Bindungen des Patterns verwenden. Arme mit Guard zählen nicht für
  die Exhaustiveness; ein `_`-Arm (oder vollständige Abdeckung ohne Guards)
  bleibt erforderlich.
- Benannte Varianten-Patterns müssen alle Felder auflisten (`feld: _` zum
  Ignorieren).
- Bindungen sind im Arm lokal und unveränderlich.
- Matching auf Nicht-Enums ist nur mit `_`/Bindungen/Literalen erlaubt
  (E1044 sonst).
- Vergleiche (`==`) und direkte Ausgabe von Enums sind nicht unterstützt
  (E1012/E1019); `match` verwenden.

### 7.4 Option und Result

`Option<T>` und `Result<T, E>` sind eingebaute generische Enums und
verwenden dieselben Regeln wie benutzerdefinierte Enums (Konstruktion,
Matching, Exhaustiveness). Verschachtelte Instanzen wie
`Option<Option<int>>` sind erlaubt und exhaustiv prüfbar.

### 7.5 Fehlerweitergabe mit `?`

Der `?`-Operator gibt den Fehler einer `Result`- oder `Option`-Berechnung
an den Aufrufer weiter:

```bloom
fn parse(text: string) -> Result<int, string> {
    let value = int_or_fail(text)?;
    return Result::Ok(value * 2);
}

fn first(o: Option<int>) -> Option<int> {
    let v = o?;            // None wird sofort weitergereicht
    return Option::Some(v + 1);
}
```

- Der Operand muss ein `Result<T, E>` oder `Option<T>` sein (E1060 sonst).
- Die umgebende Funktion muss `Result<_, E>` mit demselben Fehlertyp bzw.
  `Option<_>` zurückgeben (E1061 sonst). `Result?` in einer `Option`-
  Funktion (und umgekehrt) ist ein Fehler.
- Bei `Err(e)`/`None` wird sofort der passende Fehlerwert zurückgegeben;
  bei `Ok(v)`/`Some(v)` liefert der Ausdruck `v`. Der Operand wird genau
  einmal ausgewertet.
- Der Fehlerpfad gibt Statement-Temporäre und Funktions-Slots frei
  (Referenzzählung), bevor er zurückkehrt.
- Verkettung ist erlaubt: `let x = step_a()?.field;`

## 8. Funktionen

```bloom
pub fn add(a: int, b: int) -> int {
    return a + b;
}
```

- Ohne `-> Typ` ist der Rückgabetyp `void`.
- Parameter brauchen Typangaben; doppelte Namen sind Fehler.
- Nicht-`void`-Funktionen müssen auf allen Pfaden zurückgeben (E1007).
- Rekursion und Vorwärtsaufrufe sind erlaubt; Überladung gibt es nicht.
- Eingebaute Funktionen können nicht neu definiert werden.
- `main` ist der Einstiegspunkt (`fn main() -> int`, keine Parameter) und
  muss im Einstiegsmodul stehen (E2001/E2002).

### Generische Funktionen

```bloom
fn identity<T>(value: T) -> T {
    return value;
}

fn first_of<T>(values: [T]) -> Option<T> {
    if len(values) == 0 {
        return Option::None;
    }
    return Option::Some(values[0]);
}

let a = identity(5);          // T = int (Inferenz aus dem Argument)
let b = identity::<float>(2); // explizite Typargumente (Turbofish)
```

- Typparameter nach dem Funktionsnamen; sie können in Parametern, Rückgabe
  und im Körper (lokale Typannotationen) verwendet werden.
- Inferenz aus Argumenten und dem erwarteten Rückgabetyp (auch aus
  Rückgabekontext wie `let x: Box<int> = wrap(...)`).
- Nicht ableitbare Typparameter sind E1045; falsche Anzahl ist E1016.
- Jede konkrete Instanziierung wird einmal monomorphisiert; generische
  Funktionen können generische Funktionen und Methoden aufrufen.

## 8a. Externe Funktionen (FFI) und `unsafe`

```bloom
extern "C" {
    fn bloom_ffi_add(a: c_int, b: c_int) -> c_int;
}

extern "system" {
    fn GetCurrentProcessId() -> c_uint;
}

fn main() -> int {
    unsafe {
        let sum = bloom_ffi_add(c_int(2), c_int(3));
        println(int(sum));
        println(int(GetCurrentProcessId()) > 0);
    }
    return 0;
}
```

- **Syntax:** `extern "ABI" { fn name(params) -> ret; ... }` (optional `pub`
  pro Funktion oder `pub extern`); die Deklaration endet mit `;` und hat
  keinen Körper. Erlaubte ABIs: `"C"` und `"system"` (`system` = Windows-
  Systemkonvention; unter 32-bit Windows `__stdcall`, sonst Standard).
- **Symbolname:** Der Bloom-Name **ist** der externe Symbolname (keine
  Mangling-Transformation). Doppelte Symbole oder Kollisionen mit Bloom-
  Funktionen sind E1010; unbekannte ABIs E1071.
- **FFI-Typen** (nur diese sind in `extern`-Signaturen erlaubt, sonst
  E1072): `c_int`, `c_uint`, `c_long`, `c_ulong`, `c_longlong`,
  `c_ulonglong` (targetgerechte C-Darstellung: `int`, `unsigned int`,
  `long`, `unsigned long`, `long long`, `unsigned long long` — `long` ist
  unter Windows 32-bit und auf LP64-Systemen 64-bit), `f32` (`float`) und
  `rawptr` (`void *`, undurchsichtiger Zeiger). Rückgabetyp darf zusätzlich
  `void` sein. Bloom-`string`/Arrays/Structs sind **nicht** erlaubt.
- **Konvertierungen (explizit):** `c_int(x)`, `c_uint(x)`, `c_long(x)`,
  `c_ulong(x)`, `c_longlong(x)`, `c_ulonglong(x)`, `f32(x)` (C-Cast-
  Semantik; akzeptieren `int`, `float` und ABI-Skalare); `int(x)` und
  `float(x)` akzeptieren ABI-Skalare; `rawptr_null()` liefert den
  Null-Zeiger. Keine impliziten Konvertierungen: `f(1)` mit `c_int`-
  Parameter ist E1002.
- **`rawptr`** ist undurchsichtig: keine Dereferenzierung, keine
  Zeigerarithmetik, keine Konvertierung nach `int` (nur `==`/`!=`).
  Ein `rawptr` ist kein gültiger Bloom-String/Array/Struct.
- **`unsafe`:** Jeder Aufruf einer externen Funktion ist nur innerhalb
  eines `unsafe { ... }`-Blocks erlaubt (sonst E1070). `unsafe` ist
  lexikalisch geschachtelt (verschachtelte `unsafe`-Blöcke sind erlaubt)
  und hebt **keine** Typprüfung auf: Parameter-, Rückgabe- und
  ABI-Prüfungen laufen weiterhin. Die Grenze markiert nur, dass Bloom den
  Vertrag der nativen Funktion nicht beweisen kann.
- **Codegenerierung:** Pro Deklaration wird ein `extern`-Prototyp mit dem
  Symbolnamen und der Konvention emittiert; es entsteht keine
  Bloom-Implementierung. Fehlende Symbole melden sich beim Linken
  (E3003 mit der Linker-Meldung).
- **Bibliotheken linken:** `bloom build/run --link <name-oder-datei>`
  (wiederholbar): Bibliotheksnamen (`[A-Za-z0-9_.-]+`, keine Pfade) oder
  vorhandene Dateien (`.c`, `.obj`, `.o`, `.lib`, `.a`; `.c`-Dateien
  werden vom C-Compiler mitkompiliert). Zusätzlich `bloom.toml`:

  ```toml
  [build]
  link_libraries = ["kernel32", "user32"]
  ```

  Ungültige Namen sind Konfigurationsfehler (E3005 über die CLI, sonst
  E3001 beim Manifest). Die geladenen Standardmodule linken ihre
  benötigten Bibliotheken automatisch (`std::win32` → kernel32/user32,
  `std::net`/`std::http` → ws2_32).

## 9. Kontrollfluss

```bloom
if score >= 90 { ... } else if score >= 50 { ... } else { ... }

while counter < 5 { counter = counter + 1; }

for i in 0..10 { println(i); }   // 0..9 (halboffen)
```

- Bedingungen müssen `bool` sein (E1009), keine Truthiness.
- `break`/`continue` nur in Schleifen (E1008).
- Die Laufvariable eines `for` ist unveränderlich und nur im Körper gültig.

## 10. Operatoren

| Stufe | Operatoren | Assoziativität |
| --- | --- | --- |
| 1 (schwächste) | `=` | rechts |
| 2 | `\|\|` | links |
| 3 | `&&` | links |
| 4 | `==` `!=` | links |
| 5 | `<` `<=` `>` `>=` | links |
| 6 | `+` `-` | links |
| 7 | `*` `/` `%` | links |
| 8 | unäres `-` `!` | rechts |
| 9 (stärkste) | Aufruf, Index, Feld | links |

| Operator | Operanden | Ergebnis |
| --- | --- | --- |
| `+` | numerisch / `string`,`string` | `int`/`float`/`string` |
| `-` `*` `/` | numerisch | `int`/`float` |
| `%` | `int`,`int` | `int` |
| `==` `!=` | gleiche Typen, numerisch gemischt, `string`, `char`, `bool` | `bool` |
| `<` `<=` `>` `>=` | numerisch gemischt oder `char`,`char` | `bool` |
| `&&` `\|\|` `!` | `bool` | `bool` |

- Gemischte `int`/`float`-Operanden: `int` wird zu `float` erweitert.
- Implizite `int`→`float`-Erweiterung auch bei Initialisierung, Zuweisung,
  `return`, Argumenten, Struct-Feldern, Enum-Nutzdaten und Array-Elementen.
- Keine implizite `float`→`int`-Konvertierung (E1002).
- Kein `%` für `float`; keine sortierenden String-Vergleiche;
  keine Struct-/Enum-Vergleiche.

## 11. Laufzeitsemantik

### Ganzzahlen

- Überläufe wickeln definiert (Zweierkomplement); Division/Modulo durch 0
  sind Laufzeitfehler (Exit 70); `INT64_MIN / -1` liefert `INT64_MIN`.
- Ganzzahldivision schneidet zur Null ab.

### Gleitkommazahlen

- IEEE 754; Division durch 0.0 liefert `±inf`, `0.0/0.0` `nan`.
- Ausgabe mit bis zu 15 signifikanten Stellen; `2.0`, `nan`, `inf`, `-inf`.

### Strings

- Unveränderlich, UTF-8, dürfen Nullbytes enthalten.
- `+` konkateniert; `==`/`!=` vergleichen Bytes.
- `len(s)` zählt Unicode-Skalarwerte; `s[i]` liefert den `char` am i-ten
  Skalarwert; Indexfehler sind Laufzeitfehler mit Datei/Zeile.
- String-Funktionen (`contains`, `starts_with`, `ends_with`, `trim`,
  `replace`, `split`, `join`) arbeiten byteweise und damit UTF-8-sicher für
  Teilstring-Suche; `to_upper`/`to_lower` konvertieren **nur ASCII**
  (`a`–`z`/`A`–`Z`), andere Bytes bleiben unverändert.
- String-Literale sind unsterblich (statisch); andere Strings werden per
  Referenzzählung verwaltet (siehe unten).

### Arrays

- Referenzwerte: Kopien teilen die Struktur; Elementzuweisungen, `push`,
  `pop`, `insert` und `remove` sind über alle Kopien sichtbar.
- Grenzprüfung zur Laufzeit; `push`/`pop`/`insert`/`remove` nur über
  veränderliche Wurzeln (E1006). `pop` und `remove` liefern `Option<T>`
  (leeres Array bzw. ungültiger Index → `None`); `insert(a, i, v)` erlaubt
  `0 <= i <= len(a)` (Laufzeitfehler sonst).
- `slice(a, start, end)` erzeugt eine **eigenständige Kopie** der Elemente
  (`start` inklusiv, `end` exklusiv, `0 <= start <= end <= len(a)`;
  Verletzungen sind Laufzeitfehler).
- Der Array-Header ist **stabil**: Wachstum reallokiert nur den Elementpuffer;
  Referenzen auf den Header (etwa Slice-Views) bleiben gültig.

### Slice-Views (`Slice<T>`)

Ein Slice-View ist eine Referenz auf einen zusammenhängenden Bereich eines
Arrays, ohne die Elemente zu kopieren:

```bloom
var values = [10, 20, 30, 40, 50];
let view = slice_view(values, 1, 4);   // [1, 4): Elemente 20, 30, 40
println(len(view));                    // 3
println(view[1]);                      // 30
```

- `Slice<T>` ist ein eigener Typ (kein `[T]`); `T` muss ein Basistyp sein.
  Verwendung als Variable, Parameter, Rückgabetyp, Struct-Feld und
  Enum-Nutzlast ist unterstützt; `push`, `pop`, `insert`, `remove`, `slice`
  und `join` lehnen `Slice<T>` mit E1019 ab.
- **Erzeugung:** `slice_view(a, start, end)` verlangt
  `0 <= start <= end <= len(a)`; Verletzungen sind Laufzeitfehler mit
  Quelle. Der View übernimmt eine Referenz auf den Array-Header.
- **Lesen:** `view[i]` liest das Element an der absoluten Position
  `start + i`; der Index wird gegen die View-Länge geprüft. Der Zugriff
  erfolgt stets über den stabilen Header und dessen aktuellen Puffer —
  ein `push`, das den Puffer reallokiert, macht den View **nicht** ungültig.
- **Schreiben:** `view[i] = v` verändert das Element im ursprünglichen
  Array (sichtbar über alle Array-Referenzen und andere Views). Die
  View-Bindung muss mit `var` deklariert sein (E1006). Ersetzen invalidiert
  keine Views.
- **`len(view)`** liefert `end - start` (geprüft; siehe Invalidierung).
- **Lebensdauer:** Solange ein View besteht, bleibt der Header (und damit
  der Puffer) am Leben — auch wenn die Array-Variable neu zugewiesen wird
  oder ihren Scope verlässt. Der letzte Besitzer (Array oder View) gibt frei.
- **Invalidierung:** `insert`, `remove` und ein erfolgreiches `pop`
  verändern die Indexzuordnung strukturell und invalidieren **alle**
  bestehenden Views (Generation des Headers). Jeder spätere Zugriff
  (`view[i]`, `view[i] = v`, `len(view)`) meldet dann einen Laufzeitfehler
  (Exit 70). Fehlgeschlagene Operationen (z. B. `pop` auf einem leeren
  Array), `push` und Elementzuweisungen invalidieren nicht.
- Views sind weder vergleichbar (`==`) noch direkt ausgebbar; `Slice<T>`
  in `==`/`print` ist E1012/E1019.

### Enums

- Tagged Union (Tag + Union der Nutzdaten), Werte werden kopiert.
- Nutzdaten werden inline gespeichert (daher keine Rekursion).
- Match wertet den Tag aus; Exhaustiveness garantiert, dass ein Wert immer
  einen Arm trifft.

### Speicherverwaltung

Bloom 0.8 verwaltet Heap-Speicher (Strings, Arrays, Slice-Views und
Structs/Enums mit verwalteten Feldern) mit **deterministischer
Referenzzählung**:

- Jeder Variablen-Slot besitzt eine Referenz; Speichern klont, Überschreiben
  und Blockende geben frei.
- Parameter sind besessen (Aufrufer klont), Rückgaben sind besessen,
  `self`-Empfänger sind geliehen.
- Der Fehlerpfad von `?`, `return`, `break` und `continue` geben die
  betroffenen Slots frei.
- Slice-Views zählen den Array-Header mit: Der Header lebt, solange ein
  Array-Besitzer **oder** ein View auf ihn verweist.
- `panic` beendet den Prozess direkt (Exit 70); Cleanup-Pfade laufen dabei
  **nicht** durch.
- Das Modell ist nicht threadsicher (Bloom bietet keine Threads an).

Details, Invarianten und Grenzen: `docs/MEMORY_MODEL.md`.

## 12. Module, Imports und Sichtbarkeit

### 12.1 Dateien und Modulnamen

- Ein Modul ist eine Datei. `use a::b;` sucht `a/b.blm` oder `a/b/mod.blm`.
- Zuerst wird relativ zum Verzeichnis der **importierenden Datei** gesucht,
  danach (für Untermodule) relativ zum Verzeichnis der Einstiegsdatei.
- Der kanonische Modulname ergibt sich aus dem Pfad relativ zum
  Einstiegsverzeichnis (`networking::client`). Die Einstiegsdatei selbst ist
  das Einstiegsmodul.
- Dieselbe Datei wird nur einmal geladen (Deduplizierung über den Pfad);
  zyklische Importe sind erlaubt und definiert (zweiphasige Prüfung).
- Das Einstiegsmodul kann nicht importiert werden (E1054).
- Jeder `use`-Pfad muss auflösbar sein (E1050); fehlende Symbole E1055.

### 12.2 Imports

```bloom
use geometry;               // Modul: geometry::distance(...), geometry::Point { ... }
use geometry::Point;        // Symbol: Point direkt verwenden
use networking::client;     // Untermodul (client::...)
pub use geometry::Point;    // Re-Export (siehe unten)
```

- Der lokale Name ist das letzte Segment; doppelte lokale Namen oder
  Kollisionen mit lokalen Deklarationen sind E1052.
- Importierte Symbole müssen `pub` sein (E1051), ebenso bei qualifiziertem
  Zugriff (`geometry::helper()`) und bei Methodenaufrufen auf fremden Typen.
- Importierte Symbole dürfen keine lokalen Deklarationen still überschreiben
  (Kollision = E1052).

### 12.2a Re-Exports (`pub use`)

`pub use` stellt ein importiertes Symbol unter dem eigenen Modul erneut
öffentlich bereit:

```bloom
// shapes.blm
pub use geometry::Point;
pub use geometry::distance;
```

- Andere Module können das Symbol dann über das Zwischenmodul importieren
  (`use shapes::Point;`, `use shapes::distance;`) oder qualifiziert
  verwenden (`shapes::distance(...)`, `shapes::Point { ... }`).
- Re-Exports ändern die **Identität** des Symbols nicht: aufgelöst wird
  immer die ursprüngliche kanonische Deklaration (gleicher C-Name, gleiche
  Methoden, gleiche Typargumente).
- Nur `pub`-Symbole dürfen re-exportiert werden; ein Re-Export eines
  privaten Symbols ist E1051.
- Mehrstufige Re-Exports funktionieren (transitive Auflösung); Zyklen
  terminieren und führen zu E1055 (nicht auflösbar).
- Das Re-Exportieren ganzer **Module** (`pub use geometry;`) wird nicht
  unterstützt (E1055 bei der Verwendung).

### 12.3 Sichtbarkeit

- `pub` auf Funktionen, Structs, Enums, Methoden und `use`-Deklarationen
  (Re-Exports).
- Ohne `pub` ist ein Symbol nur im eigenen Modul sichtbar.
- Struct-Felder und Enum-Varianten sind immer öffentlich.
- Globale Konstanten (`let`/`var`/`const` auf oberster Ebene) sind immer
  importierbar.
- `impl`-Blöcke müssen im Modul des Structs/Enums stehen; Ausnahme sind die
  eingebauten Enums `Option`/`Result`, deren `impl`-Blöcke in jedem Modul
  erlaubt sind.

### 12.4 Qualifizierte Pfade

- Ausdrücke: `geometry::distance(a, b)`, `geometry::Point { ... }`,
  `geometry::Direction::North`, `geometry::MAX`.
- Typen: `geometry::Point`, `geometry::Shape` in Signaturen, Feldern usw.
- Ein qualifizierter Pfad beginnt mit einem importierten Modulnamen; der
  Rest wird an den kanonischen Namen angehängt.

### 12.5 C-Codegenerierung

- Alle Module werden in eine C-Übersetzungseinheit übersetzt.
- Symbole des Einstiegsmoduls behalten kurze Namen (`bf_main`,
  `bloom_struct_Point`); Symbole anderer Module werden mit dem kanonischen
  Modulnamen präfixiert (`bf_geometry_distance`,
  `bloom_struct_geometry_Point`). Doppelte Unterstriche sind reserviert
  (E1056), daher sind Kollisionen ausgeschlossen.

## 13. Eingebaute Funktionen

| Aufruf | Bedeutung |
| --- | --- |
| `print(x)` | Ausgabe ohne Zeilenumbruch (Basistypen) |
| `println()` / `println(x)` | Ausgabe mit Zeilenumbruch |
| `input(prompt)` | Prompt ausgeben, Zeile lesen → `string` (EOF → "") |
| `len(x)` | Skalarwerte eines Strings / Elemente eines Arrays → `int` |
| `str(x)` | `int`/`float`/`bool`/`char`/`string` → `string` |
| `int(x)` | `float`/`bool`/`char`/`string` → `int` |
| `float(x)` | `int`/`float`/`string` → `float` |
| `char(x)` | `int`-Codepunkt → `char` |
| `exit(code)` | Programm mit Exit-Code beenden |
| `panic(msg)` | Absichtlicher fataler Fehler: Meldung + Quelle, Exit 70 |
| `push(a, v)` | An ein Array anhängen (veränderliche Wurzel) |
| `pop(a)` | Letztes Element entnehmen → `Option<T>` (leer → `None`) |
| `insert(a, i, v)` | `v` an Position `i` einfügen (`0 <= i <= len(a)`) |
| `remove(a, i)` | Element an `i` entnehmen → `Option<T>` (ungültig → `None`) |
| `slice(a, start, end)` | Kopie des Bereichs `[start, end)` → `[T]` |
| `slice_view(a, start, end)` | View auf `[start, end)` → `Slice<T>` (siehe Slice-Views) |
| `join(parts, sep)` | `[string]` mit `sep` verbinden → `string` |
| `contains(s, sub)` | Teilstring enthalten → `bool` |
| `starts_with(s, prefix)` | Präfix-Test → `bool` |
| `ends_with(s, suffix)` | Suffix-Test → `bool` |
| `trim(s)` | Führende/abschließende ASCII-Whitespaces entfernen |
| `replace(s, from, to)` | Alle Vorkommen von `from` ersetzen |
| `to_upper(s)` / `to_lower(s)` | ASCII-Groß-/Kleinschreibung |
| `split(s, sep)` | Zerlegen → `[string]` (leeres `sep` = Laufzeitfehler) |

Laufzeitfehler: `int(float)` bei NaN/inf/außerhalb des Bereichs;
`int(string)`/`float(string)` bei ungültiger Eingabe; `char(int)` bei
ungültigem Codepunkt; `insert` mit Index außerhalb `0..=len`;
`slice` mit ungültigen Grenzen; `split` mit leerem Separator.

`panic(msg)` ist divergierend: Nach `panic(...)` muss kein `return` mehr
folgen (die Rückgabeanalyse akzeptiert es als Abschluss); der Prozess
endet sofort mit Exit-Code 70 und der Meldung `Bloom panic: <msg>` samt
Datei/Zeile auf stderr. Cleanup-Pfade laufen dabei nicht durch.

### 13.1 Standardmodul `std.math`

```bloom
use std::math;          // Zugriff über math::sqrt(...)
use std::math::sqrt;    // oder einzelne Symbole importieren
```

| Funktion | Signatur | Semantik |
| --- | --- | --- |
| `sqrt(x)` | `float -> float` | Quadratwurzel; negative Eingaben → `nan` |
| `pow(x, y)` | `float, float -> float` | Potenz (C `pow`) |
| `floor(x)` / `ceil(x)` / `round(x)` | `float -> float` | Runden; `nan`/`inf` unverändert |
| `abs(x)` | `float -> float` | Betrag (`fabs`) |
| `min(x, y)` / `max(x, y)` | `float, float -> float` | IEEE-`fmin`/`fmax` (NaN wird ignoriert) |
| `clamp(v, lo, hi)` | `float, float, float -> float` | `lo > hi` = Laufzeitfehler |
| `abs_int(x)` | `int -> int` | Betrag; `INT64_MIN` bleibt `INT64_MIN` (wickelnd) |
| `min_int(a, b)` / `max_int(a, b)` | `int, int -> int` | Vergleich |
| `clamp_int(v, lo, hi)` | `int, int, int -> int` | `lo > hi` = Laufzeitfehler |

- Es gibt keine Überladung; Ganzzahl- und Gleitkomma-Varianten haben
  eigene Namen (`abs` vs. `abs_int`).
- `int`-Argumente werden an `float`-Parameter implizit erweitert.
- MSVC linkt die Mathematikfunktionen über die CRT; GCC/Clang erhalten
  `-lm` (bereits in der Toolchain-Anbindung). tcc wird mit Mathematik-
  Funktionen nicht getestet (dokumentierte Einschränkung).

### 13.2 Standardmodul `std::fs`

```bloom
use std::fs;          // fs::read_text(...)
use std::fs::read_text;
```

Alle Operationen liefern `Result<T, string>`; Fehlermeldungen nennen
Operation, Pfad und einen kurzen Grund (`read_text: dir/datei.txt: file
not found`). Kein Dateisystemfehler wird stillschweigend zu `false`.

| Funktion | Signatur | Bedeutung |
| --- | --- | --- |
| `read_text(path)` | `string -> Result<string, string>` | Ganze Datei als UTF-8-Text; ungültige UTF-8-Daten sind ein Fehler; ein UTF-8-BOM bleibt **Bestandteil** des Textes |
| `write_text(path, content)` | `string, string -> Result<int, string>` | Datei erzeugen/überschreiben; Ergebnis = Anzahl geschriebener Bytes |
| `append_text(path, content)` | `string, string -> Result<int, string>` | Am Ende anfügen; Ergebnis = Anzahl geschriebener Bytes |
| `exists(path)` | `string -> Result<bool, string>` | Existenz (auch für nicht zugreifbare Pfade `false`) |
| `is_file(path)` / `is_dir(path)` | `string -> Result<bool, string>` | Typprüfung; fehlende Pfade → `false` |
| `file_size(path)` | `string -> Result<int, string>` | Größe in Bytes; Verzeichnis = Fehler |
| `create_dir_all(path)` | `string -> Result<bool, string>` | Verzeichnis(se) anlegen (auch verschachtelt); bereits vorhanden = ok |
| `remove_file(path)` | `string -> Result<bool, string>` | Datei löschen |
| `remove_dir(path)` | `string -> Result<bool, string>` | **Leeres** Verzeichnis löschen |
| `copy_file(src, dst)` | `string, string -> Result<int, string>` | Kopieren; Ergebnis = Anzahl Bytes; Ziel wird überschrieben |
| `rename(src, dst)` | `string, string -> Result<bool, string>` | Verschieben/Umbenennen; Ziel wird ersetzt |
| `current_dir()` | `() -> Result<string, string>` | Aktuelles Arbeitsverzeichnis |
| `join_path(base, child)` | `string, string -> string` | Pfad verbinden (`\` unter Windows, `/` unter POSIX); absolute `child`-Pfade werden unverändert zurückgegeben; erzeugt keine Datei |
| `read_dir(path)` | `string -> Result<[string], string>` | Eintragsnamen (ohne `.`/`..`), byteweise sortiert |
| `read_text_limited(path, limit)` | `string, int -> Result<string, string>` | Wie `read_text`, aber mit Größenlimit: die Dateigröße wird **vor** dem Lesen geprüft (`file exceeds N bytes`); `limit <= 0` lehnt jede nicht-leere Datei ab |
| `read_bytes(path)` | `string -> Result<Bytes, string>` | Binärer Inhalt als verwaltetes `Bytes`-Handle (max. 8 MiB); NUL-Bytes bleiben erhalten; Verzeichnisse sind ein Fehler |
| `bytes_len(bytes)` | `Bytes -> int` | Länge in Bytes |

```bloom
pub struct Bytes { id }   // undurchsichtiges, verwaltetes Handle
```

- **`Bytes`** ist ein referenzgezähltes Byte-Puffer-Handle (`std.fs::Bytes`),
  das wie `JsonValue`/`Template` automatisch verwaltet wird. Der Inhalt
  ist **kein** gültiger UTF-8-Text garantiert; `read_text*` ist für Text,
  `read_bytes` für binäre Daten gedacht (z. B. für
  `http::write_response_bytes`).

- **Plattform:** Unter Windows werden alle Pfade von UTF-8 nach UTF-16
  konvertiert und an die Wide-Char-APIs gegeben — Nicht-ASCII-Dateinamen
  und Leerzeichen funktionieren. POSIX-Pfade verwenden die UTF-8-Bytes
  direkt (Code vorhanden, auf Windows nicht testbar).
- `read_text` liefert den tatsächlichen Inhalt (keine Zeilenenden-
  Normalisierung); leere Dateien liefern `""`.
- Die Bibliothek nutzt keine Shell; symbolische Links, Race Conditions und
  parallele Änderungen werden nicht zusätzlich abgesichert.

### 13.3 Standardmodul `std::process`

```bloom
use std::process;

let result = process::run("cmd.exe", ["/C", "echo", "Hallo"]);
```

```bloom
pub struct ProcessOutput {
    exit_code: int,
    stdout: string,
    stderr: string
}
```

| Funktion | Signatur |
| --- | --- |
| `run(program, args)` | `string, [string] -> Result<ProcessOutput, string>` |
| `run_in(program, args, cwd)` | `string, [string], string -> Result<ProcessOutput, string>` |

- **Keine Shell:** Programm und Argumente werden getrennt übergeben; unter
  Windows wird die Argumentliste nach den MSVC-Regeln in eine
  Kommandozeile übersetzt (Leerzeichen, Anführungszeichen, Backslashes).
  `lpApplicationName` ist NULL, daher sucht das System das Programm auch
  im `PATH`. Eine Shell startet nur, wenn sie ausdrücklich als Programm
  angegeben wird.
- **Ergebnis vs. Fehler:** Nur ein *Startfehler* ergibt `Err(...)`. Ein
  Programm mit Exit-Code ≠ 0 ist `Ok(ProcessOutput)` mit dem echten
  Exit-Code; der Aufrufer entscheidet.
- stdout und stderr werden getrennt und **nebenläufig per Polling**
  gelesen (kein Deadlock bei großen Ausgaben auf beiden Kanälen); die
  Bytes bleiben unverändert (keine Konvertierung). Der Speicherverbrauch
  wächst mit der Ausgabegröße (dokumentierte Strategie).
- Umgebungsvariablen werden vererbt; stdin wird nicht weitergeleitet.
- POSIX-Implementierung (fork/exec + poll) ist vorhanden, aber auf Windows
  nicht testbar.

### 13.4 Standardmodul `std::time`

| Funktion | Signatur | Bedeutung |
| --- | --- | --- |
| `unix_seconds()` | `() -> int` | Sekunden seit 1970-01-01T00:00:00Z (Systemuhr) |
| `unix_millis()` | `() -> int` | Millisekunden seit der Epoche (gleiche Uhr) |
| `monotonic_millis()` | `() -> int` | Monotone Millisekunden seit einem unspezifizierten Startpunkt (Windows: `QueryPerformanceCounter`; POSIX: `CLOCK_MONOTONIC`) — für Dauermessungen, nicht als Kalenderzeit |
| `sleep_ms(ms)` | `int -> void` | Pausiert; `0` = No-op, negative Werte = Laufzeitfehler, große Werte werden in Blöcken geschlafen. Das Betriebssystem kann überschlafen (keine exakte Aufwachgarantie) |
| `format_utc(secs)` | `int -> string` | `YYYY-MM-DDTHH:MM:SSZ` (ISO 8601, UTC, zeitzonenunabhängig, rein rechnerisch); Jahre außerhalb 0000–9999 sind ein Laufzeitfehler |

### 13.5 `bloom fmt`

```text
bloom fmt <datei.blm>       formatiert die Datei (sicherer Schreibvorgang)
bloom fmt <ordner>          formatiert alle .blm-Dateien rekursiv
bloom fmt --check <pfad>    prüft nur; Exit-Code 1, wenn Dateien abweichen
```

- Der Formatter arbeitet **tokenbasiert**: Jedes Token wird aus seinem
  exakten Quelltext-Slice neu ausgegeben; Kommentare bleiben vollständig
  erhalten, Literale (Escapes, Unicode, Zahlen) werden byteweise
  unverändert übernommen. Vor dem Schreiben wird der formatierte Text mit
  dem echten Parser erneut geparst; bei Syntaxfehlern wird **keine** Datei
  verändert.
- Stil: 4 Leerzeichen Einrückung; eine Anweisung pro Zeile; `{` bleibt in
  der Kopfzeile; Statement-/Match-Braces (mit `;` oder `=>`) setzen jedes
  Komma-Element auf eine eigene Zeile; Daten-Braces (Struct-Literale,
  benannte Felder, Patterns) behalten das Layout der Quelle (inline bleibt
  inline); ein Leerzeichen um binäre Operatoren, nach `,` und `:`; keine
  Leerzeichen um `.`, `::`, `..`; `<`/`>` gelten als Generics (ohne
  Leerzeichen), wenn in der Quelle kein Leerzeichen davor stand.
- Höchstens eine Leerzeile bleibt erhalten, wo die Quelle eine hatte; die
  Datei endet mit genau einem Zeilenumbruch; CRLF/LF und ein führendes BOM
  werden beibehalten.
- Idempotent: `format(format(x)) == format(x)`; `--check` verändert nichts
  und meldet nicht formatierte Dateien mit Exit-Code 1. Verzeichnisse
  überspringen versteckte Einträge und `target`; nur `.blm`-Dateien werden
  verarbeitet. Schreibvorgänge laufen über eine temporäre Datei im selben
  Verzeichnis (mit dokumentiertem Windows-Fallback).

### 13.6 Standardmodul `std::win32` (nur Windows)

| Funktion | Signatur | Bedeutung |
| --- | --- | --- |
| `process_id()` | `() -> int` | Prozess-ID (`GetCurrentProcessId`) |
| `tick_count_ms()` | `() -> int` | Millisekunden seit Systemstart (`GetTickCount64`) |
| `last_error()` | `() -> int` | `GetLastError()` des Threads |
| `get_computer_name()` | `() -> Result<string, string>` | Computername (UTF-16-API, UTF-8-konvertiert) |
| `sleep_ms(ms)` | `int -> void` | `Sleep`; 0 = No-op, negativ = Laufzeitfehler, große Werte in Blöcken |
| `message_box(title, message, kind)` | `string, string, int -> Result<int, string>` | Modale Meldungsbox (kind: 0 = Info, 1 = Warnung, 2 = Fehler, 3 = Frage); liefert die Button-ID. Wird **nicht** automatisiert getestet (blockiert auf Eingabe) |

Auf anderen Plattformen ist `std::win32` nicht verfügbar (Fehler beim
Import). Alle Funktionen sind sichere Wrapper (keine rohen Handles).

### 13.7 Standardmodul `std::net` (blockierendes TCP)

```bloom
use std::net;
let listener = net::tcp_listen("127.0.0.1", 0, 4)?;   // Port 0 = OS wählt
println(net::local_port(listener)?);
```

| Funktion | Signatur |
| --- | --- |
| `tcp_connect(host, port)` | `string, int -> Result<int, string>` |
| `tcp_listen(host, port, backlog)` | `string, int, int -> Result<int, string>` |
| `tcp_accept(listener)` | `int -> Result<int, string>` |
| `tcp_read(socket, max_bytes)` | `int, int -> Result<Option<string>, string>` |
| `tcp_write_all(socket, data)` | `int, string -> Result<int, string>` |
| `tcp_close(handle)` | `int -> Result<bool, string>` |
| `tcp_set_timeout(socket, ms)` | `int, int -> Result<bool, string>` |
| `local_port(handle)` | `int -> Result<int, string>` |

- **Handles** sind interne Bloom-Runtime-IDs (Slot + Generation), **keine**
  OS-Deskriptoren. Unbekannte, geschlossene oder falsch verwendete Handles
  (z. B. `tcp_read` auf einem Listener) liefern `Err`; doppeltes
  Schließen ist ein Fehler. Nach `tcp_close` ist der Handle endgültig
  ungültig (keine Wiederverwendung als „gültig“).
- `tcp_read` liefert höchstens `max_bytes` (1..1048576, sonst `Err`),
  `Some(data)` bei Daten, `None` bei EOF; empfangene Daten müssen gültiges
  UTF-8 sein (sonst `Err` — binäre Protokolle werden in dieser Version
  nicht unterstützt). `tcp_write_all` schreibt alles oder meldet `Err`
  (partielle Sends werden geschleift).
- `tcp_listen` unterstützt Port 0 (OS wählt); `local_port` liefert den
  tatsächlich gebundenen Port. `backlog` >= 1 (hoch geklemmt auf 4096).
- `tcp_set_timeout(ms)` setzt Lese-/Schreib-Timeouts (0 = kein Timeout,
  negativ = `Err`); **DNS-Auflösung und `tcp_connect` sind nicht
  abgedeckt** (dokumentiert). Timeouts liefern `Err` mit „timeout“.
- **`tcp_close` ist erforderlich** — es gibt kein RAII; vergessene Sockets
  werden nicht automatisch freigegeben. Fehlgeschlagene Verbindungen geben
  ihre Ressourcen selbst frei.
- Windows nutzt Winsock (WSAStartup automatisch); die POSIX-Variante ist
  vorhanden, aber nicht getestet.

### 13.8 Standardmodul `std::http` (HTTP/1.0 und HTTP/1.1, HTTPS, Chunked)

```bloom
use std::http;
let r = http::get("http://127.0.0.1:8080/")?;
println(str(r.status) + " " + r.body);
```

| Funktion | Signatur |
| --- | --- |
| `get(url)` | `string -> Result<HttpResponse, string>` |
| `post(url, content_type, body)` | `string, string, string -> Result<HttpResponse, string>` |
| `request(method, url, headers, body)` | `string, string, string, string -> Result<HttpResponse, string>` |
| `get_with_ca(url, ca_file)` | `string, string -> Result<HttpResponse, string>` |
| `post_with_ca(url, content_type, body, ca_file)` | `string, string, string, string -> Result<HttpResponse, string>` |
| `request_with_ca(method, url, headers, body, ca_file)` | `string, string, string, string, string -> Result<HttpResponse, string>` |
| `read_request(connection)` | `int -> Result<HttpRequest, string>` |
| `write_response(connection, status, content_type, body)` | `int, int, string, string -> Result<bool, string>` |
| `write_response_full(connection, status, content_type, headers, body)` | `int, int, string, string, string -> Result<bool, string>` |
| `write_response_bytes(connection, status, content_type, headers, bytes)` | `int, int, string, string, Bytes -> Result<bool, string>` |
| `write_response_bytes_head(connection, status, content_type, headers, bytes)` | `int, int, string, string, Bytes -> Result<bool, string>` |
| `header_get(headers, name)` | `string, string -> Option<string>` |

```bloom
pub struct HttpRequest  { method, target, version, headers, body }   // method/target/version/headers/body: string
pub struct HttpResponse { status: int, reason, version, headers, body }
```

- **Client:** `http://` (Standardport 80) und `https://` (Standardport 443,
  TLS über `std.tls`, Zertifikat wird immer validiert). Die
  `_with_ca`-Varianten verwenden eine eigene CA-Datei (PEM oder DER,
  erstes Zertifikat) statt des Windows-Zertifikatsspeichers. Der Client
  setzt `Host`, `Content-Length` und `Connection: close` und nutzt einen
  Lese-Timeout (15 s; DNS/Connect nicht abgedeckt).
- **Antworten:** Body per `Content-Length`, per
  `Transfer-Encoding: chunked` (Chunk-Größen in Hex, optionale
  Chunk-Extensions werden akzeptiert und ignoriert, Trailer-Felder
  validiert und verworfen) oder bis zum Verbindungsende. Statuszeilen
  1xx werden als Interim-Antworten verworfen (maximal 5; `101` wird
  abgelehnt); `HEAD`, `204` und `304` haben nie einen Body (ein
  irreführendes `Content-Length` wird ignoriert). Doppelte
  `Content-Length`, `Transfer-Encoding` neben `Content-Length` sowie
  unbekannte/mehrdeutige Transfer-Encodings werden abgelehnt
  (Request-Smuggling-Schutz).
- **Server:** `read_request` parst Request-Line (HTTP/1.0 oder 1.1),
  Header und Body (`Content-Length` oder `Transfer-Encoding: chunked`);
  bei HTTP/1.1 ist ein Host-Header Pflicht. Mehrere TCP-Segmente pro
  Nachricht werden korrekt zusammengesetzt. `write_response` erzeugt
  Statuszeile (Standard-Reason für bekannte Codes), `Content-Type` (ohne
  CR/LF), `Content-Length` und `Connection: close`; der Aufrufer schließt
  die Verbindung danach mit `tcp_close`. Antworten werden immer mit
  `Content-Length` geschrieben (kein Chunked-Encoding auf der
  Server-Schreibseite).
- **Binäre Antworten:** `write_response_bytes` schreibt den exakten
  Byte-Inhalt eines `std.fs::Bytes`-Puffers (eingebettete NUL-Bytes
  bleiben erhalten) mit `Content-Length` = Byte-Länge;
  `write_response_bytes_head` sendet dieselben Kopfzeilen, aber keinen
  Body (für `HEAD`). `204`/`304` erhalten unabhängig vom Puffer einen
  `Content-Length: 0` und keinen Body. `content_type` und Zusatz-Header
  werden wie bei `write_response_full` validiert (CR/LF werden
  abgelehnt, bevor irgendetwas geschrieben wird).
- **Zusatz-Header:** `write_response_full` nimmt einen rohen Headerblock
  (`Name: value`-Zeilen) entgegen, validiert ihn (Zeilenlimits,
  Token-Namen, keine CR/LF in Werten, höchstens 16384 Bytes) und emittiert
  ihn normalisiert. `Content-Length`, `Connection` und
  `Transfer-Encoding` bleiben unter Kontrolle der Bibliothek und werden
  abgelehnt. `204`- und `304`-Antworten erhalten nie einen Body
  (`Content-Length: 0`), auch wenn einer übergeben wird.
- **Limits (erzwungen, nie stillschweigend gekürzt):** Request-/Status-
  Zeile 8192 Bytes, Headerblock 16384 Bytes, Body 4 MiB, Chunk-Größe
  4 MiB, Interim-Antworten 5, Trailer-Felder 32. Überschreitungen sind
  `Err`.
- **Header:** `headers` ist der rohe Headerblock (Text). `header_get`
  vergleicht Namen case-insensitiv und liefert den **ersten** Treffer;
  Duplikate werden nicht zusammengeführt (dokumentiert).
- **Sicherheit:** Headernamen/-werte werden validiert, CRLF-Injection in
  generierten Headern ist ausgeschlossen, Längenangaben werden geprüft,
  mehrdeutiges Framing wird abgelehnt. **Keine Cookies, kein Proxy,
  keine Redirects, kein HTTP/2** — nichts davon wird behauptet.
- Alles ist synchron/blockierend; es gibt keine Threads oder Async-I/O.

### 13.9 Standardmodul `std::tls` (TLS-Client, nur Windows)

```bloom
use std::tls;
let handle = tls::tls_connect_with_ca("example.com", 443, "ca.der")?;
tls::tls_write_all(handle, "GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n")?;
match tls::tls_read(handle, 4096) {
    Result::Ok(Option::Some(text)) => println(text),
    Result::Ok(Option::None) => println("(Ende)"),
    Result::Err(e) => println("Fehler: " + e)
};
tls::tls_close(handle);
```

| Funktion | Signatur |
| --- | --- |
| `tls_connect(host, port)` | `string, int -> Result<int, string>` |
| `tls_connect_with_ca(host, port, ca_file)` | `string, int, string -> Result<int, string>` |
| `tls_connect_ex(host, port, timeout_ms, ca_file)` | `string, int, int, string -> Result<int, string>` |
| `tls_read(handle, max_bytes)` | `int, int -> Result<Option<string>, string>` |
| `tls_write_all(handle, data)` | `int, string -> Result<int, string>` |
| `tls_close(handle)` | `int -> Result<bool, string>` |
| `tls_set_timeout(handle, milliseconds)` | `int, int -> Result<bool, string>` |

- **Backend:** Windows Schannel (keine eigene Kryptografie). TLS 1.2 ist
  das Minimum, TLS 1.3 wird genutzt, wenn SDK und Betriebssystem es
  anbieten; starke Verschlüsselung ist erzwungen (`SCH_USE_STRONG_CRYPTO`).
  Auf anderen Plattformen liefern alle Funktionen einen Fehler.
- **Zertifikatsprüfung (immer aktiv, kein unsicherer Modus):** Mit
  System-Trust (`tls_connect`/`tls_connect_ex` mit leerem `ca_file`)
  werden Kette, Gültigkeitszeitraum, ServerAuth-EKU und Hostname gegen
  den Windows-Zertifikatsspeicher geprüft. Mit `ca_file` (PEM oder DER,
  erstes Zertifikat) muss die Kette exakt an dieser CA enden; zusätzlich
  werden Gültigkeitszeitraum, EKU und Hostname geprüft. Wildcards (`*.`)
  matchen genau ein Label; IP-Literale matchen `iPAddress`-SAN-Einträge.
  Es gibt keine Option zum Abschalten der Prüfung. Eine
  Sperrlistenprüfung (CRL/OCSP) findet nicht statt (dokumentiert).
- **Handshake:** SNI mit dem angegebenen Hostnamen, begrenzt durch einen
  Timeout (Standard 15 s, `tls_connect_ex` erlaubt 1..600000 ms).
  TLS-1.3-Post-Handshake-Nachrichten (Session Tickets, KeyUpdate) werden
  transparent verarbeitet; echte Renegotiation (TLS 1.2) wird abgelehnt.
- **Daten:** `tls_read` liefert bis zu `max_bytes` (1..1048576) UTF-8-Text
  oder `None` nach einem sauberen `close_notify` des Peers. Ein
  Verbindungsende **ohne** `close_notify` ist ein Fehler (die Nachricht
  könnte unvollständig sein). `tls_write_all` sendet den gesamten String
  und liefert die Anzahl der Bytes; `tls_close` sendet best-effort ein
  `close_notify`, schließt den Socket und invalidiert das Handle.
  Timeouts gelten für Lese-/Schreiboperationen und werden mit
  `tls_set_timeout` gesetzt (0 löscht sie).
- **Handles:** TLS-Verbindungen nutzen dieselbe interne Handle-Registry
  wie `std::net`; `tcp_*`-Funktionen lehnen TLS-Handles ab (und
  umgekehrt). `BLOOM_MEM_REPORT=1` weist TLS-Verbindungen als
  `live sockets` aus.
- **Limits:** wie `std::net`; die HTTP-Schicht nutzt zusätzlich die dort
  dokumentierten Größenlimits. Alles ist synchron/blockierend.

### 13.10 Standardmodul `std::path` (lexikalische Pfadoperationen)

```bloom
use std::path;
let file = path::join("C:\\projekte", "src\\main.blm");
let dir = path::normalize("C:\\projekte\\src\\..\\tests")?;
let name = path::file_name(file);
```

| Funktion | Signatur |
| --- | --- |
| `join(base, child)` | `string, string -> string` |
| `parent(path)` | `string -> Option<string>` |
| `file_name(path)` | `string -> Option<string>` |
| `extension(path)` | `string -> Option<string>` |
| `stem(path)` | `string -> Option<string>` |
| `is_absolute(path)` | `string -> bool` |
| `normalize(path)` | `string -> Result<string, string>` |
| `with_extension(path, extension)` | `string, string -> string` |

Alle Funktionen arbeiten **nur auf dem Text** des Pfades und greifen nie
auf das Dateisystem zu (keine Existenz-, Symlink- oder Metadatenprüfung).
Es gilt die Syntax der **Host-Plattform**: Unter Windows sind `\` und `/`
Trennzeichen (Ausgabe mit `\`), Laufwerkspräfixe (`C:`) und
UNC-Präfixe (`\\server\share`) werden erkannt; unter POSIX ist `/` das
einzige Trennzeichen und `\` ein normales Dateinamenzeichen. Auf dieser
Maschine (Windows) ist ausschließlich die Windows-Semantik getestet.

- **`join`:** Ist `child` absolut (oder hat ein Laufwerks-/UNC-Präfix),
  wird `child` zurückgegeben; ein nur gewurzeltes `child` (`\foo`)
  übernimmt das Präfix der Basis (`join("C:\a", "\b")` → `C:\b`).
  Sonst wird mit genau einem Trennzeichen angehängt.
- **`parent`:** Liefert den Pfad ohne die letzte Komponente (Rust-artig):
  `parent("C:\a\b")` → `C:\a`, `parent("a")` → `""` (leerer Pfad),
  `parent("C:\")` → `None`, `parent("..")` → `""`.
- **`file_name`:** Letzte Komponente; abschließende Trennzeichen und
  `.`-Komponenten werden ignoriert, `..` ist kein Dateiname
  (`file_name("..")` → `None`, `file_name("a/./")` → `a`).
- **`extension`/`stem`:** Rust-artig, ohne führenden Punkt; ein führender
  Punkt zählt nicht als Endung (`.config` → Endung `None`, Stamm
  `.config`); `foo.tar.gz` → Endung `gz`, Stamm `foo.tar`; `trailing.` →
  Endung `""`, Stamm `trailing`.
- **`is_absolute`:** Windows: nur Laufwerk + Wurzel (`C:\`) oder UNC;
  `C:foo` und `\foo` sind **nicht** absolut. POSIX: führendes `/`.
- **`normalize`:** Lexikalisches Aufräumen im Stil von Go `filepath.Clean`:
  Trennzeichen vereinheitlichen/zusammenfassen, `.` entfernen, `..`
  gegen vorherige Komponenten auflösen (bei relativen Pfaden bleiben
  führende `..` erhalten, an Wurzeln wird `..` verworfen), abschließende
  Trennzeichen entfernen (außer an Wurzeln). Ein relativer Pfad wird nie
  absolut; `.`-Eingaben ergeben `.`. **Fehler (statt still falscher
  Ergebnisse):** leerer Pfad, `\\?\`-/`\\.\`-Präfixe, UNC ohne
  Server/Share. `normalize` ist idempotent. Es werden **keine**
  symbolischen Links, Junctions, 8.3-Namen, Groß-/Kleinschreibung oder
  andere reale Dateisystemeigenschaften aufgelöst; ein lexikalisch
  normalisierter Pfad ist keine Sicherheitsgarantie und kein Beweis,
  dass ein Zugriff in einem Basisverzeichnis bleibt.
- **`with_extension`:** Ersetzt die Endung oder hängt sie an; eine leere
  Endung entfernt sie; ein einzelner führender Punkt im Argument wird
  akzeptiert (`"md"` und `".md"` sind gleich). Ohne Dateinamen (Wurzel,
  `..`) wird der Pfad unverändert zurückgegeben.
- **`std::fs` bleibt für Dateisystemzugriffe zuständig**;
  `std::path` dupliziert keine Existenzprüfungen.

### 13.11 Standardmodul `std::json` (JSON nach RFC 8259)

```bloom
use std::json;
let doc = json::parse("{\"name\": \"Ada\", \"tags\": [\"math\"]}")?;
let name = json::get_string(doc, "name")?;
println(json::stringify_pretty(doc)?);
```

| Funktion | Signatur |
| --- | --- |
| `parse(input)` | `string -> Result<JsonValue, string>` |
| `stringify(value)` | `JsonValue -> Result<string, string>` |
| `stringify_pretty(value)` | `JsonValue -> Result<string, string>` |
| `get(value, key)` | `JsonValue, string -> Option<JsonValue>` |
| `has(value, key)` | `JsonValue, string -> bool` |
| `get_string/get_int/get_float/get_bool(value, key)` | `JsonValue, string -> Result<T, string>` |
| `as_string/as_int/as_float/as_bool(value)` | `JsonValue -> Result<T, string>` |
| `array_length(value)` | `JsonValue -> Result<int, string>` |
| `array_get(value, index)` | `JsonValue, int -> Option<JsonValue>` |
| `object_keys(value)` | `JsonValue -> Result<[string], string>` |
| `set(value, key, item)` | `JsonValue, string, JsonValue -> Result<bool, string>` |
| `push(value, item)` | `JsonValue, JsonValue -> Result<bool, string>` |
| `null()` | `-> JsonValue` |
| `make_bool(b)` | `bool -> JsonValue` |
| `make_int(i)` | `int -> JsonValue` |
| `make_float(f)` | `float -> Result<JsonValue, string>` |
| `make_string(s)` | `string -> JsonValue` |
| `make_array()` | `-> JsonValue` |
| `make_object()` | `-> JsonValue` |
| `type_of(value)` | `JsonValue -> string` |

- **Wertmodell:** `json::JsonValue` ist ein undurchsichtiges Handle auf
  einen **referenzgezählten** Baum (sechs Grundtypen: `null`, `boolean`,
  `number`, `string`, `array`, `object`). Das Handle ist ein *verwalteter*
  Bloom-Wert: Kopieren hält den Baum am Leben, das Ende des
  Gültigkeitsbereichs gibt ihn frei — es gibt **kein** manuelles
  `free`. Teilwerte aus `get`/`array_get` sind eigene, mitverwaltete
  Referenzen (keine tiefen Kopien); Zyklen können nicht entstehen
  (`set`/`push` lehnen sie ab), daher wird jeder Baum vollständig
  freigegeben.
- **Objekte:** Die Einfügereihenfolge bleibt erhalten (`object_keys`
  liefert sie in dieser Reihenfolge; `stringify` ebenfalls). `set`
  ersetzt vorhandene Werte **an Ort und Stelle** (Position bleibt).
  Doppelte Schlüssel lehnt der Parser ab (kein stilles Last-wins).
- **Zahlen:** Geparste Zahlen behalten ihr **Original-Literal**, die
  Serialisierung ist daher verlustfrei (auch für Ganzzahlen > 2^53 und
  lange Dezimalbrüche). `get_int`/`as_int` funktionieren nur für
  Literale ohne Bruch/Exponent, die in `int` (i64) passen; sonst klarer
  Fehler. `get_float`/`as_float` konvertieren über `double` (bei Werten
  > 2^53 kann das runden; sehr große Literale ergeben `inf`).
  Konstruierte Gleitkommazahlen werden in der kürzesten
  round-trip-fähigen Form serialisiert; `NaN`/`inf` lehnt `make_float`
  ab, der Serializer gibt sie nie aus.
- **Strings:** UTF-8; alle RFC-Escapes (`\"`, `\\`, `\/`, `\b`, `\f`,
  `\n`, `\r`, `\t`, `\uXXXX`) inklusive Surrogatpaaren. Unpaarige
  Surrogate, ungültige Escapes und rohe Steuerzeichen sind Fehler. Der
  Serializer escaped Steuerzeichen und `"`/`\`, alle anderen Bytes
  (inkl. UTF-8) bleiben unverändert.
- **Fehler:** `parse` liefert `Result::Err` mit Ursache und Position als
  **Byte-Offset (1-basiert), Zeile und Spalte** (1-basiert, Zeilen zählen
  `\n`), z. B. `unexpected character at byte 6 (line 1, column 6)`.
  Inhalt wird nicht in Meldungen kopiert. `get_*` liefert `Err` bei
  fehlendem Schlüssel oder falschem Typ; `get`/`array_get` liefern
  dagegen `None` (Array-Index außerhalb → `None`).
- **Limits (erzwungen, nie stillschweigend gekürzt):** Eingabe
  8 388 608 Bytes, Verschachtelungstiefe 128, String 4 194 304 Bytes,
  Zahlen-Token 128 Zeichen, Array-Elemente/Objektfelder je 1 048 576,
  geparste Werte insgesamt 2 097 152; die Serialisierung lehnt Bäume
  tiefer als 512 Ebenen ab. Tiefe Bäume werden iterativ freigegeben
  (kein Stack-Überlauf).
- **HTTP:** JSON lässt sich direkt auf `http::get`-Bodies anwenden
  (`json::parse(r.body)`); Komfort-APIs wie JSON-Routing sind nicht Teil
  von 0.12.0.

### 13.12 Standardmodul `std::web` (REST-Routing, synchron)

```bloom
use std::web;
use std::json;

let router = web::router();
web::route(router, "GET", "/users/:id", "show_user")?;

let m = web::resolve(router, "GET", "/users/42?q=ada")?;
if m.status == 200 {
    println(m.name);                                  // "show_user"
    println(json::get_string(m.params, "id")?);       // "42"
    println(json::get_string(m.params, "q")?);        // "ada"
}
```

| Funktion | Signatur |
| --- | --- |
| `router()` | `-> Router` |
| `route(router, method, pattern, name)` | `Router, string, string, string -> Result<bool, string>` |
| `resolve(router, method, target)` | `Router, string, string -> Result<Match, string>` |
| `resolve_request(router, request)` | `Router, HttpRequest -> Result<Match, string>` |
| `json_body(body)` | `string -> Result<JsonValue, string>` |
| `json_body_request(request)` | `HttpRequest -> Result<JsonValue, string>` |
| `json_response(connection, status, value)` | `int, int, JsonValue -> Result<bool, string>` |
| `json_response_with(connection, status, value, headers)` | `int, int, JsonValue, string -> Result<bool, string>` |
| `static_file(connection, root, request)` | `int, string, HttpRequest -> Result<int, string>` |
| `form_body(body)` | `string -> Result<FormData, string>` |
| `form_body_request(request)` | `HttpRequest -> Result<FormData, string>` |
| `form_count(form)` | `FormData -> int` |
| `form_has(form, name)` | `FormData, string -> bool` |
| `form_get(form, name)` | `FormData, string -> Result<string, string>` |
| `form_first(form, name)` | `FormData, string -> Option<string>` |
| `form_values(form, name)` | `FormData, string -> [string]` |
| `form_names(form)` | `FormData -> [string]` |
| `form_encode(form)` | `FormData -> Result<string, string>` |
| `cookies(request)` | `HttpRequest -> Result<Cookies, string>` |
| `cookie_count(cookies)` | `Cookies -> int` |
| `cookie_has(cookies, name)` | `Cookies, string -> bool` |
| `cookie_get(cookies, name)` | `Cookies, string -> Result<string, string>` |
| `cookie_first(cookies, name)` | `Cookies, string -> Option<string>` |
| `cookie_names(cookies)` | `Cookies -> [string]` |
| `set_cookie(name, value, options)` | `string, string, CookieOptions -> Result<string, string>` |
| `delete_cookie(name, path, insecure_http)` | `string, string, bool -> Result<string, string>` |
| `cookie_headers(lines)` | `[string] -> Result<string, string>` |
| `session_store(max_sessions, ttl_seconds)` | `int, int -> Result<SessionStore, string>` |
| `session_count(store)` | `SessionStore -> int` |
| `session_new(store)` | `SessionStore -> Result<Session, string>` |
| `session_load(store, cookies, cookie_name)` | `SessionStore, Cookies, string -> Result<Session, string>` |
| `session_id(session)` | `Session -> string` |
| `session_data(session)` | `Session -> JsonValue` |
| `session_destroy(store, session)` | `SessionStore, Session -> bool` |
| `session_expire(store)` | `SessionStore -> int` |
| `session_cookie(session, name, max_age)` | `Session, string, int -> Result<string, string>` |
| `session_cookie_insecure(session, name, max_age)` | `Session, string, int -> Result<string, string>` |
| `csrf_token(session)` | `Session -> Result<string, string>` |
| `csrf_check(session, token)` | `Session, string -> bool` |
| `csrf_rotate(session)` | `Session -> Result<string, string>` |
| `csrf_clear(session)` | `Session -> void` |
| `session_registered(store, session)` | `SessionStore, Session -> bool` |
| `session_regenerate(store, session)` | `SessionStore, Session -> Result<Session, string>` |

```bloom
pub struct Router { id }   // undurchsichtiges, verwaltetes Handle
pub struct Match  { status: int, name, allow, params }  // params: JsonValue
pub struct FormData { id } // undurchsichtiges, verwaltetes Handle
pub struct Cookies { id }  // undurchsichtiges, verwaltetes Handle
pub struct SessionStore { id }
pub struct Session { id }

// Wird explizit gebaut, damit jede Sicherheitsentscheidung sichtbar ist:
pub struct CookieOptions {
    path: string,          // "" = kein Path-Attribut, sonst beginnt mit "/"
    max_age: int,          // 0 = kein Max-Age, sonst 1..31536000
    same_site: string,     // "Lax", "Strict", "None" oder "" (weglassen)
    http_only: bool,
    insecure_http: bool    // true = Secure weglassen (nur Entwicklung)
}
```

- **Ausführungsmodell:** Alles ist synchron; Bloom hat keine Closures,
  daher gibt es **keine Callback-Handler**. `route` registriert eine
  Route unter einem Anwendungsnamen; nach `resolve`/`resolve_request`
  verzweigt die Anwendung selbst über `Match.name` (z. B. mit `match`
  oder `if`-Ketten) und beantwortet die Verbindung mit `std::http`.
- **Routensyntax:** Segmente durch `/` getrennt; feste Segmente und
  `:name`-Parameter (Name: Buchstabe/Unterstrich, dann Buchstaben,
  Ziffern, Unterstriche; höchstens 64 Zeichen; pro Muster eindeutig).
  Abschließende Slashes werden ignoriert; leere Segmente in der Mitte
  (`/a//b`) werden abgelehnt; `:`, `?`, `#` und Steuerbytes sind in
  festen Segmenten reserviert. Musterliterale werden **nicht**
  prozentdekodiert (sie matchen dekodierte Anfragesegmente).
- **Methoden:** 1..16 ASCII-Buchstaben, werden zu Großbuchstaben
  normalisiert (`get` ≡ `GET`).
- **Registrierung:** Doppelte Muster (nach Normalisierung), doppelte
  Namen pro Methode und Muster mit **gleicher Shape** (gleiche feste
  Literale und Parameterpositionen, z. B. `/users/:id` vs. `/users/:x`)
  werden mit klarer Meldung abgelehnt; höchstens 4096 Routen, Muster
  höchstens 4096 Bytes, Namen höchstens 256 Bytes.
- **Auflösung:** Die Anfrage wird zuerst an `?` geteilt, der Pfad an `/`
  segmentiert und **danach** jedes Segment prozentdekodiert — kodierte
  Trennzeichen (`%2F`) erzeugen keine neuen Routen. Query-Parameter
  (`&`/`=`, formartig: `+` wird zum Leerzeichen, leere Schlüssel
  entfallen, **der erste Wert eines wiederholten Schlüssels gewinnt**)
  landen zusammen mit den Pfadparametern in `Match.params` (Pfad
  gewinnt bei Namenskollision; Reihenfolge: Pfadparameter, dann Query).
  Ungültige Prozentkodierung oder ungültige UTF-8-Ergebnisse sind
  `Err` (die Anwendung antwortet z. B. mit 400). Oberhalb von 256
  Pfadsegmenten oder 256 Query-Parametern: `Err`; Ziel höchstens
  65536 Bytes.
- **Status:** `200` = Route gefunden (`name`/`params` gefüllt); `404` =
  kein Pfad matcht; `405` = Pfad matcht andere Methoden (`allow` enthält
  die deduplizierte, sortierte Methode­nliste für den `Allow`-Header).
  Unter mehreren passenden Routen gewinnt die **spezifischste** (festes
  Segment schlägt `:param` an der ersten abweichenden Position); gleiche
  Shapes sind bereits bei der Registrierung ausgeschlossen, daher ist der
  Gewinner eindeutig.
- **JSON-Hilfen:** `json_body`/`json_body_request` parsen den
  Anfragekörper mit `std::json` (leerer Körper: klarer Fehler); die
  Größenlimits von `read_request` (4 MiB Body) gelten bereits beim Lesen.
  `json_response`/`json_response_with` serialisieren einen
  `json::JsonValue` und senden ihn mit `Content-Type: application/json`
  über `write_response_full`; Serialisierungsfehler werden
  durchgereicht.
- **Sicherheit:** Routing ist **keine Autorisierung** — Anwendungen
  müssen Authentifizierung und Berechtigungen selbst implementieren; ein
  200 von `resolve` bedeutet nur, dass ein Routenmuster passte. Fehler
  geben keine Speicherinterna preis. `Router` ist ein verwalteter
  Bloom-Wert (Referenzzählung, kein manuelles Freigeben); `Match.params`
  ist ein mitverwalteter `json::JsonValue`.
- **Statische Dateien:** `static_file(connection, root, request)`
  beantwortet `GET` und `HEAD` direkt (andere Methoden: `405` mit
  `Allow: GET, HEAD`). Der Anfragepfad wird zuerst am `?` geteilt, dann
  prozentdekodiert (jeweils genau einmal); ungültige Escapes/UTF-8,
  dekodierte Trenner (`/`, `\`), `..`-Segmente, absolute Pfade und
  Laufwerks-/UNC-Präfixe ergeben `400`. Der Pfad wird gegen `root`
  verbunden, geöffnet und **nach dem Öffnen** (inklusive aufgelöster
  Symlinks/Junctions) mit dem kanonischen Wurzelpfad verglichen — ein
  Ausbruch ergibt `400`; `std::path`-Normalisierung allein ist keine
  Sicherheitsgrenze. Fehlende Dateien und Ordner: `404`; Dateien über
  8 MiB (`BLOOM_FS_MAX_BYTES`) und andere Öffnungs-/Lesefehler: `500`.
  Der `Content-Type` kommt ausschließlich aus der Endung (html/htm, css,
  js, json, svg, txt, png, jpg/jpeg, gif, webp, ico, woff, woff2;
  unbekannt: `application/octet-stream`) und wird immer zusammen mit
  `X-Content-Type-Options: nosniff` gesendet. `HEAD` sendet dieselben
  Kopfzeilen inklusive echtem `Content-Length` ohne Body. Antworten
  werden vollständig gepuffert (kein Streaming, keine Range-Requests,
  kein ETag/Last-Modified, kein `index.html`-Fallback: ein leerer Pfad
  ergibt `404`). Der Rückgabewert ist der gesendete Status; `Err`
  bedeutet, dass die Antwort nicht geschrieben werden konnte.
- **Formulare:** `form_body` parst einen
  `application/x-www-form-urlencoded`-Body, `form_body_request` prüft
  zusätzlich den `Content-Type`-Header. Einträge bleiben **in der
  Reihenfolge des Requests** erhalten; wiederholte Feldnamen werden
  nicht überschrieben (Checkbox-Gruppen, Mehrfachauswahlen). Ein Feld
  ist entweder **nicht vorhanden** (dann ist `has` false und `get`
  liefert `Err`), vorhanden mit **leerem Wert** (`get` liefert
  `Ok("")`) oder vorhanden mit einem bzw. mehreren Werten.
  `form_get` verlangt genau einen Wert: fehlt das Feld oder kommt es
  mehrfach vor, ist das ein `Err` (kein stiller „erster Wert“).
  `form_first` liefert den ersten Wert (`Option`), `form_values` alle
  Werte in Reihenfolge, `form_names` die verschiedenen Namen in der
  Reihenfolge des ersten Auftretens (für generische Iteration),
  `form_count` die Zahl der Einträge (Mehrfachwerte zählen einzeln).
  `form_encode` serialisiert in Eintragsreihenfolge: unreservierte
  Bytes bleiben, Leerzeichen wird `+`, alles andere `%XX`
  (Großbuchstaben). Parse → Encode → Parse erhält Namen, Werte,
  Mehrfachwerte und Reihenfolge.
- **Formulargrammatik:** `&` trennt Einträge, das **erste** `=` trennt
  Name und Wert, `+` ist ein Leerzeichen, Prozentkodierung wird genau
  einmal dekodiert, das Ergebnis muss gültiges UTF-8 sein. Leere
  Segmente (`&&`, führendes/abschließendes `&`) werden übersprungen;
  ein Eintrag ohne `=` hat einen leeren Wert; leere Namen sind
  zulässig und verschieden von „Feld fehlt“. Ungültige Escapes
  (`%`, `%0`, `%GG`) und ungültiges UTF-8 nach der Dekodierung sind
  Fehler. `%2F` erzeugt in Formularen keinen Trenner (im Gegensatz zur
  Pfadsegmentierung, die unverändert bleibt); `+` gilt nur hier als
  Leerzeichen, nicht in Pfaden.
- **Formular-Content-Type:** `form_body_request` akzeptiert
  `application/x-www-form-urlencoded` (Groß-/Kleinschreibung egal,
  Parameter erlaubt). Ein explizites `charset` muss UTF-8 sein. Fehlt
  der Header oder ist der Medientyp ein anderer, ist das ein `Err`.
  Ein leerer Body ist ein gültiges, leeres Formular.
- **Formularlimits (erzwungen):** Body 1 MiB (`BLOOM_FORM_MAX_BYTES`),
  höchstens 4096 Einträge, Feldname 256 Bytes, Feldwert 64 KiB (jeweils
  nach der Dekodierung geprüft). Der Body wird schon von
  `http::read_request` auf 4 MiB begrenzt; `form_body`/`form_body_request`
  prüfen zusätzlich die 1-MiB-Formulargrenze, bevor geparst wird.
  `form_encode` verweigert Ergebnisse über 1 MiB.
- **Sicherheit:** Formularwerte sind **nicht vertrauenswürdig**.
  Dekodieren ist keine Validierung, kein Beweis für Authentizität und
  keine Autorisierung; Feldnamen sagen nichts über die Herkunft eines
  Wertes. Die API setzt keine CSRF-Tokens und keine Sessions um, und
  URL-Kodierung schützt nicht vor SQL-Injection oder anderen
  kontextabhängigen Injection-Problemen. HTML-Templates escapen Werte
  weiterhin automatisch; es gibt keinen neuen Rohausgabe-Pfad.
  `FormData` ist ein verwalteter Besitzwert (Referenzzählung, kein
  manuelles Freigeben).
- **Cookies lesen:** `cookies(request)` liest **alle** `Cookie`-Header in
  Reihenfolge. Paare werden an `;` getrennt, das erste `=` trennt Name
  und Wert, Leerraum um beide wird entfernt. Werte werden **wörtlich**
  zurückgegeben: keine Prozentdekodierung, kein Entfernen von
  Anführungszeichen. Namen müssen RFC-Token sein (1..64 Bytes), Werte
  druckbares ASCII ohne `"`, `,`, `;`, `\` (max. 4096 Bytes); ungültige
  Paare werden ignoriert, statt die ganze Anfrage scheitern zu lassen.
  Mehrere `Cookie`-Header werden zusammengeführt; insgesamt höchstens 64
  Cookies, weitere werden ignoriert. `cookie_get` verlangt genau einen
  Wert: ein fehlender **oder mehrfach** vorkommender Name ist ein `Err`
  (doppelte Cookies können z. B. durch Cookie-Tossing entstehen und
  werden nie stillschweigend auf den ersten Wert reduziert).
  `cookie_first` liefert den ersten Wert, `cookie_names` die
  verschiedenen Namen in Reihenfolge des ersten Auftretens.
- **`Set-Cookie` erzeugen:** `set_cookie` baut aus
  `CookieOptions` eine validierte Kopfzeile der Form
  `Set-Cookie: name=value[; Path=…][; Max-Age=…][; HttpOnly][; Secure][; SameSite=…]`.
  Name/Wert/Pfad werden geprüft; CR/LF, Steuerbytes, `;` in Werten und
  ungültige Attributwerte sind Fehler. `Secure` ist der Standard und
  lässt sich nur über das ausdrücklich benannte `insecure_http`-Feld
  weglassen (nur für Entwicklung über HTTP). `SameSite=None` ohne
  `Secure` wird abgelehnt, ebenso unbekannte SameSite-Werte. Ein
  `Domain`-Attribut gibt es bewusst nicht (kein Domain-Scope).
  `delete_cookie(name, path, insecure_http)` setzt denselben Namen und
  Pfad mit leerem Wert und `Max-Age=0`; der Pfad muss beim Setzen und
  Löschen übereinstimmen. `cookie_headers([...])` verbindet mehrere
  solcher Zeilen mit CRLF zu einem Header-Block (jede Zeile wird erneut
  geprüft). `std::http` schreibt jede Header-Zeile einzeln, daher
  bleiben mehrere `Set-Cookie`-Header als getrennte Zeilen erhalten.
- **Sessions:** `session_store(max_sessions, ttl_seconds)` erzeugt einen
  reinen In-Memory-Store (max. 1..65535 Sessions, TTL 1..31536000
  Sekunden **absolut** ab Erzeugung). `session_new` erzeugt eine
  Session mit **32 zufälligen Bytes** aus der Betriebssystem-CSPRNG
  (Windows `BCryptGenRandom`, POSIX `/dev/urandom`) als Base64url ohne
  Padding (43 Zeichen); schlägt die Zufallsquelle fehl, ist das ein
  `Err` und es gibt keinen unsicheren Ersatz. `session_load` liest den
  benannten Cookie, lehnt fehlende, doppelte, malformed, unbekannte und
  abgelaufene IDs ab; die Anwendung entscheidet selbst, ob sie dann
  `session_new` aufruft (und weiß dadurch, ob eine Session
  wiederaufgenommen wurde). `session_data` liefert das gemeinsame
  JSON-Objekt (Änderungen sind für alle Handles sichtbar).
  `session_destroy` entfernt die Session aus dem Store; bestehende
  Handles behalten ihre Daten bis zum Scope-Ende. `session_expire`
  entfernt abgelaufene Sessions und liefert die Anzahl;
  `session_save` gibt es nicht, die Daten liegen direkt im Store.
  Ist der Store voll, schlägt `session_new` fehl, statt eine aktive
  Session zu verdrängen. Ablauf wird mit der **monotonen** Uhr
  berechnet; ein Sprung der Systemuhr verlängert oder verkürzt nichts.
  `session_count` liefert die Zahl der aktuell gespeicherten Einträge
  (abgelaufene werden bei `session_new`, `session_load` und
  `session_expire` entfernt). `session_registered(store, session)`
  beantwortet, ob ein Handle noch **im Store registriert** ist:
  `true` nur für lebende, nicht abgelaufene Sessions. Nach
  `session_destroy` oder Ablauf liefert es `false`, auch wenn das
  Handle seine Daten noch hält. Ein Handle ist damit ein Datenobjekt,
  keine Gültigkeitsbescheinigung: Für jede Anfrage muss die Anwendung
  den Cookie erneut mit `session_load` auflösen. Ein nicht mehr
  registriertes Handle kann nicht wieder in den Store eingefügt werden
  (eine solche Funktion gibt es nicht); `session_new` erzeugt immer
  eine neue Kennung. Sessions gehen beim Prozessende verloren; einen
  verteilten Store gibt es nicht.
- **Session-Rotation:** `session_regenerate(store, session)` ersetzt eine
  **registrierte** Session durch eine neue: neue 32-Byte-ID, neuer
  CSRF-Token, dieselben Daten (dasselbe JSON-Objekt) und dieselbe
  absolute Ablaufzeit. Die alte ID wird sofort aus dem Store entfernt
  (`session_load` damit schlägt fehl); das alte Handle behält seine
  Daten, ist aber nicht mehr registriert. Ist die Session bereits
  abgelaufen oder gelöscht oder schlägt die Zufallsquelle fehl, ändert
  sich nichts und das Ergebnis ist `Err`. Anwendungen rufen das nach
  erfolgreicher Anmeldung auf (macht `auth::login` bereits) oder wenn
  Rechte wechseln, um Session Fixation zu verhindern.
- **Session-Cookies:** `session_cookie(session, name, max_age)` erzeugt
  eine `Set-Cookie`-Zeile mit `Path=/`, `HttpOnly`, `Secure`,
  `SameSite=Lax`, `Max-Age=max_age` (1..31536000).
  `session_cookie_insecure` lässt `Secure` bewusst weg und ist nur für
  lokale HTTP-Entwicklung gedacht.
- **CSRF:** `csrf_token(session)` liefert das Token der Session und
  erzeugt beim ersten Aufruf 32 zufällige Bytes (Base64url,
  43 Zeichen); `csrf_check` vergleicht in konstanter Zeit (bei gleicher
  Länge) und schlägt für fehlende Tokens und Tokens anderer Sessions
  fehl; `csrf_rotate` erzeugt ein neues Token, `csrf_clear` verwirft
  es. Der Token liegt getrennt von den Anwendungsdaten in der Session.
  Ein CSRF-Token ist **keine** Authentifizierung und keine
  Autorisierung; Anwendungen müssen `csrf_check` bei jedem
  zustandsverändernden Request selbst aufrufen. `SameSite` ist nur
  eine zusätzliche Schutzschicht.
- **Sicherheit (Cookies/Sessions):** Cookie- und Sessionwerte sind
  nicht vertrauenswürdig; Dekodieren ist keine Validierung.
  Session-IDs dürfen nur über `Secure`-Cookies übertragen werden (außer
  bewusst in der lokalen Entwicklung), und die In-Memory-Sessions sind
  nicht threadsicher (Bloom hat keine Threads).

### 13.13 Standardmodul `std::html` (Escaping und Templates)

```bloom
use std::html;
use std::json;

let data = json::make_object();
json::set(data, "name", json::make_string("Ada <admin>"));

println(html::escape("<b>Tom & Jerry</b>"));           // &lt;b&gt;Tom &amp; Jerry&lt;/b&gt;
let t = html::template("<h1>{{name}}</h1>")?;
println(html::render(t, data)?);                        // <h1>Ada &lt;admin&gt;</h1>
html::render_page(conn, 200, t, data);                  // text/html; charset=utf-8
```

| Funktion | Signatur |
| --- | --- |
| `escape(text)` | `string -> string` |
| `template(source)` | `string -> Result<Template, string>` |
| `render(template, data)` | `Template, JsonValue -> Result<string, string>` |
| `render_page(connection, status, template, data)` | `int, int, Template, JsonValue -> Result<bool, string>` |
| `render_page_with(connection, status, template, data, headers)` | `int, int, Template, JsonValue, string -> Result<bool, string>` |
| `template_file(path)` | `string -> Result<Template, string>` |
| `template_file_from(root, relative)` | `string, string -> Result<Template, string>` |

```bloom
pub struct Template { id }   // undurchsichtiges, verwaltetes Handle
```

- **Escaping:** `escape` maskiert `&` → `&amp;`, `<` → `&lt;`,
  `>` → `&gt;`, `"` → `&quot;`, `'` → `&#39;`; alle anderen Bytes
  (inkl. UTF-8) bleiben unverändert, die Ausgabe wird nie abgeschnitten
  (Puffergröße 6 × Eingabe + 1). **Grenze:** Escaping schützt HTML-Text
  und quotierte Attributwerte; es schützt **nicht** JavaScript-,
  CSS- oder URL-Kontexte und keine unquotierten Attribute. Raw-HTML
  gibt es nicht.
- **Templatesyntax:** `{{pfad}}` (Pfad: `name` oder `a.b.c`, Segmente
  `[A-Za-z_][A-Za-z0-9_]*`, höchstens 32 Segmente/256 Bytes);
  `{{#if pfad}} … {{else}} … {{/if}}` (Wert muss `bool` sein);
  `{{#each pfad}} … {{/each}}` (Wert muss ein Array sein; `{{this}}`
  bzw. `{{this.feld}}` adressiert das aktuelle Element, auch in
  verschachtelten Schleifen nur das innerste). Direktiven sind genau
  `if`, `each` und `else`; alles andere ist ein Parserfehler.
- **Werte:** Strings werden escaped eingesetzt, Zahlen mit ihrem
  Original-Literal (geparst) bzw. als Dezimaltext (konstruiert),
  Booleans als `true`/`false`. `null`, Arrays und Objekte sind
  **Renderfehler** (keine Debug-Ausgaben), ebenso fehlende Pfade,
  `this` außerhalb von `#each` und falsche Typen in `#if`/`#each`.
  Fehlende Werte werden bewusst nicht stillschweigend zu leerem Text.
- **Parsen vs. Rendern:** `template` parst einmal und liefert ein
  verwaltetes `Template` (Referenzzählung, kein manuelles Freigeben,
  beliebig viele gleichzeitige Instanzen); `render` ist zustandslos,
  verändert die Daten nicht und kann dieselbe Vorlage beliebig oft
  rendern. Ein fehlgeschlagenes Parsen liefert `Err`; ein
  fehlgeschlagenes Rendern liefert `Err`, ohne den Ausgabepuffer zu
  behalten.
- **Fehler:** Parserfehler nennen Ursache sowie Byte-/Zeilen-/Spalte
  (`… at byte N (line L, column C)`); Renderfehler nennen den Pfad
  (`render: missing value for \`a.b\``). `<script>`- und
  `<style>`-Blöcke werden beim Parsen abgelehnt (keine
  JavaScript/CSS-Kontexte in Vorlagen).
- **Limits (erzwungen):** Quelle 65536 Bytes, höchstens 16384
  Vorlagenteile, Verschachtelung 32, Ausgabe 1048576 Bytes,
  Schleifeniterationen insgesamt 65536 (auch verschachtelt), Pfad
  256 Bytes. Der Parser ist iterativ (kein C-Stack-Wachstum durch
  Verschachtelung), Größenberechnungen sind überlaufgeprüft.
- **HTTP:** `render_page`/`render_page_with` rendern **vollständig vor
  dem Schreiben** (ein Renderfehler erzeugt keine halbe Antwort) und
  senden über `write_response_full` mit
  `Content-Type: text/html; charset=utf-8`; Zusatz-Header werden
  validiert (CR/LF-Injection ausgeschlossen, Framing-Header gesperrt),
  `204`/`304` erhalten keinen Body. Routing, Authentifizierung und
  Sessions bleiben Aufgabe der Anwendung (`std::web`).
- **Dateien:** `template_file(path)` liest die Vorlage über
  `fs::read_text_limited` (Grenze `BLOOM_HTML_MAX_SOURCE` = 65536
  Bytes; die Größe wird vor dem Lesen geprüft) und parst sie mit
  demselben Parser wie `template`. `template_file_from(root,
  relative)` validiert den relativen Pfad (`..`, absolute Pfade,
  Laufwerks-/UNC-Präfixe und leere Segmente sind Fehler), öffnet die
  Datei mit Wurzelordner-Schutz (kanonischer Vergleich nach dem
  Öffnen, Symlinks/Junctions inklusive) und prüft UTF-8 vor dem
  Parsen. Vorlagen werden **einmal** geladen; es gibt kein Neuladen
  oder Beobachten von Dateien. Der Fehlertext beginnt mit
  `template_file:` bzw. `template_file_from:` und nennt den Pfad und
  den Grund.

### 13.14 Standardmodul `std::db` (SQLite)

```bloom
use std::db;

let database = db::open("app.db")?;
let stmt = db::prepare(database, "INSERT INTO notes (text) VALUES (?1)")?;
db::bind_text(stmt, 1, "Hallo")?;
db::execute(stmt)?;
```

| Funktion | Signatur |
| --- | --- |
| `open(path)` | `string -> Result<Database, string>` |
| `open_memory()` | `() -> Result<Database, string>` |
| `busy_timeout(database, ms)` | `Database, int -> Result<bool, string>` |
| `prepare(database, sql)` | `Database, string -> Result<Statement, string>` |
| `bind_null(stmt, index)` | `Statement, int -> Result<bool, string>` |
| `bind_int(stmt, index, value)` | `Statement, int, int -> Result<bool, string>` |
| `bind_float(stmt, index, value)` | `Statement, int, float -> Result<bool, string>` |
| `bind_bool(stmt, index, value)` | `Statement, int, bool -> Result<bool, string>` |
| `bind_text(stmt, index, value)` | `Statement, int, string -> Result<bool, string>` |
| `bind_bytes(stmt, index, value)` | `Statement, int, fs::Bytes -> Result<bool, string>` |
| `step(stmt)` | `Statement -> Result<bool, string>` |
| `execute(stmt)` | `Statement -> Result<int, string>` |
| `reset(stmt)` | `Statement -> Result<bool, string>` |
| `column_count(stmt)` | `Statement -> int` |
| `column_name(stmt, index)` | `Statement, int -> Result<string, string>` |
| `column_index(stmt, name)` | `Statement, string -> Result<int, string>` |
| `column_type(stmt, index)` | `Statement, int -> Result<string, string>` |
| `column_int(stmt, index)` | `Statement, int -> Result<int, string>` |
| `column_float(stmt, index)` | `Statement, int -> Result<float, string>` |
| `column_bool(stmt, index)` | `Statement, int -> Result<bool, string>` |
| `column_text(stmt, index)` | `Statement, int -> Result<string, string>` |
| `column_bytes(stmt, index)` | `Statement, int -> Result<fs::Bytes, string>` |
| `changes(database)` | `Database -> int` |
| `last_insert_rowid(database)` | `Database -> int` |
| `begin(database)` / `commit(database)` / `rollback(database)` | `Database -> Result<bool, string>` |

```bloom
pub struct Database { id }   // verwaltetes Handle
pub struct Statement { id }  // verwaltetes Handle, hält die Datenbank am Leben
```

- **Engine:** eingebettetes **SQLite 3.54.1** (Amalgamation,
  `runtime/sqlite3.c`/`.h`, Public Domain, siehe
  `runtime/sqlite3_NOTICE.md`). Der Compiler schreibt die Amalgamation
  bei Programmen mit `std::db` neben den generierten C-Code und
  übersetzt sie als eigene Übersetzungseinheit mit
  `SQLITE_THREADSAFE=0`, `SQLITE_OMIT_LOAD_EXTENSION` und
  `SQLITE_DQS=0`. Kein separater Datenbankserver, keine Installation,
  keine Netzwerk-Downloads zur Build- oder Laufzeit.
- **Indizes:** Bind-Parameter sind **1-basiert** (`?1` = Index 1,
  Bereich 1..`sqlite3_bind_parameter_count`). Spalten sind **0-basiert**
  (Bereich 0..`column_count-1`). Beides entspricht SQLite und wird bei
  Verletzung mit einem `Err` abgelehnt.
- **Statement-Zustände:** `fresh` (nach `prepare` oder `reset`), `row`
  (letzter `step` lieferte eine Zeile), `done` (letzter `step`/`execute`
  ist fertig), `error` (letzter Aufruf schlug fehl). Binden ist nur in
  `fresh` erlaubt; Spaltenzugriffe nur in `row`; `step` nur in
  `fresh`/`row`; `execute` nur in `fresh`; `reset` ist aus jedem Zustand
  erlaubt, setzt den Zustand auf `fresh` und löscht **alle Bindungen**.
  Nach einem Fehler muss `reset` aufgerufen werden, bevor das Statement
  weiterverwendet wird (auch nach `SQLITE_BUSY`, damit die implizite
  Transaktion sauber beendet wird).
- **`step` vs. `execute`:** `step` liefert `true`, wenn eine Zeile
  bereitsteht, und `false`, wenn die Anweisung abgeschlossen ist.
  `execute` ist für Anweisungen ohne Ergebnismenge und liefert
  `sqlite3_changes`; taucht eine Zeile auf, ist das ein `Err` (Hinweis
  auf `step`).
- **Prepared Statements:** `prepare` akzeptiert **genau eine**
  SQL-Anweisung; weitere Anweisungen (auch nach einem Semikolon) sind
  ein Fehler. SQL mit eingebettetem NUL-Byte wird abgelehnt. Werte
  werden ausschließlich über `bind_*` übergeben — es gibt keine
  SQL-Stringinterpolation in Bloom und Parameter ersetzen keine
  Tabellen- oder Spaltennamen.
- **Typen:** `INTEGER` ist 64 Bit (`int`), `REAL` ist IEEE-754-Double
  (`float`), `TEXT` ist UTF-8 (`string`, inklusive eingebetteter
  NUL-Bytes), `BLOB` ist `fs::Bytes` (NUL-sicher, nie als C-String
  interpretiert), `NULL` ist ein eigener Zustand. Die typisierten
  Getter sind **strikt**: `column_int` akzeptiert nur INTEGER,
  `column_text` nur TEXT, `column_bytes` nur BLOB, `column_bool` nur
  INTEGER 0/1, `column_float` nur REAL. Eine falsche Zieltype ist ein
  `Err` mit erwartetem und tatsächlichem Typ. `column_type` liefert
  `"null"`, `"integer"`, `"float"`, `"text"` oder `"blob"`. `NULL`,
  leerer String und fehlende Spalten sind dadurch unterscheidbar.
  Ungebundene Parameter sind in SQLite `NULL` (kein Fehler).
- **Spaltennamen:** `column_name`/`column_index` vergleichen exakt
  (Groß-/Kleinschreibung zählt). `column_index` lehnt mehrdeutige
  Namen (mehr als eine Spalte mit dem Namen) und unbekannte Namen ab.
- **Transaktionen:** `begin`, `commit`, `rollback` sind explizit;
  verschachteltes `begin` ist ein Fehler (SQLite-Savepoints werden
  nicht angeboten). Ein offener Transaktionszustand wird beim
  Freigeben der Datenbank zurückgerollt (`sqlite3_close_v2`).
- **Ressourcen:** Ein `Statement` hält seine `Database` am Leben
  (Referenzzählung); es darf die ursprüngliche Bloom-Variable
  überleben. `Database`/`Statement` sind verwaltete Werte. Ein Fehler
  beim Präparieren/Binden/Ausführen gibt alle Teilressourcen frei.
- **Dateipfade:** `open` nimmt einen normalen Dateipfad (UTF-8,
  Unicode und Leerzeichen funktionieren unter Windows). SQLite-URIs
  werden nicht aktiviert; die Sonderdatei `:memory:` funktioniert, dafür
  gibt es `open_memory`. Ein Verzeichnis oder ein nicht existierendes
  Elternverzeichnis ist ein `Err`. Eine schreibgeschützte Datei lässt
  sich öffnen; schreibende Anweisungen scheitern dann mit
  `SQLITE_READONLY`.
- **Betrieb:** Standard-Busy-Timeout 5000 ms, einstellbar mit
  `busy_timeout` (0..60000 ms). Fehlertexte nennen die Operation, die
  SQLite-Meldung und den SQLite-Code (z. B.
  `execute: database is locked (SQLite code 5)`); SQL-Text und
  Dateipfade werden nicht zusätzlich wiederholt. Keine automatischen
  Wiederholungen.
- **Grenzen:** SQL-Text höchstens `INT32_MAX` Bytes, Text/BLOB bis zur
  SQLite-Grenze (1 GiB). Keine Threads; das Handle ist nicht für
  parallele Nutzung ausgelegt. Kein ORM, keine Migrationen, keine
  Connection-Pools, kein PostgreSQL/MySQL, keine asynchronen Zugriffe.
- **Sicherheit:** Parametrisierte Abfragen sind der einzige
  vorgesehene Weg für Werte. Dynamische Bezeichner (Tabellen- oder
  Spaltennamen) müssen Anwendungen selbst aus einer festen Auswahl
  wählen; sie lassen sich nicht binden. Datenbankinhalte sind
  potenziell angreifbar und müssen beim Ausgeben weiterhin escaped
  werden (HTML-Templates tun das automatisch).

### 13.15 Standardmodul `std::auth` (Konten, Rollen, Anmeldung)

```bloom
use std::auth;
use std::db;

let database = db::open("app.db")?;
auth::setup(database)?;
let id = auth::register_user(database, "Ada", "ein langes Passwort")?;
let login = auth::authenticate(database, "ada", "ein langes Passwort")?;
```

| Funktion | Signatur |
| --- | --- |
| `password_hash(password)` | `string -> Result<string, string>` |
| `password_verify(password, hash)` | `string, string -> Result<bool, string>` |
| `password_needs_rehash(hash)` | `string -> Result<bool, string>` |
| `setup(database)` | `Database -> Result<bool, string>` |
| `register_user(database, username, password)` | `Database, string, string -> Result<int, string>` |
| `authenticate(database, username, password)` | `Database, string, string -> Result<int, string>` |
| `user_find(database, username)` | `Database, string -> Result<int, string>` |
| `user_get(database, user_id)` | `Database, int -> Result<UserInfo, string>` |
| `user_disable(database, user_id)` / `user_enable(database, user_id)` | `Database, int -> Result<bool, string>` |
| `password_change(database, user_id, new_password)` | `Database, int, string -> Result<bool, string>` |
| `login(store, session, database, username, password)` | `SessionStore, Session, Database, string, string -> Result<Session, string>` |
| `logout(store, session)` | `SessionStore, Session -> Result<bool, string>` |
| `current_user(database, store, session)` | `Database, SessionStore, Session -> Result<Option<UserInfo>, string>` |
| `role_create(database, role)` / `role_delete(database, role)` | `Database, string -> Result<bool, string>` |
| `role_assign(database, user_id, role)` / `role_remove(database, user_id, role)` | `Database, int, string -> Result<bool, string>` |
| `roles_of(database, user_id)` | `Database, int -> Result<[string], string>` |
| `has_role(database, user_id, role)` | `Database, int, string -> Result<bool, string>` |
| `limiter(max_entries, window_seconds, max_attempts)` | `int, int, int -> Result<Limiter, string>` |
| `limiter_check(limiter, key)` | `Limiter, string -> Result<bool, string>` |
| `limiter_reset(limiter, key)` | `Limiter, string -> bool` |
| `limiter_count(limiter)` / `limiter_expire(limiter)` | `Limiter -> int` |

```bloom
pub struct UserInfo { id: int, username: string, display_name: string, active: bool, auth_version: int }
pub struct Limiter { id }   // verwaltetes Handle
```

- **Argon2id:** eingebettete Referenzimplementierung **20190702**
  (`runtime/argon2/`, CC0/Apache-2.0, Hinweise in
  `runtime/argon2_NOTICE.md`), ohne Threads kompiliert. Standardparameter:
  `m=65536` KiB (64 MiB), `t=3`, `p=1`, Salt 16 Bytes, Tag 32 Bytes
  (RFC 9106, zweite Empfehlung mit p=1; mindestens die OWASP-Untergrenze).
  Gemessen auf einem AMD Ryzen 7 5800X unter Windows 11: etwa
  **115–120 ms** pro Hash (MSVC-Release, 5 Läufe, Einzelprozess).
- **Hashformat:** PHC-String
  `$argon2id$v=19$m=65536,t=3,p=1$<salt-base64>$<hash-base64>`
  (Standard-Base64 ohne Padding). Jedes Passwort bekommt ein eigenes
  Zufalls-Salt; der Hash enthält nie Klartext.
- **Verifikation:** `password_verify` prüft den PHC-String **vor** dem
  Aufruf der Bibliothek: nur Argon2id, Version 19, `8 MiB <= m <= 256 MiB`,
  `1 <= t <= 10`, `1 <= p <= 4`, Salt 8..64 Bytes, Tag 16..64 Bytes,
  höchstens 512 Zeichen. Unplausible Parameter werden abgelehnt, ohne
  Speicher zu reservieren. Der Vergleich läuft über `argon2id_verify`
  (konstante Zeit in der Bibliothek). `password_needs_rehash` liefert
  `true`, wenn Version oder Parameter vom Standard abweichen (z. B. für
  den bekannten Testvektor mit `t=2`).
- **Passwortregeln:** Kein Trimmen, keine Normalisierung, kein
  Abschneiden. Neue Passwörter: **8..1024 Bytes** (Bytes, nicht Zeichen);
  keine Zeichenklassen-Pflicht. `password_verify` akzeptiert 0..1024
  Bytes, damit alte gültige Passwörter weiter funktionieren.
  `password_hash`/`password_change`/`register_user` wenden die
  Mindestregel an.
- **Schema (Version 1):** eigene Tabellen `bloom_auth_meta`
  (`schema_version`), `bloom_auth_users` (id, eindeutiger normalisierter
  `username`, `display_name`, `password_hash`, `active`,
  `auth_version`, `created_at`), `bloom_auth_roles` und
  `bloom_auth_user_roles` (Primärschlüssel user_id+role). `setup` ist
  idempotent und läuft in einer Transaktion. Eine fremde oder neuere
  Auth-Schema-Version wird abgelehnt (`unsupported auth schema version`),
  ebenso vorhandene `bloom_auth_*`-Tabellen ohne Metatabelle.
- **Benutzernamen:** ASCII, getrimmt, **case-insensitiv normalisiert**
  (klein), 3..32 Bytes, Zeichen `a-z 0-9 . _ -`, erstes Zeichen
  Buchstabe oder Ziffer. `display_name` behält die Schreibweise der
  Eingabe (getrimmt). Es gibt **keine** Unicode-Identitäts- oder
  Homoglyph-Behandlung. Doppelte normalisierte Namen verhindert der
  `UNIQUE`-Constraint (nicht nur ein vorheriges `SELECT`).
- **Registrierung:** prüft Name und Passwort vor dem Hashen, hasht
  außerhalb der Einfügeoperation und schreibt genau eine Zeile. Ein
  Duplikat wird als `username is already taken` gemeldet; es entstehen
  keine Teilzeilen. Alle Werte sind gebunden (keine SQL-Interpolation).
- **Anmeldung:** `authenticate` normalisiert den Namen, lädt Hash/Status/
  Version und verifiziert. Unbekannter Name und falsches Passwort liefern
  **dieselbe** Meldung `invalid credentials`; für unbekannte Namen wird
  derselbe Argon2id-Aufwand mit einem Dummy-Hash ausgeführt. Deaktivierte
  Konten werden ebenfalls generisch abgelehnt. Fehler der Datenbank werden
  davon unterschieden (eigene `Err`-Texte). Perfekte Timing-Gleichheit
  wird nicht behauptet.
- **Sessions:** `login` verifiziert zuerst und ruft dann
  `web::session_regenerate` auf (neue ID, neuer CSRF-Token, gleiche Daten,
  gleiche absolute Lebensdauer); erst die neue Session gilt als
  angemeldet. Gespeichert werden nur `_auth_user_id` und
  `_auth_version` in den Sessiondaten — **keine Passwörter oder Hashes**.
  `current_user` prüft in dieser Reihenfolge: Session registriert (nicht
  abgelaufen), Schlüssel vorhanden, Benutzer existiert, aktiv und
  `auth_version` stimmt. Jede Abweichung ergibt `Ok(None)`
  (nicht angemeldet); Datenbankfehler ergeben `Err`. Ein lebendes
  Bloom-Handle ist keine gültige Anmeldung.
- **Invalidierung:** `password_change`, `user_disable` und `user_enable`
  erhöhen `auth_version`. Alle älteren Sessions werden dadurch beim
  nächsten `current_user` abgelehnt. `logout` löscht die serverseitige
  Session (`session_destroy`); der Cookie wird von der Anwendung mit
  `delete_cookie` entfernt. Logout und Passwortwechsel sind
  zustandsverändernd und müssen per `POST` mit CSRF-Prüfung laufen.
- **Rollen:** Rollennamen werden wie Benutzernamen normalisiert
  (1..64 Bytes). `role_create` ist idempotent (`false` = existierte
  bereits). Unbekannte Rollen werden bei `role_assign` abgelehnt und bei
  `has_role` als `false` behandelt — nie als Freigabe. `role_delete`
  entfernt die Rolle samt Zuordnungen in einer Transaktion. Es gibt
  **keinen** Standard-Admin und keine vorinstallierten Konten; Rollen
  werden von einem Operator/einem Setup-Programm vergeben. Eine bloße
  Session ist kein Berechtigungsnachweis.
- **Rate-Limiter:** `limiter(max_entries, window_seconds, max_attempts)`
  mit 1..65536 Einträgen, Fenster 1..86400 s und 1..1000 Versuchen pro
  Fenster. `limiter_check` zählt pro Schlüssel (1..256 Bytes) im festen
  Zeitfenster (monotone Uhr): `true` = erlaubt (und gezählt), `false` =
  gesperrt (nicht gezählt). Abgelaufene Einträge werden bei jedem Zugriff
  entfernt. Ist die Tabelle voll und ist nichts abgelaufen, wird der
  Eintrag mit dem ältesten Fenster ersetzt — der Speicher bleibt
  beschränkt; ein Flut neuer Schlüssel kann dadurch andere Zähler
  verdrängen. Der Limiter arbeitet **pro Prozess**, ist kein verteilter
  Schutz und ersetzt keine Authentifizierung. Die Web-API stellt keine
  vertrauenswürdige Client-IP bereit; der Schlüssel wird von der
  Anwendung gewählt (z. B. normalisierter Benutzername) und darf nicht
  ungeprüft aus clientgesteuerten Headern stammen.
- **Sicherheitsgrenzen:** `std::auth` schützt keine Route automatisch.
  Anwendungen müssen bei Login, Logout, Passwortwechsel und anderen
  Änderungen selbst CSRF prüfen (`csrf_check`), HTTPS verwenden und
  Session-Cookies mit `Secure` senden. Die Beispiele nutzen bewusst
  `insecure_http` nur für lokale HTTP-Demos. `SameSite` ersetzt keine
  CSRF-Prüfung. Passwort-Hashes und Sessiondaten dürfen nicht geloggt
  werden.

## 14. Fehlerdiagnostik

Format:

```text
error[E1041]: non-exhaustive match: the pattern `Color::Blue` is not covered
  --> src/main.blm:4:18
  |
4 |     return match c {
  |                  ^
  |
  = help: add an arm for `Color::Blue` or a wildcard arm `_ => ...`
```

Fehlercodes:

| Code | Bedeutung |
| --- | --- |
| E0001 | Unerwartetes Zeichen |
| E0002 | Nicht geschlossener String |
| E0003 | Ungültige oder zu große Zahl |
| E0005 | Nicht geschlossener Blockkommentar |
| E0006 | Ungültige Escape-Sequenz |
| E0007 | Ungültiges Zeichenliteral |
| E1001 | Unerwartetes Token (Syntaxfehler) |
| E1002 | Typfehler |
| E1003 | Unbekannte Variable |
| E1004 | Unbekannte Funktion |
| E1005 | Falsche Argumentanzahl |
| E1006 | Zuweisung an Unveränderliches |
| E1007 | Fehlende Rückgabe |
| E1008 | `break`/`continue` außerhalb einer Schleife |
| E1009 | Bedingung ist nicht `bool` |
| E1010 | Doppelte Deklaration |
| E1011 | Aufruf eines Nicht-Funktionswerts |
| E1012 | Ungültige Operanden |
| E1013 | Ungültiger Index / nicht indizierbar |
| E1014 | Kein konstanter Ausdruck |
| E1015 | Nicht unterstütztes Sprachkonstrukt |
| E1016 | Unbekannter Typ |
| E1017 | Ungültiges Zuweisungsziel |
| E1018 | Ungültige `return`-Anweisung |
| E1019 | Ungültiges Argument einer eingebauten Funktion |
| E1020 | Unbekanntes Struct-/Varianten-Feld |
| E1021 | Feld mehrfach initialisiert |
| E1022 | Fehlendes Feld im Literal |
| E1023 | Feldzugriff auf einen Nicht-Struct-Wert |
| E1024 | Rekursiver Struct-Wert |
| E1030 | Doppelte Methode / Methoden-Feld-Konflikt |
| E1031 | Unbekannte Methode |
| E1032 | Mutierende Methode auf unveränderlichem/temporärem Empfänger |
| E1033 | `impl` für unbekannten Typ / Enum |
| E1040 | Unbekannte Enum-Variante |
| E1041 | Nicht exhaustives `match` |
| E1042 | Unerreichbarer Match-Arm |
| E1043 | Ungültige Enum-Deklaration |
| E1044 | Ungültiges Pattern |
| E1045 | Typparameter nicht ableitbar |
| E1047 | Rekursives Enum |
| E1050 | Modul nicht gefunden |
| E1051 | Privates Symbol |
| E1052 | Doppelter/konfligierender Import |
| E1053 | Modulname-Konflikt |
| E1054 | Einstiegsmodul kann nicht importiert werden |
| E1055 | Unbekanntes Symbol im Modul |
| E1056 | Reservierter Bezeichner (`__`) |
| E1060 | `?`-Operand ist kein `Result` |
| E1061 | `?`-Fehlertyp passt nicht zum Rückgabetyp |
| E1070 | Aufruf einer externen Funktion außerhalb von `unsafe` |
| E1071 | Ungültige extern-Deklaration (ABI, Symbol) |
| E1072 | Nicht unterstützter FFI-Typ / ungültige ABI-Konvertierung |
| E2001 | `main` fehlt |
| E2002 | `main` hat falsche Signatur |
| E3001 | Datei-/E/A-Fehler |
| E3002 | Kein C-Compiler gefunden |
| E3003 | Native Kompilierung fehlgeschlagen (inkl. fehlender Symbole) |
| E3005 | Ung�ltige Link-Konfiguration (--link) |
| E4001 | Fehlerhafte Kommandozeile |

Diagnosen des Compilers sind Englisch, CLI-Meldungen Deutsch.

## 15. Exit-Codes

| Code | Bedeutung |
| --- | --- |
| 0 | Erfolg |
| 1 | Compilerfehler im Bloom-Quellcode |
| 2 | Fehlerhafte Kommandozeile |
| 3 | Kein C-Compiler gefunden |
| 4 | C-Kompilierung fehlgeschlagen |
| 5 | Datei-/E/A-Fehler |
| 70 | Laufzeitfehler im Bloom-Programm |

## 16. Ausdrücklich nicht unterstützt (Version 0.19.0)

- Referenzempfänger (`&self`/`&mut self`), `var self` auf Enums
- Klassen, Vererbung, Interfaces
- Rekursive Structs/Enums (keine Referenztypen)
- Arrays von Structs/Enums, verschachtelte Arrays, `Slice<Slice<T>>`
- Views auf Arrays von Structs/Enums (Arrays solcher Elemente existieren nicht)
- `push`/`pop`/`insert`/`remove`/`slice`/`join` auf `Slice<T>` (E1019)
- Range-Patterns, `?` in `void`-Funktionen
- Re-Exports ganzer Module (`pub use geometry;`), Modul-Aliasse,
  `super`/`self`-Pfade in `use`
- Closures, Iteratoren, Ausnahmen, Threads
- Verbundzuweisungen (`+=`, …), `++`/`--`
- Automatische Speicherfreigabe für `panic`-Pfade (Prozess endet direkt)
- Umgebungsvariablen, stdin-Weitergabe und Timeouts bei `std::process`
- Symlink-spezifische Dateisystemgarantien (siehe `std::fs`)
- **Cookies, Proxys, Redirects, HTTP/2, TLS-Server, CRL/OCSP-Prüfung,
  Callback-Handler, Middleware, JSON-Streaming, Template-Includes,
  Raw-HTML-Platzhalter, ausführbare Template-Ausdrücke und
  kontextabhängige JavaScript/CSS/URL-Escapierung**
- **Streaming/Range-Requests, ETag/Last-Modified, Verzeichnis-Indizes
  und automatisches Neuladen von Dateien bei `web::static_file` bzw.
  `html::template_file`**
- **Multipart-Formulare und Datei-Uploads, automatische
  Formularvalidierung** (Formulare sind reine
  `application/x-www-form-urlencoded`-Daten; alles Weitere ist Sache
  der Anwendung)
- **Persistente oder verteilte Sessions, Benutzerkonten,
  Passworthashes, Login-Frameworks, Rollen/Rechte, OAuth/OIDC, JWT,
  vollständige Middleware** (Sessions liegen nur im Speicher des
  Prozesses und sind keine Anmeldung)
- **Datenbanken außer SQLite, ORMs, Schema-Migrationen,
  Connection-Pools, asynchrone Datenbankzugriffe, Savepoints**
- **OAuth/OIDC, externe Login-Anbieter, E-Mail-Verifikation,
  Passwort-zurücksetzen per E-Mail, MFA/Passkeys, JWT als
  Session-Ersatz, verteilte Session-Stores, ABAC/Policy-Sprachen,
  automatische Rollenvererbung, Admin-Dashboards** (std::auth liefert
  die Grundlagen, keine vollständige Identitätsplattform)
- **C-Structs by value, Funktionszeiger/Callbacks, variadische
  C-Funktionen, Pointer-Arithmetik, Dereferenzierung von `rawptr`**
- **UDP, asynchrone/Thread-Netzwerkoperationen, Threads allgemein**
- Paketmanager, LSP

## 17. Beispielprojekt

`src/main.blm`:

```bloom
use shapes;
use networking::client;

fn main() -> int {
    let rect = shapes::Rectangle { width: 3, height: 4 };
    println(shapes::area(rect));

    let connection = client::new_client("localhost", 8080, rect);
    println(client::describe(connection));
    return 0;
}
```

`src/shapes.blm`:

```bloom
pub struct Rectangle {
    width: int,
    height: int
}

pub enum Shape {
    Circle(int),
    Square(int)
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

`src/networking/client.blm`:

```bloom
use shapes;

pub struct Client {
    host: string,
    port: int,
    reference: shapes::Rectangle
}

pub fn new_client(host: string, port: int, reference: shapes::Rectangle) -> Client {
    return Client { host: host, port: port, reference: reference };
}

pub fn describe(c: Client) -> string {
    return c.host + ":" + str(c.port) + " (" + str(shapes::area(c.reference)) + ")";
}
```

Vollständig lauffähig unter `examples/modules_project/`.
