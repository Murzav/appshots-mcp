use crate::model::config::Caption;
use crate::model::locale::{AsoLocale, Script};

/// A single keyword match found in text.
#[derive(Debug, Clone)]
pub struct KeywordMatch {
    pub keyword: String,
    pub found_in: String,
    pub position: usize,
}

/// Result of matching keywords against captions.
#[derive(Debug, Clone)]
pub struct CoverageReport {
    pub total_keywords: usize,
    pub matched_keywords: usize,
    pub coverage_percent: f64,
    pub matches: Vec<KeywordMatch>,
    pub gaps: Vec<String>,
}

/// Check if the character is a word-boundary character (not alphanumeric).
fn is_boundary(c: char) -> bool {
    !c.is_alphanumeric()
}

/// Word-boundary match for Latin/Cyrillic/Greek/Arabic/Hebrew scripts.
/// Returns the char offset if found, or None.
fn word_boundary_match(text: &str, keyword: &str) -> Option<(usize, &'static str)> {
    let text_lower = text.to_lowercase();
    let kw_lower = keyword.to_lowercase();

    let mut start = 0;
    while let Some(byte_pos) = text_lower[start..].find(&kw_lower) {
        let abs_byte = start + byte_pos;
        let char_pos = text_lower[..abs_byte].chars().count();

        // Check boundary before match
        let before_ok = if abs_byte == 0 {
            true
        } else {
            text_lower[..abs_byte]
                .chars()
                .next_back()
                .is_none_or(is_boundary)
        };

        // Check boundary after match
        let end_byte = abs_byte + kw_lower.len();
        let after_ok = if end_byte >= text_lower.len() {
            true
        } else {
            text_lower[end_byte..]
                .chars()
                .next()
                .is_none_or(is_boundary)
        };

        if before_ok && after_ok {
            return Some((char_pos, ""));
        }

        // Move past this occurrence
        start = abs_byte + kw_lower.len().max(1);
        if start >= text_lower.len() {
            break;
        }
    }
    None
}

/// CJK substring match (case-insensitive for mixed Latin).
fn substring_match(text: &str, keyword: &str) -> Option<usize> {
    let text_lower = text.to_lowercase();
    let kw_lower = keyword.to_lowercase();
    text_lower
        .find(&kw_lower)
        .map(|byte_pos| text_lower[..byte_pos].chars().count())
}

/// Determine which field a char offset falls into, given field boundaries.
fn field_for_offset(title_len: usize, subtitle_len: usize, offset: usize) -> &'static str {
    if offset < title_len {
        "title"
    } else if offset < title_len + 1 + subtitle_len {
        "subtitle"
    } else {
        "keyword"
    }
}

/// Build combined searchable text from a caption, returning field char lengths.
fn combined_text(caption: &Caption) -> (String, usize, usize) {
    let sub = caption.subtitle.as_deref().unwrap_or("");
    let kw = caption.keyword.as_deref().unwrap_or("");
    let title_chars = caption.title.chars().count();
    let sub_chars = sub.chars().count();
    let combined = format!("{} {} {}", caption.title, sub, kw);
    (combined, title_chars, sub_chars)
}

/// Match keywords against a single caption's text fields.
/// Script-aware: CJK uses substring matching (no word boundaries),
/// Latin/Cyrillic/Greek use case-insensitive word-boundary matching.
pub fn match_keywords_in_caption(
    caption: &Caption,
    keywords: &[String],
    script: Script,
) -> Vec<KeywordMatch> {
    let (text, title_chars, sub_chars) = combined_text(caption);
    let use_substring = matches!(script, Script::CJK);

    let mut results = Vec::new();
    for kw in keywords {
        let found = if use_substring {
            substring_match(&text, kw).map(|pos| (pos, ""))
        } else {
            word_boundary_match(&text, kw)
        };

        if let Some((pos, _)) = found {
            let field = field_for_offset(title_chars, sub_chars, pos);
            results.push(KeywordMatch {
                keyword: kw.clone(),
                found_in: field.to_owned(),
                position: pos,
            });
        }
    }
    results
}

/// Calculate coverage across all captions for a locale.
pub fn coverage_report(
    captions: &[Caption],
    keywords: &[String],
    locale: &AsoLocale,
) -> CoverageReport {
    if keywords.is_empty() {
        return CoverageReport {
            total_keywords: 0,
            matched_keywords: 0,
            coverage_percent: 100.0,
            matches: Vec::new(),
            gaps: Vec::new(),
        };
    }

    let script = locale.script();
    let mut all_matches = Vec::new();
    let mut matched_set: Vec<bool> = vec![false; keywords.len()];

    for caption in captions {
        let caption_matches = match_keywords_in_caption(caption, keywords, script);
        for m in &caption_matches {
            if let Some(idx) = keywords
                .iter()
                .position(|k| k.to_lowercase() == m.keyword.to_lowercase())
            {
                matched_set[idx] = true;
            }
        }
        all_matches.extend(caption_matches);
    }

    let matched_count = matched_set.iter().filter(|&&b| b).count();
    let gaps: Vec<String> = keywords
        .iter()
        .enumerate()
        .filter(|(i, _)| !matched_set[*i])
        .map(|(_, k)| k.clone())
        .collect();

    let coverage = (matched_count as f64 / keywords.len() as f64) * 100.0;

    CoverageReport {
        total_keywords: keywords.len(),
        matched_keywords: matched_count,
        coverage_percent: coverage,
        matches: all_matches,
        gaps,
    }
}

