use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::check::{self, CheckedProgram};
use crate::codegen;
use crate::diagnostics::{codes, render_all_with_sources, Diagnostic};
use crate::lexer::lex;
use crate::modules;
use crate::parser::parse;
use crate::toolchain;

pub enum BloomError {
    Diagnostics {
        diagnostics: Vec<Diagnostic>,
        sources: Vec<(String, String)>,
    },
    Io(String),
    Toolchain(String),
    NativeCompile {
        toolchain: String,
        output: String,
    },
}

impl fmt::Debug for BloomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl BloomError {
    pub fn exit_code(&self) -> i32 {
        match self {
            BloomError::Diagnostics { .. } => 1,
            BloomError::Io(_) => 5,
            BloomError::Toolchain(_) => 3,
            BloomError::NativeCompile { .. } => 4,
        }
    }

    pub fn report(&self) {
        match self {
            BloomError::Diagnostics {
                diagnostics,
                sources,
            } => {
                let (fallback_source, fallback_file) = sources
                    .first()
                    .map(|(f, s)| (s.as_str(), f.as_str()))
                    .unwrap_or(("", "<input>"));
                eprint!(
                    "{}",
                    render_all_with_sources(diagnostics, sources, fallback_source, fallback_file)
                );
            }
            BloomError::Io(message) => {
                eprintln!("error[{}]: {}", codes::IO, message);
            }
            BloomError::Toolchain(message) => {
                eprintln!("error[{}]: {}", codes::NO_C_COMPILER, message);
            }
            BloomError::NativeCompile { toolchain, output } => {
                eprintln!(
                    "error[{}]: native Kompilierung mit {} fehlgeschlagen",
                    codes::NATIVE_COMPILE,
                    toolchain
                );
                if !output.is_empty() {
                    eprintln!("--- Compiler-Ausgabe ---");
                    eprintln!("{}", output);
                    eprintln!("------------------------");
                }
            }
        }
    }
}

impl fmt::Display for BloomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BloomError::Diagnostics { diagnostics, .. } => {
                write!(f, "{} compiler error(s)", diagnostics.len())
            }
            BloomError::Io(m) => write!(f, "{}", m),
            BloomError::Toolchain(m) => write!(f, "{}", m),
            BloomError::NativeCompile { toolchain, .. } => {
                write!(f, "native compilation with {} failed", toolchain)
            }
        }
    }
}

#[allow(dead_code)]
pub fn compile_source(source: &str) -> Result<CheckedProgram, Vec<Diagnostic>> {
    let lexed = lex(source);
    if !lexed.diagnostics.is_empty() {
        return Err(lexed.diagnostics);
    }
    let parsed = parse(lexed.tokens);
    if !parsed.diagnostics.is_empty() {
        return Err(parsed.diagnostics);
    }
    check::check(parsed.program)
}

type CheckFailure = (Vec<Diagnostic>, Vec<(String, String)>);

fn check_loaded(loaded: modules::LoadedProgram) -> Result<CheckedProgram, CheckFailure> {
    let paths = modules::module_display_paths(&loaded.modules);
    let sources: Vec<(String, String)> = loaded
        .modules
        .iter()
        .zip(paths.iter())
        .map(|(m, p)| (p.clone(), m.source.clone()))
        .collect();
    match check::check_modules(loaded, &sources) {
        Ok(mut checked) => {
            checked.sources = sources;
            Ok(checked)
        }
        Err(diagnostics) => Err((diagnostics, sources)),
    }
}

