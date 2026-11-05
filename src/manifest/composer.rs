use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use jsonc_parser::cst::CstNode;

use super::{existing, package_json};
use crate::version::parse_version as parse_semver;

const FILE: &str = "composer.json";
const LOCK: &str = "composer.lock";

/// The pre-release labels Composer knows, in lowercase.
const LABELS: [&str; 5] = ["alpha", "beta", "rc", "patch", "dev"];

/// Reads a version; a pre-release must carry a label that Composer takes.
pub fn parse_version(text: &str) -> Result<semver::Version> {
    let version = parse_semver(text)?;
    let label = version
        .pre
        .as_str()
        .split(|c: char| !c.is_ascii_alphabetic())
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !version.pre.is_empty() && !LABELS.contains(&label.as_str()) {
        bail!("\"{text}\": Composer takes only the labels alpha, beta, RC, patch and dev");
    }
    Ok(version)
}

/// The top-level keys Composer hashes, `config.platform` and `config.policy` besides.
const HASHED_KEYS: [&str; 11] = [
    "name",
    "version",
    "require",
    "require-dev",
    "conflict",
    "replace",
    "provide",
    "minimum-stability",
    "prefer-stable",
    "repositories",
    "extra",
];
const HASHED_CONFIG_KEYS: [&str; 2] = ["platform", "policy"];

/// `Locker::getContentHash` of Composer for the `composer.json` in `text`.
pub fn content_hash(text: &str) -> Result<String> {
    let root = package_json::parse(text)?;
    let object = root.object_value().context("not an object")?;
    let mut fields: Vec<(String, CstNode)> = Vec::new();
    for prop in object.properties() {
        let (Some(name), Some(value)) = (prop.decoded_name(), prop.value()) else {
            continue;
        };
        if HASHED_KEYS.contains(&name.as_str()) {
            fields.push((name, value));
        }
    }
    let mut out = String::new();
    let config = object.object_value("config").map(|config| {
        let mut parts = String::from("{");
        for key in HASHED_CONFIG_KEYS {
            let Some(value) = config.get(key).and_then(|prop| prop.value()) else {
                continue;
            };
            if value.as_null_keyword().is_some() {
                continue;
            }
            if parts.len() > 1 {
                parts.push(',');
            }
            php_string(key, &mut parts);
            parts.push(':');
            php_json(&value, &mut parts)?;
        }
        parts.push('}');
        Ok::<_, anyhow::Error>(parts)
    });
    let config = config.transpose()?.filter(|parts| parts != "{}");
    let mut entries: Vec<(String, String)> = Vec::new();
    for (name, value) in &fields {
        let mut encoded = String::new();
        php_json(value, &mut encoded)?;
        entries.push((name.clone(), encoded));
    }
    entries.extend(config.map(|parts| ("config".to_string(), parts)));
    entries.sort();
    out.push('{');
    for (index, (name, encoded)) in entries.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        php_string(name, &mut out);
        out.push(':');
        out.push_str(encoded);
    }
    out.push('}');
    Ok(format!("{:x}", md5::compute(out)))
}

/// `json_encode($value, 0)` of the PHP array that `json_decode($json, true)` makes of `node`.
fn php_json(node: &CstNode, out: &mut String) -> Result<()> {
    if let Some(object) = node.as_object() {
        let mut entries = Vec::new();
        for prop in object.properties() {
            let (Some(name), Some(value)) = (prop.decoded_name(), prop.value()) else {
                bail!("unreadable property");
            };
            entries.push((name, value));
        }
        let is_list = entries
            .iter()
            .enumerate()
            .all(|(index, (name, _))| *name == index.to_string());
        if is_list {
            out.push('[');
        } else {
            out.push('{');
        }
        for (index, (name, value)) in entries.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            if !is_list {
                php_string(name, out);
                out.push(':');
            }
            php_json(value, out)?;
        }
        out.push(if is_list { ']' } else { '}' });
    } else if let Some(array) = node.as_array() {
        out.push('[');
        for (index, value) in array.elements().iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            php_json(value, out)?;
        }
        out.push(']');
    } else if let Some(string) = node.as_string_lit() {
        php_string(&string.decoded_value()?, out);
    } else if let Some(number) = node.as_number_lit() {
        out.push_str(&number.to_string());
    } else if let Some(boolean) = node.as_boolean_lit() {
        out.push_str(if boolean.value() { "true" } else { "false" });
    } else if node.as_null_keyword().is_some() {
        out.push_str("null");
    } else {
        bail!("unsupported JSON value");
    }
    Ok(())
}

