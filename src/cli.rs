use std::path::{Path, PathBuf};

use crate::compiler;
use crate::diagnostics::codes;
use crate::project;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn print_version() {
    println!("bloom {}", VERSION);
}

fn print_help() {
    println!(
        "Bloom – Compiler fuer die Programmiersprache Bloom (Version {})

VERWENDUNG:
    bloom <kommando> [optionen]

KOMMANDOS:
    build [datei.blm] [optionen]   Bloom-Quellcode zu einem nativen Programm kompilieren
    run [datei.blm]                Kompilieren und das Programm ausfuehren
    check [datei.blm]              Nur pruefen (Lexer, Parser, Typpruefung)
    init <name>                    Ein neues Bloom-Projekt anlegen
    fmt [--check] <datei|ordner>   Bloom-Quellcode formatieren (--check: nur pruefen)

OPTIONEN FUER build:
    -o <pfad>            Ausgabedatei (Standard: neben dem Quellcode oder build/<name>.exe)
    --emit-c <pfad>      Den generierten C-Code zusaetzlich an dieser Stelle speichern
    --console            Konsolen-Subsystem erzwingen (auch fuer std::desktop)

WEITERE OPTIONEN:
    -h, --help           Diese Hilfe anzeigen
    -V, --version        Version anzeigen

Ohne Dateiangabe sucht bloom im aktuellen Verzeichnis nach `bloom.toml`
und verwendet den dort konfigurierten Einstiegspunkt (Standard: src/main.blm).

Das Backend erzeugt C-Code und uebersetzt ihn mit MSVC, Clang, GCC oder tcc.
Die Auswahl des C-Compilers kann mit der Umgebungsvariable BLOOM_CC erfolgen.",
        VERSION
    );
}

fn usage_error(message: &str) -> i32 {
    eprintln!("error[{}]: {}", codes::USAGE, message);
    eprintln!("Hinweis: `bloom --help` zeigt die verfuegbaren Kommandos und Optionen.");
    2
}

fn resolve_input(
    file: Option<&str>,
    build_default_dir: bool,
) -> Result<(PathBuf, PathBuf, Vec<crate::toolchain::LinkItem>), i32> {
    let cwd = match std::env::current_dir() {
        Ok(p) => p,
        Err(e) => {
            eprintln!(
                "error[{}]: aktuelles Verzeichnis nicht verfuegbar: {}",
                codes::IO,
                e
            );
            return Err(5);
        }
    };
    match file {
        Some(f) => {
            let src = PathBuf::from(f);
            if !src.exists() {
                eprintln!(
                    "error[{}]: Datei `{}` nicht gefunden",
                    codes::IO,
                    src.display()
                );
                return Err(5);
            }
            if src.is_dir() {
                eprintln!(
                    "error[{}]: `{}` ist ein Verzeichnis; bitte eine .blm-Datei angeben",
                    codes::IO,
                    src.display()
                );
                return Err(2);
            }
            let parent = src.parent().unwrap_or_else(|| Path::new("."));
            let stem = src
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "program".to_string());
            let default_out = parent.join(exe_file_name(&stem));
            Ok((src, default_out, Vec::new()))
        }
        None => match project::find_project(&cwd) {
            Ok(Some(p)) => {
                if !p.entry.exists() {
                    eprintln!(
                        "error[{}]: Einstiegspunkt `{}` aus bloom.toml existiert nicht",
                        codes::IO,
                        p.entry.display()
                    );
                    return Err(5);
                }
                let out = if build_default_dir {
                    p.root.join("build").join(exe_file_name(&p.name))
                } else {
                    p.entry.with_extension(exe_ext())
                };
                let mut links = Vec::new();
                for lib in &p.link_libraries {
                    match crate::toolchain::LinkItem::parse_lib(lib) {
                        Ok(item) => links.push(item),
                        Err(message) => {
                            eprintln!("error[{}]: {}", codes::BUILD_CONFIG, message);
                            return Err(5);
                        }
                    }
                }
                Ok((p.entry, out, links))
            }
            Ok(None) => Err(usage_error(
                "keine Eingabedatei angegeben und keine bloom.toml im aktuellen Verzeichnis gefunden",
            )),
            Err(message) => {
                eprintln!("error[{}]: {}", codes::IO, message);
                Err(5)
            }
        },
    }
}

fn exe_ext() -> &'static str {
    if cfg!(windows) {
        "exe"
    } else {
        ""
    }
}

fn exe_file_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{}.exe", stem)
    } else {
        stem.to_string()
    }
}

