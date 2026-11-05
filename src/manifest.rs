mod cargo;
mod composer;
mod gradle;
mod msbuild;
mod package_json;
mod pubspec;
mod pyproject;
mod version_file;
mod web_extension;
mod xcode;
mod xcodegen;

use std::fs;
use std::ops::Range;
use std::path::Path;

use anyhow::{Context, Result, bail};
use semver::Version;
use toml_edit::{DocumentMut, Item, Value};

use crate::version::{parse_final, parse_version};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    PackageJson,
    CargoToml,
    Pyproject,
    VersionFile,
    Deno,
    Jsr,
    Gradle,
    XcodeGen,
    Xcode,
    Pubspec,
    Composer,
    WebExtension,
    MsBuildProps,
    DotnetProject,
}

impl Kind {
    pub const ALL: [Kind; 14] = [
        Kind::PackageJson,
        Kind::CargoToml,
        Kind::Pyproject,
        Kind::VersionFile,
        Kind::Deno,
        Kind::Jsr,
        Kind::Gradle,
        Kind::XcodeGen,
        Kind::Xcode,
        Kind::Pubspec,
        Kind::Composer,
        Kind::WebExtension,
        Kind::MsBuildProps,
        Kind::DotnetProject,
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
            Kind::XcodeGen => &["project.yml"],
            Kind::Pubspec => &["pubspec.yaml"],
            Kind::Composer => &["composer.json"],
            Kind::WebExtension => &["manifest.json"],
            Kind::MsBuildProps => &["Directory.Build.props"],
            Kind::DotnetProject => return msbuild::find_project(root, is_ignored),
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
            // The generated project is updated even when git ignores it.
            Kind::XcodeGen => return xcodegen::generated_project(root),
            Kind::Composer => return composer::lock_file(root),
            Kind::VersionFile
            | Kind::Deno
            | Kind::Jsr
            | Kind::Gradle
            | Kind::Xcode
            | Kind::Pubspec
            | Kind::WebExtension
            | Kind::MsBuildProps
            | Kind::DotnetProject => {
                return Ok(None);
            }
        };
        Ok(existing(root, name))
    }

    /// What limits this kind's version to numbers, `None` when a pre-release is fine.
    fn numbers_only(self) -> Option<&'static str> {
        match self {
            Kind::Xcode | Kind::XcodeGen => Some("MARKETING_VERSION"),
            Kind::WebExtension => Some("a browser extension's version"),
            _ => None,
        }
    }

    /// Parses `text`, a version as this kind's file spells it.
    pub fn parse_version(self, text: &str) -> Result<Version> {
        match self {
            Kind::Pyproject => pyproject::parse_pep440(text),
            Kind::Composer => composer::parse_version(text),
            _ => match self.numbers_only() {
                Some(what) => parse_final(text, what),
                None => parse_version(text),
            },
        }
    }

    /// The text this kind's files get for `version`.
    pub fn spell(self, version: &Version) -> Result<String> {
        match (self, self.numbers_only()) {
            (Kind::Pyproject, _) => pyproject::pep440(version),
            (_, Some(what)) if !version.pre.is_empty() => {
                bail!("{what} takes numbers only, so it cannot be {version}")
            }
            _ => Ok(version.to_string()),
        }
    }

    pub fn read_version(self, text: &str) -> Result<Option<String>> {
        match self {
            Kind::PackageJson | Kind::Deno | Kind::Jsr | Kind::Composer => {
                package_json::read_version(text)
            }
            Kind::CargoToml => cargo::read_version(text),
            Kind::Pyproject => pyproject::read_version(text),
            Kind::VersionFile => version_file::read_version(text),
            Kind::Gradle => gradle::read_version(text),
            Kind::XcodeGen => xcodegen::read_version(text),
            Kind::Xcode => xcode::read_version(text),
            Kind::Pubspec => pubspec::read_version(text),
            Kind::WebExtension => web_extension::read_version(text),
            Kind::MsBuildProps | Kind::DotnetProject => msbuild::read_version(text),
        }
    }

    pub fn set_version(self, text: &str, version: &str) -> Result<String> {
        match self {
            Kind::PackageJson | Kind::Deno | Kind::Jsr | Kind::Composer => {
                package_json::set_version(text, version)
            }
            Kind::CargoToml => cargo::set_version(text, version),
            Kind::Pyproject => pyproject::set_version(text, version),
            Kind::VersionFile => version_file::set_version(text, version),
            Kind::Gradle => gradle::set_version(text, version),
            Kind::XcodeGen => xcodegen::set_version(text, version),
            Kind::Xcode => xcode::set_version(text, version),
            Kind::Pubspec => pubspec::set_version(text, version),
            Kind::WebExtension => web_extension::set_version(text, version),
            Kind::MsBuildProps | Kind::DotnetProject => msbuild::set_version(text, version),
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
            | Kind::XcodeGen
            | Kind::Xcode
            | Kind::Pubspec
            | Kind::Composer
            | Kind::WebExtension
            | Kind::MsBuildProps
            | Kind::DotnetProject => Ok(()),
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
            Kind::XcodeGen => xcode::set_version(lock, version),
            Kind::Composer => composer::set_lock_hash(root, lock),
            Kind::VersionFile
            | Kind::Deno
            | Kind::Jsr
            | Kind::Gradle
            | Kind::Xcode
            | Kind::Pubspec
            | Kind::WebExtension
            | Kind::MsBuildProps
            | Kind::DotnetProject => Ok(lock.to_string()),
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

/// The version files and their lock files, relative to the repository root.
pub fn release_files(manifests: &[Manifest]) -> Vec<String> {
    manifests
        .iter()
        .flat_map(|manifest| [Some(&manifest.path), manifest.lock.as_ref()])
        .flatten()
        .cloned()
        .collect()
}

/// Manifests in `root` that carry a version. A file that `is_ignored` is skipped.
pub fn detect(root: &Path, is_ignored: impl Fn(&str) -> Result<bool>) -> Result<Vec<Manifest>> {
    let mut found: Vec<Manifest> = Vec::new();
    for kind in Kind::ALL {
        let generated =
            kind == Kind::Xcode && found.iter().any(|manifest| manifest.kind == Kind::XcodeGen);
        if generated {
            continue;
        }
        let Some(path) = kind.find(root, &is_ignored)? else {
            continue;
        };
        let version = fs::read_to_string(root.join(&path))
            .map_err(anyhow::Error::from)
            .and_then(|text| kind.read_version(&text))
            .and_then(|version| {
                version
                    .as_deref()
                    .map(|text| kind.parse_version(text))
                    .transpose()
            })
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

/// The one name in `found`, `None` for none; several are refused as "more than one `what`".
fn single(mut found: Vec<String>, what: &str) -> Result<Option<String>> {
    found.sort();
    match found.as_slice() {
        [] => Ok(None),
        [name] => Ok(Some(name.clone())),
        _ => bail!("more than one {what}: {}", found.join(", ")),
    }
}

/// `name` when `root` has a regular file with exactly that name, whatever the file system's case rules.
pub(crate) fn existing(root: &Path, name: &str) -> Option<String> {
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

/// The value after `key` at the start of `line`, without its quotes or a trailing comment.
pub(super) fn yaml_value(line: &str, key: &str) -> Option<Range<usize>> {
    let value = line.strip_prefix(key)?.trim_start();
    let start = line.len() - value.len();
    if let Some(quote) = value.chars().next().filter(|c| matches!(c, '"' | '\'')) {
        let inner = &value[1..];
        let len = inner.find(quote).unwrap_or(inner.trim_end().len());
        return Some(start + 1..start + 1 + len);
    }
    let end = value
        .char_indices()
        .find(|&(i, c)| c == '#' && value[..i].ends_with(char::is_whitespace))
        .map_or(value.len(), |(i, _)| i);
    Some(start..start + value[..end].trim_end().len())
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

    const VERSIONED_CSPROJ: &str =
        "<Project><PropertyGroup><Version>1.2.3</Version></PropertyGroup></Project>";

    #[test]
    fn dotnet_project_finds_the_single_csproj_that_sets_a_version() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Tool.csproj"), "<Project />").unwrap();
        fs::create_dir(dir.path().join("Dir.csproj")).unwrap();
        assert_eq!(find(Kind::DotnetProject, dir.path()), None);

        fs::write(dir.path().join("App.csproj"), VERSIONED_CSPROJ).unwrap();
        assert_eq!(
            find(Kind::DotnetProject, dir.path()).as_deref(),
            Some("App.csproj")
        );
    }

    #[test]
    fn dotnet_project_skips_a_symlinked_project() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("real")).unwrap();
        fs::write(dir.path().join("real/App.csproj"), VERSIONED_CSPROJ).unwrap();
        std::os::unix::fs::symlink("real/App.csproj", dir.path().join("App.csproj")).unwrap();

        assert_eq!(find(Kind::DotnetProject, dir.path()), None);
    }

    #[test]
    fn dotnet_project_counts_fsproj_and_vbproj() {
        for name in ["App.fsproj", "App.vbproj"] {
            let dir = tempfile::tempdir().unwrap();
            fs::write(dir.path().join(name), VERSIONED_CSPROJ).unwrap();
            assert_eq!(find(Kind::DotnetProject, dir.path()).as_deref(), Some(name));
        }
    }

    #[test]
    fn dotnet_project_refuses_several() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["Zed.csproj", "App.csproj"] {
            fs::write(dir.path().join(name), VERSIONED_CSPROJ).unwrap();
        }
        let err = Kind::DotnetProject
            .find(dir.path(), |_| Ok(false))
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "more than one project file sets a version: App.csproj, Zed.csproj"
        );
    }

    #[test]
    fn dotnet_project_skips_one_that_git_ignores() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["Zed.csproj", "App.csproj"] {
            fs::write(dir.path().join(name), VERSIONED_CSPROJ).unwrap();
        }
        let found = Kind::DotnetProject
            .find(dir.path(), |path| Ok(path == "Zed.csproj"))
            .unwrap();
        assert_eq!(found.as_deref(), Some("App.csproj"));
    }

    #[test]
    fn dotnet_project_names_a_file_that_is_not_utf8() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("App.csproj"), [0xff, 0xfe, b'<', 0]).unwrap();
        let err = Kind::DotnetProject
            .find(dir.path(), |_| Ok(false))
            .unwrap_err();
        assert!(format!("{err:#}").starts_with("App.csproj: "));
    }

    #[test]
    fn directory_build_props_must_be_named_exactly() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("directory.build.props"), VERSIONED_CSPROJ).unwrap();
        assert_eq!(find(Kind::MsBuildProps, dir.path()), None);

        fs::remove_file(dir.path().join("directory.build.props")).unwrap();
        fs::write(dir.path().join("Directory.Build.props"), VERSIONED_CSPROJ).unwrap();
        assert_eq!(
            find(Kind::MsBuildProps, dir.path()).as_deref(),
            Some("Directory.Build.props")
        );
    }

    #[test]
    fn dotnet_files_have_no_lock_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("packages.lock.json"), "{}").unwrap();
        assert_eq!(Kind::MsBuildProps.lock_file(dir.path()).unwrap(), None);
        assert_eq!(Kind::DotnetProject.lock_file(dir.path()).unwrap(), None);
    }

    #[test]
    fn xcodegen_takes_over_the_generated_project() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("project.yml"),
            "name: App\nsettings:\n  MARKETING_VERSION: 1.2.3\n",
        )
        .unwrap();
        fs::create_dir(dir.path().join("App.xcodeproj")).unwrap();
        fs::write(
            dir.path().join("App.xcodeproj/project.pbxproj"),
            "\t\tMARKETING_VERSION = 1.2.3;\n",
        )
        .unwrap();

        let found = detect(dir.path(), |_| Ok(false)).unwrap();

        assert_eq!(
            found,
            vec![Manifest {
                kind: Kind::XcodeGen,
                path: "project.yml".to_string(),
                lock: Some("App.xcodeproj/project.pbxproj".to_string()),
                version: Version::new(1, 2, 3)
            }]
        );
    }

    #[test]
    fn xcodegen_lock_is_the_project_named_in_project_yml() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("project.yml"),
            "name: App\nsettings:\n  MARKETING_VERSION: 1.2.3\n",
        )
        .unwrap();
        write_project(dir.path(), "Example.xcodeproj", VERSIONED_PBXPROJ);
        let locks = |root: &Path| -> Vec<Option<String>> {
            detect(root, |_| Ok(false))
                .unwrap()
                .into_iter()
                .map(|manifest| manifest.lock)
                .collect()
        };
        assert_eq!(locks(dir.path()), [None]);

        write_project(dir.path(), "App.xcodeproj", VERSIONED_PBXPROJ);
        assert_eq!(
            locks(dir.path()),
            [Some("App.xcodeproj/project.pbxproj".to_string())]
        );
    }

    #[test]
    fn a_project_yml_without_a_version_leaves_the_xcode_project() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("project.yml"), "name: App\n").unwrap();
        fs::create_dir(dir.path().join("App.xcodeproj")).unwrap();
        fs::write(
            dir.path().join("App.xcodeproj/project.pbxproj"),
            "\t\tMARKETING_VERSION = 1.2.3;\n",
        )
        .unwrap();

        let found = detect(dir.path(), |_| Ok(false)).unwrap();

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, Kind::Xcode);
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
    fn composer_json_without_a_version_is_not_a_version_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("composer.json"),
            r#"{ "name": "acme/app" }"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("composer.lock"),
            r#"{ "content-hash": "0123456789abcdef0123456789abcdef" }"#,
        )
        .unwrap();

        assert!(detect(dir.path(), |_| Ok(false)).unwrap().is_empty());
    }

    #[test]
    fn manifest_json_needs_manifest_version() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("manifest.json"),
            r#"{ "name": "App", "start_url": "/", "version": "1.2.3" }"#,
        )
        .unwrap();
        assert!(detect(dir.path(), |_| Ok(false)).unwrap().is_empty());

        fs::write(
            dir.path().join("manifest.json"),
            r#"{ "manifest_version": 3, "version": "1.2.3" }"#,
        )
        .unwrap();
        let found = detect(dir.path(), |_| Ok(false)).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, Kind::WebExtension);
    }

    #[test]
    fn pubspec_must_be_named_exactly() {
        for name in ["pubspec.yml", "Pubspec.yaml"] {
            let dir = tempfile::tempdir().unwrap();
            fs::write(dir.path().join(name), "version: 1.2.3").unwrap();
            assert_eq!(find(Kind::Pubspec, dir.path()), None, "{name}");
        }

        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("pubspec.yaml"), "version: 1.2.3").unwrap();
        assert_eq!(
            find(Kind::Pubspec, dir.path()).as_deref(),
            Some("pubspec.yaml")
        );
    }

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn semver_files_spell_a_pre_release_as_is() {
        for kind in [
            Kind::PackageJson,
            Kind::CargoToml,
            Kind::VersionFile,
            Kind::Deno,
            Kind::Jsr,
            Kind::Gradle,
            Kind::Pubspec,
            Kind::Composer,
            Kind::MsBuildProps,
            Kind::DotnetProject,
        ] {
            assert_eq!(
                kind.spell(&v("1.4.0-rc.1")).unwrap(),
                "1.4.0-rc.1",
                "{kind:?}"
            );
        }
    }

    #[test]
    fn pyproject_spells_pep440() {
        assert_eq!(Kind::Pyproject.spell(&v("1.4.0-rc.1")).unwrap(), "1.4.0rc1");
        assert_eq!(Kind::Pyproject.spell(&v("1.4.0")).unwrap(), "1.4.0");
    }

    #[test]
    fn numbers_only_files_cannot_spell_a_pre_release() {
        let cases = [
            (Kind::Xcode, "MARKETING_VERSION"),
            (Kind::XcodeGen, "MARKETING_VERSION"),
            (Kind::WebExtension, "a browser extension's version"),
        ];
        for (kind, what) in cases {
            let err = kind.spell(&v("1.4.0-rc.1")).unwrap_err().to_string();
            assert_eq!(
                err,
                format!("{what} takes numbers only, so it cannot be 1.4.0-rc.1")
            );
            assert_eq!(kind.spell(&v("1.4.0")).unwrap(), "1.4.0");
            let err = kind.parse_version("1.4.0-rc.1").unwrap_err().to_string();
            assert_eq!(err, format!("\"1.4.0-rc.1\": {what} takes numbers only"));
        }
    }

    #[test]
    fn detects_a_pre_release_in_package_json() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{ "version": "1.4.0-rc.1" }"#,
        )
        .unwrap();

        let found = detect(dir.path(), |_| Ok(false)).unwrap();

        assert_eq!(found[0].version, v("1.4.0-rc.1"));
    }

    #[test]
    fn detects_pep440_in_pyproject() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nversion = \"1.4.0rc1\"\n",
        )
        .unwrap();

        let found = detect(dir.path(), |_| Ok(false)).unwrap();

        assert_eq!(found[0].version, v("1.4.0-rc.1"));
    }

    #[test]
    fn pyproject_in_another_spelling_is_refused_with_the_file_name() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nversion = \"1.4.0-rc.1\"\n",
        )
        .unwrap();

        let err = format!("{:#}", detect(dir.path(), |_| Ok(false)).unwrap_err());

        assert_eq!(
            err,
            "pyproject.toml: \"1.4.0-rc.1\" is not X.Y.Z or X.Y.Z with aN, bN or rcN, such as 1.4.0rc1"
        );
    }

    #[test]
    fn reads_snapshot_from_gradle_properties() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("gradle.properties"),
            "version=1.4.0-SNAPSHOT\n",
        )
        .unwrap();

        let found = detect(dir.path(), |_| Ok(false)).unwrap();

        assert_eq!(found[0].version, v("1.4.0-SNAPSHOT"));
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
