use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use semver::Version;

use crate::git::Git;
use crate::manifest::{self, Manifest};
use crate::version::{Level, bump, latest_release_tag, release_tag_version};

const ORIGIN: &str = "origin";

/// What the repository looks like before a release.
pub struct Release {
    git: Git,
    pub manifests: Vec<Manifest>,
    pub current: Version,
    latest: Option<(String, Version)>,
    /// The commit HEAD pointed at when kraken started.
    head: String,
    pub branch: String,
    /// The branch on origin to push to; `None` without an origin remote.
    pub remote_branch: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub version: Version,
    pub tag: String,
    /// Files the release rewrites, relative to the repository root.
    pub files: Vec<String>,
    /// Rewritten files that git does not track; they stay out of the commit.
    pub untracked: Vec<String>,
    /// Commit message; `None` when there are no version files and only HEAD gets tagged.
    pub commit: Option<String>,
}

impl Release {
    pub fn prepare(dir: &Path) -> Result<Release> {
        let git = Git::discover(dir)?;
        let head = git.head()?;
        let branch = git.branch()?;
        if git.is_shallow()? {
            bail!(
                "this is a shallow clone and may miss tags; run `git fetch --unshallow --tags` first"
            );
        }
        git.ensure_clean()?;
        let manifests = manifest::detect(git.root(), |path| git.is_ignored(path))?;
        let tags = git.merged_tags()?;
        let latest = latest_release_tag(&tags).map(|(tag, version)| (tag.to_string(), version));
        let current = current_version(&manifests, latest.as_ref().map(|(_, version)| version))?;
        let remote_branch = git.has_remote(ORIGIN)?.then(|| {
            git.upstream(&branch, ORIGIN)
                .unwrap_or_else(|| branch.clone())
        });
        Ok(Release {
            git,
            manifests,
            current,
            latest,
            head,
            branch,
            remote_branch,
        })
    }

    pub fn latest_tag(&self) -> Option<&str> {
        self.latest.as_ref().map(|(tag, _)| tag.as_str())
    }

    /// Set when the version files and the latest tag disagree.
    pub fn warning(&self) -> Option<String> {
        let (tag, version) = self.latest.as_ref()?;
        (!self.manifests.is_empty() && *version != self.current).then(|| {
            format!(
                "latest tag {tag} differs from version {} in the version files",
                self.current
            )
        })
    }

    /// Fails when origin has release tags that are missing here.
    /// Returns a warning instead when origin does not answer.
    pub fn check_origin_tags(&self) -> Result<Option<String>> {
        let remote = match self.git.remote_tags(ORIGIN) {
            Ok(remote) => remote,
            Err(err) => return Ok(Some(format!("could not compare tags with origin: {err:#}"))),
        };
        let missing = missing_release_tags(&self.git.tags()?, &remote);
        if !missing.is_empty() {
            bail!(
                "{}; run `git fetch --tags origin` first",
                describe_missing_tags(&missing)
            );
        }
        Ok(None)
    }

    pub fn plan(&self, level: Level) -> Result<Plan> {
        let version = bump(&self.current, level);
        let prefix = tag_prefix(self.latest.as_ref().map(|(tag, _)| tag.as_str()));
        let tag = format!("{prefix}{version}");
        if self.git.tag_exists(&tag) {
            bail!("tag {tag} already exists");
        }
        if let Some((latest_tag, latest)) = &self.latest
            && version <= *latest
        {
            bail!(
                "{version} is not higher than the latest tag {latest_tag}; bring the version files up to date first"
            );
        }

        let mut files = Vec::new();
        for manifest in &self.manifests {
            files.push(manifest.path.clone());
            files.extend(manifest.lock.clone());
        }
        let file_refs: Vec<&str> = files.iter().map(String::as_str).collect();
        let tracked = self.git.tracked(&file_refs)?;
        let untracked: Vec<String> = files
            .iter()
            .filter(|file| !tracked.contains(file))
            .cloned()
            .collect();
        if let Some(manifest) = self
            .manifests
            .iter()
            .find(|manifest| untracked.contains(&manifest.path))
        {
            bail!("{} is not tracked by git; commit it first", manifest.path);
        }

        let commit = (!files.is_empty()).then(|| format!("chore(release): {tag}"));
        Ok(Plan {
            version,
            tag,
            files,
            untracked,
            commit,
        })
    }

