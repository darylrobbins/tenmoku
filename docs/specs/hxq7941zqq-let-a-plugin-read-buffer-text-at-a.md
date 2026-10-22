---
task_uid: "01m5jjkym4bzn0h0hxq7941zqq"
task: "T284"
rev: 2
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "editor::read_returns_text_and_version_with_buffer_read: the editor.wasm guest reads a buffer and gets its text and version, and a range's text, with buffer.read"
    required: true
    manual: false
  - id: "C2"
    text: "editor::reads_and_edits_without_the_capability_are_refused: without buffer.read a read fails and without buffer.write an edit or a proposal fails, as denied, and the buffer is unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "editor::a_read_over_the_max_is_an_error_not_a_truncation: a buffer over 16 MiB is refused with an error rather than returned in part"
    required: true
    manual: false
  - id: "C4"
    text: "editor::edits_apply_as_one_host_origin_transaction: three edits make one transaction with Origin::Host(id) and one undo reverts them all"
    required: true
    manual: false
  - id: "C5"
    text: "editor::a_stale_version_applies_nothing: an edit against an older version is refused and the buffer is unchanged"
    required: true
    manual: false
  - id: "C6"
    text: "editor::bad_ranges_are_refused: overlapping, unsorted, out-of-range and non-char-boundary ranges are errors and apply nothing"
    required: true
    manual: false
  - id: "C7"
    text: "editor::undo_by_origin_reverts_only_the_plugins_edits: user and plugin edits interleaved, undo of Origin::Host(id) reverts only the plugin's edits"
    required: true
    manual: false
  - id: "C8"
    text: "editor::a_proposed_change_waits_for_the_user: propose creates a ProposedChange and leaves the buffer unchanged until the user accepts it"
    required: true
    manual: false
  - id: "C9"
    text: "cargo nextest run -p fe-plugin and cargo clippy -p fe-plugin --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Let a plugin read buffer text at a version and edit it as one Host-origin transaction against that version, or propose a change the user accepts (Mac T1058)

## Problem

The editor's value to plugins is reading buffer text and editing it (§7): snapshot text and ranges at a version; edit
one transaction with `Origin::Host("<extension id>")`, applied against the version read
(`Buffer::edit_from`, conflicting edits refused), undoable per origin; or a `ProposedChange` the user accepts. No
host side of `fastedit:editor` exists.

## Change

`crates/fe-plugin/src/editor.rs`, `crates/fe-plugin/wit/buffer.wit`, headless over `fe-buffer` (no GPUI):

- `BufferAccess` trait implemented by the app over its buffers (and by tests over `fe_buffer::Buffer`):
  `snapshot(buffer) -> (text, version)`, `edit(buffer, version, edits, origin)`, `propose(buffer, change)`.
- Host imports: `read(buffer, range?) -> (text, version)` needs `buffer.read` and fails over `Limits::max_read_bytes`
  (16 MB) with an error, not a truncation; `edit(buffer, version, edits)` needs `buffer.write`, takes sorted
  non-overlapping UTF-8 ranges on char boundaries, and applies them as one transaction with
  `Origin::Host(<id>)` through `Buffer::edit_from`, so a stale version applies nothing and says so;
  `propose(buffer, version, edits)` creates a `ProposedChange` instead of editing (needs `buffer.write`).
  Undo per origin reverts only that plugin's transactions.
- Rust guest fixture, because lists of records are impractical in WAT: `crates/fe-plugin/tests/guests/editor/`
  (`Cargo.toml` with its own empty `[workspace]` table, `src/lib.rs` using `wit-bindgen`, `build.zsh` running
  `cargo build --release --target wasm32-wasip2` and copying `editor.wasm` beside it). `editor.wasm` is
  checked in; CI never builds it.

## Notes

Decorations, gutter and status items are P12; the command allowlist is P8. The guest's source and the build script
are the only way to rebuild `editor.wasm`; say in the finish notes which toolchain target was used.

Project rules: keep tests lean and fast (hostile and process fixtures bounded to under 2 s each); new tests go in new

Lend note (helium lead): the lend file cut long lines (title, scope, some criteria and spec sentences); this card's title, scope and the ends of cut criteria were completed from the spec. The Mac copy is authoritative: if it differs, respec to it.

## Implementation notes

- `buffer.wit`: `read` takes `range: option<range>` and `read-ranges` is gone, as the change lists
  three imports. `propose` keeps its `proposed-change` record (buffer, version, title, edits), so
  the title the user sees comes with it.
- A version other than the buffer's current one is `conflict` for `edit` and `propose` even when
  the edits are apart from what changed since, as the WIT says; the ranges are checked against
  that version's text before `BufferAccess` sees them, and `BufferAccess::edit` applies them
  through `Buffer::edit_from`. Two insertions at one offset are refused, as
  `ProposedChange::from_edits` would panic on them and `Buffer::edit` merges them.
- A read over `Limits::max_read_bytes` is `failed`, as kiln's over-limit response body is.
- fe-buffer's `Version` has no public number, so `BufferAccess` deals in the `u64` version plugins
  see; the test's impl counts the buffer versions it has shown.
- `editor.wasm` was built by `build.zsh --offline` with rustc 1.98.1 for `wasm32-wasip2`
  (wit-bindgen 0.57.1). Its std imports WASI it never calls; the test links those imports as traps.
