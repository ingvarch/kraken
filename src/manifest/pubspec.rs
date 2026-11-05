use std::ops::Range;

use anyhow::Result;

use super::{agreed_value, line_values, replace_ranges, yaml_value};

pub fn read_version(text: &str) -> Result<Option<String>> {
    let ranges = line_values(text, version_value);
    agreed_value("version", text, &ranges)
}

pub fn set_version(text: &str, version: &str) -> Result<String> {
    Ok(replace_ranges(
        text,
        &line_values(text, version_value),
        version,
    ))
}

/// The top-level `version`, up to the `+` that starts the build number.
fn version_value(line: &str) -> Option<Range<usize>> {
    let quote = line.chars().next().filter(|c| matches!(c, '"' | '\''));
    let key = match quote {
        Some(q) => line[1..].strip_prefix("version")?.strip_prefix(q)?,
        None => line.strip_prefix("version")?,
    };
    let colon = line.len() - key.trim_start_matches([' ', '\t']).len();
    let range = yaml_value(line.get(colon..)?, ":")?;
    let range = range.start + colon..range.end + colon;
    let end = line[range.clone()]
        .find('+')
        .map_or(range.end, |plus| range.start + plus);
    Some(range.start..end)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLUTTER: &str = "name: app\ndescription: An app.\nversion: 1.2.3+4\n\ndependencies:\n  flutter:\n    sdk: flutter\n  http:\n    hosted: https://pub.dev\n    version: ^2.0.0\n";

    #[test]
    fn reads_the_top_level_version() {
        assert_eq!(read_version(FLUTTER).unwrap().as_deref(), Some("1.2.3"));
    }

    #[test]
    fn reads_every_value_style() {
        for line in [
            "version: 1.2.3",
            "version: 1.2.3+4",
            "version: \"1.2.3+4\" # store",
            "version: '1.2.3+build.5'",
        ] {
            assert_eq!(
                read_version(&format!("name: app\n{line}\n"))
                    .unwrap()
                    .as_deref(),
                Some("1.2.3"),
                "{line}"
            );
        }
    }

    #[test]
    fn reads_a_key_with_a_space_before_the_colon() {
        let text = "name: app\nversion : 1.2.3+4\n";
        assert_eq!(read_version(text).unwrap().as_deref(), Some("1.2.3"));
        assert_eq!(
            set_version(text, "1.2.4").unwrap(),
            "name: app\nversion : 1.2.4+4\n"
        );
    }

    #[test]
    fn reads_a_quoted_key() {
        for key in ["\"version\"", "'version'", "\"version\" "] {
            let text = format!("name: app\n{key}: 1.2.3+4\n");
            assert_eq!(
                read_version(&text).unwrap().as_deref(),
                Some("1.2.3"),
                "{key}"
            );
            assert_eq!(
                set_version(&text, "1.2.4").unwrap(),
                text.replace("1.2.3", "1.2.4"),
                "{key}"
            );
        }
        assert_eq!(read_version("\"version': 1.2.3\n").unwrap(), None);
    }

    #[test]
    fn sets_every_equal_version_and_keeps_each_build_number() {
        let text = "version: 1.2.3+1\nversion: '1.2.3+2'\n";
        assert_eq!(
            set_version(text, "1.2.4").unwrap(),
            "version: 1.2.4+1\nversion: '1.2.4+2'\n"
        );
    }

    #[test]
    fn indented_version_keys_are_not_the_version() {
        let text =
            "name: app\ndependencies:\n  http:\n    version: ^2.0.0\nlist:\n  - version: 1.0.0\n";
        assert_eq!(read_version(text).unwrap(), None);
    }

    #[test]
    fn no_version_is_none() {
        assert_eq!(read_version("name: app\n# version: 1.2.3\n").unwrap(), None);
    }

    #[test]
    fn a_pre_release_keeps_its_suffix() {
        let version = read_version("version: 1.2.3-dev.1+4\n").unwrap().unwrap();
        assert_eq!(version, "1.2.3-dev.1");
        let err = crate::version::parse_release(&version)
            .unwrap_err()
            .to_string();
        assert_eq!(
            err,
            "\"1.2.3-dev.1\": pre-release versions are not supported"
        );
    }

    #[test]
    fn a_missing_value_reads_as_empty() {
        assert_eq!(read_version("version:\n").unwrap().as_deref(), Some(""));
    }

    #[test]
    fn different_versions_are_an_error() {
        let err = read_version("version: 1.2.3\nversion: 1.3.0+1\n")
            .unwrap_err()
            .to_string();
        assert_eq!(err, "version has two values: 1.2.3 and 1.3.0");
    }

    #[test]
    fn sets_the_version_and_keeps_the_build_number() {
        let expected = FLUTTER.replace("1.2.3+4", "1.2.4+4");
        assert_eq!(set_version(FLUTTER, "1.2.4").unwrap(), expected);
    }

    #[test]
    fn keeps_quotes_spacing_comments_and_crlf() {
        for line in [
            "version: 1.2.3",
            "version: 1.2.3+4",
            "version:   1.2.3+4   # store",
            "version: \"1.2.3+4\" # store",
            "version: '1.2.3+build.5'",
        ] {
            let text = format!("name: app\r\n{line}\r\nother: 1\r\n");
            assert_eq!(
                set_version(&text, "1.2.4").unwrap(),
                text.replace("1.2.3", "1.2.4"),
                "{line}"
            );
        }
    }
}
