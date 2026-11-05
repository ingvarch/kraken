use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::cmd;
use crate::commit::Commit;

pub struct TagObject {
    pub annotated: bool,
    /// The tag object, or the commit for a lightweight tag.
    pub id: String,
    pub commit: String,
    /// The first line of the tag message; empty for a lightweight tag.
    pub subject: String,
}

pub struct Git {
    root: PathBuf,
}

impl Git {
    pub fn discover(dir: &Path) -> Result<Git> {
        let root = cmd::run("git", &["rev-parse", "--show-toplevel"], dir)
            .context("not inside a git repository")?;
        Ok(Git { root: root.into() })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn head(&self) -> Result<String> {
        self.run(&["rev-parse", "--verify", "--quiet", "HEAD"])
            .context("the repository has no commits")
    }

    pub fn branch(&self) -> Result<String> {
        self.run(&["symbolic-ref", "--quiet", "--short", "HEAD"])
            .context("HEAD is detached; check out a branch first")
    }

    pub fn is_shallow(&self) -> Result<bool> {
        Ok(self.run(&["rev-parse", "--is-shallow-repository"])? == "true")
    }

    pub fn ensure_not_shallow(&self) -> Result<()> {
        if self.is_shallow()? {
            bail!(
                "this is a shallow clone and may miss tags; run `git fetch --unshallow --tags` first"
            );
        }
        Ok(())
    }

    pub fn ensure_clean(&self) -> Result<()> {
        if !self
            .run(&["status", "--porcelain", "--untracked-files=no"])?
            .is_empty()
        {
            bail!("there are uncommitted changes; commit or stash them first");
        }
        Ok(())
    }

    /// Tags reachable from HEAD.
    pub fn merged_tags(&self) -> Result<Vec<String>> {
        let list = self.run(&["tag", "--list", "--merged", "HEAD"])?;
        Ok(list.lines().map(str::to_string).collect())
    }

    /// Commits after `tag` up to HEAD, oldest first and without merges; every commit without `tag`.
    pub fn commits_since(&self, tag: Option<&str>) -> Result<Vec<Commit>> {
        let range = tag.map_or("HEAD".to_string(), |tag| format!("refs/tags/{tag}..HEAD"));
        let log = self.run(&[
            "log",
            "--no-show-signature",
            "--no-merges",
            "--reverse",
            "--format=%s%x00%b%x1e",
            &range,
        ])?;
        Ok(log
            .split('\x1e')
            .filter_map(|record| record.trim_start().split_once('\0'))
            .map(|(subject, body)| Commit {
                subject: subject.to_string(),
                body: body.trim().to_string(),
            })
            .collect())
    }

    /// The name, email, time and time zone the next commit gets.
    pub fn committer_ident(&self) -> Result<String> {
        self.run(&["var", "GIT_COMMITTER_IDENT"])
    }

    pub fn tags(&self) -> Result<Vec<String>> {
        let list = self.run(&["tag", "--list"])?;
        Ok(list.lines().map(str::to_string).collect())
    }

    /// Tags on `remote`.
    pub fn remote_tags(&self, remote: &str) -> Result<Vec<String>> {
        Ok(self
            .remote_refs(remote)?
            .into_iter()
            .filter_map(|(_, name)| name.strip_prefix("refs/tags/").map(str::to_string))
            .collect())
    }

    /// The branches and tags on `remote` as (object id, ref name). Asks the remote over the
    /// network without prompting for credentials and gives up after a few seconds.
    pub fn remote_refs(&self, remote: &str) -> Result<Vec<(String, String)>> {
        let mut command = Command::new("git");
        command
            .args(["ls-remote", "--heads", "--tags", "--refs", remote])
            .current_dir(&self.root)
            .env("GIT_TERMINAL_PROMPT", "0");
        let list = cmd::run_with_limit(&mut command, Duration::from_secs(10))?;
        Ok(list
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .map(|(id, name)| (id.to_string(), name.to_string()))
            .collect())
    }

    /// The remote-tracking branches that contain `commit`, such as `origin/main`.
    /// Skips `origin/HEAD`, which only points at one of them.
    pub fn remote_branches_containing(&self, commit: &str) -> Result<Vec<String>> {
        let list = self.run(&[
            "for-each-ref",
            "--contains",
            commit,
            "--format=%(if)%(symref)%(then)%(else)%(refname:lstrip=2)%(end)",
            "refs/remotes",
        ])?;
        Ok(list
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect())
    }

    /// What `tag` is. A lightweight tag has no object of its own, so `id` is its commit.
    pub fn tag_object(&self, tag: &str) -> Result<TagObject> {
        let format =
            "--format=%(objecttype)%00%(objectname)%00%(*objectname)%00%(contents:subject)";
        let line = self.run(&["for-each-ref", format, &format!("refs/tags/{tag}")])?;
        let mut fields = line.split('\0');
        let (Some(kind), Some(id), Some(target), Some(subject)) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            bail!("tag {tag} does not exist");
        };
        let annotated = kind == "tag";
        Ok(TagObject {
            annotated,
            id: id.to_string(),
            commit: if annotated { target } else { id }.to_string(),
            subject: if annotated { subject } else { "" }.to_string(),
        })
    }

