use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use toml_edit::{DocumentMut, Item, Table, value};

pub const PIN_FILE: &str = "linguo.toml";
pub const GLOBAL_CONFIG: &str = "config.toml";

/// Root of linguo's state: `$LINGUO_ROOT` or `~/.linguo`.
pub fn linguo_root() -> Result<PathBuf> {
    if let Some(root) = std::env::var_os("LINGUO_ROOT") {
        return Ok(PathBuf::from(root));
    }
    let home = dirs::home_dir().context("could not determine home directory")?;
    Ok(home.join(".linguo"))
}

pub fn toolchains_dir(language: &str) -> Result<PathBuf> {
    Ok(linguo_root()?.join("toolchains").join(language))
}

/// Where a resolved pin came from, for display purposes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinSource {
    Project(PathBuf),
    Global,
}

/// A pin as written in a pin file. The value is uninterpreted here: most
/// languages parse it as a version request, but backends may support richer
/// specs (e.g. terraform's `opentofu@1.10`).
#[derive(Debug, Clone)]
pub struct Pin {
    pub raw: String,
    pub source: PinSource,
}

/// Walk up from `start` looking for a `linguo.toml`.
pub fn find_pin_file(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|dir| dir.join(PIN_FILE))
        .find(|candidate| candidate.is_file())
}

fn read_pin_from(path: &Path, language: &str) -> Result<Option<String>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let doc: DocumentMut = text
        .parse()
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(doc
        .get("runtimes")
        .and_then(|t| t.get(language))
        .and_then(|v| v.as_str())
        .map(str::to_string))
}

/// The pin from the nearest project `linguo.toml`, if any covers `language`.
pub fn project_pin(language: &str, cwd: &Path) -> Result<Option<Pin>> {
    if let Some(path) = find_pin_file(cwd)
        && let Some(raw) = read_pin_from(&path, language)?
    {
        return Ok(Some(Pin {
            raw,
            source: PinSource::Project(path),
        }));
    }
    Ok(None)
}

/// Whether the shell hook may install unsatisfied pins on activation.
/// Opt-in via `[settings] auto-install = true` in the global config only —
/// machine-level consent, so a cloned repo can't trigger downloads by itself.
pub fn auto_install_enabled() -> Result<bool> {
    let global = linguo_root()?.join(GLOBAL_CONFIG);
    let Ok(text) = std::fs::read_to_string(&global) else {
        return Ok(false);
    };
    let doc: DocumentMut = text
        .parse()
        .with_context(|| format!("failed to parse {}", global.display()))?;
    Ok(doc
        .get("settings")
        .and_then(|s| s.get("auto-install"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false))
}

/// The pin from the global config, if any covers `language`.
pub fn global_pin(language: &str) -> Result<Option<Pin>> {
    let global = linguo_root()?.join(GLOBAL_CONFIG);
    if global.is_file()
        && let Some(raw) = read_pin_from(&global, language)?
    {
        return Ok(Some(Pin {
            raw,
            source: PinSource::Global,
        }));
    }
    Ok(None)
}

/// Resolve the pin for `language`: nearest project `linguo.toml` first, then
/// the global config. (Languages with ecosystem pin-file fallbacks resolve
/// through `store::resolve_pin`, which slots the fallback between the two.)
pub fn resolve_pin(language: &str, cwd: &Path) -> Result<Option<Pin>> {
    if let Some(pin) = project_pin(language, cwd)? {
        return Ok(Some(pin));
    }
    global_pin(language)
}

/// The JVM binding for a JVM-based language: nearest project linguo.toml's
/// `[jvm] <language> = "..."`, then the global config's.
pub fn jvm_binding(language: &str, cwd: &Path) -> Result<Option<String>> {
    let read = |path: &Path| -> Result<Option<String>> {
        if !path.is_file() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let doc: DocumentMut = text
            .parse()
            .with_context(|| format!("failed to parse {}", path.display()))?;
        Ok(doc
            .get("jvm")
            .and_then(|t| t.get(language))
            .and_then(|v| v.as_str())
            .map(str::to_string))
    };
    for dir in cwd.ancestors() {
        let candidate = dir.join(PIN_FILE);
        if candidate.is_file()
            && let Some(binding) = read(&candidate)?
        {
            return Ok(Some(binding));
        }
    }
    read(&linguo_root()?.join(GLOBAL_CONFIG))
}

/// Set `[jvm] <language> = "<value>"` in `path`, creating the file if needed.
pub fn write_jvm_binding(path: &Path, language: &str, raw: &str) -> Result<()> {
    let mut doc: DocumentMut = match std::fs::read_to_string(path) {
        Ok(text) => text
            .parse()
            .with_context(|| format!("failed to parse {}", path.display()))?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => DocumentMut::new(),
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read {}", path.display()));
        }
    };
    if doc.get("jvm").is_none() {
        doc["jvm"] = Item::Table(Table::new());
    }
    doc["jvm"][language] = value(raw);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    std::fs::write(path, doc.to_string())
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

/// A declared developer tool: the tool name, its version requirement, and
/// where the declaration came from. Lives in `[tools.<language>]` tables.
#[derive(Debug, Clone)]
pub struct ToolPin {
    pub tool: String,
    pub raw: String,
    pub source: PinSource,
}

/// Read a `[tools.<language>]` table from `path`, if present.
fn read_tool_table(path: &Path, language: &str) -> Result<Vec<(String, String)>> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let doc: DocumentMut = text
        .parse()
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(doc
        .get("tools")
        .and_then(|t| t.get(language))
        .and_then(|t| t.as_table())
        .map(|table| {
            table
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.to_string(), s.to_string())))
                .collect()
        })
        .unwrap_or_default())
}

