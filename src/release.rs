use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use semver::Version;

use crate::changelog::{self, Changes, Links, Section};
use crate::commit::Tally;
use crate::git::Git;
use crate::manifest::{self, Manifest};
use crate::origin::{self, Pages, Repo};
use crate::version::{
    Level, Pre, latest_final_tag, latest_release_tag, next, release_tag_version, suggest,
};

pub(crate) const ORIGIN: &str = "origin";

/// What the repository looks like before a release.
pub struct Release {
    git: Git,
    pub manifests: Vec<Manifest>,
    pub current: Version,
    latest: Option<(String, Version)>,
    /// The latest tag that is not a pre-release; the commits are counted from it.
    final_tag: Option<String>,
    /// Nothing was committed after the latest tag, pre-releases included.
    idle: bool,
    /// The commit HEAD pointed at when kraken started.
    head: String,
    pub branch: String,
    /// The branch on origin to push to; `None` without an origin remote.
    pub remote_branch: Option<String>,
    /// The commits since the latest final tag.
    changes: Changes,
    /// Release date; set only when the release adds a section to CHANGELOG.md.
    date: Option<String>,
    /// The repository origin pushes to; set only when the release adds a section to CHANGELOG.md.
    origin: Option<Repo>,
}

/// The version kraken releases from, read without changing or fetching anything.
pub struct Current {
    pub(crate) manifests: Vec<Manifest>,
    pub version: Version,
    pub(crate) latest: Option<(String, Version)>,
    final_tag: Option<String>,
    idle: bool,
}

/// The subject of the commit that releases `tag`.
pub(crate) fn release_subject(tag: &str) -> String {
    format!("chore(release): {tag}")
}

impl Current {
    /// Reads the version of the project at `dir`. Without version files the tags decide,
    /// and a shallow clone may miss them.
    pub fn of(dir: &Path) -> Result<Current> {
        let git = Git::discover(dir)?;
        git.head()?;
        let current = Current::read(&git)?;
        if current.manifests.is_empty() {
            git.ensure_not_shallow()?;
        }
        Ok(current)
    }

    pub(crate) fn read(git: &Git) -> Result<Current> {
        let manifests = manifest::detect(git.root(), |path| git.is_ignored(path))?;
        let tags = git.merged_tags()?;
        let latest = latest_release_tag(&tags).map(|(tag, version)| (tag.to_string(), version));
        let final_tag = latest_final_tag(&tags).map(|(tag, _)| tag.to_string());
        let version = current_version(&manifests, latest.as_ref().map(|(_, version)| version))?;
        let idle = match &latest {
            Some((tag, _)) => git.commits_since(Some(tag))?.is_empty(),
            None => false,
        };
        Ok(Current {
            manifests,
            version,
            latest,
            final_tag,
            idle,
        })
    }

    /// Set when the version files and the latest tag disagree.
    pub fn warning(&self) -> Option<String> {
        tag_warning(&self.manifests, &self.version, self.latest.as_ref())
    }
}

/// The release the commits since the latest final tag call for, worked out without changing anything.
pub struct Next {
    pub current: Version,
    pub version: Version,
    pub level: Level,
    pub commits: Tally,
    pub latest_tag: Option<String>,
    warning: Option<String>,
}

impl Next {
    /// Runs the tag checks of a release; skips the branch, clean-tree and origin checks.
    pub fn of(dir: &Path, pre: Option<Pre>) -> Result<Next> {
        let git = Git::discover(dir)?;
        git.head()?;
        git.ensure_not_shallow()?;
        let current = Current::read(&git)?;
        ensure_new_commits(pre, current.idle, current.latest.as_ref())?;
        let latest_tag = current.final_tag.clone();
        let commits = Tally::of(&git.commits_since(latest_tag.as_deref())?);
        let level = auto_level(&current.version, &commits, latest_tag.as_deref())?;
        let version = next(&current.version, level, pre)?;
        next_tag(&git, current.latest.as_ref(), &version)?;
        Ok(Next {
            warning: current.warning(),
            current: current.version,
            version,
            level,
            commits,
            latest_tag,
        })
    }

    /// Set when the version files and the latest tag disagree.
    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub version: Version,
    pub tag: String,
    /// Files the release rewrites, relative to the repository root.
    pub files: Vec<String>,
    /// Rewritten files that git does not track; they stay out of the commit.
    pub untracked: Vec<String>,
    /// Commit message; `None` when there are no files to change and only HEAD gets tagged.
    pub commit: Option<String>,
    /// The section CHANGELOG.md gets; `None` without CHANGELOG.md.
    pub changelog: Option<Section>,
    /// The link definitions CHANGELOG.md gets; `None` without a section or links.
    pub links: Option<Links>,
    /// Spellings that differ from the tag's, with the files that get each.
    pub other_spellings: Vec<(String, Vec<String>)>,
    /// The final version that gets the changelog section a pre-release leaves out.
    pub changelog_waits: Option<Version>,
}

