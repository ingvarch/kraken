use std::path::Path;

use anyhow::{Result, bail};
use semver::{Version, VersionReq};
use serde::Deserialize;
use toml_edit::{DocumentMut, Item};

use super::{either_version, replace_str, set_toml_str, toml_str};
use crate::cmd;

const PACKAGE: &[&str] = &["package", "version"];
const WORKSPACE: &[&str] = &["workspace", "package", "version"];

pub fn read_version(text: &str) -> Result<Option<String>> {
    let doc: DocumentMut = text.parse()?;
    let version = either_version(
        ("[package]", toml_str(&doc, PACKAGE)),
        ("[workspace.package]", toml_str(&doc, WORKSPACE)),
    )?;
    if version.is_none() && doc.contains_key("workspace") {
        bail!("workspace has no [workspace.package] version; per-crate versions are not supported");
    }
    Ok(version)
}

pub fn set_version(text: &str, version: &str) -> Result<String> {
    let mut doc: DocumentMut = text.parse()?;
    set_toml_str(&mut doc, PACKAGE, version);
    set_toml_str(&mut doc, WORKSPACE, version);
    Ok(doc.to_string())
}

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Member>,
}

/// A workspace member as `cargo metadata` reports it.
#[derive(Deserialize)]
pub struct Member {
    name: String,
    version: String,
    dependencies: Vec<Dependency>,
}

#[derive(Deserialize)]
struct Dependency {
    name: String,
    req: String,
    path: Option<String>,
}

/// Workspace members as the manifests in `root` describe them now. Needs no network or cache.
pub fn workspace_members(root: &Path) -> Result<Vec<Member>> {
    let args = [
        "metadata",
        "--no-deps",
        "--offline",
        "--format-version",
        "1",
    ];
    let metadata: Metadata = serde_json::from_str(&cmd::run("cargo", &args, root)?)?;
    Ok(metadata.packages)
}

/// Fails when a path dependency inside the workspace no longer accepts the member's version.
pub fn check_path_requirements(members: &[Member]) -> Result<()> {
    for member in members {
        for dep in member.dependencies.iter().filter(|dep| dep.path.is_some()) {
            let Some(target) = members.iter().find(|target| target.name == dep.name) else {
                continue;
            };
            if !VersionReq::parse(&dep.req)?.matches(&Version::parse(&target.version)?) {
                bail!(
                    "{} depends on {} \"{}\", which does not accept {}; kraken does not change dependency requirements",
                    member.name,
                    dep.name,
                    dep.req,
                    target.version
                );
            }
        }
    }
    Ok(())
}

