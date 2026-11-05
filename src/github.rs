use std::borrow::Cow;
use std::io;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use anyhow::{Result, bail};

use crate::changelog::Section;
use crate::cmd;
use crate::origin::{self, Repo};

/// GitHub refuses a release body longer than this.
const MAX_NOTES_CHARS: usize = 125_000;
const CHECK_LIMIT: Duration = Duration::from_secs(10);
const CREATE_LIMIT: Duration = Duration::from_secs(30);

/// The GitHub release kraken makes for a new tag.
#[derive(Clone, Debug, PartialEq)]
pub struct GitHubRelease {
    pub repo: Repo,
    pub tag: String,
    pub notes: Section,
    /// False when a higher final release exists, so GitHub keeps that one as Latest.
    pub latest: bool,
    pub prerelease: bool,
}

impl GitHubRelease {
    /// `release v1.2.4 on github.com/o/r`, with `, not marked as latest` for an older release.
    pub fn describe(&self) -> String {
        let kind = if self.prerelease {
            "pre-release"
        } else {
            "release"
        };
        let older = if self.latest {
            ""
        } else {
            ", not marked as latest"
        };
        format!("{kind} {} on {}{older}", self.tag, self.repo)
    }

    /// The arguments after `gh`.
    pub fn args(&self, notes_file: &Path) -> Vec<String> {
        let mut args: Vec<String> = [
            "release",
            "create",
            &self.tag,
            "--repo",
            &self.repo.to_string(),
            "--verify-tag",
            "--title",
            &self.tag,
            "--notes-file",
            &notes_file.to_string_lossy(),
        ]
        .map(String::from)
        .into();
        if self.prerelease {
            args.push("--prerelease".to_string());
        } else if !self.latest {
            args.push("--latest=false".to_string());
        }
        args
    }

    /// The `gh` command to run by hand, quoted for any shell.
    pub fn retry_command(&self, notes_file: &Path) -> String {
        let args = self.args(notes_file);
        let quoted: Vec<_> = args.iter().map(|arg| shell_quote(arg)).collect();
        format!("gh {}", quoted.join(" "))
    }
}

fn shell_quote(arg: &str) -> Cow<'_, str> {
    let plain = |c: char| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c);
    if !arg.is_empty() && arg.chars().all(plain) {
        Cow::Borrowed(arg)
    } else {
        let escaped: String = arg
            .chars()
            .map(|c| match c {
                '\'' => "'\\''".to_string(),
                '\\' => "'\\\\'".to_string(),
                c => c.to_string(),
            })
            .collect();
        Cow::Owned(format!("'{escaped}'"))
    }
}

/// The GitHub repository origin pushes to at `url`.
pub fn repo(url: &str, dir: &Path) -> Result<Repo> {
    match origin::resolve(url, dir) {
        Some(repo) if repo.owner_and_name().is_some() => Ok(repo),
        _ => bail!(
            "--github-release needs origin on GitHub; origin pushes to {}",
            origin::without_userinfo(url)
        ),
    }
}

pub fn check_notes_length(notes: &str) -> Result<()> {
    let length = notes.chars().count();
    if length > MAX_NOTES_CHARS {
        bail!(
            "the GitHub release notes have {length} characters, more than the {MAX_NOTES_CHARS} GitHub takes; release without --github-release and make the GitHub release by hand"
        );
    }
    Ok(())
}

fn gh(args: &[String], dir: &Path, limit: Duration) -> Result<String> {
    let mut command = Command::new("gh");
    command
        .args(args)
        .current_dir(dir)
        .env("GH_PROMPT_DISABLED", "1");
    cmd::run_with_limit(&mut command, limit)
}

/// Asks GitHub through gh whether the token can see `repo`.
pub fn check(repo: &Repo, dir: &Path) -> Result<()> {
    let args = [
        "api",
        "--hostname",
        &repo.host,
        &format!("repos/{}", repo.path),
        "--silent",
    ]
    .map(String::from);
    gh(&args, dir, CHECK_LIMIT).map(drop).map_err(|err| {
        let missing = err.chain().any(|cause| {
            cause
                .downcast_ref::<io::Error>()
                .is_some_and(|io| io.kind() == io::ErrorKind::NotFound)
        });
        if missing {
            anyhow::anyhow!(
                "--github-release needs the GitHub CLI; install gh from https://cli.github.com"
            )
        } else {
            anyhow::anyhow!("gh cannot reach {repo}, so kraken changed nothing: {err:#}")
        }
    })
}

