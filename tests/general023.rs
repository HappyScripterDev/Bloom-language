use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bloom-it-023-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(path, content).expect("write file");
}

fn norm(s: &str) -> String {
    s.replace("\r\n", "\n")
}

fn no_toolchain(out: &std::process::Output) -> bool {
    out.status.code() == Some(3)
}

fn run_bloom(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_bloom"))
        .args(args)
        .output()
        .expect("run bloom")
}

fn build_program(dir: &Path, name: &str, source: &str) -> Option<PathBuf> {
    let src = dir.join(format!("{name}.blm"));
    write_file(&src, source);
    let exe = dir.join(format!("{name}.exe"));
    let build = run_bloom(&[
        "build",
        src.to_string_lossy().as_ref(),
        "-o",
        exe.to_string_lossy().as_ref(),
        "--console",
    ]);
    if no_toolchain(&build) {
        eprintln!("skipping: no C toolchain available");
        return None;
    }
    assert_eq!(
        build.status.code(),
        Some(0),
        "{}\n{}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );
    Some(exe)
}

struct RunResult {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run_exe(dir: &Path, exe: &Path, args: &[&str], stdin: Option<&str>) -> RunResult {
    let mut command = Command::new(exe);
    command
        .args(args)
        .current_dir(dir)
        .env("BLOOM_MEM_REPORT", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if stdin.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command.spawn().expect("spawn program");
    if let Some(input) = stdin {
        use std::io::Write;
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(input.as_bytes())
            .expect("write stdin");
    }
    let out = child.wait_with_output().expect("wait program");
    RunResult {
        code: out.status.code().unwrap_or(-1),
        stdout: norm(&String::from_utf8_lossy(&out.stdout)),
        stderr: norm(&String::from_utf8_lossy(&out.stderr)),
    }
}

fn assert_run_ok(name: &str, run: &RunResult) {
    assert_eq!(run.code, 0, "{name}: {}\n{}", run.stdout, run.stderr);
    assert!(
        run.stderr.contains("live allocations: 0"),
        "{name}: {}",
        run.stderr
    );
}

#[test]
fn env_natively() {
    let dir = temp_dir("env");
    let source = r#"use std::env;

fn main() -> int {
    return match run() {
        Result::Ok(code) => code,
        Result::Err(e) => fehler(e)
    };
}

fn run() -> Result<int, string> {
    let args = env::args();
    println("count=" + str(len(args)));
    println("arg=" + args[1]);
    println("os=" + env::os() + " arch=" + env::arch());
    println("pid=" + str(env::process_id() > 0));
    let cwd = env::current_dir()?;
    println("cwd=" + str(len(cwd) > 0));
    let exe = env::executable_path()?;
    println("exe=" + str(len(exe) > 0));
    env::set("BLOOM_023_UNI", "Grüße 🚀")?;
    match env::get("BLOOM_023_UNI") {
        Option::Some(v) => println("uni=" + v),
        Option::None => println("uni=missing")
    };
    env::set("BLOOM_023_EMPTY", "")?;
    match env::get("BLOOM_023_EMPTY") {
        Option::Some(v) => println("empty=" + str(len(v))),
        Option::None => println("empty=missing")
    };
    println("missing=" + str(env::has("BLOOM_023_MISSING")));
    env::remove("BLOOM_023_EMPTY")?;
    println("after-remove=" + str(env::has("BLOOM_023_EMPTY")));
    let target = args[2];
    env::set_current_dir(target)?;
    let moved = env::current_dir()?;
    println("moved=" + str(contains(moved, "bloom-it-023-env")));
    return Result::Ok(0);
}

fn fehler(e: string) -> int {
    println("FEHLER: " + e);
    return 1;
}
"#;
    let exe = match build_program(&dir, "env_probe", source) {
        Some(p) => p,
        None => return,
    };
    let arg = "arg mit Leerzeichen & Zeichen";
    let target = dir.to_string_lossy().to_string();
    let run = run_exe(&dir, &exe, &[arg, &target], None);
    assert_run_ok("env", &run);
    for needle in [
        "count=3",
        "arg=arg mit Leerzeichen & Zeichen",
        "os=windows",
        "pid=true",
        "cwd=true",
        "exe=true",
        "uni=Grüße 🚀",
        "empty=0",
        "missing=false",
        "after-remove=false",
        "moved=true",
    ] {
        assert!(
            run.stdout.contains(needle),
            "missing `{needle}` in {}",
            run.stdout
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn collections_natively() {
    let dir = temp_dir("collections");
    let source = r#"use std::collections;

fn main() -> int {
    return match run() {
        Result::Ok(code) => code,
        Result::Err(e) => fehler(e)
    };
}

fn run() -> Result<int, string> {
    let list = collections::list_new();
    println("empty=" + str(collections::list_len(list)));
    collections::list_push(list, "a")?;
    collections::list_push(list, "b")?;
    collections::list_insert(list, 1, "c")?;
    println("list=" + join(collections::list_values(list), ","));
    println("get=" + collections::list_get(list, 1)?);
    println("removed=" + collections::list_remove(list, 0)?);
    collections::list_set(list, 0, "d")?;
    println("set=" + join(collections::list_values(list), ","));
    for i in 0..1500 {
        collections::list_push(list, "v" + str(i))?;
    }
    println("grown=" + str(collections::list_len(list)));
    match collections::list_get(list, 99999) {
        Result::Ok(_) => println("bad-index=accepted"),
        Result::Err(_) => println("bad-index=rejected")
    };
    collections::list_clear(list);
    println("cleared=" + str(collections::list_len(list)));

    let map = collections::map_new();
    println("map-replace1=" + str(collections::map_insert(map, "a", "1")?));
    println("map-replace2=" + str(collections::map_insert(map, "a", "2")?));
    for i in 0..500 {
        collections::map_insert(map, "k" + str(i), "v" + str(i))?;
    }
    println("map-len=" + str(collections::map_len(map)));
    match collections::map_get(map, "a") {
        Option::Some(v) => println("map-a=" + v),
        Option::None => println("map-a=missing")
    };
    println("map-remove=" + str(collections::map_remove(map, "a")?));
    println("map-has=" + str(collections::map_has(map, "a")));
    match collections::map_get(map, "") {
        Option::Some(_) => println("bad-key=accepted"),
        Option::None => println("bad-key=missing")
    };

    let set = collections::set_new();
    println("set-add=" + str(collections::set_insert(set, "x")?));
    println("set-dup=" + str(collections::set_insert(set, "x")?));
    for i in 0..500 {
        collections::set_insert(set, "s" + str(i))?;
    }
    println("set-len=" + str(collections::set_len(set)));
    println("set-remove=" + str(collections::set_remove(set, "x")?));
    println("set-has=" + str(collections::set_has(set, "x")));
    collections::set_clear(set);
    println("set-clear=" + str(collections::set_len(set)));
    return Result::Ok(0);
}

fn fehler(e: string) -> int {
    println("FEHLER: " + e);
    return 1;
}
"#;
    let exe = match build_program(&dir, "collections_probe", source) {
        Some(p) => p,
        None => return,
    };
    let run = run_exe(&dir, &exe, &[], None);
    assert_run_ok("collections", &run);
    for needle in [
        "empty=0",
        "list=a,c,b",
        "get=c",
        "removed=a",
        "set=d,b",
        "grown=1502",
        "bad-index=rejected",
        "cleared=0",
        "map-replace1=false",
        "map-replace2=true",
        "map-len=501",
        "map-a=2",
        "map-remove=true",
        "map-has=false",
        "bad-key=missing",
        "set-add=true",
        "set-dup=false",
        "set-len=501",
        "set-remove=true",
        "set-has=false",
        "set-clear=0",
    ] {
        assert!(
            run.stdout.contains(needle),
            "missing `{needle}` in {}",
            run.stdout
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn process_natively() {
    let dir = temp_dir("process");
    let echo_src = r#"use std::env;
fn main() -> int {
    let args = env::args();
    println(join(slice(args, 1, len(args)), "|"));
    return 0;
}
"#;
    let big_src = r#"fn main() -> int {
    for i in 0..5000 {
        print("xxxxxxxxxx");
    }
    return 0;
}
"#;
    let echo = match build_program(&dir, "argv_echo", echo_src) {
        Some(p) => p,
        None => return,
    };
    let big = match build_program(&dir, "big_output", big_src) {
        Some(p) => p,
        None => return,
    };
    let parent_src = r#"use std::env;
use std::process;

fn main() -> int {
    return match run() {
        Result::Ok(code) => code,
        Result::Err(e) => fehler(e)
    };
}

fn run() -> Result<int, string> {
    let args = env::args();
    let echo = args[1];
    let big = args[2];
    let options = process::ProcessOptions {
        directory: "",
        timeout_ms: 5000,
        max_output: 1048576
    };
    let out = process::run(echo, ["a b", "x&y", "Grüße", "q\"x", "back\\slash"])?;
    println("exit=" + str(process::exit_code(out)) + " ok=" + str(process::success(out)));
    println("quoted=" + str(contains(process::stdout(out), "a b|x&y|Grüße|q\"x|back\\slash")));

    let options2 = process::ProcessOptions {
        directory: "",
        timeout_ms: 300,
        max_output: 65536
    };
    match process::run_options("C:\\Windows\\System32\\cmd.exe", ["/C", "ping -n 4 127.0.0.1 > nul"], options2) {
        Result::Ok(_) => println("timeout=not-triggered"),
        Result::Err(e) => println("timeout=" + str(contains(e, "timed out")))
    };
    let options3 = process::ProcessOptions {
        directory: "",
        timeout_ms: 5000,
        max_output: 1000
    };
    match process::run_options(big, [], options3) {
        Result::Ok(_) => println("limit=not-triggered"),
        Result::Err(e) => println("limit=" + str(contains(e, "limit")))
    };
    match process::run("C:\\Windows\\System32\\does-not-exist.exe", []) {
        Result::Ok(_) => println("missing=accepted"),
        Result::Err(_) => println("missing=rejected")
    };
    let child = process::spawn("C:\\Windows\\System32\\cmd.exe", ["/C", "ping -n 4 127.0.0.1 > nul"], options)?;
    match process::try_wait(child) {
        Option::Some(_) => println("running=exited"),
        Option::None => println("running=yes")
    };
    println("kill=" + str(process::kill(child)?));
    let killed = process::wait(child)?;
    println("killed-exit=" + str(process::exit_code(killed) >= 0));
    return Result::Ok(0);
}

fn fehler(e: string) -> int {
    println("FEHLER: " + e);
    return 1;
}
"#;
    let parent = match build_program(&dir, "process_probe", parent_src) {
        Some(p) => p,
        None => return,
    };
    let run = run_exe(
        &dir,
        &parent,
        &[
            echo.to_string_lossy().as_ref(),
            big.to_string_lossy().as_ref(),
        ],
        None,
    );
    assert_run_ok("process", &run);
    for needle in [
        "exit=0 ok=true",
        "quoted=true",
        "timeout=true",
        "limit=true",
        "missing=rejected",
        "running=yes",
        "kill=true",
        "killed-exit=true",
    ] {
        assert!(
            run.stdout.contains(needle),
            "missing `{needle}` in {}",
            run.stdout
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_natively() {
    let dir = temp_dir("cli");
    let source = r#"use std::cli;
use std::env;

fn main() -> int {
    return match run() {
        Result::Ok(code) => code,
        Result::Err(e) => fehler(e)
    };
}

fn run() -> Result<int, string> {
    let cmd = cli::command("tool", "CLI-Test");
    cli::flag(cmd, "verbose", "mehr ausgeben")?;
    cli::option(cmd, "target", "Ziel", "windows", false)?;
    cli::argument(cmd, "input", "Datei", true)?;
    let build = cli::subcommand(cmd, "build", "Bauen")?;
    cli::flag(build, "release", "Release")?;
    cli::option(build, "target", "Ziel", "windows", false)?;
    let args = env::args();
    let argv = slice(args, 1, len(args));
    match cli::parse(cmd, argv) {
        Result::Ok(parsed) => show(parsed),
        Result::Err(e) => fail(cmd, e)
    };
    return Result::Ok(0);
}

fn show(parsed: cli::Parsed) -> void {
    println("verbose=" + str(cli::has(parsed, "verbose")));
    match cli::subcommand_name(parsed) {
        Option::Some(v) => println("sub=" + v),
        Option::None => println("sub=npne")
    };
    match cli::value(parsed, "target") {
        Option::Some(v) => println("target=" + v),
        Option::None => println("target=npne")
    };
    match cli::positional(parsed, 0) {
        Option::Some(v) => println("input=" + v),
        Option::None => println("input=npne")
    };
    println("release=" + str(cli::has(parsed, "release")));
}

fn fail(cmd: cli::Command, error: string) -> void {
    println("error=" + error);
    println("help=" + str(contains(cli::help(cmd), "Usage:")));
}

fn fehler(e: string) -> int {
    println("FEHLER: " + e);
    return 1;
}
"#;
    let exe = match build_program(&dir, "cli_probe", source) {
        Some(p) => p,
        None => return,
    };
    let run = run_exe(
        &dir,
        &exe,
        &["--verbose", "--target=linux", "datei.txt"],
        None,
    );
    assert_run_ok("cli1", &run);
    assert!(run.stdout.contains("verbose=true"), "{}", run.stdout);
    assert!(run.stdout.contains("target=linux"), "{}", run.stdout);
    assert!(run.stdout.contains("input=datei.txt"), "{}", run.stdout);

    let run = run_exe(&dir, &exe, &["datei.txt"], None);
    assert_run_ok("cli2", &run);
    assert!(run.stdout.contains("verbose=false"), "{}", run.stdout);
    assert!(run.stdout.contains("target=windows"), "{}", run.stdout);

    let run = run_exe(
        &dir,
        &exe,
        &["build", "--release", "--target", "wasm"],
        None,
    );
    assert_run_ok("cli3", &run);
    assert!(run.stdout.contains("sub=build"), "{}", run.stdout);
    assert!(run.stdout.contains("release=true"), "{}", run.stdout);
    assert!(run.stdout.contains("target=wasm"), "{}", run.stdout);

    let run = run_exe(&dir, &exe, &["--unknown", "x"], None);
    assert_run_ok("cli4", &run);
    assert!(
        run.stdout.contains("error=parse: unknown option"),
        "{}",
        run.stdout
    );
    assert!(run.stdout.contains("help=true"), "{}", run.stdout);

    let run = run_exe(&dir, &exe, &[], None);
    assert_run_ok("cli5", &run);
    assert!(
        run.stdout
            .contains("error=parse: required argument is missing"),
        "{}",
        run.stdout
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn io_natively() {
    let dir = temp_dir("io");
    let source = r#"use std::io;

fn main() -> int {
    return match run() {
        Result::Ok(code) => code,
        Result::Err(e) => fehler(e)
    };
}

fn run() -> Result<int, string> {
    io::stdout_write("out\n")?;
    io::stderr_write("err\n")?;
    io::stdout_flush()?;
    io::stderr_flush()?;
    match io::read_line() {
        Result::Ok(Option::Some(line)) => println("line1=" + line),
        Result::Ok(Option::None) => println("line1=eof"),
        Result::Err(e) => println("line1-error=" + e)
    };
    match io::read_line() {
        Result::Ok(Option::Some(line)) => println("line2=" + line),
        Result::Ok(Option::None) => println("line2=eof"),
        Result::Err(e) => println("line2-error=" + e)
    };
    let rest = io::read_stdin(1024)?;
    println("rest=" + rest);
    return Result::Ok(0);
}

fn fehler(e: string) -> int {
    println("FEHLER: " + e);
    return 1;
}
"#;
    let exe = match build_program(&dir, "io_probe", source) {
        Some(p) => p,
        None => return,
    };
    let run = run_exe(&dir, &exe, &[], Some("eins\nzwei\nrest\n"));
    assert_run_ok("io", &run);
    assert!(run.stdout.contains("out\n"), "{}", run.stdout);
    assert!(run.stdout.contains("line1=eins"), "{}", run.stdout);
    assert!(run.stdout.contains("line2=zwei"), "{}", run.stdout);
    assert!(run.stdout.contains("rest=rest\n"), "{}", run.stdout);
    assert!(run.stderr.contains("err"), "{}", run.stderr);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn examples_natively() {
    let dir = temp_dir("examples");
    let cases: &[(&str, &[&str], &[&str])] = &[
        (
            "env_info",
            &[],
            &["os=windows", "cwd=", "after-remove=false"],
        ),
        (
            "cli_basic",
            &["--verbose", "--target", "linux", "datei.txt"],
            &["verbose aktiv", "target=linux", "input=datei.txt"],
        ),
        (
            "cli_subcommands",
            &["build", "--release", "--target", "wasm"],
            &["subcommand=build", "release=true", "target=wasm"],
        ),
        ("process_run", &[], &["Hallo aus einem Kindprozess"]),
        (
            "process_output",
            &[],
            &[
                "exit=7",
                "success=false",
                "stdout=standard",
                "stderr=fehler",
            ],
        ),
        ("process_timeout", &[], &["timeout=true"]),
        (
            "collections_list",
            &[],
            &[
                "values=eins,mitte,zwei",
                "bad-index=rejected",
                "after-clear=0",
            ],
        ),
        (
            "collections_map",
            &[],
            &["replace=true", "name=Bloom 0.23", "has-after-remove=false"],
        ),
        (
            "collections_set",
            &[],
            &["insert2=false", "len=3", "after-clear=0"],
        ),
        (
            "automation_build",
            &[],
            &["build gestartet", "build fertig", "build-success=true"],
        ),
        ("automation_project", &[], &["entries=", "README.md"]),
        ("bloom_task_runner", &[], &["task-exit=0", "task-exit=0"]),
    ];
    for (name, args, needles) in cases {
        let source =
            std::fs::read_to_string(example(&format!("{name}.blm"))).expect("read example");
        let exe = match build_program(&dir, name, &source) {
            Some(p) => p,
            None => return,
        };
        let run = run_exe(&dir, &exe, args, None);
        assert_run_ok(name, &run);
        assert!(!run.stdout.contains("FEHLER"), "{name}: {}", run.stdout);
        for needle in *needles {
            assert!(
                run.stdout.contains(needle),
                "{name}: missing `{needle}` in {}",
                run.stdout
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn emit_c_contains_new_runtimes() {
    let dir = temp_dir("emitc");
    let src = dir.join("emit.blm");
    write_file(
        &src,
        "use std::env;\nuse std::cli;\nuse std::collections;\nuse std::process;\nuse std::io;\nfn main() -> int { return 0; }\n",
    );
    let c_path = dir.join("emit.c");
    let build = run_bloom(&[
        "build",
        src.to_string_lossy().as_ref(),
        "-o",
        dir.join("emit.exe").to_string_lossy().as_ref(),
        "--emit-c",
        c_path.to_string_lossy().as_ref(),
        "--console",
    ]);
    if no_toolchain(&build) {
        eprintln!("skipping: no C toolchain available");
        return;
    }
    assert_eq!(
        build.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let emitted = std::fs::read_to_string(&c_path).expect("emit-c");
    for needle in [
        "bloom_env_args",
        "bloom_cli_parse",
        "bloom_collections_list_push",
        "bloom_process_spawn",
        "bloom_io_read_line",
    ] {
        assert!(emitted.contains(needle), "missing `{needle}` in emitted C");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