    /// Writes the version files, commits and tags. On failure puts HEAD and the files back.
    pub fn apply(&self, plan: &Plan) -> Result<()> {
        // The repository may have changed while the user was choosing.
        self.git.ensure_clean()?;
        let head = self.git.head()?;
        if head != self.head || self.git.branch()? != self.branch {
            bail!("the repository changed since kraken started; run it again");
        }
        let originals = self.read_files(&plan.files)?;
        let result = self
            .write_files(plan, &originals)
            .and_then(|()| self.commit(plan))
            .and_then(|()| self.git.tag(&plan.tag));
        result.map_err(|err| match self.restore(&head, &originals) {
            Ok(()) => err,
            Err(restore) => err.context(format!("rollback incomplete: {restore:#}")),
        })
    }

    pub fn push(&self, plan: &Plan) -> Result<()> {
        let remote_branch = self
            .remote_branch
            .as_deref()
            .context("there is no origin remote")?;
        self.git
            .push(ORIGIN, &self.branch, remote_branch, &plan.tag)
            .context("push failed; the release commit and tag stay local")
    }

    fn read_files(&self, files: &[String]) -> Result<HashMap<String, String>> {
        files
            .iter()
            .map(|file| {
                Ok((
                    file.clone(),
                    fs::read_to_string(self.git.root().join(file))?,
                ))
            })
            .collect()
    }

    fn write_files(&self, plan: &Plan, originals: &HashMap<String, String>) -> Result<()> {
        let root = self.git.root();
        let version = plan.version.to_string();
        for manifest in &self.manifests {
            let name = &manifest.path;
            let text = manifest
                .kind
                .set_version(&originals[name], &version)
                .with_context(|| name.clone())?;
            fs::write(root.join(name), text).with_context(|| name.clone())?;
        }
        for manifest in &self.manifests {
            manifest.kind.check_requirements(root)?;
        }
        for manifest in &self.manifests {
            if let Some(lock) = &manifest.lock {
                let text = manifest
                    .kind
                    .update_lock(root, &version, &originals[lock])
                    .with_context(|| lock.clone())?;
                fs::write(root.join(lock), text).with_context(|| lock.clone())?;
            }
        }
        Ok(())
    }

    fn commit(&self, plan: &Plan) -> Result<()> {
        let Some(message) = &plan.commit else {
            return Ok(());
        };
        let files: Vec<&str> = plan
            .files
            .iter()
            .filter(|file| !plan.untracked.contains(file))
            .map(String::as_str)
            .collect();
        self.git.commit(&files, message)
    }

    /// Writes the original files back first: git can recover tracked files, but not untracked ones.
    fn restore(&self, head: &str, originals: &HashMap<String, String>) -> Result<()> {
        let mut errors: Vec<String> = originals
            .iter()
            .filter_map(|(file, text)| {
                let written = fs::write(self.git.root().join(file), text);
                written
                    .err()
                    .map(|err| format!("could not write back {file}: {err}"))
            })
            .collect();
        if let Err(err) = self.git.reset(head) {
            errors.push(format!(
                "could not move HEAD back; run `git reset {head}`: {err:#}"
            ));
        }
        if !errors.is_empty() {
            bail!("{}", errors.join("; "));
        }
        Ok(())
    }
}

/// "main and v1.2.4 to origin", naming the remote branch when it differs; `None` without origin.
pub fn push_target(branch: &str, remote_branch: Option<&str>, tag: &str) -> Option<String> {
    let remote_branch = remote_branch?;
    Some(if remote_branch == branch {
        format!("{branch} and {tag} to origin")
    } else {
        format!("{branch} to origin/{remote_branch} and {tag} to origin")
    })
}

/// The version files decide when there are any; otherwise the latest tag does.
fn current_version(manifests: &[Manifest], latest_tag: Option<&Version>) -> Result<Version> {
    let Some(first) = manifests.first() else {
        return Ok(latest_tag.cloned().unwrap_or(Version::new(0, 0, 0)));
    };
    if manifests
        .iter()
        .any(|manifest| manifest.version != first.version)
    {
        let list: Vec<String> = manifests
            .iter()
            .map(|manifest| format!("{} {}", manifest.path, manifest.version))
            .collect();
        bail!("version files disagree: {}", list.join(", "));
    }
    Ok(first.version.clone())
}