/// Sets the Cargo.lock entries of workspace members to their versions.
pub fn set_lock_versions(text: &str, members: &[Member]) -> Result<String> {
    let mut doc: DocumentMut = text.parse()?;
    let Some(packages) = doc
        .get_mut("package")
        .and_then(Item::as_array_of_tables_mut)
    else {
        return Ok(text.to_string());
    };

    // Lock entries refer to a package as "name version" when two packages share the name.
    let mut references = Vec::new();
    for package in packages.iter_mut().filter(|p| !p.contains_key("source")) {
        let name = package
            .get("name")
            .and_then(Item::as_str)
            .unwrap_or_default();
        let old = package
            .get("version")
            .and_then(Item::as_str)
            .unwrap_or_default();
        let Some(member) = members.iter().find(|member| member.name == name) else {
            continue;
        };
        if old != member.version {
            references.push((
                format!("{name} {old}"),
                format!("{name} {}", member.version),
            ));
            if let Some(value) = package.get_mut("version").and_then(Item::as_value_mut) {
                replace_str(value, &member.version);
            }
        }
    }

    for package in packages.iter_mut() {
        let Some(deps) = package.get_mut("dependencies").and_then(Item::as_array_mut) else {
            continue;
        };
        for dep in deps.iter_mut() {
            let new = references
                .iter()
                .find(|(old, _)| dep.as_str() == Some(old.as_str()))
                .map(|(_, new)| new);
            if let Some(new) = new {
                replace_str(dep, new);
            }
        }
    }
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(name: &str, version: &str, path_dep: Option<(&str, &str)>) -> Member {
        Member {
            name: name.to_string(),
            version: version.to_string(),
            dependencies: path_dep
                .map(|(name, req)| Dependency {
                    name: name.to_string(),
                    req: req.to_string(),
                    path: Some(format!("/repo/{name}")),
                })
                .into_iter()
                .collect(),
        }
    }

    #[test]
    fn rejects_root_package_without_version_in_a_workspace() {
        let text = "[package]\nname = \"demo\"\n\n[workspace]\nmembers = [\"engine\"]\n";
        let err = read_version(text).unwrap_err().to_string();
        assert!(err.contains("[workspace.package]"), "{err}");
    }

    #[test]
    fn keeps_literal_string_quotes() {
        let text = "[package]\nversion = '0.1.0'  # v\n";
        assert_eq!(
            set_version(text, "0.2.0").unwrap(),
            "[package]\nversion = '0.2.0'  # v\n"
        );
    }

    #[test]
    fn path_requirement_that_rejects_new_version_is_an_error() {
        let members = [
            member("app", "0.2.0", Some(("engine", "^0.1.0"))),
            member("engine", "0.2.0", None),
        ];

        let err = check_path_requirements(&members).unwrap_err().to_string();

        assert!(err.contains("app"), "{err}");
        assert!(err.contains("engine \"^0.1.0\""), "{err}");
    }

    #[test]
    fn path_requirement_that_accepts_new_version_is_fine() {
        let members = [
            member("app", "0.1.1", Some(("engine", "^0.1.0"))),
            member("engine", "0.1.1", None),
        ];
        check_path_requirements(&members).unwrap();
    }

    #[test]
    fn updates_member_lock_entries_only() {
        let lock = r#"version = 3

[[package]]
name = "app"
version = "0.1.0"
dependencies = [
 "engine 0.1.0",
 "serde",
]

[[package]]
name = "engine"
version = "0.1.0"

[[package]]
name = "engine"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "serde"
version = "0.1.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;
        let expected = r#"version = 3

[[package]]
name = "app"
version = "0.2.0"
dependencies = [
 "engine 0.2.0",
 "serde",
]

[[package]]
name = "engine"
version = "0.2.0"

[[package]]
name = "engine"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "serde"
version = "0.1.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;
        let members = [
            member("app", "0.2.0", None),
            member("engine", "0.2.0", None),
        ];

        assert_eq!(set_lock_versions(lock, &members).unwrap(), expected);
    }

    const PACKAGE: &str = r#"[package]
name = "demo"
version = "0.1.0" # bumped by kraken
edition = "2024"

[dependencies]
serde = { version = "0.1.0" }
"#;

    const WORKSPACE: &str = r#"[package]
name = "demo"
version.workspace = true

[workspace]
members = ["core"]

[workspace.package]
version = "2.0.0"
"#;

    #[test]
    fn reads_package_version() {
        assert_eq!(read_version(PACKAGE).unwrap().as_deref(), Some("0.1.0"));
    }

    #[test]
    fn reads_workspace_version() {
        assert_eq!(read_version(WORKSPACE).unwrap().as_deref(), Some("2.0.0"));
    }

    #[test]
    fn package_without_version() {
        assert_eq!(read_version("[package]\nname = \"demo\"\n").unwrap(), None);
    }

    #[test]
    fn rejects_package_and_workspace_mismatch() {
        let text = "[package]\nversion = \"1.0.0\"\n\n[workspace.package]\nversion = \"2.0.0\"\n";
        let err = read_version(text).unwrap_err().to_string();
        assert!(err.contains("differs"), "{err}");
    }

    #[test]
    fn rejects_workspace_without_shared_version() {
        let err = read_version("[workspace]\nmembers = [\"a\", \"b\"]\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("[workspace.package]"), "{err}");
    }

    #[test]
    fn sets_package_version_and_keeps_the_rest() {
        let expected = PACKAGE.replace("version = \"0.1.0\" #", "version = \"0.2.0\" #");
        assert_eq!(set_version(PACKAGE, "0.2.0").unwrap(), expected);
    }

    #[test]
    fn sets_workspace_version() {
        let expected = WORKSPACE.replace("\"2.0.0\"", "\"2.1.0\"");
        assert_eq!(set_version(WORKSPACE, "2.1.0").unwrap(), expected);
    }
}
