mod cargo;
mod package_json;
mod pyproject;

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use semver::Version;
use toml_edit::{DocumentMut, Item, Value};

use crate::version::parse_release;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    PackageJson,
    CargoToml,
    Pyproject,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::PackageJson, Kind::CargoToml, Kind::Pyproject];

    pub fn file_name(self) -> &'static str {
        match self {
            Kind::PackageJson => "package.json",
            Kind::CargoToml => "Cargo.toml",
            Kind::Pyproject => "pyproject.toml",
        }
    }

    pub fn lock_file(self) -> &'static str {
        match self {
            Kind::PackageJson => "package-lock.json",
            Kind::CargoToml => "Cargo.lock",
            Kind::Pyproject => "uv.lock",
        }
    }

    pub fn read_version(self, text: &str) -> Result<Option<String>> {
        match self {
            Kind::PackageJson => package_json::read_version(text),
            Kind::CargoToml => cargo::read_version(text),
            Kind::Pyproject => pyproject::read_version(text),
        }
    }

    pub fn set_version(self, text: &str, version: &str) -> Result<String> {
        match self {
            Kind::PackageJson => package_json::set_version(text, version),
            Kind::CargoToml => cargo::set_version(text, version),
            Kind::Pyproject => pyproject::set_version(text, version),
        }
    }

    /// Runs after the manifests in `root` are written. For Cargo, path dependencies
    /// inside the workspace must still accept the members' versions.
    pub fn check_requirements(self, root: &Path) -> Result<()> {
        match self {
            Kind::CargoToml => cargo::check_path_requirements(&cargo::workspace_members(root)?),
            Kind::PackageJson | Kind::Pyproject => Ok(()),
        }
    }

    /// The lock file text for the manifest in `root` that now has `version`.
    pub fn update_lock(self, root: &Path, version: &str, lock: &str) -> Result<String> {
        let updated = match self {
            Kind::PackageJson => package_json::set_lock_version(lock, version),
            Kind::CargoToml => {
                let members = cargo::workspace_members(root)?;
                cargo::set_lock_versions(lock, &members)
            }
            Kind::Pyproject => pyproject::set_uv_lock_version(lock, version),
        };
        updated.with_context(|| self.lock_file())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Manifest {
    pub kind: Kind,
    pub version: Version,
}

/// Manifests in `root` that carry a version.
pub fn detect(root: &Path) -> Result<Vec<Manifest>> {
    let mut found = Vec::new();
    for kind in Kind::ALL {
        let path = root.join(kind.file_name());
        if !path.exists() {
            continue;
        }
        let version = fs::read_to_string(&path)
            .map_err(anyhow::Error::from)
            .and_then(|text| kind.read_version(&text))
            .and_then(|version| version.as_deref().map(parse_release).transpose())
            .with_context(|| kind.file_name())?;
        if let Some(version) = version {
            found.push(Manifest { kind, version });
        }
    }
    Ok(found)
}

fn toml_str<'a>(doc: &'a DocumentMut, path: &[&str]) -> Option<&'a str> {
    let (first, rest) = path.split_first()?;
    let item = rest
        .iter()
        .try_fold(doc.get(first)?, |item, key| item.get(key))?;
    item.as_str()
}

fn set_toml_str(doc: &mut DocumentMut, path: &[&str], new: &str) {
    let Some((first, rest)) = path.split_first() else {
        return;
    };
    let item = doc
        .get_mut(first)
        .and_then(|item| rest.iter().try_fold(item, |item, key| item.get_mut(key)));
    if let Some(value) = item.and_then(Item::as_value_mut) {
        replace_str(value, new);
    }
}

/// Replaces a string value, keeping its quotes, comments and spacing. Other values stay as they are.
fn replace_str(value: &mut Value, new: &str) {
    let Value::String(old) = value else {
        return;
    };
    let raw = old.as_repr().and_then(|repr| repr.as_raw().as_str());
    let quote = if raw.is_some_and(|raw| raw.starts_with('\'')) {
        '\''
    } else {
        '"'
    };
    let Ok(mut fresh) = format!("{quote}{new}{quote}").parse::<Value>() else {
        return;
    };
    *fresh.decor_mut() = value.decor().clone();
    *value = fresh;
}

/// The version set in either of two places; when both are set they must agree.
fn either_version(
    (first_name, first): (&str, Option<&str>),
    (second_name, second): (&str, Option<&str>),
) -> Result<Option<String>> {
    match (first, second) {
        (Some(a), Some(b)) if a != b => {
            bail!("{first_name} version {a} differs from {second_name} version {b}")
        }
        (a, b) => Ok(a.or(b).map(str::to_string)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn detects_only_versioned_manifests() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("package.json"), r#"{ "name": "app" }"#).unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"1.2.3\"\n",
        )
        .unwrap();

        let found = detect(dir.path()).unwrap();

        assert_eq!(
            found,
            vec![Manifest {
                kind: Kind::CargoToml,
                version: Version::new(1, 2, 3)
            }]
        );
    }

    #[test]
    fn empty_directory_has_no_manifests() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(detect(dir.path()).unwrap(), vec![]);
    }

    #[test]
    fn errors_name_the_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nversion = \"1.2\"\n",
        )
        .unwrap();

        let err = format!("{:#}", detect(dir.path()).unwrap_err());

        assert!(err.contains("pyproject.toml"), "{err}");
        assert!(err.contains("\"1.2\""), "{err}");
    }
}
