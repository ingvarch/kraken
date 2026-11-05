use std::fs;
use std::ops::Range;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use quick_xml::Reader;
use quick_xml::events::Event;

use super::{replace_ranges, single};

const PROPERTIES: [&str; 3] = ["Version", "VersionPrefix", "PackageVersion"];
const BOM: &str = "\u{feff}";
const PROJECT_EXTENSIONS: [&str; 3] = [".csproj", ".fsproj", ".vbproj"];

struct Property {
    name: &'static str,
    range: Range<usize>,
}

pub fn read_version(text: &str) -> Result<Option<String>> {
    let properties = properties(text)?;
    let Some((first, rest)) = properties.split_first() else {
        return Ok(None);
    };
    let value = &text[first.range.clone()];
    if let Some(other) = rest.iter().find(|p| &text[p.range.clone()] != value) {
        bail!(
            "<{}> {} differs from <{}> {value}",
            other.name,
            &text[other.range.clone()],
            first.name
        );
    }
    Ok(Some(value.to_string()))
}

pub fn set_version(text: &str, version: &str) -> Result<String> {
    let properties = properties(text)?;
    if version.contains('-') && properties.iter().any(|p| p.name == "VersionPrefix") {
        bail!("<VersionPrefix> holds X.Y.Z only, so it cannot be {version}");
    }
    let ranges: Vec<_> = properties.into_iter().map(|p| p.range).collect();
    Ok(replace_ranges(text, &ranges, version))
}

/// The path of the single project file in `root` that sets a version. A file that `is_ignored`
/// does not count, nor does a symlink.
pub fn find_project(
    root: &Path,
    is_ignored: impl Fn(&str) -> Result<bool>,
) -> Result<Option<String>> {
    let mut found = Vec::new();
    for entry in fs::read_dir(root).with_context(|| root.display().to_string())? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if PROJECT_EXTENSIONS.iter().any(|ext| name.ends_with(ext))
            && entry.file_type()?.is_file()
            && !is_ignored(&name)?
            && sets_version(&entry.path()).with_context(|| name.clone())?
        {
            found.push(name);
        }
    }
    single(found, "project file sets a version")
}

fn sets_version(path: &Path) -> Result<bool> {
    Ok(!properties(&fs::read_to_string(path)?)?.is_empty())
}