    /// The parents and the subject of `rev`.
    pub fn parents_and_subject(&self, rev: &str) -> Result<(Vec<String>, String)> {
        let line = self.run(&["log", "-1", "--no-show-signature", "--format=%P%x00%s", rev])?;
        let (parents, subject) = line.split_once('\0').unwrap_or((&line, ""));
        Ok((
            parents.split_whitespace().map(str::to_string).collect(),
            subject.to_string(),
        ))
    }

    /// The files `commit` changes; empty for a merge.
    pub fn changed_files(&self, commit: &str) -> Result<Vec<String>> {
        let list = self.run(&[
            "diff-tree",
            "--no-commit-id",
            "--name-only",
            "-r",
            "-z",
            commit,
        ])?;
        Ok(list
            .split('\0')
            .filter(|file| !file.is_empty())
            .map(str::to_string)
            .collect())
    }

    /// Whether this repository has the commit `id`.
    pub fn has_commit(&self, id: &str) -> bool {
        self.run(&[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{id}^{{commit}}"),
        ])
        .is_ok()
    }

    pub fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool> {
        cmd::run_check(
            "git",
            &["merge-base", "--is-ancestor", ancestor, descendant],
            &self.root,
        )
    }

    pub fn delete_tag(&self, tag: &str) -> Result<()> {
        self.run(&["tag", "-d", tag]).map(drop)
    }

    /// Moves the branch, the index and the tracked files back to `commit`;
    /// stops instead of throwing a change away.
    pub fn reset_keep(&self, commit: &str) -> Result<()> {
        self.run(&["reset", "--keep", "--quiet", commit]).map(drop)
    }

    pub fn tag_exists(&self, tag: &str) -> bool {
        self.run(&[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/tags/{tag}"),
        ])
        .is_ok()
    }

    pub fn has_remote(&self, name: &str) -> Result<bool> {
        Ok(self.run(&["remote"])?.lines().any(|remote| remote == name))
    }

    /// The branch on `remote` that `branch` tracks, if it tracks one there.
    pub fn upstream(&self, branch: &str, remote: &str) -> Option<String> {
        let config = |key: &str| self.run(&["config", "--get", &format!("branch.{branch}.{key}")]);
        let tracked_remote = config("remote").ok()?;
        let merge = config("merge").ok()?;
        let name = merge.strip_prefix("refs/heads/").unwrap_or(&merge);
        (tracked_remote == remote).then(|| name.to_string())
    }

    /// The ones among `files` that git tracks.
    pub fn tracked(&self, files: &[&str]) -> Result<Vec<String>> {
        let mut tracked = Vec::new();
        for file in files {
            if !self.run(&["ls-files", "-z", "--", file])?.is_empty() {
                tracked.push(file.to_string());
            }
        }
        Ok(tracked)
    }

    /// Whether git ignores `path`. A tracked file is never ignored.
    pub fn is_ignored(&self, path: &str) -> Result<bool> {
        cmd::run_check("git", &["check-ignore", "-q", "--", path], &self.root)
    }

    /// Commits `files`, which git must track. `--update` stages them even inside an ignored directory.
    pub fn commit(&self, files: &[&str], message: &str) -> Result<()> {
        self.run(&[&["add", "--update", "--"], files].concat())?;
        self.run(&["commit", "--quiet", "-m", message]).map(drop)
    }

    /// Moves HEAD and the index back to `commit`; the working tree stays as it is.
    pub fn reset(&self, commit: &str) -> Result<()> {
        self.run(&["reset", "--quiet", commit]).map(drop)
    }

    pub fn tag(&self, tag: &str) -> Result<()> {
        self.run(&["tag", "-a", tag, "-m", tag]).map(drop)
    }

    pub fn push(&self, remote: &str, branch: &str, remote_branch: &str, tag: &str) -> Result<()> {
        let branch = format!("refs/heads/{branch}:refs/heads/{remote_branch}");
        let tag = format!("refs/tags/{tag}");
        self.run(&["push", "--atomic", "--quiet", remote, &branch, &tag])
            .map(drop)
    }

    fn run(&self, args: &[&str]) -> Result<String> {
        cmd::run("git", args, &self.root)
    }
}
