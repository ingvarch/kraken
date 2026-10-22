mod cargo;
mod gradle;
mod package_json;
mod pyproject;
mod version_file;
mod xcode;

use std::fs;
use std::ops::Range;
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
    VersionFile,
    Deno,
    Jsr,
    Gradle,
    Xcode,
}

impl Kind {
    pub const ALL: [Kind; 8] = [
        Kind::PackageJson,
        Kind::CargoToml,
        Kind::Pyproject,
        Kind::VersionFile,
        Kind::Deno,
        Kind::Jsr,
        Kind::Gradle,
        Kind::Xcode,
    ];

    /// The path of this kind's file in `root`, relative to it, or `None` when there is none.
    /// A file that `is_ignored` does not count.
    pub fn find(
        self,
        root: &Path,
        is_ignored: impl Fn(&str) -> Result<bool>,
    ) -> Result<Option<String>> {
        let names: &[&str] = match self {
            Kind::PackageJson => &["package.json"],
            Kind::CargoToml => &["Cargo.toml"],
            Kind::Pyproject => &["pyproject.toml"],
            Kind::VersionFile => &["VERSION"],
            Kind::Deno => &["deno.json", "deno.jsonc"],
            Kind::Jsr => &["jsr.json", "jsr.jsonc"],
            Kind::Gradle => &["gradle.properties"],
            Kind::Xcode => return xcode::find(root, is_ignored),
        };
        for path in names.iter().filter_map(|name| existing(root, name)) {
            if !is_ignored(&path)? {
                return Ok(Some(path));
            }
        }
        Ok(None)
    }

    /// The path of this kind's lock file in `root`, relative to it, or `None` when there is none.
    pub fn lock_file(self, root: &Path) -> Result<Option<String>> {
        let name = match self {
            Kind::PackageJson => "package-lock.json",
            Kind::CargoToml => "Cargo.lock",
            Kind::Pyproject => "uv.lock",
            Kind::VersionFile | Kind::Deno | Kind::Jsr | Kind::Gradle | Kind::Xcode => {
                return Ok(None);
            }
        };
        Ok(existing(root, name))
    }

    pub fn read_version(self, text: &str) -> Result<Option<String>> {
        match self {
            Kind::PackageJson | Kind::Deno | Kind::Jsr => package_json::read_version(text),
            Kind::CargoToml => cargo::read_version(text),
            Kind::Pyproject => pyproject::read_version(text),
            Kind::VersionFile => version_file::read_version(text),
            Kind::Gradle => gradle::read_version(text),
            Kind::Xcode => xcode::read_version(text),
        }
    }

    pub fn set_version(self, text: &str, version: &str) -> Result<String> {
        match self {
            Kind::PackageJson | Kind::Deno | Kind::Jsr => package_json::set_version(text, version),
            Kind::CargoToml => cargo::set_version(text, version),
            Kind::Pyproject => pyproject::set_version(text, version),
            Kind::VersionFile => version_file::set_version(text, version),
            Kind::Gradle => gradle::set_version(text, version),
            Kind::Xcode => xcode::set_version(text, version),
        }
    }

    /// Runs after the manifests in `root` are written. For Cargo, path dependencies
    /// inside the workspace must still accept the members' versions.
    pub fn check_requirements(self, root: &Path) -> Result<()> {
        match self {
            Kind::CargoToml => cargo::check_path_requirements(&cargo::workspace_members(root)?),
            Kind::PackageJson
            | Kind::Pyproject
            | Kind::VersionFile
            | Kind::Deno
            | Kind::Jsr
            | Kind::Gradle
            | Kind::Xcode => Ok(()),
        }
    }

