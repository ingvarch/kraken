/// Conventional commit types: `feat` and `fix` make entries, the others are left out.
const TYPES: [&str; 11] = [
    "feat", "fix", "build", "chore", "ci", "docs", "perf", "refactor", "revert", "style", "test",
];

#[derive(Clone, Debug, PartialEq)]
pub struct Commit {
    pub subject: String,
    pub body: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Breaking,
    Feature,
    Fix,
    LeftOut,
    Unconventional,
}

impl Commit {
    pub fn header(&self) -> Option<Header<'_>> {
        Header::parse(&self.subject)
    }

    pub fn kind(&self) -> Kind {
        let Some(header) = self.header() else {
            return Kind::Unconventional;
        };
        if header.breaking || has_breaking_footer(&self.body) {
            return Kind::Breaking;
        }
        match header.kind.to_ascii_lowercase().as_str() {
            "feat" => Kind::Feature,
            "fix" => Kind::Fix,
            _ => Kind::LeftOut,
        }
    }
}

/// A conventional commit subject: `type(scope)!: description`.
pub struct Header<'a> {
    kind: &'a str,
    pub scope: Option<&'a str>,
    breaking: bool,
    pub description: &'a str,
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

/// The commits since a tag, counted by kind.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tally {
    pub breaking: usize,
    pub features: usize,
    pub fixes: usize,
    pub other: usize,
}

impl Tally {
    pub fn of(commits: &[Commit]) -> Tally {
        let mut tally = Tally::default();
        for commit in commits {
            match commit.kind() {
                Kind::Breaking => tally.breaking += 1,
                Kind::Feature => tally.features += 1,
                Kind::Fix => tally.fixes += 1,
                Kind::LeftOut | Kind::Unconventional => tally.other += 1,
            }
        }
        tally
    }

    pub fn is_empty(&self) -> bool {
        *self == Tally::default()
    }

    /// The `Changes` value, such as `2 features, 1 fix since v1.4.2`.
    pub fn summary(&self, tag: Option<&str>) -> String {
        let since = tag.map_or("the first commit", |tag| tag);
        let counts = [
            (self.breaking, "breaking change", "breaking changes"),
            (self.features, "feature", "features"),
            (self.fixes, "fix", "fixes"),
            (self.other, "other commit", "other commits"),
        ];
        let parts: Vec<String> = counts
            .iter()
            .filter(|(count, ..)| *count > 0)
            .map(|&(count, one, many)| format!("{count} {}", if count == 1 { one } else { many }))
            .collect();
        if parts.is_empty() {
            format!("none since {since}")
        } else {
            format!("{} since {since}", parts.join(", "))
        }
    }
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

    fn with_body(subject: &str, body: &str) -> Commit {
        Commit {
            subject: subject.to_string(),
            body: body.to_string(),
        }
    }

    #[test]
    fn kinds_of_conventional_commits() {
        let kind = |subject: &str| commit(subject).kind();

        assert_eq!(kind("feat: a"), Kind::Feature);
        assert_eq!(kind("Fix: crash"), Kind::Fix);
        assert_eq!(kind("feat(cli)!: a"), Kind::Breaking);
        assert_eq!(kind("chore!: drop Node 18"), Kind::Breaking);
        assert_eq!(
            with_body("fix: a", "BREAKING CHANGE: gone").kind(),
            Kind::Breaking
        );
        assert_eq!(kind("docs: a"), Kind::LeftOut);
        assert_eq!(kind("Update README"), Kind::Unconventional);
        assert_eq!(kind("feat:no space"), Kind::Unconventional);
    }

    #[test]
    fn footer_under_an_unconventional_subject_is_not_breaking() {
        let commit = with_body("Update the API", "BREAKING CHANGE: gone");

        assert_eq!(commit.kind(), Kind::Unconventional);
    }

    #[test]
    fn tally_counts_each_kind() {
        let commits = [
            commit("feat!: a"),
            commit("feat: b"),
            commit("feat: c"),
            commit("fix: d"),
            commit("docs: e"),
            commit("Update README"),
        ];

        assert_eq!(
            Tally::of(&commits),
            Tally {
                breaking: 1,
                features: 2,
                fixes: 1,
                other: 2
            }
        );
        assert!(Tally::of(&[]).is_empty());
        assert!(!Tally::of(&commits).is_empty());
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
    fn summary_lists_the_counts_in_order() {
        assert_eq!(
            tally(1, 2, 1, 3).summary(Some("v1.4.2")),
            "1 breaking change, 2 features, 1 fix, 3 other commits since v1.4.2"
        );
    }

    #[test]
    fn summary_uses_singular_and_plural() {
        assert_eq!(
            tally(0, 1, 2, 0).summary(Some("v1.4.2")),
            "1 feature, 2 fixes since v1.4.2"
        );
        assert_eq!(
            tally(2, 0, 0, 1).summary(Some("v1.4.2")),
            "2 breaking changes, 1 other commit since v1.4.2"
        );
    }

    #[test]
    fn summary_says_none_without_commits() {
        assert_eq!(
            Tally::default().summary(Some("v1.4.2")),
            "none since v1.4.2"
        );
    }

    #[test]
    fn summary_counts_from_the_first_commit_without_a_tag() {
        assert_eq!(
            tally(0, 0, 0, 1).summary(None),
            "1 other commit since the first commit"
        );
    }
}
