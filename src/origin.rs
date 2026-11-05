use std::fmt;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::cmd;

/// A repository on a host, such as `github.com/o/r`.
#[derive(Clone, Debug, PartialEq)]
pub struct Repo {
    pub host: String,
    pub path: String,
}

impl fmt::Display for Repo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}/{}", self.host, self.path)
    }
}

impl Repo {
    pub fn from_url(url: &str) -> Option<Repo> {
        let (authority, path) = if let Some((scheme, rest)) = url.split_once("://") {
            if !["https", "http", "ssh", "git", "git+ssh", "ssh+git"].contains(&scheme) {
                return None;
            }
            let (authority, path) = rest.split_once('/')?;
            let host = authority.rsplit('@').next()?.split(':').next()?;
            (host, path)
        } else {
            let (host, path) = scp_like(url)?;
            (
                host.rsplit('@').next()?,
                path.strip_prefix('/').unwrap_or(path),
            )
        };
        let path = path.strip_suffix('/').unwrap_or(path);
        let path = path.strip_suffix(".git").unwrap_or(path);
        let valid = !authority.is_empty()
            && !path.contains(['?', '#'])
            && path.split('/').count() >= 2
            && path.split('/').all(|part| !part.is_empty() && part != "-");
        valid.then(|| Repo {
            host: normal_host(authority),
            path: path.to_string(),
        })
    }

    pub fn owner_and_name(&self) -> Option<(&str, &str)> {
        let (owner, name) = self.path.split_once('/')?;
        (!name.contains('/')).then_some((owner, name))
    }
}

/// The host and path of `host:path`, which has its `:` before any `/`.
fn scp_like(url: &str) -> Option<(&str, &str)> {
    let (host, path) = url.split_once(':')?;
    (!host.contains('/')).then_some((host, path))
}

fn normal_host(host: &str) -> String {
    let host = host.to_ascii_lowercase();
    match host.as_str() {
        "ssh.github.com" => "github.com".to_string(),
        "altssh.gitlab.com" => "gitlab.com".to_string(),
        _ => host,
    }
}

/// Reads the repository behind `url`. An SSH host is looked up with `ssh -G`.
pub fn resolve(url: &str, dir: &Path) -> Option<Repo> {
    let repo = Repo::from_url(url)?;
    if !is_ssh(url) {
        return Some(repo);
    }
    let mut command = Command::new("ssh");
    command.args(["-G", &repo.host]).current_dir(dir);
    let hostname = cmd::run_with_limit(&mut command, Duration::from_secs(10))
        .ok()
        .and_then(|config| hostname_in(&config).map(normal_host));
    Some(Repo {
        host: hostname.unwrap_or(repo.host),
        path: repo.path,
    })
}

fn is_ssh(url: &str) -> bool {
    match url.split_once("://") {
        Some((scheme, _)) => ["ssh", "git+ssh", "ssh+git"].contains(&scheme),
        None => scp_like(url).is_some(),
    }
}

