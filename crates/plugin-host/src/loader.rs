//! Plugin discovery and loading.
//!
//! A plugin directory holds `plugin.toml` and its Rhai scripts. Loading is
//! **isolated**: a bad manifest, a missing script, a compile error, a refused
//! gate or a duplicate id lands in [`LoadedPlugins::plugins_rejected`] (or a
//! note) and never interrupts the assembly of the rest.
//!
//! The pipeline is framework-neutral: it returns the declarations and lets the
//! host commit them. The frameworks' adapters (a Registry commit, a shell
//! hand-off) sit on top.

use std::path::{Path, PathBuf};

use crate::host::{build_load_engine, Collected, RhaiProgram, SharedCollected};
use crate::manifest::{parse, split_entry_point, validate, PluginManifest};

/// A dock panel a plugin declared.
#[derive(Debug, Clone)]
pub struct PanelRegistration {
    pub plugin: String,
    pub id: String,
    pub title: String,
    pub script: PathBuf,
    pub area: String,
}

/// A central view a plugin declared.
#[derive(Debug, Clone)]
pub struct ViewRegistration {
    pub plugin: String,
    pub id: String,
    pub title: String,
    pub script: PathBuf,
}

/// A command a plugin declared. `source` is the entry script it is defined in.
#[derive(Debug, Clone)]
pub struct CommandRegistration {
    pub plugin: String,
    pub id: String,
    pub title: String,
    pub callback: String,
    pub background: bool,
    pub source: RhaiProgram,
}

/// A toolbar item a plugin declared.
#[derive(Debug, Clone)]
pub struct ToolbarItemRegistration {
    pub plugin: String,
    pub group: String,
    pub command: String,
    pub label: Option<String>,
}

/// Everything the loaded plugins contribute, plus what was rejected and why.
#[derive(Debug, Default)]
pub struct LoadedPlugins {
    pub plugins_loaded: Vec<String>,
    pub panels: Vec<PanelRegistration>,
    pub views: Vec<ViewRegistration>,
    pub commands: Vec<CommandRegistration>,
    pub toolbar: Vec<ToolbarItemRegistration>,
    pub notes: Vec<String>,
    /// `(plugin directory, reason)` for every plugin that did not load.
    pub plugins_rejected: Vec<(String, String)>,
}

