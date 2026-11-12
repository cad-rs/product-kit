//! The shared Rhai script-plugin host.
//!
//! A plugin is **declarative**: a `plugin.toml` manifest plus Rhai scripts whose
//! entry point registers panels, views, commands and toolbar items. A script
//! panel renders a declarative `UiNode` tree — never arbitrary code — and a
//! plugin never contributes domain capability: that stays a compile-time Rust
//! module.
//!
//! ```text
//! plugin.toml  ->  manifest (id / api_version / entry_point / permissions / requires)
//! plugin.rhai  ->  register() calling register_panel / register_view /
//!                   register_command / register_background_command /
//!                   add_toolbar_item / log_info
//! ```
//!
//! The crate is framework-neutral: it owns the manifest, the load/exec engines
//! with the injected Host API, and a loading pipeline that returns the
//! declarations ([`LoadedPlugins`]). The frameworks' adapters commit them —
//! workbench's into its `Registry`, nanobot's shell into its panel host — and
//! supply the license/entitlement [`PluginGate`].
//!
//! Loading is isolated: a rejected plugin lands in
//! [`LoadedPlugins::plugins_rejected`] and does not interrupt the assembly of
//! the others.

mod host;
mod loader;
mod manifest;

pub use host::{
    call_sync_callback, run_background_callback, CommandArgs, CommandResult, Collected,
    CollectedCommand, ProgressSink, RhaiProgram, TaskFailure,
};
pub use loader::{
    discover, load_all, load_all_with, read_manifest, run_entry, CommandRegistration,
    LoadedPlugins, PanelRegistration, PluginGate, ToolbarItemRegistration, ViewRegistration,
};
pub use manifest::{
    parse, split_entry_point, validate, Compatibility, Permissions, PluginInfo, PluginManifest,
    HOST_API_VERSION,
};
