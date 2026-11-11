//! Product infrastructure shared by the rcad frameworks: where user files
//! live ([`storage`]), how they are picked without an OS dialog
//! ([`picker`]), and the browser's origin-private file system ([`opfs`]).
//!
//! Zero gpui by design: the frameworks render their own overlays; this crate
//! carries the state machines and the file-system layer they both share.

pub mod picker;
pub mod storage;

#[cfg(target_family = "wasm")]
pub mod opfs;

pub use picker::FilePicker;
pub use storage::{
    failed, has_extension, join_key, split_file_name, with_extension, Entry, Location,
    Storage, StorageFuture, StorageProvider,
};
