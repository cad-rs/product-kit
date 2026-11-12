# product-kit

Product infrastructure shared by the rcad frameworks (the nanobot harness and
the workbench framework): the pieces both products need and that must not
drift into two implementations.

- **storage** — `Location` (provider id + provider key), `Entry`,
  `StorageProvider` / `Storage`: where user files live, addressed uniformly
  across the browser's origin-private file system, OS paths and cloud stores.
- **picker** — `FilePicker`, the in-app file picker's state machine (sources,
  breadcrumbs, folder-first listings, save-name rules). Pure logic, fully
  unit-tested; the frameworks draw their own overlays over it.
- **opfs** — the origin-private file system as a `StorageProvider` (wasm
  builds; async page-thread API only).
- **atomic** — whole-file replacement that never risks the previous content
  (native builds; temp file + fsync + rename, Windows lock retry). Every
  durable file the frameworks write goes through it.
- **entitlement_refresh** — `RefreshSchedule` (when to re-check the product's
  plan: startup, foreground, sign-in change, half-hourly, user-asked, and the
  two-minute post-purchase poll) and `SessionGrace` (a capability lost
  mid-session stays usable until the app closes). The frameworks plug their
  own answer type in through the `EntitlementSource` / `EntitlementAnswer`
  pair.
- **settings** — `Settings`, flat dotted-key JSON (`ui.theme`,
  `plugin.<id>.enabled`) written through on every change, loaded never-fatal.
  Persistence goes through the `SettingsStore` seam (workbench's keyed app
  state, nanobot's settings file); workbench adds a `SettingSpec` schema on
  top.
- **diagnostics** — `RollingFile` (5 MB, three generations), `tail`, the UTC
  clock helpers, and the crash-report text (`crash_report` / `panic_message`).
  Native-only for the file parts; the frameworks keep their own logger, crash
  report backend and export assembly.
- **selfcheck** — the `Check` record and the `report`/`Summary` that print
  PASS/FAIL/SKIP lines and turn them into a process exit code. The check
  suites and the crash-recovery probe driver stay in each framework.
- **auth_code** — the browser sign-in redirect's loopback HTTP mechanics
  (`read_callback_query`/`respond`) and the PKCE helpers
  (`code_challenge`/`read_callback` with a product-neutral `Callback`
  verdict). The transport, the web callback variant and the token ownership
  stay in each framework.
- **oauth** — the device-flow grant's request shapes
  (`device_authorization_form`/`token_poll_form`) and the RFC 8628 §3.5 error
  vocabulary (`classify_poll_error`). The transport, endpoint discovery and
  token cache stay in each framework.
- **crash_probe** — the selfcheck's crash-recovery subprocess probe: the
  `ProbePhase` (A/B/C) read from the environment, the working directory, and
  `run_phase` (spawn + timeout + capture) with a product-neutral `ProbeFailure`.
  The probe bodies and the error wording stay in each framework.

The `crates/` members are the GPU view slot, shared with the same rule:

- **gpu-readback** — `ReadbackTarget`: an offscreen color texture plus its
  aligned readback buffer, and `read` copying a frame back as tightly packed
  BGRA rows. The one piece of the slot that is not platform-specific; a
  separate crate so headless render cores (no GUI dependency) share it too.
- **gpui-gpu-view** — the slot itself: the process-wide wgpu device
  (`shared_gpu` / `Gpu` / `GpuRequirements`), the per-platform presentation
  (web dynamic texture, Windows DirectComposition surface, offscreen readback),
  the `Viewport` / `ViewportContent` / `ViewportInput` shell, and the
  `ViewportSlot` contract a product renderer implements.
- **plugin-host** — the Rhai script-plugin host: the `plugin.toml` manifest
  (`parse` / `validate` / `split_entry_point`), the load/exec engines with the
  injected Host API (seven registration globals, six `Task` methods), and the
  neutral loading pipeline (`LoadedPlugins`, `PluginGate`, `requires_gate`).
  The frameworks' adapters commit the declarations — workbench's into its
  `Registry`, nanobot's shell into its panel host.

Later arrivals by the same rule (duplicated in both frameworks today):
i18n mechanics, product identity,
and the account / entitlements transport.

## Dependency direction

`product-kit` itself is zero gpui: UI-framework-agnostic on purpose, so both
consumers (which sit on gpui-fast) can share it, and native builds never
compile the web-sys layer (it is target-gated).

The `crates/` members are the exception by design: `gpui-gpu-view` is the
shared GPU render slot and depends on gpui + wgpu, `gpu-readback` is its
platform-independent readback target (wgpu only, so headless cores can use it
without pulling gpui in), and `plugin-host` is the shared Rhai plugin host
(rhai + toml + serde, no GUI).

## Consumers

Path dependencies from sibling checkouts (the rcad-pro root lays every
checkout out side by side):

- nanobot (`nanobot/crates/nanobot-ui`, `nanobot-session`, `nanobot-boot`,
  `nanobot-rhai`): `../../../product-kit`; the GPU slot:
  `../../../product-kit/crates/gpui-gpu-view`; the plugin host:
  `../../../product-kit/crates/plugin-host`
- workbench-rs (`rcad/workbench-rs/crates/workbench-core`,
  `crates/workbench-api`): `../../../../product-kit`; the GPU slot:
  `../../../../product-kit/crates/gpui-gpu-view`, the readback target:
  `../../../../product-kit/crates/gpu-readback`, the plugin host:
  `../../../../product-kit/crates/plugin-host`
