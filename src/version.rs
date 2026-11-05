use std::fmt;

use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use semver::{Prerelease, Version};

use crate::commit::Tally;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
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

/// The label of a pre-release, in release order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum Pre {
    Alpha,
    Beta,
    Rc,
}

impl fmt::Display for Pre {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.pad(match self {
            Pre::Alpha => "alpha",
            Pre::Beta => "beta",
            Pre::Rc => "rc",
        })
    }
}

fn bump(current: &Version, level: Level) -> Version {
    let (major, minor, patch) = (current.major, current.minor, current.patch);
    match level {
        Level::Patch => Version::new(major, minor, patch + 1),
        Level::Minor => Version::new(major, minor + 1, 0),
        Level::Major => Version::new(major + 1, 0, 0),
    }
}

/// The lowest part of the final version that is not zero; `None` for a final version.
pub fn pre_release_level(version: &Version) -> Option<Level> {
    if version.pre.is_empty() {
        None
    } else if version.patch > 0 {
        Some(Level::Patch)
    } else if version.minor > 0 {
        Some(Level::Minor)
    } else {
        Some(Level::Major)
    }
}

/// The label and number of a pre-release in the form `<label>.<number>`.
pub fn kraken_pre(version: &Version) -> Option<(Pre, u64)> {
    let (label, number) = version.pre.as_str().split_once('.')?;
    let label = Pre::value_variants()
        .iter()
        .copied()
        .find(|pre| pre.to_string() == label)?;
    Some((label, number.parse().ok()?))
}

/// The version after `current` at `level`, a pre-release when `pre` is set.
pub fn next(current: &Version, level: Level, pre: Option<Pre>) -> Result<Version> {
    let base = match pre_release_level(current) {
        Some(own) if level < own => {
            let levels: Vec<String> = Level::ALL
                .iter()
                .filter(|&&allowed| allowed >= own)
                .map(Level::to_string)
                .collect();
            let (last, first) = levels.split_last().expect("the own level is in the list");
            let choice = if first.is_empty() {
                last.clone()
            } else {
                format!("{} or {last}", first.join(", "))
            };
            bail!(
                "{current} leads to {}, a {own} release; pass {choice}",
                Version::new(current.major, current.minor, current.patch)
            );
        }
        Some(own) if level == own => Version::new(current.major, current.minor, current.patch),
        _ => bump(current, level),
    };
    let Some(label) = pre else {
        return Ok(base);
    };
    let same_release =
        (current.major, current.minor, current.patch) == (base.major, base.minor, base.patch);
    let number = match kraken_pre(current) {
        Some((current_label, number)) if same_release && current_label == label => number + 1,
        _ => 1,
    };
    let version = Version {
        pre: Prerelease::new(&format!("{label}.{number}"))?,
        ..base
    };
    if version <= *current {
        bail!("{version} is lower than {current}; a pre-release goes from alpha to beta to rc");
    }
    Ok(version)
}

/// The level the commits in `tally` call for; `None` without commits.
/// From a pre-release it is never below the pre-release's own level.
pub fn suggest(current: &Version, tally: &Tally) -> Option<Level> {
    let level = if tally.is_empty() {
        return None;
    } else if tally.breaking > 0 && current.major > 0 {
        Level::Major
    } else if tally.breaking > 0 || tally.features > 0 {
        Level::Minor
    } else {
        Level::Patch
    };
    Some(pre_release_level(current).map_or(level, |own| level.max(own)))
}

/// A warning when `level` is lower than `suggested`.
pub fn suggestion_warning(level: Level, suggested: Option<Level>) -> Option<String> {
    suggested
        .filter(|&suggested| level < suggested)
        .map(|suggested| format!("the new commits suggest a {suggested} release"))
}

/// Parses an `X.Y.Z` or `X.Y.Z-<pre-release>` version; build metadata is rejected.
pub fn parse_version(text: &str) -> Result<Version> {
    let version =
        Version::parse(text).with_context(|| format!("\"{text}\" is not an X.Y.Z version"))?;
    if !version.build.is_empty() {
        bail!("\"{text}\": build metadata is not supported");
    }
    Ok(version)
}

