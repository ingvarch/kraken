use std::fmt;

use anyhow::{Context, Result, bail};
use semver::Version;

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Level {
    Patch,
    Minor,
    Major,
}

impl Level {
    pub const ALL: [Level; 3] = [Level::Patch, Level::Minor, Level::Major];
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.pad(match self {
            Level::Patch => "patch",
            Level::Minor => "minor",
            Level::Major => "major",
        })
    }
}

pub fn bump(current: &Version, level: Level) -> Version {
    let (major, minor, patch) = (current.major, current.minor, current.patch);
    match level {
        Level::Patch => Version::new(major, minor, patch + 1),
        Level::Minor => Version::new(major, minor + 1, 0),
        Level::Major => Version::new(major + 1, 0, 0),
    }
}

/// Parses a plain `X.Y.Z` version; pre-release and build metadata are rejected.
pub fn parse_release(text: &str) -> Result<Version> {
    let version =
        Version::parse(text).with_context(|| format!("\"{text}\" is not an X.Y.Z version"))?;
    if !version.pre.is_empty() {
        bail!("\"{text}\": pre-release versions are not supported");
    }
    if !version.build.is_empty() {
        bail!("\"{text}\": build metadata is not supported");
    }
    Ok(version)
}

/// The version of an `X.Y.Z` or `vX.Y.Z` tag.
pub fn release_tag_version(tag: &str) -> Option<Version> {
    parse_release(tag.strip_prefix('v').unwrap_or(tag)).ok()
}

/// Highest release tag; other tags are ignored.
pub fn latest_release_tag(tags: &[String]) -> Option<(&str, Version)> {
    tags.iter()
        .filter_map(|tag| release_tag_version(tag).map(|version| (tag.as_str(), version)))
        .max_by(|a, b| a.1.cmp(&b.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    fn tags(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn level_prints_as_cli_name() {
        assert_eq!(Level::Minor.to_string(), "minor");
    }

    #[test]
    fn bumps_each_level() {
        assert_eq!(bump(&v("1.4.2"), Level::Patch), v("1.4.3"));
        assert_eq!(bump(&v("1.4.2"), Level::Minor), v("1.5.0"));
        assert_eq!(bump(&v("1.4.2"), Level::Major), v("2.0.0"));
    }

    #[test]
    fn parses_plain_version() {
        assert_eq!(parse_release("1.2.3").unwrap(), v("1.2.3"));
    }

    #[test]
    fn rejects_incomplete_version() {
        let err = parse_release("1.2").unwrap_err().to_string();
        assert!(err.contains("\"1.2\""), "{err}");
    }

    #[test]
    fn rejects_pre_release_and_build() {
        let err = parse_release("1.2.3-rc.1").unwrap_err().to_string();
        assert!(err.contains("pre-release"), "{err}");
        let err = parse_release("1.2.3+abc").unwrap_err().to_string();
        assert!(err.contains("build metadata"), "{err}");
    }

    #[test]
    fn picks_highest_release_tag() {
        let list = tags(&["v1.2.0", "v1.10.0", "v1.9.3", "nightly", "v2.0.0-rc.1"]);
        assert_eq!(latest_release_tag(&list), Some(("v1.10.0", v("1.10.0"))));
    }

    #[test]
    fn accepts_tags_without_prefix() {
        let list = tags(&["0.3.0", "0.2.9"]);
        assert_eq!(latest_release_tag(&list), Some(("0.3.0", v("0.3.0"))));
    }

    #[test]
    fn no_release_tags() {
        assert_eq!(latest_release_tag(&tags(&["nightly"])), None);
        assert_eq!(latest_release_tag(&[]), None);
    }
}
