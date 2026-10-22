use std::fs;
use std::ops::Range;
use std::path::Path;

use anyhow::{Context, Result};

use super::{agreed_value, line_values, replace_ranges};

const FILE: &str = "project.yml";
const KEY: &str = "MARKETING_VERSION:";

pub fn read_version(text: &str) -> Result<Option<String>> {
    let ranges = line_values(text, version_value);
    agreed_value("MARKETING_VERSION", text, &ranges)
}

pub fn set_version(text: &str, version: &str) -> Result<String> {
    Ok(replace_ranges(
        text,
        &line_values(text, version_value),
        version,
    ))
}

/// The `project.pbxproj` that XcodeGen generates from the `project.yml` in `root`, relative to it:
/// `<name>.xcodeproj`, after the top-level `name`. `None` until it is generated.
pub fn generated_project(root: &Path) -> Result<Option<String>> {
    let text = fs::read_to_string(root.join(FILE)).context(FILE)?;
    let Some(name) = project_name(&text) else {
        return Ok(None);
    };
    let pbxproj = format!("{name}.xcodeproj/project.pbxproj");
    Ok(root.join(&pbxproj).is_file().then_some(pbxproj))
}

fn project_name(text: &str) -> Option<&str> {
    text.lines()
        .find_map(|line| value_range(line, "name:").map(|range| &line[range]))
        .filter(|name| !name.is_empty())
}

fn version_value(line: &str) -> Option<Range<usize>> {
    let indent = line.len() - line.trim_start().len();
    let range = value_range(&line[indent..], KEY)?;
    Some(indent + range.start..indent + range.end)
}

/// The value after `key` at the start of `line`, without its quotes or a trailing comment.
fn value_range(line: &str, key: &str) -> Option<Range<usize>> {
    let value = line.strip_prefix(key)?.trim_start();
    let start = line.len() - value.len();
    if let Some(quote) = value.chars().next().filter(|c| matches!(c, '"' | '\'')) {
        let inner = &value[1..];
        let len = inner.find(quote).unwrap_or(inner.trim_end().len());
        return Some(start + 1..start + 1 + len);
    }
    let end = value
        .char_indices()
        .find(|&(i, c)| c == '#' && value[..i].ends_with(char::is_whitespace))
        .map_or(value.len(), |(i, _)| i);
    Some(start..start + value[..end].trim_end().len())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROJECT: &str = "name: App\nsettings:\n  base:\n    MARKETING_VERSION: \"1.2.3\"\n    CURRENT_PROJECT_VERSION: 7\ntargets:\n  App:\n    type: application\n";

    #[test]
    fn reads_the_top_level_project_name() {
        assert_eq!(project_name(PROJECT), Some("App"));
        assert_eq!(project_name("name: \"My App\" # main\n"), Some("My App"));
        assert_eq!(project_name("name: My App # main\r\n"), Some("My App"));
        assert_eq!(
            project_name("targets:\n  Tool:\n    name: Tool\nname: App\n"),
            Some("App")
        );
        assert_eq!(project_name("settings:\n  name: App\n"), None);
        assert_eq!(project_name("name:\n"), None);
    }

    #[test]
    fn reads_every_value_style() {
        for line in [
            "MARKETING_VERSION: \"1.2.3\"",
            "MARKETING_VERSION: '1.2.3'",
            "MARKETING_VERSION: 1.2.3",
            "MARKETING_VERSION:   1.2.3   # bumped by kraken",
            "MARKETING_VERSION: \"1.2.3\" # comment",
        ] {
            assert_eq!(
                read_version(&format!("settings:\n  base:\n    {line}\n"))
                    .unwrap()
                    .as_deref(),
                Some("1.2.3"),
                "{line}"
            );
        }
    }

    #[test]
    fn no_marketing_version_is_none() {
        let text = "name: App\nsettings:\n  base:\n    CURRENT_PROJECT_VERSION: 7\n    OTHER_MARKETING_VERSION: 1.2.3\n";
        assert_eq!(read_version(text).unwrap(), None);
    }

    #[test]
    fn agreeing_values_in_several_places_are_fine() {
        let text = "settings:\n  base:\n    MARKETING_VERSION: 1.2.3\ntargets:\n  App:\n    settings:\n      MARKETING_VERSION: \"1.2.3\"\n";
        assert_eq!(read_version(text).unwrap().as_deref(), Some("1.2.3"));
    }

    #[test]
    fn disagreeing_values_are_an_error() {
        let text = "a:\n  MARKETING_VERSION: 1.0.0\nb:\n  MARKETING_VERSION: '1.1.0'\n";
        let err = read_version(text).unwrap_err().to_string();
        assert_eq!(err, "MARKETING_VERSION has two values: 1.0.0 and 1.1.0");
    }

    #[test]
    fn sets_only_the_values() {
        let expected = PROJECT.replace("\"1.2.3\"", "\"1.2.4\"");
        assert_eq!(set_version(PROJECT, "1.2.4").unwrap(), expected);
    }

    #[test]
    fn keeps_quotes_spacing_and_comments() {
        let text = "a:\n  MARKETING_VERSION: '1.2.3'\n  MARKETING_VERSION:   1.2.3   # keep\n  MARKETING_VERSION: \"1.2.3\" # keep\r\n";
        assert_eq!(
            set_version(text, "1.3.0").unwrap(),
            text.replace("1.2.3", "1.3.0")
        );
    }
}