fn cmd_build(args: &[String]) -> i32 {
    let mut file: Option<String> = None;
    let mut output: Option<String> = None;
    let mut emit_c: Option<String> = None;
    let mut link_values: Vec<String> = Vec::new();
    let mut force_console = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => {
                if i + 1 >= args.len() {
                    return usage_error("`-o` erwartet einen Pfad");
                }
                output = Some(args[i + 1].clone());
                i += 2;
            }
            "--emit-c" => {
                if i + 1 >= args.len() {
                    return usage_error("`--emit-c` erwartet einen Pfad");
                }
                emit_c = Some(args[i + 1].clone());
                i += 2;
            }
            "--link" => {
                if i + 1 >= args.len() {
                    return usage_error("`--link` erwartet eine Bibliothek oder Datei");
                }
                link_values.push(args[i + 1].clone());
                i += 2;
            }
            "--console" => {
                force_console = true;
                i += 1;
            }
            "-h" | "--help" => {
                print_help();
                return 0;
            }
            s if s.starts_with('-') => {
                return usage_error(&format!("unbekannte Option `{}` fuer `bloom build`", s));
            }
            s => {
                if file.is_some() {
                    return usage_error("zu viele Dateiargumente fuer `bloom build`");
                }
                file = Some(s.to_string());
                i += 1;
            }
        }
    }
    let (src, default_out, mut links) = match resolve_input(file.as_deref(), true) {
        Ok(v) => v,
        Err(code) => return code,
    };
    for value in &link_values {
        match crate::toolchain::LinkItem::parse_cli(value) {
            Ok(item) => links.push(item),
            Err(message) => {
                eprintln!("error[{}]: {}", codes::BUILD_CONFIG, message);
                return 5;
            }
        }
    }
    let out = output.map(PathBuf::from).unwrap_or(default_out);
    let emit_c_path = emit_c.map(PathBuf::from);
    match compiler::build(&src, &out, emit_c_path.as_deref(), &links, force_console) {
        Ok(()) => {
            if let Some(p) = &emit_c_path {
                println!("C-Code geschrieben: {}", p.display());
            }
            println!("Erstellt: {}", out.display());
            0
        }
        Err(e) => {
            e.report();
            e.exit_code()
        }
    }
}

fn cmd_run(args: &[String]) -> i32 {
    let mut link_values: Vec<String> = Vec::new();
    let mut file: Option<String> = None;
    let mut force_console = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--link" => {
                if i + 1 >= args.len() {
                    return usage_error("`--link` erwartet eine Bibliothek oder Datei");
                }
                link_values.push(args[i + 1].clone());
                i += 2;
            }
            "--console" => {
                force_console = true;
                i += 1;
            }
            "-h" | "--help" => {
                print_help();
                return 0;
            }
            s if s.starts_with('-') => {
                return usage_error(&format!("unbekannte Option `{}` fuer `bloom run`", s));
            }
            s => {
                if file.is_some() {
                    return usage_error("zu viele Dateiargumente fuer `bloom run`");
                }
                file = Some(s.to_string());
                i += 1;
            }
        }
    }
    let (src, _, mut links) = match resolve_input(file.as_deref(), false) {
        Ok(v) => v,
        Err(code) => return code,
    };
    for value in &link_values {
        match crate::toolchain::LinkItem::parse_cli(value) {
            Ok(item) => links.push(item),
            Err(message) => {
                eprintln!("error[{}]: {}", codes::BUILD_CONFIG, message);
                return 5;
            }
        }
    }
    match compiler::run(&src, &links, force_console) {
        Ok(code) => code,
        Err(e) => {
            e.report();
            e.exit_code()
        }
    }
}

fn cmd_check(args: &[String]) -> i32 {
    let (file, help) = parse_single_file_args(args, "check");
    if help {
        print_help();
        return 0;
    }
    let (src, _, _) = match resolve_input(file.as_deref(), false) {
        Ok(v) => v,
        Err(code) => return code,
    };
    match compiler::check_only(&src) {
        Ok(()) => {
            println!(
                "OK: {} (Lexer, Parser und Typpruefung ohne Fehler)",
                src.display()
            );
            0
        }
        Err(e) => {
            e.report();
            e.exit_code()
        }
    }
}

fn parse_single_file_args(args: &[String], command: &str) -> (Option<String>, bool) {
    let mut file = None;
    for a in args {
        match a.as_str() {
            "-h" | "--help" => return (None, true),
            s if s.starts_with('-') => {
                eprintln!(
                    "error[{}]: unbekannte Option `{}` fuer `bloom {}`",
                    codes::USAGE,
                    s,
                    command
                );
                eprintln!("Hinweis: `bloom --help` zeigt die verfuegbaren Optionen.");
                std::process::exit(2);
            }
            s => {
                if file.is_some() {
                    eprintln!(
                        "error[{}]: zu viele Dateiargumente fuer `bloom {}`",
                        codes::USAGE,
                        command
                    );
                    std::process::exit(2);
                }
                file = Some(s.to_string());
            }
        }
    }
    (file, false)
}

fn cmd_init(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_help();
        return 0;
    }
    let name = match args.first() {
        Some(n) => n.clone(),
        None => return usage_error("`bloom init` erwartet einen Projektnamen"),
    };
    if args.len() > 1 {
        return usage_error("`bloom init` erwartet genau einen Projektnamen");
    }
    if !project::valid_project_name(&name) {
        return usage_error("ungueltiger Projektname (erlaubt: Buchstaben, Ziffern, `_` und `-`)");
    }
    let cwd = match std::env::current_dir() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error[{}]: {}", codes::IO, e);
            return 5;
        }
    };
    let dir = cwd.join(&name);
    match project::init_project(&dir, &name) {
        Ok(()) => {
            println!("Projekt erstellt: {}", dir.display());
            println!("Naechste Schritte:");
            println!("    cd {}", name);
            println!("    bloom run");
            0
        }
        Err(message) => {
            eprintln!("error[{}]: {}", codes::IO, message);
            5
        }
    }
}