    /// The lock file text for the manifest in `root` that now has `version`.
    pub fn update_lock(self, root: &Path, version: &str, lock: &str) -> Result<String> {
        match self {
            Kind::PackageJson => package_json::set_lock_version(lock, version),
            Kind::CargoToml => {
                let members = cargo::workspace_members(root)?;
                cargo::set_lock_versions(lock, &members)
            }
            Kind::Pyproject => pyproject::set_uv_lock_version(lock, version),
            Kind::VersionFile | Kind::Deno | Kind::Jsr | Kind::Gradle | Kind::Xcode => {
                Ok(lock.to_string())
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Manifest {
    pub kind: Kind,
    pub path: String,
    pub lock: Option<String>,
    pub version: Version,
}

/// Manifests in `root` that carry a version. A file that `is_ignored` is skipped.
pub fn detect(root: &Path, is_ignored: impl Fn(&str) -> Result<bool>) -> Result<Vec<Manifest>> {
    let mut found = Vec::new();
    for kind in Kind::ALL {
        let Some(path) = kind.find(root, &is_ignored)? else {
            continue;
        };
        let version = fs::read_to_string(root.join(&path))
            .map_err(anyhow::Error::from)
            .and_then(|text| kind.read_version(&text))
            .and_then(|version| version.as_deref().map(parse_release).transpose())
            .with_context(|| path.clone())?;
        if let Some(version) = version {
            let lock = kind.lock_file(root)?;
            found.push(Manifest {
                kind,
                path,
                lock,
                version,
            });
        }
    }
    Ok(found)
}

/// `name` when `root` has a regular file with exactly that name, whatever the file system's case rules.
fn existing(root: &Path, name: &str) -> Option<String> {
    fs::read_dir(root)
        .ok()?
        .flatten()
        .any(|entry| entry.file_name() == name && entry.path().is_file())
        .then(|| name.to_string())
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

/// The byte ranges that `value_range` finds on the lines of `text`.
fn line_values(text: &str, value_range: fn(&str) -> Option<Range<usize>>) -> Vec<Range<usize>> {
    let mut offset = 0;
    let mut found = Vec::new();
    for line in text.split_inclusive('\n') {
        if let Some(range) = value_range(line) {
            found.push(offset + range.start..offset + range.end);
        }
        offset += line.len();
    }
    found
}

/// The one value at `ranges` in `text`; several must agree.
fn agreed_value(setting: &str, text: &str, ranges: &[Range<usize>]) -> Result<Option<String>> {
    let mut values = ranges.iter().map(|range| &text[range.clone()]);
    let Some(first) = values.next() else {
        return Ok(None);
    };
    if let Some(other) = values.find(|value| *value != first) {
        bail!("{setting} has two values: {first} and {other}");
    }
    Ok(Some(first.to_string()))
}

fn replace_ranges(text: &str, ranges: &[Range<usize>], version: &str) -> String {
    let mut out = text.to_string();
    for range in ranges.iter().rev() {
        out.replace_range(range.clone(), version);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// `kind`'s file in `root` when git ignores nothing.
    fn find(kind: Kind, root: &Path) -> Option<String> {
        kind.find(root, |_| Ok(false)).unwrap()
    }

    #[test]
    fn detects_only_versioned_manifests() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("package.json"), r#"{ "name": "app" }"#).unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"1.2.3\"\n",
        )
        .unwrap();

        let found = detect(dir.path(), |_| Ok(false)).unwrap();

        assert_eq!(
            found,
            vec![Manifest {
                kind: Kind::CargoToml,
                path: "Cargo.toml".to_string(),
                lock: None,
                version: Version::new(1, 2, 3)
            }]
        );
    }

    #[test]
    fn find_falls_back_to_the_jsonc_file() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(find(Kind::Deno, dir.path()), None);
        assert_eq!(find(Kind::Jsr, dir.path()), None);

        fs::write(dir.path().join("deno.jsonc"), "{}").unwrap();
        fs::write(dir.path().join("jsr.jsonc"), "{}").unwrap();
        assert_eq!(find(Kind::Deno, dir.path()).as_deref(), Some("deno.jsonc"));
        assert_eq!(find(Kind::Jsr, dir.path()).as_deref(), Some("jsr.jsonc"));

        fs::write(dir.path().join("deno.json"), "{}").unwrap();
        fs::write(dir.path().join("jsr.json"), "{}").unwrap();
        assert_eq!(find(Kind::Deno, dir.path()).as_deref(), Some("deno.json"));
        assert_eq!(find(Kind::Jsr, dir.path()).as_deref(), Some("jsr.json"));
    }

    const VERSIONED_PBXPROJ: &str = "\t\tMARKETING_VERSION = 1.2.3;\n";

    fn write_project(root: &Path, name: &str, pbxproj: &str) {
        fs::create_dir(root.join(name)).unwrap();
        fs::write(root.join(name).join("project.pbxproj"), pbxproj).unwrap();
    }

    #[test]
    fn xcode_finds_the_single_project() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(find(Kind::Xcode, dir.path()), None);

        fs::create_dir(dir.path().join("Empty.xcodeproj")).unwrap();
        fs::create_dir(dir.path().join("notes")).unwrap();
        assert_eq!(find(Kind::Xcode, dir.path()), None);

        write_project(dir.path(), "App.xcodeproj", VERSIONED_PBXPROJ);
        assert_eq!(
            find(Kind::Xcode, dir.path()).as_deref(),
            Some("App.xcodeproj/project.pbxproj")
        );
    }

    #[test]
    fn xcode_counts_only_projects_that_set_a_version() {
        let dir = tempfile::tempdir().unwrap();
        write_project(dir.path(), "Tool.xcodeproj", "\t\tPRODUCT_NAME = Tool;\n");
        assert_eq!(find(Kind::Xcode, dir.path()), None);

        write_project(dir.path(), "App.xcodeproj", VERSIONED_PBXPROJ);
        assert_eq!(
            find(Kind::Xcode, dir.path()).as_deref(),
            Some("App.xcodeproj/project.pbxproj")
        );
    }

    #[test]
    fn xcode_refuses_several_projects() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["Zed.xcodeproj", "App.xcodeproj"] {
            write_project(dir.path(), name, VERSIONED_PBXPROJ);
        }

        let err = Kind::Xcode
            .find(dir.path(), |_| Ok(false))
            .unwrap_err()
            .to_string();

        assert_eq!(
            err,
            "more than one Xcode project sets MARKETING_VERSION: App.xcodeproj, Zed.xcodeproj"
        );
    }

    #[test]
    fn xcode_skips_a_project_that_git_ignores() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["Zed.xcodeproj", "App.xcodeproj"] {
            write_project(dir.path(), name, VERSIONED_PBXPROJ);
        }

        let found = Kind::Xcode
            .find(dir.path(), |path| {
                Ok(path == "Zed.xcodeproj/project.pbxproj")
            })
            .unwrap();

        assert_eq!(found.as_deref(), Some("App.xcodeproj/project.pbxproj"));
    }