/// Every plugin directory under `dirs`: immediate subdirectories that hold a
/// `plugin.toml`, sorted for deterministic load order.
pub fn discover(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join("plugin.toml").is_file() {
                found.push(path);
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// Reads and validates a plugin directory's manifest, running nothing.
pub fn read_manifest(dir: &Path) -> Result<PluginManifest, String> {
    let manifest_path = dir.join("plugin.toml");
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("cannot read {}: {e}", manifest_path.display()))?;
    let manifest = parse(&text).map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    validate(&manifest).map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    Ok(manifest)
}

/// Loads one plugin directory. `Err(reason)` means the plugin is rejected.
pub fn load_one(dir: &Path) -> Result<(PluginManifest, RhaiProgram, Collected), String> {
    let manifest = read_manifest(dir)?;
    let (program, collected) = run_entry(dir, &manifest)?;
    Ok((manifest, program, collected))
}

/// Compiles the plugin's entry script and runs its entry point, collecting
/// what it declares.
///
/// Public so a host can run its gate between [`read_manifest`] and the entry
/// point (a refused plugin's script is never compiled).
pub fn run_entry(dir: &Path, manifest: &PluginManifest) -> Result<(RhaiProgram, Collected), String> {
    let (entry_file, entry_fn) = split_entry_point(&manifest.plugin.entry_point);
    let entry_path = dir.join(&entry_file);
    let source = std::fs::read_to_string(&entry_path)
        .map_err(|e| format!("cannot read {}: {e}", entry_path.display()))?;

    let collected: SharedCollected = std::sync::Arc::new(std::sync::Mutex::new(Collected::default()));
    let program: RhaiProgram = std::sync::Arc::new(source);
    {
        let engine = build_load_engine(dir.to_path_buf(), collected.clone());
        let ast = engine
            .compile(program.as_str())
            .map_err(|e| format!("{}: {e}", entry_path.display()))?;
        let mut scope = rhai::Scope::new();
        engine
            .call_fn::<()>(&mut scope, &ast, &entry_fn, ())
            .map_err(|e| format!("{}: entry point `{entry_fn}` failed: {e}", entry_path.display()))?;
        // The engine's injected globals hold clones of the collection bucket, so
        // it must be dropped before the bucket can be unwrapped.
    }

    let collected = std::sync::Arc::try_unwrap(collected)
        .map_err(|_| "plugin engine still alive".to_string())?
        .into_inner()
        .map_err(|_| "collected bucket poisoned".to_string())?;
    Ok((program, collected))
}

/// The gate a plugin must pass before it loads: given the validated manifest
/// and the plugin directory, `Err(reason)` rejects it.
///
/// A plugin that ships no license is open and passes any gate that allows it;
/// a plugin that ships one must verify. The host supplies the gate — typically
/// a license check over the vendor's verifying keys — so the plugin host stays
/// free of keys and crypto.
pub type PluginGate<'a> = &'a dyn Fn(&PluginManifest, &Path) -> Result<(), String>;

/// A [`PluginGate`] over the product's entitlement answer: a plugin loads
/// only if every capability its manifest `requires` is allowed.
///
/// `check` is the product's allow/deny question — typically
/// `|capability| harness.check_entitlement(capability)` mapped to `Err(reason)`
/// on a denial — so the plugin host stays free of accounts. Combine it with a
/// license gate by calling both from one closure.
pub fn requires_gate<'a>(
    check: &'a dyn Fn(&str) -> Result<(), String>,
) -> impl Fn(&PluginManifest, &Path) -> Result<(), String> + 'a {
    move |manifest, _dir| {
        manifest.plugin.requires.iter().try_for_each(|capability| {
            check(capability)
                .map_err(|reason| format!("requires `{capability}`: {reason}"))
        })
    }
}

/// Loads every plugin under `dirs` without license checking (an open plugin
/// set). See [`load_all_with`].
pub fn load_all(dirs: &[PathBuf]) -> LoadedPlugins {
    load_all_with(dirs, &|_, _| Ok(()))
}

