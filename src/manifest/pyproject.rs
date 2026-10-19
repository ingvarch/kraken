use anyhow::Result;
use toml_edit::{DocumentMut, Item};

use super::{either_version, replace_str, set_toml_str, toml_str};

const PROJECT: &[&str] = &["project", "version"];
const POETRY: &[&str] = &["tool", "poetry", "version"];

pub fn read_version(text: &str) -> Result<Option<String>> {
    let doc: DocumentMut = text.parse()?;
    either_version(
        ("[project]", toml_str(&doc, PROJECT)),
        ("[tool.poetry]", toml_str(&doc, POETRY)),
    )
}

pub fn set_version(text: &str, version: &str) -> Result<String> {
    let mut doc: DocumentMut = text.parse()?;
    set_toml_str(&mut doc, PROJECT, version);
    set_toml_str(&mut doc, POETRY, version);
    Ok(doc.to_string())
}

/// Sets the version of the project's own uv.lock entry, the one whose source is `.`.
pub fn set_uv_lock_version(text: &str, version: &str) -> Result<String> {
    let mut doc: DocumentMut = text.parse()?;
    let packages = doc
        .get_mut("package")
        .and_then(Item::as_array_of_tables_mut);
    for package in packages
        .into_iter()
        .flat_map(|packages| packages.iter_mut())
    {
        let own = package
            .get("source")
            .and_then(Item::as_inline_table)
            .is_some_and(|source| {
                ["editable", "virtual"]
                    .iter()
                    .any(|key| source.get(key).and_then(|path| path.as_str()) == Some("."))
            });
        if let Some(value) = package
            .get_mut("version")
            .and_then(Item::as_value_mut)
            .filter(|_| own)
        {
            replace_str(value, version);
        }
    }
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const UV_LOCK: &str = r#"version = 1
requires-python = ">=3.9"

[[package]]
name = "demo"
version = "0.1.0"
source = { editable = "." }

[[package]]
name = "helper"
version = "0.1.0"
source = { editable = "packages/helper" }

[[package]]
name = "idna"
version = "0.1.0"
source = { registry = "https://pypi.org/simple" }
"#;

    #[test]
    fn sets_own_editable_uv_lock_entry() {
        let expected = UV_LOCK.replacen("version = \"0.1.0\"", "version = \"0.2.0\"", 1);
        assert_eq!(set_uv_lock_version(UV_LOCK, "0.2.0").unwrap(), expected);
    }

    #[test]
    fn sets_own_virtual_uv_lock_entry() {
        let lock =
            "[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\nsource = { virtual = \".\" }\n";
        assert_eq!(
            set_uv_lock_version(lock, "0.2.0").unwrap(),
            lock.replace("0.1.0", "0.2.0")
        );
    }

    const PROJECT: &str = r#"[project]
name = "demo"
version = "0.1.0"  # bumped by kraken
dependencies = ["requests>=0.1.0"]
"#;

    const POETRY: &str = r#"[tool.poetry]
name = "demo"
version = "1.2.3"
"#;

    #[test]
    fn reads_project_version() {
        assert_eq!(read_version(PROJECT).unwrap().as_deref(), Some("0.1.0"));
    }

    #[test]
    fn reads_poetry_version() {
        assert_eq!(read_version(POETRY).unwrap().as_deref(), Some("1.2.3"));
    }

    #[test]
    fn dynamic_version_is_none() {
        let text = "[project]\nname = \"demo\"\ndynamic = [\"version\"]\n";
        assert_eq!(read_version(text).unwrap(), None);
    }

    #[test]
    fn rejects_project_and_poetry_mismatch() {
        let text = "[project]\nversion = \"1.0.0\"\n\n[tool.poetry]\nversion = \"2.0.0\"\n";
        let err = read_version(text).unwrap_err().to_string();
        assert!(err.contains("differs"), "{err}");
    }

    #[test]
    fn sets_project_version_and_keeps_the_rest() {
        let expected = PROJECT.replace("version = \"0.1.0\"", "version = \"0.2.0\"");
        assert_eq!(set_version(PROJECT, "0.2.0").unwrap(), expected);
    }

    #[test]
    fn sets_poetry_version() {
        let expected = POETRY.replace("1.2.3", "1.3.0");
        assert_eq!(set_version(POETRY, "1.3.0").unwrap(), expected);
    }
}
