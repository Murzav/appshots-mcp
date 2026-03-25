use std::collections::BTreeMap;
use std::path::Path;

use tokio::sync::Mutex;

use crate::error::AppShotsError;
use crate::io::FileStore;

/// Glossary: maps "source→target" locale pairs to term translations.
pub type Glossary = BTreeMap<String, BTreeMap<String, String>>;

fn locale_pair_key(source: &str, target: &str) -> String {
    format!("{source}\u{2192}{target}")
}

/// Parse raw JSON into a Glossary. `None` or empty string yields an empty glossary.
pub fn parse_glossary(raw: Option<&str>) -> Result<Glossary, AppShotsError> {
    match raw {
        None | Some("") => Ok(Glossary::new()),
        Some(s) => serde_json::from_str(s).map_err(|e| AppShotsError::JsonParse(e.to_string())),
    }
}

/// Serialize a Glossary to pretty-printed JSON.
pub fn serialize_glossary(glossary: &Glossary) -> Result<String, AppShotsError> {
    serde_json::to_string_pretty(glossary).map_err(|e| AppShotsError::JsonParse(e.to_string()))
}

/// Get glossary entries, optionally filtered by locale pair and/or substring.
pub(crate) async fn handle_get_glossary(
    store: &dyn FileStore,
    glossary_path: &Path,
    source_locale: Option<&str>,
    target_locale: Option<&str>,
    filter: Option<&str>,
) -> Result<serde_json::Value, AppShotsError> {
    let glossary = if store.exists(glossary_path) {
        let raw = store.read(glossary_path)?;
        parse_glossary(Some(&raw))?
    } else {
        Glossary::new()
    };

    let filtered: Glossary = glossary
        .into_iter()
        .filter(|(key, _)| match (source_locale, target_locale) {
            (Some(src), Some(tgt)) => *key == locale_pair_key(src, tgt),
            (Some(src), None) => key.starts_with(&format!("{src}\u{2192}")),
            (None, Some(tgt)) => key.ends_with(&format!("\u{2192}{tgt}")),
            (None, None) => true,
        })
        .map(|(key, entries)| {
            let filtered_entries = match filter {
                Some(f) => {
                    let f_lower = f.to_lowercase();
                    entries
                        .into_iter()
                        .filter(|(term, translation)| {
                            term.to_lowercase().contains(&f_lower)
                                || translation.to_lowercase().contains(&f_lower)
                        })
                        .collect()
                }
                None => entries,
            };
            (key, filtered_entries)
        })
        .filter(|(_, entries)| !entries.is_empty())
        .collect();

    serde_json::to_value(&filtered).map_err(|e| AppShotsError::JsonParse(e.to_string()))
}

