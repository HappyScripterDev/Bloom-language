use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn bloom_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_bloom"))
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn example(name: &str) -> String {
    root()
        .join("examples")
        .join(name)
        .to_string_lossy()
        .to_string()
}

fn known_error(name: &str) -> String {
    root()
        .join("examples")
        .join("known_errors")
        .join(name)
        .to_string_lossy()
        .to_string()
}

struct CmdOutput {
    code: i32,
    stdout: String,
    stderr: String,
}

impl CmdOutput {
    fn combined(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }
}

fn run_bloom(args: &[&str]) -> CmdOutput {
    let output = Command::new(bloom_bin())
        .args(args)
        .output()
        .expect("failed to run bloom");
    CmdOutput {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    }
}

fn run_bloom_in(dir: &PathBuf, args: &[&str]) -> CmdOutput {
    let output = Command::new(bloom_bin())
        .args(args)
        .current_dir(dir)
        .output()
        .expect("failed to run bloom");
    CmdOutput {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    }
}

fn run_bloom_env(args: &[&str], envs: &[(&str, &str)]) -> CmdOutput {
    let mut cmd = Command::new(bloom_bin());
    cmd.args(args);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let output = cmd.output().expect("failed to run bloom");
    CmdOutput {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    }
}

fn run_bloom_env_in(dir: &std::path::Path, args: &[&str], envs: &[(&str, &str)]) -> CmdOutput {
    let mut cmd = Command::new(bloom_bin());
    cmd.args(args);
    cmd.current_dir(dir);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let output = cmd.output().expect("failed to run bloom");
    CmdOutput {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    }
}

fn run_bloom_with_stdin(args: &[&str], input: &str) -> CmdOutput {
    let mut child = Command::new(bloom_bin())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn bloom");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait for bloom");
    CmdOutput {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    }
}

fn no_toolchain(o: &CmdOutput) -> bool {
    o.code == 3
}

fn norm(s: &str) -> String {
    s.replace("\r\n", "\n")
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bloom-it-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn write_file(path: &PathBuf, content: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(path, content).expect("write file");
}

fn write_bytes(path: &PathBuf, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(path, bytes).expect("write bytes");
}

#[test]
fn version_is_printed() {
    let out = run_bloom(&["--version"]);
    assert_eq!(out.code, 0);
    assert!(out.stdout.contains("bloom"), "{}", out.stdout);
    assert!(out.stdout.contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn help_is_printed() {
    let out = run_bloom(&["--help"]);
    assert_eq!(out.code, 0);
    assert!(out.stdout.contains("build"));
    assert!(out.stdout.contains("run"));
    assert!(out.stdout.contains("check"));
    assert!(out.stdout.contains("init"));
}

#[test]
fn unknown_command_fails() {
    let out = run_bloom(&["frobnicate"]);
    assert_eq!(out.code, 2);
    assert!(out.combined().contains("unbekanntes Kommando"));
}

#[test]
fn fmt_formats_file_and_check_mode() {
    let dir = temp_dir("fmt-basic");
    let file = dir.join("f.blm");
    write_file(&file, "fn  main( )->int{let x=1+2;return x;}");
    // --check on an unformatted file: reports and exits non-zero, no change.
    let check = run_bloom(&["fmt", "--check", file.to_string_lossy().as_ref()]);
    assert_eq!(check.code, 1, "{}", check.combined());
    let unchanged = std::fs::read_to_string(&file).expect("read");
    assert_eq!(unchanged, "fn  main( )->int{let x=1+2;return x;}");
    // Formatting writes the canonical text.
    let run = run_bloom(&["fmt", file.to_string_lossy().as_ref()]);
    assert_eq!(run.code, 0, "{}", run.combined());
    let formatted = std::fs::read_to_string(&file).expect("read");
    assert_eq!(
        formatted,
        "fn main() -> int {\n    let x = 1 + 2;\n    return x;\n}\n"
    );
    // Formatting again changes nothing; --check now passes.
    let again = run_bloom(&["fmt", file.to_string_lossy().as_ref()]);
    assert_eq!(again.code, 0, "{}", again.combined());
    let same = std::fs::read_to_string(&file).expect("read");
    assert_eq!(same, formatted);
    let check2 = run_bloom(&["fmt", "--check", file.to_string_lossy().as_ref()]);
    assert_eq!(check2.code, 0, "{}", check2.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn missing_input_file_fails() {
    let out = run_bloom(&["check", "does_not_exist.blm"]);
    assert_ne!(out.code, 0);
    assert!(out.combined().contains("nicht gefunden"));
}

#[test]
fn check_accepts_all_examples() {
    let names = [
        "hello.blm",
        "variables.blm",
        "functions.blm",
        "loops.blm",
        "arrays.blm",
        "calculator.blm",
        "fizzbuzz.blm",
        "primes.blm",
        "strings.blm",
        "constants.blm",
        "structs.blm",
        "methods.blm",
        "enums.blm",
        "option_result.blm",
        "generics.blm",
        "enum_methods.blm",
        "result_try.blm",
        "text_tools.blm",
        "collections.blm",
        "slices.blm",
        "file_io.blm",
        "process.blm",
        "time.blm",
        "ffi_basic.blm",
        "win32_info.blm",
        "tcp_client.blm",
        "tcp_server.blm",
        "http_get.blm",
        "http_server.blm",
        "https_client.blm",
        "json_parse.blm",
        "json_create.blm",
        "json_pretty.blm",
        "json_file_config.blm",
        "path_operations.blm",
        "web_router.blm",
        "web_path_params.blm",
        "web_json_api.blm",
        "web_errors.blm",
        "web_client_json.blm",
        "html_escape.blm",
        "template_basic.blm",
        "template_conditions.blm",
        "template_lists.blm",
        "web_html_page.blm",
        "template_errors.blm",
    ];
    for name in names {
        let out = run_bloom(&["check", &example(name)]);
        assert_eq!(out.code, 0, "check failed for {}: {}", name, out.combined());
        assert!(out.stdout.contains("OK"));
    }
}

#[test]
fn check_reports_type_mismatch() {
    let out = run_bloom(&["check", &known_error("type_mismatch.blm")]);
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("E1002"));
    assert!(out.stderr.contains("expected `int`, found `string`"));
    assert!(out.stderr.contains("type_mismatch.blm:2"));
}

#[test]
fn check_reports_undefined_variable() {
    let out = run_bloom(&["check", &known_error("undefined_variable.blm")]);
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("E1003"));
    assert!(out.stderr.contains("missing_variable"));
}

#[test]
fn check_reports_missing_return() {
    let out = run_bloom(&["check", &known_error("missing_return.blm")]);
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("E1007"));
}

#[test]
fn check_reports_immutable_assignment() {
    let out = run_bloom(&["check", &known_error("immutable_assignment.blm")]);
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("E1006"));
}

#[test]
fn check_reports_syntax_error_with_position() {
    let dir = temp_dir("syntax");
    let file = dir.join("broken.blm");
    write_file(
        &file,
        "fn main() -> int {\n    let x = ;\n    return 0;\n}\n",
    );
    let out = run_bloom(&["check", file.to_string_lossy().as_ref()]);
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("E1001"));
    assert!(out.stderr.contains("broken.blm:2"));
}

#[test]
fn run_hello() {
    let out = run_bloom(&["run", &example("hello.blm")]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "Hello from Bloom!\n");
}

