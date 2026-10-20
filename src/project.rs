use std::fs;
use std::path::{Path, PathBuf};

use crate::toolchain::LinkItem;

#[derive(Debug, Clone)]
pub struct Project {
    pub name: String,
    pub entry: PathBuf,
    pub root: PathBuf,
    pub link_libraries: Vec<String>,
}

pub fn parse_manifest(text: &str) -> Result<(String, String, String, Vec<String>), String> {
    let mut section = String::new();
    let mut name = None;
    let mut version = None;
    let mut entry = None;
    let mut link_libraries: Vec<String> = Vec::new();
    for (idx, raw) in text.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            if !line.ends_with(']') {
                return Err(format!("line {}: unterminated section header", line_no));
            }
            section = line[1..line.len() - 1].trim().to_string();
            continue;
        }
        let Some(eq) = line.find('=') else {
            return Err(format!("line {}: expected `key = value`", line_no));
        };
        let key = line[..eq].trim();
        let value = line[eq + 1..].trim();
        if section == "build" && key == "link_libraries" {
            link_libraries = parse_string_array(value)
                .map_err(|e| format!("line {}: link_libraries: {}", line_no, e))?;
            continue;
        }
        let value = if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
            value[1..value.len() - 1].to_string()
        } else {
            value.to_string()
        };
        if section == "package" {
            match key {
                "name" => name = Some(value),
                "version" => version = Some(value),
                "entry" => entry = Some(value),
                _ => {}
            }
        }
    }
    let name = name.ok_or_else(|| "missing `name` in [package]".to_string())?;
    let version = version.unwrap_or_else(|| "0.5.0".to_string());
    let entry = entry.unwrap_or_else(|| "src/main.blm".to_string());
    for lib in &link_libraries {
        LinkItem::parse_lib(lib).map_err(|e| format!("link_libraries: {}", e))?;
    }
    Ok((name, version, entry, link_libraries))
}

/// Parses a TOML-style inline array of strings: `["a", "b"]`.
fn parse_string_array(value: &str) -> Result<Vec<String>, String> {
    let inner = value
        .strip_prefix('[')
        .and_then(|v| v.strip_suffix(']'))
        .ok_or_else(|| "expected an array like [\"name\"]".to_string())?;
    let mut items = Vec::new();
    for part in inner.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let text = part
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .ok_or_else(|| format!("`{}` is not a quoted string", part))?;
        items.push(text.to_string());
    }
    Ok(items)
}

pub fn find_project(start: &Path) -> Result<Option<Project>, String> {
    let manifest = start.join("bloom.toml");
    if !manifest.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&manifest).map_err(|e| {
        format!(
            "`{}` konnte nicht gelesen werden: {}",
            manifest.display(),
            e
        )
    })?;
    let (name, _version, entry, link_libraries) =
        parse_manifest(&text).map_err(|e| format!("`{}`: {}", manifest.display(), e))?;
    Ok(Some(Project {
        name,
        entry: start.join(entry),
        root: start.to_path_buf(),
        link_libraries,
    }))
}

fn default_main(name: &str) -> String {
    format!(
        "fn main() -> int {{\n    println(\"Hello from {}!\");\n    println(\"Dieses Projekt wurde mit Bloom erstellt.\");\n    return 0;\n}}\n",
        name
    )
}

fn default_manifest(name: &str) -> String {
    format!(
        "[package]\nname = \"{}\"\nversion = \"0.5.0\"\nentry = \"src/main.blm\"\n",
        name
    )
}

fn default_readme(name: &str) -> String {
    format!(
        "# {}\n\nEin Bloom-Projekt.\n\n## Bauen und starten\n\n```bash\nbloom run\nbloom build\n```\n\nDer Standard-Einstiegspunkt ist `src/main.blm` (konfiguriert in `bloom.toml`).\n",
        name
    )
}

pub fn init_project(dir: &Path, name: &str) -> Result<(), String> {
    if dir.exists() {
        let is_empty = match fs::read_dir(dir) {
            Ok(mut entries) => entries.next().is_none(),
            Err(_) => false,
        };
        if !is_empty {
            return Err(format!(
                "directory `{}` exists and is not empty",
                dir.display()
            ));
        }
    }
    fs::create_dir_all(dir.join("src"))
        .map_err(|e| format!("could not create `{}`: {}", dir.display(), e))?;
    fs::write(dir.join("bloom.toml"), default_manifest(name))
        .map_err(|e| format!("could not write bloom.toml: {}", e))?;
    fs::write(dir.join("src/main.blm"), default_main(name))
        .map_err(|e| format!("could not write src/main.blm: {}", e))?;
    fs::write(dir.join("README.md"), default_readme(name))
        .map_err(|e| format!("could not write README.md: {}", e))?;
    fs::write(dir.join(".gitignore"), "build/\n*.exe\n")
        .map_err(|e| format!("could not write .gitignore: {}", e))?;
    Ok(())
}

pub fn valid_project_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_manifest() {
        let text = "[package]\nname = \"Hello\"\nversion = \"1.2.3\"\nentry = \"src/main.blm\"\n";
        let (name, version, entry, _libs) = parse_manifest(text).unwrap();
        assert_eq!(name, "Hello");
        assert_eq!(version, "1.2.3");
        assert_eq!(entry, "src/main.blm");
    }

    #[test]
    fn manifest_defaults_entry() {
        let text = "[package]\nname = \"Hello\"\n";
        let (name, _, entry, _libs) = parse_manifest(text).unwrap();
        assert_eq!(name, "Hello");
        assert_eq!(entry, "src/main.blm");
    }

    #[test]
    fn manifest_requires_name() {
        assert!(parse_manifest("[package]\nversion = \"1.0\"\n").is_err());
    }

    #[test]
    fn validates_project_names() {
        assert!(valid_project_name("HelloWorld"));
        assert!(valid_project_name("my-app_2"));
        assert!(!valid_project_name(""));
        assert!(!valid_project_name("bad name"));
    }

    #[test]
    fn parses_link_libraries() {
        let text = "[package]\nname = \"x\"\nversion = \"1.0.0\"\nentry = \"src/main.blm\"\n\n[build]\nlink_libraries = [\"kernel32\", \"user32\"]\n";
        let (name, _, _, libs) = parse_manifest(text).expect("ok");
        assert_eq!(name, "x");
        assert_eq!(libs, vec!["kernel32".to_string(), "user32".to_string()]);
        let bad = "[package]\nname = \"x\"\n\n[build]\nlink_libraries = [\"../evil\"]\n";
        assert!(parse_manifest(bad).is_err());
    }
}