/// Update glossary entries for a locale pair. Merges with existing entries.
pub(crate) async fn handle_update_glossary(
    store: &dyn FileStore,
    glossary_write_lock: &Mutex<()>,
    glossary_path: &Path,
    source_locale: &str,
    target_locale: &str,
    entries: BTreeMap<String, String>,
) -> Result<serde_json::Value, AppShotsError> {
    let _guard = glossary_write_lock.lock().await;

    let mut glossary = if store.exists(glossary_path) {
        let raw = store.read(glossary_path)?;
        parse_glossary(Some(&raw))?
    } else {
        Glossary::new()
    };

    let key = locale_pair_key(source_locale, target_locale);
    let locale_entries = glossary.entry(key).or_default();
    for (term, translation) in &entries {
        locale_entries.insert(term.clone(), translation.clone());
    }

    let json = serialize_glossary(&glossary)?;
    store.create_parent_dirs(glossary_path)?;
    store.write(glossary_path, &json)?;

    let added = entries.len();
    let total = glossary.values().map(|e| e.len()).sum::<usize>();

    Ok(serde_json::json!({
        "source_locale": source_locale,
        "target_locale": target_locale,
        "entries_updated": added,
        "total_entries": total,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::memory::MemoryStore;
    use std::path::PathBuf;

    fn glossary_path() -> PathBuf {
        PathBuf::from("/project/glossary.json")
    }

    #[test]
    fn parse_none_returns_empty() {
        let g = parse_glossary(None).unwrap();
        assert!(g.is_empty());
    }

    #[test]
    fn parse_empty_string_returns_empty() {
        let g = parse_glossary(Some("")).unwrap();
        assert!(g.is_empty());
    }

    #[test]
    fn parse_invalid_json_errors() {
        let err = parse_glossary(Some("{bad}")).unwrap_err();
        assert!(matches!(err, AppShotsError::JsonParse(_)));
    }

    #[test]
    fn parse_serialize_roundtrip() {
        let mut inner = BTreeMap::new();
        inner.insert("hello".to_owned(), "hola".to_owned());
        inner.insert("world".to_owned(), "mundo".to_owned());

        let mut glossary = Glossary::new();
        glossary.insert(locale_pair_key("en-US", "es-ES"), inner);

        let json = serialize_glossary(&glossary).unwrap();
        let parsed = parse_glossary(Some(&json)).unwrap();
        assert_eq!(parsed, glossary);
    }

    #[tokio::test]
    async fn get_empty_glossary_no_file() {
        let store = MemoryStore::new();
        let result = handle_get_glossary(&store, &glossary_path(), None, None, None)
            .await
            .unwrap();
        let obj = result.as_object().unwrap();
        assert!(obj.is_empty());
    }

    #[tokio::test]
    async fn update_then_get_roundtrip() {
        let store = MemoryStore::new();
        let lock = Mutex::new(());
        let path = glossary_path();

        let mut entries = BTreeMap::new();
        entries.insert("hello".to_owned(), "hola".to_owned());
        entries.insert("goodbye".to_owned(), "adiós".to_owned());

        let update_result = handle_update_glossary(&store, &lock, &path, "en-US", "es-ES", entries)
            .await
            .unwrap();
        assert_eq!(update_result["entries_updated"], 2);
        assert_eq!(update_result["total_entries"], 2);

        // Get all
        let get_result = handle_get_glossary(&store, &path, None, None, None)
            .await
            .unwrap();
        let obj = get_result.as_object().unwrap();
        assert_eq!(obj.len(), 1);
        let key = locale_pair_key("en-US", "es-ES");
        assert!(obj.contains_key(&key));
    }

    #[tokio::test]
    async fn get_filtered_by_source_locale() {
        let store = MemoryStore::new();
        let lock = Mutex::new(());
        let path = glossary_path();

        let mut en_es = BTreeMap::new();
        en_es.insert("hello".to_owned(), "hola".to_owned());
        handle_update_glossary(&store, &lock, &path, "en-US", "es-ES", en_es)
            .await
            .unwrap();

        let mut en_de = BTreeMap::new();
        en_de.insert("hello".to_owned(), "hallo".to_owned());
        handle_update_glossary(&store, &lock, &path, "en-US", "de-DE", en_de)
            .await
            .unwrap();

        let mut fr_de = BTreeMap::new();
        fr_de.insert("bonjour".to_owned(), "guten tag".to_owned());
        handle_update_glossary(&store, &lock, &path, "fr-FR", "de-DE", fr_de)
            .await
            .unwrap();

        // Filter source=en-US: should get en→es and en→de
        let result = handle_get_glossary(&store, &path, Some("en-US"), None, None)
            .await
            .unwrap();
        assert_eq!(result.as_object().unwrap().len(), 2);

        // Filter source=fr-FR: should get fr→de only
        let result = handle_get_glossary(&store, &path, Some("fr-FR"), None, None)
            .await
            .unwrap();
        assert_eq!(result.as_object().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn get_filtered_by_target_locale() {
        let store = MemoryStore::new();
        let lock = Mutex::new(());
        let path = glossary_path();

        let mut en_es = BTreeMap::new();
        en_es.insert("hello".to_owned(), "hola".to_owned());
        handle_update_glossary(&store, &lock, &path, "en-US", "es-ES", en_es)
            .await
            .unwrap();

        let mut en_de = BTreeMap::new();
        en_de.insert("hello".to_owned(), "hallo".to_owned());
        handle_update_glossary(&store, &lock, &path, "en-US", "de-DE", en_de)
            .await
            .unwrap();

        // Filter target=de-DE
        let result = handle_get_glossary(&store, &path, None, Some("de-DE"), None)
            .await
            .unwrap();
        assert_eq!(result.as_object().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn get_filtered_by_substring() {
        let store = MemoryStore::new();
        let lock = Mutex::new(());
        let path = glossary_path();

        let mut entries = BTreeMap::new();
        entries.insert("blood sugar".to_owned(), "azúcar en sangre".to_owned());
        entries.insert("glucose".to_owned(), "glucosa".to_owned());
        entries.insert("insulin".to_owned(), "insulina".to_owned());
        handle_update_glossary(&store, &lock, &path, "en-US", "es-ES", entries)
            .await
            .unwrap();

        // Filter by "sugar" — should match "blood sugar" (term) and nothing else
        let result = handle_get_glossary(&store, &path, None, None, Some("sugar"))
            .await
            .unwrap();
        let key = locale_pair_key("en-US", "es-ES");
        let pair = result.get(&key).unwrap().as_object().unwrap();
        assert_eq!(pair.len(), 1);
        assert!(pair.contains_key("blood sugar"));

        // Filter by "glucosa" — matches translation
        let result = handle_get_glossary(&store, &path, None, None, Some("glucosa"))
            .await
            .unwrap();
        let pair = result.get(&key).unwrap().as_object().unwrap();
        assert_eq!(pair.len(), 1);
        assert!(pair.contains_key("glucose"));
    }

    #[tokio::test]
    async fn update_merges_entries() {
        let store = MemoryStore::new();
        let lock = Mutex::new(());
        let path = glossary_path();

        let mut batch1 = BTreeMap::new();
        batch1.insert("hello".to_owned(), "hola".to_owned());
        handle_update_glossary(&store, &lock, &path, "en-US", "es-ES", batch1)
            .await
            .unwrap();

        // Second update adds new + overwrites existing
        let mut batch2 = BTreeMap::new();
        batch2.insert("hello".to_owned(), "¡hola!".to_owned());
        batch2.insert("world".to_owned(), "mundo".to_owned());
        let result = handle_update_glossary(&store, &lock, &path, "en-US", "es-ES", batch2)
            .await
            .unwrap();

        assert_eq!(result["entries_updated"], 2);
        assert_eq!(result["total_entries"], 2); // "hello" overwritten, "world" added

        // Verify the overwrite
        let get_result = handle_get_glossary(&store, &path, Some("en-US"), Some("es-ES"), None)
            .await
            .unwrap();
        let key = locale_pair_key("en-US", "es-ES");
        let pair = get_result.get(&key).unwrap();
        assert_eq!(pair["hello"], "¡hola!");
        assert_eq!(pair["world"], "mundo");
    }

    #[tokio::test]
    async fn get_with_both_locale_filters() {
        let store = MemoryStore::new();
        let lock = Mutex::new(());
        let path = glossary_path();

        let mut en_es = BTreeMap::new();
        en_es.insert("hi".to_owned(), "hola".to_owned());
        handle_update_glossary(&store, &lock, &path, "en-US", "es-ES", en_es)
            .await
            .unwrap();

        let mut en_de = BTreeMap::new();
        en_de.insert("hi".to_owned(), "hallo".to_owned());
        handle_update_glossary(&store, &lock, &path, "en-US", "de-DE", en_de)
            .await
            .unwrap();

        // Exact locale pair filter
        let result = handle_get_glossary(&store, &path, Some("en-US"), Some("es-ES"), None)
            .await
            .unwrap();
        assert_eq!(result.as_object().unwrap().len(), 1);
        let key = locale_pair_key("en-US", "es-ES");
        assert!(result.as_object().unwrap().contains_key(&key));
    }
}
