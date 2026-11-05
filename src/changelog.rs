use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{ErrorKind, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};
use semver::Version;

use crate::commit::{Commit, Kind};
use crate::git::Git;
use crate::manifest;
use crate::origin::{self, Pages};
use crate::release::ORIGIN;
use crate::version::release_tags;

pub const FILE: &str = "CHANGELOG.md";

macro_rules! kraken_line {
    () => {
        "<!-- kraken adds a section for every release. Delete this line to stop it. -->"
    };
}

/// Releases add sections only to a CHANGELOG.md with this line. GitHub does not show it.
pub const KRAKEN_LINE: &str = kraken_line!();

/// What `kraken changelog` writes.
pub const TEMPLATE: &str = concat!(
    "# Changelog\n\nAll notable changes to this project are documented in this file.\nThe format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).\n\n",
    kraken_line!(),
    "\n"
);

/// What `kraken changelog` did.
#[derive(Debug, PartialEq)]
pub enum Setup {
    /// With a section for each of `releases` past releases.
    Created { releases: usize },
    /// CHANGELOG.md was there already, with [`KRAKEN_LINE`].
    Ready,
}

/// What a release adds to the changelog: the commits since the latest tag, oldest first.
pub struct Changes {
    pub commits: Vec<Commit>,
}

impl Changes {
    /// The section for `version`, dated `date` (`YYYY-MM-DD`).
    pub fn section(&self, version: &Version, date: String) -> Section {
        Section::new(version, &date, &self.commits)
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
    /// The section for `version`, dated `date` (`YYYY-MM-DD`), of `commits` oldest first.
    pub fn new(version: &Version, date: &str, commits: &[Commit]) -> Section {
        let mut groups: BTreeMap<Group, Vec<Entry>> = BTreeMap::new();
        for (group, entry) in commits.iter().filter_map(entry) {
            groups.entry(group).or_default().push(entry);
        }
        Section {
            version: version.clone(),
            date: date.to_string(),
            groups: groups.into_iter().collect(),
        }
    }

    pub fn markdown(&self) -> String {
        let heading = format!("## [{}] - {}\n", self.version, self.date);
        match self.notes() {
            notes if notes.is_empty() => heading,
            notes => format!("{heading}\n{notes}"),
        }
    }

    /// The groups without the version heading; empty when there are no entries.
    pub fn notes(&self) -> String {
        let mut text = String::new();
        for (index, (group, entries)) in self.groups.iter().enumerate() {
            if index > 0 {
                text.push('\n');
            }
            text.push_str(&format!("### {}\n\n", group.title()));
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
    let group = match commit.kind() {
        Kind::Breaking => Group::Breaking,
        Kind::Feature => Group::Added,
        Kind::Fix => Group::Fixed,
        Kind::LeftOut => return None,
        Kind::Unconventional => Group::Changed,
    };
    let entry = match commit.header() {
        Some(header) => Entry {
            scope: header.scope.map(str::to_string),
            text: header.description.to_string(),
        },
        None => Entry {
            scope: None,
            text: commit.subject.trim().to_string(),
        },
    };
    Some((group, entry))
}

/// The link definitions at the end of CHANGELOG.md that a release sets, as whole lines.
#[derive(Clone, Debug, PartialEq)]
pub struct Links {
    /// The definition of the new version.
    pub release: String,
    /// The `[Unreleased]` definition moved to the new tag, spelled as in the file.
    pub unreleased: Option<String>,
}

/// The links a release sets in `changelog`; `None` when the file gets no links.
/// `previous` is the tag the section starts after, `tag` the new tag.
pub fn links(
    changelog: &str,
    pages: &Pages,
    version: &Version,
    previous: Option<&str>,
    tag: &str,
) -> Option<Links> {
    if !takes_links(changelog) {
        return None;
    }
    let unreleased = previous.and_then(|previous| {
        let from = format!("{}..", pages.compare_prefix(previous));
        changelog[link_block(changelog)..]
            .lines()
            .filter_map(definition)
            .find(|(label, _)| label.eq_ignore_ascii_case("unreleased"))
            .and_then(|(label, target)| {
                let rest = target.strip_prefix(&from)?;
                Some(format!("[{label}]: {}..{rest}", pages.compare_prefix(tag)))
            })
    });
    Some(Links {
        release: link_line(version, &pages.release_page(previous, tag)),
        unreleased,
    })
}

/// `[1.4.0]: url`
pub fn link_line(version: &Version, url: &str) -> String {
    format!("[{version}]: {url}")
}

/// `changelog` with `section` above the latest release and below an Unreleased section,
/// or above the link definitions at the end when there is no release, then `links` set,
/// in the line endings of `changelog`.
pub fn insert(changelog: &str, section: &str, links: Option<&Links>) -> String {
    let newline = if changelog.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let section = section.replace('\n', newline);
    let mut offset = 0;
    let mut text = None;
    for line in changelog.split_inclusive('\n') {
        if is_release_heading(line) {
            let (before, after) = changelog.split_at(offset);
            text = Some(format!("{before}{section}{newline}{after}"));
            break;
        }
        offset += line.len();
    }
    let text = text.unwrap_or_else(|| {
        let (before, block) = changelog.split_at(link_block(changelog));
        let gap = if block.is_empty() { "" } else { newline };
        format!(
            "{}{newline}{newline}{section}{gap}{block}",
            before.trim_end()
        )
    });
    match links {
        Some(links) => set_links(&text, links, newline),
        None => text,
    }
}

/// `text` with the new definition above the first version definition of its link block,
/// else below `[Unreleased]`, else at the end; and the `[Unreleased]` definition moved.
fn set_links(text: &str, links: &Links, newline: &str) -> String {
    let (head, block) = text.split_at(link_block(text));
    let mut lines: Vec<String> = block.split_inclusive('\n').map(str::to_string).collect();
    let label = |line: &String| definition(line).map(|(label, _)| label.to_string());
    let unreleased = lines
        .iter()
        .position(|line| label(line).is_some_and(|l| l.eq_ignore_ascii_case("unreleased")));
    if let (Some(index), Some(moved)) = (unreleased, &links.unreleased) {
        let ending = &lines[index][lines[index].trim_end().len()..];
        lines[index] = format!("{moved}{ending}");
    }
    let at = lines
        .iter()
        .position(|line| label(line).is_some_and(|l| is_version(&l)))
        .or(unreleased.map(|index| index + 1))
        .or(lines
            .iter()
            .rposition(|line| label(line).is_some())
            .map(|index| index + 1))
        .unwrap_or(lines.len());
    if lines.is_empty() {
        return format!("{head}{newline}{}{newline}", links.release);
    }
    if at > 0 && !lines[at - 1].ends_with('\n') {
        lines[at - 1].push_str(newline);
    }
    lines.insert(at, format!("{}{newline}", links.release));
    format!("{head}{}", lines.concat())
}

fn is_version(label: &str) -> bool {
    Version::parse(label.strip_prefix('v').unwrap_or(label)).is_ok()
}

/// Whether releases set links: the link block has a version, or there is no release yet.
fn takes_links(changelog: &str) -> bool {
    changelog[link_block(changelog)..]
        .lines()
        .filter_map(definition)
        .any(|(label, _)| is_version(label))
        || !changelog.lines().any(is_release_heading)
}

/// Where the run of link definitions at the end starts; blank lines may sit between them.
/// `changelog.len()` without one.
fn link_block(changelog: &str) -> usize {
    let mut start = changelog.len();
    let mut end = changelog.len();
    for line in changelog.split_inclusive('\n').rev() {
        end -= line.len();
        if definition(line).is_some() {
            start = end;
        } else if !line.trim().is_empty() {
            break;
        }
    }
    start
}

/// The label and target of a link definition such as `[1.4.0]: https://...`; not indented.
fn definition(line: &str) -> Option<(&str, &str)> {
    let (label, target) = line.trim_end().strip_prefix('[')?.split_once("]:")?;
    let target = target.trim();
    (!label.is_empty() && !label.contains(['[', ']']) && !target.is_empty())
        .then_some((label, target))
}

fn is_release_heading(line: &str) -> bool {
    line.strip_prefix("## ").is_some_and(|title| {
        let title = title.trim().trim_start_matches('[').trim_end_matches(']');
        !title.eq_ignore_ascii_case("unreleased")
    })
}

/// [`TEMPLATE`] with `sections`, newest first, and the link definitions `links` at the end.
pub fn history(sections: &[Section], links: &[String]) -> String {
    let mut text = TEMPLATE.to_string();
    for section in sections {
        text.push('\n');
        text.push_str(&section.markdown());
    }
    if !links.is_empty() {
        text.push('\n');
        for link in links {
            text.push_str(link);
            text.push('\n');
        }
    }
    text
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

/// Whether releases add sections to CHANGELOG.md in `root`. Fails on a file with
/// [`KRAKEN_LINE`] that is not UTF-8, since kraken cannot write to it.
pub fn is_on(root: &Path) -> Result<bool> {
    if !is_regular(root) {
        return Ok(false);
    }
    let bytes = fs::read(root.join(FILE)).context(FILE)?;
    if !has_kraken_line(&bytes) {
        return Ok(false);
    }
    std::str::from_utf8(&bytes).with_context(|| format!("{FILE} is not UTF-8"))?;
    Ok(true)
}

/// Whether `root` has CHANGELOG.md, named exactly so. A symlink does not count: git would
/// commit the link and leave out the file it points to.
fn is_regular(root: &Path) -> bool {
    manifest::existing(root, FILE).is_some() && !root.join(FILE).is_symlink()
}

/// An indented line does not count.
fn has_kraken_line(text: &[u8]) -> bool {
    let text = text.strip_prefix("\u{feff}".as_bytes()).unwrap_or(text);
    text.split(|&byte| byte == b'\n')
        .any(|line| line.trim_ascii_end() == KRAKEN_LINE.as_bytes())
}

/// The text of a new CHANGELOG.md and the number of releases in it: a section for every
/// final release tag reachable from HEAD, with links on a known host. Reads git only.
fn new_file(git: &Git) -> Result<(String, usize)> {
    if git.head().is_err() {
        return Ok((TEMPLATE.to_string(), 0));
    }
    git.ensure_not_shallow()?;
    let dates = git.merged_tag_dates()?;
    let names: Vec<String> = dates.iter().map(|(name, _)| name.clone()).collect();
    let releases: Vec<(&str, Version)> = release_tags(&names)
        .into_iter()
        .filter(|(_, version)| version.pre.is_empty())
        .collect();
    if releases.is_empty() {
        return Ok((TEMPLATE.to_string(), 0));
    }
    let pages = if git.has_remote(ORIGIN)? {
        origin::resolve(&git.push_url(ORIGIN)?, git.root())
            .as_ref()
            .and_then(Pages::of)
    } else {
        None
    };
    let mut sections = Vec::new();
    let mut links = Vec::new();
    let mut below = None;
    for (tag, version) in &releases {
        let date = dates
            .iter()
            .find_map(|(name, date)| (name == tag).then_some(date))
            .expect("the tag comes from the dates");
        sections.push(Section::new(version, date, &git.commits_of(below, tag)?));
        if let Some(pages) = &pages {
            links.push(link_line(version, &pages.release_page(below, tag)));
        }
        below = Some(*tag);
    }
    sections.reverse();
    links.reverse();
    Ok((history(&sections, &links), releases.len()))
}

/// Creates CHANGELOG.md in the root of the repository at `dir`, or checks the one there.
pub fn create(dir: &Path) -> Result<Setup> {
    let git = Git::discover(dir)?;
    let root = git.root();
    if root.join(FILE).symlink_metadata().is_err() {
        let (text, releases) = new_file(&git)?;
        match File::create_new(root.join(FILE)) {
            Ok(mut file) => {
                file.write_all(text.as_bytes()).with_context(|| FILE)?;
                return Ok(Setup::Created { releases });
            }
            Err(err) if err.kind() != ErrorKind::AlreadyExists => return Err(err).context(FILE),
            Err(_) => {}
        }
    }
    if !is_regular(root) {
        bail!(
            "{FILE} is a symlink or is spelled in another case; kraken writes only to a regular file named exactly {FILE}"
        );
    }
    if !is_on(root)? {
        bail!(
            "{FILE} already exists. To have every release add a section to it, add this line on a line of its own, not indented:\n\n{KRAKEN_LINE}"
        );
    }
    Ok(Setup::Ready)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::origin::Repo;

    fn commit(subject: &str) -> Commit {
        Commit {
            subject: subject.to_string(),
            body: String::new(),
        }
    }

    fn section(commits: Vec<Commit>) -> String {
        let changes = Changes { commits };
        changes
            .section(&Version::new(1, 5, 0), "2026-11-04".to_string())
            .markdown()
    }

    fn release(version: Version, date: &str, subjects: &[&str]) -> Section {
        let commits: Vec<Commit> = subjects.iter().map(|subject| commit(subject)).collect();
        Section::new(&version, date, &commits)
    }

    #[test]
    fn section_new_groups_like_a_release() {
        let commits = vec![commit("fix: a"), commit("feat(ui): b"), commit("chore: c")];

        let direct = Section::new(&Version::new(1, 5, 0), "2026-11-04", &commits);

        let through_changes =
            Changes { commits }.section(&Version::new(1, 5, 0), "2026-11-04".to_string());
        assert_eq!(direct, through_changes);
        assert_eq!(
            direct.markdown(),
            "## [1.5.0] - 2026-11-04\n\n### Added\n\n- **ui**: b\n\n### Fixed\n\n- a\n"
        );
    }

    #[test]
    fn history_puts_sections_newest_first_below_the_header() {
        let sections = [
            release(
                Version::new(1, 1, 0),
                "2026-10-01",
                &["feat(cli): add a command"],
            ),
            release(Version::new(1, 0, 0), "2026-09-01", &["init"]),
        ];

        assert_eq!(
            history(&sections, &[]),
            format!(
                "{TEMPLATE}\n\
                 ## [1.1.0] - 2026-10-01\n\
                 \n\
                 ### Added\n\
                 \n\
                 - **cli**: add a command\n\
                 \n\
                 ## [1.0.0] - 2026-09-01\n\
                 \n\
                 ### Changed\n\
                 \n\
                 - init\n"
            )
        );
    }

    #[test]
    fn history_ends_with_a_link_per_release() {
        let sections = [
            release(Version::new(1, 1, 0), "2026-10-01", &[]),
            release(Version::new(1, 0, 0), "2026-09-01", &[]),
        ];
        let links = [
            "[1.1.0]: https://github.com/o/r/compare/v1.0.0...v1.1.0".to_string(),
            "[1.0.0]: https://github.com/o/r/releases/tag/v1.0.0".to_string(),
        ];

        assert_eq!(
            history(&sections, &links),
            format!(
                "{TEMPLATE}\n\
                 ## [1.1.0] - 2026-10-01\n\
                 \n\
                 ## [1.0.0] - 2026-09-01\n\
                 \n\
                 [1.1.0]: https://github.com/o/r/compare/v1.0.0...v1.1.0\n\
                 [1.0.0]: https://github.com/o/r/releases/tag/v1.0.0\n"
            )
        );
    }

    #[test]
    fn history_without_releases_is_the_template() {
        assert_eq!(history(&[], &[]), TEMPLATE);
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
    fn notes_leave_out_the_version_heading() {
        let changes = Changes {
            commits: vec![
                commit("feat(ui): add the release panel"),
                commit("fix: keep comments in deno.jsonc"),
                commit("feat: read gradle.properties"),
            ],
        };
        let notes = changes
            .section(&Version::new(1, 5, 0), "2026-11-04".to_string())
            .notes();

        assert_eq!(
            notes,
            "### Added\n\
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
    fn notes_of_a_section_without_entries_are_empty() {
        let changes = Changes {
            commits: vec![commit("docs: explain the panel")],
        };

        let section = changes.section(&Version::new(1, 5, 0), "2026-11-04".to_string());

        assert_eq!(section.notes(), "");
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
            insert(&changelog, SECTION, None),
            format!("{TEMPLATE}\n{SECTION}\n## [1.4.0] - 2026-10-01\n\n### Fixed\n\n- b\n")
        );
    }

    #[test]
    fn inserts_below_an_unreleased_section() {
        let changelog = "# Changelog\n\n## [Unreleased]\n\n- c\n\n## [1.4.0] - 2026-10-01\n";

        assert_eq!(
            insert(changelog, SECTION, None),
            format!(
                "# Changelog\n\n## [Unreleased]\n\n- c\n\n{SECTION}\n## [1.4.0] - 2026-10-01\n"
            )
        );
    }

    #[test]
    fn appends_to_a_changelog_without_releases() {
        assert_eq!(
            insert(TEMPLATE, SECTION, None),
            format!("{TEMPLATE}\n{SECTION}")
        );
        assert_eq!(
            insert("# Changelog\n\n\n", SECTION, None),
            format!("# Changelog\n\n{SECTION}")
        );
    }

    const HOST: &str = "https://github.com/o/r";

    fn pages() -> Pages {
        Pages::of(&Repo::from_url("https://github.com/o/r").unwrap()).unwrap()
    }

    fn links_of(changelog: &str, previous: Option<&str>) -> Option<Links> {
        links(
            changelog,
            &pages(),
            &Version::new(1, 5, 0),
            previous,
            "v1.5.0",
        )
    }

    fn set(changelog: &str, previous: Option<&str>) -> String {
        let links = links_of(changelog, previous);
        insert(changelog, SECTION, links.as_ref())
    }

    #[test]
    fn appends_above_the_link_block() {
        let changelog = format!("{TEMPLATE}\n[Unreleased]: {HOST}/compare/v1.0.0...HEAD\n");

        assert_eq!(
            insert(&changelog, SECTION, None),
            format!("{TEMPLATE}\n{SECTION}\n[Unreleased]: {HOST}/compare/v1.0.0...HEAD\n")
        );
    }

    #[test]
    fn starts_links_in_a_changelog_without_releases() {
        assert_eq!(
            set(TEMPLATE, None),
            format!("{TEMPLATE}\n{SECTION}\n[1.5.0]: {HOST}/releases/tag/v1.5.0\n")
        );
    }

    #[test]
    fn adds_the_link_above_the_latest_release_link() {
        let changelog = format!(
            "# Changelog\n\n## [1.4.0] - 2026-10-01\n\n- b\n\n[1.4.0]: {HOST}/compare/v1.3.0...v1.4.0\n[1.3.0]: {HOST}/compare/v1.2.0...v1.3.0\n"
        );

        assert_eq!(
            set(&changelog, Some("v1.4.0")),
            format!(
                "# Changelog\n\n{SECTION}\n## [1.4.0] - 2026-10-01\n\n- b\n\n[1.5.0]: {HOST}/compare/v1.4.0...v1.5.0\n[1.4.0]: {HOST}/compare/v1.3.0...v1.4.0\n[1.3.0]: {HOST}/compare/v1.2.0...v1.3.0\n"
            )
        );
    }

    #[test]
    fn adds_the_link_below_the_unreleased_link() {
        let changelog = format!(
            "# Changelog\n\n## [1.4.0] - 2026-10-01\n\n[1.0.0]: x\n\n[Unreleased]: {HOST}/compare/v1.4.0...HEAD\n"
        );
        let only_unreleased = format!(
            "# Changelog\n\n## [Unreleased]\n\n[1.4.0]: {HOST}/releases/tag/v1.4.0\n[Unreleased]: {HOST}/compare/v1.4.0...HEAD\n"
        );
        let links = links_of(&only_unreleased, Some("v1.4.0")).unwrap();
        let text = insert(&only_unreleased, SECTION, Some(&links));

        assert!(text.ends_with(&format!(
            "[1.5.0]: {HOST}/compare/v1.4.0...v1.5.0\n[1.4.0]: {HOST}/releases/tag/v1.4.0\n[Unreleased]: {HOST}/compare/v1.5.0...HEAD\n"
        )), "{text}");
        let bare = "# Changelog\n\n[Unreleased]: u\n";
        assert_eq!(
            set(bare, None),
            format!(
                "# Changelog\n\n{SECTION}\n[Unreleased]: u\n[1.5.0]: {HOST}/releases/tag/v1.5.0\n"
            )
        );
        let with_other = "# Changelog\n\n[Unreleased]: u\n[docs]: d\n";
        assert_eq!(
            set(with_other, None),
            format!(
                "# Changelog\n\n{SECTION}\n[Unreleased]: u\n[1.5.0]: {HOST}/releases/tag/v1.5.0\n[docs]: d\n"
            )
        );
        assert!(links_of(&changelog, Some("v1.4.0")).is_some());
    }

    #[test]
    fn moves_the_unreleased_link_to_the_new_tag() {
        let changelog = format!(
            "# Changelog\n\n## [1.4.0] - 2026-10-01\n\n[unreleased]: {HOST}/compare/v1.4.0...main\n[1.4.0]: {HOST}/releases/tag/v1.4.0\n"
        );

        let links = links_of(&changelog, Some("v1.4.0")).unwrap();

        assert_eq!(
            links.unreleased.as_deref(),
            Some(format!("[unreleased]: {HOST}/compare/v1.5.0...main").as_str())
        );
        assert_eq!(
            set(&changelog, Some("v1.4.0")),
            format!(
                "# Changelog\n\n{SECTION}\n## [1.4.0] - 2026-10-01\n\n[unreleased]: {HOST}/compare/v1.5.0...main\n[1.5.0]: {HOST}/compare/v1.4.0...v1.5.0\n[1.4.0]: {HOST}/releases/tag/v1.4.0\n"
            )
        );
    }

    #[test]
    fn leaves_an_unreleased_link_from_another_tag_alone() {
        for target in [
            format!("{HOST}/compare/v1.3.0...HEAD"),
            format!("{HOST}/compare/v1.4.01...HEAD"),
            format!("{HOST}/commits/main"),
        ] {
            let changelog = format!(
                "## [1.4.0] - x\n\n[Unreleased]: {target}\n[1.4.0]: {HOST}/releases/tag/v1.4.0\n"
            );

            let links = links_of(&changelog, Some("v1.4.0")).unwrap();

            assert_eq!(links.unreleased, None, "{target}");
            assert!(set(&changelog, Some("v1.4.0")).contains(&format!("[Unreleased]: {target}\n")));
        }
    }

    #[test]
    fn a_first_release_leaves_the_unreleased_link_alone() {
        let changelog = format!("# Changelog\n\n[Unreleased]: {HOST}/compare/v1.0.0...HEAD\n");

        assert_eq!(links_of(&changelog, None).unwrap().unreleased, None);
    }

    #[test]
    fn adds_no_links_to_a_changelog_with_releases_and_no_links() {
        let changelog = "# Changelog\n\n## [1.4.0] - 2026-10-01\n\n- b\n";
        let unreleased_only = format!("{changelog}\n[Unreleased]: {HOST}/compare/v1.4.0...HEAD\n");

        assert_eq!(links_of(changelog, Some("v1.4.0")), None);
        assert_eq!(links_of(&unreleased_only, Some("v1.4.0")), None);
    }

    #[test]
    fn version_links_count_with_or_without_v() {
        for label in ["1.4.0", "v1.4.0"] {
            let changelog = format!("## [1.4.0] - x\n\n[{label}]: {HOST}/releases/tag/v1.4.0\n");

            assert!(takes_links(&changelog), "{label}");
        }
        assert!(!takes_links(
            "## [1.4.0] - x\n\n[docs]: https://example.com\n"
        ));
    }

    #[test]
    fn finds_the_link_block_across_blank_lines() {
        let tail = |text: &str| text[link_block(text)..].to_string();

        assert_eq!(tail("text\n\n[a]: x\n\n[b]: y\n"), "[a]: x\n\n[b]: y\n");
        assert_eq!(tail("text\n[a]: x\n  [b]: y\n"), "");
        assert_eq!(tail("[a]: x\ntext\n[b]: y"), "[b]: y");
        assert_eq!(tail("text\n"), "");
        assert_eq!(tail("[a]: x\n\n\n"), "[a]: x\n\n\n");
    }

    #[test]
    fn reads_a_definition() {
        assert_eq!(
            definition("[1.0.0]: https://x/y\r\n"),
            Some(("1.0.0", "https://x/y"))
        );
        assert_eq!(definition(" [a]: x"), None);
        assert_eq!(definition("[a] x"), None);
        assert_eq!(definition("[]: x"), None);
        assert_eq!(definition("[a]:"), None);
    }

    #[test]
    fn keeps_crlf_line_endings_in_links() {
        let changelog = format!(
            "# Changelog\r\n\r\n## [1.4.0] - x\r\n\r\n[Unreleased]: {HOST}/compare/v1.4.0...HEAD\r\n[1.4.0]: {HOST}/releases/tag/v1.4.0\r\n"
        );

        let text = set(&changelog, Some("v1.4.0"));

        assert!(text.ends_with(&format!(
            "[Unreleased]: {HOST}/compare/v1.5.0...HEAD\r\n[1.5.0]: {HOST}/compare/v1.4.0...v1.5.0\r\n[1.4.0]: {HOST}/releases/tag/v1.4.0\r\n"
        )), "{text:?}");
        assert_eq!(text.matches('\n').count(), text.matches("\r\n").count());
    }

    #[test]
    fn ends_a_last_line_without_a_newline() {
        let changelog = format!("# Changelog\n\n[Unreleased]: {HOST}/x");

        assert_eq!(
            set(&changelog, None),
            format!(
                "# Changelog\n\n{SECTION}\n[Unreleased]: {HOST}/x\n[1.5.0]: {HOST}/releases/tag/v1.5.0\n"
            )
        );
    }

    #[test]
    fn keeps_crlf_line_endings() {
        let changelog = "# Changelog\r\n\r\n## [1.4.0] - 2026-10-01\r\n";

        assert_eq!(
            insert(changelog, SECTION, None),
            format!(
                "# Changelog\r\n\r\n{}\r\n## [1.4.0] - 2026-10-01\r\n",
                SECTION.replace('\n', "\r\n")
            )
        );
    }

    #[test]
    fn finds_the_kraken_line() {
        let has = |text: &str| has_kraken_line(text.as_bytes());
        assert!(has(TEMPLATE));
        assert!(has(&format!("# Changelog\r\n\r\n{KRAKEN_LINE}\r\n")));
        assert!(has(&format!("{KRAKEN_LINE}  \n")));
        assert!(has(&format!("\u{feff}{KRAKEN_LINE}\n# Changelog\n")));
        assert!(!has("# Changelog\n"));
        assert!(!has(&format!("Add `{KRAKEN_LINE}` to it.\n")));
        assert!(!has(&format!("Add it:\n\n    {KRAKEN_LINE}\n")));
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
