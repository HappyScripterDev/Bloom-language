use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::ast::{Item, Program, UseDecl};
use crate::compiler::BloomError;
use crate::diagnostics::{codes, Diagnostic};
use crate::lexer::lex;
use crate::parser::parse_with_offset;

#[derive(Debug, Clone)]
pub enum ImportedItem {
    Module(String),
    Symbol(String),
}

#[derive(Debug, Clone)]
pub struct ImportEdge {
    pub local_name: String,
    pub target: ImportedItem,
    pub path_span: crate::token::Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuiltinModule {
    Math,
    Fs,
    Process,
    Time,
    Net,
    Tls,
    Http,
    Path,
    Json,
    Web,
    Html,
    Db,
    Auth,
    Ui,
    Win32,
}

/// Recognizes built-in (virtual) modules that have no source file.
pub fn builtin_module(path: &[String]) -> Option<(&'static str, BuiltinModule)> {
    let joined = path.join("::");
    match joined.as_str() {
        "std::math" => Some(("std::math", BuiltinModule::Math)),
        "std::fs" => Some(("std::fs", BuiltinModule::Fs)),
        "std::process" => Some(("std::process", BuiltinModule::Process)),
        "std::time" => Some(("std::time", BuiltinModule::Time)),
        "std::net" => Some(("std::net", BuiltinModule::Net)),
        "std::tls" => Some(("std::tls", BuiltinModule::Tls)),
        "std::http" => Some(("std::http", BuiltinModule::Http)),
        "std::path" => Some(("std::path", BuiltinModule::Path)),
        "std::json" => Some(("std::json", BuiltinModule::Json)),
        "std::web" => Some(("std::web", BuiltinModule::Web)),
        "std::html" => Some(("std::html", BuiltinModule::Html)),
        "std::db" => Some(("std::db", BuiltinModule::Db)),
        "std::auth" => Some(("std::auth", BuiltinModule::Auth)),
        "std::ui" => Some(("std::ui", BuiltinModule::Ui)),
        "std::win32" => Some(("std::win32", BuiltinModule::Win32)),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct LoadedModule {
    pub name: String,
    pub path: PathBuf,
    pub source: String,
    pub program: Program,
    pub imports: Vec<ImportEdge>,
    pub builtin: Option<BuiltinModule>,
}

#[derive(Debug, Clone)]
pub struct LoadedProgram {
    pub modules: Vec<LoadedModule>,
    pub entry_index: usize,
}

struct Loader {
    entry_dir: PathBuf,
    entry_path: PathBuf,
    modules: Vec<LoadedModule>,
    by_path: HashMap<PathBuf, usize>,
    by_name: HashMap<String, usize>,
    expr_cursor: u32,
    decl_cursor: u32,
}

pub fn load_program(entry: &Path) -> Result<LoadedProgram, BloomError> {
    let entry_abs = fs::canonicalize(entry).map_err(|e| {
        BloomError::Io(format!(
            "Datei `{}` konnte nicht geoeffnet werden: {}",
            entry.display(),
            e
        ))
    })?;
    let entry_dir = entry_abs
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let entry_name = entry_abs
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "main".to_string());
    let mut loader = Loader {
        entry_dir,
        entry_path: entry_abs.clone(),
        modules: Vec::new(),
        by_path: HashMap::new(),
        by_name: HashMap::new(),
        expr_cursor: 0,
        decl_cursor: 0,
    };
    let entry_span = crate::token::Span::point(1, 1);
    loader.load_file(&entry_abs, &entry_name, &entry_span)?;
    Ok(LoadedProgram {
        modules: loader.modules,
        entry_index: 0,
    })
}

impl Loader {
    fn load_file(
        &mut self,
        path: &Path,
        canonical_name: &str,
        import_span: &crate::token::Span,
    ) -> Result<usize, BloomError> {
        let abs = fs::canonicalize(path).map_err(|e| {
            BloomError::Io(format!(
                "Moduldatei `{}` konnte nicht geoeffnet werden: {}",
                path.display(),
                e
            ))
        })?;
        if let Some(&index) = self.by_path.get(&abs) {
            if abs == self.entry_path {
                let mut diag = Diagnostic::new(
                    codes::ENTRY_IMPORT,
                    "the entry module cannot be imported",
                    *import_span,
                );
                diag.help =
                    Some("move the shared code into its own module file instead".to_string());
                return Err(BloomError::Diagnostics {
                    diagnostics: vec![diag],
                    sources: Vec::new(),
                });
            }
            return Ok(index);
        }
        let source = fs::read(&abs).map_err(|e| {
            BloomError::Io(format!(
                "Moduldatei `{}` konnte nicht gelesen werden: {}",
                abs.display(),
                e
            ))
        })?;
        let source = String::from_utf8(source).map_err(|_| {
            BloomError::Io(format!(
                "Moduldatei `{}` ist nicht UTF-8-kodiert",
                abs.display()
            ))
        })?;
        let display = display_path(&self.entry_dir, &abs);
        let lexed = lex(&source);
        if !lexed.diagnostics.is_empty() {
            return Err(BloomError::Diagnostics {
                diagnostics: lexed.diagnostics,
                sources: vec![(display, source)],
            });
        }
        let parsed = parse_with_offset(lexed.tokens, self.expr_cursor, self.decl_cursor);
        if !parsed.diagnostics.is_empty() {
            return Err(BloomError::Diagnostics {
                diagnostics: parsed.diagnostics,
                sources: vec![(display, source)],
            });
        }
        self.expr_cursor = parsed.next_expr_id;
        self.decl_cursor = parsed.next_decl_id;
        let program = parsed.program;
        if let Some(&existing) = self.by_name.get(canonical_name) {
            let existing_path = self.modules[existing].path.clone();
            if existing_path != abs {
                let mut diag = Diagnostic::new(
                    codes::MODULE_NAME_CONFLICT,
                    format!(
                        "module name `{}` is already used by `{}`",
                        canonical_name,
                        display_path(&self.entry_dir, &existing_path)
                    ),
                    *import_span,
                );
                diag.help = Some("rename one of the module files".to_string());
                return Err(BloomError::Diagnostics {
                    diagnostics: vec![diag],
                    sources: Vec::new(),
                });
            }
        }
        let index = self.modules.len();
        self.modules.push(LoadedModule {
            name: canonical_name.to_string(),
            path: abs.clone(),
            source: source.clone(),
            program,
            imports: Vec::new(),
            builtin: None,
        });
        self.by_path.insert(abs.clone(), index);
        self.by_name.insert(canonical_name.to_string(), index);
        let uses: Vec<UseDecl> = self.modules[index]
            .program
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Use(u) => Some(u.clone()),
                _ => None,
            })
            .collect();
        let module_prefix = parent_prefix(canonical_name);
        let module_dir = abs
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| self.entry_dir.clone());
        let mut edges = Vec::new();
        for u in &uses {
            let edge = match self.resolve_use(u, &module_prefix, &module_dir) {
                Ok(edge) => edge,
                Err(BloomError::Diagnostics {
                    mut diagnostics,
                    sources,
                }) => {
                    let mut sources = sources;
                    if sources.is_empty() {
                        sources.push((display.clone(), source.clone()));
                    }
                    for d in &mut diagnostics {
                        if d.file.is_none() {
                            d.file = Some(display.clone());
                        }
                    }
                    return Err(BloomError::Diagnostics {
                        diagnostics,
                        sources,
                    });
                }
                Err(other) => return Err(other),
            };
            edges.push(edge);
        }
        self.modules[index].imports = edges;
        Ok(index)
    }

    fn resolve_use(
        &mut self,
        u: &UseDecl,
        module_prefix: &str,
        module_dir: &Path,
    ) -> Result<ImportEdge, BloomError> {
        let local_name = u.path.last().cloned().unwrap_or_default();
        let joined = u.path.join("::");
        let canonical_full = join_prefix(module_prefix, &joined);
        if let Some((canonical, builtin)) = builtin_module(&u.path) {
            let idx = self.load_builtin_module(canonical, builtin);
            let canonical = self.modules[idx].name.clone();
            return Ok(ImportEdge {
                local_name,
                target: ImportedItem::Module(canonical),
                path_span: u.path_span,
            });
        }
        if let Some(file) = find_module_file(module_dir, &u.path) {
            let idx = self.load_file(&file, &canonical_full, &u.path_span)?;
            let canonical = self.modules[idx].name.clone();
            return Ok(ImportEdge {
                local_name,
                target: ImportedItem::Module(canonical),
                path_span: u.path_span,
            });
        }
        if module_dir != self.entry_dir {
            if let Some(file) = find_module_file(&self.entry_dir, &u.path) {
                let idx = self.load_file(&file, &joined, &u.path_span)?;
                let canonical = self.modules[idx].name.clone();
                return Ok(ImportEdge {
                    local_name,
                    target: ImportedItem::Module(canonical),
                    path_span: u.path_span,
                });
            }
        }
        if u.path.len() >= 2 {
            let parent = &u.path[..u.path.len() - 1];
            if let Some((canonical, builtin)) = builtin_module(parent) {
                let idx = self.load_builtin_module(canonical, builtin);
                let parent_name = self.modules[idx].name.clone();
                return Ok(ImportEdge {
                    target: ImportedItem::Symbol(format!("{}::{}", parent_name, local_name)),
                    local_name,
                    path_span: u.path_span,
                });
            }
            let parent_dir_candidates: Vec<(PathBuf, String)> = {
                let mut v = vec![(
                    module_dir.to_path_buf(),
                    join_prefix(module_prefix, &parent.join("::")),
                )];
                if module_dir != self.entry_dir {
                    v.push((self.entry_dir.clone(), parent.join("::")));
                }
                v
            };
            for (dir, parent_canonical) in parent_dir_candidates {
                if let Some(file) = find_module_file(&dir, parent) {
                    let idx = self.load_file(&file, &parent_canonical, &u.path_span)?;
                    let parent_name = self.modules[idx].name.clone();
                    return Ok(ImportEdge {
                        target: ImportedItem::Symbol(format!("{}::{}", parent_name, local_name)),
                        local_name,
                        path_span: u.path_span,
                    });
                }
            }
        }
        let mut diag = Diagnostic::new(
            codes::MODULE_NOT_FOUND,
            format!("module `{}` not found", joined),
            u.path_span,
        );
        let candidate_a = module_file_candidate(module_dir, &u.path);
        let candidate_b = module_file_candidate(&self.entry_dir, &u.path);
        diag.help = Some(format!(
            "searched for `{}` and `{}`",
            candidate_a.display(),
            candidate_b.display()
        ));
        Err(BloomError::Diagnostics {
            diagnostics: vec![diag],
            sources: Vec::new(),
        })
    }
    fn load_builtin_module(&mut self, canonical_name: &str, builtin: BuiltinModule) -> usize {
        if let Some(&index) = self.by_name.get(canonical_name) {
            return index;
        }
        let index = self.modules.len();
        let path = PathBuf::from(format!("<builtin:{}>", canonical_name));
        self.modules.push(LoadedModule {
            name: canonical_name.to_string(),
            path: path.clone(),
            source: String::new(),
            program: Program { items: Vec::new() },
            imports: Vec::new(),
            builtin: Some(builtin),
        });
        self.by_path.insert(path, index);
        self.by_name.insert(canonical_name.to_string(), index);
        index
    }
}

