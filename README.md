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

Later arrivals by the same rule (duplicated in both frameworks today):
i18n mechanics, product identity,
and the account / entitlements transport.

## Dependency direction

Zero gpui. The crate is UI-framework-agnostic on purpose: both consumers sit
on gpui-fast, but the overlays that render the picker stay in each framework.
Native builds never compile the web-sys layer (it is target-gated).

## Consumers

Path dependencies from sibling checkouts (the rcad-pro root lays every
checkout out side by side):

- nanobot (`nanobot/crates/nanobot-ui`, `nanobot-session`, `nanobot-boot`):
  `../../../product-kit`
- workbench-rs (`rcad/workbench-rs/crates/workbench-core`,
  `crates/workbench-api`): `../../../../product-kit`