/// Runs `gh release create`; returns the last line gh printed, which is the release URL.
pub fn create(release: &GitHubRelease, notes_file: &Path, dir: &Path) -> Result<String> {
    let out = gh(&release.args(notes_file), dir, CREATE_LIMIT)?;
    Ok(out.lines().last().unwrap_or_default().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use semver::Version;

    fn release(latest: bool) -> GitHubRelease {
        GitHubRelease {
            repo: Repo {
                host: "github.com".to_string(),
                path: "demo/app".to_string(),
            },
            tag: "v1.2.4".to_string(),
            notes: Section::new(&Version::new(1, 2, 4), "2026-11-04", &[]),
            latest,
            prerelease: false,
        }
    }

    #[test]
    fn github_needs_owner_and_repo() {
        let dir = Path::new(".");
        for url in [
            "https://kraken:s3cret@github.com/demo/app.git",
            "http://git.example.com:8080/team/app.git",
        ] {
            assert!(repo(url, dir).is_ok(), "{url}");
        }
        for url in [
            "https://gitlab.com/group/sub/app.git",
            "https://github.com/demo",
            "/srv/git/app.git",
            "https://kraken:s3cret@example.com/app.git",
        ] {
            let err = repo(url, dir).unwrap_err().to_string();
            assert!(err.starts_with("--github-release needs origin on GitHub; origin pushes to "));
            assert!(!err.contains("s3cret"), "{err}");
        }
        let err = repo("https://kraken:s3cret@example.com/app.git", dir)
            .unwrap_err()
            .to_string();
        assert!(err.ends_with("https://example.com/app.git"), "{err}");
    }

    #[test]
    fn release_args_name_the_repo_tag_title_and_notes_file() {
        let args = release(true).args(Path::new("/r/.git/NOTES.md"));

        assert_eq!(
            args,
            [
                "release",
                "create",
                "v1.2.4",
                "--repo",
                "github.com/demo/app",
                "--verify-tag",
                "--title",
                "v1.2.4",
                "--notes-file",
                "/r/.git/NOTES.md"
            ]
        );
    }

    #[test]
    fn release_args_keep_an_older_release_off_latest() {
        let args = release(false).args(Path::new("/n.md"));

        assert_eq!(args.last().map(String::as_str), Some("--latest=false"));
    }

    #[test]
    fn retry_command_quotes_what_a_shell_would_split() {
        let command = release(false).retry_command(Path::new("/my repo/it's/NOTES.md"));

        assert_eq!(
            command,
            "gh release create v1.2.4 --repo github.com/demo/app --verify-tag --title v1.2.4 --notes-file '/my repo/it'\\''s/NOTES.md' --latest=false"
        );
    }

    #[test]
    fn retry_command_keeps_a_backslash_outside_the_quotes() {
        let command = release(true).retry_command(Path::new("/work/a\\/NOTES.md"));

        assert!(
            command.contains("--notes-file '/work/a'\\\\'/NOTES.md'"),
            "{command}"
        );
    }

    #[test]
    fn release_args_mark_a_pre_release() {
        let pre = GitHubRelease {
            tag: "v1.4.0-rc.1".to_string(),
            prerelease: true,
            ..release(true)
        };

        assert_eq!(pre.args(Path::new("/n.md")).last().unwrap(), "--prerelease");
        assert_eq!(
            pre.describe(),
            "pre-release v1.4.0-rc.1 on github.com/demo/app"
        );
    }

    #[test]
    fn describes_the_release() {
        assert_eq!(
            release(true).describe(),
            "release v1.2.4 on github.com/demo/app"
        );
        assert_eq!(
            release(false).describe(),
            "release v1.2.4 on github.com/demo/app, not marked as latest"
        );
    }

    #[test]
    fn refuses_notes_longer_than_github_takes() {
        assert!(check_notes_length(&"a".repeat(125_000)).is_ok());
        assert!(check_notes_length(&"é".repeat(125_000)).is_ok());

        let err = check_notes_length(&"a".repeat(125_001))
            .unwrap_err()
            .to_string();

        assert_eq!(
            err,
            "the GitHub release notes have 125001 characters, more than the 125000 GitHub takes; release without --github-release and make the GitHub release by hand"
        );
    }
}