fn parent_prefix(canonical: &str) -> String {
    match canonical.rfind("::") {
        Some(pos) => canonical[..pos].to_string(),
        None => String::new(),
    }
}

fn join_prefix(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{}::{}", prefix, name)
    }
}

pub fn module_file_candidate(dir: &Path, path: &[String]) -> PathBuf {
    let mut p = dir.to_path_buf();
    for (i, seg) in path.iter().enumerate() {
        if i + 1 == path.len() {
            p.push(format!("{}.blm", seg));
        } else {
            p.push(seg);
        }
    }
    p
}

fn mod_file_candidate(dir: &Path, path: &[String]) -> PathBuf {
    let mut p = dir.to_path_buf();
    for seg in path {
        p.push(seg);
    }
    p.push("mod.blm");
    p
}

fn find_module_file(dir: &Path, path: &[String]) -> Option<PathBuf> {
    if path.is_empty() {
        return None;
    }
    let a = module_file_candidate(dir, path);
    if a.is_file() {
        return Some(a);
    }
    let b = mod_file_candidate(dir, path);
    if b.is_file() {
        return Some(b);
    }
    None
}

pub fn display_path(entry_dir: &Path, abs: &Path) -> String {
    match abs.strip_prefix(entry_dir) {
        Ok(rel) => rel.to_string_lossy().to_string(),
        Err(_) => abs.to_string_lossy().to_string(),
    }
}

