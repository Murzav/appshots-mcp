use std::fmt::Write;

use indexmap::IndexMap;

use crate::error::AppShotsError;

const PLIST_HEADER: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
"#;

const PLIST_FOOTER: &str = "</plist>\n";

/// Build an XML plist string from JSON key-value pairs.
///
/// Type mapping:
/// - `String` → `<string>` (but if starts with `"base64:"` → `<data>` with the base64 payload)
/// - `Number` (integer) → `<integer>`
/// - `Number` (float) → `<real>`
/// - `Bool` → `<true/>` or `<false/>`
/// - `Array` → `<array>` (recurse)
/// - `Object` → `<dict>` (recurse)
/// - `Null` → skipped
pub(crate) fn build_xml_plist(
    entries: &IndexMap<String, serde_json::Value>,
) -> Result<String, AppShotsError> {
    let mut out = String::with_capacity(256);
    out.push_str(PLIST_HEADER);
    write_dict(&mut out, entries, 0)?;
    out.push_str(PLIST_FOOTER);
    Ok(out)
}

fn write_dict(
    out: &mut String,
    entries: &IndexMap<String, serde_json::Value>,
    depth: usize,
) -> Result<(), AppShotsError> {
    let indent = "\t".repeat(depth);
    let _ = writeln!(out, "{indent}<dict>");
    for (key, value) in entries {
        if value.is_null() {
            continue;
        }
        let _ = writeln!(out, "{indent}\t<key>{}</key>", xml_escape(key));
        write_value(out, value, depth + 1)?;
    }
    let _ = writeln!(out, "{indent}</dict>");
    Ok(())
}

fn write_value(
    out: &mut String,
    value: &serde_json::Value,
    depth: usize,
) -> Result<(), AppShotsError> {
    let indent = "\t".repeat(depth);
    match value {
        serde_json::Value::Null => {}
        serde_json::Value::Bool(b) => {
            if *b {
                let _ = writeln!(out, "{indent}\t<true/>");
            } else {
                let _ = writeln!(out, "{indent}\t<false/>");
            }
        }
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                let _ = writeln!(out, "{indent}\t<integer>{i}</integer>");
            } else if let Some(u) = n.as_u64() {
                let _ = writeln!(out, "{indent}\t<integer>{u}</integer>");
            } else if let Some(f) = n.as_f64() {
                let _ = writeln!(out, "{indent}\t<real>{f}</real>");
            }
        }
        serde_json::Value::String(s) => {
            if let Some(payload) = s.strip_prefix("base64:") {
                validate_base64(payload)?;
                let _ = writeln!(out, "{indent}\t<data>{payload}</data>");
            } else {
                let _ = writeln!(out, "{indent}\t<string>{}</string>", xml_escape(s));
            }
        }
        serde_json::Value::Array(arr) => {
            let _ = writeln!(out, "{indent}\t<array>");
            for item in arr {
                write_value(out, item, depth + 1)?;
            }
            let _ = writeln!(out, "{indent}\t</array>");
        }
        serde_json::Value::Object(map) => {
            // Convert to IndexMap to preserve order
            let entries: IndexMap<String, serde_json::Value> =
                map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            write_dict(out, &entries, depth + 1)?;
        }
    }
    Ok(())
}

/// Minimal XML escaping for plist text content.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Validate that a string is valid base64 (standard alphabet + padding).
fn validate_base64(s: &str) -> Result<(), AppShotsError> {
    if s.is_empty() {
        return Err(AppShotsError::InvalidFormat("base64 data is empty".into()));
    }
    // Strip whitespace for validation
    let clean: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if clean.is_empty() {
        return Err(AppShotsError::InvalidFormat(
            "base64 data is empty after stripping whitespace".into(),
        ));
    }
    for c in clean.chars() {
        if !c.is_ascii_alphanumeric() && c != '+' && c != '/' && c != '=' {
            return Err(AppShotsError::InvalidFormat(format!(
                "invalid base64 character: '{c}'"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn build(entries: &[(&str, serde_json::Value)]) -> Result<String, AppShotsError> {
        let map: IndexMap<String, serde_json::Value> = entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        build_xml_plist(&map)
    }

    #[test]
    fn string_value() {
        let result = build(&[("name", json!("hello"))]).unwrap();
        assert!(result.contains("<key>name</key>"));
        assert!(result.contains("<string>hello</string>"));
    }

    #[test]
    fn integer_value() {
        let result = build(&[("count", json!(42))]).unwrap();
        assert!(result.contains("<integer>42</integer>"));
    }

    #[test]
    fn negative_integer() {
        let result = build(&[("offset", json!(-7))]).unwrap();
        assert!(result.contains("<integer>-7</integer>"));
    }

    #[test]
    fn float_value() {
        let result = build(&[("pi", json!(3.14))]).unwrap();
        assert!(result.contains("<real>3.14</real>"));
    }

    #[test]
    fn bool_true() {
        let result = build(&[("enabled", json!(true))]).unwrap();
        assert!(result.contains("<true/>"));
    }

    #[test]
    fn bool_false() {
        let result = build(&[("enabled", json!(false))]).unwrap();
        assert!(result.contains("<false/>"));
    }

    #[test]
    fn array_value() {
        let result = build(&[("items", json!(["a", "b"]))]).unwrap();
        assert!(result.contains("<array>"));
        assert!(result.contains("<string>a</string>"));
        assert!(result.contains("<string>b</string>"));
        assert!(result.contains("</array>"));
    }

    #[test]
    fn dict_value() {
        let result = build(&[("nested", json!({"key": "val"}))]).unwrap();
        assert!(result.contains("<dict>"));
        assert!(result.contains("<key>key</key>"));
        assert!(result.contains("<string>val</string>"));
    }

    #[test]
    fn nested_structures() {
        let result = build(&[("outer", json!({"inner": [1, 2, {"deep": true}]}))]).unwrap();
        assert!(result.contains("<integer>1</integer>"));
        assert!(result.contains("<integer>2</integer>"));
        assert!(result.contains("<key>deep</key>"));
        assert!(result.contains("<true/>"));
    }

    #[test]
    fn null_skipped() {
        let result = build(&[("present", json!("yes")), ("absent", json!(null))]).unwrap();
        assert!(result.contains("<key>present</key>"));
        assert!(!result.contains("<key>absent</key>"));
    }

    #[test]
    fn base64_data() {
        let result = build(&[("icon", json!("base64:SGVsbG8="))]).unwrap();
        assert!(result.contains("<data>SGVsbG8=</data>"));
        assert!(!result.contains("<string>"));
    }

    #[test]
    fn base64_invalid_chars() {
        let result = build(&[("bad", json!("base64:!!!"))]);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("invalid base64 character"));
    }

    #[test]
    fn base64_empty() {
        let result = build(&[("empty", json!("base64:"))]);
        assert!(result.is_err());
    }

    #[test]
    fn empty_map() {
        let result = build(&[]).unwrap();
        assert!(result.contains("<dict>"));
        assert!(result.contains("</dict>"));
        assert!(result.contains("<plist version=\"1.0\">"));
    }

    #[test]
    fn xml_special_chars_escaped() {
        let result = build(&[("html", json!("<b>bold & italic</b>"))]).unwrap();
        assert!(result.contains("&lt;b&gt;bold &amp; italic&lt;/b&gt;"));
    }

    #[test]
    fn plist_has_proper_header() {
        let result = build(&[("key", json!("val"))]).unwrap();
        assert!(result.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(result.contains("<!DOCTYPE plist"));
        assert!(result.contains("<plist version=\"1.0\">"));
        assert!(result.ends_with("</plist>\n"));
    }
}