/// Loads every plugin under `dirs`, draining each entry point's declarations
/// into one result. Duplicate ids are rejected individually so one plugin
/// cannot silently shadow another's panel or command.
///
/// `gate` runs after the manifest validates and before the entry point
/// compiles; a plugin it rejects lands in `plugins_rejected` with the reason,
/// next to the other load failures.
pub fn load_all_with(dirs: &[PathBuf], gate: PluginGate<'_>) -> LoadedPlugins {
    let mut loaded = LoadedPlugins::default();
    let mut plugin_ids = Vec::new();
    let mut panel_ids = Vec::new();
    let mut view_ids = Vec::new();
    let mut command_ids = Vec::new();

    for dir in discover(dirs) {
        let name = dir.display().to_string();
        let manifest = match read_manifest(&dir) {
            Ok(manifest) => manifest,
            Err(reason) => {
                loaded.plugins_rejected.push((name, reason));
                continue;
            }
        };
        // Before anything of the plugin runs: a refused plugin's script is
        // never compiled.
        if let Err(reason) = gate(&manifest, &dir) {
            loaded.plugins_rejected.push((name, format!("license: {reason}")));
            continue;
        }
        let (program, collected) = match run_entry(&dir, &manifest) {
            Ok(loaded) => loaded,
            Err(reason) => {
                loaded.plugins_rejected.push((name, reason));
                continue;
            }
        };
        if plugin_ids.contains(&manifest.plugin.id) {
            loaded.plugins_rejected.push((
                name,
                format!("duplicate plugin id `{}`", manifest.plugin.id),
            ));
            continue;
        }
        plugin_ids.push(manifest.plugin.id.clone());

        for command in collected.commands {
            if command_ids.contains(&command.id) {
                loaded.notes.push(format!(
                    "command `{}` from `{}` duplicates an existing id; ignored",
                    command.id, manifest.plugin.id
                ));
                continue;
            }
            command_ids.push(command.id.clone());
            loaded.commands.push(CommandRegistration {
                plugin: manifest.plugin.id.clone(),
                id: command.id,
                title: command.title,
                callback: command.callback,
                background: command.background,
                source: program.clone(),
            });
        }
        for (id, command, label) in collected.toolbar {
            loaded.toolbar.push(ToolbarItemRegistration {
                plugin: manifest.plugin.id.clone(),
                group: id,
                command,
                label,
            });
        }
        for (id, title, script, area) in collected.panels {
            if panel_ids.contains(&id) {
                loaded.notes.push(format!(
                    "panel `{id}` from `{}` duplicates an existing id; ignored",
                    manifest.plugin.id
                ));
                continue;
            }
            panel_ids.push(id.clone());
            loaded.panels.push(PanelRegistration {
                plugin: manifest.plugin.id.clone(),
                id,
                title,
                script,
                area,
            });
        }
        for (id, title, script) in collected.views {
            if view_ids.contains(&id) {
                loaded.notes.push(format!(
                    "view `{id}` from `{}` duplicates an existing id; ignored",
                    manifest.plugin.id
                ));
                continue;
            }
            view_ids.push(id.clone());
            loaded.views.push(ViewRegistration {
                plugin: manifest.plugin.id.clone(),
                id,
                title,
                script,
            });
        }
        loaded.notes.extend(collected.notes);
        loaded.plugins_loaded.push(manifest.plugin.id);
    }

    loaded
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{call_sync_callback, run_background_callback, CommandArgs, ProgressSink, TaskFailure};
    use std::path::PathBuf;
    use std::sync::Arc;

    /// A parent directory holding one plugin per call, the shape
    /// [`load_all`] discovers from.
    fn plugin_parent(test: &str) -> PathBuf {
        let parent = std::env::temp_dir().join(format!(
            "plugin-host-test-{test}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&parent);
        std::fs::create_dir_all(&parent).expect("create parent dir");
        parent
    }

    /// Writes one plugin into `parent/<name>` and returns its directory.
    fn write_plugin(
        parent: &Path,
        name: &str,
        manifest: &str,
        entry: &str,
        extra: &[(&str, &str)],
    ) -> PathBuf {
        let dir = parent.join(name);
        std::fs::create_dir_all(&dir).expect("create plugin dir");
        std::fs::write(dir.join("plugin.toml"), manifest).expect("write manifest");
        std::fs::write(dir.join("plugin.rhai"), entry).expect("write entry");
        for (file, content) in extra {
            std::fs::write(dir.join(file), content).expect("write extra script");
        }
        dir
    }

    const MANIFEST: &str = r#"
[plugin]
id = "com.example.demo"
name = "Demo"
version = "0.1.0"
api_version = "1"
entry_point = "plugin.rhai"

[compatibility]
products = ["*"]

[permissions]
document_read = false
network = false
"#;

    const ENTRY: &str = r#"
fn register() {
    register_panel("acme.chart", "温度曲线", "chart.rhai", "right");
    register_view("acme.thermal", "热力学视图", "thermal.rhai");
    register_command("acme.analyze", "分析", "analyze");
    register_background_command("acme.job", "后台任务", "job");
    add_toolbar_item("acme", "acme.analyze", "分析");
    log_info("demo loaded");
}
fn analyze(args) { `分析 ${args.value}` }
fn job(task, args) {
    task.report_progress(0.5);
    task.log("working");
    task.check_cancelled()
}
"#;

    #[test]
    fn a_plugin_declares_its_panels_views_and_commands() {
        let parent = plugin_parent("ok");
        let dir = write_plugin(
            &parent,
            "demo",
            MANIFEST,
            ENTRY,
            &[("chart.rhai", "fn main() { text(\"chart\") }")],
        );
        let loaded = load_all(std::slice::from_ref(&parent));
        assert!(
            loaded.plugins_rejected.is_empty(),
            "{:?}",
            loaded.plugins_rejected
        );
        assert_eq!(loaded.plugins_loaded, vec!["com.example.demo".to_string()]);
        assert_eq!(loaded.panels.len(), 1);
        assert_eq!(loaded.panels[0].id, "acme.chart");
        assert_eq!(loaded.panels[0].area, "right");
        assert_eq!(loaded.panels[0].script, dir.join("chart.rhai"));
        assert_eq!(loaded.views.len(), 1);
        assert_eq!(loaded.views[0].id, "acme.thermal");
        assert_eq!(loaded.commands.len(), 2);
        assert_eq!(loaded.commands[0].id, "acme.analyze");
        assert!(!loaded.commands[0].background);
        assert!(loaded.commands[1].background);
        assert_eq!(loaded.toolbar.len(), 1);
        assert_eq!(loaded.toolbar[0].label.as_deref(), Some("分析"));
        assert!(loaded.notes.iter().any(|n| n.contains("demo loaded")));
    }

    #[test]
    fn a_rejected_plugin_does_not_stop_the_others() {
        let parent = plugin_parent("mixed");
        let bad = write_plugin(&parent, "bad", MANIFEST, ENTRY, &[]);
        std::fs::write(bad.join("plugin.rhai"), "fn register() { syntax error")
            .expect("break it");
        write_plugin(&parent, "good", MANIFEST, ENTRY, &[]);
        let loaded = load_all(&[parent]);
        assert_eq!(loaded.plugins_rejected.len(), 1);
        assert!(loaded.plugins_rejected[0].0.contains("bad"));
        assert_eq!(loaded.plugins_loaded.len(), 1);
        assert_eq!(loaded.commands.len(), 2);
    }

    #[test]
    fn a_duplicate_plugin_id_is_rejected() {
        let parent = plugin_parent("dup");
        write_plugin(&parent, "dup1", MANIFEST, ENTRY, &[]);
        write_plugin(&parent, "dup2", MANIFEST, ENTRY, &[]);
        let loaded = load_all(&[parent]);
        assert_eq!(loaded.plugins_loaded.len(), 1);
        assert_eq!(loaded.plugins_rejected.len(), 1);
        assert!(loaded.plugins_rejected[0].1.contains("duplicate plugin id"));
    }

    #[test]
    fn an_unknown_dock_area_is_a_note_not_a_rejection() {
        let parent = plugin_parent("area");
        write_plugin(
            &parent,
            "area",
            MANIFEST,
            r#"fn register() { register_panel("acme.x", "X", "chart.rhai", "everywhere"); }"#,
            &[],
        );
        let loaded = load_all(&[parent]);
        assert!(loaded.plugins_rejected.is_empty());
        assert!(loaded.panels.is_empty());
        assert!(loaded.notes.iter().any(|n| n.contains("unknown dock area")));
    }

    #[test]
    fn a_sync_callback_receives_its_arguments() {
        let parent = plugin_parent("cb");
        let loaded = {
            write_plugin(&parent, "cb", MANIFEST, ENTRY, &[]);
            load_all(&[parent])
        };
        let analyze = loaded
            .commands
            .iter()
            .find(|c| c.id == "acme.analyze")
            .expect("analyze registered");
        let args = CommandArgs {
            values: std::collections::BTreeMap::new(),
            value: Some("齿轮箱".to_string()),
        };
        let result = call_sync_callback(&analyze.source, &analyze.callback, &args);
        match result {
            crate::host::CommandResult::Done { note } => assert_eq!(note.as_deref(), Some("分析 齿轮箱")),
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn a_background_callback_reports_progress_and_logs() {
        let parent = plugin_parent("bg");
        write_plugin(&parent, "bg", MANIFEST, ENTRY, &[]);
        let loaded = load_all(&[parent]);
        let job = loaded
            .commands
            .iter()
            .find(|c| c.id == "acme.job")
            .expect("job registered");

        #[derive(Default)]
        struct Recorder {
            fractions: std::sync::Mutex<Vec<Option<f32>>>,
            logs: std::sync::Mutex<Vec<String>>,
            cancelled: std::sync::atomic::AtomicBool,
        }
        impl ProgressSink for Recorder {
            fn report(&self, fraction: Option<f32>, _stage: Option<String>) {
                self.fractions.lock().unwrap().push(fraction);
            }
            fn log(&self, message: String) {
                self.logs.lock().unwrap().push(message);
            }
            fn is_cancelled(&self) -> bool {
                self.cancelled.load(std::sync::atomic::Ordering::SeqCst)
            }
        }

        let recorder = Arc::new(Recorder::default());
        let result = run_background_callback(&job.source, &job.callback, recorder.clone());
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(
            *recorder.fractions.lock().unwrap(),
            vec![Some(0.5)],
            "the script reported half progress"
        );
        assert_eq!(*recorder.logs.lock().unwrap(), vec!["working".to_string()]);
    }

    #[test]
    fn a_cancelled_background_callback_reports_cancellation() {
        let parent = plugin_parent("cancel");
        write_plugin(
            &parent,
            "cancel",
            MANIFEST,
            r#"fn register() { register_background_command("acme.c", "C", "c"); }
fn c(task, args) { task.check_cancelled() }"#,
            &[],
        );
        let loaded = load_all(&[parent]);
        let command = loaded.commands.first().expect("c registered");

        struct Cancelled;
        impl ProgressSink for Cancelled {
            fn report(&self, _fraction: Option<f32>, _stage: Option<String>) {}
            fn log(&self, _message: String) {}
            fn is_cancelled(&self) -> bool {
                true
            }
        }

        match run_background_callback(&command.source, &command.callback, Arc::new(Cancelled)) {
            Err(TaskFailure::Cancelled) => {}
            other => panic!("expected cancellation, got {other:?}"),
        }
    }

    #[test]
    fn a_plugin_loads_only_with_the_capabilities_it_requires() {
        let gated = MANIFEST.replace(
            "entry_point = \"plugin.rhai\"",
            "entry_point = \"plugin.rhai\"\nrequires = [\"export.gds\", \"pro\"]",
        );
        let owns = |owned: &'static [&'static str]| {
            move |capability: &str| {
                if owned.contains(&capability) {
                    Ok(())
                } else {
                    Err("not in your plan".to_string())
                }
            }
        };

        // Refused: the reason names the missing capability, and the script
        // never runs — a broken one is not even compiled.
        let parent = plugin_parent("requires-refused");
        write_plugin(&parent, "gated", &gated, "fn register() { syntax error", &[]);
        let check = owns(&["export.gds"]);
        let loaded = load_all_with(std::slice::from_ref(&parent), &requires_gate(&check));
        assert!(loaded.plugins_loaded.is_empty());
        let reason = &loaded.plugins_rejected[0].1;
        assert_eq!(reason, "license: requires `pro`: not in your plan");
        let _ = std::fs::remove_dir_all(&parent);

        // Every requirement allowed: loads.
        let parent = plugin_parent("requires-allowed");
        write_plugin(&parent, "gated", &gated, ENTRY, &[]);
        let check = owns(&["export.gds", "pro"]);
        let loaded = load_all_with(std::slice::from_ref(&parent), &requires_gate(&check));
        assert_eq!(loaded.plugins_loaded.len(), 1, "{:?}", loaded.plugins_rejected);
        let _ = std::fs::remove_dir_all(&parent);

        // A plugin that requires nothing passes any answer.
        let parent = plugin_parent("requires-nothing");
        write_plugin(&parent, "open", MANIFEST, ENTRY, &[]);
        let check = owns(&[]);
        let loaded = load_all_with(std::slice::from_ref(&parent), &requires_gate(&check));
        assert_eq!(loaded.plugins_loaded.len(), 1, "{:?}", loaded.plugins_rejected);
        let _ = std::fs::remove_dir_all(&parent);
    }
}