pub fn check_only(src: &Path) -> Result<(), BloomError> {
    let loaded = modules::load_program(src)?;
    check_loaded(loaded).map_err(|(diagnostics, sources)| BloomError::Diagnostics {
        diagnostics,
        sources,
    })?;
    Ok(())
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Result<TempDir, BloomError> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path =
            std::env::temp_dir().join(format!("{}-{}-{}", prefix, std::process::id(), nanos));
        fs::create_dir_all(&path).map_err(|e| {
            BloomError::Io(format!(
                "temporary directory `{}` could not be created: {}",
                path.display(),
                e
            ))
        })?;
        Ok(TempDir { path })
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Vendored SQLite amalgamation (see `runtime/sqlite3_NOTICE.md`). Only
/// written next to the generated C when the program loads `std::db`.
const SQLITE_C: &str = include_str!("../runtime/sqlite3.c");
const SQLITE_H: &str = include_str!("../runtime/sqlite3.h");

/// Vendored Argon2 reference implementation (CC0/Apache-2.0). Copied into
/// the build directory only for programs that load `std::auth`.
const ARGON2_FILES: &[(&str, &str)] = &[
    (
        "argon2/include/argon2.h",
        include_str!("../runtime/argon2/include/argon2.h"),
    ),
    (
        "argon2/src/argon2.c",
        include_str!("../runtime/argon2/src/argon2.c"),
    ),
    (
        "argon2/src/core.c",
        include_str!("../runtime/argon2/src/core.c"),
    ),
    (
        "argon2/src/core.h",
        include_str!("../runtime/argon2/src/core.h"),
    ),
    (
        "argon2/src/encoding.c",
        include_str!("../runtime/argon2/src/encoding.c"),
    ),
    (
        "argon2/src/encoding.h",
        include_str!("../runtime/argon2/src/encoding.h"),
    ),
    (
        "argon2/src/ref.c",
        include_str!("../runtime/argon2/src/ref.c"),
    ),
    (
        "argon2/src/thread.c",
        include_str!("../runtime/argon2/src/thread.c"),
    ),
    (
        "argon2/src/thread.h",
        include_str!("../runtime/argon2/src/thread.h"),
    ),
    (
        "argon2/src/blake2/blake2.h",
        include_str!("../runtime/argon2/src/blake2/blake2.h"),
    ),
    (
        "argon2/src/blake2/blake2b.c",
        include_str!("../runtime/argon2/src/blake2/blake2b.c"),
    ),
    (
        "argon2/src/blake2/blake2-impl.h",
        include_str!("../runtime/argon2/src/blake2/blake2-impl.h"),
    ),
    (
        "argon2/src/blake2/blamka-round-ref.h",
        include_str!("../runtime/argon2/src/blake2/blamka-round-ref.h"),
    ),
];
const ARGON2_SOURCES: &[&str] = &[
    "argon2/src/argon2.c",
    "argon2/src/core.c",
    "argon2/src/encoding.c",
    "argon2/src/ref.c",
    "argon2/src/thread.c",
    "argon2/src/blake2/blake2b.c",
];

struct Prepared {
    c_code: String,
    _tmp: TempDir,
    c_path: PathBuf,
    links: Vec<toolchain::LinkItem>,
    extra_sources: Vec<String>,
    defines: Vec<String>,
    include_dirs: Vec<String>,
    uses_desktop: bool,
}

/// Link inputs implied by loaded standard library modules.
fn auto_links(checked: &CheckedProgram) -> Vec<toolchain::LinkItem> {
    let mut libs: Vec<String> = Vec::new();
    for sig in &checked.functions {
        let needed: &[&str] = match sig.module.as_str() {
            "std::win32" => &["kernel32", "user32"],
            "std::net" | "std::http" => &["ws2_32"],
            "std::tls" => &["ws2_32", "secur32", "crypt32"],
            "std::web" if cfg!(windows) => &["bcrypt"],
            "std::web" => &[],
            "std::desktop" if cfg!(windows) => &["user32", "gdi32"],
            "std::desktop" => &[],
            _ => &[],
        };
        for lib in needed {
            if !libs.iter().any(|l| l == lib) {
                libs.push(lib.to_string());
            }
        }
    }
    libs.into_iter().map(toolchain::LinkItem::Lib).collect()
}

fn prepare(src: &Path, links: &[toolchain::LinkItem]) -> Result<Prepared, BloomError> {
    let loaded = modules::load_program(src)?;
    let checked =
        check_loaded(loaded).map_err(|(diagnostics, sources)| BloomError::Diagnostics {
            diagnostics,
            sources,
        })?;
    let c_code = codegen::emit(&checked);
    let tmp = TempDir::new("bloom")?;
    let c_path = tmp.path.join("program.c");
    let uses_db = checked.functions.iter().any(|sig| sig.module == "std::db");
    let uses_auth = checked
        .functions
        .iter()
        .any(|sig| sig.module == "std::auth");
    let uses_desktop = checked
        .functions
        .iter()
        .any(|sig| sig.module == "std::desktop");
    let mut extra_sources = Vec::new();
    let mut defines = Vec::new();
    let mut include_dirs = Vec::new();
    if uses_db {
        fs::write(tmp.path.join("sqlite3.c"), SQLITE_C).map_err(|e| {
            BloomError::Io(format!("SQLite amalgamation could not be written: {}", e))
        })?;
        fs::write(tmp.path.join("sqlite3.h"), SQLITE_H)
            .map_err(|e| BloomError::Io(format!("SQLite header could not be written: {}", e)))?;
        extra_sources.push("sqlite3.c".to_string());
        defines.extend(
            [
                "SQLITE_THREADSAFE=0",
                "SQLITE_OMIT_LOAD_EXTENSION",
                "SQLITE_DQS=0",
            ]
            .iter()
            .map(|s| s.to_string()),
        );
    }
    if uses_auth {
        for (rel, content) in ARGON2_FILES {
            let path = tmp.path.join(rel);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| {
                    BloomError::Io(format!("Argon2 directory could not be created: {}", e))
                })?;
            }
            fs::write(&path, content).map_err(|e| {
                BloomError::Io(format!(
                    "Argon2 source `{}` could not be written: {}",
                    rel, e
                ))
            })?;
        }
        extra_sources.extend(ARGON2_SOURCES.iter().map(|s| s.to_string()));
        defines.push("ARGON2_NO_THREADS".to_string());
        include_dirs.push("argon2/include".to_string());
    }
    fs::write(&c_path, &c_code).map_err(|e| {
        BloomError::Io(format!(
            "generated C code could not be written to `{}`: {}",
            c_path.display(),
            e
        ))
    })?;
    let mut all_links = auto_links(&checked);
    for item in links {
        let duplicate = all_links.iter().any(|existing| match (existing, item) {
            (toolchain::LinkItem::Lib(a), toolchain::LinkItem::Lib(b)) => a == b,
            (toolchain::LinkItem::Obj(a), toolchain::LinkItem::Obj(b)) => a == b,
            _ => false,
        });
        if !duplicate {
            all_links.push(item.clone());
        }
    }
    Ok(Prepared {
        c_code,
        _tmp: tmp,
        c_path,
        links: all_links,
        extra_sources,
        defines,
        include_dirs,
        uses_desktop,
    })
}