/// Find keywords not covered by any caption.
pub fn find_gaps(captions: &[Caption], keywords: &[String], locale: &AsoLocale) -> Vec<String> {
    coverage_report(captions, keywords, locale).gaps
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_caption(title: &str, subtitle: Option<&str>, keyword: Option<&str>) -> Caption {
        Caption {
            mode: 1,
            title: title.to_owned(),
            subtitle: subtitle.map(|s| s.to_owned()),
            keyword: keyword.map(|s| s.to_owned()),
        }
    }

    // 1. Latin exact word match
    #[test]
    fn latin_exact_word_match() {
        let caption = make_caption("Track Glucose Levels", None, None);
        let matches = match_keywords_in_caption(&caption, &["glucose".to_owned()], Script::Latin);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].keyword, "glucose");
        assert_eq!(matches[0].found_in, "title");
    }

    // 2. Latin no partial match
    #[test]
    fn latin_no_partial_match() {
        let caption = make_caption("Backtracking Data", None, None);
        let matches = match_keywords_in_caption(&caption, &["track".to_owned()], Script::Latin);
        assert!(matches.is_empty());
    }

    // 3. Latin case insensitive
    #[test]
    fn latin_case_insensitive() {
        let caption = make_caption("glucose tracker", None, None);
        let matches = match_keywords_in_caption(&caption, &["GLUCOSE".to_owned()], Script::Latin);
        assert_eq!(matches.len(), 1);
    }

    // 4. Latin multi-word keyword
    #[test]
    fn latin_multi_word_keyword() {
        let caption = make_caption("Monitor Blood Sugar", None, None);
        let matches =
            match_keywords_in_caption(&caption, &["blood sugar".to_owned()], Script::Latin);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].keyword, "blood sugar");
    }

    // 5. CJK substring match
    #[test]
    fn cjk_substring_match() {
        let caption = make_caption("血糖トラッカー", None, None);
        let matches = match_keywords_in_caption(&caption, &["血糖".to_owned()], Script::CJK);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].keyword, "血糖");
    }

    // 6. CJK mixed with Latin
    #[test]
    fn cjk_mixed_with_latin() {
        let caption = make_caption("Glucose血糖", None, None);
        let matches = match_keywords_in_caption(&caption, &["Glucose".to_owned()], Script::CJK);
        assert_eq!(matches.len(), 1);
    }

    // 7. Coverage report partial
    #[test]
    fn coverage_report_partial() {
        let captions = vec![
            make_caption("Track Glucose Levels", None, None),
            make_caption("Monitor Blood Sugar", None, None),
        ];
        let keywords: Vec<String> = vec![
            "glucose".into(),
            "blood sugar".into(),
            "insulin".into(),
            "diabetes".into(),
            "health".into(),
        ];
        let report = coverage_report(&captions, &keywords, &AsoLocale::EnUs);
        assert_eq!(report.total_keywords, 5);
        assert_eq!(report.matched_keywords, 2);
        assert!((report.coverage_percent - 40.0).abs() < 0.01);
        assert_eq!(report.gaps.len(), 3);
    }

    // 8. Full coverage
    #[test]
    fn full_coverage() {
        let captions = vec![make_caption(
            "Track Glucose",
            Some("Blood Sugar Monitor"),
            None,
        )];
        let keywords: Vec<String> = vec!["glucose".into(), "blood sugar".into(), "monitor".into()];
        let report = coverage_report(&captions, &keywords, &AsoLocale::EnUs);
        assert_eq!(report.matched_keywords, 3);
        assert!((report.coverage_percent - 100.0).abs() < 0.01);
        assert!(report.gaps.is_empty());
    }

    // 9. No coverage
    #[test]
    fn no_coverage() {
        let captions = vec![make_caption("Hello World", None, None)];
        let keywords: Vec<String> = vec!["glucose".into(), "insulin".into()];
        let report = coverage_report(&captions, &keywords, &AsoLocale::EnUs);
        assert_eq!(report.matched_keywords, 0);
        assert!((report.coverage_percent - 0.0).abs() < 0.01);
        assert_eq!(report.gaps.len(), 2);
    }

    // 10. find_gaps returns only unmatched
    #[test]
    fn find_gaps_returns_unmatched() {
        let captions = vec![make_caption("Track Glucose", None, None)];
        let keywords: Vec<String> = vec!["glucose".into(), "insulin".into()];
        let gaps = find_gaps(&captions, &keywords, &AsoLocale::EnUs);
        assert_eq!(gaps, vec!["insulin"]);
    }

    // 11. Empty keywords list
    #[test]
    fn empty_keywords_full_coverage() {
        let captions = vec![make_caption("Something", None, None)];
        let report = coverage_report(&captions, &[], &AsoLocale::EnUs);
        assert!((report.coverage_percent - 100.0).abs() < 0.01);
        assert!(report.gaps.is_empty());
    }

    // 12. Empty captions
    #[test]
    fn empty_captions_all_gaps() {
        let keywords: Vec<String> = vec!["glucose".into(), "insulin".into()];
        let report = coverage_report(&[], &keywords, &AsoLocale::EnUs);
        assert_eq!(report.matched_keywords, 0);
        assert_eq!(report.gaps.len(), 2);
    }
}
