use anyhow::{Result, anyhow, bail};
use semver::{Prerelease, Version};
use toml_edit::{DocumentMut, Item};

use super::{either_version, replace_str, set_toml_str, toml_str};
use crate::version::{Pre, kraken_pre, parse_version};

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

const PEP440_LABELS: [(&str, Pre); 3] = [("a", Pre::Alpha), ("b", Pre::Beta), ("rc", Pre::Rc)];

/// The PEP 440 spelling of `version`: `1.4.0rc1` for `1.4.0-rc.1`.
pub fn pep440(version: &Version) -> Result<String> {
    if version.pre.is_empty() {
        return Ok(version.to_string());
    }
    let Some((pre, number)) = kraken_pre(version) else {
        bail!("{version} has no PEP 440 spelling; kraken makes alpha.N, beta.N and rc.N");
    };
    let label = PEP440_LABELS
        .iter()
        .find_map(|&(label, known)| (known == pre).then_some(label))
        .expect("every label has a spelling");
    Ok(format!(
        "{}.{}.{}{label}{number}",
        version.major, version.minor, version.patch
    ))
}

/// Reads `X.Y.Z` and `X.Y.Z` with `aN`, `bN` or `rcN`, the normalized PEP 440 spelling.
pub fn parse_pep440(text: &str) -> Result<Version> {
    let split = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    let (base, tag) = text.split_at(split);
    if tag.is_empty() || tag.starts_with('+') {
        return parse_version(text);
    }
    let unreadable =
        || anyhow!("\"{text}\" is not X.Y.Z or X.Y.Z with aN, bN or rcN, such as 1.4.0rc1");
    let (pre, number) = PEP440_LABELS
        .iter()
        .find_map(|&(label, pre)| tag.strip_prefix(label).map(|number| (pre, number)))
        .ok_or_else(unreadable)?;
    let normalized = number.len() == 1 || !number.starts_with('0');
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) || !normalized {
        return Err(unreadable());
    }
    let base = parse_version(base).map_err(|_| unreadable())?;
    Ok(Version {
        pre: Prerelease::new(&format!("{pre}.{}", number.parse::<u64>()?))?,
        ..base
    })
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
    fn spells_pre_releases_in_pep440() {
        let spell = |text: &str| pep440(&Version::parse(text).unwrap()).unwrap();
        assert_eq!(spell("1.4.0-rc.1"), "1.4.0rc1");
        assert_eq!(spell("1.4.0-beta.2"), "1.4.0b2");
        assert_eq!(spell("1.4.0-alpha.10"), "1.4.0a10");
        assert_eq!(spell("1.4.0"), "1.4.0");
        assert!(pep440(&Version::parse("1.4.0-SNAPSHOT").unwrap()).is_err());
    }

    #[test]
    fn reads_pep440_pre_releases() {
        let read = |text: &str| parse_pep440(text).unwrap().to_string();
        assert_eq!(read("1.4.0rc1"), "1.4.0-rc.1");
        assert_eq!(read("1.4.0b2"), "1.4.0-beta.2");
        assert_eq!(read("1.4.0a10"), "1.4.0-alpha.10");
        assert_eq!(read("1.4.0"), "1.4.0");
    }

    #[test]
    fn refuses_other_pep440_forms() {
        for text in [
            "1.4.0-rc.1",
            "1.4.0.dev1",
            "1.4.0rc",
            "1.4.0.post1",
            "1.4.0c1",
            "1.4.0.rc1",
            "1.4rc1",
            "1.4.0rc01",
        ] {
            let err = parse_pep440(text).unwrap_err().to_string();
            assert_eq!(
                err,
                format!("\"{text}\" is not X.Y.Z or X.Y.Z with aN, bN or rcN, such as 1.4.0rc1")
            );
        }
        let err = parse_pep440("1.2.3+abc").unwrap_err().to_string();
        assert_eq!(err, "\"1.2.3+abc\": build metadata is not supported");
    }

    #[test]
    fn sets_poetry_version() {
        let expected = POETRY.replace("1.2.3", "1.3.0");
        assert_eq!(set_version(POETRY, "1.3.0").unwrap(), expected);
    }
}
