use crate::model::config::LocaleMetadata;

/// Parse keywords.txt content: comma-separated keywords, trimmed.
pub fn parse_keywords(content: &str) -> Vec<String> {
    content
        .split(',')
        .map(|k| k.trim().to_owned())
        .filter(|k| !k.is_empty())
        .collect()
}

/// Parse name.txt content: single line, trimmed, max 30 chars.
pub fn parse_name(content: &str) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(truncate_chars(trimmed, 30))
}

/// Parse subtitle.txt content: single line, trimmed, max 30 chars.
pub fn parse_subtitle(content: &str) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(truncate_chars(trimmed, 30))
}

/// Build LocaleMetadata from raw file contents.
pub fn build_metadata(
    keywords_content: Option<&str>,
    name_content: Option<&str>,
    subtitle_content: Option<&str>,
) -> LocaleMetadata {
    LocaleMetadata {
        keywords: keywords_content.map(parse_keywords).unwrap_or_default(),
        name: name_content.and_then(parse_name),
        subtitle: subtitle_content.and_then(parse_subtitle),
    }
}

/// Truncate a string to `max` characters (not bytes).
fn truncate_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_keywords_normal() {
        let kws = parse_keywords("photo,editor,filter");
        assert_eq!(kws, vec!["photo", "editor", "filter"]);
    }

    #[test]
    fn parse_keywords_with_whitespace() {
        let kws = parse_keywords("  photo , editor ,  filter  ");
        assert_eq!(kws, vec!["photo", "editor", "filter"]);
    }

    #[test]
    fn parse_keywords_empty_input() {
        let kws = parse_keywords("");
        assert!(kws.is_empty());
    }

    #[test]
    fn parse_keywords_only_commas() {
        let kws = parse_keywords(",,,");
        assert!(kws.is_empty());
    }

    #[test]
    fn parse_keywords_single() {
        let kws = parse_keywords("photo");
        assert_eq!(kws, vec!["photo"]);
    }

    #[test]
    fn parse_keywords_trailing_comma() {
        let kws = parse_keywords("photo,editor,");
        assert_eq!(kws, vec!["photo", "editor"]);
    }

    #[test]
    fn parse_name_normal() {
        assert_eq!(parse_name("MyApp"), Some("MyApp".into()));
    }

    #[test]
    fn parse_name_empty() {
        assert_eq!(parse_name(""), None);
    }

    #[test]
    fn parse_name_whitespace_only() {
        assert_eq!(parse_name("   "), None);
    }

    #[test]
    fn parse_name_trimmed() {
        assert_eq!(parse_name("  MyApp  \n"), Some("MyApp".into()));
    }

    #[test]
    fn parse_name_truncated_to_30() {
        let long = "A".repeat(50);
        let result = parse_name(&long).unwrap();
        assert_eq!(result.chars().count(), 30);
    }

    #[test]
    fn parse_subtitle_normal() {
        assert_eq!(parse_subtitle("Fast & Easy"), Some("Fast & Easy".into()));
    }

    #[test]
    fn parse_subtitle_empty() {
        assert_eq!(parse_subtitle(""), None);
    }

    #[test]
    fn parse_subtitle_truncated_to_30() {
        let long = "B".repeat(40);
        let result = parse_subtitle(&long).unwrap();
        assert_eq!(result.chars().count(), 30);
    }

    #[test]
    fn build_metadata_all_provided() {
        let meta = build_metadata(Some("photo,editor"), Some("MyApp"), Some("Fast & Easy"));
        assert_eq!(meta.keywords, vec!["photo", "editor"]);
        assert_eq!(meta.name, Some("MyApp".into()));
        assert_eq!(meta.subtitle, Some("Fast & Easy".into()));
    }

    #[test]
    fn build_metadata_none_provided() {
        let meta = build_metadata(None, None, None);
        assert!(meta.keywords.is_empty());
        assert!(meta.name.is_none());
        assert!(meta.subtitle.is_none());
    }

    #[test]
    fn build_metadata_partial() {
        let meta = build_metadata(Some("photo"), None, Some(""));
        assert_eq!(meta.keywords, vec!["photo"]);
        assert!(meta.name.is_none());
        assert!(meta.subtitle.is_none());
    }

    #[test]
    fn parse_name_unicode_truncation() {
        // 35 CJK characters — should truncate to 30
        let cjk = "写真編集写真編集写真編集写真編集写真編集写真編集写真編集写真編集写真編";
        let result = parse_name(cjk).unwrap();
        assert_eq!(result.chars().count(), 30);
    }
}
