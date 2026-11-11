//! Product infrastructure shared by the rcad frameworks: where user files
//! live ([`storage`]), how they are picked without an OS dialog
//! ([`picker`]), the browser's origin-private file system ([`opfs`]), and
//! whole-file replacement that never risks the previous content
//! ([`atomic`]).
//!
//! Zero gpui by design: the frameworks render their own overlays; this crate
//! carries the state machines and the file-system layer they both share.

pub mod entitlement_refresh;
pub mod pack;
pub mod picker;
pub mod settings;
pub mod storage;

#[cfg(not(target_family = "wasm"))]
pub mod atomic;

#[cfg(target_family = "wasm")]
pub mod opfs;

#[cfg(not(target_family = "wasm"))]
pub use atomic::write as atomic_write;
pub use entitlement_refresh::{EntitlementAnswer, EntitlementSource, RefreshSchedule, SessionGrace};

pub use pack::pack_tree;
pub use picker::FilePicker;
pub use settings::{Settings, SettingsStore};
pub use storage::{
    failed, has_extension, join_key, split_file_name, with_extension, Entry, Location,
    Storage, StorageFuture, StorageProvider,
};