impl Release {
    pub fn prepare(dir: &Path, with_changelog: bool) -> Result<Release> {
        let git = Git::discover(dir)?;
        let head = git.head()?;
        let branch = git.branch()?;
        git.ensure_not_shallow()?;
        git.ensure_clean()?;
        let Current {
            manifests,
            version: current,
            latest,
            final_tag,
            idle,
        } = Current::read(&git)?;
        let remote_branch = git.has_remote(ORIGIN)?.then(|| {
            git.upstream(&branch, ORIGIN)
                .unwrap_or_else(|| branch.clone())
        });
        let writes_changelog = with_changelog && changelog::is_on(git.root())?;
        let date = writes_changelog
            .then(|| changelog::ident_date(&git.committer_ident()?))
            .transpose()?;
        let origin = match &remote_branch {
            Some(_) if writes_changelog => origin::resolve(&git.push_url(ORIGIN)?, git.root()),
            _ => None,
        };
        let changes = Changes {
            commits: git.commits_since(final_tag.as_deref())?,
        };
        Ok(Release {
            git,
            manifests,
            current,
            latest,
            final_tag,
            idle,
            head,
            branch,
            remote_branch,
            changes,
            date,
            origin,
        })
    }

    pub fn tally(&self) -> Tally {
        Tally::of(&self.changes.commits)
    }

    /// The level the commits since the latest final tag call for.
    pub fn suggested(&self) -> Option<Level> {
        suggest(&self.current, &self.tally())
    }

    /// The suggested level; fails when there are no commits to take it from.
    pub fn auto_level(&self, pre: Option<Pre>) -> Result<Level> {
        ensure_new_commits(pre, self.idle, self.latest.as_ref())?;
        auto_level(&self.current, &self.tally(), self.final_tag())
    }

    /// The highest release tag, pre-releases included.
    pub fn latest_tag(&self) -> Option<&str> {
        self.latest.as_ref().map(|(tag, _)| tag.as_str())
    }

    /// The tag the commits are counted from.
    pub fn final_tag(&self) -> Option<&str> {
        self.final_tag.as_deref()
    }