fn cmd_fmt(args: &[String]) -> i32 {
    let mut check_only = false;
    let mut target: Option<String> = None;
    for a in args {
        match a.as_str() {
            "-h" | "--help" => {
                print_help();
                return 0;
            }
            "--check" => check_only = true,
            s if s.starts_with('-') => {
                return usage_error(&format!("unbekannte Option `{}` fuer `bloom fmt`", s));
            }
            s => {
                if target.is_some() {
                    return usage_error(
                        "`bloom fmt` erwartet genau ein Datei- oder Ordnerargument",
                    );
                }
                target = Some(s.to_string());
            }
        }
    }
    let Some(target) = target else {
        return usage_error("`bloom fmt` erwartet eine Datei oder einen Ordner");
    };
    let path = PathBuf::from(&target);
    let mut files = Vec::new();
    if path.is_dir() {
        collect_bloom_files(&path, &mut files);
        files.sort();
    } else if path.is_file() {
        files.push(path.clone());
    } else {
        eprintln!(
            "error[{}]: `{}` wurde nicht gefunden",
            codes::IO,
            path.display()
        );
        return 5;
    }
    if files.is_empty() {
        println!("Keine .blm-Dateien in {} gefunden.", path.display());
        return 0;
    }
    let mut changed = 0usize;
    let mut failed = 0usize;
    for file in &files {
        let source = match std::fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error[{}]: {}: {}", codes::IO, file.display(), e);
                failed += 1;
                continue;
            }
        };
        let formatted = match crate::fmt::format_and_verify(&source) {
            Ok(f) => f,
            Err(diags) => {
                eprintln!(
                    "error[{}]: {} konnte nicht formatiert werden",
                    codes::UNEXPECTED_TOKEN,
                    file.display()
                );
                for d in &diags {
                    eprintln!("  {}", d.message);
                }
                failed += 1;
                continue;
            }
        };
        if formatted == source {
            continue;
        }
        changed += 1;
        if check_only {
            println!("Nicht formatiert: {}", file.display());
        } else {
            match write_formatted(file, &formatted) {
                Ok(()) => println!("Formatiert: {}", file.display()),
                Err(e) => {
                    eprintln!("error[{}]: {}: {}", codes::IO, file.display(), e);
                    failed += 1;
                }
            }
        }
    }
    if failed > 0 {
        return 1;
    }
    if check_only && changed > 0 {
        println!(
            "{} von {} Datei(en) sind nicht formatiert.",
            changed,
            files.len()
        );
        return 1;
    }
    if check_only {
        println!("Alle {} Datei(en) sind formatiert.", files.len());
    } else {
        println!("{} von {} Datei(en) formatiert.", changed, files.len());
    }
    0
}

/// Collects `.blm` files recursively, skipping hidden entries, `target` and
/// following no symlinked directories beyond their listing (plain entries).
fn collect_bloom_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "target" {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            collect_bloom_files(&path, out);
        } else if kind.is_file() && path.extension().map(|e| e == "blm").unwrap_or(false) {
            out.push(path);
        }
    }
}

/// Writes formatted source safely: a temporary file next to the target is
/// written and flushed first, then moved over the target (with a documented
/// fallback because Windows does not replace on plain rename).
fn write_formatted(file: &Path, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    let dir = file.parent().unwrap_or_else(|| Path::new("."));
    let file_name = file
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "out.blm".to_string());
    let tmp = dir.join(format!(".{}.bloomfmt.tmp", file_name));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
    }
    match std::fs::rename(&tmp, file) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Windows: rename does not replace an existing file.
            if std::fs::remove_file(file).is_err() {
                let _ = std::fs::remove_file(&tmp);
                return Err(std::io::Error::other(
                    "Zieldatei konnte nicht ersetzt werden",
                ));
            }
            match std::fs::rename(&tmp, file) {
                Ok(()) => Ok(()),
                Err(e) => {
                    let _ = std::fs::remove_file(&tmp);
                    Err(e)
                }
            }
        }
    }
}

pub fn run(args: Vec<String>) -> i32 {
    if args.is_empty() {
        print_help();
        return 0;
    }
    match args[0].as_str() {
        "-h" | "--help" | "help" => {
            print_help();
            0
        }
        "-V" | "--version" | "version" => {
            print_version();
            0
        }
        "build" => cmd_build(&args[1..]),
        "run" => cmd_run(&args[1..]),
        "check" => cmd_check(&args[1..]),
        "init" => cmd_init(&args[1..]),
        "fmt" => cmd_fmt(&args[1..]),
        other => usage_error(&format!("unbekanntes Kommando `{}`", other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_names() {
        let n = exe_file_name("hello");
        if cfg!(windows) {
            assert_eq!(n, "hello.exe");
        } else {
            assert_eq!(n, "hello");
        }
    }
}
