use std::collections::BTreeMap;
use std::fs::File;
use std::io::{ErrorKind, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};
use semver::Version;

use crate::git::Git;
use crate::manifest;

pub const FILE: &str = "CHANGELOG.md";

/// Conventional commit types: `feat` and `fix` make entries, the others are left out.
const TYPES: [&str; 11] = [
    "feat", "fix", "build", "chore", "ci", "docs", "perf", "refactor", "revert", "style", "test",
];

/// What `kraken changelog` writes.
pub const TEMPLATE: &str = "# Changelog\n\nAll notable changes to this project are documented in this file.\nThe format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).\n";

#[derive(Clone, Debug, PartialEq)]
pub struct Commit {
    pub subject: String,
    pub body: String,
}

/// What a release adds to the changelog: the commits since the latest tag, oldest first.
pub struct Changes {
    /// `YYYY-MM-DD`.
    pub date: String,
    pub commits: Vec<Commit>,
}

impl Changes {
    /// The section for `version`.
    pub fn section(&self, version: &Version) -> Section {
        let mut groups: BTreeMap<Group, Vec<Entry>> = BTreeMap::new();
        for (group, entry) in self.commits.iter().filter_map(entry) {
            groups.entry(group).or_default().push(entry);
        }
        Section {
            version: version.clone(),
            date: self.date.clone(),
            groups: groups.into_iter().collect(),
        }
    }
}

/// What a release adds to the changelog.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub version: Version,
    pub date: String,
    /// In the order they appear, each with its entries oldest first.
    pub groups: Vec<(Group, Vec<Entry>)>,
}

impl Section {
    pub fn markdown(&self) -> String {
        let mut text = format!("## [{}] - {}\n", self.version, self.date);
        for (group, entries) in &self.groups {
            text.push_str(&format!("\n### {}\n\n", group.title()));
            for entry in entries {
                match &entry.scope {
                    Some(scope) => text.push_str(&format!("- **{scope}**: {}\n", entry.text)),
                    None => text.push_str(&format!("- {}\n", entry.text)),
                }
            }
        }
        text
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub scope: Option<String>,
    pub text: String,
}

/// Changelog groups in the order they appear.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    Breaking,
    Added,
    Changed,
    Fixed,
}

impl Group {
    pub fn title(self) -> &'static str {
        match self {
            Group::Breaking => "Breaking",
            Group::Added => "Added",
            Group::Changed => "Changed",
            Group::Fixed => "Fixed",
        }
    }
}

/// The group and entry of `commit`; `None` leaves it out.
fn entry(commit: &Commit) -> Option<(Group, Entry)> {
    let Some(header) = Header::parse(&commit.subject) else {
        let entry = Entry {
            scope: None,
            text: commit.subject.trim().to_string(),
        };
        return Some((Group::Changed, entry));
    };
    let group = if header.breaking || has_breaking_footer(&commit.body) {
        Group::Breaking
    } else {
        match header.kind.to_ascii_lowercase().as_str() {
            "feat" => Group::Added,
            "fix" => Group::Fixed,
            _ => return None,
        }
    };
    let entry = Entry {
        scope: header.scope.map(str::to_string),
        text: header.description.to_string(),
    };
    Some((group, entry))
}

/// A conventional commit subject: `type(scope)!: description`.
struct Header<'a> {
    kind: &'a str,
    scope: Option<&'a str>,
    breaking: bool,
    description: &'a str,
}

impl Header<'_> {
    fn parse(subject: &str) -> Option<Header<'_>> {
        let (prefix, description) = subject.split_once(": ")?;
        let description = description.trim();
        let (prefix, breaking) = match prefix.strip_suffix('!') {
            Some(prefix) => (prefix, true),
            None => (prefix, false),
        };
        let (kind, scope) = match prefix.split_once('(') {
            Some((kind, rest)) => (kind, Some(rest.strip_suffix(')')?.trim())),
            None => (prefix, None),
        };
        let known = TYPES.iter().any(|known| known.eq_ignore_ascii_case(kind));
        if !known || description.is_empty() || scope.is_some_and(|s| s.contains(')')) {
            return None;
        }
        Some(Header {
            kind,
            scope: scope.filter(|scope| !scope.is_empty()),
            breaking,
            description,
        })
    }
}

fn has_breaking_footer(body: &str) -> bool {
    body.lines()
        .any(|line| line.starts_with("BREAKING CHANGE:") || line.starts_with("BREAKING-CHANGE:"))
}

/// `changelog` with `section` above the latest release and below an Unreleased section,
/// in the line endings of `changelog`.
pub fn insert(changelog: &str, section: &str) -> String {
    if changelog.trim().is_empty() {
        return format!("{TEMPLATE}\n{section}");
    }
    let newline = if changelog.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let section = section.replace('\n', newline);
    let mut offset = 0;
    for line in changelog.split_inclusive('\n') {
        if is_release_heading(line) {
            let (before, after) = changelog.split_at(offset);
            return format!("{before}{section}{newline}{after}");
        }
        offset += line.len();
    }
    format!("{}{newline}{newline}{section}", changelog.trim_end())
}