/// Parses a plain `X.Y.Z` version for a file that `what` limits to numbers.
pub fn parse_final(text: &str, what: &str) -> Result<Version> {
    let version = parse_version(text)?;
    if !version.pre.is_empty() {
        bail!("\"{text}\": {what} takes numbers only");
    }
    Ok(version)
}

/// The version of an `X.Y.Z` or `vX.Y.Z` tag, with or without a pre-release.
pub fn release_tag_version(tag: &str) -> Option<Version> {
    parse_version(tag.strip_prefix('v').unwrap_or(tag)).ok()
}

/// Highest release tag, pre-releases included; other tags are ignored.
pub fn latest_release_tag(tags: &[String]) -> Option<(&str, Version)> {
    release_tags(tags).pop()
}

/// Highest release tag that is not a pre-release.
pub fn latest_final_tag(tags: &[String]) -> Option<(&str, Version)> {
    release_tags(tags)
        .into_iter()
        .rfind(|(_, version)| version.pre.is_empty())
}

/// The release tags in version order, pre-releases included. With `1.2.0` and `v1.2.0`
/// only the `v` one stays, the one `git tag --list` sorts last.
pub fn release_tags(tags: &[String]) -> Vec<(&str, Version)> {
    let mut found: Vec<(&str, Version)> = tags
        .iter()
        .filter_map(|tag| release_tag_version(tag).map(|version| (tag.as_str(), version)))
        .collect();
    found.sort_by(|a, b| {
        a.1.cmp(&b.1)
            .then(a.0.starts_with('v').cmp(&b.0.starts_with('v')))
    });
    let mut kept: Vec<(&str, Version)> = Vec::new();
    for tag in found {
        match kept.last_mut() {
            Some(last) if last.1 == tag.1 => *last = tag,
            _ => kept.push(tag),
        }
    }
    kept
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

    fn tally(breaking: usize, features: usize, fixes: usize, other: usize) -> Tally {
        Tally {
            breaking,
            features,
            fixes,
            other,
        }
    }

    #[test]
    fn levels_are_ordered() {
        assert!(Level::Patch < Level::Minor);
        assert!(Level::Minor < Level::Major);
    }

    #[test]
    fn suggests_major_for_a_breaking_change() {
        assert_eq!(suggest(&v("1.4.2"), &tally(1, 2, 1, 0)), Some(Level::Major));
    }

    #[test]
    fn suggests_minor_for_a_feature() {
        assert_eq!(suggest(&v("1.4.2"), &tally(0, 1, 3, 2)), Some(Level::Minor));
    }

    #[test]
    fn suggests_patch_for_fixes_and_other_commits() {
        assert_eq!(suggest(&v("1.4.2"), &tally(0, 0, 1, 0)), Some(Level::Patch));
        assert_eq!(suggest(&v("1.4.2"), &tally(0, 0, 0, 2)), Some(Level::Patch));
    }

    #[test]
    fn suggests_minor_for_a_breaking_change_in_0x() {
        assert_eq!(suggest(&v("0.3.0"), &tally(1, 0, 0, 0)), Some(Level::Minor));
        assert_eq!(suggest(&v("0.0.4"), &tally(1, 0, 0, 0)), Some(Level::Minor));
    }

    #[test]
    fn suggests_nothing_without_commits() {
        assert_eq!(suggest(&v("1.4.2"), &Tally::default()), None);
    }

    #[test]
    fn warns_only_below_the_suggestion() {
        let minor = Some(Level::Minor);

        assert_eq!(
            suggestion_warning(Level::Patch, minor).as_deref(),
            Some("the new commits suggest a minor release")
        );
        assert_eq!(suggestion_warning(Level::Minor, minor), None);
        assert_eq!(suggestion_warning(Level::Major, minor), None);
        assert_eq!(suggestion_warning(Level::Patch, None), None);
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
        assert_eq!(parse_version("1.2.3").unwrap(), v("1.2.3"));
    }

    #[test]
    fn rejects_incomplete_version() {
        let err = parse_version("1.2").unwrap_err().to_string();
        assert!(err.contains("\"1.2\""), "{err}");
    }

    #[test]
    fn parses_pre_release_and_refuses_build_metadata() {
        assert_eq!(parse_version("1.2.3-rc.1").unwrap(), v("1.2.3-rc.1"));
        let err = parse_version("1.2.3+abc").unwrap_err().to_string();
        assert_eq!(err, "\"1.2.3+abc\": build metadata is not supported");
    }

    #[test]
    fn parse_final_refuses_a_pre_release() {
        assert_eq!(
            parse_final("1.2.3", "MARKETING_VERSION").unwrap(),
            v("1.2.3")
        );
        let err = parse_final("1.2.3-rc.1", "MARKETING_VERSION")
            .unwrap_err()
            .to_string();
        assert_eq!(err, "\"1.2.3-rc.1\": MARKETING_VERSION takes numbers only");
    }

    #[test]
    fn picks_highest_release_tag() {
        let list = tags(&[
            "v1.2.0",
            "v1.10.0",
            "v1.9.3",
            "nightly",
            "v2.0.0-rc.1",
            "v2.0.0-rc.10",
            "v2.0.0-rc.2",
            "v2.0.0+build",
        ]);
        assert_eq!(
            latest_release_tag(&list),
            Some(("v2.0.0-rc.10", v("2.0.0-rc.10")))
        );
        let list = tags(&["v1.4.0-rc.10", "v1.4.0", "v1.4.0-rc.2"]);
        assert_eq!(latest_release_tag(&list), Some(("v1.4.0", v("1.4.0"))));
    }

    #[test]
    fn release_tags_come_in_version_order() {
        let list = tags(&["v1.10.0", "v1.2.0", "nightly", "v2.0.0-rc.1", "1.9.3"]);
        assert_eq!(
            release_tags(&list),
            vec![
                ("v1.2.0", v("1.2.0")),
                ("1.9.3", v("1.9.3")),
                ("v1.10.0", v("1.10.0")),
                ("v2.0.0-rc.1", v("2.0.0-rc.1")),
            ]
        );
    }

    #[test]
    fn same_version_takes_the_v_tag() {
        for list in [tags(&["1.2.0", "v1.2.0"]), tags(&["v1.2.0", "1.2.0"])] {
            assert_eq!(release_tags(&list), vec![("v1.2.0", v("1.2.0"))]);
            assert_eq!(latest_release_tag(&list), Some(("v1.2.0", v("1.2.0"))));
            assert_eq!(latest_final_tag(&list), Some(("v1.2.0", v("1.2.0"))));
        }
    }

    #[test]
    fn latest_final_tag_skips_pre_releases() {
        let list = tags(&["v1.3.0", "v1.4.0-rc.2", "v1.2.0"]);
        assert_eq!(latest_final_tag(&list), Some(("v1.3.0", v("1.3.0"))));
        assert_eq!(latest_final_tag(&tags(&["v1.4.0-rc.2"])), None);
    }

    fn next_text(current: &str, level: Level, pre: Option<Pre>) -> String {
        match next(&v(current), level, pre) {
            Ok(version) => version.to_string(),
            Err(err) => err.to_string(),
        }
    }

    #[test]
    fn pre_release_starts_at_one() {
        let from = |level, pre| next_text("1.3.0", level, Some(pre));
        assert_eq!(from(Level::Minor, Pre::Rc), "1.4.0-rc.1");
        assert_eq!(from(Level::Patch, Pre::Beta), "1.3.1-beta.1");
        assert_eq!(from(Level::Major, Pre::Alpha), "2.0.0-alpha.1");
    }

    #[test]
    fn pre_release_counts_up() {
        let rc = |current| next_text(current, Level::Minor, Some(Pre::Rc));
        assert_eq!(rc("1.4.0-rc.1"), "1.4.0-rc.2");
        assert_eq!(rc("1.4.0-rc.9"), "1.4.0-rc.10");
    }

    #[test]
    fn pre_release_moves_from_beta_to_rc() {
        assert_eq!(
            next_text("1.4.0-beta.2", Level::Minor, Some(Pre::Rc)),
            "1.4.0-rc.1"
        );
        assert_eq!(
            next_text("1.4.0-alpha.3", Level::Minor, Some(Pre::Beta)),
            "1.4.0-beta.1"
        );
    }

    #[test]
    fn pre_release_does_not_go_back() {
        assert_eq!(
            next_text("1.4.0-rc.2", Level::Minor, Some(Pre::Beta)),
            "1.4.0-beta.1 is lower than 1.4.0-rc.2; a pre-release goes from alpha to beta to rc"
        );
    }

    #[test]
    fn own_level_gives_the_final_version() {
        assert_eq!(next_text("1.4.0-rc.2", Level::Minor, None), "1.4.0");
        assert_eq!(next_text("1.4.3-rc.1", Level::Patch, None), "1.4.3");
        assert_eq!(next_text("2.0.0-rc.1", Level::Major, None), "2.0.0");
        assert_eq!(next_text("1.4.0-SNAPSHOT", Level::Minor, None), "1.4.0");
    }

    #[test]
    fn higher_level_bumps_the_final_version() {
        assert_eq!(next_text("1.4.0-rc.2", Level::Major, None), "2.0.0");
        assert_eq!(next_text("1.4.3-rc.1", Level::Minor, None), "1.5.0");
        assert_eq!(
            next_text("1.4.0-rc.2", Level::Major, Some(Pre::Rc)),
            "2.0.0-rc.1"
        );
        assert_eq!(
            next_text("1.4.0-SNAPSHOT", Level::Minor, Some(Pre::Rc)),
            "1.4.0-rc.1"
        );
    }

    #[test]
    fn lower_level_than_the_pre_release_is_refused() {
        assert_eq!(
            next_text("1.4.0-rc.2", Level::Patch, None),
            "1.4.0-rc.2 leads to 1.4.0, a minor release; pass minor or major"
        );
        assert_eq!(
            next_text("2.0.0-rc.1", Level::Minor, None),
            "2.0.0-rc.1 leads to 2.0.0, a major release; pass major"
        );
    }

    #[test]
    fn final_versions_bump_as_before() {
        assert_eq!(next_text("1.4.2", Level::Patch, None), "1.4.3");
        assert_eq!(next_text("1.4.2", Level::Minor, None), "1.5.0");
    }

    #[test]
    fn pre_release_level_of_each_shape() {
        assert_eq!(pre_release_level(&v("1.4.3-rc.1")), Some(Level::Patch));
        assert_eq!(pre_release_level(&v("1.4.0-rc.1")), Some(Level::Minor));
        assert_eq!(pre_release_level(&v("2.0.0-rc.1")), Some(Level::Major));
        assert_eq!(pre_release_level(&v("1.4.0")), None);
    }

    #[test]
    fn other_pre_releases_start_again() {
        let rc = |current| next_text(current, Level::Minor, Some(Pre::Rc));
        assert_eq!(rc("1.4.0-SNAPSHOT"), "1.4.0-rc.1");
        assert_eq!(rc("1.4.0-rc"), "1.4.0-rc.1");
        assert_eq!(
            next_text("1.4.0-nightly.5", Level::Minor, Some(Pre::Beta)),
            "1.4.0-beta.1 is lower than 1.4.0-nightly.5; a pre-release goes from alpha to beta to rc"
        );
    }

    #[test]
    fn pre_labels_print_and_order() {
        assert_eq!(Pre::Rc.to_string(), "rc");
        assert!(Pre::Alpha < Pre::Beta && Pre::Beta < Pre::Rc);
    }

    #[test]
    fn pre_releases_follow_semver_order() {
        let order = [
            "1.3.0",
            "1.4.0-SNAPSHOT",
            "1.4.0-alpha.1",
            "1.4.0-beta.3",
            "1.4.0-rc",
            "1.4.0-rc.2",
            "1.4.0-rc.10",
            "1.4.0",
        ];
        assert!(order.windows(2).all(|pair| v(pair[0]) < v(pair[1])));
    }

    #[test]
    fn suggestion_is_never_below_the_pre_release_level() {
        assert_eq!(
            suggest(&v("1.4.0-rc.2"), &tally(0, 0, 1, 0)),
            Some(Level::Minor)
        );
        assert_eq!(
            suggest(&v("1.4.0-rc.2"), &tally(1, 0, 0, 0)),
            Some(Level::Major)
        );
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