/// The version properties in a `PropertyGroup`, in document order, with the range of their text.
fn properties(text: &str) -> Result<Vec<Property>> {
    let bom = if text.starts_with(BOM) { BOM.len() } else { 0 };
    let mut reader = Reader::from_str(&text[bom..]);
    let mut stack: Vec<String> = Vec::new();
    let mut found = Vec::new();
    // The property being read, and the range of its text once seen.
    let mut open: Option<(&'static str, Option<Range<usize>>)> = None;
    loop {
        let before = reader.buffer_position() as usize + bom;
        let event = reader.read_event().map_err(|err| anyhow!("{err}"))?;
        let after = reader.buffer_position() as usize + bom;
        match event {
            Event::Start(_) | Event::Empty(_) if open.is_some() => {
                let (name, _) = open.as_ref().expect("checked above");
                bail!("<{name}> must hold only the version");
            }
            Event::Start(start) => {
                let name = start.name().as_ref().to_string();
                if stack.last().is_some_and(|parent| parent == "PropertyGroup") {
                    open = PROPERTIES
                        .into_iter()
                        .find(|p| p.eq_ignore_ascii_case(&name))
                        .map(|p| (p, None));
                }
                stack.push(name);
            }
            Event::End(_) => {
                stack.pop();
                if let Some((name, Some(range))) = open.take() {
                    let value = &text[range.clone()];
                    if ["$(", "@(", "%("].iter().any(|start| value.contains(start)) {
                        bail!(
                            "<{name}> holds the MSBuild expression {value}; kraken needs a literal X.Y.Z version"
                        );
                    }
                    found.push(Property { name, range });
                }
            }
            Event::Text(_)
            | Event::CData(_)
            | Event::Comment(_)
            | Event::PI(_)
            | Event::GeneralRef(_)
                if open.is_some() =>
            {
                let (name, seen) = open.as_mut().expect("checked above");
                let whole = &text[before..after];
                let value = whole.trim();
                if !matches!(event, Event::Text(_)) {
                    bail!("<{name}> must hold only the version");
                }
                if !value.is_empty() {
                    let start = before + (whole.len() - whole.trim_start().len());
                    *seen = Some(start..start + value.len());
                }
            }
            Event::Eof => {
                if let Some(unclosed) = stack.first() {
                    bail!("<{unclosed}> is not closed");
                }
                return Ok(found);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(body: &str) -> String {
        format!("<Project Sdk=\"Microsoft.NET.Sdk\">\n{body}\n</Project>\n")
    }

    fn group(body: &str) -> String {
        project(&format!("  <PropertyGroup>\n{body}\n  </PropertyGroup>"))
    }

    fn read(text: &str) -> Option<String> {
        read_version(text).unwrap()
    }

    fn read_err(text: &str) -> String {
        format!("{:#}", read_version(text).unwrap_err())
    }

    #[test]
    fn reads_the_version_property() {
        let text = project(
            "  <PropertyGroup>\n    <Version>1.2.3</Version>\n  </PropertyGroup>\n  <ItemGroup>\n    <PackageReference Include=\"Serilog\" Version=\"4.0.0\" />\n  </ItemGroup>",
        );
        assert_eq!(read(&text).as_deref(), Some("1.2.3"));
    }

    #[test]
    fn reads_version_prefix_and_package_version() {
        assert_eq!(
            read(&group("<VersionPrefix>1.2.3</VersionPrefix>")).as_deref(),
            Some("1.2.3")
        );
        assert_eq!(
            read(&group("<PackageVersion>1.2.3</PackageVersion>")).as_deref(),
            Some("1.2.3")
        );
    }

    #[test]
    fn property_names_ignore_case_as_in_msbuild() {
        let text = group("<version>1.2.3</version><PACKAGEVERSION>1.2.3</PACKAGEVERSION>");
        assert_eq!(read(&text).as_deref(), Some("1.2.3"));
        assert_eq!(
            set_version(&text, "1.2.4").unwrap(),
            text.replace("1.2.3", "1.2.4")
        );
    }

    #[test]
    fn reads_a_property_group_at_any_depth() {
        let text = project(
            "<Choose><When Condition=\"'$(A)' > '1'\"><PropertyGroup><Version>1.2.3</Version></PropertyGroup></When></Choose>",
        );
        assert_eq!(read(&text).as_deref(), Some("1.2.3"));
    }

    #[test]
    fn item_metadata_is_not_the_version() {
        let text = project(
            "  <ItemGroup>\n    <PackageReference Include=\"X\">\n      <Version>2.0.0</Version>\n    </PackageReference>\n  </ItemGroup>",
        );
        assert_eq!(read(&text), None);
    }

    #[test]
    fn a_commented_out_version_is_not_the_version() {
        let text = group("    <!--\n    <Version>0.9.0</Version>\n    -->");
        assert_eq!(read(&text), None);
    }

    #[test]
    fn assembly_and_file_versions_are_not_the_version() {
        let text = group(
            "<AssemblyVersion>1.0.0.0</AssemblyVersion><FileVersion>1.0.0.0</FileVersion><VersionSuffix>beta</VersionSuffix><InformationalVersion>x</InformationalVersion>",
        );
        assert_eq!(read(&text), None);
    }

    #[test]
    fn an_empty_element_is_skipped() {
        let text = group(
            "<Version /><PackageVersion></PackageVersion><VersionPrefix>1.2.3</VersionPrefix>",
        );
        assert_eq!(read(&text).as_deref(), Some("1.2.3"));
    }

    #[test]
    fn disagreeing_values_are_an_error() {
        let text = group("<Version>1.2.3</Version><VersionPrefix>1.3.0</VersionPrefix>");
        assert_eq!(
            read_err(&text),
            "<VersionPrefix> 1.3.0 differs from <Version> 1.2.3"
        );
    }

    #[test]
    fn refuses_an_msbuild_expression() {
        let text = group("<PackageVersion>$(Version)</PackageVersion>");
        assert_eq!(
            read_err(&text),
            "<PackageVersion> holds the MSBuild expression $(Version); kraken needs a literal X.Y.Z version"
        );
    }

    #[test]
    fn refuses_an_entity_in_the_version() {
        let text = group("<Version>1.2&#46;3</Version>");
        assert_eq!(read_err(&text), "<Version> must hold only the version");
    }

    #[test]
    fn refuses_an_entity_after_the_version() {
        let text = group("<Version>1.2.3&#48;</Version>");
        assert_eq!(read_err(&text), "<Version> must hold only the version");
    }

    #[test]
    fn refuses_a_processing_instruction_in_the_version() {
        for body in [
            "<Version>9<?x?>1.2.3</Version>",
            "<Version>1.2.3<?x?></Version>",
        ] {
            assert_eq!(
                read_err(&group(body)),
                "<Version> must hold only the version"
            );
        }
    }

    #[test]
    fn refuses_a_comment_in_the_version() {
        let text = group("<Version>1.2.3<!-- x --></Version>");
        assert_eq!(read_err(&text), "<Version> must hold only the version");
    }

    #[test]
    fn sets_every_equal_value() {
        let text = group(
            "<Version>1.2.3</Version>\n<Version Condition=\"'$(A)' == 'b'\">1.2.3</Version><PackageVersion>1.2.3</PackageVersion>",
        );
        let out = set_version(&text, "1.2.4").unwrap();
        assert_eq!(out, text.replace("1.2.3", "1.2.4"));
    }

    #[test]
    fn version_takes_a_pre_release() {
        let text = group("<Version>1.2.3</Version><PackageVersion>1.2.3</PackageVersion>");
        let out = set_version(&text, "1.4.0-rc.1").unwrap();
        assert_eq!(out, text.replace("1.2.3", "1.4.0-rc.1"));
    }

    #[test]
    fn version_prefix_refuses_a_pre_release() {
        let text = group("<VersionPrefix>1.2.3</VersionPrefix>");
        let err = set_version(&text, "1.4.0-rc.1").unwrap_err().to_string();
        assert_eq!(
            err,
            "<VersionPrefix> holds X.Y.Z only, so it cannot be 1.4.0-rc.1"
        );
    }

    #[test]
    fn sets_only_the_text() {
        let text = project(
            "  <!-- <Version>1.2.3</Version> -->\n  <PropertyGroup Condition=\"'$(A)' > '1'\" Label=\"a &gt; b\">\n    <VersionPrefix>1.2.3</VersionPrefix><VersionSuffix>beta</VersionSuffix>\n    <AssemblyVersion>1.0.0.0</AssemblyVersion>\n  </PropertyGroup>\n  <ItemGroup><PackageReference Include=\"X\"><Version>1.2.3</Version></PackageReference></ItemGroup>",
        );
        let expected = text.replace("<VersionPrefix>1.2.3<", "<VersionPrefix>1.2.4<");
        assert_eq!(set_version(&text, "1.2.4").unwrap(), expected);
    }

    #[test]
    fn keeps_the_bom_spacing_and_crlf() {
        let text = "\u{feff}<Project>\r\n  <PropertyGroup>\r\n    <Version> 1.2.3 </Version>\r\n  </PropertyGroup>\r\n</Project>\r\n";
        assert_eq!(read(text).as_deref(), Some("1.2.3"));
        assert_eq!(
            set_version(text, "1.2.4").unwrap(),
            text.replace("1.2.3", "1.2.4")
        );
    }

    #[test]
    fn malformed_xml_is_an_error() {
        let text = "<Project><PropertyGroup><Version>1.2.3</Verson></PropertyGroup></Project>";
        assert_eq!(
            read_err(text),
            "ill-formed document: expected `</Version>`, but `</Verson>` was found"
        );
    }

    #[test]
    fn a_truncated_document_is_an_error() {
        let text = "<Project><PropertyGroup><Version>1.2.3</Version>";
        assert_eq!(read_err(text), "<Project> is not closed");
    }

    #[test]
    fn refuses_a_child_element_in_the_version() {
        for body in [
            "<Version><X>1.2.3</X></Version>",
            "<Version><X/>1.2.3</Version>",
        ] {
            assert_eq!(
                read_err(&group(body)),
                "<Version> must hold only the version"
            );
        }
    }

    #[test]
    fn refuses_a_cdata_version() {
        let text = group("<Version><![CDATA[1.2.3]]></Version>");
        assert_eq!(read_err(&text), "<Version> must hold only the version");
    }

    #[test]
    fn refuses_item_and_metadata_expressions() {
        for expr in ["@(Foo)", "%(Bar)"] {
            let text = group(&format!("<Version>{expr}</Version>"));
            assert_eq!(
                read_err(&text),
                format!(
                    "<Version> holds the MSBuild expression {expr}; kraken needs a literal X.Y.Z version"
                )
            );
        }
    }
}