    /// Warns when the version files and the latest tag disagree,
    /// and when a tag exists but nothing was committed after it.
    /// A pre-release counts from the latest tag of any kind, a final release from the latest final tag.
    pub fn warnings(&self, pre: Option<Pre>) -> Vec<String> {
        let since = if pre.is_some() {
            self.latest_tag().filter(|_| self.idle)
        } else {
            self.final_tag().filter(|_| self.changes.commits.is_empty())
        };
        let empty = since.map(|tag| format!("no commits since {tag}"));
        tag_warning(&self.manifests, &self.current, self.latest.as_ref())
            .into_iter()
            .chain(empty)
            .collect()
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

    pub fn plan(&self, level: Level, pre: Option<Pre>) -> Result<Plan> {
        let version = next(&self.current, level, pre)?;
        let tag = next_tag(&self.git, self.latest.as_ref(), &version)?;

        let mut files = manifest::release_files(&self.manifests);
        let mut spellings: Vec<(String, Vec<String>)> = Vec::new();
        for manifest in &self.manifests {
            let spelled = manifest
                .kind
                .spell(&version)
                .with_context(|| manifest.path.clone())?;
            let text = fs::read_to_string(self.git.root().join(&manifest.path))
                .with_context(|| manifest.path.clone())?;
            manifest
                .kind
                .set_version(&text, &spelled)
                .with_context(|| manifest.path.clone())?;
            let mut own = vec![manifest.path.clone()];
            own.extend(manifest.lock.clone());
            if spelled != version.to_string() {
                match spellings.iter_mut().find(|(known, _)| *known == spelled) {
                    Some((_, known)) => known.extend(own),
                    None => spellings.push((spelled, own)),
                }
            }
        }
        let is_final = version.pre.is_empty();
        let section = self
            .date
            .clone()
            .filter(|_| is_final)
            .map(|date| self.changes.section(&version, date));
        let links = match (&section, self.origin.as_ref().and_then(Pages::of)) {
            (Some(_), Some(pages)) => {
                let text = fs::read_to_string(self.git.root().join(changelog::FILE))
                    .context(changelog::FILE)?;
                changelog::links(&text, &pages, &version, self.final_tag(), &tag)
            }
            _ => None,
        };
        let changelog_waits = (self.date.is_some() && !is_final)
            .then(|| Version::new(version.major, version.minor, version.patch));
        if section.is_some() {
            files.push(changelog::FILE.to_string());
        }
        let file_refs: Vec<&str> = files.iter().map(String::as_str).collect();
        let tracked = self.git.tracked(&file_refs)?;
        let untracked: Vec<String> = files
            .iter()
            .filter(|file| !tracked.contains(file))
            .cloned()
            .collect();
        let changelog_file = section.is_some().then_some(changelog::FILE);
        let mut required = self
            .manifests
            .iter()
            .map(|manifest| manifest.path.as_str())
            .chain(changelog_file);
        if let Some(file) =
            required.find(|file| untracked.iter().any(|untracked| untracked == file))
        {
            bail!("{file} is not tracked by git; commit it first");
        }

        let commit = (!files.is_empty()).then(|| release_subject(&tag));
        Ok(Plan {
            version,
            tag,
            files,
            untracked,
            commit,
            changelog: section,
            links,
            other_spellings: spellings,
            changelog_waits,
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
            .context(
                "push failed; the release commit and tag stay local; `kraken undo` removes them",
            )
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
        for manifest in &self.manifests {
            let name = &manifest.path;
            let version = manifest.kind.spell(&plan.version)?;
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
                let version = manifest.kind.spell(&plan.version)?;
                let text = manifest
                    .kind
                    .update_lock(root, &version, &originals[lock])
                    .with_context(|| lock.clone())?;
                fs::write(root.join(lock), text).with_context(|| lock.clone())?;
            }
        }
        if let Some(section) = &plan.changelog {
            let text = changelog::insert(
                &originals[changelog::FILE],
                &section.markdown(),
                plan.links.as_ref(),
            );
            fs::write(root.join(changelog::FILE), text).context(changelog::FILE)?;
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

/// The level the commits call for, or an error when there are none.
pub fn auto_level(current: &Version, tally: &Tally, since: Option<&str>) -> Result<Level> {
    suggest(current, tally).with_context(|| {
        format!(
            "no commits since {}; nothing to release",
            since.unwrap_or("the first commit")
        )
    })
}

/// A pre-release counts from the latest tag of any kind, so it fails when nothing was committed after it.
fn ensure_new_commits(
    pre: Option<Pre>,
    idle: bool,
    latest: Option<&(String, Version)>,
) -> Result<()> {
    match latest {
        Some((tag, _)) if pre.is_some() && idle => {
            bail!("no commits since {tag}; nothing to release")
        }
        _ => Ok(()),
    }
}

/// The tag for `version`; fails when it exists or is not above the latest tag.
fn next_tag(git: &Git, latest: Option<&(String, Version)>, version: &Version) -> Result<String> {
    let prefix = tag_prefix(latest.map(|(tag, _)| tag.as_str()));
    let tag = format!("{prefix}{version}");
    if git.tag_exists(&tag) {
        bail!("tag {tag} already exists");
    }
    if let Some((latest_tag, latest)) = latest
        && version <= latest
    {
        bail!(
            "{version} is not higher than the latest tag {latest_tag}; bring the version files up to date first"
        );
    }
    Ok(tag)
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

fn tag_warning(
    manifests: &[Manifest],
    current: &Version,
    latest: Option<&(String, Version)>,
) -> Option<String> {
    let (tag, version) = latest?;
    (!manifests.is_empty() && version != current)
        .then(|| format!("latest tag {tag} differs from version {current} in the version files"))
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
                Kind::XcodeGen => "project.yml",
                Kind::Xcode => "App.xcodeproj/project.pbxproj",
                Kind::Pubspec => "pubspec.yaml",
                Kind::Composer => "composer.json",
                Kind::WebExtension => "manifest.json",
                Kind::MsBuildProps => "Directory.Build.props",
                Kind::DotnetProject => "App.csproj",
            }
            .to_string(),
            lock: None,
            version: Version::parse(version).unwrap(),
        }
    }

    #[test]
    fn auto_level_follows_the_commits() {
        let tally = Tally {
            features: 1,
            ..Tally::default()
        };

        let level = auto_level(&Version::new(1, 4, 2), &tally, Some("v1.4.2")).unwrap();

        assert_eq!(level, Level::Minor);
    }

    #[test]
    fn auto_level_refuses_without_commits() {
        let err = auto_level(&Version::new(1, 4, 2), &Tally::default(), Some("v1.4.2"))
            .unwrap_err()
            .to_string();

        assert_eq!(err, "no commits since v1.4.2; nothing to release");
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

        assert_eq!(
            missing_release_tags(&local, &remote),
            ["v2.0.0-rc.1", "v1.1.0"]
        );
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
