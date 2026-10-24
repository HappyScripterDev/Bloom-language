use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub enum Toolchain {
    Msvc { vcvars: PathBuf },
    GnuLike { exe: PathBuf, kind: GnuKind },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GnuKind {
    Clang,
    Gcc,
    Tcc,
}

/// One linker input: a system/user library by name, or an object/library
/// file by path. Library names are validated before they reach the linker.
#[derive(Debug, Clone)]
pub enum LinkItem {
    Lib(String),
    Obj(PathBuf),
}

impl LinkItem {
    /// Validates a library name from configuration or the CLI: only
    /// `[A-Za-z0-9_.-]+` is accepted (no paths, no shell metacharacters).
    pub fn parse_lib(value: &str) -> Result<LinkItem, String> {
        if value.is_empty() {
            return Err("empty library name".to_string());
        }
        if value.contains(['\\', '/', ':', '"', '\'', ' ', '\t']) {
            return Err(format!(
                "`{}` is not a valid library name (paths are not allowed; pass object files with `--link <pfad.obj>`)",
                value
            ));
        }
        if !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
        {
            return Err(format!("`{}` is not a valid library name", value));
        }
        Ok(LinkItem::Lib(value.to_string()))
    }

    /// Validates a CLI `--link` value: an existing file (C source, object or
    /// library) or a library name.
    pub fn parse_cli(value: &str) -> Result<LinkItem, String> {
        let looks_like_path = value.contains(['\\', '/'])
            || value.ends_with(".c")
            || value.ends_with(".obj")
            || value.ends_with(".o")
            || value.ends_with(".lib")
            || value.ends_with(".a");
        if looks_like_path {
            let path = PathBuf::from(value);
            if !path.is_file() {
                return Err(format!("`{}` is not an existing file", value));
            }
            return Ok(LinkItem::Obj(path));
        }
        LinkItem::parse_lib(value)
    }

    /// The argument passed to the linker for this item.
    fn msvc_arg(&self) -> String {
        match self {
            LinkItem::Lib(name) => {
                if name.ends_with(".lib") {
                    name.clone()
                } else {
                    format!("{}.lib", name)
                }
            }
            LinkItem::Obj(path) => path.display().to_string(),
        }
    }

    fn gnu_args(&self) -> Vec<String> {
        match self {
            LinkItem::Lib(name) => vec![format!("-l{}", name)],
            LinkItem::Obj(path) => vec![path.display().to_string()],
        }
    }
}

impl Toolchain {
    pub fn label(&self) -> String {
        match self {
            Toolchain::Msvc { .. } => "MSVC (cl.exe)".to_string(),
            Toolchain::GnuLike { exe, kind } => {
                let name = match kind {
                    GnuKind::Clang => "clang",
                    GnuKind::Gcc => "gcc",
                    GnuKind::Tcc => "tcc",
                };
                format!("{} ({})", name, exe.display())
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn compile(
        &self,
        workdir: &Path,
        c_name: &str,
        exe_name: &str,
        links: &[LinkItem],
        extra_sources: &[String],
        defines: &[String],
        include_dirs: &[String],
    ) -> Result<PathBuf, String> {
        match self {
            Toolchain::Msvc { vcvars, .. } => compile_msvc(
                vcvars,
                workdir,
                c_name,
                exe_name,
                links,
                extra_sources,
                defines,
                include_dirs,
            ),
            Toolchain::GnuLike { exe, kind } => compile_gnu(
                exe,
                *kind,
                workdir,
                c_name,
                exe_name,
                links,
                extra_sources,
                defines,
                include_dirs,
            ),
        }
    }
}

/// Quotes a path for the MSVC batch file. Only characters that are safe in
/// a `cmd.exe` command line are allowed; library names are validated by
/// `LinkItem` beforehand.
fn bat_arg(text: &str) -> Result<String, String> {
    if text.contains(['"', '%', '^', '&', '|', '<', '>', '\r', '\n']) {
        return Err(format!(
            "`{}` contains characters that are not allowed in linker arguments",
            text
        ));
    }
    if text.contains(' ') {
        Ok(format!("\"{}\"", text))
    } else {
        Ok(text.to_string())
    }
}

#[allow(clippy::too_many_arguments)]
fn compile_msvc(
    vcvars: &Path,
    workdir: &Path,
    c_name: &str,
    exe_name: &str,
    links: &[LinkItem],
    extra_sources: &[String],
    defines: &[String],
    include_dirs: &[String],
) -> Result<PathBuf, String> {
    let bat_path = workdir.join("bloom_build.bat");
    let mut link_args = String::new();
    for item in links {
        let arg = item.msvc_arg();
        link_args.push(' ');
        link_args.push_str(&bat_arg(&arg)?);
    }
    let mut sources = String::from(c_name);
    for source in extra_sources {
        sources.push(' ');
        sources.push_str(&bat_arg(source)?);
    }
    let mut flags = String::new();
    for define in defines {
        flags.push_str("/D");
        flags.push_str(&bat_arg(define)?);
        flags.push(' ');
    }
    for dir in include_dirs {
        flags.push_str("/I");
        flags.push_str(&bat_arg(dir)?);
        flags.push(' ');
    }
    let bat = format!(
        "@echo off\r\ncall \"{}\" >nul 2>&1\r\ncl /nologo /std:c17 /utf-8 /O2 {}{} /Fe:{} {}\r\n",
        vcvars.display(),
        flags,
        sources,
        exe_name,
        link_args
    );
    std::fs::write(&bat_path, bat).map_err(|e| format!("could not write build script: {}", e))?;
    let output = Command::new("cmd")
        .arg("/C")
        .arg("bloom_build.bat")
        .current_dir(workdir)
        .output()
        .map_err(|e| format!("could not start cmd.exe: {}", e))?;
    let exe = workdir.join(exe_name);
    if !output.status.success() || !exe.exists() {
        let mut text = String::new();
        text.push_str(&String::from_utf8_lossy(&output.stdout));
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        return Err(text.trim().to_string());
    }
    Ok(exe)
}

#[allow(clippy::too_many_arguments)]
fn compile_gnu(
    exe: &Path,
    kind: GnuKind,
    workdir: &Path,
    c_name: &str,
    exe_name: &str,
    links: &[LinkItem],
    extra_sources: &[String],
    defines: &[String],
    include_dirs: &[String],
) -> Result<PathBuf, String> {
    let mut cmd = Command::new(exe);
    cmd.current_dir(workdir);
    for define in defines {
        cmd.arg(format!("-D{}", define));
    }
    for dir in include_dirs {
        cmd.arg(format!("-I{}", dir));
    }
    if kind == GnuKind::Tcc {
        cmd.args(["-o", exe_name, c_name]);
    } else {
        cmd.args(["-O2", "-o", exe_name, c_name, "-lm"]);
    }
    for source in extra_sources {
        cmd.arg(source);
    }
    for item in links {
        for arg in item.gnu_args() {
            cmd.arg(arg);
        }
    }
    let output = cmd
        .output()
        .map_err(|e| format!("could not start {}: {}", exe.display(), e))?;
    let exe_path = workdir.join(exe_name);
    if !output.status.success() || !exe_path.exists() {
        let mut text = String::new();
        text.push_str(&String::from_utf8_lossy(&output.stdout));
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        return Err(text.trim().to_string());
    }
    Ok(exe_path)
}

pub fn exe_name() -> &'static str {
    if cfg!(windows) {
        "program.exe"
    } else {
        "program"
    }
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let output = Command::new("where.exe").arg(name).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let first = text.lines().next()?.trim().to_string();
    if first.is_empty() {
        None
    } else {
        let path = PathBuf::from(first);
        if path.exists() {
            Some(path)
        } else {
            None
        }
    }
}

fn find_vswhere() -> Option<PathBuf> {
    let candidates = [
        r"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe",
        r"C:\Program Files\Microsoft Visual Studio\Installer\vswhere.exe",
    ];
    for c in candidates {
        let p = PathBuf::from(c);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn find_msvc() -> Option<Toolchain> {
    let vswhere = find_vswhere()?;
    let output = Command::new(&vswhere)
        .args([
            "-latest",
            "-products",
            "*",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-property",
            "installationPath",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let install = text.lines().map(|l| l.trim()).find(|l| !l.is_empty())?;
    let vcvars = PathBuf::from(install).join("VC/Auxiliary/Build/vcvars64.bat");
    if vcvars.exists() {
        Some(Toolchain::Msvc { vcvars })
    } else {
        None
    }
}

fn kind_for_name(name: &str) -> GnuKind {
    let lower = name.to_ascii_lowercase();
    if lower.contains("clang") {
        GnuKind::Clang
    } else if lower.contains("tcc") {
        GnuKind::Tcc
    } else {
        GnuKind::Gcc
    }
}

fn from_override(value: &str) -> Option<Toolchain> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    let file_name = Path::new(trimmed)
        .file_name()
        .map(|f| f.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if lower == "cl" || file_name == "cl.exe" {
        return find_msvc();
    }
    let exe = if trimmed.contains('\\') || trimmed.contains('/') {
        let p = PathBuf::from(trimmed);
        if p.exists() {
            p
        } else {
            return None;
        }
    } else {
        find_on_path(trimmed)?
    };
    let kind = kind_for_name(&file_name);
    Some(Toolchain::GnuLike { exe, kind })
}

pub fn detect() -> Option<Toolchain> {
    if let Ok(value) = std::env::var("BLOOM_CC") {
        if let Some(tc) = from_override(&value) {
            return Some(tc);
        }
    }
    if let Some(tc) = find_msvc() {
        return Some(tc);
    }
    for name in ["clang", "gcc", "tcc"] {
        if let Some(exe) = find_on_path(name) {
            let kind = kind_for_name(name);
            return Some(Toolchain::GnuLike { exe, kind });
        }
    }
    None
}

pub fn setup_help() -> String {
    let mut s = String::new();
    s.push_str(
        "Bloom benoetigt einen C-Compiler als Backend (die aktuelle Version erzeugt C-Code).\n",
    );
    s.push_str("Optionen:\n");
    s.push_str(
        "  1. Visual Studio Build Tools mit \"Desktop development with C++\" installieren\n",
    );
    s.push_str("     (https://visualstudio.microsoft.com/visual-cpp-build-tools/)\n");
    s.push_str("  2. Clang installieren (https://releases.llvm.org/ oder LLVM-Builds)\n");
    s.push_str("  3. MinGW-w64 GCC installieren und in den PATH aufnehmen\n");
    s.push_str("  4. Tiny C Compiler (tcc) installieren\n");
    s.push_str("Alternativ die Umgebungsvariable BLOOM_CC auf den Compiler setzen, z.B.\n");
    s.push_str("  set BLOOM_CC=clang   oder   set BLOOM_CC=C:\\pfad\\zu\\cl.exe");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_detection_works() {
        assert_eq!(kind_for_name("clang.exe"), GnuKind::Clang);
        assert_eq!(kind_for_name("gcc.exe"), GnuKind::Gcc);
        assert_eq!(kind_for_name("tcc.exe"), GnuKind::Tcc);
        assert_eq!(kind_for_name("gcc-13"), GnuKind::Gcc);
    }

    #[test]
    fn exe_name_has_extension_on_windows() {
        if cfg!(windows) {
            assert!(exe_name().ends_with(".exe"));
        }
    }
}
