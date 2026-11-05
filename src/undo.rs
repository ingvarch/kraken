use std::fs;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};

use crate::changelog;
use crate::git::Git;
use crate::manifest::{self, Manifest};
use crate::release::{Current, release_subject};

const ORIGIN: &str = "origin";

/// A local release that `kraken undo` can remove.
pub struct Undo {
    git: Git,
    pub tag: String,
    head: String,
    branch: String,
    /// The release commit; `None` when the release only tagged HEAD.
    pub commit: Option<ReleaseCommit>,
    untracked_locks: Vec<String>,
}

pub struct ReleaseCommit {
    pub id: String,
    pub short: String,
    pub parent: String,
    pub files: Vec<String>,
}

impl ReleaseCommit {
    pub fn describe(&self, tag: &str) -> String {
        format!("{} \"{}\"", self.short, release_subject(tag))
    }
}

/// What `apply` did besides dropping the commit and deleting the tag.
#[derive(Debug)]
pub struct Undone {
    /// Lock files that git does not track, with the version each got.
    pub locks: Vec<(String, String)>,
    pub warnings: Vec<String>,
}

impl Undo {
    /// The checks that need no network.
    pub fn prepare(dir: &Path) -> Result<Undo> {
        let git = Git::discover(dir)?;
        let head = git.head()?;
        let branch = git.branch()?;
        git.ensure_not_shallow()?;
        git.ensure_clean()?;
        let current = Current::read(&git)?;
        let (tag, _) = current.latest.context("there is no release tag to undo")?;

        let object = git.tag_object(&tag)?;
        if object.commit != head {
            bail!(
                "the latest release {tag} is not at HEAD; kraken undo removes only a release with no commits after it"
            );
        }
        if !object.annotated || object.subject != tag {
            bail!(
                "kraken did not make tag {tag}: kraken tags are annotated, with the tag name as the message"
            );
        }

        let commit = release_commit(&git, &tag, &current.manifests)?;
        let mut untracked_locks = Vec::new();
        if let Some(commit) = &commit {
            if let Some(branch) = git.remote_branches_containing(&commit.id)?.first() {
                bail!(
                    "the release commit {} is already on {branch}; kraken undo removes only a release that is not pushed",
                    commit.short
                );
            }
            let locks: Vec<&str> = current
                .manifests
                .iter()
                .filter_map(|manifest| manifest.lock.as_deref())
                .collect();
            let tracked = git.tracked(&locks)?;
            untracked_locks = locks
                .into_iter()
                .filter(|lock| !tracked.iter().any(|file| file == lock))
                .map(str::to_string)
                .collect();
        }
        Ok(Undo {
            git,
            tag,
            head,
            branch,
            commit,
            untracked_locks,
        })
    }

    pub fn has_origin(&self) -> Result<bool> {
        self.git.has_remote(ORIGIN)
    }

    /// Fails when origin has the tag or the release commit, or does not answer.
    pub fn check_origin(&self) -> Result<()> {
        let tag = &self.tag;
        let refs = self.git.remote_refs(ORIGIN).map_err(|err| {
            anyhow!("could not check origin for {tag}: {err:#}; kraken undo needs origin to answer")
        })?;
        let local = self.git.tag_object(tag)?;
        let tag_ref = format!("refs/tags/{tag}");
        if refs
            .iter()
            .any(|(id, name)| *name == tag_ref && *id == local.id)
        {
            bail!(
                "tag {tag} is already on origin, and others may have fetched it; kraken undo removes only a release that is not pushed"
            );
        }
        let Some(commit) = &self.commit else {
            return Ok(());
        };
        for (id, name) in &refs {
            let Some(branch) = name.strip_prefix("refs/heads/") else {
                continue;
            };
            let contains = *id == commit.id
                || (self.git.has_commit(id) && self.git.is_ancestor(&commit.id, id)?);
            if contains {
                bail!(
                    "the release commit {} is already on origin/{branch}; kraken undo removes only a release that is not pushed",
                    commit.short
                );
            }
        }
        Ok(())
    }

    /// The lines under `kraken undo will:`.
    pub fn steps(&self) -> Vec<String> {
        let mut steps = Vec::new();
        if let Some(commit) = &self.commit {
            steps.push(format!(
                "drop commit {}, which changed {}",
                commit.describe(&self.tag),
                commit.files.join(", ")
            ));
        }
        steps.push(format!("delete tag {}", self.tag));
        match (&self.commit, self.untracked_locks.as_slice()) {
            (None, _) => steps.push(format!(
                "leave HEAD at {}: the release only tagged it",
                &self.head[..7]
            )),
            (Some(_), []) => {}
            (Some(_), [lock]) => steps.push(format!(
                "set {lock} to the version in the version files; git does not track it"
            )),
            (Some(_), locks) => steps.push(format!(
                "set {} to the version in the version files; git does not track them",
                locks.join(", ")
            )),
        }
        steps
    }

