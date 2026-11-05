use anyhow::{Result, bail};

use super::package_json::{parse, render, set_string, string_field};

pub fn read_version(text: &str) -> Result<Option<String>> {
    // Other files named manifest.json need not be JSON at all.
    if !text.contains("manifest_version") {
        return Ok(None);
    }
    let root = parse(text)?;
    let Some(object) = root
        .object_value()
        .filter(|o| o.get("manifest_version").is_some())
    else {
        return Ok(None);
    };
    let version = string_field(&object, "version")?;
    if let (Some(name), Some(version)) = (string_field(&object, "version_name")?, &version)
        && name != *version
    {
        bail!("version_name \"{name}\" differs from version {version}");
    }
    Ok(version)
}

pub fn set_version(text: &str, version: &str) -> Result<String> {
    let root = parse(text)?;
    set_string(root.object_value(), "version", version);
    set_string(root.object_value(), "version_name", version);
    Ok(render(text, &root))
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXTENSION: &str = r#"{
    "manifest_version": 3,
    "name": "Demo",
    "version": "1.2.3",
    "browser_specific_settings": { "gecko": { "strict_min_version": "109.0" } }
}
"#;

    #[test]
    fn reads_the_extension_version() {
        assert_eq!(read_version(EXTENSION).unwrap().as_deref(), Some("1.2.3"));
    }

    #[test]
    fn a_web_app_manifest_is_not_a_version_file() {
        let text = r#"{ "name": "App", "icons": [], "start_url": "/", "version": "1.2.3" }"#;
        assert_eq!(read_version(text).unwrap(), None);
    }

    #[test]
    fn a_manifest_json_that_is_not_json_is_not_a_version_file() {
        let text =
            "---\nlayout: null\n---\n{ \"name\": \"{{ site.title }}\", \"start_url\": \"/\" }\n";
        assert_eq!(read_version(text).unwrap(), None);
    }

    #[test]
    fn manifest_version_in_camel_case_does_not_count() {
        let text = r#"{ "manifestVersion": "1.17", "version": "1.2.3" }"#;
        assert_eq!(read_version(text).unwrap(), None);
    }

    #[test]
    fn manifest_version_below_the_top_level_does_not_count() {
        let nested = r#"{ "name": "App", "version": "9.9.9", "x": { "manifest_version": 3 } }"#;
        assert_eq!(read_version(nested).unwrap(), None);
        let in_string = r#"{ "name": "manifest_version", "version": "9.9.9" }"#;
        assert_eq!(read_version(in_string).unwrap(), None);
    }

    #[test]
    fn an_extension_without_a_version_is_none() {
        assert_eq!(read_version(r#"{ "manifest_version": 3 }"#).unwrap(), None);
    }

    #[test]
    fn version_must_be_a_string() {
        let err = read_version(r#"{ "manifest_version": 3, "version": 1 }"#)
            .unwrap_err()
            .to_string();
        assert_eq!(err, "\"version\" is not a string");
    }

    #[test]
    fn version_name_must_equal_version() {
        let text = r#"{ "manifest_version": 3, "version": "1.2.3", "version_name": "1.2.3 beta" }"#;
        let err = read_version(text).unwrap_err().to_string();
        assert_eq!(
            err,
            "version_name \"1.2.3 beta\" differs from version 1.2.3"
        );
    }

    #[test]
    fn version_name_must_be_a_string() {
        let text = r#"{ "manifest_version": 3, "version": "1.2.3", "version_name": 5 }"#;
        let err = read_version(text).unwrap_err().to_string();
        assert_eq!(err, "\"version_name\" is not a string");
    }

    #[test]
    fn sets_the_version_and_an_equal_version_name() {
        let text = r#"{ "manifest_version": 3, "version": "1.2.3", "version_name": "1.2.3" }"#;
        assert_eq!(read_version(text).unwrap().as_deref(), Some("1.2.3"));
        assert_eq!(
            set_version(text, "1.2.4").unwrap(),
            text.replace("1.2.3", "1.2.4")
        );
    }

    #[test]
    fn sets_only_the_version_without_a_version_name() {
        assert_eq!(
            set_version(EXTENSION, "1.2.4").unwrap(),
            EXTENSION.replace(r#""version": "1.2.3""#, r#""version": "1.2.4""#)
        );
    }

    #[test]
    fn keeps_comments_and_formatting() {
        let text = "{\n  // store listing\n  \"manifest_version\": 3,\n  \"version\": \"1.2.3\", /* current */\n  \"version_name\": \"1.2.3\",\n}\n";
        assert_eq!(
            set_version(text, "1.2.4").unwrap(),
            text.replace("1.2.3", "1.2.4")
        );
    }

    #[test]
    fn a_bom_does_not_hide_the_manifest() {
        let text = format!("\u{feff}{EXTENSION}");
        assert_eq!(read_version(&text).unwrap().as_deref(), Some("1.2.3"));
        let out = set_version(&text, "1.2.4").unwrap();
        assert_eq!(
            out,
            format!("\u{feff}{}", EXTENSION.replace("1.2.3", "1.2.4"))
        );
        let app = "\u{feff}{ \"name\": \"App\" }";
        assert_eq!(read_version(app).unwrap(), None);
    }
}