/// The tools declared for `language` active in `cwd`: the global config's
/// `[tools.<language>]` provides the base, and the nearest ancestor
/// linguo.toml that declares the table overrides/extends it per tool.
pub fn tool_pins(language: &str, cwd: &Path) -> Result<Vec<ToolPin>> {
    let mut pins: Vec<ToolPin> = read_tool_table(&linguo_root()?.join(GLOBAL_CONFIG), language)?
        .into_iter()
        .map(|(tool, raw)| ToolPin {
            tool,
            raw,
            source: PinSource::Global,
        })
        .collect();

    for dir in cwd.ancestors() {
        let candidate = dir.join(PIN_FILE);
        let project = read_tool_table(&candidate, language)?;
        if project.is_empty() {
            continue;
        }
        for (tool, raw) in project {
            pins.retain(|p| p.tool != tool);
            pins.push(ToolPin {
                tool,
                raw,
                source: PinSource::Project(candidate.clone()),
            });
        }
        break; // nearest declaring linguo.toml wins for the project layer
    }
    pins.sort_by(|a, b| a.tool.cmp(&b.tool));
    Ok(pins)
}

/// Set `[tools.<language>] <tool> = "<raw>"` in `path`, creating it if needed.
pub fn write_tool_pin(path: &Path, language: &str, tool: &str, raw: &str) -> Result<()> {
    let mut doc = read_or_new(path)?;
    if doc.get("tools").is_none() {
        doc["tools"] = Item::Table(Table::new());
    }
    if let Some(tools) = doc["tools"].as_table_mut() {
        tools.set_implicit(true);
    }
    if doc["tools"].get(language).is_none() {
        doc["tools"][language] = Item::Table(Table::new());
    }
    doc["tools"][language][tool] = value(raw);
    write_doc(path, &doc)
}

/// Remove `[tools.<language>] <tool>` from `path`; returns whether it existed.
pub fn remove_tool_pin(path: &Path, language: &str, tool: &str) -> Result<bool> {
    if !path.is_file() {
        return Ok(false);
    }
    let mut doc = read_or_new(path)?;
    let removed = doc
        .get_mut("tools")
        .and_then(|t| t.get_mut(language))
        .and_then(|t| t.as_table_mut())
        .and_then(|table| table.remove(tool))
        .is_some();
    if removed {
        write_doc(path, &doc)?;
    }
    Ok(removed)
}

/// Parse an existing pin file, or start a fresh document if it's absent.
fn read_or_new(path: &Path) -> Result<DocumentMut> {
    match std::fs::read_to_string(path) {
        Ok(text) => text
            .parse()
            .with_context(|| format!("failed to parse {}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(DocumentMut::new()),
        Err(err) => Err(err).with_context(|| format!("failed to read {}", path.display())),
    }
}

fn write_doc(path: &Path, doc: &DocumentMut) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    std::fs::write(path, doc.to_string())
        .with_context(|| format!("failed to write {}", path.display()))
}

/// Set `[runtimes] <language> = "<raw>"` in `path`, creating the file if needed.
pub fn write_pin(path: &Path, language: &str, raw: &str) -> Result<()> {
    let mut doc: DocumentMut = match std::fs::read_to_string(path) {
        Ok(text) => text
            .parse()
            .with_context(|| format!("failed to parse {}", path.display()))?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => DocumentMut::new(),
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read {}", path.display()));
        }
    };
    if doc.get("runtimes").is_none() {
        doc["runtimes"] = Item::Table(Table::new());
    }
    doc["runtimes"][language] = value(raw);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    std::fs::write(path, doc.to_string())
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_file_discovery_walks_up() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("a/b/c");
        std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(find_pin_file(&nested), None);

        let pin_path = tmp.path().join("a").join(PIN_FILE);
        std::fs::write(&pin_path, "[runtimes]\npython = \"3.12\"\n").unwrap();
        assert_eq!(find_pin_file(&nested), Some(pin_path.clone()));

        assert_eq!(
            read_pin_from(&pin_path, "python").unwrap(),
            Some("3.12".to_string())
        );
        assert_eq!(read_pin_from(&pin_path, "go").unwrap(), None);
    }

    #[test]
    fn write_pin_preserves_existing_content() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(PIN_FILE);
        std::fs::write(&path, "# project pins\n[runtimes]\ngo = \"1.22\"\n").unwrap();

        write_pin(&path, "python", "3.12").unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# project pins"));
        assert!(text.contains("go = \"1.22\""));
        assert!(text.contains("python = \"3.12\""));
    }

    #[test]
    fn write_pin_creates_new_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(PIN_FILE);
        write_pin(&path, "python", "3.13").unwrap();
        assert_eq!(
            read_pin_from(&path, "python").unwrap(),
            Some("3.13".to_string())
        );
    }

    #[test]
    fn tool_pins_roundtrip_and_coexist_with_runtimes() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(PIN_FILE);
        write_pin(&path, "python", "3.12").unwrap();
        write_tool_pin(&path, "python", "ruff", "0.6").unwrap();
        write_tool_pin(&path, "python", "black", "25").unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("[tools.python]"));
        assert!(text.contains("python = \"3.12\"")); // runtimes untouched

        let mut tools = read_tool_table(&path, "python").unwrap();
        tools.sort();
        assert_eq!(
            tools,
            vec![
                ("black".to_string(), "25".to_string()),
                ("ruff".to_string(), "0.6".to_string()),
            ]
        );

        assert!(remove_tool_pin(&path, "python", "ruff").unwrap());
        assert!(!remove_tool_pin(&path, "python", "ruff").unwrap());
        assert_eq!(
            read_tool_table(&path, "python").unwrap(),
            vec![("black".to_string(), "25".to_string())]
        );
    }
}
