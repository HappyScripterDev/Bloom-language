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

struct Prepared {
    c_code: String,
    _tmp: TempDir,
    c_path: PathBuf,
    links: Vec<toolchain::LinkItem>,
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
    })
}

fn build_exe(prepared: &Prepared) -> Result<PathBuf, BloomError> {
    let tool = toolchain::detect().ok_or_else(|| {
        BloomError::Toolchain(format!(
            "kein C-Compiler gefunden\n\n{}",
            toolchain::setup_help()
        ))
    })?;
    let workdir = prepared.c_path.parent().expect("temp dir");
    let exe = tool
        .compile(workdir, "program.c", toolchain::exe_name(), &prepared.links)
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
) -> Result<(), BloomError> {
    let prepared = prepare(src, links)?;
    if let Some(path) = emit_c {
        write_emit_c(&prepared, path)?;
    }
    let exe = build_exe(&prepared)?;
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

pub fn run(src: &Path, links: &[toolchain::LinkItem]) -> Result<i32, BloomError> {
    let prepared = prepare(src, links)?;
    let exe = build_exe(&prepared)?;
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
