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

Later arrivals by the same rule (duplicated in both frameworks today):
i18n mechanics, the settings model, product identity,
and the account / entitlements stack.

## Dependency direction

Zero gpui. The crate is UI-framework-agnostic on purpose: both consumers sit
on gpui-fast, but the overlays that render the picker stay in each framework.
Native builds never compile the web-sys layer (it is target-gated).

## Consumers

Path dependencies from sibling checkouts (the rcad-pro root lays every
checkout out side by side):

- nanobot (`nanobot/crates/nanobot-ui`, `nanobot-session`, `nanobot-boot`):
  `../../../product-kit`
- workbench-rs (`rcad/workbench-rs/crates/workbench-core`):
  `../../../../product-kit`
