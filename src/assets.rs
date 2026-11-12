//! A GUI-independent asset loading contract (SVG icons and other static
//! assets).
//!
//! Workbenches/plugins declare asset paths on UI elements (such as
//! `"icons/save.svg"`), the product supplies an [`AssetSource`] through its
//! application builder, and the frontend fetches the bytes by path and renders
//! them. A path is only a stable key and need not correspond to a real file —
//! the most common form is a static table embedded in the binary with
//! `include_bytes!`.
//!
//! The contract is deliberately gpui-free: each framework bridges it to gpui's
//! own `AssetSource` (which adds a `Result` and a path-scoped `list`).

use std::borrow::Cow;
use std::sync::Arc;

/// A static asset source: loads asset bytes by path.
///
/// Returning `None` means this source does not have the asset (the caller keeps
/// looking further down or falls back to showing no icon); a load failure
/// should also return `None` rather than break the UI.
pub trait AssetSource: Send + Sync + 'static {
    fn load(&self, path: &str) -> Option<Cow<'static, [u8]>>;

    /// Enumerate every asset path this source holds (used by the GUI frontend
    /// to register paths with gpui's `AssetRegistry`; the registry only
    /// resolves registered paths). Empty by default — a purely on-demand
    /// source cannot enumerate itself and should switch to an enumerable form
    /// (such as [`StaticAssets`]).
    fn paths(&self) -> Vec<String> {
        Vec::new()
    }
}

/// The empty source: a convenience for "no assets" (gpui's own `()` impl
/// mirrors this).
impl AssetSource for () {
    fn load(&self, _path: &str) -> Option<Cow<'static, [u8]>> {
        None
    }
}

/// A chained asset source: looks up in order, and the first hit wins (later
/// sources are fallbacks).
///
/// Typical use: `ChainedAssets::new(vec![product_source, platform_standard_source])` — the
/// product can override the platform's standard icons with the same paths, or just add icons of
/// its own.
pub struct ChainedAssets {
    sources: Vec<Arc<dyn AssetSource>>,
}

impl ChainedAssets {
    pub fn new(sources: Vec<Arc<dyn AssetSource>>) -> Self {
        Self { sources }
    }
}

impl AssetSource for ChainedAssets {
    fn load(&self, path: &str) -> Option<Cow<'static, [u8]>> {
        self.sources.iter().find_map(|s| s.load(path))
    }

    fn paths(&self) -> Vec<String> {
        self.sources.iter().flat_map(|s| s.paths()).collect()
    }
}

/// Convenience constructor: build an asset source from a static `(path, bytes)` table (use with
/// `include_bytes!`).
#[derive(Clone, Copy)]
pub struct StaticAssets {
    entries: &'static [(&'static str, &'static [u8])],
}

impl StaticAssets {
    pub const fn new(entries: &'static [(&'static str, &'static [u8])]) -> Self {
        Self { entries }
    }
}

impl AssetSource for StaticAssets {
    fn load(&self, path: &str) -> Option<Cow<'static, [u8]>> {
        self.entries
            .iter()
            .find(|(p, _)| *p == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes))
    }

    fn paths(&self) -> Vec<String> {
        self.entries.iter().map(|(p, _)| (*p).to_string()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static TABLE: &[(&str, &[u8])] = &[("icons/a.svg", b"<svg/>"), ("icons/b.svg", b"<svg/>")];

    #[test]
    fn a_static_table_loads_and_enumerates() {
        let assets = StaticAssets::new(TABLE);
        assert_eq!(assets.load("icons/a.svg").as_deref(), Some(b"<svg/>".as_slice()));
        assert!(assets.load("icons/missing.svg").is_none());
        assert_eq!(assets.paths(), vec!["icons/a.svg".to_string(), "icons/b.svg".to_string()]);
    }

    #[test]
    fn the_empty_source_has_nothing() {
        let assets = ();
        assert!(assets.load("icons/a.svg").is_none());
        assert!(assets.paths().is_empty());
    }

    #[test]
    fn the_first_hit_wins_and_paths_chain() {
        struct Second;
        impl AssetSource for Second {
            fn load(&self, path: &str) -> Option<Cow<'static, [u8]>> {
                (path == "icons/b.svg").then(|| Cow::Borrowed(&b"second"[..]))
            }
            fn paths(&self) -> Vec<String> {
                vec!["icons/b.svg".to_string()]
            }
        }

        let chained = ChainedAssets::new(vec![
            Arc::new(StaticAssets::new(TABLE)),
            Arc::new(Second),
        ]);
        assert_eq!(chained.load("icons/a.svg").as_deref(), Some(b"<svg/>".as_slice()));
        // The first source that has it wins.
        assert_eq!(chained.load("icons/b.svg").as_deref(), Some(b"<svg/>".as_slice()));
        // Paths concatenate (a duplicate across sources is left to the frontend's
        // registry, which deduplicates).
        assert_eq!(chained.paths().len(), 3);
    }
}
