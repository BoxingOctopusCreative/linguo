use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use toml_edit::{DocumentMut, Item, Table, value};

use crate::versions::VersionReq;

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

#[derive(Debug, Clone)]
pub struct Pin {
    pub req: VersionReq,
    pub source: PinSource,
}

/// Walk up from `start` looking for a `linguo.toml`.
pub fn find_pin_file(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|dir| dir.join(PIN_FILE))
        .find(|candidate| candidate.is_file())
}

fn read_pin_from(path: &Path, language: &str) -> Result<Option<VersionReq>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let doc: DocumentMut = text
        .parse()
        .with_context(|| format!("failed to parse {}", path.display()))?;
    let Some(raw) = doc
        .get("runtimes")
        .and_then(|t| t.get(language))
        .and_then(|v| v.as_str())
    else {
        return Ok(None);
    };
    let req = raw
        .parse()
        .with_context(|| format!("invalid {language} version '{raw}' in {}", path.display()))?;
    Ok(Some(req))
}

/// Resolve the pinned version request for `language`: nearest project
/// `linguo.toml` first, then the global config.
pub fn resolve_pin(language: &str, cwd: &Path) -> Result<Option<Pin>> {
    if let Some(path) = find_pin_file(cwd)
        && let Some(req) = read_pin_from(&path, language)?
    {
        return Ok(Some(Pin {
            req,
            source: PinSource::Project(path),
        }));
    }
    let global = linguo_root()?.join(GLOBAL_CONFIG);
    if global.is_file()
        && let Some(req) = read_pin_from(&global, language)?
    {
        return Ok(Some(Pin {
            req,
            source: PinSource::Global,
        }));
    }
    Ok(None)
}

/// Set `[runtimes] <language> = "<req>"` in `path`, creating the file if needed.
pub fn write_pin(path: &Path, language: &str, req: &VersionReq) -> Result<()> {
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
    doc["runtimes"][language] = value(req.to_string());
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
            Some("3.12".parse().unwrap())
        );
        assert_eq!(read_pin_from(&pin_path, "go").unwrap(), None);
    }

    #[test]
    fn write_pin_preserves_existing_content() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(PIN_FILE);
        std::fs::write(&path, "# project pins\n[runtimes]\ngo = \"1.22\"\n").unwrap();

        write_pin(&path, "python", &"3.12".parse().unwrap()).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# project pins"));
        assert!(text.contains("go = \"1.22\""));
        assert!(text.contains("python = \"3.12\""));
    }

    #[test]
    fn write_pin_creates_new_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(PIN_FILE);
        write_pin(&path, "python", &"3.13".parse().unwrap()).unwrap();
        assert_eq!(
            read_pin_from(&path, "python").unwrap(),
            Some("3.13".parse().unwrap())
        );
    }
}