    #[test]
    fn xcode_skips_a_symlinked_project() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("real")).unwrap();
        write_project(&dir.path().join("real"), "App.xcodeproj", VERSIONED_PBXPROJ);
        std::os::unix::fs::symlink("real/App.xcodeproj", dir.path().join("App.xcodeproj")).unwrap();

        assert_eq!(find(Kind::Xcode, dir.path()), None);
    }

    #[test]
    fn deno_and_jsr_are_two_manifests() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("deno.json"), r#"{ "version": "1.2.3" }"#).unwrap();
        fs::write(dir.path().join("jsr.jsonc"), r#"{ "version": "1.2.3" }"#).unwrap();

        let found = detect(dir.path(), |_| Ok(false)).unwrap();

        let paths: Vec<_> = found
            .iter()
            .map(|manifest| manifest.path.as_str())
            .collect();
        assert_eq!(paths, ["deno.json", "jsr.jsonc"]);
    }

    #[test]
    fn version_file_must_be_a_regular_file_named_exactly() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("VERSION")).unwrap();
        assert_eq!(find(Kind::VersionFile, dir.path()), None);

        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("version"), "1.2.3").unwrap();
        assert_eq!(find(Kind::VersionFile, dir.path()), None);

        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("VERSION"), "1.2.3").unwrap();
        assert_eq!(
            find(Kind::VersionFile, dir.path()).as_deref(),
            Some("VERSION")
        );
    }

    #[test]
    fn empty_directory_has_no_manifests() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(detect(dir.path(), |_| Ok(false)).unwrap(), vec![]);
    }

    #[test]
    fn skips_a_version_file_that_git_ignores() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("VERSION"), "1.2.3\n").unwrap();
        fs::write(dir.path().join("gradle.properties"), "version=1.0.0\n").unwrap();

        let found = detect(dir.path(), |path| Ok(path == "VERSION")).unwrap();

        let paths: Vec<_> = found.iter().map(|m| m.path.as_str()).collect();
        assert_eq!(paths, ["gradle.properties"]);
    }

    #[test]
    fn errors_name_the_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nversion = \"1.2\"\n",
        )
        .unwrap();

        let err = format!("{:#}", detect(dir.path(), |_| Ok(false)).unwrap_err());

        assert!(err.contains("pyproject.toml"), "{err}");
        assert!(err.contains("\"1.2\""), "{err}");
    }
}
