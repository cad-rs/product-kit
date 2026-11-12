//! Project packages: a whole storage tree packed into one zip archive.
//!
//! The archive keeps the root directory's own name at the top level, so an
//! unpack lands a self-contained project directory (a package of `ocad-web`
//! holds `ocad-web/…` and nothing else).

use std::io::Write as _;

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::storage::{Location, Storage};

/// Packs the storage tree under `provider`/`root` into zip bytes. The walk
/// starts at the provider's own root so keys come back absolute; entries
/// outside `root` are skipped, and `root`'s name heads every stored path.
pub async fn pack_tree(storage: &Storage, provider: &str, root: &str) -> Result<Vec<u8>, String> {
    let options =
        SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut zip = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let mut queue = vec![Location::new(provider, "")];
    while let Some(dir) = queue.pop() {
        let listing = storage.list(&dir).await?;
        for entry in listing {
            let Some(path) = entry.key.strip_prefix(root) else {
                continue;
            };
            if entry.is_dir {
                // The root itself (path empty) heads the queue; so do the
                // folders below it.
                queue.push(Location::new(provider, &entry.key));
                continue;
            }
            if path.is_empty() {
                continue;
            }
            let path = path.strip_prefix('/').unwrap_or(path);
            let bytes = storage
                .read(&Location::new(provider, &entry.key))
                .await?;
            zip.start_file(format!("{root}/{path}"), options)
                .map_err(|error| error.to_string())?;
            zip.write_all(&bytes).map_err(|error| error.to_string())?;
        }
    }
    let cursor = zip.finish().map_err(|error| error.to_string())?;
    Ok(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StorageProvider;

    /// An in-memory provider over a fixed tree.
    struct Memory {
        tree: std::collections::BTreeMap<String, Vec<u8>>,
    }

    impl StorageProvider for Memory {
        fn label(&self) -> &str {
            "memory"
        }
        fn read(&self, key: &str) -> crate::storage::StorageFuture<Vec<u8>> {
            let key = key.to_string();
            let tree = self.tree.clone();
            Box::pin(async move {
                tree.get(&key)
                    .cloned()
                    .ok_or_else(|| format!("{key} not found"))
            })
        }
        fn write(&self, _key: &str, _bytes: Vec<u8>) -> crate::storage::StorageFuture<()> {
            crate::failed("read-only".to_string())
        }
        fn list(&self, dir: &str) -> crate::storage::StorageFuture<Vec<crate::storage::Entry>> {
            let dir = dir.to_string();
            let tree = self.tree.clone();
            Box::pin(async move {
                let mut entries = Vec::new();
                let mut dirs = std::collections::BTreeSet::new();
                let prefix = if dir.is_empty() {
                    String::new()
                } else {
                    format!("{dir}/")
                };
                for key in tree.keys() {
                    let Some(rest) = key.strip_prefix(&prefix) else {
                        continue;
                    };
                    let rest = rest.trim_end_matches('/');
                    if rest.is_empty() {
                        continue;
                    }
                    if let Some((first, _tail)) = rest.split_once('/') {
                        dirs.insert(format!("{prefix}{first}"));
                    } else {
                        entries.push(crate::storage::Entry {
                            key: key.clone(),
                            name: rest.to_string(),
                            is_dir: false,
                        });
                    }
                }
                for key in dirs {
                    let name = key.rsplit('/').next().unwrap_or(&key).to_string();
                    entries.push(crate::storage::Entry {
                        key: key.clone(),
                        name,
                        is_dir: true,
                    });
                }
                entries.sort_by_cached_key(|entry| (!entry.is_dir, entry.name.to_lowercase()));
                Ok(entries)
            })
        }
    }

    fn tree() -> std::collections::BTreeMap<String, Vec<u8>> {
        [
            ("ocad-web/project.ocad.toml".to_string(), b"project".to_vec()),
            (
                "ocad-web/_ocad/conversations/s-board/events.jsonl".to_string(),
                b"{}\n".to_vec(),
            ),
            (
                "other-project/notes.txt".to_string(),
                b"untouched".to_vec(),
            ),
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn a_pack_holds_only_the_rooted_tree_under_its_own_name() {
        let storage = Storage::default();
        let provider = std::sync::Arc::new(Memory { tree: tree() });
        let mut storage = storage;
        storage.register("memory", provider);
        let bytes = futures_executor::block_on(pack_tree(&storage, "memory", "ocad-web"))
            .expect("the pack builds");
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip reads");
        let mut names = Vec::new();
        for index in 0..archive.len() {
            names.push(archive.by_index(index).expect("entry").name().to_string());
        }
        names.sort();
        assert_eq!(
            names,
            [
                "ocad-web/_ocad/conversations/s-board/events.jsonl",
                "ocad-web/project.ocad.toml",
            ]
        );
    }
}