pub fn module_display_paths(modules: &[LoadedModule]) -> Vec<String> {
    let entry_dir = modules
        .first()
        .and_then(|m| m.path.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    modules
        .iter()
        .map(|m| display_path(&entry_dir, &m.path))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("bloom-mod-test-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn write(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent");
        }
        fs::write(&path, content).expect("write");
        path
    }

    #[test]
    fn loads_single_module() {
        let dir = temp_dir("single");
        let entry = write(&dir, "main.blm", "fn main() -> int { return 0; }\n");
        let loaded = load_program(&entry).expect("load");
        assert_eq!(loaded.modules.len(), 1);
        assert_eq!(loaded.entry_index, 0);
        assert_eq!(loaded.modules[0].name, "main");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn loads_imported_module() {
        let dir = temp_dir("import");
        let entry = write(
            &dir,
            "main.blm",
            "use helper;\nfn main() -> int { return 0; }\n",
        );
        write(&dir, "helper.blm", "pub fn f() -> int { return 1; }\n");
        let loaded = load_program(&entry).expect("load");
        assert_eq!(loaded.modules.len(), 2);
        assert_eq!(loaded.modules[1].name, "helper");
        assert!(matches!(
            loaded.modules[0].imports[0].target,
            ImportedItem::Module(ref c) if c == "helper"
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn loads_submodule_with_mod_blm() {
        let dir = temp_dir("submod");
        let entry = write(
            &dir,
            "main.blm",
            "use util;\nfn main() -> int { return 0; }\n",
        );
        write(&dir, "util/mod.blm", "pub fn f() -> int { return 1; }\n");
        let loaded = load_program(&entry).expect("load");
        assert_eq!(loaded.modules.len(), 2);
        assert_eq!(loaded.modules[1].name, "util");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn loads_nested_module_paths() {
        let dir = temp_dir("nested");
        let entry = write(
            &dir,
            "main.blm",
            "use net::client;\nfn main() -> int { return 0; }\n",
        );
        write(&dir, "net/client.blm", "pub fn f() -> int { return 1; }\n");
        let loaded = load_program(&entry).expect("load");
        assert_eq!(loaded.modules.len(), 2);
        assert_eq!(loaded.modules[1].name, "net::client");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn deduplicates_repeated_imports() {
        let dir = temp_dir("dedupe");
        let entry = write(
            &dir,
            "main.blm",
            "use a;\nuse b;\nfn main() -> int { return 0; }\n",
        );
        write(
            &dir,
            "a.blm",
            "use shared;\npub fn f() -> int { return 1; }\n",
        );
        write(
            &dir,
            "b.blm",
            "use shared;\npub fn g() -> int { return 2; }\n",
        );
        write(&dir, "shared.blm", "pub fn h() -> int { return 3; }\n");
        let loaded = load_program(&entry).expect("load");
        assert_eq!(loaded.modules.len(), 4);
        let names: Vec<String> = loaded.modules.iter().map(|m| m.name.clone()).collect();
        assert_eq!(names.iter().filter(|n| *n == "shared").count(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn supports_cyclic_imports() {
        let dir = temp_dir("cycle");
        let entry = write(&dir, "main.blm", "use a;\nfn main() -> int { return 0; }\n");
        write(&dir, "a.blm", "use b;\npub fn f() -> int { return 1; }\n");
        write(&dir, "b.blm", "use a;\npub fn g() -> int { return 2; }\n");
        let loaded = load_program(&entry).expect("load");
        assert_eq!(loaded.modules.len(), 3);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_missing_module() {
        let dir = temp_dir("missing");
        let entry = write(
            &dir,
            "main.blm",
            "use nowhere;\nfn main() -> int { return 0; }\n",
        );
        let err = load_program(&entry).expect_err("should fail");
        match err {
            BloomError::Diagnostics { diagnostics, .. } => {
                assert_eq!(diagnostics[0].code, codes::MODULE_NOT_FOUND);
            }
            other => panic!("unexpected error: {}", other),
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_entry_import() {
        let dir = temp_dir("entryimport");
        let entry = write(
            &dir,
            "main.blm",
            "use helper;\nfn main() -> int { return 0; }\n",
        );
        write(
            &dir,
            "helper.blm",
            "use main;\npub fn f() -> int { return 1; }\n",
        );
        let err = load_program(&entry).expect_err("should fail");
        match err {
            BloomError::Diagnostics { diagnostics, .. } => {
                assert_eq!(diagnostics[0].code, codes::ENTRY_IMPORT);
            }
            other => panic!("unexpected error: {}", other),
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn submodule_can_import_top_level_module() {
        let dir = temp_dir("fallback");
        let entry = write(
            &dir,
            "main.blm",
            "use net::client;\nfn main() -> int { return 0; }\n",
        );
        write(
            &dir,
            "net/client.blm",
            "use shapes;\npub fn f() -> int { return shapes::one(); }\n",
        );
        write(&dir, "shapes.blm", "pub fn one() -> int { return 1; }\n");
        let loaded = load_program(&entry).expect("load");
        assert_eq!(loaded.modules.len(), 3);
        let client = loaded
            .modules
            .iter()
            .find(|m| m.name == "net::client")
            .expect("client module");
        assert!(matches!(
            client.imports[0].target,
            ImportedItem::Module(ref c) if c == "shapes"
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn symbol_import_records_canonical_name() {
        let dir = temp_dir("symimport");
        let entry = write(
            &dir,
            "main.blm",
            "use helper::Point;\nfn main() -> int { return 0; }\n",
        );
        write(&dir, "helper.blm", "pub struct Point { x: int }\n");
        let loaded = load_program(&entry).expect("load");
        assert!(matches!(
            loaded.modules[0].imports[0].target,
            ImportedItem::Symbol(ref c) if c == "helper::Point"
        ));
        let _ = fs::remove_dir_all(&dir);
    }
}
