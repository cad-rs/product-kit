//! User settings: flat dotted-key JSON persistence.
//!
//! A `BTreeMap<String, Value>` behind dotted key strings (`ui.theme`,
//! `plugin.<id>.enabled`, `autosave.interval_secs`). The dot is part of the
//! key, not a nesting operator; there is no schema and no defaults table.
//! Writes hit the store immediately, reads go through the in-memory map.
//!
//! Persistence goes through [`SettingsStore`], so each framework plugs in its
//! own backend: workbench's keyed app-state store, nanobot's settings file.
//! Loading is never fatal — a missing document is empty settings, a corrupt
//! one resets with a message for the caller to surface. The document is only
//! ever written by an explicit change ([`Settings::set`] /
//! [`Settings::remove`]): an assembly that changed nothing writes nothing.
//!
//! Shared by the workbench framework and the nanobot harness; it replaced
//! their two same-shaped copies.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;

/// Where the settings document lives. One document per store; the framework
/// decides what that means (a key in its app-state store, a file path).
pub trait SettingsStore: Send + Sync {
    /// The stored document, if there is one.
    fn read(&self) -> Option<Vec<u8>>;
    /// Create or replace the document. Errors are user-readable.
    fn write(&self, bytes: &[u8]) -> Result<(), String>;
    /// Where the document lives, for diagnostics (a path, a key, ...).
    fn describe(&self) -> Option<String> {
        None
    }
}

/// The settings store. Owned by whoever runs the UI; writes go through the
/// store immediately so a crash after a change loses nothing.
#[derive(Default)]
pub struct Settings {
    values: BTreeMap<String, Value>,
    store: Option<Arc<dyn SettingsStore>>,
}

impl Settings {
    /// Load the document from `store`; empty when missing, reset with a
    /// message when it fails to parse.
    pub fn load(store: Arc<dyn SettingsStore>) -> (Self, Option<String>) {
        let parsed = store.read().map(|bytes| serde_json::from_slice(&bytes));
        let (values, error) = match parsed {
            None => (BTreeMap::new(), None),
            Some(Ok(values)) => (values, None),
            Some(Err(e)) => (
                BTreeMap::new(),
                Some(format!("Failed to parse the settings file; reset: {e}")),
            ),
        };
        let settings = Self {
            values,
            store: Some(store),
        };
        (settings, error)
    }

    /// In-memory settings that are not persisted (for tests).
    pub fn in_memory() -> Self {
        Self::default()
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    pub fn get_str(&self, key: &str, default: &str) -> String {
        self.get(key)
            .and_then(Value::as_str)
            .unwrap_or(default)
            .to_string()
    }

    pub fn get_f64(&self, key: &str, default: f64) -> f64 {
        self.get(key).and_then(Value::as_f64).unwrap_or(default)
    }

    pub fn get_bool(&self, key: &str, default: bool) -> bool {
        self.get(key).and_then(Value::as_bool).unwrap_or(default)
    }

    /// Write and persist immediately (memory only when there is no store).
    /// Returns the error message, if any.
    pub fn set(&mut self, key: &str, value: Value) -> Result<(), String> {
        self.values.insert(key.to_string(), value);
        self.save()
    }

    /// Seed a default into memory only (no store write): used at build time so
    /// a registered default is readable before the user ever changes it,
    /// without polluting the stored document with untouched defaults.
    pub fn seed(&mut self, key: &str, value: Value) {
        self.values.entry(key.to_string()).or_insert(value);
    }

    pub fn remove(&mut self, key: &str) -> Result<(), String> {
        self.values.remove(key);
        self.save()
    }

    /// Every key, in key order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.values.keys().map(|k| k.as_str())
    }