fn write_emit_c(prepared: &Prepared, emit_c: &Path) -> Result<(), BloomError> {
    if let Some(parent) = emit_c.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| {
                BloomError::Io(format!(
                    "directory `{}` could not be created: {}",
                    parent.display(),
                    e
                ))
            })?;
        }
    }
    fs::write(emit_c, &prepared.c_code).map_err(|e| {
        BloomError::Io(format!(
            "C code could not be written to `{}`: {}",
            emit_c.display(),
            e
        ))
    })?;
    if !prepared.extra_sources.is_empty() {
        let dir = emit_c.parent().unwrap_or_else(|| Path::new("."));
        fs::write(dir.join("sqlite3.c"), SQLITE_C).map_err(|e| {
            BloomError::Io(format!("SQLite amalgamation could not be written: {}", e))
        })?;
        fs::write(dir.join("sqlite3.h"), SQLITE_H)
            .map_err(|e| BloomError::Io(format!("SQLite header could not be written: {}", e)))?;
    }
    Ok(())
}

fn build_exe(prepared: &Prepared, force_console: bool) -> Result<PathBuf, BloomError> {
    let tool = toolchain::detect().ok_or_else(|| {
        BloomError::Toolchain(format!(
            "kein C-Compiler gefunden\n\n{}",
            toolchain::setup_help()
        ))
    })?;
    let workdir = prepared.c_path.parent().expect("temp dir");
    let exe = tool
        .compile(
            workdir,
            "program.c",
            toolchain::exe_name(),
            &prepared.links,
            &prepared.extra_sources,
            &prepared.defines,
            &prepared.include_dirs,
            prepared.uses_desktop && !force_console,
        )
        .map_err(|output| BloomError::NativeCompile {
            toolchain: tool.label(),
            output,
        })?;
    Ok(exe)
}

pub fn build(
    src: &Path,
    out: &Path,
    emit_c: Option<&Path>,
    links: &[toolchain::LinkItem],
    force_console: bool,
) -> Result<(), BloomError> {
    let prepared = prepare(src, links)?;
    if let Some(path) = emit_c {
        write_emit_c(&prepared, path)?;
    }
    let exe = build_exe(&prepared, force_console)?;
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| {
                BloomError::Io(format!(
                    "directory `{}` could not be created: {}",
                    parent.display(),
                    e
                ))
            })?;
        }
    }
    fs::copy(&exe, out).map_err(|e| {
        BloomError::Io(format!(
            "executable could not be written to `{}`: {}",
            out.display(),
            e
        ))
    })?;
    Ok(())
}

pub fn run(
    src: &Path,
    links: &[toolchain::LinkItem],
    force_console: bool,
) -> Result<i32, BloomError> {
    let prepared = prepare(src, links)?;
    let exe = build_exe(&prepared, force_console)?;
    let status = Command::new(&exe).status().map_err(|e| {
        BloomError::Io(format!(
            "program `{}` could not be started: {}",
            exe.display(),
            e
        ))
    })?;
    Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_simple_source() {
        let result = compile_source("fn main() -> int { println(\"hi\"); return 0; }");
        assert!(result.is_ok());
    }

    #[test]
    fn rejects_bad_source() {
        let result = compile_source("fn main() -> int { let x = ; return 0; }");
        assert!(result.is_err());
    }

    #[test]
    fn rejects_type_error() {
        let result = compile_source("fn main() -> int { let x: int = \"a\"; return 0; }");
        let diags = result.err().expect("expected type error");
        assert!(diags.iter().any(|d| d.code == codes::TYPE_MISMATCH));
    }
}
