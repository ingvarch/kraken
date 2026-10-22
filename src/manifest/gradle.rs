use std::ops::Range;

use anyhow::{Result, bail};

pub fn read_version(text: &str) -> Result<Option<String>> {
    let mut values = version_values(text).into_iter().map(|range| &text[range]);
    let Some(first) = values.next() else {
        return Ok(None);
    };
    if let Some(other) = values.find(|value| *value != first) {
        bail!("version has two values: {first} and {other}");
    }
    Ok(Some(first.to_string()))
}

pub fn set_version(text: &str, version: &str) -> Result<String> {
    let mut out = text.to_string();
    for range in version_values(text).into_iter().rev() {
        out.replace_range(range, version);
    }
    Ok(out)
}

/// The byte ranges of the values on the `version` lines.
fn version_values(text: &str) -> Vec<Range<usize>> {
    let mut offset = 0;
    let mut found = Vec::new();
    for line in text.split_inclusive('\n') {
        if let Some(range) = version_value(line) {
            found.push(offset + range.start..offset + range.end);
        }
        offset += line.len();
    }
    found
}

fn version_value(line: &str) -> Option<Range<usize>> {
    let body = line.trim_end();
    let entry = body.trim_start();
    if entry.is_empty() || entry.starts_with(['#', '!']) {
        return None;
    }
    let key_end = entry.find(|c: char| matches!(c, '=' | ':') || c.is_whitespace());
    let (key, rest) = entry.split_at(key_end.unwrap_or(entry.len()));
    if key != "version" {
        return None;
    }
    let rest = rest.trim_start();
    let value = rest.strip_prefix(['=', ':']).unwrap_or(rest).trim_start();
    Some(body.len() - value.len()..body.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROPERTIES: &str = "# project\norg.gradle.jvmargs=-Xmx2g\n\nversion=1.2.3\nversionCode=7\nsystemProp.version=9.9.9\n! other\n";

    #[test]
    fn reads_the_version_line() {
        assert_eq!(read_version(PROPERTIES).unwrap().as_deref(), Some("1.2.3"));
    }

    #[test]
    fn reads_every_separator_style() {
        for line in [
            "version=1.2.3",
            "version = 1.2.3",
            "version: 1.2.3",
            "version 1.2.3",
            "  version\t=\t1.2.3  ",
        ] {
            assert_eq!(
                read_version(line).unwrap().as_deref(),
                Some("1.2.3"),
                "{line}"
            );
        }
    }

    #[test]
    fn no_version_line_is_none() {
        let text = "# version=1.2.3\n! version=1.2.3\nversionCode=7\nsystemProp.version=1.2.3\n";
        assert_eq!(read_version(text).unwrap(), None);
    }

    #[test]
    fn repeated_equal_versions_are_fine() {
        let text = "version=1.2.3\nversion = 1.2.3\n";
        assert_eq!(read_version(text).unwrap().as_deref(), Some("1.2.3"));
    }

    #[test]
    fn disagreeing_versions_are_an_error() {
        let err = read_version("version=1.0.0\nversion=1.1.0\n")
            .unwrap_err()
            .to_string();
        assert_eq!(err, "version has two values: 1.0.0 and 1.1.0");
    }

    #[test]
    fn sets_only_the_version_value() {
        let expected = PROPERTIES.replace("version=1.2.3", "version=1.2.4");
        assert_eq!(set_version(PROPERTIES, "1.2.4").unwrap(), expected);
    }

    #[test]
    fn keeps_separator_and_spacing() {
        let text = "version = 1.2.3\nversion: 1.2.3\nversion 1.2.3\n  version\t=\t1.2.3  \n";
        let expected = text.replace("1.2.3", "2.0.0");
        assert_eq!(set_version(text, "2.0.0").unwrap(), expected);
    }

    #[test]
    fn keeps_crlf_line_endings() {
        let text = "a=b\r\nversion=1.2.3\r\nc=d\r\n";
        assert_eq!(
            set_version(text, "1.2.4").unwrap(),
            "a=b\r\nversion=1.2.4\r\nc=d\r\n"
        );
        assert_eq!(read_version(text).unwrap().as_deref(), Some("1.2.3"));
    }

    #[test]
    fn sets_every_version_line() {
        let text = "version=1.2.3\nname=x\nversion=1.2.3";
        assert_eq!(
            set_version(text, "1.2.4").unwrap(),
            "version=1.2.4\nname=x\nversion=1.2.4"
        );
    }
}
