use anyhow::Result;

pub fn read_version(text: &str) -> Result<Option<String>> {
    Ok(Some(text.trim().to_string()))
}

pub fn set_version(text: &str, version: &str) -> Result<String> {
    let start = text.len() - text.trim_start().len();
    let end = text.trim_end().len().max(start);
    Ok(format!("{}{version}{}", &text[..start], &text[end..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_trimmed_text() {
        assert_eq!(read_version("1.2.3\n").unwrap().as_deref(), Some("1.2.3"));
        assert_eq!(
            read_version("\n  1.2.3 \r\n").unwrap().as_deref(),
            Some("1.2.3")
        );
    }

    #[test]
    fn an_empty_file_reads_as_an_empty_version() {
        assert_eq!(read_version(" \n").unwrap().as_deref(), Some(""));
    }

    #[test]
    fn sets_the_version_and_keeps_the_whitespace() {
        assert_eq!(set_version("1.2.3\n", "1.2.4").unwrap(), "1.2.4\n");
        assert_eq!(set_version("1.2.3", "1.2.4").unwrap(), "1.2.4");
        assert_eq!(
            set_version("\n  1.2.3 \r\n", "2.0.0").unwrap(),
            "\n  2.0.0 \r\n"
        );
    }
}
