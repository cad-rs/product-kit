//! Where user files live (ported from workbench-rs's storage design).
//!
//! A [`Location`] names a file by the storage provider that holds it and
//! that provider's key for it: a path in the browser's origin-private file
//! system, an OS path, an object id in a cloud store. Callers remember a
//! `Location`, never an OS path, so opening and saving work the same
//! whichever provider holds the file.
//!
//! Providers are asynchronous (the origin-private file system has only async
//! APIs on the page's thread). Futures carry no `Send` bound: the OPFS
//! provider exists for web builds only, where everything runs on the page's
//! thread.

use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// A storage operation's future.
pub type StorageFuture<T> = Pin<Box<dyn Future<Output = Result<T, String>>>>;

/// A failed storage operation.
pub fn failed<T: 'static>(message: String) -> StorageFuture<T> {
    Box::pin(std::future::ready(Err(message)))
}

/// One item of a folder listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The provider's key for it.
    pub key: String,
    /// What the user sees.
    pub name: String,
    pub is_dir: bool,
}

/// A file in some storage provider.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Location {
    /// The id the provider was registered under.
    pub provider: String,
    /// The provider's key for the file.
    pub key: String,
}

impl Location {
    pub fn new(provider: impl Into<String>, key: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            key: key.into(),
        }
    }

    /// The key's last segment: the file name for providers whose keys are
    /// paths.
    pub fn file_name(&self) -> &str {
        self.key.rsplit(['/', '\\']).next().unwrap_or(&self.key)
    }

    /// The file name without its extension.
    pub fn file_stem(&self) -> &str {
        split_file_name(self.file_name()).0
    }

    /// The file or folder `name` in this folder (keys are `/`-separated
    /// paths; the root folder's key is `""`).
    pub fn child(&self, name: &str) -> Location {
        Location::new(self.provider.as_str(), join_key(&self.key, name))
    }
}

impl fmt::Display for Location {
    /// Local files show as their path, other files as `provider:key`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.provider, self.key)
    }
}

/// The key of `name` in the folder `dir` (the root is `""`).
pub fn join_key(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

/// A file name's stem and extension (without the dot). Like `Path`, a
/// leading dot starts a name, not an extension.
pub fn split_file_name(name: &str) -> (&str, Option<&str>) {
    match name.rfind('.') {
        Some(dot) if dot > 0 => (&name[..dot], Some(&name[dot + 1..])),
        _ => (name, None),
    }
}

/// Whether `name` has one of `extensions` (lowercase, no dot); any name
/// passes an empty list.
pub fn has_extension(name: &str, extensions: &[String]) -> bool {
    let ext = split_file_name(name).1.unwrap_or_default().to_lowercase();
    extensions.is_empty() || extensions.contains(&ext)
}

/// The file name to save `name` under: the first of `extensions` is added
/// when it has none of them.
pub fn with_extension(name: &str, extensions: &[String]) -> String {
    let name = name.trim();
    match extensions.first() {
        Some(ext) if !has_extension(name, extensions) => format!("{name}.{ext}"),
        _ => name.to_string(),
    }
}

/// One place files can live. Errors are user-readable messages.
pub trait StorageProvider: Send + Sync {
    /// What the file picker calls this place.
    fn label(&self) -> &str;
    /// The whole content of the file at `key`.
    fn read(&self, key: &str) -> StorageFuture<Vec<u8>>;
    /// Create the file at `key`, or replace its content, with `bytes`. All
    /// or nothing: a write that fails or is cut off must leave the old
    /// content.
    fn write(&self, key: &str, bytes: Vec<u8>) -> StorageFuture<()>;
    /// The folders and files in the folder `dir` (the root is `""`); the
    /// in-app picker browses with this.
    fn list(&self, dir: &str) -> StorageFuture<Vec<Entry>> {
        let _ = dir;
        failed("Browsing is not supported here".to_string())
    }
    /// Create the folder `key`.
    fn create_dir(&self, key: &str) -> StorageFuture<()> {
        let _ = key;
        failed("Creating folders is not supported here".to_string())
    }
}

/// The product's storage providers, by id.
#[derive(Clone, Default)]
pub struct Storage {
    providers: BTreeMap<String, Arc<dyn StorageProvider>>,
}

impl Storage {
    /// Register `provider` under `id`, replacing any provider already there.
    pub fn register(&mut self, id: impl Into<String>, provider: Arc<dyn StorageProvider>) {
        self.providers.insert(id.into(), provider);
    }

    /// Whether a provider is registered under `id`.
    pub fn contains(&self, id: &str) -> bool {
        self.providers.contains_key(id)
    }

    /// The places the in-app picker browses, as (id, label).
    pub fn in_app_sources(&self) -> Vec<(String, String)> {
        self.providers
            .iter()
            .map(|(id, provider)| (id.clone(), provider.label().to_string()))
            .collect()
    }

    /// List the folder at `location`.
    pub fn list(&self, location: &Location) -> StorageFuture<Vec<Entry>> {
        self.route(location, format!("read folder {location}"), |p, key| {
            p.list(key)
        })
    }

    /// Create the folder at `location`.
    pub fn create_dir(&self, location: &Location) -> StorageFuture<()> {
        self.route(location, format!("create folder {location}"), |p, key| {
            p.create_dir(key)
        })
    }

    /// Read the file at `location`.
    pub fn read(&self, location: &Location) -> StorageFuture<Vec<u8>> {
        self.route(location, format!("read {location}"), |p, key| p.read(key))
    }

    /// Write `bytes` to the file at `location`.
    pub fn write(&self, location: &Location, bytes: Vec<u8>) -> StorageFuture<()> {
        self.route(location, format!("write {location}"), move |p, key| {
            p.write(key, bytes)
        })
    }

    fn route<T: 'static>(
        &self,
        location: &Location,
        missing: String,
        call: impl FnOnce(&Arc<dyn StorageProvider>, &str) -> StorageFuture<T>,
    ) -> StorageFuture<T> {
        match self.providers.get(&location.provider) {
            Some(provider) => call(provider, &location.key),
            None => failed(missing),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_split_like_paths_do() {
        assert_eq!(split_file_name("part.rcad"), ("part", Some("rcad")));
        assert_eq!(split_file_name("part"), ("part", None));
        // A leading dot starts a name, not an extension.
        assert_eq!(split_file_name(".rcad"), (".rcad", None));
        assert_eq!(split_file_name("a.b.rcad"), ("a.b", Some("rcad")));
    }

    #[test]
    fn extension_checks_are_case_insensitive_and_empty_passes_all() {
        assert!(has_extension("part.RCAD", &["rcad".to_string()]));
        assert!(!has_extension("part.txt", &["rcad".to_string()]));
        assert!(has_extension("anything", &[]));
    }

    #[test]
    fn save_names_get_the_first_accepted_extension_once() {
        let exts = ["rcad".to_string()];
        assert_eq!(with_extension(" part ", &exts), "part.rcad");
        assert_eq!(with_extension("part.RCAD", &exts), "part.RCAD");
        assert_eq!(with_extension("part.v2", &exts), "part.v2.rcad");
    }

    #[test]
    fn keys_join_into_slash_paths_with_an_empty_root() {
        assert_eq!(join_key("", "a.rcad"), "a.rcad");
        assert_eq!(join_key("docs", "a.rcad"), "docs/a.rcad");
        let root = Location::new("opfs", "");
        assert_eq!(root.child("docs").key, "docs");
        assert_eq!(root.child("docs").child("a.rcad").key, "docs/a.rcad");
    }
}
