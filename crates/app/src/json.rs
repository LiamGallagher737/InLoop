//! Writing JSON config files that belong to the project (`.claude/settings.json`,
//! `.mcp.json`) in the style they already use, so `inloop init` changes only
//! what it means to change.

use serde::Serialize;
use serde_json::Value;
use serde_json::ser::{PrettyFormatter, Serializer};

/// Two spaces, for a new file or one with no indented line to go by.
const DEFAULT_INDENT: &str = "  ";

/// The indentation `existing` uses: the leading whitespace of its first
/// indented line, which in a pretty-printed file is one level.
pub fn detect_indent(existing: &str) -> Option<String> {
    existing.lines().find_map(|line| {
        let rest = line.trim_start_matches([' ', '\t']);
        let ws = &line[..line.len() - rest.len()];
        (!ws.is_empty() && !rest.is_empty()).then(|| ws.to_string())
    })
}

/// `doc` pretty-printed with the indentation of `existing` (the file's
/// current text, if any), ending in a newline.
pub fn to_string_like(doc: &Value, existing: Option<&str>) -> Result<String, serde_json::Error> {
    let indent = existing
        .and_then(detect_indent)
        .unwrap_or_else(|| DEFAULT_INDENT.to_string());
    let mut out = Vec::new();
    let mut ser =
        Serializer::with_formatter(&mut out, PrettyFormatter::with_indent(indent.as_bytes()));
    doc.serialize(&mut ser)?;
    // serde_json writes valid UTF-8; lossy only to avoid an unwrap.
    let mut text = String::from_utf8_lossy(&out).into_owned();
    text.push('\n');
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keeps_tabs() {
        let old = "{\n\t\"a\": {\n\t\t\"b\": 1\n\t}\n}\n";
        let v: Value = serde_json::from_str(old).unwrap();
        assert_eq!(to_string_like(&v, Some(old)).unwrap(), old);
    }

    #[test]
    fn keeps_four_spaces() {
        let old = "{\n    \"a\": [\n        1\n    ]\n}\n";
        let v: Value = serde_json::from_str(old).unwrap();
        assert_eq!(to_string_like(&v, Some(old)).unwrap(), old);
    }

    #[test]
    fn defaults_to_two_spaces() {
        let v = json!({ "a": 1 });
        assert_eq!(to_string_like(&v, None).unwrap(), "{\n  \"a\": 1\n}\n");
        assert_eq!(
            to_string_like(&v, Some("{}")).unwrap(),
            "{\n  \"a\": 1\n}\n"
        );
    }
}
