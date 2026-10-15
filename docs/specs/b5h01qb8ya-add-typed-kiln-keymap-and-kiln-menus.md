---
task_uid: "01m50ewgzhtctt61b5h01qb8ya"
task: "T91"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "kiln tests/typed_tables.rs: .keys(kiln::keymap!{..}) binds its keys (per-platform and in a context too) and replaces the commands' attribute default keys, under the app's TOML and the user's file"
    required: true
    manual: false
  - id: "C2"
    text: "typed_tables.rs: .menu_bar(kiln::menus!{..}) installs those menus with each command's title and separators, replacing contributed placement and any .menus(path, toml)"
    required: true
    manual: false
  - id: "C3"
    text: "A compile_fail doctest in tables.rs: a misspelt command type in menus! fails to compile"
    required: true
    manual: false
---

# Add typed kiln::keymap! and kiln::menus! tables, so a curated keymap or menu bar names command types and a typo is a compile error (Mac T681)

Filed on argon from Mac T681 (card ~/coordination/argon-cards/T681.json rev 1; note zh). Mac task uid 01m4vbjarppw2gzabx3q6134n2. Base: argon trunk bb63a81 = mac/main 6ac346f synced. Design record: docs/decisions/kiln-app-layer.md. Argon deps: T90 (Mac T680). Mac T675 rewrites kiln/crates/kiln/examples/host and tests/host.rs and re-lands soon; don't touch those, the spec cites them only as background. Title the host commit "... (Mac T681)".

## Problem

Part of the Kiln app layer the owner approved on 2026-10-13 (phase 2; design record
`docs/decisions/kiln-app-layer.md`). Attribute defaults (`#[kiln::command(key = .., menu = ..)]`) suit a small
app; a curated app wants one reviewable table, but today's tables are TOML naming command IDs as strings, so a
typo is a run-time warning (`kiln-core/src/keymap.rs:506`, `menu.rs:733`). The design prototype's typed
`menus!` turned a typo into `error[E0425]: cannot find type NewNotes … similarly named struct NewNote defined
here`.

## Change

Add `kiln::keymap!` and `kiln::menus!` as `macro_rules!` in a new `kiln/crates/kiln/src/tables.rs` (no proc
macro needed), over the `Command` trait typed commands implement:

```rust
kiln::app("dev.kiln.Notes", "Notes")
    .keys(kiln::keymap! { "secondary-shift-n" => NewNote, "f2" => RenameNote in "note" })
    .menu_bar(kiln::menus! { "File" => [NewNote, Separator, kiln::app::Quit] })
```

- `keymap!` builds a keymap layer (keys, command ID, optional context); per-platform keys as
  `{ macos = "..", default = ".." }`. `.keys(..)` **replaces** the commands' attribute default keys (the
  contributions layer), under the app's TOML and the user's file as before.
- `menus!` builds a `MenuBar` (titles from each command's title, `Separator`); `.menu_bar(..)` replaces the
  contributed menu placement and any `.menus(path, toml)`.
- Both are checked like the TOML tables at init (an unknown context is still a diagnostic), and documented with
  their TOML equivalents.

New tests go in `kiln/crates/kiln/tests/typed_tables.rs`; the typo check is a `compile_fail` doctest in
`tables.rs`.