fn hostname_in(ssh_config: &str) -> Option<&str> {
    ssh_config.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        (words.next()? == "hostname")
            .then(|| words.next())
            .flatten()
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Site {
    GitHub,
    GitLab,
    Codeberg,
}

/// The web pages of a repository on a host kraken knows.
#[derive(Clone, Debug, PartialEq)]
pub struct Pages {
    site: Site,
    base: String,
}

impl Pages {
    pub fn of(repo: &Repo) -> Option<Pages> {
        let site = match repo.host.as_str() {
            "github.com" if repo.owner_and_name().is_some() => Site::GitHub,
            "codeberg.org" if repo.owner_and_name().is_some() => Site::Codeberg,
            "gitlab.com" => Site::GitLab,
            _ => return None,
        };
        Some(Pages {
            site,
            base: format!("https://{repo}"),
        })
    }

    /// The compare page from `from`, up to the two dots that start the other end.
    pub fn compare_prefix(&self, from: &str) -> String {
        let route = if self.site == Site::GitLab { "/-" } else { "" };
        format!("{}{route}/compare/{from}", self.base)
    }

    /// The compare page from `previous` to `tag`, or the page of `tag` for a first release.
    pub fn release_page(&self, previous: Option<&str>, tag: &str) -> String {
        match (previous, self.site) {
            (Some(previous), _) => format!("{}...{tag}", self.compare_prefix(previous)),
            (None, Site::GitLab) => format!("{}/-/tags/{tag}", self.base),
            (None, _) => format!("{}/releases/tag/{tag}", self.base),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(url: &str) -> Option<String> {
        Repo::from_url(url).map(|repo| repo.to_string())
    }

    #[test]
    fn reads_https_urls() {
        for url in [
            "https://github.com/o/r.git",
            "https://github.com/o/r",
            "https://github.com/o/r/",
            "http://github.com/o/r",
        ] {
            assert_eq!(repo(url).as_deref(), Some("github.com/o/r"), "{url}");
        }
        assert_eq!(
            repo("http://git.example.com:8080/team/app.git").as_deref(),
            Some("git.example.com/team/app")
        );
    }

    #[test]
    fn reads_ssh_urls() {
        for url in [
            "git@github.com:o/r.git",
            "github.com:o/r",
            "ssh://git@github.com/o/r.git",
            "ssh://git@github.com:22/o/r.git",
            "git+ssh://git@github.com/o/r",
            "git://github.com/o/r.git",
        ] {
            assert_eq!(repo(url).as_deref(), Some("github.com/o/r"), "{url}");
        }
        for url in [
            "git@github.com:o/r.git",
            "ssh://git@github.com/o/r.git",
            "git+ssh://git@github.com/o/r",
        ] {
            assert!(is_ssh(url), "{url}");
        }
        for url in ["https://github.com/o/r", "git://github.com/o/r.git"] {
            assert!(!is_ssh(url), "{url}");
        }
    }

    #[test]
    fn drops_credentials_and_port() {
        let found = Repo::from_url("https://user:token@github.com:443/o/r.git").unwrap();

        assert_eq!(found.to_string(), "github.com/o/r");
        let page = Pages::of(&found).unwrap().release_page(Some("v1"), "v2");
        assert!(!page.contains("user") && !page.contains("token"), "{page}");
    }

    #[test]
    fn host_name_has_no_case() {
        assert_eq!(
            repo("https://GitHub.COM/O/R").as_deref(),
            Some("github.com/O/R")
        );
    }

    #[test]
    fn ssh_github_com_is_github_com() {
        for url in [
            "ssh://git@ssh.github.com:443/o/r.git",
            "git@GitHub.com:o/r.git",
        ] {
            assert_eq!(Repo::from_url(url).unwrap().host, "github.com", "{url}");
        }
    }

    #[test]
    fn altssh_gitlab_com_is_gitlab_com() {
        let url = "ssh://git@altssh.gitlab.com:443/g/r.git";
        assert_eq!(Repo::from_url(url).unwrap().host, "gitlab.com");
    }

    #[test]
    fn keeps_dots_in_the_repository_name() {
        assert_eq!(
            repo("git@github.com:o/r.github.io.git").as_deref(),
            Some("github.com/o/r.github.io")
        );
    }

    #[test]
    fn reads_gitlab_subgroups() {
        let found = Repo::from_url("git@gitlab.com:g/sub/r.git").unwrap();

        assert_eq!(found.to_string(), "gitlab.com/g/sub/r");
        assert_eq!(found.owner_and_name(), None);
        assert_eq!(
            Repo::from_url("https://github.com/o/r")
                .unwrap()
                .owner_and_name(),
            Some(("o", "r"))
        );
    }

    #[test]
    fn needs_a_repository_path() {
        for url in [
            "https://github.com/o",
            "https://gitlab.com/g/r/-/tree/main",
            "https://github.com/o/r?x=1",
            "https://github.com/o/r#readme",
            "https://github.com//r",
        ] {
            assert_eq!(repo(url), None, "{url}");
        }
        let deep = Repo::from_url("https://github.com/o/r/tree/main").unwrap();
        assert_eq!(Pages::of(&deep), None);
    }

    #[test]
    fn refuses_local_paths() {
        for url in [
            "/srv/git/r.git",
            "../r.git",
            "./dir:o/r.git",
            "file:///srv/git/r.git",
            "ftp://github.com/o/r",
            "",
        ] {
            assert_eq!(repo(url), None, "{url:?}");
        }
    }

    #[test]
    fn reads_the_hostname_from_ssh_g() {
        assert_eq!(
            hostname_in("user git\nhostname github.com\nport 22\n"),
            Some("github.com")
        );
        assert_eq!(hostname_in("user git\nport 22\n"), None);
    }

    #[test]
    fn other_hosts_get_no_pages() {
        for url in [
            "https://github.example.com/o/r.git",
            "https://gitlab.example.com/g/r.git",
            "git@bitbucket.org:o/r.git",
            "git@work:o/r.git",
            "https://gitlab.com/r",
        ] {
            assert_eq!(
                Repo::from_url(url).and_then(|repo| Pages::of(&repo)),
                None,
                "{url}"
            );
        }
    }

    #[test]
    fn builds_the_pages_of_each_host() {
        let pages = |url: &str| Pages::of(&Repo::from_url(url).unwrap()).unwrap();
        let github = pages("git@github.com:o/r.git");
        let gitlab = pages("git@gitlab.com:g/sub/r.git");
        let codeberg = pages("https://codeberg.org/o/r.git");

        assert_eq!(
            github.release_page(Some("v1.3.0"), "v1.4.0"),
            "https://github.com/o/r/compare/v1.3.0...v1.4.0"
        );
        assert_eq!(
            github.release_page(None, "v1.0.0"),
            "https://github.com/o/r/releases/tag/v1.0.0"
        );
        assert_eq!(
            gitlab.release_page(Some("1.3.0"), "1.4.0"),
            "https://gitlab.com/g/sub/r/-/compare/1.3.0...1.4.0"
        );
        assert_eq!(
            gitlab.release_page(None, "1.0.0"),
            "https://gitlab.com/g/sub/r/-/tags/1.0.0"
        );
        assert_eq!(
            codeberg.release_page(Some("v1.3.0"), "v1.4.0"),
            "https://codeberg.org/o/r/compare/v1.3.0...v1.4.0"
        );
        assert_eq!(
            codeberg.release_page(None, "v1.0.0"),
            "https://codeberg.org/o/r/releases/tag/v1.0.0"
        );
        assert_eq!(
            github.compare_prefix("v1.3.0"),
            "https://github.com/o/r/compare/v1.3.0"
        );
    }
}