fn is_release_heading(line: &str) -> bool {
    line.strip_prefix("## ").is_some_and(|title| {
        let title = title.trim().trim_start_matches('[').trim_end_matches(']');
        !title.eq_ignore_ascii_case("unreleased")
    })
}

/// The date of a git ident such as `Name <email> 1762300800 +0100`, in its time zone.
pub fn ident_date(ident: &str) -> Result<String> {
    let mut parts = ident.rsplitn(3, ' ');
    let offset = parts.next().and_then(offset_seconds);
    let seconds = parts.next().and_then(|seconds| seconds.parse::<i64>().ok());
    let (Some(offset), Some(seconds)) = (offset, seconds) else {
        bail!("no date in the git ident \"{ident}\"");
    };
    Ok(civil_date((seconds + offset).div_euclid(86_400)))
}

/// Seconds east of UTC in a `+hhmm` or `-hhmm` offset.
fn offset_seconds(text: &str) -> Option<i64> {
    let (sign, digits) = text.split_at_checked(1)?;
    if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let hours: i64 = digits[..2].parse().ok()?;
    let minutes: i64 = digits[2..].parse().ok()?;
    let seconds = hours * 3600 + minutes * 60;
    match sign {
        "+" => Some(seconds),
        "-" => Some(-seconds),
        _ => None,
    }
}