    pub fn apply(&self) -> Result<Undone> {
        // The repository may have changed while the user was answering.
        self.git.ensure_clean()?;
        let unchanged = self.git.head()? == self.head
            && self.git.branch()? == self.branch
            && self
                .git
                .tag_object(&self.tag)
                .is_ok_and(|object| object.commit == self.head);
        if !unchanged {
            bail!("the repository changed since kraken started; run it again");
        }
        if let Some(commit) = &self.commit {
            self.git.reset_keep(&commit.parent)?;
        }
        self.git.delete_tag(&self.tag).map_err(|err| {
            if self.commit.is_some() {
                err.context(format!(
                    "dropped the release commit, but could not delete tag {tag}; run `git tag -d {tag}`",
                    tag = self.tag
                ))
            } else {
                err
            }
        })?;
        let mut undone = Undone {
            locks: Vec::new(),
            warnings: Vec::new(),
        };
        if self.commit.is_some() {
            self.restore_locks(&mut undone)?;
        }
        Ok(undone)
    }

    /// Sets the lock files that git does not track to the restored version.
    fn restore_locks(&self, undone: &mut Undone) -> Result<()> {
        let root = self.git.root();
        let manifests = manifest::detect(root, |path| self.git.is_ignored(path))?;
        for manifest in &manifests {
            let Some(lock) = manifest
                .lock
                .as_ref()
                .filter(|lock| self.untracked_locks.contains(lock))
            else {
                continue;
            };
            let version = manifest.kind.spell(&manifest.version)?;
            let result = fs::read_to_string(root.join(lock))
                .map_err(anyhow::Error::from)
                .and_then(|text| manifest.kind.update_lock(root, &version, &text))
                .and_then(|text| Ok(fs::write(root.join(lock), text)?));
            match result {
                Ok(()) => undone.locks.push((lock.clone(), version)),
                Err(err) => undone
                    .warnings
                    .push(format!("could not set {lock} to {version}: {err:#}")),
            }
        }
        Ok(())
    }
}

/// The release commit at HEAD, or `None` for a release that only tagged HEAD.
/// Refuses a HEAD that kraken would not have made for `tag`.
fn release_commit(git: &Git, tag: &str, manifests: &[Manifest]) -> Result<Option<ReleaseCommit>> {
    let (parents, subject) = git.parents_and_subject("HEAD")?;
    let subject_is_release = subject == release_subject(tag);
    if !subject_is_release {
        if manifests.is_empty() {
            return Ok(None);
        }
        bail!(
            "{tag} is at HEAD, but HEAD is not its release commit \"{}\"",
            release_subject(tag)
        );
    }
    let id = git.head()?;
    let short = id[..7].to_string();
    let [parent] = parents.as_slice() else {
        if parents.is_empty() {
            bail!("the release commit {short} is the first commit; there is nothing to go back to");
        }
        bail!("the release commit {short} is a merge; kraken does not make merges");
    };
    let files = git.changed_files(&id)?;
    let mut allowed = manifest::release_files(manifests);
    allowed.push(changelog::FILE.to_string());
    let other: Vec<&str> = files
        .iter()
        .filter(|file| !allowed.contains(file))
        .map(String::as_str)
        .collect();
    if !other.is_empty() {
        bail!(
            "the release commit {short} changes files kraken does not write: {}",
            other.join(", ")
        );
    }
    Ok(Some(ReleaseCommit {
        id,
        short,
        parent: parent.clone(),
        files,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn undo(commit: Option<ReleaseCommit>, locks: &[&str]) -> Undo {
        let dir = tempfile::tempdir().unwrap();
        let init = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        assert!(init.success());
        Undo {
            git: Git::discover(dir.path()).unwrap(),
            tag: "v0.2.0".to_string(),
            head: "9c1e2f0aaaaaaaa".to_string(),
            branch: "main".to_string(),
            commit,
            untracked_locks: locks.iter().map(ToString::to_string).collect(),
        }
    }

    fn commit() -> ReleaseCommit {
        ReleaseCommit {
            id: "3632a93bbbbbbbb".to_string(),
            short: "3632a93".to_string(),
            parent: "9c1e2f0aaaaaaaa".to_string(),
            files: vec!["Cargo.toml".to_string(), "CHANGELOG.md".to_string()],
        }
    }

    #[test]
    fn steps_name_the_commit_and_the_files() {
        assert_eq!(
            undo(Some(commit()), &[]).steps(),
            [
                "drop commit 3632a93 \"chore(release): v0.2.0\", which changed Cargo.toml, CHANGELOG.md",
                "delete tag v0.2.0",
            ]
        );
    }

    #[test]
    fn steps_of_a_tag_only_release() {
        assert_eq!(
            undo(None, &[]).steps(),
            [
                "delete tag v0.2.0",
                "leave HEAD at 9c1e2f0: the release only tagged it",
            ]
        );
    }

    #[test]
    fn steps_mention_an_untracked_lock() {
        let steps = undo(Some(commit()), &["Cargo.lock"]).steps();

        assert_eq!(
            steps.last().unwrap(),
            "set Cargo.lock to the version in the version files; git does not track it"
        );
        let many = undo(Some(commit()), &["Cargo.lock", "uv.lock"]).steps();
        assert_eq!(
            many.last().unwrap(),
            "set Cargo.lock, uv.lock to the version in the version files; git does not track them"
        );
    }
}