/// Release tags in `remote` that are not in `local`, newest first.
fn missing_release_tags<'a>(local: &[String], remote: &'a [String]) -> Vec<&'a str> {
    let mut missing: Vec<(&str, Version)> = remote
        .iter()
        .filter(|tag| !local.contains(tag))
        .filter_map(|tag| release_tag_version(tag).map(|version| (tag.as_str(), version)))
        .collect();
    missing.sort_by(|a, b| b.1.cmp(&a.1));
    missing.into_iter().map(|(tag, _)| tag).collect()
}

fn describe_missing_tags(missing: &[&str]) -> String {
    const SHOWN: usize = 3;
    let mut list = missing[..missing.len().min(SHOWN)].join(", ");
    if missing.len() > SHOWN {
        list.push_str(&format!(" and {} more", missing.len() - SHOWN));
    }
    let noun = if missing.len() == 1 { "tag" } else { "tags" };
    format!(
        "origin has {} release {noun} missing here: {list}",
        missing.len()
    )
}

/// `v` unless the latest tag goes without one.
fn tag_prefix(latest_tag: Option<&str>) -> &'static str {
    match latest_tag {
        Some(tag) if !tag.starts_with('v') => "",
        _ => "v",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Kind;

    fn manifest(kind: Kind, version: &str) -> Manifest {
        Manifest {
            kind,
            path: match kind {
                Kind::PackageJson => "package.json",
                Kind::CargoToml => "Cargo.toml",
                Kind::Pyproject => "pyproject.toml",
                Kind::VersionFile => "VERSION",
                Kind::Deno => "deno.json",
                Kind::Jsr => "jsr.json",
                Kind::Gradle => "gradle.properties",
                Kind::Xcode => "App.xcodeproj/project.pbxproj",
            }
            .to_string(),
            lock: None,
            version: Version::parse(version).unwrap(),
        }
    }

    #[test]
    fn version_files_decide() {
        let manifests = [
            manifest(Kind::PackageJson, "1.2.3"),
            manifest(Kind::CargoToml, "1.2.3"),
        ];
        let tag = Version::new(1, 0, 0);

        let current = current_version(&manifests, Some(&tag)).unwrap();

        assert_eq!(current, Version::new(1, 2, 3));
    }

    #[test]
    fn disagreeing_version_files_are_an_error() {
        let manifests = [
            manifest(Kind::PackageJson, "1.2.3"),
            manifest(Kind::CargoToml, "1.3.0"),
        ];

        let err = current_version(&manifests, None).unwrap_err().to_string();

        assert!(
            err.contains("package.json 1.2.3, Cargo.toml 1.3.0"),
            "{err}"
        );
    }

    #[test]
    fn latest_tag_decides_without_version_files() {
        let tag = Version::new(0, 4, 1);
        assert_eq!(current_version(&[], Some(&tag)).unwrap(), tag);
    }

    #[test]
    fn starts_from_zero() {
        assert_eq!(current_version(&[], None).unwrap(), Version::new(0, 0, 0));
    }

    #[test]
    fn tag_prefix_follows_latest_tag() {
        assert_eq!(tag_prefix(Some("v1.0.0")), "v");
        assert_eq!(tag_prefix(Some("1.0.0")), "");
        assert_eq!(tag_prefix(None), "v");
    }

    #[test]
    fn finds_release_tags_missing_locally() {
        let local = ["v1.0.0".to_string()];
        let remote = ["v1.0.0", "v1.1.0", "nightly", "v2.0.0-rc.1"].map(String::from);

        assert_eq!(missing_release_tags(&local, &remote), ["v1.1.0"]);
    }

    #[test]
    fn lists_newest_missing_tags_first() {
        let remote = ["v1.0.0", "v1.2.0", "v1.1.0"].map(String::from);

        assert_eq!(
            missing_release_tags(&[], &remote),
            ["v1.2.0", "v1.1.0", "v1.0.0"]
        );
    }

    #[test]
    fn shortens_a_long_list_of_missing_tags() {
        let missing = ["v1.4.0", "v1.3.0", "v1.2.0", "v1.1.0", "v1.0.0"];

        assert_eq!(
            describe_missing_tags(&missing),
            "origin has 5 release tags missing here: v1.4.0, v1.3.0, v1.2.0 and 2 more"
        );
    }

    #[test]
    fn names_a_single_missing_tag() {
        assert_eq!(
            describe_missing_tags(&["v1.4.0"]),
            "origin has 1 release tag missing here: v1.4.0"
        );
    }
}
