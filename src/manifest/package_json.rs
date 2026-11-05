use anyhow::{Context, Result};
use jsonc_parser::ParseOptions;
use jsonc_parser::cst::{CstInputValue, CstObject, CstRootNode};

pub fn read_version(text: &str) -> Result<Option<String>> {
    let root = parse(text)?;
    match root.object_value() {
        Some(object) => string_field(&object, "version"),
        None => Ok(None),
    }
}

/// The top-level string `key`, `None` when it is absent.
pub(super) fn string_field(object: &CstObject, key: &str) -> Result<Option<String>> {
    let Some(prop) = object.get(key) else {
        return Ok(None);
    };
    let literal = prop
        .value()
        .and_then(|value| value.as_string_lit())
        .with_context(|| format!("\"{key}\" is not a string"))?;
    Ok(Some(literal.decoded_value()?))
}

pub fn set_version(text: &str, version: &str) -> Result<String> {
    let root = parse(text)?;
    set_string(root.object_value(), "version", version);
    Ok(render(text, &root))
}

/// Sets the root `version` and `packages[""].version` of a package-lock.json.
pub fn set_lock_version(text: &str, version: &str) -> Result<String> {
    let root = parse(text)?;
    let object = root.object_value();
    let own_package = object
        .as_ref()
        .and_then(|object| object.object_value("packages"))
        .and_then(|packages| packages.object_value(""));
    set_string(object, "version", version);
    set_string(own_package, "version", version);
    Ok(render(text, &root))
}

const BOM: char = '\u{feff}';

pub(super) fn parse(text: &str) -> Result<CstRootNode> {
    let text = text.strip_prefix(BOM).unwrap_or(text);
    Ok(CstRootNode::parse(text, &ParseOptions::default())?)
}

/// `root` as text, with the BOM that `original` started with.
pub(super) fn render(original: &str, root: &CstRootNode) -> String {
    let bom = if original.starts_with(BOM) {
        "\u{feff}"
    } else {
        ""
    };
    format!("{bom}{root}")
}

pub(super) fn set_string(object: Option<CstObject>, key: &str, value: &str) {
    if let Some(prop) = object.and_then(|object| object.get(key)) {
        prop.set_value(CstInputValue::String(value.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACKAGE: &str = r#"{
    "name": "demo",
    "engines": { "version": "9.9.9" },
    "version": "0.1.0",
    "private": true
}
"#;

    #[test]
    fn reads_root_version() {
        assert_eq!(read_version(PACKAGE).unwrap().as_deref(), Some("0.1.0"));
    }

    #[test]
    fn ignores_nested_version() {
        let text = r#"{ "name": "demo", "engines": { "version": "9.9.9" } }"#;
        assert_eq!(read_version(text).unwrap(), None);
    }

    #[test]
    fn rejects_non_string_version() {
        let err = read_version(r#"{ "version": 1 }"#).unwrap_err().to_string();
        assert!(err.contains("not a string"), "{err}");
    }

    #[test]
    fn sets_only_root_version() {
        let expected = PACKAGE.replace(r#""version": "0.1.0""#, r#""version": "0.2.0""#);
        assert_eq!(set_version(PACKAGE, "0.2.0").unwrap(), expected);
    }

    #[test]
    fn keeps_jsonc_comments() {
        let text = "{\n  // the release\n  \"version\": \"1.2.3\", /* current */\n  \"name\": \"demo\",\n}\n";
        let expected = text.replace("1.2.3", "1.2.4");
        assert_eq!(read_version(text).unwrap().as_deref(), Some("1.2.3"));
        assert_eq!(set_version(text, "1.2.4").unwrap(), expected);
    }

    #[test]
    fn sets_lock_root_and_own_package_entry() {
        let lock = r#"{
  "name": "demo",
  "version": "0.1.0",
  "lockfileVersion": 3,
  "packages": {
    "": { "name": "demo", "version": "0.1.0" },
    "node_modules/left-pad": { "version": "0.1.0" }
  }
}
"#;
        let expected = r#"{
  "name": "demo",
  "version": "0.2.0",
  "lockfileVersion": 3,
  "packages": {
    "": { "name": "demo", "version": "0.2.0" },
    "node_modules/left-pad": { "version": "0.1.0" }
  }
}
"#;
        assert_eq!(set_lock_version(lock, "0.2.0").unwrap(), expected);
    }

    #[test]
    fn sets_a_lock_with_a_bom() {
        let lock = "\u{feff}{ \"version\": \"0.1.0\", \"packages\": { \"\": { \"version\": \"0.1.0\" } } }";
        assert_eq!(
            set_lock_version(lock, "0.2.0").unwrap(),
            lock.replace("0.1.0", "0.2.0")
        );
    }

    #[test]
    fn reads_and_sets_with_a_bom() {
        let text = format!("\u{feff}{PACKAGE}");
        assert_eq!(read_version(&text).unwrap().as_deref(), Some("0.1.0"));
        let out = set_version(&text, "0.2.0").unwrap();
        assert_eq!(
            out,
            format!("\u{feff}{}", PACKAGE.replace("0.1.0", "0.2.0"))
        );
    }
}
