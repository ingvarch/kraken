use std::fs;
use std::ops::Range;
use std::path::Path;

use anyhow::{Context, Result, bail};

use super::{agreed_value, line_values, replace_ranges};

const SETTING: &str = "MARKETING_VERSION = ";

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

/// The path of the single `project.pbxproj` in `root` that sets `MARKETING_VERSION`, relative to it.
/// A project whose `project.pbxproj` `is_ignored` does not count, nor does a symlink: git tracks the
/// link, not the project behind it.
pub fn find(root: &Path, is_ignored: impl Fn(&str) -> Result<bool>) -> Result<Option<String>> {
    let mut projects = Vec::new();
    for entry in fs::read_dir(root).with_context(|| root.display().to_string())? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let pbxproj = format!("{name}/project.pbxproj");
        if name.ends_with(".xcodeproj")
            && entry.file_type()?.is_dir()
            && root.join(&pbxproj).is_file()
            && !is_ignored(&pbxproj)?
            && sets_version(&root.join(&pbxproj))?
        {
            projects.push(name);
        }
    }
    projects.sort();
    match projects.as_slice() {
        [] => Ok(None),
        [name] => Ok(Some(format!("{name}/project.pbxproj"))),
        _ => bail!(
            "more than one Xcode project sets MARKETING_VERSION: {}",
            projects.join(", ")
        ),
    }
}

fn sets_version(pbxproj: &Path) -> Result<bool> {
    let text = fs::read_to_string(pbxproj).with_context(|| pbxproj.display().to_string())?;
    Ok(!line_values(&text, version_value).is_empty())
}

fn version_value(line: &str) -> Option<Range<usize>> {
    let entry = line.trim_start();
    let rest = entry.strip_prefix(SETTING)?;
    let start = line.len() - rest.len();
    let (start, rest) = match rest.strip_prefix('"') {
        Some(quoted) => (start + 1, quoted),
        None => (start, rest),
    };
    let len = rest.find(['"', ';']).unwrap_or(rest.trim_end().len());
    Some(start..start + len)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PBXPROJ: &str = "// !$*UTF8*$!\n{\n\tbuildSettings = {\n\t\t\t\tCURRENT_PROJECT_VERSION = 7;\n\t\t\t\tMARKETING_VERSION = 1.2.3;\n\t\t\t\tPRODUCT_NAME = App;\n\t\t\t};\n\t\t\tname = Debug;\n\t\t\tbuildSettings = {\n\t\t\t\tCURRENT_PROJECT_VERSION = 7;\n\t\t\t\tMARKETING_VERSION = 1.2.3;\n\t\t\t};\n\t\t\tname = Release;\n}\n";

    #[test]
    fn reads_the_marketing_version() {
        assert_eq!(read_version(PBXPROJ).unwrap().as_deref(), Some("1.2.3"));
    }

    #[test]
    fn reads_a_quoted_value() {
        let text = "\t\tMARKETING_VERSION = \"1.2.3\";\n";
        assert_eq!(read_version(text).unwrap().as_deref(), Some("1.2.3"));
    }

    #[test]
    fn no_marketing_version_is_none() {
        let text = "\t\tCURRENT_PROJECT_VERSION = 7;\n\t\tOTHER_MARKETING_VERSION = 1.2.3;\n";
        assert_eq!(read_version(text).unwrap(), None);
    }

    #[test]
    fn disagreeing_values_are_an_error() {
        let text = "\t\tMARKETING_VERSION = 1.0.0;\n\t\tMARKETING_VERSION = 1.1.0;\n";
        let err = read_version(text).unwrap_err().to_string();
        assert_eq!(err, "MARKETING_VERSION has two values: 1.0.0 and 1.1.0");
    }

    #[test]
    fn sets_every_configuration_and_leaves_the_build_number() {
        let expected = PBXPROJ.replace("MARKETING_VERSION = 1.2.3", "MARKETING_VERSION = 1.2.4");
        assert_eq!(set_version(PBXPROJ, "1.2.4").unwrap(), expected);
    }

    #[test]
    fn keeps_quotes() {
        let text = "\t\tMARKETING_VERSION = \"1.2.3\";\n\t\tMARKETING_VERSION = 1.2.3;\n";
        assert_eq!(
            set_version(text, "1.2.4").unwrap(),
            "\t\tMARKETING_VERSION = \"1.2.4\";\n\t\tMARKETING_VERSION = 1.2.4;\n"
        );
    }
}