/// `text` as a JSON string with the escapes of PHP's `json_encode` and no flags.
fn php_string(text: &str, out: &mut String) {
    use std::fmt::Write;
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '/' => out.push_str("\\/"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' || !c.is_ascii() => {
                for unit in c.encode_utf16(&mut [0; 2]) {
                    let _ = write!(out, "\\u{unit:04x}");
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn lock_hash(lock: &str) -> Result<Option<String>> {
    let root = package_json::parse(lock)?;
    match root.object_value() {
        Some(object) => package_json::string_field(&object, "content-hash"),
        None => Ok(None),
    }
}

/// The `composer.lock` next to `composer.json`, after checking that its `content-hash` is the one kraken computes.
pub fn lock_file(root: &Path) -> Result<Option<String>> {
    let Some(name) = existing(root, LOCK) else {
        return Ok(None);
    };
    let lock = fs::read_to_string(root.join(&name)).context(name.clone())?;
    let composer = fs::read_to_string(root.join(FILE)).context(FILE)?;
    let Some(hash) = lock_hash(&lock).context(name.clone())? else {
        bail!("{LOCK} has no content-hash; run `composer update --lock` first");
    };
    if hash != content_hash(&composer).context(FILE)? {
        bail!("{LOCK} does not match {FILE}; run `composer update --lock` first");
    }
    Ok(Some(name))
}

/// `lock` with the `content-hash` of the `composer.json` in `root`.
pub fn set_lock_hash(root: &Path, lock: &str) -> Result<String> {
    let composer = fs::read_to_string(root.join(FILE)).context(FILE)?;
    let hash = content_hash(&composer).context(FILE)?;
    let parsed = package_json::parse(lock)?;
    package_json::set_string(parsed.object_value(), "content-hash", &hash);
    Ok(package_json::render(lock, &parsed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_the_labels_composer_knows() {
        for text in [
            "1.4.0-rc.1",
            "1.4.0-RC1",
            "1.4.0-beta.2",
            "1.4.0-alpha.1",
            "1.4.0-dev",
            "1.4.0",
        ] {
            assert_eq!(parse_version(text).unwrap().to_string(), text);
        }
    }

    #[test]
    fn refuses_a_label_composer_does_not_take() {
        for text in ["1.4.0-preview.1", "1.4.0-SNAPSHOT", "1.4.0-1"] {
            let err = parse_version(text).unwrap_err().to_string();
            assert_eq!(
                err,
                format!(
                    "\"{text}\": Composer takes only the labels alpha, beta, RC, patch and dev"
                )
            );
        }
    }

    const COMPOSER: &str = r#"{
    "name": "acme/widget",
    "description": "A widget",
    "version": "1.2.3",
    "type": "wordpress-plugin",
    "require": {
        "php": ">=8.1",
        "psr/log": "^3.0"
    },
    "require-dev": {},
    "autoload": { "psr-4": { "Acme\\Widget\\": "src/" } },
    "extra": { "author": "Jürgen", "branch-alias": { "dev-main": "1.x-dev" } },
    "config": { "sort-packages": true, "platform": { "php": "8.1" } }
}
"#;
    const ENCODED: &str = r#"{"config":{"platform":{"php":"8.1"}},"extra":{"author":"J\u00fcrgen","branch-alias":{"dev-main":"1.x-dev"}},"name":"acme\/widget","require":{"php":">=8.1","psr\/log":"^3.0"},"require-dev":[],"version":"1.2.3"}"#;
    const HASH: &str = "ba6abe21b53dc5723447b7e88299a7b4";
    const HASH_124: &str = "9ccd3108f58143b0fc888427a47fde1c";

    fn lock(hash: &str) -> String {
        format!(
            "{{\n    \"_readme\": [\"x\"],\n    \"content-hash\": \"{hash}\",\n    \"packages\": [{{ \"name\": \"psr/log\", \"version\": \"3.0.0\" }}],\n    \"plugin-api-version\": \"2.6.0\"\n}}\n"
        )
    }

    fn project(composer: &str, lock: Option<&str>) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(FILE), composer).unwrap();
        if let Some(lock) = lock {
            fs::write(dir.path().join(LOCK), lock).unwrap();
        }
        dir
    }

    fn encode(json: &str) -> String {
        let root = package_json::parse(json).unwrap();
        let mut out = String::new();
        php_json(&root.value().unwrap(), &mut out).unwrap();
        out
    }

    fn escaped(text: &str) -> String {
        let mut out = String::new();
        php_string(text, &mut out);
        out[1..out.len() - 1].to_string()
    }

    #[test]
    fn encodes_like_php_json_encode() {
        let relevant = r#"{"config":{"platform":{"php":"8.1"}},"extra":{"author":"Jürgen","branch-alias":{"dev-main":"1.x-dev"}},"name":"acme/widget","require":{"php":">=8.1","psr/log":"^3.0"},"require-dev":{},"version":"1.2.3"}"#;
        assert_eq!(encode(relevant), ENCODED);
    }

    #[test]
    fn hashes_the_fields_composer_hashes() {
        assert_eq!(content_hash(COMPOSER).unwrap(), HASH);
        let next = COMPOSER.replace("1.2.3", "1.2.4");
        assert_eq!(content_hash(&next).unwrap(), HASH_124);
    }

    const ALL_KEYS: &str = r#"{
    "version": "1.0.0",
    "repositories": [{ "type": "vcs", "url": "https://x.test/r" }],
    "prefer-stable": true,
    "minimum-stability": "dev",
    "provide": { "a/b": "1.0" },
    "replace": { "c/d": "1.0" },
    "conflict": { "e/f": "<1" },
    "require-dev": { "g/h": "^1" },
    "require": { "i/j": "^2" },
    "name": "acme/all",
    "extra": { "k": 1 },
    "config": { "policy": { "block": true }, "platform": { "php": "8.1" } }
}"#;
    const ALL_KEYS_ENCODED: &str = r#"{"config":{"platform":{"php":"8.1"},"policy":{"block":true}},"conflict":{"e\/f":"<1"},"extra":{"k":1},"minimum-stability":"dev","name":"acme\/all","prefer-stable":true,"provide":{"a\/b":"1.0"},"replace":{"c\/d":"1.0"},"repositories":[{"type":"vcs","url":"https:\/\/x.test\/r"}],"require":{"i\/j":"^2"},"require-dev":{"g\/h":"^1"},"version":"1.0.0"}"#;

    fn md5_hex(text: &str) -> String {
        format!("{:x}", md5::compute(text))
    }

    #[test]
    fn hashes_all_eleven_keys_and_both_config_keys() {
        assert_eq!(content_hash(ALL_KEYS).unwrap(), md5_hex(ALL_KEYS_ENCODED));
    }

    #[test]
    fn config_policy_changes_the_hash() {
        let next = ALL_KEYS.replace("\"block\": true", "\"block\": false");
        assert_ne!(
            content_hash(&next).unwrap(),
            content_hash(ALL_KEYS).unwrap()
        );
    }

    #[test]
    fn top_level_null_is_hashed() {
        let text = r#"{ "version": "1.0.0", "extra": null }"#;
        assert_eq!(
            content_hash(text).unwrap(),
            md5_hex(r#"{"extra":null,"version":"1.0.0"}"#)
        );
    }

    #[test]
    fn config_without_platform_or_policy_is_left_out() {
        let text = r#"{ "version": "1.0.0", "config": { "sort-packages": true } }"#;
        assert_eq!(
            content_hash(text).unwrap(),
            md5_hex(r#"{"version":"1.0.0"}"#)
        );
    }

    #[test]
    fn null_config_keys_are_left_out() {
        let only_null = r#"{ "version": "1.0.0", "config": { "platform": null } }"#;
        assert_eq!(
            content_hash(only_null).unwrap(),
            md5_hex(r#"{"version":"1.0.0"}"#)
        );
        let mixed = r#"{ "version": "1.0.0", "config": { "platform": null, "policy": { "block": true } } }"#;
        assert_eq!(
            content_hash(mixed).unwrap(),
            md5_hex(r#"{"config":{"policy":{"block":true}},"version":"1.0.0"}"#)
        );
    }

    #[test]
    fn other_fields_do_not_change_the_hash() {
        let text = COMPOSER
            .replace("A widget", "Another widget")
            .replace("src/", "lib/")
            .replace("sort-packages", "optimize-autoloader");
        assert_eq!(content_hash(&text).unwrap(), HASH);
    }

    #[test]
    fn escapes_strings_like_php() {
        assert_eq!(escaped("a/b\"c\\d"), r#"a\/b\"c\\d"#);
        assert_eq!(escaped("\n\r\t\u{8}\u{c}"), r"\n\r\t\b\f");
        assert_eq!(escaped("\u{1}\u{1f}"), r"\u0001\u001f");
        assert_eq!(escaped("\u{fc}"), r"\u00fc");
        assert_eq!(escaped("\u{1f600}"), r"\ud83d\ude00");
        assert_eq!(escaped("\u{7f}"), "\u{7f}");
    }

    #[test]
    fn empty_object_is_an_empty_list() {
        assert_eq!(encode(r#"{"a": {}, "b": []}"#), r#"{"a":[],"b":[]}"#);
    }

    #[test]
    fn index_keys_are_a_list() {
        assert_eq!(encode(r#"{"0": "a", "1": "b"}"#), r#"["a","b"]"#);
        assert_eq!(encode(r#"{"1": "a", "0": "b"}"#), r#"{"1":"a","0":"b"}"#);
    }

    #[test]
    fn keeps_numbers_and_literals_as_written() {
        assert_eq!(
            encode(r#"[1, 2.5, -3, true, false, null]"#),
            "[1,2.5,-3,true,false,null]"
        );
    }

    #[test]
    fn sets_only_the_content_hash() {
        let dir = project(COMPOSER, None);
        let old = lock("0123456789abcdef0123456789abcdef");
        let expected = old.replace("0123456789abcdef0123456789abcdef", HASH);
        assert_eq!(set_lock_hash(dir.path(), &old).unwrap(), expected);
    }

    #[test]
    fn sets_the_content_hash_of_a_lock_with_a_bom() {
        let dir = project(COMPOSER, None);
        let old = format!("\u{feff}{}", lock(HASH_124));
        assert_eq!(
            set_lock_hash(dir.path(), &old).unwrap(),
            old.replace(HASH_124, HASH)
        );
    }

    #[test]
    fn accepts_a_lock_that_matches() {
        let dir = project(COMPOSER, Some(&lock(HASH)));
        assert_eq!(lock_file(dir.path()).unwrap().as_deref(), Some(LOCK));
    }

    #[test]
    fn refuses_a_lock_that_does_not_match() {
        let dir = project(COMPOSER, Some(&lock(HASH_124)));
        let err = lock_file(dir.path()).unwrap_err().to_string();
        assert_eq!(
            err,
            "composer.lock does not match composer.json; run `composer update --lock` first"
        );
    }

    #[test]
    fn refuses_a_lock_without_content_hash() {
        let dir = project(COMPOSER, Some(r#"{ "hash": "abc", "packages": [] }"#));
        let err = lock_file(dir.path()).unwrap_err().to_string();
        assert_eq!(
            err,
            "composer.lock has no content-hash; run `composer update --lock` first"
        );
    }

    #[test]
    fn no_lock_file_is_none() {
        let dir = project(COMPOSER, None);
        assert_eq!(lock_file(dir.path()).unwrap(), None);
    }
}