/// `YYYY-MM-DD` of a day counted from 1970-01-01, by Howard Hinnant's `civil_from_days`.
fn civil_date(days: i64) -> String {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Whether `root` has CHANGELOG.md, named exactly so. A symlink does not count: git would
/// commit the link and leave out the file it points to.
pub fn is_in(root: &Path) -> bool {
    manifest::existing(root, FILE).is_some() && !root.join(FILE).is_symlink()
}

/// Creates CHANGELOG.md in the root of the repository at `dir`.
pub fn create(dir: &Path) -> Result<()> {
    let path = Git::discover(dir)?.root().join(FILE);
    let mut file = match File::create_new(&path) {
        Err(err) if err.kind() == ErrorKind::AlreadyExists => bail!("{FILE} already exists"),
        file => file.with_context(|| FILE)?,
    };
    file.write_all(TEMPLATE.as_bytes()).with_context(|| FILE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(subject: &str) -> Commit {
        Commit {
            subject: subject.to_string(),
            body: String::new(),
        }
    }

    fn section(commits: Vec<Commit>) -> String {
        let changes = Changes {
            date: "2026-11-04".to_string(),
            commits,
        };
        changes.section(&Version::new(1, 5, 0)).markdown()
    }

    #[test]
    fn groups_features_and_fixes() {
        let text = section(vec![
            commit("feat(ui): add the release panel"),
            commit("fix: keep comments in deno.jsonc"),
            commit("feat: read gradle.properties"),
        ]);

        assert_eq!(
            text,
            "## [1.5.0] - 2026-11-04\n\
             \n\
             ### Added\n\
             \n\
             - **ui**: add the release panel\n\
             - read gradle.properties\n\
             \n\
             ### Fixed\n\
             \n\
             - keep comments in deno.jsonc\n"
        );
    }

    #[test]
    fn leaves_out_other_conventional_types() {
        let text = section(vec![
            commit("chore(release): v1.4.0"),
            commit("docs: explain the panel"),
            commit("refactor(manifests): carry the path"),
            commit("ci: cache cargo"),
            commit("test: cover rollback"),
            commit("build: link statically"),
            commit("perf: read the lock once"),
            commit("revert: feat: add jsr.json"),
            commit("style: format"),
        ]);

        assert_eq!(text, "## [1.5.0] - 2026-11-04\n");
    }

    #[test]
    fn puts_other_commits_under_changed() {
        let text = section(vec![commit("Update README"), commit("feat: add VERSION")]);

        assert_eq!(
            text,
            "## [1.5.0] - 2026-11-04\n\n### Added\n\n- add VERSION\n\n### Changed\n\n- Update README\n"
        );
    }

    #[test]
    fn breaking_changes_come_first() {
        let footer = Commit {
            subject: "fix: read the version from the workspace".to_string(),
            body: "Explain why.\n\nBREAKING CHANGE: per-crate versions are gone".to_string(),
        };
        let text = section(vec![
            commit("feat: add jsr.json"),
            commit("feat(cli)!: rename --force to --yes"),
            footer,
            commit("chore!: drop Node 18"),
        ]);

        assert_eq!(
            text,
            "## [1.5.0] - 2026-11-04\n\
             \n\
             ### Breaking\n\
             \n\
             - **cli**: rename --force to --yes\n\
             - read the version from the workspace\n\
             - drop Node 18\n\
             \n\
             ### Added\n\
             \n\
             - add jsr.json\n"
        );
    }

    #[test]
    fn hyphenated_breaking_footer_counts() {
        let commit = Commit {
            subject: "feat: new config".to_string(),
            body: "BREAKING-CHANGE: the old one is gone".to_string(),
        };

        assert!(section(vec![commit]).contains("### Breaking\n\n- new config\n"));
    }

    #[test]
    fn type_is_case_insensitive() {
        assert!(
            section(vec![commit("Fix: crash on start")])
                .contains("### Fixed\n\n- crash on start\n")
        );
    }

    #[test]
    fn subjects_that_only_look_conventional_are_changed() {
        let text = section(vec![
            commit("Update docs: add a section"),
            commit("feat(ui: missing bracket"),
            commit("feat:no space"),
            commit("feat: "),
            commit("feat(a)b): stray bracket"),
            commit("parser: handle empty input"),
            commit("WIP: try a new layout"),
        ]);

        assert_eq!(
            text,
            "## [1.5.0] - 2026-11-04\n\
             \n\
             ### Changed\n\
             \n\
             - Update docs: add a section\n\
             - feat(ui: missing bracket\n\
             - feat:no space\n\
             - feat:\n\
             - feat(a)b): stray bracket\n\
             - parser: handle empty input\n\
             - WIP: try a new layout\n"
        );
    }

    #[test]
    fn empty_scope_is_no_scope() {
        assert!(section(vec![commit("feat(): add it")]).contains("- add it\n"));
    }

    const SECTION: &str = "## [1.5.0] - 2026-11-04\n\n### Added\n\n- a\n";

    #[test]
    fn inserts_above_the_latest_release() {
        let changelog = format!("{TEMPLATE}\n## [1.4.0] - 2026-10-01\n\n### Fixed\n\n- b\n");

        assert_eq!(
            insert(&changelog, SECTION),
            format!("{TEMPLATE}\n{SECTION}\n## [1.4.0] - 2026-10-01\n\n### Fixed\n\n- b\n")
        );
    }

    #[test]
    fn inserts_below_an_unreleased_section() {
        let changelog = "# Changelog\n\n## [Unreleased]\n\n- c\n\n## [1.4.0] - 2026-10-01\n";

        assert_eq!(
            insert(changelog, SECTION),
            format!(
                "# Changelog\n\n## [Unreleased]\n\n- c\n\n{SECTION}\n## [1.4.0] - 2026-10-01\n"
            )
        );
    }

    #[test]
    fn appends_to_a_changelog_without_releases() {
        assert_eq!(insert(TEMPLATE, SECTION), format!("{TEMPLATE}\n{SECTION}"));
        assert_eq!(
            insert("# Changelog\n\n\n", SECTION),
            format!("# Changelog\n\n{SECTION}")
        );
    }

    #[test]
    fn keeps_crlf_line_endings() {
        let changelog = "# Changelog\r\n\r\n## [1.4.0] - 2026-10-01\r\n";

        assert_eq!(
            insert(changelog, SECTION),
            format!(
                "# Changelog\r\n\r\n{}\r\n## [1.4.0] - 2026-10-01\r\n",
                SECTION.replace('\n', "\r\n")
            )
        );
    }

    #[test]
    fn fills_an_empty_file_with_the_template() {
        assert_eq!(insert("", SECTION), format!("{TEMPLATE}\n{SECTION}"));
        assert_eq!(insert("\n", SECTION), format!("{TEMPLATE}\n{SECTION}"));
    }

    #[test]
    fn reads_the_date_in_the_ident_time_zone() {
        let date = |ident: &str| ident_date(ident).unwrap();

        assert_eq!(date("A <a@b.c> 0 +0000"), "1970-01-01");
        assert_eq!(date("A <a@b.c> 1793831400 +0000"), "2026-11-04");
        assert_eq!(date("A <a@b.c> 1793831400 +0100"), "2026-11-04");
        assert_eq!(date("A <a@b.c> 1793831400 +0200"), "2026-11-05");
        assert_eq!(date("A <a@b.c> 1793836800 -0500"), "2026-11-04");
        assert_eq!(date("A <a@b.c> 1793836800 +0530"), "2026-11-05");
    }

    #[test]
    fn reads_leap_days() {
        assert_eq!(
            ident_date("A <a@b.c> 1709164800 +0000").unwrap(),
            "2024-02-29"
        );
        assert_eq!(
            ident_date("A <a@b.c> 951782400 +0000").unwrap(),
            "2000-02-29"
        );
    }

    #[test]
    fn refuses_a_broken_ident() {
        for ident in ["", "A <a@b.c>", "A <a@b.c> soon +0000", "A <a@b.c> 0 0100"] {
            assert!(ident_date(ident).is_err(), "{ident:?}");
        }
    }
}
