---
task_uid: "01m46tzwfgxdefe5dmtwg695rw"
task: "T81"
rev: 10
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo run -p fe-workbench --example screenshots prints ok for the workbench scene and exits 0; a second UPDATE_SCREENSHOTS=1 run leaves golden/workbench.png byte-identical"
    required: true
    manual: false
  - id: "C2"
    text: "golden/workbench.png shows the demo tree on the right with src expanded, the tab bar with README.md and main.rs (main.rs active) and highlighted Rust in the editor"
    required: true
    manual: false
  - id: "C3"
    text: "The example builds as a test in the gate (test = true) and includes crates/fe-gpui/examples/screenshots/compare.rs by #[path], so compare's unit tests run for it; nothing under crates/fe-gpui changes"
    required: true
    manual: false
  - id: "C4"
    text: "Cargo.lock changes only in fe-workbench's own dependency list (gpui_platform and image as dev-dependencies), adding no package; .github/workflows/ci.yml runs the new example and uploads its diffs on failure; TESTING.md documents the scene"
    required: true
    manual: false
  - id: "C5"
    text: "The golden harness registers fe_ui::Assets (VisualTestAppContext::with_asset_source) so the screenshot includes the tree's icons and chevrons from T115/T116"
    required: true
    manual: false
  - id: "C6"
    text: "The golden scene enables the unified title bar (set_title_bar(true), T128) so the screenshot shows the 32 px bar; AppKit's traffic lights are not expected in the offscreen render"
    required: true
    manual: false
---

# Add a golden screenshot of the workbench: tree, tabs and editor

# Add a golden screenshot of the workbench: tree, tabs and editor

Milestone M3 "Browse", third batch. T11's harness is `crates/fe-gpui/examples/screenshots/`:

- `main.rs` renders scenes off screen through `VisualTestAppContext` with the real Metal renderer
- `compare.rs` does the pixel comparison and writes the failure diff
- goldens live in `golden/`

`TESTING.md` "Screenshots" documents it, and GitHub CI runs
`cargo run -p fe-gpui --example screenshots`. The Regatta gate doesn't render, because AppKit needs the
main thread, so the gate builds the example as a test and runs `compare.rs`'s unit tests.

T63 left the workbench without a look check. Its "Decided" section names a workbench screenshot scene
as a later card.

## Problem

Nothing catches a visual regression in the workbench chrome: the tree rows, the dock header, the tab
bar, the borders, or the editor's placement in the window. `fe-gpui` can't render the workbench,
because `fe-gpui` must not depend on `fe-workbench` (the dependency points the other way).

## Change

- **The scene lives in `fe-workbench`:** new `crates/fe-workbench/examples/screenshots/main.rs`, with
  goldens in `crates/fe-workbench/examples/screenshots/golden/`.
  - It reuses fe-gpui's comparison by including the file with
    `#[path = "../../../fe-gpui/examples/screenshots/compare.rs"] mod compare;`. That avoids a new
    crate and a copy.
  - The few lines of capture and settle loop are duplicated from fe-gpui's `capture`: draw until a
    frame leaves the root view un-notified, at most 10 frames. Note in a comment where they come
    from.
  - Factoring a shared helper crate isn't worth it for two callers.
- **One scene, `workbench`, at 1000×600 logical pixels** (2× on the goldens' display). It contains:
  - a project written by the example at run time into `<tempdir>/demo`, so the root name is fixed:
    `src/` with `main.rs`, `lib.rs` and `util/` (collapsed), plus `Cargo.toml`, `README.md`, a
    `.gitignore` and an ignored `target/`
  - `src/` expanded
  - `README.md` and `src/main.rs` open, with `main.rs` active, a caret in it, and Rust highlighting
    landed
  - the dock showing Project with `src/main.rs` selected, if the tree-keys card has landed (this card
    depends on it)
  - the editor font pinned to Menlo, as fe-gpui's scenes are. The UI font is whatever `UiTheme` uses;
    pin it too if `UiTheme` has a font family token.
  - the file watcher may run, but the scene must settle before capture
- The same commands as fe-gpui's harness:
  - `cargo run -p fe-workbench --example screenshots` compares
  - `UPDATE_SCREENSHOTS=1 …` rewrites
  - `-- render out.png <dir>` renders a given folder for a look at a UI change
- `crates/fe-workbench/Cargo.toml`:
  - dev-dependencies `gpui_platform` (`test-support`) and `image`, both workspace crates already in
    `Cargo.lock`
  - `[[example]] name = "screenshots", test = true`

  Only `fe-workbench`'s entry in `Cargo.lock` changes. That needs a person's approval at merge, as do
  the CI changes.
- `.github/workflows/ci.yml`:
  - a step `cargo run -p fe-workbench --example screenshots` after fe-gpui's
  - the failure artifact also uploads `crates/fe-workbench`'s diffs. Use the same `target/screenshots/`
    output dir, with the scene name prefixed `workbench`.
- `TESTING.md`: the Screenshots section and table row name the workbench scene and its commands.

## Out of scope

- Scenes for the search, outline, Cmd-P overlay or splits. Each can be added as one scene later, once
  their look settles.
- Running screenshots in the Regatta gate.

## Decided (lead defaults; the user may revisit)

- The scene lives in `fe-workbench/examples`, sharing `compare.rs` by `#[path]` include. There's no
  shared helper crate and no copy.
- One 1000×600 scene of the default layout.
- A later card that changes the default layout's look regenerates this golden in the same change, as
  TESTING.md already asks for fe-gpui's scenes.