    /// Every key and value, in key order (the diagnostics export).
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.values.iter().map(|(key, value)| (key.as_str(), value))
    }

    /// Where the document lives, when the store can say.
    pub fn location(&self) -> Option<String> {
        self.store.as_ref().and_then(|store| store.describe())
    }

    /// Save everything (a no-op for in-memory settings).
    pub fn save(&self) -> Result<(), String> {
        let Some(store) = &self.store else {
            return Ok(());
        };
        let json = serde_json::to_string_pretty(&self.values).map_err(|e| e.to_string())?;
        store.write(json.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A store that keeps the document in memory and records writes.
    #[derive(Default)]
    struct MemoryStore {
        bytes: Mutex<Option<Vec<u8>>>,
        writes: Mutex<usize>,
    }

    impl SettingsStore for MemoryStore {
        fn read(&self) -> Option<Vec<u8>> {
            self.bytes.lock().unwrap().clone()
        }

        fn write(&self, bytes: &[u8]) -> Result<(), String> {
            *self.bytes.lock().unwrap() = Some(bytes.to_vec());
            *self.writes.lock().unwrap() += 1;
            Ok(())
        }

        fn describe(&self) -> Option<String> {
            Some("memory".to_string())
        }
    }

    #[test]
    fn a_missing_document_loads_empty_without_a_message() {
        let store = Arc::new(MemoryStore::default());
        let (settings, error) = Settings::load(store);
        assert!(error.is_none(), "a missing document is not an event");
        assert!(settings.get("ui.theme").is_none());
    }

    #[test]
    fn a_corrupt_document_resets_with_a_message() {
        struct Corrupt;
        impl SettingsStore for Corrupt {
            fn read(&self) -> Option<Vec<u8>> {
                Some(b"{ not json".to_vec())
            }
            fn write(&self, _bytes: &[u8]) -> Result<(), String> {
                Ok(())
            }
        }
        let (settings, error) = Settings::load(Arc::new(Corrupt));
        assert!(error.is_some(), "the reset is reported, not silent");
        assert!(settings.get("ui.theme").is_none());
    }

    #[test]
    fn set_writes_through_and_round_trips() {
        let store = Arc::new(MemoryStore::default());
        let mut settings = Settings::load(store.clone()).0;
        settings.set("ui.theme", Value::String("light".into())).expect("set");
        assert_eq!(*store.writes.lock().unwrap(), 1, "set writes immediately");

        let reloaded = Settings::load(store).0;
        assert_eq!(reloaded.get_str("ui.theme", "dark"), "light");
    }

    #[test]
    fn remove_writes_through() {
        let store = Arc::new(MemoryStore::default());
        let mut settings = Settings::load(store.clone()).0;
        settings.set("k", Value::Bool(true)).unwrap();
        settings.remove("k").unwrap();
        assert_eq!(*store.writes.lock().unwrap(), 2);
        let reloaded = Settings::load(store).0;
        assert!(reloaded.get("k").is_none());
    }

    #[test]
    fn get_str_falls_back_to_its_default() {
        let mut settings = Settings::in_memory();
        settings.set("ui.theme", Value::String("light".into())).unwrap();
        assert_eq!(settings.get_str("ui.theme", "dark"), "light");
        assert_eq!(settings.get_str("absent", "dark"), "dark");
    }

    #[test]
    fn in_memory_settings_accept_changes_without_a_store() {
        let mut settings = Settings::in_memory();
        settings.set("k", Value::Bool(true)).expect("memory set");
        assert_eq!(settings.get("k"), Some(&Value::Bool(true)));
        assert!(settings.location().is_none());
    }

    #[test]
    fn seed_does_not_write_and_does_not_override() {
        let store = Arc::new(MemoryStore::default());
        let mut settings = Settings::load(store.clone()).0;
        settings.seed("ui.theme", Value::String("dark".into()));
        assert_eq!(*store.writes.lock().unwrap(), 0, "seed stays in memory");
        assert_eq!(settings.get_str("ui.theme", "light"), "dark");
        settings.set("ui.theme", Value::String("light".into())).unwrap();
        settings.seed("ui.theme", Value::String("dark".into()));
        assert_eq!(settings.get_str("ui.theme", "dark"), "light", "seed keeps the stored value");
    }

    #[test]
    fn location_comes_from_the_store() {
        let store = Arc::new(MemoryStore::default());
        let settings = Settings::load(store).0;
        assert_eq!(settings.location().as_deref(), Some("memory"));
    }
}