#[test]
fn run_functions_example() {
    let out = run_bloom(&["run", &example("functions.blm")]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(stdout.contains("42\n"));
    assert!(stdout.contains("3628800\n"));
    assert!(stdout.contains("Hello, Bloom!"));
}

#[test]
fn run_loops_example() {
    let out = run_bloom(&["run", &example("loops.blm")]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(stdout.starts_with("while:\n0\n1\n2\n3\n4\n"));
    assert!(stdout.contains("break/continue:\n1\n3\n5\n7\n9\n"));
}

#[test]
fn run_fizzbuzz_example() {
    let out = run_bloom(&["run", &example("fizzbuzz.blm")]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let lines: Vec<&str> = out.stdout.lines().collect();
    assert_eq!(lines.len(), 15);
    assert_eq!(lines[2], "Fizz");
    assert_eq!(lines[4], "Buzz");
    assert_eq!(lines[14], "FizzBuzz");
}

#[test]
fn run_calculator_with_input() {
    let out = run_bloom_with_stdin(&["run", &example("calculator.blm")], "7\n5\n");
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert!(out.stdout.contains("a + b = 12"));
    assert!(out.stdout.contains("a - b = 2"));
    assert!(out.stdout.contains("a * b = 35"));
    assert!(out.stdout.contains("a / b = 1"));
}

#[test]
fn run_strings_example_handles_utf8() {
    let out = run_bloom(&["run", &example("strings.blm")]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert!(out.stdout.contains("Snowman: \u{2603}"));
    assert!(out.stdout.contains("starts with H"));
}

#[test]
fn build_creates_native_executable() {
    let dir = temp_dir("build");
    let out_path = dir.join("hello.exe");
    let out = run_bloom(&[
        "build",
        &example("hello.blm"),
        "-o",
        out_path.to_string_lossy().as_ref(),
    ]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert!(out_path.exists(), "executable was not created");
    let run = Command::new(&out_path).output().expect("run built exe");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(stdout.contains("Hello from Bloom!"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn emit_c_writes_readable_c() {
    let dir = temp_dir("emitc");
    let c_path = dir.join("out").join("hello.c");
    let exe_path = dir.join("hello.exe");
    let out = run_bloom(&[
        "build",
        &example("hello.blm"),
        "-o",
        exe_path.to_string_lossy().as_ref(),
        "--emit-c",
        c_path.to_string_lossy().as_ref(),
    ]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let c = std::fs::read_to_string(&c_path).expect("C file exists");
    assert!(c.contains("int main(void)"));
    assert!(c.contains("bloom_println_str"));
    assert!(c.contains("Hello from Bloom!"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn division_by_zero_is_runtime_error() {
    let dir = temp_dir("divzero");
    let file = dir.join("div.blm");
    write_file(
        &file,
        "fn main() -> int {\n    let a = 0;\n    let b = 1 / a;\n    println(b);\n    return 0;\n}\n",
    );
    let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 70, "{}", out.combined());
    assert!(out.stderr.contains("division by zero"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn out_of_bounds_is_runtime_error() {
    let dir = temp_dir("bounds");
    let file = dir.join("bounds.blm");
    write_file(
        &file,
        "fn main() -> int {\n    let xs = [1, 2];\n    println(xs[5]);\n    return 0;\n}\n",
    );
    let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 70, "{}", out.combined());
    assert!(out.stderr.contains("out of bounds"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn init_creates_working_project() {
    let dir = temp_dir("init");
    let out = run_bloom_in(&dir, &["init", "Demo"]);
    assert_eq!(out.code, 0, "{}", out.combined());
    let project = dir.join("Demo");
    assert!(project.join("bloom.toml").exists());
    assert!(project.join("src/main.blm").exists());
    let check = run_bloom_in(&project, &["check"]);
    assert_eq!(check.code, 0, "{}", check.combined());
    let ran = run_bloom_in(&project, &["run"]);
    if ran.code == 3 {
        eprintln!("skipping run: no C toolchain available");
    } else {
        assert_eq!(ran.code, 0, "{}", ran.combined());
        assert!(ran.stdout.contains("Hello from Demo!"));
        let build = run_bloom_in(&project, &["build"]);
        assert_eq!(build.code, 0, "{}", build.combined());
        assert!(project.join("build").join("Demo.exe").exists());
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn recursive_fibonacci_runs() {
    let dir = temp_dir("fib");
    let file = dir.join("fib.blm");
    write_file(
        &file,
        "fn fib(n: int) -> int {\n    if n < 2 {\n        return n;\n    }\n    return fib(n - 1) + fib(n - 2);\n}\n\nfn main() -> int {\n    println(fib(20));\n    return 0;\n}\n",
    );
    let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert!(out.stdout.contains("6765"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_structs_example() {
    let out = run_bloom(&["run", &example("structs.blm")]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert_eq!(
        stdout, "10\n20\n10\n99\n40\n20\nflaeche\n40\n",
        "unexpected structs example output"
    );
}

#[test]
fn struct_create_and_read_fields_natively() {
    let dir = temp_dir("struct-fields");
    let file = dir.join("fields.blm");
    write_file(
        &file,
        "struct Point {\n    x: int,\n    y: int\n}\n\nfn main() -> int {\n    let p = Point { x: 10, y: 20 };\n    println(p.x);\n    println(p.y);\n    return 0;\n}\n",
    );
    let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "10\n20\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn struct_field_mutation_natively() {
    let dir = temp_dir("struct-mutate");
    let file = dir.join("mutate.blm");
    write_file(
        &file,
        "struct Point {\n    x: int,\n    y: int\n}\n\nfn main() -> int {\n    var p = Point { x: 10, y: 20 };\n    p.x = 100;\n    p.y = 200;\n    println(p.x);\n    println(p.y);\n    return 0;\n}\n",
    );
    let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "100\n200\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn struct_value_semantics_natively() {
    let dir = temp_dir("struct-copy");
    let file = dir.join("copy.blm");
    write_file(
        &file,
        "struct Point {\n    x: int,\n    y: int\n}\n\nfn main() -> int {\n    var a = Point { x: 10, y: 20 };\n    var b = a;\n    b.x = 99;\n    println(a.x);\n    println(b.x);\n    return 0;\n}\n",
    );
    let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "10\n99\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn struct_returned_from_function_natively() {
    let dir = temp_dir("struct-return");
    let file = dir.join("ret.blm");
    write_file(
        &file,
        "struct Point {\n    x: int,\n    y: int\n}\n\nfn make_point(x: int, y: int) -> Point {\n    return Point { x: x, y: y };\n}\n\nfn main() -> int {\n    let p = make_point(15, 25);\n    println(p.x);\n    println(p.y);\n    return 0;\n}\n",
    );
    let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "15\n25\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn struct_passed_to_function_natively() {
    let dir = temp_dir("struct-param");
    let file = dir.join("param.blm");
    write_file(
        &file,
        "struct Point {\n    x: int,\n    y: int\n}\n\nfn sum_point(p: Point) -> int {\n    return p.x + p.y;\n}\n\nfn main() -> int {\n    let p = Point { x: 15, y: 25 };\n    println(sum_point(p));\n    return 0;\n}\n",
    );
    let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "40\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn multiple_and_nested_struct_types_natively() {
    let dir = temp_dir("struct-nested");
    let file = dir.join("nested.blm");
    write_file(
        &file,
        "struct Inner {\n    value: int,\n    label: string\n}\n\nstruct Outer {\n    inner: Inner,\n    scale: float\n}\n\nfn build(scale: float) -> Outer {\n    return Outer {\n        inner: Inner { value: 7, label: \"seven\" },\n        scale: scale\n    };\n}\n\nfn main() -> int {\n    var o = build(2);\n    println(o.inner.value);\n    println(o.inner.label);\n    println(o.scale);\n    o.inner.value = 42;\n    println(o.inner.value);\n    var copy = o;\n    copy.inner.value = 100;\n    println(o.inner.value);\n    println(copy.inner.value);\n    return 0;\n}\n",
    );
    let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "7\nseven\n2.0\n42\n42\n100\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn struct_array_field_has_reference_semantics_natively() {
    let dir = temp_dir("struct-arrfield");
    let file = dir.join("arrfield.blm");
    write_file(
        &file,
        "struct Bag {\n    items: [int],\n    label: string\n}\n\nfn main() -> int {\n    var b = Bag { items: [1, 2], label: \"zahlen\" };\n    push(b.items, 3);\n    println(len(b.items));\n    var c = b;\n    push(c.items, 4);\n    println(len(b.items));\n    println(len(c.items));\n    c.label = \"kopie\";\n    println(b.label);\n    println(c.label);\n    return 0;\n}\n",
    );
    let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "3\n4\n4\nzahlen\nkopie\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn struct_error_cases_are_rejected() {
    let cases = [
        ("struct_unknown_field.blm", "E1020"),
        ("struct_missing_field.blm", "E1022"),
        ("struct_recursive.blm", "E1024"),
        ("struct_immutable_field.blm", "E1006"),
        ("struct_field_on_int.blm", "E1023"),
    ];
    for (name, code) in cases {
        let out = run_bloom(&["check", &known_error(name)]);
        assert_eq!(out.code, 1, "expected failure for {}", name);
        assert!(
            out.stderr.contains(code),
            "expected {} in diagnostics for {}:\n{}",
            code,
            name,
            out.combined()
        );
    }
}

#[test]
fn struct_duplicate_and_extra_fields_rejected_inline() {
    let dir = temp_dir("struct-errors");
    let file = dir.join("dup.blm");
    write_file(
        &file,
        "struct P {\n    n: int\n}\n\nfn main() -> int {\n    let p = P { n: 1, n: 2 };\n    return 0;\n}\n",
    );
    let out = run_bloom(&["check", file.to_string_lossy().as_ref()]);
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("E1021"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn struct_in_array_rejected_inline() {
    let dir = temp_dir("struct-in-array");
    let file = dir.join("arr.blm");
    write_file(
        &file,
        "struct P {\n    n: int\n}\n\nfn main() -> int {\n    let xs = [P { n: 1 }, P { n: 2 }];\n    return 0;\n}\n",
    );
    let out = run_bloom(&["check", file.to_string_lossy().as_ref()]);
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("E1015"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn struct_emit_c_contains_typedef_and_value_copy() {
    let dir = temp_dir("struct-emitc");
    let file = dir.join("copy.blm");
    let c_path = dir.join("out.c");
    let exe_path = dir.join("copy.exe");
    write_file(
        &file,
        "struct Point {\n    x: int,\n    y: int\n}\n\nfn main() -> int {\n    var a = Point { x: 1, y: 2 };\n    var b = a;\n    b.x = 3;\n    println(a.x);\n    return 0;\n}\n",
    );
    let out = run_bloom(&[
        "build",
        file.to_string_lossy().as_ref(),
        "-o",
        exe_path.to_string_lossy().as_ref(),
        "--emit-c",
        c_path.to_string_lossy().as_ref(),
    ]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let c = std::fs::read_to_string(&c_path).expect("C file");
    assert!(c.contains("} bloom_struct_Point;"));
    assert!(c.contains("int64_t f_x;"));
    assert!(c.contains("= v_a_"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_methods_example() {
    let out = run_bloom(&["run", &example("methods.blm")]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert_eq!(
        stdout, "25\n13\n24\n3\n4\n5\n46\n4\n104\n85\n",
        "unexpected methods example output"
    );
}

#[test]
fn methods_value_semantics_natively() {
    let dir = temp_dir("method-semantics");
    let file = dir.join("sem.blm");
    write_file(
        &file,
        "struct Point {\n    x: int,\n    y: int\n}\n\nimpl Point {\n    fn translate(var self, dx: int, dy: int) -> void {\n        self.x = self.x + dx;\n        self.y = self.y + dy;\n    }\n\n    fn moved(self, dx: int) -> Point {\n        return Point { x: self.x + dx, y: self.y };\n    }\n}\n\nfn main() -> int {\n    var a = Point { x: 1, y: 2 };\n    var b = a;\n    b.translate(10, 20);\n    println(a.x);\n    println(b.x);\n    let c = a.moved(5);\n    println(c.x);\n    println(a.x);\n    return 0;\n}\n",
    );
    let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "1\n11\n6\n1\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn method_chain_and_mutating_call_natively() {
    let dir = temp_dir("method-chain");
    let file = dir.join("chain.blm");
    write_file(
        &file,
        "struct Counter {\n    n: int\n}\n\nimpl Counter {\n    fn inc(var self) -> void {\n        self.n = self.n + 1;\n    }\n    fn plus(self, k: int) -> Counter {\n        return Counter { n: self.n + k };\n    }\n    fn get(self) -> int {\n        return self.n;\n    }\n}\n\nfn main() -> int {\n    var c = Counter { n: 0 };\n    c.inc();\n    c.inc();\n    println(c.get());\n    println(c.plus(10).plus(100).get());\n    println(c.get());\n    return 0;\n}\n",
    );
    let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "2\n112\n2\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn method_errors_are_reported() {
    let dir = temp_dir("method-errors");
    let cases = [
        (
            "unknown.blm",
            "struct P {\n    x: int\n}\n\nimpl P {\n    fn get(self) -> int {\n        return self.x;\n    }\n}\n\nfn main() -> int {\n    let p = P { x: 1 };\n    return p.missing();\n}\n",
            "E1031",
        ),
        (
            "immutable.blm",
            "struct P {\n    x: int\n}\n\nimpl P {\n    fn set(var self) -> void {\n        self.x = 1;\n    }\n}\n\nfn main() -> int {\n    let p = P { x: 1 };\n    p.set();\n    return 0;\n}\n",
            "E1032",
        ),
        (
            "implmissing.blm",
            "impl Missing {\n    fn f(self) -> int {\n        return 0;\n    }\n}\n\nfn main() -> int {\n    return 0;\n}\n",
            "E1033",
        ),
    ];
    for (name, src, code) in cases {
        let file = dir.join(name);
        write_file(&file, src);
        let out = run_bloom(&["check", file.to_string_lossy().as_ref()]);
        assert_eq!(out.code, 1, "expected failure for {}", name);
        assert!(
            out.stderr.contains(code),
            "expected {} for {}:\n{}",
            code,
            name,
            out.combined()
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn methods_called_before_definition_natively() {
    let dir = temp_dir("method-forward");
    let file = dir.join("fwd.blm");
    write_file(
        &file,
        "fn main() -> int {\n    let p = Point { x: 2, y: 3 };\n    println(p.total());\n    return 0;\n}\n\nstruct Point {\n    x: int,\n    y: int\n}\n\nimpl Point {\n    fn total(self) -> int {\n        return self.x + self.y;\n    }\n}\n",
    );
    let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "5\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_enums_example() {
    let out = run_bloom(&["run", &example("enums.blm")]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "north\nwest\n0\n7\n5\nsouth\n");
}

#[test]
fn run_option_result_example() {
    let out = run_bloom(&["run", &example("option_result.blm")]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(
        norm(&out.stdout),
        "ok: 5\nerror: division by zero\n7\n-1\n42\n"
    );
}

#[test]
fn enum_match_natively() {
    let dir = temp_dir("enum-match");
    let file = dir.join("m.blm");
    write_file(
        &file,
        "enum Shape {\n    Circle(int),\n    Rect { w: int, h: int },\n    Empty\n}\n\nfn area(s: Shape) -> int {\n    return match s {\n        Shape::Circle(r) => 3 * r * r,\n        Shape::Rect { w, h } => w * h,\n        Shape::Empty => 0\n    };\n}\n\nfn main() -> int {\n    println(area(Shape::Circle(2)));\n    println(area(Shape::Rect { w: 3, h: 4 }));\n    println(area(Shape::Empty));\n    return 0;\n}\n",
    );
    let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "12\n12\n0\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn nested_patterns_natively() {
    let dir = temp_dir("nested-patterns");
    let file = dir.join("n.blm");
    write_file(
        &file,
        "fn f(o: Option<Option<int>>) -> int {\n    return match o {\n        Option::Some(Option::Some(v)) => v,\n        Option::Some(Option::None) => 100,\n        Option::None => -1\n    };\n}\n\nfn main() -> int {\n    println(f(Option::Some(Option::Some(7))));\n    println(f(Option::Some(Option::None)));\n    println(f(Option::None));\n    return 0;\n}\n",
    );
    let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "7\n100\n-1\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn user_generic_enum_natively() {
    let dir = temp_dir("generic-enum");
    let file = dir.join("g.blm");
    write_file(
        &file,
        "enum Pair<A, B> {\n    Both(A, B),\n    Empty\n}\n\nfn swap(p: Pair<int, string>) -> Pair<string, int> {\n    return match p {\n        Pair::Both(a, b) => Pair::Both(b, a),\n        Pair::Empty => Pair::Empty\n    };\n}\n\nfn main() -> int {\n    let p = Pair::Both(1, \"one\");\n    let q = swap(p);\n    let n = match q {\n        Pair::Both(s, num) => len(s) + num,\n        Pair::Empty => 0\n    };\n    println(n);\n    let both: Pair<int, int> = Pair::Both(2, 3);\n    let sum = match both {\n        Pair::Both(x, y) => x + y,\n        Pair::Empty => 0\n    };\n    println(sum);\n    return 0;\n}\n",
    );
    let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "4\n5\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn enum_in_struct_and_struct_in_enum_natively() {
    let dir = temp_dir("enum-struct-mix");
    let file = dir.join("mix.blm");
    write_file(
        &file,
        "struct Point {\n    x: int,\n    y: int\n}\n\nenum Shape {\n    Dot(Point),\n    None\n}\n\nstruct Drawing {\n    shape: Shape,\n    name: string\n}\n\nfn weight(d: Drawing) -> int {\n    return match d.shape {\n        Shape::Dot(p) => p.x * p.y,\n        Shape::None => 0\n    };\n}\n\nfn main() -> int {\n    var d = Drawing { shape: Shape::Dot(Point { x: 3, y: 5 }), name: \"dot\" };\n    println(weight(d));\n    d.shape = Shape::None;\n    println(weight(d));\n    println(d.name);\n    return 0;\n}\n",
    );
    let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "15\n0\ndot\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn non_exhaustive_match_rejected_natively() {
    let dir = temp_dir("non-exhaustive");
    let file = dir.join("m.blm");
    write_file(
        &file,
        "enum Color {\n    Red,\n    Green,\n    Blue\n}\n\nfn name(c: Color) -> string {\n    return match c {\n        Color::Red => \"red\",\n        Color::Green => \"green\"\n    };\n}\n\nfn main() -> int {\n    return 0;\n}\n",
    );
    let out = run_bloom(&["check", file.to_string_lossy().as_ref()]);
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("E1041"));
    assert!(out.stderr.contains("Color::Blue"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unreachable_match_arm_rejected_natively() {
    let dir = temp_dir("unreachable");
    let file = dir.join("m.blm");
    write_file(
        &file,
        "enum E {\n    A,\n    B\n}\n\nfn f(e: E) -> int {\n    return match e {\n        E::A => 1,\n        _ => 0,\n        E::B => 2\n    };\n}\n\nfn main() -> int {\n    return 0;\n}\n",
    );
    let out = run_bloom(&["check", file.to_string_lossy().as_ref()]);
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("E1042"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn recursive_enum_rejected_natively() {
    let dir = temp_dir("recursive-enum");
    let file = dir.join("r.blm");
    write_file(
        &file,
        "enum List {\n    Nil,\n    Cons(int, List)\n}\n\nfn main() -> int {\n    return 0;\n}\n",
    );
    let out = run_bloom(&["check", file.to_string_lossy().as_ref()]);
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("E1047"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn emit_c_contains_enum_representation() {
    let dir = temp_dir("enum-emitc");
    let file = dir.join("e.blm");
    let c_path = dir.join("out.c");
    let exe_path = dir.join("e.exe");
    write_file(
        &file,
        "enum Msg {\n    Quit,\n    Text(string)\n}\n\nfn main() -> int {\n    let m = Msg::Text(\"hi\");\n    let n = match m {\n        Msg::Quit => 0,\n        Msg::Text(t) => len(t)\n    };\n    println(n);\n    return 0;\n}\n",
    );
    let out = run_bloom(&[
        "build",
        file.to_string_lossy().as_ref(),
        "-o",
        exe_path.to_string_lossy().as_ref(),
        "--emit-c",
        c_path.to_string_lossy().as_ref(),
    ]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let c = std::fs::read_to_string(&c_path).expect("C file");
    assert!(c.contains("int32_t tag;"));
    assert!(c.contains("union {"));
    assert!(c.contains("bloom_enum_Msg"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_modules_project_example() {
    let project = root().join("examples").join("modules_project");
    let out = run_bloom_in(&project, &["run"]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(
        norm(&out.stdout),
        "area:\n12\ncircle:\n12\nlocalhost:8080 (12)\nscaled:\n50\n"
    );
}

#[test]
fn multi_module_imports_natively() {
    let dir = temp_dir("multi-module");
    write_file(
        &dir.join("main.blm"),
        "use geometry;\nuse geometry::Point;\n\nfn main() -> int {\n    let p = Point { x: 3, y: 4 };\n    let o = Point { x: 0, y: 0 };\n    println(geometry::distance(p, o));\n    println(p.sum());\n    println(geometry::origin().sum());\n    return 0;\n}\n",
    );
    write_file(
        &dir.join("geometry.blm"),
        "pub struct Point {\n    x: int,\n    y: int\n}\n\nimpl Point {\n    pub fn sum(self) -> int {\n        return self.x + self.y;\n    }\n}\n\npub fn distance(a: Point, b: Point) -> int {\n    let dx = a.x - b.x;\n    let dy = a.y - b.y;\n    return dx * dx + dy * dy;\n}\n\npub fn origin() -> Point {\n    return Point { x: 0, y: 0 };\n}\n",
    );
    let out = run_bloom(&["run", dir.join("main.blm").to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "25\n7\n0\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn submodule_with_mod_blm_natively() {
    let dir = temp_dir("mod-blm");
    write_file(
        &dir.join("main.blm"),
        "use util;\n\nfn main() -> int {\n    println(util::tag());\n    return 0;\n}\n",
    );
    write_file(
        &dir.join("util/mod.blm"),
        "pub fn tag() -> string {\n    return \"from mod.blm\";\n}\n",
    );
    let out = run_bloom(&["run", dir.join("main.blm").to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "from mod.blm\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cyclic_module_imports_natively() {
    let dir = temp_dir("cyclic");
    write_file(
        &dir.join("main.blm"),
        "use alpha;\nuse beta;\n\nfn main() -> int {\n    println(alpha::from_alpha(1));\n    println(beta::from_beta(2));\n    return 0;\n}\n",
    );
    write_file(
        &dir.join("alpha.blm"),
        "use beta;\n\npub fn from_alpha(x: int) -> int {\n    return x + beta::helper();\n}\n\npub fn one() -> int {\n    return 1;\n}\n",
    );
    write_file(
        &dir.join("beta.blm"),
        "use alpha;\n\npub fn from_beta(x: int) -> int {\n    return x + alpha::one();\n}\n\npub fn helper() -> int {\n    return 10;\n}\n",
    );
    let out = run_bloom(&["run", dir.join("main.blm").to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "11\n3\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn module_visibility_errors_are_reported() {
    let dir = temp_dir("mod-vis");
    write_file(
        &dir.join("lib.blm"),
        "pub struct Public {\n    x: int\n}\n\npub fn visible() -> int {\n    return 1;\n}\n\nfn hidden() -> int {\n    return 2;\n}\n",
    );
    write_file(
        &dir.join("main.blm"),
        "use lib;\n\nfn main() -> int {\n    println(lib::hidden());\n    return 0;\n}\n",
    );
    let out = run_bloom(&["check", dir.join("main.blm").to_string_lossy().as_ref()]);
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("E1051"), "{}", out.combined());
    write_file(
        &dir.join("main.blm"),
        "use lib::hidden;\n\nfn main() -> int {\n    return 0;\n}\n",
    );
    let out = run_bloom(&["check", dir.join("main.blm").to_string_lossy().as_ref()]);
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("E1051"), "{}", out.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn module_import_errors_are_reported() {
    let dir = temp_dir("mod-errs");
    write_file(
        &dir.join("lib.blm"),
        "pub fn visible() -> int {\n    return 1;\n}\n",
    );
    let cases = [
        ("use nowhere;\nfn main() -> int { return 0; }\n", "E1050"),
        (
            "use lib::missing;\nfn main() -> int { return 0; }\n",
            "E1055",
        ),
        (
            "use lib::visible;\nuse lib::visible;\nfn main() -> int { return 0; }\n",
            "E1052",
        ),
        (
            "use lib;\nuse lib;\nfn main() -> int { return 0; }\n",
            "E1052",
        ),
    ];
    for (src, code) in cases {
        write_file(&dir.join("main.blm"), src);
        let out = run_bloom(&["check", dir.join("main.blm").to_string_lossy().as_ref()]);
        assert_eq!(out.code, 1, "expected failure for {}", src);
        assert!(
            out.stderr.contains(code),
            "expected {} for `{}`:\n{}",
            code,
            src,
            out.combined()
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn importing_entry_module_is_rejected() {
    let dir = temp_dir("mod-entry");
    write_file(
        &dir.join("main.blm"),
        "use helper;\nfn main() -> int { return 0; }\n",
    );
    write_file(
        &dir.join("helper.blm"),
        "use main;\npub fn f() -> int { return 1; }\n",
    );
    let out = run_bloom(&["check", dir.join("main.blm").to_string_lossy().as_ref()]);
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("E1054"), "{}", out.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn imported_structs_keep_value_semantics_natively() {
    let dir = temp_dir("mod-value");
    write_file(
        &dir.join("main.blm"),
        "use lib::Counter;\n\nfn main() -> int {\n    var a = Counter { n: 1 };\n    var b = a;\n    b.n = 99;\n    println(a.n);\n    println(b.n);\n    return 0;\n}\n",
    );
    write_file(
        &dir.join("lib.blm"),
        "pub struct Counter {\n    n: int\n}\n",
    );
    let out = run_bloom(&["run", dir.join("main.blm").to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "1\n99\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_generics_example() {
    let out = run_bloom(&["run", &example("generics.blm")]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(
        norm(&out.stdout),
        "42\nBloom\n99\nhi\nbye\n7\ntext\n3\n5\n3\n"
    );
}

#[test]
fn run_enum_methods_example() {
    let out = run_bloom(&["run", &example("enum_methods.blm")]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(
        norm(&out.stdout),
        "circle 2.5\nrect 3.0x4.0\ntrue\nfalse\ntrue\nfalse\n"
    );
}

#[test]
fn run_result_try_example() {
    let out = run_bloom(&["run", &example("result_try.blm")]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(
        norm(&out.stdout),
        "ok 42\nvalue is 10\nerror: empty input\n"
    );
}

#[test]
fn generic_functions_natively() {
    let dir = temp_dir("generic-fns");
    let file = dir.join("g.blm");
    write_file(
        &file,
        "fn identity<T>(value: T) -> T {\n    return value;\n}\n\nfn first<A, B>(a: A, b: B) -> A {\n    return a;\n}\n\nfn main() -> int {\n    println(identity(1));\n    println(identity(\"text\"));\n    println(identity(2.5));\n    println(first(9, \"ignored\"));\n    return 0;\n}\n",
    );
    let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "1\ntext\n2.5\n9\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn generic_struct_value_semantics_natively() {
    let dir = temp_dir("generic-semantics");
    let file = dir.join("g.blm");
    write_file(
        &file,
        "struct Box<T> {\n    value: T\n}\n\nfn main() -> int {\n    var a = Box { value: 1 };\n    var b = a;\n    b.value = 99;\n    println(a.value);\n    println(b.value);\n    return 0;\n}\n",
    );
    let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "1\n99\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn generic_enum_methods_natively() {
    let dir = temp_dir("generic-enum-methods");
    let file = dir.join("g.blm");
    write_file(
        &file,
        "impl<T> Option<T> {\n    fn is_some(self) -> bool {\n        return match self {\n            Option::Some(_) => true,\n            Option::None => false\n        };\n    }\n\n    fn value_or(self, fallback: T) -> T {\n        return match self {\n            Option::Some(v) => v,\n            Option::None => fallback\n        };\n    }\n}\n\nfn main() -> int {\n    let a: Option<int> = Option::Some(7);\n    let b: Option<int> = Option::None;\n    println(a.is_some());\n    println(b.is_some());\n    println(a.value_or(-1));\n    println(b.value_or(-1));\n    let s: Option<string> = Option::None;\n    println(s.value_or(\"fallback\"));\n    return 0;\n}\n",
    );
    let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "true\nfalse\n7\n-1\nfallback\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn try_operator_evaluates_operand_once() {
    let dir = temp_dir("try-once");
    let file = dir.join("t.blm");
    write_file(
        &file,
        "var calls = 0;\n\nfn bump() -> Result<int, string> {\n    calls = calls + 1;\n    return Result::Err(\"boom\");\n}\n\nfn run() -> Result<int, string> {\n    let value = bump()?;\n    return Result::Ok(value);\n}\n\nfn main() -> int {\n    let r = run();\n    println(calls);\n    println(match r {\n        Result::Ok(v) => \"ok\",\n        Result::Err(e) => e\n    });\n    return 0;\n}\n",
    );
    let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "1\nboom\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn try_operator_preserves_error_natively() {
    let dir = temp_dir("try-error");
    let file = dir.join("t.blm");
    write_file(
        &file,
        "fn fail() -> Result<int, string> {\n    return Result::Err(\"the message\");\n}\n\nfn chain() -> Result<int, string> {\n    let v = fail()?;\n    return Result::Ok(v + 1);\n}\n\nfn main() -> int {\n    println(match chain() {\n        Result::Ok(v) => \"ok\",\n        Result::Err(e) => e\n    });\n    return 0;\n}\n",
    );
    let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "the message\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn generic_errors_are_reported() {
    let dir = temp_dir("generic-errors");
    let cases = [
        (
            "wrong_field.blm",
            "struct Box<T> {\n    value: T\n}\n\nfn main() -> int {\n    let b: Box<int> = Box { value: \"wrong\" };\n    return 0;\n}\n",
            "E1002",
        ),
        (
            "missing_args.blm",
            "struct Box<T> {\n    value: T\n}\n\nfn main() -> int {\n    let b: Box = Box { value: 1 };\n    return 0;\n}\n",
            "E1016",
        ),
        (
            "not_inferable.blm",
            "struct Tagged<T> {\n    label: string\n}\n\nfn main() -> int {\n    let t = Tagged { label: \"x\" };\n    return 0;\n}\n",
            "E1045",
        ),
        (
            "try_bad_operand.blm",
            "fn f() -> Result<int, string> {\n    let x = 5;\n    let y = x?;\n    return Result::Ok(y);\n}\n\nfn main() -> int { return 0; }\n",
            "E1060",
        ),
        (
            "try_bad_return.blm",
            "fn f() -> int {\n    let r: Result<int, string> = Result::Ok(1);\n    return r?;\n}\n\nfn main() -> int { return 0; }\n",
            "E1061",
        ),
        (
            "assoc_conflict.blm",
            "enum E {\n    A\n}\n\nimpl E {\n    fn A() -> E {\n        return E::A;\n    }\n}\n\nfn main() -> int { return 0; }\n",
            "E1030",
        ),
    ];
    for (name, src, code) in cases {
        let file = dir.join(name);
        write_file(&file, src);
        let out = run_bloom(&["check", file.to_string_lossy().as_ref()]);
        assert_eq!(out.code, 1, "expected failure for {}", name);
        assert!(
            out.stderr.contains(code),
            "expected {} for {}:\n{}",
            code,
            name,
            out.combined()
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn generics_across_modules_natively() {
    let dir = temp_dir("generic-modules");
    write_file(
        &dir.join("main.blm"),
        "use containers::Box;\nuse containers;\n\nfn main() -> int {\n    let b = Box { value: 11 };\n    println(b.get());\n    let c = containers::wrap(\"text\");\n    println(c.get());\n    return 0;\n}\n",
    );
    write_file(
        &dir.join("containers.blm"),
        "pub struct Box<T> {\n    value: T\n}\n\nimpl<T> Box<T> {\n    pub fn get(self) -> T {\n        return self.value;\n    }\n}\n\npub fn wrap<T>(value: T) -> Box<T> {\n    return Box { value: value };\n}\n",
    );
    let out = run_bloom(&["run", dir.join("main.blm").to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "11\ntext\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_text_tools_example() {
    let out = run_bloom(&["run", &example("text_tools.blm")]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(
        norm(&out.stdout),
        "A\nB\nzero\nF\n3\nvalues: 10 1 2 40\ntrue\ntrue\ntrue\nHello, Bloom\nHELLO, WORLD\nhello, world\n2\nHello\nWorld\n"
    );
}

#[test]
fn literal_patterns_and_guards_natively() {
    let dir = temp_dir("literal-guards");
    let file = dir.join("l.blm");
    write_file(
        &file,
        "fn classify(n: int) -> string {\n    return match n {\n        0 => \"zero\",\n        1 => \"one\",\n        n if n < 0 => \"negative\",\n        _ => \"other\"\n    };\n}\n\nfn flag(b: bool) -> int {\n    return match b {\n        true => 1,\n        false => 0\n    };\n}\n\nfn main() -> int {\n    println(classify(0));\n    println(classify(1));\n    println(classify(-5));\n    println(classify(9));\n    println(flag(true) + flag(false));\n    return 0;\n}\n",
    );
    let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "zero\none\nnegative\nother\n1\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn array_pop_insert_natively() {
    let dir = temp_dir("pop-insert");
    let file = dir.join("p.blm");
    write_file(
        &file,
        "fn main() -> int {\n    var xs: [int] = [1, 2, 3];\n    let a = pop(xs);\n    let b = pop(xs);\n    let c = pop(xs);\n    let d = pop(xs);\n    insert(xs, 0, 10);\n    insert(xs, 1, 20);\n    println(match a {\n        Option::Some(v) => v,\n        Option::None => -1\n    });\n    println(match d {\n        Option::Some(v) => v,\n        Option::None => -1\n    });\n    println(xs[0]);\n    println(xs[1]);\n    println(len(xs));\n    return 0;\n}\n",
    );
    let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "3\n-1\n10\n20\n2\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn generic_function_builds_variant_natively() {
    let dir = temp_dir("generic-variant");
    let file = dir.join("g.blm");
    write_file(
        &file,
        "fn first_of<T>(values: [T]) -> Option<T> {\n    if len(values) == 0 {\n        return Option::None;\n    }\n    return Option::Some(values[0]);\n}\n\nfn main() -> int {\n    match first_of([10, 20]) {\n        Option::Some(v) => println(v),\n        Option::None => println(-1)\n    };\n    match first_of([]) {\n        Option::Some(v) => println(v),\n        Option::None => println(-1)\n    };\n    return 0;\n}\n",
    );
    let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "10\n-1\n");
    let _ = std::fs::remove_dir_all(&dir);
}

fn assert_no_leaks(dir: &std::path::Path, file: &std::path::Path, expected_stdout: &str) {
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), expected_stdout);
    assert!(
        out.stderr.contains("live allocations: 0"),
        "expected no live allocations:\n{}",
        out.combined()
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn refcounting_releases_strings_natively() {
    let dir = temp_dir("rc-strings");
    let file = dir.join("s.blm");
    write_file(
        &file,
        "fn greet(name: string) -> string {\n    return \"hello \" + name + \"!\";\n}\n\nfn early(flag: bool) -> string {\n    let a = \"early \" + \"return\";\n    if flag {\n        return a + \"!\";\n    }\n    return a;\n}\n\nfn main() -> int {\n    var i = 0;\n    while i < 30 {\n        let g = greet(str(i));\n        println(g);\n        i = i + 1;\n    }\n    var text = \"a\";\n    text = text + \"b\";\n    text = replace(text, \"b\", \"c\");\n    println(text);\n    println(early(true));\n    println(early(false));\n    println(trim(\"  x  \"));\n    return 0;\n}\n",
    );
    let mut expected = String::new();
    for i in 0..30 {
        expected.push_str(&format!("hello {i}!\n"));
    }
    expected.push_str("ac\nearly return!\nearly return\nx\n");
    assert_no_leaks(&dir, &file, &expected);
}

#[test]
fn refcounting_releases_composites_natively() {
    let dir = temp_dir("rc-composites");
    let file = dir.join("c.blm");
    write_file(
        &file,
        "struct User {\n    name: string,\n    scores: [int]\n}\n\nenum Message {\n    Quit,\n    Text(string),\n    Pair(string, string)\n}\n\nimpl User {\n    fn rename(self, name: string) -> User {\n        return User { name: name, scores: self.scores };\n    }\n}\n\nfn pick(m: Message) -> string {\n    return match m {\n        Message::Quit => \"quit\",\n        Message::Text(t) => t,\n        Message::Pair(a, b) => a + b\n    };\n}\n\nfn first_of<T>(values: [T]) -> Option<T> {\n    if len(values) == 0 {\n        return Option::None;\n    }\n    return Option::Some(values[0]);\n}\n\nfn parse(text: string) -> Result<int, string> {\n    let t = trim(text);\n    if len(t) == 0 {\n        return Result::Err(\"empty\");\n    }\n    return Result::Ok(int(t));\n}\n\nfn doubled(text: string) -> Result<int, string> {\n    let v = parse(text)?;\n    return Result::Ok(v * 2);\n}\n\nfn main() -> int {\n    var u = User { name: \"ada\", scores: [1, 2, 3] };\n    var u2 = u;\n    u2.name = \"bob\";\n    let u3 = u.rename(\"grace\");\n    println(u.name);\n    println(u2.name);\n    println(u3.name);\n    println(pick(Message::Text(\"txt\")));\n    println(pick(Message::Pair(\"a\", \"b\")));\n    println(pick(Message::Quit));\n    var parts: [string] = split(\"a,b,c\", \",\");\n    insert(parts, 1, \"mid\");\n    let last = pop(parts);\n    println(match last {\n        Option::Some(v) => v,\n        Option::None => \"none\"\n    });\n    let first = first_of(parts);\n    println(match first {\n        Option::Some(v) => v,\n        Option::None => \"empty\"\n    });\n    println(match doubled(\"21\") {\n        Result::Ok(v) => str(v),\n        Result::Err(e) => e\n    });\n    println(match doubled(\"\") {\n        Result::Ok(v) => str(v),\n        Result::Err(e) => e\n    });\n    return 0;\n}\n",
    );
    assert_no_leaks(
        &dir,
        &file,
        "ada\nbob\ngrace\ntxt\nab\nquit\nc\na\n42\nempty\n",
    );
}

#[test]
fn refcounting_releases_in_loops_natively() {
    let dir = temp_dir("rc-loops");
    let file = dir.join("l.blm");
    write_file(
        &file,
        "fn main() -> int {\n    var names: [string] = [];\n    var k = 0;\n    while k < 20 {\n        let s = \"item \" + str(k);\n        push(names, replace(s, \"item\", \"thing\"));\n        k = k + 1;\n    }\n    println(names[19]);\n    var i = 0;\n    var total = 0;\n    while i < 50 {\n        let parts = split(\"x,y,z\", \",\");\n        total = total + len(parts);\n        i = i + 1;\n    }\n    println(total);\n    return 0;\n}\n",
    );
    assert_no_leaks(&dir, &file, "thing 19\n150\n");
}

#[test]
fn refcounting_handles_mutation_and_globals_natively() {
    let dir = temp_dir("rc-mutation");
    let file = dir.join("m.blm");
    write_file(
        &file,
        "struct Account {\n    name: string,\n    tags: [string]\n}\n\nimpl Account {\n    fn set_name(var self, name: string) -> void {\n        self.name = name;\n    }\n\n    fn add_tag(var self, tag: string) -> void {\n        push(self.tags, tag);\n    }\n}\n\nconst GREETING: string = \"welcome\";\n\nfn id(s: string) -> string {\n    return s;\n}\n\nfn main() -> int {\n    var a = Account { name: \"ada\", tags: [\"x\"] };\n    a.set_name(id(\"grace\"));\n    a.add_tag(\"y\");\n    println(a.name);\n    println(a.tags[1]);\n    var names: [string] = [\"a\", \"b\"];\n    names[0] = \"z\";\n    names[1] = names[0] + \"!\";\n    println(names[0]);\n    println(names[1]);\n    println(GREETING + \"!\");\n    return 0;\n}\n",
    );
    assert_no_leaks(&dir, &file, "grace\ny\nz\nz!\nwelcome!\n");
}

#[test]
fn array_remove_and_slice_natively() {
    let dir = temp_dir("remove-slice");
    let file = dir.join("rs.blm");
    write_file(
        &file,
        "fn main() -> int {\n    var values = [10, 20, 30];\n    let removed = remove(values, 1);\n    println(match removed {\n        Option::Some(v) => v,\n        Option::None => -1\n    });\n    println(values[1]);\n    let bad = remove(values, 5);\n    println(match bad {\n        Option::Some(v) => v,\n        Option::None => -1\n    });\n    let part = slice(values, 0, 2);\n    println(len(part));\n    var words: [string] = [\"a\", \"b\", \"c\", \"d\"];\n    let mid = slice(words, 1, 3);\n    println(mid[0]);\n    println(mid[1]);\n    let one = remove(words, 0);\n    println(match one {\n        Option::Some(v) => v,\n        Option::None => \"none\"\n    });\n    println(len(words));\n    return 0;\n}\n",
    );
    assert_no_leaks(&dir, &file, "20\n30\n-1\n2\nb\nc\na\n3\n");
}

#[test]
fn join_and_math_natively() {
    let dir = temp_dir("join-math");
    let file = dir.join("jm.blm");
    write_file(
        &file,
        "use std::math;\n\nfn main() -> int {\n    let parts = [\"red\", \"green\", \"blue\"];\n    println(join(parts, \", \"));\n    let empty: [string] = [];\n    println(\"[\" + join(empty, \"-\") + \"]\");\n    let words = split(\"a b c\", \" \");\n    println(join(words, \"_\"));\n    println(math::sqrt(81.0));\n    println(math::pow(2.0, 10.0));\n    println(math::floor(3.7));\n    println(math::ceil(3.2));\n    println(math::round(3.5));\n    println(math::abs(-4.5));\n    println(math::min(2.0, 7.0));\n    println(math::max(2.0, 7.0));\n    println(math::clamp(15.0, 0.0, 10.0));\n    println(math::abs_int(-9));\n    println(math::min_int(3, 8));\n    println(math::max_int(3, 8));\n    println(math::clamp_int(99, 0, 50));\n    return 0;\n}\n",
    );
    assert_no_leaks(
        &dir,
        &file,
        "red, green, blue\n[]\na_b_c\n9.0\n1024.0\n3.0\n4.0\n4.0\n4.5\n2.0\n7.0\n10.0\n9\n3\n8\n50\n",
    );
}

#[test]
fn math_symbol_import_natively() {
    let dir = temp_dir("math-symbol");
    let file = dir.join("ms.blm");
    write_file(
        &file,
        "use std::math::sqrt;\nuse std::math::abs_int;\n\nfn main() -> int {\n    println(sqrt(16.0));\n    println(abs_int(-3));\n    return 0;\n}\n",
    );
    assert_no_leaks(&dir, &file, "4.0\n3\n");
}

#[test]
fn option_try_operator_natively() {
    let dir = temp_dir("option-try");
    let file = dir.join("o.blm");
    write_file(
        &file,
        "fn get_value() -> Option<int> {\n    return Option::Some(42);\n}\n\nfn calculate() -> Option<int> {\n    let value = get_value()?;\n    return Option::Some(value * 2);\n}\n\nfn maybe_none() -> Option<int> {\n    return Option::None;\n}\n\nfn chain() -> Option<int> {\n    let v = maybe_none()?;\n    return Option::Some(v + 1);\n}\n\nfn nested() -> Option<string> {\n    let s = get_value()?;\n    let t = calculate()?;\n    return Option::Some(str(s) + \"/\" + str(t));\n}\n\nfn main() -> int {\n    println(match calculate() {\n        Option::Some(v) => v,\n        Option::None => -1\n    });\n    println(match chain() {\n        Option::Some(v) => v,\n        Option::None => -1\n    });\n    println(match nested() {\n        Option::Some(s) => s,\n        Option::None => \"none\"\n    });\n    return 0;\n}\n",
    );
    assert_no_leaks(&dir, &file, "84\n-1\n42/84\n");
}

#[test]
fn option_try_return_type_mismatch_rejected() {
    let dir = temp_dir("option-try-err");
    let file = dir.join("e.blm");
    write_file(
        &file,
        "fn f(o: Option<int>) -> Result<int, string> {\n    let v = o?;\n    return Result::Ok(v);\n}\n\nfn main() -> int { return 0; }\n",
    );
    let out = run_bloom(&["check", file.to_string_lossy().as_ref()]);
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("E1061"), "{}", out.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn panic_exits_with_message_and_code() {
    let dir = temp_dir("panic");
    let file = dir.join("p.blm");
    write_file(
        &file,
        "fn check(n: int) -> int {\n    if n < 0 {\n        panic(\"negative input\");\n    }\n    return n;\n}\n\nfn main() -> int {\n    println(check(5));\n    println(check(-1));\n    return 0;\n}\n",
    );
    let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 70, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "5\n");
    assert!(out.stderr.contains("Bloom panic: negative input"));
    assert!(out.stderr.contains("p.blm:3"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn panic_as_diverging_last_statement() {
    let dir = temp_dir("panic-diverge");
    let file = dir.join("pd.blm");
    write_file(
        &file,
        "fn always_panics() -> int {\n    panic(\"never returns\");\n}\n\nfn main() -> int {\n    if false {\n        println(always_panics());\n    }\n    println(\"done\");\n    return 0;\n}\n",
    );
    assert_no_leaks(&dir, &file, "done\n");
}

#[test]
fn or_patterns_natively() {
    let dir = temp_dir("or-patterns");
    let file = dir.join("op.blm");
    write_file(
        &file,
        "enum Color {\n    Red,\n    Green,\n    Blue\n}\n\nfn name(c: Color) -> string {\n    return match c {\n        Color::Red | Color::Green => \"warmish\",\n        Color::Blue => \"cold\"\n    };\n}\n\nfn classify(n: int) -> string {\n    return match n {\n        1 | 2 | 3 => \"small\",\n        4 | 5 => \"medium\",\n        _ => \"large\"\n    };\n}\n\nfn pick(o: Option<int>) -> string {\n    return match o {\n        Option::Some(1 | 2) => \"one-or-two\",\n        Option::Some(0) | Option::None => \"zero-or-none\",\n        Option::Some(n) => \"other \" + str(n)\n    };\n}\n\nfn main() -> int {\n    println(name(Color::Red));\n    println(name(Color::Blue));\n    println(classify(2));\n    println(classify(5));\n    println(classify(9));\n    println(pick(Option::Some(1)));\n    println(pick(Option::Some(0)));\n    println(pick(Option::None));\n    println(pick(Option::Some(7)));\n    return 0;\n}\n",
    );
    assert_no_leaks(
        &dir,
        &file,
        "warmish\ncold\nsmall\nmedium\nlarge\none-or-two\nzero-or-none\nzero-or-none\nother 7\n",
    );
}

#[test]
fn or_pattern_binding_mismatch_rejected() {
    let dir = temp_dir("or-bindings");
    let file = dir.join("ob.blm");
    write_file(
        &file,
        "fn f(o: Option<int>) -> int {\n    return match o {\n        Option::Some(v) | Option::None => v,\n        _ => 0\n    };\n}\n\nfn main() -> int { return 0; }\n",
    );
    let out = run_bloom(&["check", file.to_string_lossy().as_ref()]);
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("E1044"), "{}", out.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn or_pattern_unreachable_alternative_rejected() {
    let dir = temp_dir("or-unreachable");
    let file = dir.join("ou.blm");
    write_file(
        &file,
        "fn f(n: int) -> int {\n    return match n {\n        1 => 1,\n        1 | 2 => 2,\n        _ => 0\n    };\n}\n\nfn main() -> int { return 0; }\n",
    );
    let out = run_bloom(&["check", file.to_string_lossy().as_ref()]);
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("E1042"), "{}", out.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pub_use_reexports_natively() {
    let dir = temp_dir("pub-use");
    write_file(
        &dir.join("main.blm"),
        "use facade;\nuse facade::Point;\nuse facade::distance;\n\nfn main() -> int {\n    let a = Point { x: 0, y: 0 };\n    let b = facade::Point { x: 3, y: 4 };\n    println(facade::distance(a, b));\n    println(distance(a, b));\n    return 0;\n}\n",
    );
    write_file(
        &dir.join("geometry.blm"),
        "pub struct Point {\n    x: int,\n    y: int\n}\n\npub fn distance(p: Point, q: Point) -> int {\n    let dx = p.x - q.x;\n    let dy = p.y - q.y;\n    return dx * dx + dy * dy;\n}\n\nfn helper() -> int {\n    return 1;\n}\n",
    );
    write_file(
        &dir.join("shapes.blm"),
        "pub use geometry::Point;\npub use geometry::distance;\n",
    );
    write_file(
        &dir.join("facade.blm"),
        "pub use shapes::Point;\npub use shapes::distance;\n",
    );
    let out = run_bloom(&["run", dir.join("main.blm").to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "25\n25\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pub_use_private_symbol_rejected() {
    let dir = temp_dir("pub-use-private");
    write_file(
        &dir.join("geometry.blm"),
        "fn helper() -> int {\n    return 1;\n}\n",
    );
    write_file(&dir.join("bad.blm"), "pub use geometry::helper;\n");
    write_file(
        &dir.join("main.blm"),
        "use bad;\nfn main() -> int { return 0; }\n",
    );
    let out = run_bloom(&["check", dir.join("main.blm").to_string_lossy().as_ref()]);
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("E1051"), "{}", out.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pub_use_cycle_terminates() {
    let dir = temp_dir("pub-use-cycle");
    write_file(&dir.join("cyc_a.blm"), "pub use cyc_b::thing;\n");
    write_file(&dir.join("cyc_b.blm"), "pub use cyc_a::thing;\n");
    write_file(
        &dir.join("main.blm"),
        "use cyc_a;\nuse cyc_a::thing;\nfn main() -> int { return 0; }\n",
    );
    let out = run_bloom(&["check", dir.join("main.blm").to_string_lossy().as_ref()]);
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("E1055"), "{}", out.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_collections_example() {
    let out = run_bloom_env(
        &["run", &example("collections.blm")],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(
        norm(&out.stdout),
        "red, green, blue\nremoved second\nfirst | third\n3\n20\n40\n12.0\n256.0\n10\n7\n"
    );
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
}

#[test]
fn array_slice_out_of_bounds_panics() {
    let dir = temp_dir("slice-bounds");
    let file = dir.join("sb.blm");
    write_file(
        &file,
        "fn main() -> int {\n    let values = [1, 2, 3];\n    let bad = slice(values, 0, 5);\n    println(len(bad));\n    return 0;\n}\n",
    );
    let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 70, "{}", out.combined());
    assert!(
        out.stderr.contains("slice bounds out of range"),
        "{}",
        out.combined()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn slice_views_natively() {
    let dir = temp_dir("slice-views");
    let file = dir.join("v.blm");
    write_file(
        &file,
        "fn main() -> int {\n    var values = [10, 20, 30, 40, 50];\n    let view = slice_view(values, 1, 4);\n    println(len(view));\n    println(view[0]);\n    println(view[1]);\n    println(view[2]);\n    values[1] = 99;\n    println(view[0]);\n    var w = slice_view(values, 0, 3);\n    w[0] = 7;\n    println(values[0]);\n    var grow = [1, 2, 3];\n    let gv = slice_view(grow, 0, 2);\n    var i = 0;\n    while i < 50 {\n        push(grow, i);\n        i = i + 1;\n    }\n    println(gv[0]);\n    println(gv[1]);\n    println(len(gv));\n    println(len(grow));\n    let empty = slice_view(values, 2, 2);\n    println(len(empty));\n    var words: [string] = [\"alpha\", \"beta\", \"gamma\"];\n    let wv = slice_view(words, 0, 2);\n    println(wv[1]);\n    var ww = slice_view(words, 1, 3);\n    ww[0] = \"BETA\";\n    println(words[1]);\n    println(wv[1]);\n    return 0;\n}\n",
    );
    assert_no_leaks(
        &dir,
        &file,
        "3\n20\n30\n40\n99\n7\n1\n2\n2\n53\n0\nbeta\nBETA\nBETA\n",
    );
}

#[test]
fn slice_view_invalidation_natively() {
    let cases = [
        (
            "insert",
            "var xs = [1, 2, 3];\n    let v = slice_view(xs, 0, 2);\n    insert(xs, 0, 9);\n    println(v[0]);",
        ),
        (
            "remove",
            "var xs = [1, 2, 3];\n    let v = slice_view(xs, 0, 2);\n    let r = remove(xs, 0);\n    println(v[0]);",
        ),
        (
            "pop",
            "var xs = [1, 2, 3];\n    let v = slice_view(xs, 0, 2);\n    let p = pop(xs);\n    println(v[0]);",
        ),
        (
            "len-after-insert",
            "var xs = [1, 2, 3];\n    let v = slice_view(xs, 0, 2);\n    insert(xs, 0, 9);\n    println(len(v));",
        ),
    ];
    for (name, body) in cases {
        let dir = temp_dir(&format!("slice-inv-{name}"));
        let file = dir.join("i.blm");
        write_file(
            &file,
            &format!("fn main() -> int {{\n    {body}\n    return 0;\n}}\n"),
        );
        let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
        if no_toolchain(&out) {
            eprintln!("skipping: no C toolchain available");
            return;
        }
        assert_eq!(out.code, 70, "{}: {}", name, out.combined());
        assert!(
            out.stderr.contains("slice view was invalidated"),
            "{}: {}",
            name,
            out.combined()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn slice_view_no_false_invalidation_natively() {
    let dir = temp_dir("slice-noinv");
    let file = dir.join("n.blm");
    write_file(
        &file,
        "fn main() -> int {\n    var xs: [int] = [];\n    let v = slice_view(xs, 0, 0);\n    let p = pop(xs);\n    println(len(v));\n    println(match p {\n        Option::Some(x) => x,\n        Option::None => -1\n    });\n    var ys = [1, 2, 3];\n    let w = slice_view(ys, 0, 3);\n    push(ys, 4);\n    ys[0] = 10;\n    println(w[0]);\n    println(len(w));\n    return 0;\n}\n",
    );
    assert_no_leaks(&dir, &file, "0\n-1\n10\n3\n");
}

#[test]
fn slice_view_bounds_panics_natively() {
    let cases = [
        (
            "start-gt-end",
            "let xs = [1, 2, 3];\n    let v = slice_view(xs, 2, 1);\n    println(len(v));",
        ),
        (
            "end-too-big",
            "let xs = [1, 2, 3];\n    let v = slice_view(xs, 0, 4);\n    println(len(v));",
        ),
        (
            "negative-start",
            "let xs = [1, 2, 3];\n    let v = slice_view(xs, -1, 2);\n    println(len(v));",
        ),
        (
            "view-index-oob",
            "let xs = [1, 2, 3];\n    let v = slice_view(xs, 0, 2);\n    println(v[5]);",
        ),
    ];
    for (name, body) in cases {
        let dir = temp_dir(&format!("slice-bounds-{name}"));
        let file = dir.join("b.blm");
        write_file(
            &file,
            &format!("fn main() -> int {{\n    {body}\n    return 0;\n}}\n"),
        );
        let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
        if no_toolchain(&out) {
            eprintln!("skipping: no C toolchain available");
            return;
        }
        assert_eq!(out.code, 70, "{}: {}", name, out.combined());
        let expected = if name == "view-index-oob" {
            "slice index out of bounds"
        } else {
            "slice_view bounds out of range"
        };
        assert!(
            out.stderr.contains(expected),
            "{}: {}",
            name,
            out.combined()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn slice_view_lifetime_and_generics_natively() {
    let dir = temp_dir("slice-lifetime");
    let file = dir.join("l.blm");
    write_file(
        &file,
        "struct Window {\n    view: Slice<int>\n}\n\nfn make_view(a: [int]) -> Slice<int> {\n    return slice_view(a, 1, 3);\n}\n\nfn total(v: Slice<int>) -> int {\n    var sum = 0;\n    var i = 0;\n    while i < len(v) {\n        sum = sum + v[i];\n        i = i + 1;\n    }\n    return sum;\n}\n\nfn first_of<T>(s: Slice<T>) -> T {\n    return s[0];\n}\n\nfn main() -> int {\n    var values = [1, 2, 3, 4];\n    let v1 = slice_view(values, 0, 2);\n    values = [100, 200];\n    println(v1[0]);\n    println(v1[1]);\n    println(len(values));\n    let v2 = make_view([10, 20, 30, 40]);\n    println(v2[0]);\n    println(v2[1]);\n    println(total(slice_view([1, 2, 3, 4, 5], 1, 4)));\n    println(first_of(slice_view([7, 8, 9], 1, 3)));\n    println(first_of(slice_view([\"x\", \"y\"], 0, 2)));\n    var w = Window { view: slice_view([5, 6, 7], 0, 2) };\n    println(w.view[1]);\n    let w2 = w;\n    println(len(w2.view));\n    var i = 0;\n    while i < 100 {\n        let loop_view = slice_view(values, 0, 1);\n        let x = loop_view[0];\n        i = i + 1;\n    }\n    return 0;\n}\n",
    );
    assert_no_leaks(&dir, &file, "1\n2\n2\n20\n30\n9\n8\nx\n6\n2\n");
}

#[test]
fn slice_view_copies_and_containers_natively() {
    let dir = temp_dir("slice-copies");
    let file = dir.join("c.blm");
    write_file(
        &file,
        "enum Holder {\n    Empty,\n    WithView(Slice<int>)\n}\n\nfn pick(h: Holder) -> int {\n    return match h {\n        Holder::Empty => -1,\n        Holder::WithView(v) => v[0]\n    };\n}\n\nfn maybe_view(flag: bool) -> Option<Slice<int>> {\n    if flag {\n        return Option::Some(slice_view([9, 8, 7], 0, 2));\n    }\n    return Option::None;\n}\n\nfn try_view(o: Option<Slice<int>>) -> Option<int> {\n    let v = o?;\n    return Option::Some(v[1]);\n}\n\nfn main() -> int {\n    var values = [1, 2, 3, 4];\n    let a = slice_view(values, 0, 2);\n    let b = a;\n    println(a[1]);\n    println(b[1]);\n    println(len(a) + len(b));\n    var v = slice_view(values, 0, 1);\n    v = slice_view(values, 2, 4);\n    println(v[0]);\n    println(len(v));\n    let h = Holder::WithView(slice_view([5, 6], 0, 2));\n    println(pick(h));\n    println(match maybe_view(true) {\n        Option::Some(v) => v[0],\n        Option::None => -1\n    });\n    println(match try_view(maybe_view(true)) {\n        Option::Some(x) => x,\n        Option::None => -1\n    });\n    println(match try_view(maybe_view(false)) {\n        Option::Some(x) => x,\n        Option::None => -1\n    });\n    let r = match true {\n        true => slice_view(values, 1, 3)[0],\n        false => 0\n    };\n    println(r);\n    var outer: Slice<int> = slice_view([100, 200], 0, 2);\n    {\n        let inner_arr = [1, 2, 3];\n        outer = slice_view(inner_arr, 1, 3);\n    }\n    println(outer[0]);\n    println(outer[1]);\n    return 0;\n}\n",
    );
    assert_no_leaks(&dir, &file, "2\n2\n4\n3\n2\n5\n9\n8\n-1\n2\n2\n3\n");
}
#[test]
fn run_slices_example() {
    let out = run_bloom_env(
        &["run", &example("slices.blm")],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "3\n90\n99\n7\n1\n2\n2\n200\n300\n7\n");
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
}

#[test]
fn run_file_io_example() {
    let dir = temp_dir("file-io-example");
    let out = run_bloom_env_in(
        &dir,
        &["run", &example("file_io.blm")],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(stdout.contains("Verzeichnis bereit"), "{stdout}");
    assert!(stdout.contains("geschrieben: 14 Bytes"), "{stdout}");
    assert!(stdout.contains("Hallo, Bloom!\nZweite Zeile\n"), "{stdout}");
    assert!(stdout.contains("Groesse: 27"), "{stdout}");
    assert!(stdout.contains("ist Datei: true"), "{stdout}");
    assert!(stdout.contains("greeting.txt"), "{stdout}");
    assert!(stdout.contains("file not found"), "{stdout}");
    assert!(stdout.contains("Verzeichnis entfernt"), "{stdout}");
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    assert!(out.stderr.contains("live sockets: 0"), "{}", out.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- std::path and std::json (0.12.0) -------------------------------------

#[test]
fn path_lexical_natively() {
    let dir = temp_dir("path-lexical");
    let file = dir.join("p.blm");
    let src = r#"use std::path;

fn show_opt(o: Option<string>) -> string {
    return match o {
        Option::Some(v) => v,
        Option::None => "(none)"
    };
}

fn norm(s: string) -> string {
    return match path::normalize(s) {
        Result::Ok(v) => v,
        Result::Err(e) => "ERR: " + e
    };
}

fn main() -> int {
    println(path::join("C:\\a", "b"));
    println(path::join("C:\\a\\", "b"));
    println(path::join("C:\\a", "\\b"));
    println(path::join("C:\\a", "D:\\b"));
    println(path::join("C:a", "b"));
    println(path::join("a", "b\\c"));
    println(path::join("", "b"));
    println(path::join("a", ""));
    println(path::join("\\\\server\\share", "dir\\file"));
    println(show_opt(path::parent("C:\\a\\b\\c")));
    println(show_opt(path::parent("C:\\a")));
    println(show_opt(path::parent("C:\\")));
    println(show_opt(path::parent("a")));
    println(show_opt(path::parent("a\\b\\")));
    println(show_opt(path::parent("")));
    println(show_opt(path::parent("..")));
    println(show_opt(path::file_name("C:\\a\\b.txt")));
    println(show_opt(path::file_name("C:\\a\\")));
    println(show_opt(path::file_name("C:\\")));
    println(show_opt(path::file_name("..")));
    println(show_opt(path::file_name("a\\.\\")));
    println(show_opt(path::extension("a.tar.gz")));
    println(show_opt(path::extension("noext")));
    println(show_opt(path::extension(".hidden")));
    println(show_opt(path::extension("trailing.")));
    println(show_opt(path::stem("a.tar.gz")));
    println(show_opt(path::stem(".hidden")));
    println(show_opt(path::stem("trailing.")));
    println(show_opt(path::stem("C:\\a\\.hidden")));
    println(str(path::is_absolute("C:\\a")));
    println(str(path::is_absolute("C:a")));
    println(str(path::is_absolute("\\a")));
    println(str(path::is_absolute("\\\\server\\share\\a")));
    println(str(path::is_absolute("a\\b")));
    println(path::with_extension("C:\\a\\b.txt", "md"));
    println(path::with_extension("C:\\a\\b", "md"));
    println(path::with_extension("C:\\a\\b.txt", ""));
    println(path::with_extension("C:\\a\\.hidden", "json"));
    println(path::with_extension("C:\\a\\b.txt", ".md"));
    println(path::with_extension("C:\\", "md"));
    println(norm("C:\\a\\.\\b\\..\\c"));
    println(norm("a\\..\\..\\b"));
    println(norm("..\\a"));
    println(norm("C:\\..\\a"));
    println(norm("\\\\server\\share\\a\\..\\b"));
    println(norm("a//b///c"));
    println(norm("a\\b\\"));
    println(norm("C:\\a\\b\\"));
    println(norm("."));
    println(norm("a\\."));
    println(norm(""));
    println(norm("\\\\?\\C:\\a"));
    println(norm("\\\\server"));
    return 0;
}
"#;
    write_file(&file, src);
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    let expected = [
        "C:\\a\\b",
        "C:\\a\\b",
        "C:\\b",
        "D:\\b",
        "C:a\\b",
        "a\\b\\c",
        "b",
        "a",
        "\\\\server\\share\\dir\\file",
        "C:\\a\\b",
        "C:\\",
        "(none)",
        "",
        "a",
        "(none)",
        "",
        "b.txt",
        "a",
        "(none)",
        "(none)",
        "a",
        "gz",
        "(none)",
        "(none)",
        "",
        "a.tar",
        ".hidden",
        "trailing",
        ".hidden",
        "true",
        "false",
        "false",
        "true",
        "false",
        "C:\\a\\b.md",
        "C:\\a\\b.md",
        "C:\\a\\b",
        "C:\\a\\.hidden.json",
        "C:\\a\\b.md",
        "C:\\",
        "C:\\a\\c",
        "..\\b",
        "..\\a",
        "C:\\a",
        "\\\\server\\share\\b",
        "a\\b\\c",
        "a\\b",
        "C:\\a\\b",
        ".",
        "a",
        "ERR: normalize: empty path",
        "ERR: normalize: extended-length (`\\\\?\\`) and device (`\\\\.\\`) prefixes are not supported",
        "ERR: normalize: UNC path without a share name is not supported",
    ];
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), expected.len(), "{stdout}");
    for (i, (got, want)) in lines.iter().zip(expected.iter()).enumerate() {
        assert_eq!(got, want, "line {i}: {stdout}");
    }
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn json_parse_and_access_natively() {
    let dir = temp_dir("json-parse");
    let file = dir.join("j.blm");
    let src = r#"use std::json;

fn show_str(r: Result<string, string>) -> string {
    return match r {
        Result::Ok(v) => v,
        Result::Err(e) => "ERR: " + e
    };
}

fn show_int(r: Result<int, string>) -> string {
    return match r {
        Result::Ok(v) => str(v),
        Result::Err(e) => "ERR: " + e
    };
}

fn show_float(r: Result<float, string>) -> string {
    return match r {
        Result::Ok(v) => str(v),
        Result::Err(e) => "ERR: " + e
    };
}

fn show_bool(r: Result<bool, string>) -> string {
    return match r {
        Result::Ok(v) => str(v),
        Result::Err(e) => "ERR: " + e
    };
}

fn get_or_null(v: json::JsonValue, key: string) -> json::JsonValue {
    return match json::get(v, key) {
        Option::Some(child) => child,
        Option::None => json::null()
    };
}

fn at_or_null(v: json::JsonValue, index: int) -> json::JsonValue {
    return match json::array_get(v, index) {
        Option::Some(child) => child,
        Option::None => json::null()
    };
}

fn main() -> int {
    let text = "  {  \"null\": null, \"t\": true, \"f\": false, \"n\": -12.5e2, " +
        "\"i\": 7, \"s\": \"text\", \"empty_obj\": {}, \"empty_arr\": [], " +
        "\"nested\": {\"deep\": {\"arr\": [null, true, 42, \"x\"]}}, " +
        "\"keys\": {\"b\": 1, \"a\": 2, \"c\": 3} }  ";
    let doc = match json::parse(text) {
        Result::Ok(v) => v,
        Result::Err(e) => json::null()
    };
    println(json::type_of(doc));
    println(json::type_of(get_or_null(doc, "null")));
    println(show_bool(json::get_bool(doc, "t")));
    println(show_bool(json::get_bool(doc, "f")));
    println(show_float(json::get_float(doc, "n")));
    println(show_int(json::get_int(doc, "i")));
    println(show_str(json::get_string(doc, "s")));
    println(json::type_of(get_or_null(doc, "empty_obj")));
    println(show_int(json::array_length(get_or_null(doc, "empty_arr"))));
    let nested = get_or_null(get_or_null(doc, "nested"), "deep");
    let arr = get_or_null(nested, "arr");
    println(show_int(json::array_length(arr)));
    println(json::type_of(at_or_null(arr, 0)));
    println(show_bool(json::as_bool(at_or_null(arr, 1))));
    println(show_int(json::as_int(at_or_null(arr, 2))));
    println(show_str(json::as_string(at_or_null(arr, 3))));
    println(json::type_of(at_or_null(arr, 99)));
    println(json::type_of(at_or_null(arr, -1)));
    let keys = get_or_null(doc, "keys");
    let empty_names: [string] = [];
    let names = match json::object_keys(keys) {
        Result::Ok(v) => v,
        Result::Err(e) => empty_names
    };
    var i = 0;
    while i < len(names) {
        println(names[i]);
        i = i + 1;
    }
    println(str(json::has(keys, "b")));
    println(str(json::has(keys, "zz")));
    println(str(json::has(get_or_null(doc, "s"), "x")));
    return 0;
}
"#;
    write_file(&file, src);
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    let expected = [
        "object", "null", "true", "false", "-1250.0", "7", "text", "object", "0", "4", "null",
        "true", "42", "x", "null", "null", "b", "a", "c", "true", "false", "false",
    ];
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), expected.len(), "{stdout}");
    for (i, (got, want)) in lines.iter().zip(expected.iter()).enumerate() {
        assert_eq!(got, want, "line {i}: {stdout}");
    }
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn json_errors_natively() {
    let dir = temp_dir("json-errors");
    let file = dir.join("j.blm");
    let src = r#"use std::json;

fn err_of(s: string) -> string {
    return match json::parse(s) {
        Result::Ok(_) => "unerwartet",
        Result::Err(e) => e
    };
}

fn main() -> int {
    println(err_of(""));
    println(err_of("   "));
    println(err_of("{"));
    println(err_of("{\"a\":}"));
    println(err_of("{\"a\" 1}"));
    println(err_of("{\"a\": 1, \"a\": 2}"));
    println(err_of("[1, 2"));
    println(err_of("[1 2]"));
    println(err_of("[1,]"));
    println(err_of("tru"));
    println(err_of("nul"));
    println(err_of("01"));
    println(err_of("1."));
    println(err_of("1e"));
    println(err_of("-"));
    println(err_of("\"abc"));
    println(err_of("\"a\\x\""));
    println(err_of("\"a\\u12g4\""));
    println(err_of("\"\\uD800x\""));
    println(err_of("\"\\uDC00\""));
    println(err_of("{} extra"));
    println(err_of("123 abc"));
    println(err_of("\"a\tb\""));
    println(err_of("{\"k\" : }"));
    return 0;
}
"#;
    write_file(&file, src);
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    let expected = [
        "unexpected end of input at byte 1 (line 1, column 1)",
        "unexpected end of input at byte 4 (line 1, column 4)",
        "expected a string key at byte 2 (line 1, column 2)",
        "unexpected character at byte 6 (line 1, column 6)",
        "expected `:` after object key at byte 6 (line 1, column 6)",
        "duplicate object key at byte 17 (line 1, column 17)",
        "unterminated array at byte 6 (line 1, column 6)",
        "expected `,` or `]` in array at byte 4 (line 1, column 4)",
        "unexpected character at byte 4 (line 1, column 4)",
        "invalid literal at byte 1 (line 1, column 1)",
        "invalid literal at byte 1 (line 1, column 1)",
        "invalid number (leading zeros are not allowed) at byte 2 (line 1, column 2)",
        "invalid number (missing digits after `.`) at byte 3 (line 1, column 3)",
        "invalid number (missing exponent digits) at byte 3 (line 1, column 3)",
        "invalid number at byte 2 (line 1, column 2)",
        "unterminated string at byte 5 (line 1, column 5)",
        "invalid escape sequence at byte 4 (line 1, column 4)",
        "invalid `\\u` escape (expected four hex digits) at byte 7 (line 1, column 7)",
        "high surrogate without a following low surrogate at byte 8 (line 1, column 8)",
        "lone low surrogate in `\\u` escape at byte 8 (line 1, column 8)",
        "trailing characters after the JSON value at byte 4 (line 1, column 4)",
        "trailing characters after the JSON value at byte 5 (line 1, column 5)",
        "unescaped control character in string at byte 3 (line 1, column 3)",
        "unexpected character at byte 8 (line 1, column 8)",
    ];
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), expected.len(), "{stdout}");
    for (i, (got, want)) in lines.iter().zip(expected.iter()).enumerate() {
        assert_eq!(got, want, "line {i}: {stdout}");
    }
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn json_construct_and_serialize_natively() {
    let dir = temp_dir("json-construct");
    let file = dir.join("j.blm");
    let src = r#"use std::json;

fn show(r: Result<string, string>) -> string {
    return match r {
        Result::Ok(v) => v,
        Result::Err(e) => "ERR: " + e
    };
}

fn show_int(r: Result<int, string>) -> string {
    return match r {
        Result::Ok(v) => str(v),
        Result::Err(e) => "ERR: " + e
    };
}

fn float_or_null(f: float) -> json::JsonValue {
    return match json::make_float(f) {
        Result::Ok(v) => v,
        Result::Err(e) => json::null()
    };
}

fn main() -> int {
    let obj = json::make_object();
    json::set(obj, "b", json::make_int(2));
    json::set(obj, "a", json::make_int(1));
    json::set(obj, "c", json::make_int(3));
    json::set(obj, "b", json::make_int(20));
    println(show(json::stringify(obj)));
    let empty_keys: [string] = [];
    let keys = match json::object_keys(obj) {
        Result::Ok(v) => v,
        Result::Err(e) => empty_keys
    };
    var i = 0;
    while i < len(keys) {
        println(keys[i]);
        i = i + 1;
    }
    println(show(json::stringify(json::make_object())));
    println(show(json::stringify(json::make_array())));
    println(show(json::stringify_pretty(json::make_object())));
    println(show(json::stringify_pretty(json::make_array())));
    println(show(json::stringify(json::make_string("line\n\ttab \"q\" \\ back"))));
    println(show(json::stringify(json::make_string("äöü😀"))));
    println(show(json::stringify(float_or_null(2.0))));
    println(show(json::stringify(float_or_null(-0.5))));
    println(show(json::stringify(float_or_null(1e21))));
    println(show(json::stringify(json::make_int(0))));
    println(show_int(json::as_int(json::make_int(-9007199254740993))));
    println(match json::set(json::make_array(), "k", json::make_int(1)) {
        Result::Ok(_) => "unerwartet",
        Result::Err(e) => "ERR: " + e
    });
    println(match json::push(json::make_object(), json::make_int(1)) {
        Result::Ok(_) => "unerwartet",
        Result::Err(e) => "ERR: " + e
    });
    println(match json::set(obj, "self", obj) {
        Result::Ok(_) => "unerwartet",
        Result::Err(e) => "ERR: " + e
    });
    let arr = json::make_array();
    json::push(arr, json::make_int(1));
    println(match json::push(arr, arr) {
        Result::Ok(_) => "unerwartet",
        Result::Err(e) => "ERR: " + e
    });
    println(show(json::stringify(arr)));
    return 0;
}
"#;
    write_file(&file, src);
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    let expected = [
        "{\"b\":20,\"a\":1,\"c\":3}",
        "b",
        "a",
        "c",
        "{}",
        "[]",
        "{}",
        "[]",
        "\"line\\n\\ttab \\\"q\\\" \\\\ back\"",
        "\"äöü😀\"",
        "2",
        "-0.5",
        "1e+21",
        "0",
        "-9007199254740993",
        "ERR: set: value is not an object (create one with make_object)",
        "ERR: push: value is not an array (create one with make_array)",
        "ERR: set: this would create a cycle (the value already contains the object)",
        "ERR: push: this would create a cycle (the value already contains the array)",
        "[1]",
    ];
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), expected.len(), "{stdout}");
    for (i, (got, want)) in lines.iter().zip(expected.iter()).enumerate() {
        assert_eq!(got, want, "line {i}: {stdout}");
    }
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn json_roundtrip_and_numbers_natively() {
    let dir = temp_dir("json-roundtrip");
    let file = dir.join("j.blm");
    let src = r#"use std::json;

fn show(r: Result<string, string>) -> string {
    return match r {
        Result::Ok(v) => v,
        Result::Err(e) => "ERR: " + e
    };
}

fn show_int(r: Result<int, string>) -> string {
    return match r {
        Result::Ok(v) => str(v),
        Result::Err(e) => "ERR: " + e
    };
}

fn parse_or_null(s: string) -> json::JsonValue {
    return match json::parse(s) {
        Result::Ok(v) => v,
        Result::Err(e) => json::null()
    };
}

fn float_eq(r: Result<float, string>, expected: float) -> string {
    return match r {
        Result::Ok(v) => str(v == expected),
        Result::Err(e) => "ERR"
    };
}

fn main() -> int {
    let text = "{\"z\":[1,2.5,-3e-2,\"s\",true,null],\"o\":{\"k\":[[]]},\"big\":12345678901234567890," +
        "\"precise\":0.30000000000000004,\"exp\":1e2,\"negzero\":-0}";
    let doc = parse_or_null(text);
    let once = show(json::stringify(doc));
    let twice = show(json::stringify(parse_or_null(once)));
    println(str(once == twice));
    println(once);
    println(float_eq(json::get_float(doc, "precise"), 0.30000000000000004));
    println(float_eq(json::get_float(doc, "exp"), 100.0));
    println(show_int(json::get_int(doc, "big")));
    println(show(json::stringify(parse_or_null("[[[[[1]]]]]"))));
    println(show(json::stringify(parse_or_null("{\"a\":{\"b\":{\"c\":[1,2,3]}}}"))));
    println(json::type_of(parse_or_null("null")));
    return 0;
}
"#;
    write_file(&file, src);
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    let expected = [
        "true",
        "{\"z\":[1,2.5,-3e-2,\"s\",true,null],\"o\":{\"k\":[[]]},\"big\":12345678901234567890,\"precise\":0.30000000000000004,\"exp\":1e2,\"negzero\":-0}",
        "true",
        "true",
        "ERR: get_int: integer out of range for `int`",
        "[[[[[1]]]]]",
        "{\"a\":{\"b\":{\"c\":[1,2,3]}}}",
        "null",
    ];
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), expected.len(), "{stdout}");
    for (i, (got, want)) in lines.iter().zip(expected.iter()).enumerate() {
        assert_eq!(got, want, "line {i}: {stdout}");
    }
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn json_limits_natively() {
    let dir = temp_dir("json-limits");
    let big = dir.join("big.json");
    let longstr = dir.join("longstr.json");
    // 9 MiB input (over the 8 MiB limit) and a 4.5 MiB string (over the
    // 4 MiB string limit).
    std::fs::write(&big, "a".repeat(9 * 1024 * 1024)).expect("write big");
    let mut long = String::with_capacity(4 * 1024 * 1024 + 300 * 1024);
    long.push('"');
    long.push_str(&"b".repeat(4 * 1024 * 1024 + 256 * 1024));
    long.push('"');
    std::fs::write(&longstr, long).expect("write longstr");

    let file = dir.join("j.blm");
    let src = r#"use std::json;
use std::fs;

fn err_of(s: string) -> string {
    return match json::parse(s) {
        Result::Ok(_) => "unerwartet",
        Result::Err(e) => e
    };
}

fn err_of_file(path: string) -> string {
    let content = match fs::read_text(path) {
        Result::Ok(v) => v,
        Result::Err(e) => "FEHLER: " + e
    };
    return err_of(content);
}

fn main() -> int {
    var deep = "";
    var i = 0;
    while i < 200 {
        deep = deep + "[";
        i = i + 1;
    }
    println(err_of(deep));
    var ok_deep = "";
    i = 0;
    while i < 100 {
        ok_deep = ok_deep + "[";
        i = i + 1;
    }
    ok_deep = ok_deep + "1";
    i = 0;
    while i < 100 {
        ok_deep = ok_deep + "]";
        i = i + 1;
    }
    println(json::type_of(match json::parse(ok_deep) {
        Result::Ok(v) => v,
        Result::Err(e) => json::null()
    }));
    var digits = "1";
    i = 0;
    while i < 200 {
        digits = digits + "0";
        i = i + 1;
    }
    println(err_of(digits));
    println(err_of_file("@BIG@"));
    println(err_of_file("@LONGSTR@"));
    let root = json::make_object();
    var cur = root;
    i = 0;
    while i < 600 {
        let next = json::make_object();
        json::set(cur, "c", next);
        cur = next;
        i = i + 1;
    }
    println(match json::stringify(root) {
        Result::Ok(_) => "unerwartet",
        Result::Err(e) => e
    });
    println(match json::stringify(json::make_int(1)) {
        Result::Ok(v) => v,
        Result::Err(e) => e
    });
    return 0;
}
"#
    .replace("@BIG@", &big.to_string_lossy().replace('\\', "\\\\"))
    .replace(
        "@LONGSTR@",
        &longstr.to_string_lossy().replace('\\', "\\\\"),
    );
    write_file(&file, &src);
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    let expected = [
        "nesting too deep (max 128) at byte 129 (line 1, column 129)",
        "array",
        "number token too long (max 128 characters) at byte 1 (line 1, column 1)",
        "parse: input too large (max 8388608 bytes)",
        "string too long (max 4194304 bytes) at byte 4194306 (line 1, column 4194306)",
        "stringify: structure too deeply nested (max 512)",
        "1",
    ];
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), expected.len(), "{stdout}");
    for (i, (got, want)) in lines.iter().zip(expected.iter()).enumerate() {
        assert_eq!(got, want, "line {i}: {stdout}");
    }
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_json_and_path_examples() {
    let cases: &[(&str, &[&str])] = &[
        (
            "json_parse.blm",
            &[
                "Typ: object",
                "Name: Ada",
                "Alter: 36",
                "Aktiv: true",
                "Stadt: London",
                "Tags: 2",
                "Erster Tag: math",
                "Zweiter Tag: code",
            ],
        ),
        (
            "json_create.blm",
            &[
                "{\"name\":\"Ada Lovelace\",\"age\":37,\"score\":98.5,\"active\":true,\"skills\":[\"mathematics\",\"programming\",null]}",
                "-7",
                "2.5",
                "false",
                "null",
                "FEHLER: set: this would create a cycle",
            ],
        ),
        (
            "json_pretty.blm",
            &[
                "kompakt:",
                "{\"project\":\"bloom\",\"versions\":[0,11,12],\"stable\":true,\"notes\":null,\"limits\":{\"depth\":128,\"input\":8388608}}",
                "lesbar:",
                "  \"project\": \"bloom\",",
            ],
        ),
        (
            "path_operations.blm",
            &[
                "join:     C:\\projekte\\bloom\\src",
                "join abs: D:\\andere",
                "parent:   C:\\projekte\\bloom",
                "endung:   txt",
                "normal:   C:\\a\\c\\d",
                "relativ:  ..\\b",
                "unc:      \\\\server\\share\\b",
            ],
        ),
    ];
    for (name, needles) in cases {
        let out = run_bloom_env(&["run", &example(name)], &[("BLOOM_MEM_REPORT", "1")]);
        if no_toolchain(&out) {
            eprintln!("skipping: no C toolchain available");
            return;
        }
        assert_eq!(out.code, 0, "{name}: {}", out.combined());
        let stdout = norm(&out.stdout);
        for needle in *needles {
            assert!(stdout.contains(needle), "{name}: {stdout}");
        }
        assert!(
            out.stderr.contains("live allocations: 0"),
            "{name}: {}",
            out.combined()
        );
    }
}

#[test]
fn run_json_file_config_example() {
    let dir = temp_dir("json-config-example");
    let out = run_bloom_env_in(
        &dir,
        &["run", &example("json_file_config.blm")],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(stdout.contains("Konfiguration geschrieben"), "{stdout}");
    assert!(stdout.contains("name: bloom-demo"), "{stdout}");
    assert!(stdout.contains("version: 1"), "{stdout}");
    assert!(stdout.contains("pfad 0: C:\\projekte\\src"), "{stdout}");
    assert!(stdout.contains("pfad 1: C:\\projekte\\tests"), "{stdout}");
    assert!(
        stdout.contains("Version erhöht und gespeichert."),
        "{stdout}"
    );
    assert!(stdout.contains("Datei entfernt"), "{stdout}");
    assert!(
        !dir.join("bloom_config_demo.json").exists(),
        "config file was not removed"
    );
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fs_handles_unicode_and_spaces_in_paths() {
    let dir = temp_dir("fs-unicode");
    let file = dir.join("u.blm");
    let base = dir.join("unter ordner \u{00e4}\u{00f6}\u{00fc}");
    let base_lit = base.to_string_lossy().replace('\\', "\\\\");
    write_file(
        &file,
        &format!(
            "use std::fs;\n\nfn main() -> int {{\n    let dir = \"{base_lit}\";\n    match fs::create_dir_all(dir) {{\n        Result::Ok(_) => println(\"dir ok\"),\n        Result::Err(e) => println(e)\n    }};\n    let path = fs::join_path(dir, \"t\u{00e4}st datei.txt\");\n    match fs::write_text(path, \"unicode\" ) {{\n        Result::Ok(n) => println(str(n)),\n        Result::Err(e) => println(e)\n    }};\n    match fs::read_text(path) {{\n        Result::Ok(t) => println(t),\n        Result::Err(e) => println(e)\n    }};\n    match fs::exists(path) {{\n        Result::Ok(b) => println(str(b)),\n        Result::Err(e) => println(e)\n    }};\n    return 0;\n}}\n"
        ),
    );
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "dir ok\n7\nunicode\ntrue\n");
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    assert!(out.stderr.contains("live sockets: 0"), "{}", out.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- std::web routing and REST basics (0.13.0) -----------------------------

#[test]
fn json_shared_subtree_release_natively() {
    // Regression for a 0.12.0 bug: freeing a JSON tree released shared
    // children unconditionally instead of dropping one reference, so a
    // subtree stored in another object (or still held by a variable) was
    // freed early — use-after-free and heap corruption from the second
    // iteration on. This test shares a parsed subtree with a reply object,
    // drops the reply first and keeps using the original value.
    let dir = temp_dir("json-shared");
    let file = dir.join("s.blm");
    let src = r#"use std::json;

fn parse_or_null(s: string) -> json::JsonValue {
    return match json::parse(s) {
        Result::Ok(v) => v,
        Result::Err(e) => json::null()
    };
}

fn show(r: Result<string, string>) -> string {
    return match r {
        Result::Ok(v) => v,
        Result::Err(e) => "ERR: " + e
    };
}

// The reply shares the parsed subtree and is dropped when this returns;
// the caller's value must stay fully usable afterwards.
fn use_and_drop(parsed: json::JsonValue) -> string {
    let reply = json::make_object();
    json::set(reply, "received", parsed);
    json::set(reply, "route", json::make_string("echo"));
    return json::type_of(reply);
}

fn main() -> int {
    var i = 0;
    while i < 50 {
        let parsed = parse_or_null("{\"name\": \"ada\", \"n\": 3}");
        let kind = use_and_drop(parsed);
        println(str(i) + " " + kind + " " + show(json::stringify(parsed)));
        i = i + 1;
    }
    // Replacing a shared value must not free the still-referenced node.
    let parsed = parse_or_null("[1, 2, 3]");
    let reply = json::make_object();
    json::set(reply, "k", parsed);
    json::set(reply, "k", json::make_int(9));
    println(show(json::stringify(parsed)));
    println(show(json::stringify(reply)));
    return 0;
}
"#;
    write_file(&file, src);
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 52, "{stdout}");
    for (i, line) in lines.iter().take(50).enumerate() {
        assert_eq!(
            *line,
            format!("{i} object {{\"name\":\"ada\",\"n\":3}}"),
            "{stdout}"
        );
    }
    assert_eq!(lines[50], "[1,2,3]", "{stdout}");
    assert_eq!(lines[51], "{\"k\":9}", "{stdout}");
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    assert!(out.stderr.contains("live sockets: 0"), "{}", out.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn web_routing_natively() {
    let dir = temp_dir("web-routing");
    let file = dir.join("w.blm");
    let src = r#"use std::web;

fn show_bool(r: Result<bool, string>) -> string {
    return match r {
        Result::Ok(v) => "ok",
        Result::Err(e) => "FEHLER: " + e
    };
}

fn show_match(r: Result<web::Match, string>) -> string {
    return match r {
        Result::Ok(m) => "status " + str(m.status) + " route=" + m.name + " allow=" + m.allow,
        Result::Err(e) => "FEHLER: " + e
    };
}

fn main() -> int {
    let router = web::router();
    println(show_bool(web::route(router, "GET", "/", "home")));
    println(show_bool(web::route(router, "GET", "/users", "list_users")));
    println(show_bool(web::route(router, "POST", "/users", "create_user")));
    println(show_bool(web::route(router, "GET", "/users/me", "profile")));
    println(show_bool(web::route(router, "GET", "/users/:id", "show_user")));
    println(show_bool(web::route(router, "GET", "/files/:name.:ext", "bad_param")));
    println(show_bool(web::route(router, "GET", "/a//b", "empty_seg")));
    println(show_bool(web::route(router, "GET", "no-slash", "bad_start")));
    println(show_bool(web::route(router, "LONGER_THAN_16_CHARS", "/x", "bad_method")));
    println(show_bool(web::route(router, "GET", "/users/:id", "dup_pattern")));
    println(show_bool(web::route(router, "GET", "/users/:name", "same_shape")));
    println(show_bool(web::route(router, "GET", "/accounts", "list_users")));
    println(show_bool(web::route(router, "GET", "/users/:id/posts/:id", "dup_param")));
    println("---");
    println(show_match(web::resolve(router, "GET", "/")));
    println(show_match(web::resolve(router, "GET", "/users")));
    println(show_match(web::resolve(router, "POST", "/users")));
    println(show_match(web::resolve(router, "GET", "/users/me")));
    println(show_match(web::resolve(router, "GET", "/users/42")));
    println(show_match(web::resolve(router, "GET", "/users/42/")));
    println(show_match(web::resolve(router, "get", "/users")));
    println(show_match(web::resolve(router, "DELETE", "/users")));
    println(show_match(web::resolve(router, "PUT", "/users/7")));
    println(show_match(web::resolve(router, "GET", "/nope")));
    println(show_match(web::resolve(router, "GET", "/users/1/2")));
    println(show_match(web::resolve(router, "BAD METHOD", "/")));
    return 0;
}
"#;
    write_file(&file, src);
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    let expected = [
        "ok",
        "ok",
        "ok",
        "ok",
        "ok",
        "FEHLER: route: invalid parameter name in pattern (expected `:name`)",
        "FEHLER: route: empty path segment in pattern",
        "FEHLER: route: pattern must start with `/`",
        "FEHLER: route: method must be 1..16 ASCII letters",
        "FEHLER: route: this pattern is already registered for this method",
        "FEHLER: route: ambiguous route: a route with the same shape is already registered for this method",
        "FEHLER: route: this name is already used for a route of this method",
        "FEHLER: route: duplicate parameter name in pattern",
        "---",
        "status 200 route=home allow=",
        "status 200 route=list_users allow=",
        "status 200 route=create_user allow=",
        "status 200 route=profile allow=",
        "status 200 route=show_user allow=",
        "status 200 route=show_user allow=",
        "status 200 route=list_users allow=",
        "status 405 route= allow=GET, POST",
        "status 405 route= allow=GET",
        "status 404 route= allow=",
        "status 404 route= allow=",
        "FEHLER: resolve: method must be 1..16 ASCII letters",
    ];
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), expected.len(), "{stdout}");
    for (i, (got, want)) in lines.iter().zip(expected.iter()).enumerate() {
        assert_eq!(got, want, "line {i}: {stdout}");
    }
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    assert!(out.stderr.contains("live sockets: 0"), "{}", out.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn web_params_and_query_natively() {
    let dir = temp_dir("web-params");
    let file = dir.join("w.blm");
    let src = r#"use std::web;
use std::json;

fn show(r: Result<web::Match, string>) -> string {
    return match r {
        Result::Ok(m) => "status " + str(m.status) + " " + match json::stringify(m.params) {
            Result::Ok(s) => s,
            Result::Err(e) => "ERR"
        },
        Result::Err(e) => "FEHLER: " + e
    };
}

fn main() -> int {
    let router = web::router();
    web::route(router, "GET", "/users/:id", "user");
    web::route(router, "GET", "/a/:x/b/:y", "pair");
    println(show(web::resolve(router, "GET", "/users/42?q=hello+world&x=1&x=2&flag")));
    println(show(web::resolve(router, "GET", "/users/42?id=99")));
    println(show(web::resolve(router, "GET", "/users/a%2Fb")));
    println(show(web::resolve(router, "GET", "/users/%C3%A4%C3%B6")));
    println(show(web::resolve(router, "GET", "/users/42?=x&a=1")));
    println(show(web::resolve(router, "GET", "/users/42?a=b=c")));
    println(show(web::resolve(router, "GET", "/a/1/b/2")));
    println(show(web::resolve(router, "GET", "/users/%zz")));
    println(show(web::resolve(router, "GET", "/users/%FF")));
    println(show(web::resolve(router, "GET", "/users/42?bad=%zz")));
    println(show(web::resolve(router, "GET", "/users/42?bad=%FF")));
    var many = "?";
    var i = 0;
    while i < 260 {
        if i > 0 {
            many = many + "&";
        }
        many = many + "k" + str(i) + "=1";
        i = i + 1;
    }
    println(show(web::resolve(router, "GET", "/users/42" + many)));
    var deep = "";
    i = 0;
    while i < 260 {
        deep = deep + "/a";
        i = i + 1;
    }
    println(show(web::resolve(router, "GET", deep)));
    return 0;
}
"#;
    write_file(&file, src);
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    let expected = [
        "status 200 {\"id\":\"42\",\"q\":\"hello world\",\"x\":\"1\",\"flag\":\"\"}",
        "status 200 {\"id\":\"42\"}",
        "status 200 {\"id\":\"a/b\"}",
        "status 200 {\"id\":\"äö\"}",
        "status 200 {\"id\":\"42\",\"a\":\"1\"}",
        "status 200 {\"id\":\"42\",\"a\":\"b=c\"}",
        "status 200 {\"x\":\"1\",\"y\":\"2\"}",
        "FEHLER: resolve: invalid percent-encoding in path",
        "FEHLER: resolve: invalid UTF-8 in decoded path segment",
        "FEHLER: resolve: invalid percent-encoding in query",
        "FEHLER: resolve: invalid UTF-8 in decoded query value",
        "FEHLER: resolve: too many query parameters (max 256)",
        "FEHLER: resolve: too many path segments (max 256)",
    ];
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), expected.len(), "{stdout}");
    for (i, (got, want)) in lines.iter().zip(expected.iter()).enumerate() {
        assert_eq!(got, want, "line {i}: {stdout}");
    }
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    assert!(out.stderr.contains("live sockets: 0"), "{}", out.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn web_json_helpers_natively() {
    let dir = temp_dir("web-json-helpers");
    let file = dir.join("w.blm");
    let src = r#"use std::web;
use std::json;

fn show_bool(r: Result<bool, string>) -> string {
    return match r {
        Result::Ok(v) => str(v),
        Result::Err(e) => "FEHLER: " + e
    };
}

fn show_json(r: Result<json::JsonValue, string>) -> string {
    return match r {
        Result::Ok(v) => "JSON " + json::type_of(v),
        Result::Err(e) => "FEHLER: " + e
    };
}

fn main() -> int {
    println(show_json(web::json_body("{\"ok\": true}")));
    println(show_json(web::json_body("")));
    println(show_json(web::json_body("[1,2")));
    println("---");
    // Header-Validierung (vor dem Schreiben, daher ohne echten Socket prüfbar)
    println(show_bool(web::json_response_with(12345, 200, json::null(), "Content-Length: 5")));
    println(show_bool(web::json_response_with(12345, 200, json::null(), "nonsense")));
    println(show_bool(web::json_response_with(12345, 200, json::null(), "X-Test: a\rb")));
    println(show_bool(web::json_response_with(12345, 999, json::null(), "")));
    println(show_bool(web::json_response_with(12345, 200, json::null(), "X-Ok: yes")));
    println(show_bool(web::json_response(12345, 200, json::null())));
    return 0;
}
"#;
    write_file(&file, src);
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(stdout.contains("JSON object"), "{stdout}");
    assert!(
        stdout.contains("FEHLER: json_body: empty request body"),
        "{stdout}"
    );
    assert!(stdout.contains("FEHLER: unterminated array"), "{stdout}");
    assert!(
        stdout.contains("FEHLER: write_response_full: `Content-Length`, `Connection` and `Transfer-Encoding` are managed by the library"),
        "{stdout}"
    );
    assert!(
        stdout.contains("FEHLER: write_response_full: malformed extra headers"),
        "{stdout}"
    );
    assert!(
        stdout.contains("FEHLER: write_response_full: header values must not contain CR or LF"),
        "{stdout}"
    );
    assert!(
        stdout.contains("FEHLER: write_response_full: status out of range (100..599)"),
        "{stdout}"
    );
    assert!(
        stdout.contains("FEHLER: tcp_write_all: unknown, closed or wrong-kind handle (12345)"),
        "{stdout}"
    );
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    assert!(out.stderr.contains("live sockets: 0"), "{}", out.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_web_examples() {
    let cases: &[(&str, &[&str])] = &[
        (
            "web_router.blm",
            &[
                "status 200 route=home allow=",
                "status 200 route=profile allow=",
                "status 200 route=show_user allow=",
                "status 405 route= allow=GET, POST",
                "status 404 route= allow=",
                "FEHLER: route: ambiguous route",
                "FEHLER: route: this pattern is already registered",
            ],
        ),
        (
            "web_path_params.blm",
            &[
                "category: rust",
                "slug:     ownership",
                "page:     2",
                "draft:    []",
                "category: a/b",
                "q:        hello world",
                "q:        bloom",
                "limit:    10",
            ],
        ),
        (
            "web_errors.blm",
            &[
                "status 404 allow=[]",
                "status 405 allow=[GET]",
                "status 405 allow=[GET, POST]",
                "400 bad request: resolve: invalid percent-encoding in path",
                "JSON: object",
                "400 bad request: json_body: empty request body",
            ],
        ),
    ];
    for (name, needles) in cases {
        let out = run_bloom_env(&["run", &example(name)], &[("BLOOM_MEM_REPORT", "1")]);
        if no_toolchain(&out) {
            eprintln!("skipping: no C toolchain available");
            return;
        }
        assert_eq!(out.code, 0, "{name}: {}", out.combined());
        let stdout = norm(&out.stdout);
        for needle in *needles {
            assert!(stdout.contains(needle), "{name}: {stdout}");
        }
        assert!(
            out.stderr.contains("live allocations: 0"),
            "{name}: {}",
            out.combined()
        );
        assert!(
            out.stderr.contains("live sockets: 0"),
            "{name}: {}",
            out.combined()
        );
    }
}


#[test]
fn web_rest_server_natively() {
    use std::io::{BufRead, BufReader, Read, Write};

    let dir = temp_dir("web-rest");
    // Build and start the example server; it prints its port and serves 10 requests.
    let exe = dir.join(if cfg!(windows) {
        "web_server.exe"
    } else {
        "web_server"
    });
    let build = run_bloom(&[
        "build",
        &example("web_json_api.blm"),
        "-o",
        exe.to_string_lossy().as_ref(),
    ]);
    if no_toolchain(&build) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(build.code, 0, "{}", build.combined());
    let mut child = std::process::Command::new(&exe)
        .env("BLOOM_MEM_REPORT", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn server");
    let mut reader = BufReader::new(child.stdout.take().expect("stdout"));
    let mut port_line = String::new();
    reader.read_line(&mut port_line).expect("port");
    let port: u16 = port_line.trim().parse().expect("port number");

    let request = |method: &str, target: &str, body: &str| -> String {
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
        let head = format!(
            "{method} {target} HTTP/1.1\r\nHost: bloom\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(head.as_bytes()).expect("write head");
        stream.write_all(body.as_bytes()).expect("write body");
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .unwrap_or_else(|e| panic!("read failed for {method} {target}: {e}"));
        response
    };

    // 1) JSON echo round trip
    let r1 = request("POST", "/echo", "{\"name\": \"ada\", \"n\": 3}");
    assert!(r1.starts_with("HTTP/1.1 200 OK\r\n"), "{r1}");
    assert!(r1.contains("Content-Type: application/json\r\n"), "{r1}");
    assert!(
        r1.ends_with("{\"received\":{\"name\":\"ada\",\"n\":3},\"route\":\"echo\"}"),
        "{r1}"
    );
    // 2) invalid JSON body -> 400
    let r2 = request("POST", "/echo", "{broken");
    assert!(r2.starts_with("HTTP/1.1 400 Bad Request\r\n"), "{r2}");
    assert!(r2.contains("invalid json:"), "{r2}");
    // 3) JSON response of an empty object
    let r3 = request("GET", "/status", "");
    assert!(r3.starts_with("HTTP/1.1 200 OK\r\n"), "{r3}");
    assert!(r3.ends_with("{}"), "{r3}");
    // 4) unknown route -> 404
    let r4 = request("GET", "/missing", "");
    assert!(r4.starts_with("HTTP/1.1 404 Not Found\r\n"), "{r4}");
    assert!(r4.ends_with("not found"), "{r4}");
    // 5) known path, wrong method -> 405 with Allow
    let r5 = request("DELETE", "/status", "");
    assert!(r5.starts_with("HTTP/1.1 405 "), "{r5}");
    assert!(r5.contains("Allow: GET\r\n"), "{r5}");
    // 6) path parameter
    let r6 = request("GET", "/users/42", "");
    assert!(r6.starts_with("HTTP/1.1 200 OK\r\n"), "{r6}");
    assert!(
        r6.ends_with("{\"id\":\"42\",\"route\":\"show_user\"}"),
        "{r6}"
    );
    // 7) malformed percent encoding -> 400 from resolve
    let r7 = request("GET", "/users/%zz", "");
    assert!(r7.starts_with("HTTP/1.1 400 Bad Request\r\n"), "{r7}");
    assert!(r7.contains("invalid percent-encoding"), "{r7}");
    // 8) 204 has no body despite a passed body
    let r8 = request("GET", "/empty", "");
    assert!(r8.starts_with("HTTP/1.1 204 No Content\r\n"), "{r8}");
    assert!(r8.contains("Content-Length: 0\r\n"), "{r8}");
    assert!(!r8.contains("ignored"), "{r8}");
    // 9) client example against the same server
    let client = run_bloom_stdin_env(
        &["run", &example("web_client_json.blm")],
        &format!("{port}\n"),
        &[("BLOOM_MEM_REPORT", "1")],
    );
    assert_eq!(client.code, 0, "{}", client.combined());
    let client_out = norm(&client.stdout);
    assert!(client_out.contains("Status: 200"), "{client_out}");
    assert!(
        client_out.contains("Content-Type: application/json"),
        "{client_out}"
    );
    assert!(client_out.contains("Body: {}"), "{client_out}");
    assert!(
        client.stderr.contains("live allocations: 0"),
        "{}",
        client.combined()
    );
    // 10) last connection lets the server exit cleanly
    let r10 = request("GET", "/status", "");
    assert!(r10.starts_with("HTTP/1.1 200 OK\r\n"), "{r10}");

    let status = child.wait().expect("wait");
    let mut rest = String::new();
    reader.read_to_string(&mut rest).expect("rest");
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("stderr")
        .read_to_string(&mut stderr)
        .expect("stderr read");
    assert_eq!(status.code(), Some(0), "{rest}\n{stderr}");
    assert!(stderr.contains("live allocations: 0"), "{stderr}");
    assert!(stderr.contains("live sockets: 0"), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fs_copy_rename_remove_and_errors_natively() {
    let dir = temp_dir("fs-ops");
    let file = dir.join("o.blm");
    write_file(
        &file,
        "use std::fs;\n\nfn show(result: Result<int, string>) -> string {\n    return match result {\n        Result::Ok(v) => str(v),\n        Result::Err(e) => e\n    };\n}\n\nfn main() -> int {\n    let dir = \"bloom_fs_ops\";\n    match fs::create_dir_all(dir) {\n        Result::Ok(_) => println(\"ok\"),\n        Result::Err(e) => println(e)\n    };\n    let a = fs::join_path(dir, \"a.txt\");\n    let b = fs::join_path(dir, \"b.txt\");\n    let c = fs::join_path(dir, \"c.txt\");\n    match fs::write_text(a, \"0123456789\") {\n        Result::Ok(_) => println(\"w\"),\n        Result::Err(e) => println(e)\n    };\n    println(show(fs::copy_file(a, b)));\n    match fs::rename(b, c) {\n        Result::Ok(_) => println(\"moved\"),\n        Result::Err(e) => println(e)\n    };\n    println(match fs::exists(b) {\n        Result::Ok(v) => str(v),\n        Result::Err(e) => e\n    });\n    println(show(fs::file_size(c)));\n    match fs::remove_file(c) {\n        Result::Ok(_) => println(\"removed\"),\n        Result::Err(e) => println(e)\n    };\n    println(show(fs::file_size(c)));\n    println(match fs::read_dir(\"gibt_es_nicht\") {\n        Result::Ok(_) => \"unerwartet\",\n        Result::Err(e) => \"err: \" + e\n    });\n    match fs::remove_file(a) {\n        Result::Ok(_) => println(\"cleanup\"),\n        Result::Err(e) => println(e)\n    };\n    match fs::remove_dir(dir) {\n        Result::Ok(_) => println(\"done\"),\n        Result::Err(e) => println(e)\n    };\n    return 0;\n}\n",
    );
    let demo = dir.join("cwd");
    std::fs::create_dir_all(&demo).expect("cwd");
    let out = run_bloom_env_in(
        &demo,
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(
        stdout.starts_with("ok\nw\n10\nmoved\nfalse\n10\nremoved\n"),
        "{stdout}"
    );
    assert!(stdout.contains("file not found"), "{stdout}");
    assert!(stdout.ends_with("cleanup\ndone\n"), "{stdout}");
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fs_invalid_utf8_rejected() {
    let dir = temp_dir("fs-utf8");
    let data = dir.join("bad.bin");
    std::fs::write(&data, [0xff, 0xfe, 0x41]).expect("write");
    let file = dir.join("u.blm");
    let lit = data.to_string_lossy().replace('\\', "\\\\");
    write_file(
        &file,
        &format!(
            "use std::fs;\nfn main() -> int {{\n    println(match fs::read_text(\"{lit}\") {{\n        Result::Ok(_) => \"ok\",\n        Result::Err(e) => e\n    }});\n    return 0;\n}}\n"
        ),
    );
    let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert!(
        norm(&out.stdout).contains("not valid UTF-8"),
        "{}",
        out.combined()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn process_run_natively() {
    let dir = temp_dir("process-run");
    let file = dir.join("p.blm");
    write_file(
        &file,
        "use std::process;\n\nfn code(o: process::ProcessOutput) -> string {\n    return str(o.exit_code);\n}\n\nfn both(o: process::ProcessOutput) -> string {\n    return \"out=[\" + o.stdout + \"] err=[\" + o.stderr + \"]\";\n}\n\nfn main() -> int {\n    let r = process::run(\"cmd.exe\", [\"/C\", \"echo\", \"Hallo\", \"Welt\"]);\n    print(match r {\n        Result::Ok(o) => o.stdout,\n        Result::Err(e) => e\n    });\n    let r2 = process::run(\"cmd.exe\", [\"/C\", \"exit\", \"5\"]);\n    println(match r2 {\n        Result::Ok(o) => code(o),\n        Result::Err(e) => e\n    });\n    let r3 = process::run(\"cmd.exe\", [\"/C\", \"echo fehler 1>&2\"]);\n    println(match r3 {\n        Result::Ok(o) => both(o),\n        Result::Err(e) => e\n    });\n    let r4 = process::run(\"gibt_es_nicht_xyz.exe\", []);\n    println(match r4 {\n        Result::Ok(o) => code(o),\n        Result::Err(e) => e\n    });\n    let r5 = process::run_in(\"cmd.exe\", [\"/C\", \"cd\"], \"C:\\\\Windows\");\n    print(match r5 {\n        Result::Ok(o) => o.stdout,\n        Result::Err(e) => e\n    });\n    return 0;\n}\n",
    );
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(stdout.contains("Hallo Welt"), "{stdout}");
    assert!(stdout.contains("\n5\n"), "{stdout}");
    assert!(stdout.contains("out=[] err=[fehler"), "{stdout}");
    assert!(stdout.contains("program not found"), "{stdout}");
    assert!(stdout.contains("C:\\Windows"), "{stdout}");
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn process_large_output_no_deadlock() {
    let dir = temp_dir("process-large");
    let file = dir.join("pl.blm");
    write_file(
        &file,
        "use std::process;\n\nfn sizes(o: process::ProcessOutput) -> string {\n    return str(len(o.stdout)) + \"/\" + str(len(o.stderr));\n}\n\nfn main() -> int {\n    let r = process::run(\"cmd.exe\", [\"/C\", \"for /L %i in (1,1,800) do @(echo out %i & echo err %i 1>&2)\"]);\n    println(match r {\n        Result::Ok(o) => sizes(o),\n        Result::Err(e) => e\n    });\n    return 0;\n}\n",
    );
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    let parts: Vec<&str> = stdout.trim().split('/').collect();
    assert_eq!(parts.len(), 2, "{stdout}");
    let out_len: usize = parts[0].parse().expect("stdout length");
    let err_len: usize = parts[1].parse().expect("stderr length");
    assert!(out_len > 4000 && err_len > 4000, "{stdout}");
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn time_module_natively() {
    let dir = temp_dir("time-module");
    let file = dir.join("t.blm");
    write_file(
        &file,
        "use std::time;\n\nfn main() -> int {\n    let secs = time::unix_seconds();\n    let millis = time::unix_millis();\n    println(str(secs > 1700000000));\n    println(str(millis / 1000 - secs < 2));\n    let t0 = time::monotonic_millis();\n    time::sleep_ms(60);\n    let elapsed = time::monotonic_millis() - t0;\n    println(str(elapsed >= 40));\n    println(str(elapsed < 10000));\n    println(time::format_utc(0));\n    println(time::format_utc(1000000000));\n    println(time::format_utc(-1));\n    println(time::format_utc(253402300799));\n    time::sleep_ms(0);\n    return 0;\n}\n",
    );
    assert_no_leaks(
        &dir,
        &file,
        "true\ntrue\ntrue\ntrue\n1970-01-01T00:00:00Z\n2001-09-09T01:46:40Z\n1969-12-31T23:59:59Z\n9999-12-31T23:59:59Z\n",
    );
}

#[test]
fn time_errors_natively() {
    let cases = [
        (
            "format-range",
            "println(time::format_utc(253402300800));",
            "out of representable range",
        ),
        ("sleep-negative", "time::sleep_ms(-1);", "negative duration"),
    ];
    for (name, body, expected) in cases {
        let dir = temp_dir(&format!("time-err-{name}"));
        let file = dir.join("e.blm");
        write_file(
            &file,
            &format!("use std::time;\nfn main() -> int {{\n    {body}\n    return 0;\n}}\n"),
        );
        let out = run_bloom(&["run", file.to_string_lossy().as_ref()]);
        if no_toolchain(&out) {
            eprintln!("skipping: no C toolchain available");
            return;
        }
        assert_eq!(out.code, 70, "{}: {}", name, out.combined());
        assert!(
            out.stderr.contains(expected),
            "{}: {}",
            name,
            out.combined()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn run_process_example() {
    let out = run_bloom_env(
        &["run", &example("process.blm")],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(stdout.contains("Hallo aus einem Prozess"), "{stdout}");
    assert!(stdout.contains("exit-Code: 3"), "{stdout}");
    assert!(stdout.contains("cwd: C:\\Windows"), "{stdout}");
    assert!(stdout.contains("Startfehler"), "{stdout}");
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
}

#[test]
fn run_time_example() {
    let out = run_bloom_env(&["run", &example("time.blm")], &[("BLOOM_MEM_REPORT", "1")]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(stdout.contains("Epoche: 1970-01-01T00:00:00Z"), "{stdout}");
    assert!(stdout.contains("1e9:    2001-09-09T01:46:40Z"), "{stdout}");
    assert!(stdout.contains("mindestens 100ms: true"), "{stdout}");
    assert!(stdout.contains("sleep_ms(0) ok"), "{stdout}");
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
}

#[test]
fn fmt_preserves_comments_and_literals() {
    let dir = temp_dir("fmt-comments");
    let file = dir.join("c.blm");
    let src = "// Dateikommentar\nuse std::math; // Import-Kommentar\n/* Block\n   mehrzeilig */\n\nfn main() -> int {\n    // im Koerper\n    let s = \"a\\tb\\u{1F600}\";\n    let c = '\\n';\n    let f = 1.50;\n    let m = math::max_int(1, 2); // trailing\n    /* inline */\n    println(s); // ende\n    return 0;\n}\n// EOF-Kommentar\n";
    write_file(&file, src);
    let out = run_bloom(&["fmt", file.to_string_lossy().as_ref()]);
    assert_eq!(out.code, 0, "{}", out.combined());
    let formatted = std::fs::read_to_string(&file).expect("read");
    for needle in [
        "// Dateikommentar",
        "// Import-Kommentar",
        "/* Block",
        "mehrzeilig */",
        "// im Koerper",
        "// trailing",
        "/* inline */",
        "// ende",
        "// EOF-Kommentar",
        "\"a\\tb\\u{1F600}\"",
        "'\\n'",
        "1.50",
    ] {
        assert!(
            formatted.contains(needle),
            "missing {needle:?} in:\n{formatted}"
        );
    }
    // The formatted file is valid and idempotent.
    let check = run_bloom(&["check", file.to_string_lossy().as_ref()]);
    assert_eq!(check.code, 0, "{}", check.combined());
    let again = run_bloom(&["fmt", "--check", file.to_string_lossy().as_ref()]);
    assert_eq!(again.code, 0, "{}", again.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fmt_refuses_invalid_source() {
    let dir = temp_dir("fmt-invalid");
    let file = dir.join("bad.blm");
    let src = "fn main() -> int { let x = ; return 0; }";
    write_file(&file, src);
    let out = run_bloom(&["fmt", file.to_string_lossy().as_ref()]);
    assert_eq!(out.code, 1, "{}", out.combined());
    assert!(
        out.stderr.contains("konnte nicht formatiert werden"),
        "{}",
        out.combined()
    );
    let after = std::fs::read_to_string(&file).expect("read");
    assert_eq!(after, src, "file must not be overwritten");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fmt_formats_examples_corpus() {
    let dir = temp_dir("fmt-corpus");
    let examples_dir = root().join("examples");
    for entry in std::fs::read_dir(&examples_dir).expect("examples") {
        let entry = entry.expect("entry");
        let path = entry.path();
        if path.extension().map(|e| e == "blm").unwrap_or(false) {
            let name = path.file_name().expect("name");
            std::fs::copy(&path, dir.join(name)).expect("copy");
        }
    }
    let out = run_bloom(&["fmt", dir.to_string_lossy().as_ref()]);
    assert_eq!(out.code, 0, "{}", out.combined());
    // Every formatted file parses and typechecks.
    for entry in std::fs::read_dir(&dir).expect("dir") {
        let path = entry.expect("entry").path();
        let check = run_bloom(&["check", path.to_string_lossy().as_ref()]);
        assert_eq!(
            check.code,
            0,
            "check failed for {}:\n{}",
            path.display(),
            check.combined()
        );
    }
    // Formatting is idempotent over the whole corpus.
    let check_mode = run_bloom(&["fmt", "--check", dir.to_string_lossy().as_ref()]);
    assert_eq!(check_mode.code, 0, "{}", check_mode.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn stdlib_integration_across_modules() {
    let dir = temp_dir("stdlib-integration");
    write_file(
        &dir.join("main.blm"),
        "use tools;\nuse tools::Report;\n\nfn main() -> int {\n    let r = tools::run_report(\"bloom_integration_demo\");\n    println(match r {\n        Result::Ok(rep) => tools::render(rep),\n        Result::Err(e) => \"FEHLER: \" + e\n    });\n    return 0;\n}\n",
    );
    write_file(
        &dir.join("tools.blm"),
        "use std::fs;\nuse std::process;\nuse std::time;\n\npub struct Report {\n    lines: int,\n    stdout_len: int,\n    took_ms: int\n}\n\npub fn run_report(dir: string) -> Result<Report, string> {\n    fs::create_dir_all(dir)?;\n    let file = fs::join_path(dir, \"out.txt\");\n    let start = time::monotonic_millis();\n    let proc = process::run(\"cmd.exe\", [\"/C\", \"echo\", \"zeile1 & echo zeile2\"])?;\n    let took = time::monotonic_millis() - start;\n    let written = fs::write_text(file, proc.stdout)?;\n    fs::read_dir(dir)?;\n    fs::remove_file(file)?;\n    fs::remove_dir(dir)?;\n    return Result::Ok(Report {\n        lines: 2,\n        stdout_len: written,\n        took_ms: took\n    });\n}\n\npub fn render(r: Report) -> string {\n    return \"lines=\" + str(r.lines) + \" bytes=\" + str(r.stdout_len) + \" took>=0=\" + str(r.took_ms >= 0) + \" epoch=\" + time::format_utc(0);\n}\n",
    );
    let cwd = dir.join("cwd");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let out = run_bloom_env_in(
        &cwd,
        &["run", dir.join("main.blm").to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(stdout.contains("lines=2"), "{stdout}");
    assert!(stdout.contains("took>=0=true"), "{stdout}");
    assert!(stdout.contains("epoch=1970-01-01T00:00:00Z"), "{stdout}");
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- 0.10.0: FFI -------------------------------------------------------

#[test]
fn ffi_basic_natively() {
    let dir = temp_dir("ffi-basic");
    let lib = dir.join("ffi_lib.c");
    write_file(
        &lib,
        "int bloom_ffi_add(int a, int b) { return a + b; }\nunsigned int bloom_ffi_bits(unsigned int v) { return v ^ 0xFFu; }\nlong long bloom_ffi_big(long long v) { return v * 2; }\nfloat bloom_ffi_half(float x) { return x / 2.0f; }\nfloat bloom_ffi_pi(void) { return 3.25f; }\nvoid bloom_ffi_touch(void) { }\nvoid *bloom_ffi_echo_ptr(void *p) { return p; }\n",
    );
    let file = dir.join("f.blm");
    write_file(
        &file,
        "extern \"C\" {\n    fn bloom_ffi_add(a: c_int, b: c_int) -> c_int;\n    fn bloom_ffi_bits(v: c_uint) -> c_uint;\n    fn bloom_ffi_big(v: c_longlong) -> c_longlong;\n    fn bloom_ffi_half(x: f32) -> f32;\n    fn bloom_ffi_pi() -> f32;\n    fn bloom_ffi_touch();\n    fn bloom_ffi_echo_ptr(p: rawptr) -> rawptr;\n}\n\nfn main() -> int {\n    unsafe {\n        println(int(bloom_ffi_add(c_int(2), c_int(3))));\n        println(int(bloom_ffi_bits(c_uint(1))));\n        println(int(bloom_ffi_big(c_longlong(21))));\n        println(float(bloom_ffi_half(f32(5.0))));\n        println(float(bloom_ffi_pi()));\n        bloom_ffi_touch();\n        let p = bloom_ffi_echo_ptr(rawptr_null());\n        println(int(p == rawptr_null()));\n    }\n    return 0;\n}\n",
    );
    let out = run_bloom_env(
        &[
            "run",
            "--link",
            lib.to_string_lossy().as_ref(),
            file.to_string_lossy().as_ref(),
        ],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "5\n254\n42\n2.5\n3.25\n1\n");
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ffi_requires_unsafe_and_valid_declarations() {
    let dir = temp_dir("ffi-errors");
    let cases = [
        (
            "no_unsafe.blm",
            "extern \"C\" {\n    fn f(a: c_int) -> c_int;\n}\nfn main() -> int {\n    let x = f(c_int(1));\n    return int(x);\n}\n",
            "E1070",
        ),
        (
            "bad_abi.blm",
            "extern \"rust\" {\n    fn f() -> c_int;\n}\nfn main() -> int { return 0; }\n",
            "E1071",
        ),
        (
            "bad_type.blm",
            "extern \"C\" {\n    fn f(s: string) -> c_int;\n}\nfn main() -> int { return 0; }\n",
            "E1072",
        ),
        (
            "bad_ret.blm",
            "extern \"C\" {\n    fn f() -> [int];\n}\nfn main() -> int { return 0; }\n",
            "E1072",
        ),
        (
            "duplicate.blm",
            "extern \"C\" {\n    fn f() -> c_int;\n    fn f(a: c_int) -> c_int;\n}\nfn main() -> int { return 0; }\n",
            "E1010",
        ),
        (
            "dup_symbol.blm",
            "extern \"C\" {\n    fn a() -> c_int;\n    fn b() -> c_int;\n}\nfn b() -> c_int { return 0; }\nfn main() -> int { return 0; }\n",
            "E1010",
        ),
    ];
    for (name, src, code) in cases {
        let file = dir.join(name);
        write_file(&file, src);
        let out = run_bloom(&["check", file.to_string_lossy().as_ref()]);
        assert_eq!(out.code, 1, "{}: {}", name, out.combined());
        assert!(out.stderr.contains(code), "{}: {}", name, out.combined());
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ffi_missing_symbol_fails_at_link_time() {
    let dir = temp_dir("ffi-missing");
    let file = dir.join("m.blm");
    write_file(
        &file,
        "extern \"C\" {\n    fn bloom_ffi_not_defined_anywhere() -> c_int;\n}\nfn main() -> int {\n    unsafe {\n        return int(bloom_ffi_not_defined_anywhere());\n    }\n}\n",
    );
    let out = run_bloom(&["build", file.to_string_lossy().as_ref()]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 4, "{}", out.combined());
    assert!(out.stderr.contains("E3003"), "{}", out.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ffi_link_libraries_config() {
    let dir = temp_dir("ffi-config");
    write_file(
        &dir.join("bloom.toml"),
        "[package]\nname = \"ffitest\"\nversion = \"0.1.0\"\nentry = \"src/main.blm\"\n\n[build]\nlink_libraries = [\"kernel32\"]\n",
    );
    write_file(
        &dir.join("src/main.blm"),
        "extern \"system\" {\n    fn GetCurrentProcessId() -> c_uint;\n}\nfn main() -> int {\n    unsafe {\n        println(int(GetCurrentProcessId()) > 0);\n    }\n    return 0;\n}\n",
    );
    let out = run_bloom_in(&dir, &["run"]);
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "true\n");
    // Invalid library names are rejected (manifest validation).
    write_file(
        &dir.join("bloom.toml"),
        "[package]\nname = \"ffitest\"\nversion = \"0.1.0\"\nentry = \"src/main.blm\"\n\n[build]\nlink_libraries = [\"../evil\"]\n",
    );
    let bad = run_bloom_in(&dir, &["run"]);
    assert_eq!(bad.code, 5, "{}", bad.combined());
    assert!(
        bad.stderr.contains("not a valid library name"),
        "{}",
        bad.combined()
    );
    // The CLI `--link` option validates its arguments too.
    let bad_cli = run_bloom(&[
        "build",
        "--link",
        "../evil",
        dir.join("src/main.blm").to_string_lossy().as_ref(),
    ]);
    assert_eq!(bad_cli.code, 5, "{}", bad_cli.combined());
    assert!(bad_cli.stderr.contains("E3005"), "{}", bad_cli.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- 0.10.0: std::win32 ------------------------------------------------

#[test]
fn win32_module_natively() {
    let dir = temp_dir("win32");
    let file = dir.join("w.blm");
    write_file(
        &file,
        "use std::win32;\n\nfn name_ok(r: Result<string, string>) -> bool {\n    return match r {\n        Result::Ok(name) => len(name) > 0,\n        Result::Err(_) => false\n    };\n}\n\nfn main() -> int {\n    println(win32::process_id() > 0);\n    println(win32::tick_count_ms() > 0);\n    println(win32::last_error() >= 0);\n    println(name_ok(win32::get_computer_name()));\n    let t0 = win32::tick_count_ms();\n    win32::sleep_ms(40);\n    let t1 = win32::tick_count_ms();\n    println(t1 - t0 >= 30);\n    return 0;\n}\n",
    );
    assert_no_leaks(&dir, &file, "true\ntrue\ntrue\ntrue\ntrue\n");
}

// ---- 0.10.0: std::net --------------------------------------------------

#[test]
fn net_loopback_natively() {
    let dir = temp_dir("net-loopback");
    let file = dir.join("n.blm");
    write_file(
        &file,
        "use std::net;\n\nfn ok_int(r: Result<int, string>) -> int {\n    return match r {\n        Result::Ok(v) => v,\n        Result::Err(e) => -1\n    };\n}\n\nfn ok_bool(r: Result<bool, string>) -> int {\n    return match r {\n        Result::Ok(v) => 1,\n        Result::Err(e) => -1\n    };\n}\n\nfn opt_str(r: Result<Option<string>, string>) -> string {\n    return match r {\n        Result::Ok(o) => match o {\n            Option::Some(s) => s,\n            Option::None => \"EOF\"\n        },\n        Result::Err(e) => \"ERR: \" + e\n    };\n}\n\nfn opt_len(r: Result<Option<string>, string>) -> int {\n    return match r {\n        Result::Ok(o) => match o {\n            Option::Some(s) => len(s),\n            Option::None => -1\n        },\n        Result::Err(e) => -2\n    };\n}\n\nfn err_of_bool(r: Result<bool, string>) -> string {\n    return match r {\n        Result::Ok(_) => \"unerwartet\",\n        Result::Err(e) => e\n    };\n}\n\nfn err_of_opt(r: Result<Option<string>, string>) -> string {\n    return match r {\n        Result::Ok(_) => \"unerwartet\",\n        Result::Err(e) => e\n    };\n}\n\nfn main() -> int {\n    let listener = ok_int(net::tcp_listen(\"127.0.0.1\", 0, 4));\n    let port = ok_int(net::local_port(listener));\n    println(port > 0);\n    let client = ok_int(net::tcp_connect(\"127.0.0.1\", port));\n    let server = ok_int(net::tcp_accept(listener));\n    println(ok_int(net::tcp_write_all(client, \"hallo server\")));\n    println(opt_str(net::tcp_read(server, 64)));\n    var big = \"\";\n    var i = 0;\n    while i < 200 {\n        big = big + \"0123456789\";\n        i = i + 1;\n    }\n    println(ok_int(net::tcp_write_all(server, big)));\n    println(opt_len(net::tcp_read(client, 4096)));\n    println(ok_bool(net::tcp_close(server)));\n    println(opt_str(net::tcp_read(client, 64)));\n    println(err_of_bool(net::tcp_close(server)));\n    println(err_of_opt(net::tcp_read(999999, 10)));\n    println(ok_bool(net::tcp_close(client)));\n    println(ok_bool(net::tcp_close(listener)));\n    return 0;\n}\n",
    );
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(
        stdout.starts_with("true\n12\nhallo server\n2000\n2000\n1\nEOF\n"),
        "{stdout}"
    );
    assert!(
        stdout.contains("unknown, closed or wrong-kind handle"),
        "{stdout}"
    );
    assert!(stdout.ends_with("1\n1\n"), "{stdout}");
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn net_errors_natively() {
    let dir = temp_dir("net-errors");
    let file = dir.join("e.blm");
    write_file(
        &file,
        "use std::net;\n\nfn err_int(r: Result<int, string>) -> string {\n    return match r {\n        Result::Ok(_) => \"unerwartet\",\n        Result::Err(e) => e\n    };\n}\n\nfn err_bool(r: Result<bool, string>) -> string {\n    return match r {\n        Result::Ok(_) => \"unerwartet\",\n        Result::Err(e) => e\n    };\n}\n\nfn err_opt(r: Result<Option<string>, string>) -> string {\n    return match r {\n        Result::Ok(_) => \"unerwartet\",\n        Result::Err(e) => e\n    };\n}\n\nfn main() -> int {\n    println(err_int(net::tcp_connect(\"127.0.0.1\", 0)));\n    println(err_int(net::tcp_connect(\"127.0.0.1\", 70000)));\n    println(err_int(net::tcp_listen(\"127.0.0.1\", 0, 0)));\n    println(err_opt(net::tcp_read(1, 0)));\n    println(err_bool(net::tcp_set_timeout(1, -5)));\n    println(err_int(net::tcp_connect(\"no-such-host-xyz.invalid\", 80)));\n    return 0;\n}\n",
    );
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(stdout.contains("port out of range"), "{stdout}");
    assert!(stdout.contains("backlog must be at least 1"), "{stdout}");
    assert!(
        stdout.contains("unknown, closed or wrong-kind handle"),
        "{stdout}"
    );
    assert!(stdout.contains("negative timeout"), "{stdout}");
    assert!(stdout.contains("cannot resolve host"), "{stdout}");
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn net_timeout_and_refused_natively() {
    let dir = temp_dir("net-timeout");
    let file = dir.join("t.blm");
    write_file(
        &file,
        "use std::net;\n\nfn ok_int(r: Result<int, string>) -> int {\n    return match r {\n        Result::Ok(v) => v,\n        Result::Err(e) => -1\n    };\n}\n\nfn ok_bool(r: Result<bool, string>) -> int {\n    return match r {\n        Result::Ok(v) => 1,\n        Result::Err(e) => -1\n    };\n}\n\nfn read_result(r: Result<Option<string>, string>) -> string {\n    return match r {\n        Result::Ok(o) => match o {\n            Option::Some(_) => \"data\",\n            Option::None => \"EOF\"\n        },\n        Result::Err(e) => e\n    };\n}\n\nfn main() -> int {\n    // Lesetimeout: verbunden, aber keine Daten\n    let listener = ok_int(net::tcp_listen(\"127.0.0.1\", 0, 1));\n    let port = ok_int(net::local_port(listener));\n    let client = ok_int(net::tcp_connect(\"127.0.0.1\", port));\n    let server = ok_int(net::tcp_accept(listener));\n    println(ok_bool(net::tcp_set_timeout(server, 100)));\n    println(read_result(net::tcp_read(server, 64)));\n    net::tcp_close(client);\n    net::tcp_close(server);\n    net::tcp_close(listener);\n\n    // Verbindung abgelehnt (Port ohne Listener)\n    let l2 = ok_int(net::tcp_listen(\"127.0.0.1\", 0, 1));\n    let p2 = ok_int(net::local_port(l2));\n    net::tcp_close(l2);\n    let refused = net::tcp_connect(\"127.0.0.1\", p2);\n    println(match refused {\n        Result::Ok(_) => \"unerwartet\",\n        Result::Err(e) => e\n    });\n    return 0;\n}\n",
    );
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(stdout.starts_with("1\ntcp_read: timeout\n"), "{stdout}");
    assert!(stdout.contains("connection failed"), "{stdout}");
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn net_client_vs_rust_server_natively() {
    let dir = temp_dir("net-vs-rust");
    // A Rust-side echo server on an ephemeral port.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = std::thread::spawn(move || {
        use std::io::{Read, Write};
        let (mut stream, _) = listener.accept().expect("accept");
        let mut buf = [0u8; 1024];
        let n = stream.read(&mut buf).expect("read");
        stream.write_all(&buf[..n]).expect("write");
        stream.write_all(b"!").expect("write2");
    });
    let file = dir.join("c.blm");
    write_file(
        &file,
        &format!(
            "use std::net;\n\nfn ok_int(r: Result<int, string>) -> int {{\n    return match r {{\n        Result::Ok(v) => v,\n        Result::Err(e) => -1\n    }};\n}}\n\nfn opt_str(r: Result<Option<string>, string>) -> string {{\n    return match r {{\n        Result::Ok(o) => match o {{\n            Option::Some(s) => s,\n            Option::None => \"EOF\"\n        }},\n        Result::Err(e) => e\n    }};\n}}\n\nfn main() -> int {{\n    let sock = ok_int(net::tcp_connect(\"127.0.0.1\", {port}));\n    println(ok_int(net::tcp_write_all(sock, \"ping\")));\n    println(opt_str(net::tcp_read(sock, 64)));\n    println(opt_str(net::tcp_read(sock, 64)));\n    println(opt_str(net::tcp_read(sock, 64)));\n    return 0;\n}}\n"
        ),
    );
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    server.join().expect("server thread");
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    assert_eq!(norm(&out.stdout), "4\nping\n!\nEOF\n");
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn http_client_vs_rust_server_natively() {
    let dir = temp_dir("http-client");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Read, Write};
        let (mut stream, _) = listener.accept().expect("accept");
        let mut reader = BufReader::new(stream.try_clone().expect("clone"));
        let mut request = String::new();
        reader.read_line(&mut request).expect("request line");
        let mut content_length = 0usize;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).expect("header");
            let trimmed = line.trim_end().to_string();
            if trimmed.is_empty() {
                break;
            }
            if trimmed.to_ascii_lowercase().starts_with("content-length:") {
                content_length = trimmed[15..].trim().parse().unwrap_or(0);
            }
            request.push_str(&trimmed);
            request.push('\n');
        }
        let mut body = vec![0u8; content_length];
        if content_length > 0 {
            reader.read_exact(&mut body).expect("body");
        }
        let response = "HTTP/1.1 201 Created\r\nContent-Type: text/plain; charset=utf-8\r\nX-Test: yes\r\nContent-Length: 7\r\nConnection: close\r\n\r\nhallo!\n";
        stream.write_all(response.as_bytes()).expect("write");
        (request, String::from_utf8_lossy(&body).to_string())
    });
    let file = dir.join("c.blm");
    write_file(
        &file,
        &format!(
            "use std::http;\n\nfn show(r: http::HttpResponse) -> string {{\n    return str(r.status) + \"|\" + r.reason + \"|\" + r.body + \"|\" + match http::header_get(r.headers, \"x-test\") {{\n        Option::Some(v) => v,\n        Option::None => \"?\"\n    }};\n}}\n\nfn main() -> int {{\n    println(match http::get(\"http://127.0.0.1:{port}/greet\") {{\n        Result::Ok(r) => show(r),\n        Result::Err(e) => \"ERR: \" + e\n    }});\n    println(match http::post(\"http://127.0.0.1:{port}/submit\", \"text/plain\", \"payload\") {{\n        Result::Ok(r) => show(r),\n        Result::Err(e) => \"ERR: \" + e\n    }});\n    return 0;\n}}\n"
        ),
    );
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    let (request, body) = server.join().expect("server thread");
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(stdout.starts_with("201|Created|hallo!\n|yes\n"), "{stdout}");
    assert!(stdout.contains("201|Created|hallo!\n|yes"), "{stdout}");
    assert!(request.starts_with("GET /greet HTTP/1.1"), "{request}");
    assert!(
        request.contains(&format!("Host: 127.0.0.1:{port}")),
        "{request}"
    );
    assert!(body.is_empty());
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn http_server_vs_rust_client_natively() {
    let dir = temp_dir("http-server");
    let file = dir.join("s.blm");
    write_file(
        &file,
        "use std::net;\nuse std::http;\n\nfn ok_int(r: Result<int, string>) -> int {\n    return match r {\n        Result::Ok(v) => v,\n        Result::Err(e) => -1\n    };\n}\n\nfn ok_bool(r: Result<bool, string>) -> int {\n    return match r {\n        Result::Ok(v) => 1,\n        Result::Err(e) => -1\n    };\n}\n\nfn handle(conn: int) -> string {\n    return match http::read_request(conn) {\n        Result::Ok(req) => req.method + \" \" + req.target + \" host=\" + match http::header_get(req.headers, \"host\") {\n            Option::Some(v) => v,\n            Option::None => \"?\"\n        } + \" body=\" + req.body,\n        Result::Err(e) => \"ERR: \" + e\n    };\n}\n\nfn main() -> int {\n    let listener = ok_int(net::tcp_listen(\"127.0.0.1\", 0, 1));\n    println(ok_int(net::local_port(listener)));\n    let conn = ok_int(net::tcp_accept(listener));\n    println(handle(conn));\n    println(ok_bool(http::write_response(conn, 200, \"text/plain; charset=utf-8\", \"pong\")));\n    println(ok_bool(net::tcp_close(conn)));\n    println(ok_bool(net::tcp_close(listener)));\n    return 0;\n}\n",
    );
    let exe = dir.join(if cfg!(windows) {
        "server.exe"
    } else {
        "server"
    });
    let build = run_bloom(&[
        "build",
        file.to_string_lossy().as_ref(),
        "-o",
        exe.to_string_lossy().as_ref(),
    ]);
    if no_toolchain(&build) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(build.code, 0, "{}", build.combined());
    use std::io::{BufRead, BufReader, Read, Write};
    let mut child = std::process::Command::new(&exe)
        .env("BLOOM_MEM_REPORT", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn server");
    let stdout = child.stdout.take().expect("stdout");
    let mut reader = BufReader::new(stdout);
    let mut port_line = String::new();
    reader.read_line(&mut port_line).expect("port line");
    let port: u16 = port_line.trim().parse().expect("port number");
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream
        .write_all(b"POST /submit HTTP/1.1\r\nHost: test-host\r\nContent-Type: text/plain\r\nContent-Length: 5\r\nConnection: close\r\n\r\nworld")
        .expect("write request");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");
    let status = child.wait().expect("wait");
    let mut rest = String::new();
    reader.read_to_string(&mut rest).expect("rest stdout");
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("stderr")
        .read_to_string(&mut stderr)
        .expect("read stderr");
    assert_eq!(status.code(), Some(0), "{response}\n{rest}\n{stderr}");
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
    assert!(
        response.contains("Content-Type: text/plain; charset=utf-8\r\n"),
        "{response}"
    );
    assert!(response.contains("Content-Length: 4\r\n"), "{response}");
    assert!(response.ends_with("pong"), "{response}");
    assert!(
        norm(&rest).contains("POST /submit host=test-host body=world"),
        "{rest}"
    );
    assert!(stderr.contains("live allocations: 0"), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn http_error_paths_natively() {
    let dir = temp_dir("http-errors");
    let file = dir.join("e.blm");
    write_file(
        &file,
        "use std::net;\nuse std::http;\n\nfn ok_int(r: Result<int, string>) -> int {\n    return match r {\n        Result::Ok(v) => v,\n        Result::Err(e) => -1\n    };\n}\n\nfn send_and_read(req: string) -> string {\n    let listener = ok_int(net::tcp_listen(\"127.0.0.1\", 0, 1));\n    let port = ok_int(net::local_port(listener));\n    let client = ok_int(net::tcp_connect(\"127.0.0.1\", port));\n    let server = ok_int(net::tcp_accept(listener));\n    net::tcp_write_all(client, req);\n    let result = match http::read_request(server) {\n        Result::Ok(r) => \"OK \" + r.method + \" \" + r.body,\n        Result::Err(e) => \"ERR: \" + e\n    };\n    net::tcp_close(client);\n    net::tcp_close(server);\n    net::tcp_close(listener);\n    return result;\n}\n\nfn main() -> int {\n    println(match http::get(\"ftp://example.com/\") {\n        Result::Ok(_) => \"unerwartet\",\n        Result::Err(e) => e\n    });\n    println(send_and_read(\"GET / HTTP/1.1\\r\\n\\r\\n\"));\n    println(send_and_read(\"GET / HTTP/1.1\\r\\nHost: x\\r\\nTransfer-Encoding: chunked\\r\\n\\r\\n5\\r\\nhello\\r\\n0\\r\\n\\r\\n\"));\n    println(send_and_read(\"GET / HTTP/1.1\\r\\nHost: x\\r\\nTransfer-Encoding: chunked\\r\\nContent-Length: 5\\r\\n\\r\\n\"));\n    println(send_and_read(\"GET / HTTP/1.1\\r\\nHost: x\\r\\nContent-Length: 5\\r\\nContent-Length: 7\\r\\n\\r\\n\"));\n    println(send_and_read(\"GET / HTTP/1.1\\r\\nHost: x\\r\\nContent-Length: abc\\r\\n\\r\\n\"));\n    println(send_and_read(\"BROKEN\\r\\nHost: x\\r\\n\\r\\n\"));\n    println(send_and_read(\"GET / HTTP/2.0\\r\\nHost: x\\r\\n\\r\\n\"));\n    return 0;\n}\n",
    );
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(stdout.contains("URL must start with"), "{stdout}");
    assert!(stdout.contains("without Host header"), "{stdout}");
    assert!(stdout.contains("OK GET hello"), "{stdout}");
    assert!(stdout.contains("ambiguous message framing"), "{stdout}");
    assert!(stdout.contains("duplicate Content-Length"), "{stdout}");
    assert!(stdout.contains("malformed Content-Length"), "{stdout}");
    assert!(stdout.contains("malformed request line"), "{stdout}");
    assert!(stdout.contains("unsupported HTTP version"), "{stdout}");
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_ffi_and_win32_examples() {
    for name in ["ffi_basic.blm", "win32_info.blm"] {
        let out = run_bloom_env(&["run", &example(name)], &[("BLOOM_MEM_REPORT", "1")]);
        if no_toolchain(&out) {
            eprintln!("skipping: no C toolchain available");
            return;
        }
        assert_eq!(out.code, 0, "{}: {}", name, out.combined());
        assert!(
            norm(&out.stdout).contains("Prozess-ID"),
            "{name}: {}",
            out.stdout
        );
        assert!(
            out.stderr.contains("live allocations: 0"),
            "{}: {}",
            name,
            out.combined()
        );
    }
}

#[test]
fn run_tcp_client_example_against_rust_server() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = std::thread::spawn(move || {
        use std::io::{Read, Write};
        let (mut stream, _) = listener.accept().expect("accept");
        let mut buf = [0u8; 64];
        let n = stream.read(&mut buf).expect("read");
        stream.write_all(b"pong").expect("write");
        n
    });
    let out = run_bloom_with_stdin(&["run", &example("tcp_client.blm")], &format!("{port}\n"));
    let received = server.join().expect("server");
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(stdout.contains("verbunden"), "{stdout}");
    assert!(stdout.contains("\n5\n"), "{stdout}");
    assert!(stdout.contains("empfangen: pong"), "{stdout}");
    assert_eq!(received, 5);
}

#[test]
fn run_tcp_server_example_against_rust_client() {
    use std::io::{BufRead, BufReader, Read, Write};
    let dir = temp_dir("tcp-server-example");
    let exe = dir.join(if cfg!(windows) {
        "server.exe"
    } else {
        "server"
    });
    let build = run_bloom(&[
        "build",
        &example("tcp_server.blm"),
        "-o",
        exe.to_string_lossy().as_ref(),
    ]);
    if no_toolchain(&build) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(build.code, 0, "{}", build.combined());
    let mut child = std::process::Command::new(&exe)
        .env("BLOOM_MEM_REPORT", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn");
    let mut reader = BufReader::new(child.stdout.take().expect("stdout"));
    let mut port_line = String::new();
    reader.read_line(&mut port_line).expect("port");
    let port: u16 = port_line.trim().parse().expect("port number");
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream.write_all(b"ping\n").expect("write");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read");
    let status = child.wait().expect("wait");
    let mut rest = String::new();
    reader.read_to_string(&mut rest).expect("rest");
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("stderr")
        .read_to_string(&mut stderr)
        .expect("stderr read");
    assert_eq!(status.code(), Some(0), "{rest}\n{stderr}");
    assert_eq!(response, "echo: ping\n");
    assert!(norm(&rest).contains("empfangen: ping\n"), "{rest}");
    assert!(stderr.contains("live allocations: 0"), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_http_get_example_against_rust_server() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let (mut stream, _) = listener.accept().expect("accept");
        let mut reader = BufReader::new(stream.try_clone().expect("clone"));
        let mut line = String::new();
        reader.read_line(&mut line).expect("request line");
        loop {
            let mut h = String::new();
            reader.read_line(&mut h).expect("header");
            if h.trim_end().is_empty() {
                break;
            }
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 6\r\nConnection: close\r\n\r\nhallo!")
            .expect("write");
        line
    });
    let out = run_bloom_with_stdin(&["run", &example("http_get.blm")], &format!("{port}\n"));
    let request_line = server.join().expect("server");
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(stdout.contains("Status: 200 OK"), "{stdout}");
    assert!(stdout.contains("Content-Type: text/plain"), "{stdout}");
    assert!(stdout.contains("Body: hallo!"), "{stdout}");
    assert_eq!(request_line.trim_end(), "GET /demo HTTP/1.1");
}

#[test]
fn run_http_server_example_against_rust_client() {
    use std::io::{BufRead, BufReader, Read, Write};
    let dir = temp_dir("http-server-example");
    let exe = dir.join(if cfg!(windows) {
        "server.exe"
    } else {
        "server"
    });
    let build = run_bloom(&[
        "build",
        &example("http_server.blm"),
        "-o",
        exe.to_string_lossy().as_ref(),
    ]);
    if no_toolchain(&build) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(build.code, 0, "{}", build.combined());
    let mut child = std::process::Command::new(&exe)
        .env("BLOOM_MEM_REPORT", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn");
    let mut reader = BufReader::new(child.stdout.take().expect("stdout"));
    let mut port_line = String::new();
    reader.read_line(&mut port_line).expect("port");
    let port: u16 = port_line.trim().parse().expect("port number");
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: bloom\r\nConnection: close\r\n\r\n")
        .expect("write");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read");
    let status = child.wait().expect("wait");
    let mut rest = String::new();
    reader.read_to_string(&mut rest).expect("rest");
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("stderr")
        .read_to_string(&mut stderr)
        .expect("stderr read");
    assert_eq!(status.code(), Some(0), "{rest}\n{stderr}");
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
    assert!(response.contains("Content-Length: 16\r\n"), "{response}");
    assert!(response.ends_with("Hallo von Bloom!"), "{response}");
    assert!(norm(&rest).contains("Anfrage: GET / (bloom)"), "{rest}");
    assert!(stderr.contains("live allocations: 0"), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- TLS, HTTPS and chunked transfer encoding (0.11.0) --------------------

#[cfg(windows)]
fn tls_server_script() -> String {
    root()
        .join("tests")
        .join("tls_server.ps1")
        .to_string_lossy()
        .to_string()
}

#[cfg(windows)]
struct TlsServer {
    child: std::process::Child,
    port: u16,
    ca: PathBuf,
}

#[cfg(windows)]
impl Drop for TlsServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(windows)]
fn start_tls_server(dir: &std::path::Path, mode: &str, connections: u32) -> TlsServer {
    let port_file = dir.join("tls-port.txt");
    let ca_file = dir.join("tls-ca.der");
    let _ = std::fs::remove_file(&port_file);
    let _ = std::fs::remove_file(&ca_file);
    let child = Command::new("powershell")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(tls_server_script())
        .arg("-PortFile")
        .arg(&port_file)
        .arg("-DerFile")
        .arg(&ca_file)
        .arg("-Mode")
        .arg(mode)
        .arg("-Connections")
        .arg(connections.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn tls server");
    let mut port = None;
    for _ in 0..300 {
        if let Ok(text) = std::fs::read_to_string(&port_file) {
            if let Ok(p) = text.trim().parse::<u16>() {
                port = Some(p);
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    TlsServer {
        child,
        port: port.expect("tls server did not report a port"),
        ca: ca_file,
    }
}

#[cfg(windows)]
fn ca_arg(ca: &std::path::Path) -> String {
    ca.to_string_lossy().replace('\\', "\\\\")
}

fn run_bloom_stdin_env(args: &[&str], input: &str, envs: &[(&str, &str)]) -> CmdOutput {
    let mut child = Command::new(bloom_bin())
        .args(args)
        .envs(envs.iter().map(|(k, v)| (*k, *v)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn bloom");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait for bloom");
    CmdOutput {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    }
}

#[cfg(windows)]
#[test]
fn tls_client_handshake_echo_natively() {
    let dir = temp_dir("tls-echo");
    let server = start_tls_server(&dir, "echo", 1);
    let file = dir.join("t.blm");
    let src = r#"use std::tls;

fn show_int(r: Result<int, string>) -> string {
    return match r {
        Result::Ok(v) => "OK " + str(v),
        Result::Err(e) => "ERR: " + e
    };
}

fn show_read(r: Result<Option<string>, string>) -> string {
    return match r {
        Result::Ok(o) => match o {
            Option::Some(s) => "GOT: " + s,
            Option::None => "EOF"
        },
        Result::Err(e) => "ERR: " + e
    };
}

fn main() -> int {
    let h = tls::tls_connect_with_ca("localhost", @PORT@, "@CA@");
    println(show_int(h));
    let handle = match h {
        Result::Ok(v) => v,
        Result::Err(e) => 0
    };
    if handle == 0 {
        return 1;
    }
    println(show_int(tls::tls_write_all(handle, "hello")));
    println(show_read(tls::tls_read(handle, 4096)));
    println(match tls::tls_close(handle) {
        Result::Ok(b) => "closed",
        Result::Err(e) => "ERR: " + e
    });
    println(show_read(tls::tls_read(handle, 16)));
    return 0;
}
"#
    .replace("@PORT@", &server.port.to_string())
    .replace("@CA@", &ca_arg(&server.ca));
    write_file(&file, &src);
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(
        stdout.lines().next().unwrap_or("").starts_with("OK "),
        "{stdout}"
    );
    assert!(stdout.contains("OK 5"), "{stdout}");
    assert!(stdout.contains("GOT: hello"), "{stdout}");
    assert!(stdout.contains("closed"), "{stdout}");
    assert!(
        stdout.contains("unknown, closed or wrong-kind handle"),
        "{stdout}"
    );
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    assert!(out.stderr.contains("live sockets: 0"), "{}", out.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(windows)]
#[test]
fn tls_trust_errors_natively() {
    let dir = temp_dir("tls-trust");

    let server = start_tls_server(&dir, "echo", 1);
    let file = dir.join("sys.blm");
    let src = r#"use std::tls;

fn main() -> int {
    println(match tls::tls_connect("localhost", @PORT@) {
        Result::Ok(_) => "unerwartet",
        Result::Err(e) => "ERR: " + e
    });
    return 0;
}
"#
    .replace("@PORT@", &server.port.to_string());
    write_file(&file, &src);
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(stdout.contains("not trusted"), "{stdout}");
    assert!(out.stderr.contains("live sockets: 0"), "{}", out.combined());
    drop(server);

    let server = start_tls_server(&dir, "echo", 1);
    let file = dir.join("host.blm");
    let src = r#"use std::tls;

fn main() -> int {
    println(match tls::tls_connect_with_ca("127.0.0.1", @PORT@, "@CA@") {
        Result::Ok(_) => "unerwartet",
        Result::Err(e) => "ERR: " + e
    });
    return 0;
}
"#
    .replace("@PORT@", &server.port.to_string())
    .replace("@CA@", &ca_arg(&server.ca));
    write_file(&file, &src);
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(stdout.contains("host name mismatch"), "{stdout}");
    assert!(out.stderr.contains("live sockets: 0"), "{}", out.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(windows)]
#[test]
fn tls_handshake_timeout_and_plain_peer_natively() {
    use std::net::TcpListener;

    // Accepts TCP but never answers the TLS handshake.
    let hold = TcpListener::bind("127.0.0.1:0").expect("bind hold");
    let hold_port = hold.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        if let Ok((_stream, _)) = hold.accept() {
            std::thread::sleep(std::time::Duration::from_secs(5));
        }
    });

    // Sends plain text instead of TLS records.
    let plain = TcpListener::bind("127.0.0.1:0").expect("bind plain");
    let plain_port = plain.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = plain.accept() {
            let _ = stream.write_all(b"HELLO PLAIN TEXT\r\n");
            let _ = stream.flush();
            std::thread::sleep(std::time::Duration::from_secs(3));
        }
    });

    let dir = temp_dir("tls-timeout");
    let file = dir.join("t.blm");
    let src = r#"use std::tls;

fn err(r: Result<int, string>) -> string {
    return match r {
        Result::Ok(_) => "unerwartet",
        Result::Err(e) => e
    };
}

fn main() -> int {
    println(err(tls::tls_connect_ex("127.0.0.1", @HOLD@, 300, "")));
    println(err(tls::tls_connect_ex("127.0.0.1", @HOLD@, 0, "")));
    println(err(tls::tls_connect_ex("127.0.0.1", @HOLD@, 700000, "")));
    println(err(tls::tls_connect("127.0.0.1", @PLAIN@)));
    return 0;
}
"#
    .replace("@HOLD@", &hold_port.to_string())
    .replace("@PLAIN@", &plain_port.to_string());
    write_file(&file, &src);
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(
        stdout.contains("timeout during the TLS handshake"),
        "{stdout}"
    );
    assert!(
        stdout.contains("handshake timeout out of range"),
        "{stdout}"
    );
    assert!(stdout.contains("tls_connect"), "{stdout}");
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    assert!(out.stderr.contains("live sockets: 0"), "{}", out.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(windows)]
#[test]
fn tls_close_notify_vs_abort_natively() {
    let dir = temp_dir("tls-eof");

    // Clean shutdown: the peer sends close_notify, so the read ends with None.
    // .NET Framework's SslStream does not send close_notify on Close, so the
    // clean-close peer is a small Node/OpenSSL server (skipped when Node is
    // not installed).
    let node_ok = Command::new("node")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if node_ok {
        let pfx = dir.join("tls-cert.pfx");
        let ca = dir.join("tls-cert.der");
        let make = Command::new("powershell")
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(tls_server_script())
            .arg("-PortFile")
            .arg(dir.join("makecert-port.txt"))
            .arg("-DerFile")
            .arg(&ca)
            .arg("-Mode")
            .arg("cert-only")
            .arg("-PfxFile")
            .arg(&pfx)
            .status()
            .expect("run cert generation");
        assert!(make.success(), "cert generation failed");
        let node_script = dir.join("tls_node_server.js");
        write_file(
            &node_script,
            r#"const tls = require("tls");
const fs = require("fs");
const pfx = fs.readFileSync(process.argv[2]);
const portFile = process.argv[3];
const mode = process.argv[4];
const server = tls.createServer({ pfx, passphrase: "bloom-test" }, (socket) => {
  socket.once("data", (d) => {
    socket.write(d);
    if (mode === "close") {
      socket.end();
    } else {
      socket.destroy();
    }
  });
  socket.on("error", () => {});
});
server.listen(0, "127.0.0.1", () => {
  fs.writeFileSync(portFile, String(server.address().port));
});
"#,
        );
        let port_file = dir.join("node-port.txt");
        let _ = std::fs::remove_file(&port_file);
        let mut server = Command::new("node")
            .arg(&node_script)
            .arg(&pfx)
            .arg(&port_file)
            .arg("close")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn node tls server");
        let mut port = None;
        for _ in 0..200 {
            if let Ok(text) = std::fs::read_to_string(&port_file) {
                if let Ok(p) = text.trim().parse::<u16>() {
                    port = Some(p);
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let port = port.expect("node tls server did not report a port");

        let file = dir.join("clean.blm");
        let src = r#"use std::tls;

fn show_int(r: Result<int, string>) -> string {
    return match r {
        Result::Ok(v) => "OK " + str(v),
        Result::Err(e) => "ERR: " + e
    };
}

fn show_read(r: Result<Option<string>, string>) -> string {
    return match r {
        Result::Ok(o) => match o {
            Option::Some(s) => "GOT: " + s,
            Option::None => "EOF"
        },
        Result::Err(e) => "ERR: " + e
    };
}

fn main() -> int {
    let h = tls::tls_connect_with_ca("localhost", @PORT@, "@CA@");
    println(show_int(h));
    let handle = match h {
        Result::Ok(v) => v,
        Result::Err(e) => 0
    };
    if handle == 0 {
        return 1;
    }
    println(show_int(tls::tls_write_all(handle, "bye")));
    println(show_read(tls::tls_read(handle, 4096)));
    println(show_read(tls::tls_read(handle, 4096)));
    tls::tls_close(handle);
    return 0;
}
"#
        .replace("@PORT@", &port.to_string())
        .replace("@CA@", &ca_arg(&ca));
        write_file(&file, &src);
        let out = run_bloom_env(
            &["run", file.to_string_lossy().as_ref()],
            &[("BLOOM_MEM_REPORT", "1")],
        );
        let _ = server.kill();
        let _ = server.wait();
        if no_toolchain(&out) {
            eprintln!("skipping: no C toolchain available");
            return;
        }
        assert_eq!(out.code, 0, "{}", out.combined());
        let stdout = norm(&out.stdout);
        assert!(stdout.contains("OK 3"), "{stdout}");
        assert!(stdout.contains("GOT: bye"), "{stdout}");
        assert!(stdout.contains("EOF"), "{stdout}");
        assert!(out.stderr.contains("live sockets: 0"), "{}", out.combined());
    } else {
        eprintln!("skipping clean-close case: node is not installed");
    }

    // Abrupt shutdown: no close_notify, so the read reports an error.
    let server = start_tls_server(&dir, "echo-then-abort", 1);
    let src = r#"use std::tls;

fn show_int(r: Result<int, string>) -> string {
    return match r {
        Result::Ok(v) => "OK " + str(v),
        Result::Err(e) => "ERR: " + e
    };
}

fn show_read(r: Result<Option<string>, string>) -> string {
    return match r {
        Result::Ok(o) => match o {
            Option::Some(s) => "GOT: " + s,
            Option::None => "EOF"
        },
        Result::Err(e) => "ERR: " + e
    };
}

fn main() -> int {
    let h = tls::tls_connect_with_ca("localhost", @PORT@, "@CA@");
    println(show_int(h));
    let handle = match h {
        Result::Ok(v) => v,
        Result::Err(e) => 0
    };
    if handle == 0 {
        return 1;
    }
    println(show_int(tls::tls_write_all(handle, "bye")));
    println(show_read(tls::tls_read(handle, 4096)));
    println(show_read(tls::tls_read(handle, 4096)));
    tls::tls_close(handle);
    return 0;
}
"#
    .replace("@PORT@", &server.port.to_string())
    .replace("@CA@", &ca_arg(&server.ca));
    let file = dir.join("abort.blm");
    write_file(&file, &src);
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(stdout.contains("GOT: bye"), "{stdout}");
    assert!(stdout.contains("close_notify"), "{stdout}");
    assert!(out.stderr.contains("live sockets: 0"), "{}", out.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(windows)]
#[test]
fn https_get_with_ca_natively() {
    let dir = temp_dir("https-get");
    let server = start_tls_server(&dir, "http", 2);
    let file = dir.join("h.blm");
    let src = r#"use std::http;

fn main() -> int {
    println(match http::get_with_ca("https://localhost:@PORT@/", "@CA@") {
        Result::Ok(r) => "OK " + str(r.status) + " " + r.reason + " " + r.body,
        Result::Err(e) => "ERR: " + e
    });
    println(match http::get("https://localhost:@PORT@/") {
        Result::Ok(_) => "unerwartet",
        Result::Err(e) => "ERR: " + e
    });
    return 0;
}
"#
    .replace("@PORT@", &server.port.to_string())
    .replace("@CA@", &ca_arg(&server.ca));
    write_file(&file, &src);
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(stdout.contains("OK 200 OK hallo"), "{stdout}");
    assert!(stdout.contains("not trusted"), "{stdout}");
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    assert!(out.stderr.contains("live sockets: 0"), "{}", out.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(windows)]
#[test]
fn https_chunked_response_natively() {
    let dir = temp_dir("https-chunked");
    let server = start_tls_server(&dir, "http-chunked", 1);
    let file = dir.join("h.blm");
    let src = r#"use std::http;

fn main() -> int {
    println(match http::get_with_ca("https://localhost:@PORT@/", "@CA@") {
        Result::Ok(r) => "OK " + str(r.status) + " [" + r.body + "] " + match http::header_get(r.headers, "transfer-encoding") {
            Option::Some(v) => v,
            Option::None => "(fehlt)"
        },
        Result::Err(e) => "ERR: " + e
    });
    return 0;
}
"#
    .replace("@PORT@", &server.port.to_string())
    .replace("@CA@", &ca_arg(&server.ca));
    write_file(&file, &src);
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(stdout.contains("OK 200 [first second] chunked"), "{stdout}");
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    assert!(out.stderr.contains("live sockets: 0"), "{}", out.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(windows)]
#[test]
fn https_example_runs_natively() {
    let dir = temp_dir("https-example");
    let server = start_tls_server(&dir, "http", 1);
    let input = format!(
        "localhost:{}\n{}\n",
        server.port,
        server.ca.to_string_lossy()
    );
    let out = run_bloom_stdin_env(
        &["run", &example("https_client.blm")],
        &input,
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(stdout.contains("Status: 200 OK"), "{stdout}");
    assert!(stdout.contains("hallo"), "{stdout}");
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    assert!(out.stderr.contains("live sockets: 0"), "{}", out.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

fn spawn_scripted_server(response: &'static [u8]) -> u16 {
    use std::io::Read;
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind scripted server");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 8192];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(response);
            let _ = stream.flush();
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });
    port
}

#[test]
fn http_chunked_client_natively() {
    let p1 = spawn_scripted_server(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\nX-Trailer: yes\r\n\r\n",
    );
    let p2 = spawn_scripted_server(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n");
    let p3 = spawn_scripted_server(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhelloX\r\n0\r\n\r\n",
    );
    let p4 = spawn_scripted_server(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Length: 5\r\n\r\n",
    );
    let p5 = spawn_scripted_server(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip\r\n\r\n");
    let p6 = spawn_scripted_server(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nFFFFFFFFFF\r\n",
    );
    let p7 = spawn_scripted_server(b"HTTP/1.1 204 No Content\r\nContent-Length: 5\r\n\r\n");
    let p8 = spawn_scripted_server(
        b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
    );

    let dir = temp_dir("http-chunked-client");
    let file = dir.join("c.blm");
    let src = r#"use std::http;

fn show(r: Result<http::HttpResponse, string>) -> string {
    return match r {
        Result::Ok(resp) => "OK " + str(resp.status) + " [" + resp.body + "]",
        Result::Err(e) => "ERR: " + e
    };
}

fn main() -> int {
    println(show(http::get("http://127.0.0.1:@P1@/")));
    println(show(http::get("http://127.0.0.1:@P2@/")));
    println(show(http::get("http://127.0.0.1:@P3@/")));
    println(show(http::get("http://127.0.0.1:@P4@/")));
    println(show(http::get("http://127.0.0.1:@P5@/")));
    println(show(http::get("http://127.0.0.1:@P6@/")));
    println(show(http::get("http://127.0.0.1:@P7@/")));
    println(show(http::get("http://127.0.0.1:@P8@/")));
    return 0;
}
"#
    .replace("@P1@", &p1.to_string())
    .replace("@P2@", &p2.to_string())
    .replace("@P3@", &p3.to_string())
    .replace("@P4@", &p4.to_string())
    .replace("@P5@", &p5.to_string())
    .replace("@P6@", &p6.to_string())
    .replace("@P7@", &p7.to_string())
    .replace("@P8@", &p8.to_string());
    write_file(&file, &src);
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(stdout.contains("OK 200 [hello world]"), "{stdout}");
    assert!(
        stdout.contains("invalid hexadecimal chunk size"),
        "{stdout}"
    );
    assert!(stdout.contains("missing CRLF after chunk data"), "{stdout}");
    assert!(stdout.contains("ambiguous message framing"), "{stdout}");
    assert!(
        stdout.contains("unsupported or ambiguous Transfer-Encoding"),
        "{stdout}"
    );
    assert!(
        stdout.contains("chunk exceeds the 4194304 byte body limit"),
        "{stdout}"
    );
    assert!(stdout.contains("OK 204 []"), "{stdout}");
    assert!(stdout.contains("OK 200 [ok]"), "{stdout}");
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    assert!(out.stderr.contains("live sockets: 0"), "{}", out.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn http_chunked_request_details_natively() {
    let dir = temp_dir("http-chunked-server");
    let file = dir.join("s.blm");
    let src = r#"use std::net;
use std::http;

fn ok_int(r: Result<int, string>) -> int {
    return match r {
        Result::Ok(v) => v,
        Result::Err(e) => -1
    };
}

fn send_and_read(req: string) -> string {
    let listener = ok_int(net::tcp_listen("127.0.0.1", 0, 1));
    let port = ok_int(net::local_port(listener));
    let client = ok_int(net::tcp_connect("127.0.0.1", port));
    let server = ok_int(net::tcp_accept(listener));
    net::tcp_write_all(client, req);
    let result = match http::read_request(server) {
        Result::Ok(r) => "OK " + r.method + " " + r.body,
        Result::Err(e) => "ERR: " + e
    };
    net::tcp_close(client);
    net::tcp_close(server);
    net::tcp_close(listener);
    return result;
}

fn main() -> int {
    println(send_and_read("GET / HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n5;ext=1\r\nhello\r\n0\r\n\r\n"));
    println(send_and_read("GET / HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\nX-A: 1\r\nX-B: 2\r\n\r\n"));
    println(send_and_read("GET / HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n"));
    println(send_and_read("GET / HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhelloX\r\n0\r\n\r\n"));
    println(send_and_read("GET / HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\nFFFFFFFFFF\r\n"));
    return 0;
}
"#;
    write_file(&file, src);
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert_eq!(stdout.matches("OK GET hello").count(), 2, "{stdout}");
    assert!(
        stdout.contains("invalid hexadecimal chunk size"),
        "{stdout}"
    );
    assert!(stdout.contains("missing CRLF after chunk data"), "{stdout}");
    assert!(
        stdout.contains("chunk exceeds the 4194304 byte body limit"),
        "{stdout}"
    );
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    assert!(out.stderr.contains("live sockets: 0"), "{}", out.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- std::html escaping and templates (0.14.0) -----------------------------

#[test]
fn html_escape_natively() {
    let dir = temp_dir("html-escape");
    let file = dir.join("h.blm");
    let src = r#"use std::html;

fn main() -> int {
    println(html::escape("<a href=\"x\">Tom & Jerry's</a>"));
    println("[" + html::escape("") + "]");
    println(html::escape("a<b>c\"d'e&f"));
    println(html::escape("Grüße — 日本語 & 😀 <ok>"));
    println(html::escape("&&<<>>\"\"''"));
    return 0;
}
"#;
    write_file(&file, src);
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    let expected = [
        "&lt;a href=&quot;x&quot;&gt;Tom &amp; Jerry&#39;s&lt;/a&gt;",
        "[]",
        "a&lt;b&gt;c&quot;d&#39;e&amp;f",
        "Grüße — 日本語 &amp; 😀 &lt;ok&gt;",
        "&amp;&amp;&lt;&lt;&gt;&gt;&quot;&quot;&#39;&#39;",
    ];
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines, expected, "{stdout}");
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    assert!(out.stderr.contains("live sockets: 0"), "{}", out.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn html_template_basic_natively() {
    let dir = temp_dir("html-template-basic");
    let file = dir.join("h.blm");
    let src = r#"use std::html;
use std::json;

fn show(r: Result<string, string>) -> string {
    return match r {
        Result::Ok(v) => "OK " + v,
        Result::Err(e) => "ERR: " + e
    };
}

fn float_or_null(f: float) -> json::JsonValue {
    return match json::make_float(f) {
        Result::Ok(v) => v,
        Result::Err(e) => json::null()
    };
}

fn build() -> json::JsonValue {
    let d = json::make_object();
    json::set(d, "s", json::make_string("<b>"));
    json::set(d, "i", json::make_int(-7));
    json::set(d, "f", float_or_null(2.5));
    json::set(d, "b", json::make_bool(true));
    json::set(d, "n", json::null());
    let o = json::make_object();
    json::set(o, "inner", json::make_string("deep & <x>"));
    json::set(d, "o", o);
    let a = json::make_array();
    json::push(a, json::make_int(1));
    json::set(d, "a", a);
    return d;
}

fn print_renders(t: html::Template) -> void {
    println(show(html::render(t, build())));
    println(show(html::render(t, build())));
    println(show(html::render(t, json::make_object())));
}

fn main() -> int {
    match html::template("[{{s}}|{{i}}|{{f}}|{{b}}|{{o.inner}}]") {
        Result::Ok(t) => print_renders(t),
        Result::Err(e) => println("PARSE: " + e)
    };
    match html::template("{{n}}") {
        Result::Ok(t) => println(show(html::render(t, build()))),
        Result::Err(e) => println("PARSE: " + e)
    };
    match html::template("{{a}}") {
        Result::Ok(t) => println(show(html::render(t, build()))),
        Result::Err(e) => println("PARSE: " + e)
    };
    match html::template("{{o}}") {
        Result::Ok(t) => println(show(html::render(t, build()))),
        Result::Err(e) => println("PARSE: " + e)
    };
    match html::template("{{o.inner}}") {
        Result::Ok(t) => println(show(html::render(t, build()))),
        Result::Err(e) => println("PARSE: " + e)
    };
    return 0;
}
"#;
    write_file(&file, src);
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    let expected = [
        "OK [&lt;b&gt;|-7|2.5|true|deep &amp; &lt;x&gt;]",
        "OK [&lt;b&gt;|-7|2.5|true|deep &amp; &lt;x&gt;]",
        "ERR: render: missing value for `s`",
        "ERR: render: value for `n` is null",
        "ERR: render: `a` is an array; only strings, numbers and booleans can be inserted",
        "ERR: render: `o` is an object; only strings, numbers and booleans can be inserted",
        "OK deep &amp; &lt;x&gt;",
    ];
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines, expected, "{stdout}");
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    assert!(out.stderr.contains("live sockets: 0"), "{}", out.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn html_template_parse_errors_natively() {
    let dir = temp_dir("html-template-errors");
    let file = dir.join("h.blm");
    let src = r#"use std::html;

fn err_of(src: string) -> string {
    return match html::template(src) {
        Result::Ok(t) => "unerwartet",
        Result::Err(e) => e
    };
}

fn main() -> int {
    println(err_of("{{name"));
    println(err_of("{{}}"));
    println(err_of("{{#foo x}}{{/foo}}"));
    println(err_of("{{#if}}"));
    println(err_of("{{#if b}}x"));
    println(err_of("{{/if}}"));
    println(err_of("{{else}}"));
    println(err_of("{{#each a}}{{else}}{{/each}}"));
    println(err_of("{{#each a}}{{/if}}"));
    println(err_of("{{a b}}"));
    println(err_of("{{a..b}}"));
    println(err_of("{{1abc}}"));
    println(err_of("text {{#if a}}{{#if b}}{{#if c}}{{#if d}}{{#if e}}{{#if f}}{{#if g}}{{#if h}}{{#if i}}{{#if j}}{{#if k}}{{#if l}}{{#if m}}{{#if n}}{{#if o}}{{#if p}}{{#if q}}{{#if r}}{{#if s}}{{#if t}}{{#if u}}{{#if v}}{{#if w}}{{#if x}}{{#if y}}{{#if z}}{{#if aa}}{{#if ab}}{{#if ac}}{{#if ad}}{{#if ae}}{{#if af}}{{#if ag}}{{#if ah}}x{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}"));
    println(err_of("<script>alert(1)</script>"));
    println(err_of("<STYLE>body{}</STYLE>"));
    return 0;
}
"#;
    write_file(&file, src);
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    let expected = [
        "unterminated placeholder (missing `}}`) at byte 1 (line 1, column 1)",
        "empty placeholder at byte 1 (line 1, column 1)",
        "unknown directive (only `#if` and `#each` are supported) at byte 1 (line 1, column 1)",
        "directive requires a path (for example `{{#if enabled}}`) at byte 1 (line 1, column 1)",
        "unclosed block (missing `{{/if}}` or `{{/each}}`) at byte 1 (line 1, column 1)",
        "closing tag does not match the open block at byte 1 (line 1, column 1)",
        "`{{else}}` is only allowed once inside `{{#if}}` at byte 1 (line 1, column 1)",
        "`{{else}}` is only allowed once inside `{{#if}}` at byte 12 (line 1, column 12)",
        "closing tag does not match the open block at byte 12 (line 1, column 12)",
        "invalid placeholder (expected `name` or `a.b.c`) at byte 1 (line 1, column 1)",
        "invalid placeholder (expected `name` or `a.b.c`) at byte 1 (line 1, column 1)",
        "invalid placeholder (expected `name` or `a.b.c`) at byte 1 (line 1, column 1)",
        "template nests too deeply (max 32 levels) at byte 300 (line 1, column 300)",
        "`<script>` and `<style>` blocks are not supported in templates at byte 1 (line 1, column 1)",
        "`<script>` and `<style>` blocks are not supported in templates at byte 1 (line 1, column 1)",
    ];
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines, expected, "{stdout}");
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn html_template_control_natively() {
    let dir = temp_dir("html-template-control");
    let file = dir.join("h.blm");
    let src = r#"use std::html;
use std::json;

fn show(r: Result<string, string>) -> string {
    return match r {
        Result::Ok(v) => "OK " + v,
        Result::Err(e) => "ERR: " + e
    };
}

fn data() -> json::JsonValue {
    let d = json::make_object();
    json::set(d, "on", json::make_bool(true));
    json::set(d, "off", json::make_bool(false));
    json::set(d, "num", json::make_int(3));
    let items = json::make_array();
    let a = json::make_object();
    json::set(a, "name", json::make_string("A & B"));
    json::push(items, a);
    let b = json::make_object();
    json::set(b, "name", json::make_string("plain"));
    json::push(items, b);
    json::set(d, "items", items);
    let words = json::make_array();
    json::push(words, json::make_string("a&b"));
    json::push(words, json::make_string("plain"));
    json::set(d, "words", words);
    let empty = json::make_array();
    json::set(d, "empty", empty);
    let outer = json::make_array();
    let row = json::make_array();
    json::push(row, json::make_int(1));
    json::push(row, json::make_int(2));
    json::push(outer, row);
    json::set(d, "outer", outer);
    return d;
}

fn run(src: string, label: string) -> void {
    match html::template(src) {
        Result::Ok(t) => println(label + " " + show(html::render(t, data()))),
        Result::Err(e) => println(label + " PARSE: " + e)
    };
}

fn main() -> int {
    run("{{#if on}}ja{{else}}nein{{/if}}", "t1:");
    run("{{#if off}}ja{{else}}nein{{/if}}", "t2:");
    run("{{#if on}}{{#if off}}x{{else}}y{{/if}}{{/if}}", "t3:");
    run("{{#each words}}[{{this}}]{{/each}}", "t4:");
    run("{{#each items}}<{{this.name}}>{{/each}}", "t5:");
    run("{{#each empty}}x{{/each}}leer", "t6:");
    run("{{#each outer}}{{#each this}}({{this}}){{/each}}{{/each}}", "t7:");
    run("{{#if num}}x{{/if}}", "t8:");
    run("{{#each num}}x{{/each}}", "t9:");
    run("{{this}}", "t10:");
    return 0;
}
"#;
    write_file(&file, src);
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    let expected = [
        "t1: OK ja",
        "t2: OK nein",
        "t3: OK y",
        "t4: OK [a&amp;b][plain]",
        "t5: OK <A &amp; B><plain>",
        "t6: OK leer",
        "t7: OK (1)(2)",
        "t8: ERR: render: `num` is not a boolean (the condition must be true or false)",
        "t9: ERR: render: `num` is not an array (only arrays can be iterated)",
        "t10: ERR: render: `this` is only available inside `#each`",
    ];
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines, expected, "{stdout}");
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    assert!(out.stderr.contains("live sockets: 0"), "{}", out.combined());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn html_template_limits_natively() {
    let dir = temp_dir("html-template-limits");
    let file = dir.join("h.blm");
    let src = r#"use std::html;
use std::json;

fn show(r: Result<string, string>) -> string {
    return match r {
        Result::Ok(v) => "OK",
        Result::Err(e) => "ERR: " + e
    };
}

fn main() -> int {
    // Iterationslimit: 300x300 = 90000 > 65536
    let outer = json::make_array();
    var i = 0;
    while i < 300 {
        let row = json::make_array();
        var j = 0;
        while j < 300 {
            json::push(row, json::make_int(1));
            j = j + 1;
        }
        json::push(outer, row);
        i = i + 1;
    }
    let data = json::make_object();
    json::set(data, "outer", outer);
    match html::template("{{#each outer}}{{#each this}}x{{/each}}{{/each}}") {
        Result::Ok(t) => println(show(html::render(t, data))),
        Result::Err(e) => println("PARSE: " + e)
    };

    // Ausgabelimit: 6000 Elemente mit je 200 Zeichen = 1.2 MB > 1 MiB
    let items = json::make_array();
    var k = 0;
    while k < 6000 {
        json::push(items, json::make_string("xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"));
        k = k + 1;
    }
    let data2 = json::make_object();
    json::set(data2, "items", items);
    match html::template("{{#each items}}{{this}}{{/each}}") {
        Result::Ok(t) => println(show(html::render(t, data2))),
        Result::Err(e) => println("PARSE: " + e)
    };
    return 0;
}
"#;
    write_file(&file, src);
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(
        stdout.contains("ERR: render: too many loop iterations (max 65536)"),
        "{stdout}"
    );
    assert!(
        stdout.contains("ERR: render: rendered output exceeds 1048576 bytes"),
        "{stdout}"
    );
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn html_render_page_server_natively() {
    use std::io::{BufRead, BufReader, Read, Write};

    let dir = temp_dir("html-server");
    let exe = dir.join(if cfg!(windows) {
        "html_server.exe"
    } else {
        "html_server"
    });
    let build = run_bloom(&[
        "build",
        &example("web_html_page.blm"),
        "-o",
        exe.to_string_lossy().as_ref(),
    ]);
    if no_toolchain(&build) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(build.code, 0, "{}", build.combined());
    let mut child = std::process::Command::new(&exe)
        .env("BLOOM_MEM_REPORT", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn server");
    let mut reader = BufReader::new(child.stdout.take().expect("stdout"));
    let mut port_line = String::new();
    reader.read_line(&mut port_line).expect("port");
    let port: u16 = port_line.trim().parse().expect("port number");

    let request = |method: &str, target: &str| -> String {
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
        let head =
            format!("{method} {target} HTTP/1.1\r\nHost: bloom\r\nConnection: close\r\n\r\n");
        stream.write_all(head.as_bytes()).expect("write");
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .unwrap_or_else(|e| panic!("read failed for {method} {target}: {e}"));
        response
    };

    let r1 = request("GET", "/");
    assert!(r1.starts_with("HTTP/1.1 200 OK\r\n"), "{r1}");
    assert!(
        r1.contains("Content-Type: text/html; charset=utf-8\r\n"),
        "{r1}"
    );
    assert!(r1.contains("Cache-Control: no-store\r\n"), "{r1}");
    assert!(r1.contains("<title>Startseite</title>"), "{r1}");
    assert!(r1.contains("Ada &lt;admin&gt; (Admin)"), "{r1}");
    assert!(!r1.contains("Ada <admin>"), "{r1}");

    let r2 = request("GET", "/users");
    assert!(r2.starts_with("HTTP/1.1 200 OK\r\n"), "{r2}");
    assert!(r2.contains("<title>Benutzer</title>"), "{r2}");

    let r3 = request("POST", "/users");
    assert!(r3.starts_with("HTTP/1.1 405 "), "{r3}");
    assert!(r3.contains("Allow: GET\r\n"), "{r3}");

    let r4 = request("GET", "/nope");
    assert!(r4.starts_with("HTTP/1.1 404 Not Found\r\n"), "{r4}");
    assert!(r4.ends_with("not found"), "{r4}");

    // Six more requests so the server reaches its ten-connection limit.
    for _ in 0..6 {
        let r = request("GET", "/");
        assert!(r.starts_with("HTTP/1.1 200 OK\r\n"), "{r}");
    }

    let status = child.wait().expect("wait");
    let mut rest = String::new();
    reader.read_to_string(&mut rest).expect("rest");
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("stderr")
        .read_to_string(&mut stderr)
        .expect("stderr read");
    assert_eq!(status.code(), Some(0), "{rest}\n{stderr}");
    assert!(stderr.contains("live allocations: 0"), "{stderr}");
    assert!(stderr.contains("live sockets: 0"), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_html_examples() {
    let cases: &[(&str, &[&str])] = &[
        (
            "html_escape.blm",
            &[
                "roh:     <a href=\"x\">Tom & Jerry's</a>",
                "escaped: &lt;a href=&quot;x&quot;&gt;Tom &amp; Jerry&#39;s&lt;/a&gt;",
                "leer:    []",
                "Grüße — 日本語 &amp; 😀 &lt;ok&gt;",
            ],
        ),
        (
            "template_basic.blm",
            &[
                "<h1>Bloom &amp; Du</h1>",
                "&lt;Ada&gt; hat 3 neue Nachrichten",
                "pi ~ 3.14, bereit: true",
            ],
        ),
        (
            "template_conditions.blm",
            &["<p>Ada: aktiv</p><p>Beta</p>", "<p>Ada: inaktiv</p>"],
        ),
        (
            "template_lists.blm",
            &[
                "<li>Ada &lt;admin&gt; (Admin)</li>",
                "<li>Grace (Dev)</li>",
                "leer: fertig",
            ],
        ),
        (
            "template_errors.blm",
            &[
                "PARSE-FEHLER: unterminated placeholder",
                "PARSE-FEHLER: `<script>` and `<style>` blocks are not supported",
                "RENDER-FEHLER: render: missing value for `missing`",
                "RENDER-FEHLER: render: value for `none` is null",
            ],
        ),
    ];
    for (name, needles) in cases {
        let out = run_bloom_env(&["run", &example(name)], &[("BLOOM_MEM_REPORT", "1")]);
        if no_toolchain(&out) {
            eprintln!("skipping: no C toolchain available");
            return;
        }
        assert_eq!(out.code, 0, "{name}: {}", out.combined());
        let stdout = norm(&out.stdout);
        for needle in *needles {
            assert!(stdout.contains(needle), "{name}: {stdout}");
        }
        assert!(
            out.stderr.contains("live allocations: 0"),
            "{name}: {}",
            out.combined()
        );
        assert!(
            out.stderr.contains("live sockets: 0"),
            "{name}: {}",
            out.combined()
        );
    }
}

#[test]
fn fs_read_bytes_and_limited_natively() {
    let dir = temp_dir("fs-bytes");
    let blob = dir.join("blob.bin");
    let empty = dir.join("empty.bin");
    let missing = dir.join("missing.bin");
    let invalid = dir.join("invalid.bin");
    let big = dir.join("big.txt");
    let ok = dir.join("ok.txt");
    write_bytes(&blob, &[0u8, 1, 255, 0, 65, 10, 0]);
    write_bytes(&empty, &[]);
    write_bytes(&invalid, &[0xFFu8, 0xFE, 0x41]);
    write_bytes(&big, &vec![b'x'; 100]);
    write_file(&ok, "abcdef");
    let src = r#"use std::fs;

fn main() -> int {
    match fs::read_bytes("@BLOB@") {
        Result::Ok(b) => println(fs::bytes_len(b)),
        Result::Err(e) => println("ERR: " + e)
    };
    match fs::read_bytes("@EMPTY@") {
        Result::Ok(b) => println("EMPTY:" + str(fs::bytes_len(b))),
        Result::Err(e) => println("ERR: " + e)
    };
    match fs::read_bytes("@MISSING@") {
        Result::Ok(b) => println("UNEXPECTED"),
        Result::Err(e) => println("ERR: " + e)
    };
    match fs::read_bytes("@DIR@") {
        Result::Ok(b) => println("DIR-READ"),
        Result::Err(e) => println("ERR: " + e)
    };
    match fs::read_text_limited("@OK@", 6) {
        Result::Ok(s) => println(s),
        Result::Err(e) => println("ERR: " + e)
    };
    match fs::read_text_limited("@OK@", 5) {
        Result::Ok(s) => println("TOO-BIG-ALLOWED"),
        Result::Err(e) => println("ERR: " + e)
    };
    match fs::read_text_limited("@INVALID@", 100) {
        Result::Ok(s) => println("BAD-UTF8-ALLOWED"),
        Result::Err(e) => println("ERR: " + e)
    };
    match fs::read_text_limited("@BIG@", 0) {
        Result::Ok(s) => println("ZERO-ALLOWED"),
        Result::Err(e) => println("ERR: " + e)
    };
    match fs::read_text_limited("@OK@", 1024) {
        Result::Ok(s) => println("[" + s + "]"),
        Result::Err(e) => println("ERR: " + e)
    };
    return 0;
}
"#.replace("@BLOB@", &blob.to_string_lossy().replace('\\', "\\\\"))
                .replace("@EMPTY@", &empty.to_string_lossy().replace('\\', "\\\\"))
                .replace("@MISSING@", &missing.to_string_lossy().replace('\\', "\\\\"))
                .replace("@DIR@", &dir.to_string_lossy().replace('\\', "\\\\"))
                .replace("@INVALID@", &invalid.to_string_lossy().replace('\\', "\\\\"))
                .replace("@BIG@", &big.to_string_lossy().replace('\\', "\\\\"))
                .replace("@OK@", &ok.to_string_lossy().replace('\\', "\\\\"));
    let file = dir.join("main.blm");
    write_file(&file, &src);
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(stdout.starts_with("7\nEMPTY:0\n"), "{stdout}");
    assert!(stdout.contains("ERR: read_bytes: "), "{stdout}");
    assert!(stdout.contains("missing.bin"), "{stdout}");
    assert!(!stdout.contains("UNEXPECTED"), "{stdout}");
    assert!(!stdout.contains("DIR-READ"), "{stdout}");
    assert!(stdout.contains("abcdef\n"), "{stdout}");
    assert!(stdout.contains("file exceeds 5 bytes"), "{stdout}");
    assert!(!stdout.contains("TOO-BIG-ALLOWED"), "{stdout}");
    assert!(stdout.contains("file is not valid UTF-8"), "{stdout}");
    assert!(!stdout.contains("BAD-UTF8-ALLOWED"), "{stdout}");
    assert!(!stdout.contains("ZERO-ALLOWED"), "{stdout}");
    assert!(stdout.contains("[abcdef]\n"), "{stdout}");
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn html_template_file_natively() {
    let dir = temp_dir("html-template-file");
    let root = dir.join("site");
    write_file(&root.join("page.html"), "<h1>{{title}}</h1>");
    write_file(&root.join("sub/nested.html"), "<p>{{name}}</p>");
    write_file(&root.join("empty.html"), "");
    write_file(&root.join("syntax.html"), "{{#if}}");
    write_file(&root.join("outside.txt"), "SECRET");
    write_file(&root.join("big.html"), &"x".repeat(70000));
    write_bytes(&root.join("invalid.html"), &[0xFFu8, 0xFE]);
    let src = r#"use std::html;
use std::json;

fn render_file(path: string, key: string, value: string) -> void {
    match html::template_file(path) {
        Result::Ok(t) => render_ok(t, key, value),
        Result::Err(e) => println("ERR: " + e)
    };
}

fn render_ok(t: html::Template, key: string, value: string) -> void {
    let data = json::make_object();
    json::set(data, key, json::make_string(value));
    match html::render(t, data) {
        Result::Ok(s) => println("OUT:" + s),
        Result::Err(e) => println("RENDER-ERR: " + e)
    };
}

fn render_from(root: string, rel: string, key: string, value: string) -> void {
    match html::template_file_from(root, rel) {
        Result::Ok(t) => render_ok(t, key, value),
        Result::Err(e) => println("FROM-ERR: " + e)
    };
}

fn main() -> int {
    render_file("@PAGE@", "title", "Willkommen");
    render_file("@PAGE@", "title", "Nochmal");
    render_file("@EMPTY@", "title", "x");
    render_file("@SYNTAX@", "title", "x");
    render_file("@BIG@", "title", "x");
    render_file("@INVALID@", "title", "x");
    render_file("@MISSING@", "title", "x");
    render_from("@ROOT@", "page.html", "title", "Ada");
    render_from("@ROOT@", "sub/nested.html", "name", "Ada");
    render_from("@ROOT@", "", "name", "Ada");
    render_from("@ROOT@", "../outside.txt", "name", "Ada");
    return 0;
}
"#.replace("@PAGE@", &root.join("page.html").to_string_lossy().replace('\\', "\\\\"))
                .replace("@EMPTY@", &root.join("empty.html").to_string_lossy().replace('\\', "\\\\"))
                .replace("@SYNTAX@", &root.join("syntax.html").to_string_lossy().replace('\\', "\\\\"))
                .replace("@BIG@", &root.join("big.html").to_string_lossy().replace('\\', "\\\\"))
                .replace("@INVALID@", &root.join("invalid.html").to_string_lossy().replace('\\', "\\\\"))
                .replace("@MISSING@", &root.join("missing.html").to_string_lossy().replace('\\', "\\\\"))
                .replace("@ROOT@", &root.to_string_lossy().replace('\\', "\\\\"));
    let file = dir.join("main.blm");
    write_file(&file, &src);
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(stdout.contains("OUT:<h1>Willkommen</h1>\n"), "{stdout}");
    assert!(stdout.contains("OUT:<h1>Nochmal</h1>\n"), "{stdout}");
    assert!(stdout.contains("OUT:\n"), "{stdout}");
    assert!(stdout.contains("ERR: directive requires a path"), "{stdout}");
    assert!(stdout.contains("file exceeds 65536 bytes"), "{stdout}");
    assert!(stdout.contains("file is not valid UTF-8"), "{stdout}");
    assert!(stdout.contains("file not found"), "{stdout}");
    assert!(stdout.contains("OUT:<h1>Ada</h1>\n"), "{stdout}");
    assert!(stdout.contains("OUT:<p>Ada</p>\n"), "{stdout}");
    assert!(stdout.contains("FROM-ERR: path: "), "{stdout}");
    assert!(!stdout.contains("SECRET"), "{stdout}");
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn html_template_file_confinement_natively() {
    let dir = temp_dir("html-template-confine");
    let root = dir.join("root");
    write_file(&root.join("tpl.html"), "ok");
    write_file(&dir.join("outside/secret.html"), "SECRET");
    let junction = root.join("jump");
    let junction_ok = make_junction(&junction, &dir.join("outside"));
    let drive = if cfg!(windows) { "C:\\Windows\\win.ini" } else { "/etc/hosts" };
    let src = r#"use std::html;

fn attempt(name: string, root: string, rel: string) -> void {
    match html::template_file_from(root, rel) {
        Result::Ok(t) => println(name + ": UNEXPECTED-OK"),
        Result::Err(e) => println(name + ": blocked")
    };
}

fn main() -> int {
    attempt("ok", "@ROOT@", "tpl.html");
    attempt("dotdot", "@ROOT@", "../outside/secret.html");
    attempt("nested-dotdot", "@ROOT@", "a/../../outside/secret.html");
    attempt("absolute", "@ROOT@", "@ABS@");
    attempt("junction", "@ROOT@", "jump/secret.html");
    attempt("empty", "@ROOT@", "");
    attempt("dot", "@ROOT@", "./tpl.html");
    return 0;
}
"#.replace("@ROOT@", &root.to_string_lossy().replace('\\', "\\\\"))
                .replace("@ABS@", &drive.replace('\\', "\\\\"));
    let file = dir.join("main.blm");
    write_file(&file, &src);
    let out = run_bloom_env(
        &["run", file.to_string_lossy().as_ref()],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(stdout.contains("ok: UNEXPECTED-OK"), "{stdout}");
    assert!(stdout.contains("dotdot: blocked"), "{stdout}");
    assert!(stdout.contains("nested-dotdot: blocked"), "{stdout}");
    assert!(stdout.contains("absolute: blocked"), "{stdout}");
    assert!(stdout.contains("empty: blocked"), "{stdout}");
    assert!(stdout.contains("dot: UNEXPECTED-OK"), "{stdout}");
    if junction_ok {
        assert!(stdout.contains("junction: blocked"), "{stdout}");
    } else {
        eprintln!("skipping junction assertion: mklink/symlink failed");
    }
    assert!(!stdout.contains("SECRET"), "{stdout}");
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn http_write_response_bytes_natively() {
    use std::io::{BufRead, BufReader, Read, Write};

    let dir = temp_dir("http-bytes");
    let bin = dir.join("bin.dat");
    let empty = dir.join("empty.bin");
    write_bytes(&bin, &[0u8, 1, 255, 0, 65, 0, 66]);
    write_bytes(&empty, &[]);
    let src = r#"use std::http;
use std::net;
use std::fs;

fn ok_int(r: Result<int, string>) -> int {
    return match r {
        Result::Ok(v) => v,
        Result::Err(e) => -1
    };
}

fn write_bin(conn: int, b: fs::Bytes) -> void {
    match http::write_response_bytes(conn, 200, "application/octet-stream", "X-Test: 1", b) {
        Result::Ok(v) => println("sent"),
        Result::Err(e) => println("write-error: " + e)
    };
}

fn write_empty(conn: int, b: fs::Bytes) -> void {
    match http::write_response_bytes(conn, 200, "text/plain; charset=utf-8", "", b) {
        Result::Ok(v) => println("sent"),
        Result::Err(e) => println("write-error: " + e)
    };
}

fn write_no_content(conn: int, b: fs::Bytes) -> void {
    match http::write_response_bytes(conn, 304, "text/plain", "", b) {
        Result::Ok(v) => println("sent"),
        Result::Err(e) => println("write-error: " + e)
    };
}

fn write_head(conn: int, b: fs::Bytes) -> void {
    match http::write_response_bytes_head(conn, 200, "application/octet-stream", "", b) {
        Result::Ok(v) => println("sent"),
        Result::Err(e) => println("write-error: " + e)
    };
}

fn write_bad_header(conn: int, b: fs::Bytes) -> void {
    match http::write_response_bytes(conn, 200, "text/plain\r\nX-Evil: 1", "", b) {
        Result::Ok(v) => println("BAD-HEADER-ACCEPTED"),
        Result::Err(e) => println("write-error: " + e)
    };
}

fn send_bin(conn: int) -> void {
    match fs::read_bytes("@BIN@") {
        Result::Ok(b) => write_bin(conn, b),
        Result::Err(e) => println("load-error: " + e)
    };
}

fn send_empty(conn: int) -> void {
    match fs::read_bytes("@EMPTY@") {
        Result::Ok(b) => write_empty(conn, b),
        Result::Err(e) => println("load-error: " + e)
    };
}

fn send_no_content(conn: int) -> void {
    match fs::read_bytes("@BIN@") {
        Result::Ok(b) => write_no_content(conn, b),
        Result::Err(e) => println("load-error: " + e)
    };
}

fn send_head(conn: int) -> void {
    match fs::read_bytes("@BIN@") {
        Result::Ok(b) => write_head(conn, b),
        Result::Err(e) => println("load-error: " + e)
    };
}

fn send_bad_header(conn: int) -> void {
    match fs::read_bytes("@BIN@") {
        Result::Ok(b) => write_bad_header(conn, b),
        Result::Err(e) => println("load-error: " + e)
    };
}

fn respond(conn: int, request: http::HttpRequest) -> void {
    if request.target == "/empty" {
        send_empty(conn);
    } else if request.target == "/204" {
        send_no_content(conn);
    } else if request.target == "/head" {
        send_head(conn);
    } else if request.target == "/bad" {
        send_bad_header(conn);
    } else {
        send_bin(conn);
    }
}

fn handle(conn: int) -> void {
    match http::read_request(conn) {
        Result::Ok(request) => respond(conn, request),
        Result::Err(e) => println("read: " + e)
    };
    net::tcp_close(conn);
}

fn accept_next(listener: int) -> void {
    match net::tcp_accept(listener) {
        Result::Ok(conn) => handle(conn),
        Result::Err(e) => println("accept: " + e)
    };
}

fn main() -> int {
    let listener = ok_int(net::tcp_listen("127.0.0.1", 0, 4));
    if listener < 0 {
        println("listen fehlgeschlagen");
        return 1;
    }
    println(ok_int(net::local_port(listener)));
    var i = 0;
    while i < 5 {
        accept_next(listener);
        i = i + 1;
    }
    net::tcp_close(listener);
    return 0;
}
"#.replace("@BIN@", &bin.to_string_lossy().replace('\\', "\\\\"))
                .replace("@EMPTY@", &empty.to_string_lossy().replace('\\', "\\\\"));
    let file = dir.join("main.blm");
    write_file(&file, &src);
    let exe = dir.join(if cfg!(windows) { "bytes.exe" } else { "bytes" });
    let build = run_bloom(&[
        "build",
        file.to_string_lossy().as_ref(),
        "-o",
        exe.to_string_lossy().as_ref(),
    ]);
    if no_toolchain(&build) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(build.code, 0, "{}", build.combined());
    let mut child = std::process::Command::new(&exe)
        .env("BLOOM_MEM_REPORT", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn server");
    let mut reader = BufReader::new(child.stdout.take().expect("stdout"));
    let mut port_line = String::new();
    reader.read_line(&mut port_line).expect("port");
    let port: u16 = port_line.trim().parse().expect("port number");

    fn split_response(resp: &[u8]) -> (String, Vec<u8>) {
        let idx = resp
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("header end");
        (
            String::from_utf8_lossy(&resp[..idx]).to_string(),
            resp[idx + 4..].to_vec(),
        )
    }

    let request = |target: &str| -> Vec<u8> {
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
        let raw = format!("GET {target} HTTP/1.1\r\nHost: bloom\r\nConnection: close\r\n\r\n");
        stream.write_all(raw.as_bytes()).expect("write");
        let mut response = Vec::new();
        stream.read_to_end(&mut response).expect("read");
        response
    };

    let r1 = request("/bin");
    let (head1, body1) = split_response(&r1);
    assert!(head1.starts_with("HTTP/1.1 200 OK\r\n"), "{head1}");
    assert!(head1.contains("Content-Type: application/octet-stream"), "{head1}");
    assert!(head1.contains("X-Test: 1\r\n"), "{head1}");
    assert!(head1.contains("Content-Length: 7\r\n"), "{head1}");
    assert_eq!(body1, vec![0u8, 1, 255, 0, 65, 0, 66], "{body1:?}");

    let r2 = request("/empty");
    let (head2, body2) = split_response(&r2);
    assert!(head2.contains("Content-Length: 0\r\n"), "{head2}");
    assert!(body2.is_empty(), "{body2:?}");

    let r3 = request("/204");
    let (head3, body3) = split_response(&r3);
    assert!(head3.starts_with("HTTP/1.1 304 Not Modified\r\n"), "{head3}");
    assert!(head3.contains("Content-Length: 0\r\n"), "{head3}");
    assert!(body3.is_empty(), "{body3:?}");

    let r4 = request("/head");
    let (head4, body4) = split_response(&r4);
    assert!(head4.contains("Content-Length: 7\r\n"), "{head4}");
    assert!(body4.is_empty(), "{body4:?}");

    let r5 = request("/bad");
    assert!(r5.is_empty(), "bad header must be rejected before writing: {r5:?}");

    let status = child.wait().expect("wait");
    let mut rest = String::new();
    reader.read_to_string(&mut rest).expect("rest");
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("stderr")
        .read_to_string(&mut stderr)
        .expect("stderr read");
    assert_eq!(status.code(), Some(0), "{rest}\n{stderr}");
    let stdout = norm(&rest);
    assert!(stdout.contains("sent\nsent\nsent\nsent\n"), "{stdout}");
    assert!(stdout.contains("write-error: "), "{stdout}");
    assert!(!stdout.contains("BAD-HEADER-ACCEPTED"), "{stdout}");
    assert!(stderr.contains("live allocations: 0"), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(windows)]
fn make_junction(link: &std::path::Path, target: &std::path::Path) -> bool {
    std::process::Command::new("cmd")
        .args([
            "/c",
            "mklink",
            "/J",
            link.to_string_lossy().as_ref(),
            target.to_string_lossy().as_ref(),
        ])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(unix)]
fn make_junction(link: &std::path::Path, target: &std::path::Path) -> bool {
    std::os::unix::fs::symlink(target, link).is_ok()
}

fn split_http_response(resp: &[u8]) -> (String, Vec<u8>) {
    let idx = resp
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("header end");
    (
        String::from_utf8_lossy(&resp[..idx]).to_string(),
        resp[idx + 4..].to_vec(),
    )
}

#[test]
fn web_static_files_natively() {
    use std::io::{BufRead, BufReader, Read, Write};

    let dir = temp_dir("web-static");
    let site = dir.join("site");
    let outside = dir.join("outside");
    write_file(&site.join("index.txt"), "hello static");
    write_file(&site.join("style.css"), "body{color:red}");
    write_file(&site.join("sub/app.js"), "console.log('hi');");
    write_file(&site.join("sub/page.html"), "<p>{{name}}</p>");
    write_file(&site.join("note.tpl"), "<b>{{raw}}</b>");
    write_file(&outside.join("secret.txt"), "TOPSECRET");
    write_bytes(&site.join("bin.dat"), &[0u8, 1, 255, 0, 65, 10, 0]);
    write_bytes(&site.join("empty.txt"), &[]);
    write_bytes(&site.join("big.bin"), &vec![b'x'; 8 * 1024 * 1024 + 1]);
    std::fs::create_dir_all(site.join("dir")).expect("dir");
    let junction = site.join("jump");
    let junction_ok = make_junction(&junction, &outside);
    let src = r#"use std::web;
use std::http;
use std::net;

fn ok_int(r: Result<int, string>) -> int {
    return match r {
        Result::Ok(v) => v,
        Result::Err(e) => -1
    };
}

fn read_failed(conn: int, e: string) -> void {
    net::tcp_close(conn);
}

fn serve(conn: int, request: http::HttpRequest) -> void {
    match web::static_file(conn, "@SITE@", request) {
        Result::Ok(status) => println("sent"),
        Result::Err(e) => println("write-error: " + e)
    };
    net::tcp_close(conn);
}

fn handle(conn: int) -> void {
    match http::read_request(conn) {
        Result::Ok(request) => serve(conn, request),
        Result::Err(e) => read_failed(conn, e)
    };
}

fn accept_next(listener: int) -> void {
    match net::tcp_accept(listener) {
        Result::Ok(conn) => handle(conn),
        Result::Err(e) => println("accept: " + e)
    };
}

fn main() -> int {
    let listener = ok_int(net::tcp_listen("127.0.0.1", 0, 8));
    if listener < 0 {
        println("listen fehlgeschlagen");
        return 1;
    }
    println(ok_int(net::local_port(listener)));
    var i = 0;
    while i < 18 {
        accept_next(listener);
        i = i + 1;
    }
    net::tcp_close(listener);
    return 0;
}
"#
    .replace("@SITE@", &site.to_string_lossy().replace('\\', "\\\\"));
    let file = dir.join("main.blm");
    write_file(&file, &src);
    let exe = dir.join(if cfg!(windows) { "static.exe" } else { "static" });
    let build = run_bloom(&[
        "build",
        file.to_string_lossy().as_ref(),
        "-o",
        exe.to_string_lossy().as_ref(),
    ]);
    if no_toolchain(&build) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(build.code, 0, "{}", build.combined());
    let mut child = std::process::Command::new(&exe)
        .env("BLOOM_MEM_REPORT", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn server");
    let mut reader = BufReader::new(child.stdout.take().expect("stdout"));
    let mut port_line = String::new();
    reader.read_line(&mut port_line).expect("port");
    let port: u16 = port_line.trim().parse().expect("port number");

    let request = |method: &str, target: &str| -> Vec<u8> {
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
        let raw = format!(
            "{method} {target} HTTP/1.1\r\nHost: bloom\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(raw.as_bytes()).expect("write");
        let mut response = Vec::new();
        stream.read_to_end(&mut response).expect("read");
        response
    };

    let r1 = request("GET", "/index.txt");
    let (head1, body1) = split_http_response(&r1);
    assert!(head1.starts_with("HTTP/1.1 200 OK\r\n"), "{head1}");
    assert!(head1.contains("Content-Type: text/plain; charset=utf-8"), "{head1}");
    assert!(head1.contains("X-Content-Type-Options: nosniff"), "{head1}");
    assert!(head1.contains("Content-Length: 12\r\n"), "{head1}");
    assert_eq!(body1, b"hello static".to_vec());

    let r2 = request("GET", "/bin.dat");
    let (head2, body2) = split_http_response(&r2);
    assert!(head2.starts_with("HTTP/1.1 200 OK\r\n"), "{head2}");
    assert!(head2.contains("Content-Type: application/octet-stream"), "{head2}");
    assert!(head2.contains("Content-Length: 7\r\n"), "{head2}");
    assert_eq!(body2, vec![0u8, 1, 255, 0, 65, 10, 0], "{body2:?}");

    let r3 = request("GET", "/empty.txt");
    let (head3, body3) = split_http_response(&r3);
    assert!(head3.starts_with("HTTP/1.1 200 OK\r\n"), "{head3}");
    assert!(head3.contains("Content-Length: 0\r\n"), "{head3}");
    assert!(body3.is_empty(), "{body3:?}");

    let r4 = request("HEAD", "/bin.dat");
    let (head4, body4) = split_http_response(&r4);
    assert!(head4.starts_with("HTTP/1.1 200 OK\r\n"), "{head4}");
    assert!(head4.contains("Content-Length: 7\r\n"), "{head4}");
    assert!(body4.is_empty(), "{body4:?}");

    let r5 = request("POST", "/index.txt");
    let (head5, body5) = split_http_response(&r5);
    assert!(head5.starts_with("HTTP/1.1 405 Method Not Allowed\r\n"), "{head5}");
    assert!(head5.contains("Allow: GET, HEAD\r\n"), "{head5}");
    assert_eq!(body5, b"method not allowed".to_vec());

    let r6 = request("GET", "/missing.txt");
    let (head6, _) = split_http_response(&r6);
    assert!(head6.starts_with("HTTP/1.1 404 Not Found\r\n"), "{head6}");

    let r7 = request("GET", "/sub/app.js");
    let (head7, body7) = split_http_response(&r7);
    assert!(head7.contains("Content-Type: text/javascript; charset=utf-8"), "{head7}");
    assert_eq!(body7, b"console.log('hi');".to_vec());

    let r8 = request("GET", "/sub/page.html");
    let (head8, body8) = split_http_response(&r8);
    assert!(head8.contains("Content-Type: text/html; charset=utf-8"), "{head8}");
    assert_eq!(body8, b"<p>{{name}}</p>".to_vec());

    let r9 = request("GET", "/style.css");
    let (head9, _) = split_http_response(&r9);
    assert!(head9.contains("Content-Type: text/css; charset=utf-8"), "{head9}");

    let r10 = request("GET", "/../outside/secret.txt");
    let (head10, _) = split_http_response(&r10);
    assert!(head10.starts_with("HTTP/1.1 400 "), "{head10}");

    let r11 = request("GET", "/sub%2Fapp.js");
    let (head11, _) = split_http_response(&r11);
    assert!(head11.starts_with("HTTP/1.1 400 "), "{head11}");

    let r12 = request("GET", "/%zz");
    let (head12, _) = split_http_response(&r12);
    assert!(head12.starts_with("HTTP/1.1 400 "), "{head12}");

    let r13 = request("GET", "/index.txt?q=1");
    let (head13, body13) = split_http_response(&r13);
    assert!(head13.starts_with("HTTP/1.1 200 OK\r\n"), "{head13}");
    assert_eq!(body13, b"hello static".to_vec());

    let r14 = request("GET", "/jump/secret.txt");
    let (head14, body14) = split_http_response(&r14);
    if junction_ok {
        assert!(head14.starts_with("HTTP/1.1 400 "), "{head14}");
    } else {
        assert!(head14.starts_with("HTTP/1.1 404 "), "{head14}");
    }
    assert!(!String::from_utf8_lossy(&body14).contains("TOPSECRET"));

    let r15 = request("GET", "/big.bin");
    let (head15, _) = split_http_response(&r15);
    assert!(head15.starts_with("HTTP/1.1 500 "), "{head15}");

    let r16 = request("GET", "/dir");
    let (head16, _) = split_http_response(&r16);
    assert!(head16.starts_with("HTTP/1.1 404 "), "{head16}");

    let r17 = request("GET", "/");
    let (head17, _) = split_http_response(&r17);
    assert!(head17.starts_with("HTTP/1.1 404 "), "{head17}");

    let r18 = request("GET", "/note.tpl");
    let (head18, body18) = split_http_response(&r18);
    assert!(head18.starts_with("HTTP/1.1 200 OK\r\n"), "{head18}");
    assert_eq!(body18, b"<b>{{raw}}</b>".to_vec(), "static files stay raw");

    let status = child.wait().expect("wait");
    let mut rest = String::new();
    reader.read_to_string(&mut rest).expect("rest");
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("stderr")
        .read_to_string(&mut stderr)
        .expect("stderr read");
    assert_eq!(status.code(), Some(0), "{rest}\n{stderr}");
    assert_eq!(norm(&rest).matches("sent\n").count(), 18, "{rest}");
    assert!(!rest.contains("write-error"), "{rest}");
    assert!(stderr.contains("live allocations: 0"), "{stderr}");
    assert!(stderr.contains("live sockets: 0"), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn template_file_example_natively() {
    let out = run_bloom_env(
        &["run", &example("template_file.blm")],
        &[("BLOOM_MEM_REPORT", "1")],
    );
    if no_toolchain(&out) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(out.code, 0, "{}", out.combined());
    let stdout = norm(&out.stdout);
    assert!(stdout.contains("<h1>Startseite</h1>"), "{stdout}");
    assert!(stdout.contains("<h1>Zweite Seite</h1>"), "{stdout}");
    assert!(stdout.contains("FEHLER: template_file: "), "{stdout}");
    assert!(stdout.contains("Verzeichnis entfernt"), "{stdout}");
    assert!(
        out.stderr.contains("live allocations: 0"),
        "{}",
        out.combined()
    );
}

fn spawn_example_server(
    dir: &PathBuf,
    name: &str,
    exe_name: &str,
) -> Option<(std::process::Child, u16, std::io::BufReader<std::process::ChildStdout>)> {
    use std::io::{BufRead, BufReader};

    let exe = dir.join(if cfg!(windows) { exe_name.to_string() + ".exe" } else { exe_name.to_string() });
    let build = run_bloom(&[
        "build",
        &example(name),
        "-o",
        exe.to_string_lossy().as_ref(),
    ]);
    if no_toolchain(&build) {
        eprintln!("skipping: no C toolchain available");
        return None;
    }
    assert_eq!(build.code, 0, "{}", build.combined());
    let mut child = std::process::Command::new(&exe)
        .env("BLOOM_MEM_REPORT", "1")
        .current_dir(dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn server");
    let mut reader = BufReader::new(child.stdout.take().expect("stdout"));
    let mut port_line = String::new();
    reader.read_line(&mut port_line).expect("port");
    let port: u16 = port_line.trim().parse().expect("port number");
    Some((child, port, reader))
}

#[test]
fn web_static_files_example_natively() {
    use std::io::{Read, Write};

    let dir = temp_dir("web-static-example");
    let (mut child, port, mut reader) = match spawn_example_server(&dir, "web_static_files.blm", "web_static") {
        Some(v) => v,
        None => return,
    };
    let request = |method: &str, target: &str| -> Vec<u8> {
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
        let raw = format!(
            "{method} {target} HTTP/1.1\r\nHost: bloom\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(raw.as_bytes()).expect("write");
        let mut response = Vec::new();
        stream.read_to_end(&mut response).expect("read");
        response
    };
    let mut statuses = Vec::new();
    for (method, target) in [
        ("GET", "/index.html"),
        ("GET", "/style.css"),
        ("GET", "/sub/app.js"),
        ("GET", "/data.json"),
        ("GET", "/notes.txt"),
        ("HEAD", "/index.html"),
        ("GET", "/nope"),
        ("POST", "/index.html"),
        ("GET", "/../outside"),
        ("GET", "/"),
    ] {
        let resp = request(method, target);
        let (head, _) = split_http_response(&resp);
        statuses.push((method, target, head));
    }
    assert!(statuses[0].2.starts_with("HTTP/1.1 200 "), "{:?}", statuses[0].2);
    assert!(statuses[1].2.contains("text/css"), "{:?}", statuses[1].2);
    assert!(statuses[2].2.contains("text/javascript"), "{:?}", statuses[2].2);
    assert!(statuses[3].2.contains("application/json"), "{:?}", statuses[3].2);
    assert!(statuses[4].2.contains("text/plain"), "{:?}", statuses[4].2);
    assert!(statuses[5].2.starts_with("HTTP/1.1 200 "), "{:?}", statuses[5].2);
    assert!(statuses[6].2.starts_with("HTTP/1.1 404 "), "{:?}", statuses[6].2);
    assert!(statuses[7].2.starts_with("HTTP/1.1 405 "), "{:?}", statuses[7].2);
    assert!(statuses[7].2.contains("Allow: GET, HEAD"), "{:?}", statuses[7].2);
    assert!(statuses[8].2.starts_with("HTTP/1.1 400 "), "{:?}", statuses[8].2);
    assert!(statuses[9].2.starts_with("HTTP/1.1 404 "), "{:?}", statuses[9].2);

    let status = child.wait().expect("wait");
    let mut rest = String::new();
    reader.read_to_string(&mut rest).expect("rest");
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("stderr")
        .read_to_string(&mut stderr)
        .expect("stderr read");
    assert_eq!(status.code(), Some(0), "{rest}\n{stderr}");
    assert_eq!(norm(&rest).matches("gesendet: ").count(), 10, "{rest}");
    assert!(stderr.contains("live allocations: 0"), "{stderr}");
    assert!(stderr.contains("live sockets: 0"), "{stderr}");
    assert!(!dir.join("bloom_static_demo").exists(), "cleanup failed");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn web_html_site_example_natively() {
    use std::io::{Read, Write};

    let dir = temp_dir("web-html-site");
    let (mut child, port, mut reader) = match spawn_example_server(&dir, "web_html_site.blm", "web_site") {
        Some(v) => v,
        None => return,
    };
    let request = |method: &str, target: &str| -> Vec<u8> {
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
        let raw = format!(
            "{method} {target} HTTP/1.1\r\nHost: bloom\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(raw.as_bytes()).expect("write");
        let mut response = Vec::new();
        stream.read_to_end(&mut response).expect("read");
        response
    };
    let r1 = request("GET", "/");
    let (head1, body1) = split_http_response(&r1);
    assert!(head1.starts_with("HTTP/1.1 200 OK\r\n"), "{head1}");
    assert!(head1.contains("Content-Type: text/html; charset=utf-8"), "{head1}");
    let page = String::from_utf8_lossy(&body1);
    assert!(page.contains("<h1>Bloom-Website</h1>"), "{page}");
    assert!(page.contains("Vorlage aus Datei"), "{page}");

    let r2 = request("GET", "/style.css");
    let (head2, body2) = split_http_response(&r2);
    assert!(head2.contains("text/css"), "{head2}");
    assert_eq!(body2, b"h1 { color: #245; }".to_vec());

    let r3 = request("GET", "/app.js");
    let (head3, body3) = split_http_response(&r3);
    assert!(head3.contains("text/javascript"), "{head3}");
    assert_eq!(body3, b"console.log('seite geladen');".to_vec());

    let r4 = request("GET", "/templates/page.html");
    let (head4, body4) = split_http_response(&r4);
    assert!(head4.contains("text/html"), "{head4}");
    assert!(String::from_utf8_lossy(&body4).contains("{{title}}"));

    let r5 = request("GET", "/missing");
    let (head5, _) = split_http_response(&r5);
    assert!(head5.starts_with("HTTP/1.1 404 "), "{head5}");

    let r6 = request("HEAD", "/style.css");
    let (head6, body6) = split_http_response(&r6);
    assert!(head6.starts_with("HTTP/1.1 200 "), "{head6}");
    assert!(body6.is_empty());

    let r7 = request("POST", "/style.css");
    let (head7, _) = split_http_response(&r7);
    assert!(head7.starts_with("HTTP/1.1 405 "), "{head7}");

    let r8 = request("GET", "/../escape");
    let (head8, _) = split_http_response(&r8);
    assert!(head8.starts_with("HTTP/1.1 400 "), "{head8}");

    let r9 = request("GET", "/");
    let (head9, body9) = split_http_response(&r9);
    assert!(head9.starts_with("HTTP/1.1 200 "), "{head9}");
    assert!(String::from_utf8_lossy(&body9).contains("<h1>Bloom-Website</h1>"));

    let r10 = request("GET", "/sub");
    let (head10, _) = split_http_response(&r10);
    assert!(head10.starts_with("HTTP/1.1 404 "), "{head10}");

    let status = child.wait().expect("wait");
    let mut rest = String::new();
    reader.read_to_string(&mut rest).expect("rest");
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("stderr")
        .read_to_string(&mut stderr)
        .expect("stderr read");
    assert_eq!(status.code(), Some(0), "{rest}\n{stderr}");
    assert_eq!(norm(&rest).matches("Seite gesendet").count(), 2, "{rest}");
    assert_eq!(norm(&rest).matches("statisch: 200").count(), 4, "{rest}");
    assert!(stderr.contains("live allocations: 0"), "{stderr}");
    assert!(stderr.contains("live sockets: 0"), "{stderr}");
    assert!(!dir.join("bloom_site_demo").exists(), "cleanup failed");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn web_static_errors_example_natively() {
    use std::io::{BufRead, BufReader, Read, Write};

    let dir = temp_dir("web-static-errors");
    let exe = dir.join(if cfg!(windows) { "static_errors.exe" } else { "static_errors" });
    let build = run_bloom(&[
        "build",
        &example("web_static_errors.blm"),
        "-o",
        exe.to_string_lossy().as_ref(),
    ]);
    if no_toolchain(&build) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(build.code, 0, "{}", build.combined());
    let mut child = std::process::Command::new(&exe)
        .env("BLOOM_MEM_REPORT", "1")
        .current_dir(&dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn server");
    let mut reader = BufReader::new(child.stdout.take().expect("stdout"));
    let mut port_line = String::new();
    reader.read_line(&mut port_line).expect("port");
    let port: u16 = port_line.trim().parse().expect("port number");
    let request = |method: &str, target: &str| -> Vec<u8> {
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
        let raw = format!(
            "{method} {target} HTTP/1.1\r\nHost: bloom\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(raw.as_bytes()).expect("write");
        let mut response = Vec::new();
        stream.read_to_end(&mut response).expect("read");
        response
    };
    let mut heads = Vec::new();
    for (method, target) in [
        ("GET", "/index.txt"),
        ("GET", "/missing.txt"),
        ("POST", "/index.txt"),
        ("GET", "/../secret"),
        ("GET", "/%zz"),
        ("HEAD", "/index.txt"),
        ("GET", "/index.txt?x=1"),
        ("GET", "/nothing"),
    ] {
        let resp = request(method, target);
        let (head, _) = split_http_response(&resp);
        heads.push(head);
    }
    assert!(heads[0].starts_with("HTTP/1.1 200 "), "{}", heads[0]);
    assert!(heads[1].starts_with("HTTP/1.1 404 "), "{}", heads[1]);
    assert!(heads[2].starts_with("HTTP/1.1 405 "), "{}", heads[2]);
    assert!(heads[3].starts_with("HTTP/1.1 400 "), "{}", heads[3]);
    assert!(heads[4].starts_with("HTTP/1.1 400 "), "{}", heads[4]);
    assert!(heads[5].starts_with("HTTP/1.1 200 "), "{}", heads[5]);
    assert!(heads[6].starts_with("HTTP/1.1 200 "), "{}", heads[6]);
    assert!(heads[7].starts_with("HTTP/1.1 404 "), "{}", heads[7]);
    let status = child.wait().expect("wait");
    let mut rest = String::new();
    reader.read_to_string(&mut rest).expect("rest");
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("stderr")
        .read_to_string(&mut stderr)
        .expect("stderr read");
    assert_eq!(status.code(), Some(0), "{rest}\n{stderr}");
    let log = norm(&rest);
    assert!(log.contains("Status 200: GET /index.txt"), "{log}");
    assert!(log.contains("Status 405: POST /index.txt"), "{log}");
    assert!(log.contains("Status 400: GET /../secret"), "{log}");
    assert!(log.contains("Status 404: GET /missing.txt"), "{log}");
    assert!(stderr.contains("live allocations: 0"), "{stderr}");
    assert!(stderr.contains("live sockets: 0"), "{stderr}");
    assert!(!dir.join("bloom_static_errors_demo").exists(), "cleanup failed");
    let _ = std::fs::remove_dir_all(&dir);
}
