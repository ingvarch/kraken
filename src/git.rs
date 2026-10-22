use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::cmd;

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

    pub fn tags(&self) -> Result<Vec<String>> {
        let list = self.run(&["tag", "--list"])?;
        Ok(list.lines().map(str::to_string).collect())
    }

    /// Tags on `remote`. Asks the remote over the network without prompting for credentials
    /// and gives up after a few seconds.
    pub fn remote_tags(&self, remote: &str) -> Result<Vec<String>> {
        let mut command = Command::new("git");
        command
            .args(["ls-remote", "--tags", "--refs", remote])
            .current_dir(&self.root)
            .env("GIT_TERMINAL_PROMPT", "0");
        let list = cmd::run_with_limit(&mut command, Duration::from_secs(10))?;
        Ok(list
            .lines()
            .filter_map(|line| line.split_once("\trefs/tags/"))
            .map(|(_, tag)| tag.to_string())
            .collect())
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
