---
task_uid: "01m6c8pkffk28kk41d9q33pxdj"
task: "T280"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "RefactorPreview::needs_preview is false when every text edit is in the source buffer, and true with an Open edit to another buffer, any Closed or Depends entry, or any Create, Rename or Delete (tests in crates/fe-workbench/src/refactor_preview_tests.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "RefactorPreview::new lists each text-edited file once, in edit order, through an fe_editor::Changeset over scratch buffers it owns: one hunk per server edit in text order, no-op edits kept, inserts at one offset merged into one hunk. No real buffer or file changes"
    required: true
    manual: false
  - id: "C3"
    text: "selected() equals the input when nothing is rejected. With hunks rejected it keeps each entry's other edits (Open keeps buffer and version; Closed keeps its TextEdits in order and its encoding), drops a file whose hunks are all rejected, and keeps entry order"
    required: true
    manual: false
  - id: "C4"
    text: "An edit with any file operation is whole-only: is_decidable() is false, accept and reject calls return an error and change nothing, and selected() equals the input"
    required: true
    manual: false
  - id: "C5"
    text: "The module uses no gpui; cargo test -p fe-workbench refactor_preview and cargo clippy -p fe-workbench --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Model a multi-file language-server edit as a refactor preview: keep or leave out each file and hunk, and give back the edit left to apply (Mac T1283)

## Problem

A language-server rename or code action applies its whole workspace edit at once, with no preview:
`Workbench::rename_symbol`'s picker confirm (`crates/fe-workbench/src/lsp_rename.rs`, `apply_user_file_edits`
at line ~164) and both code-action paths (`crates/fe-workbench/src/lsp_code_actions.rs` `apply_alone` and the
picker's `apply`) hand the decoded `Vec<fe_lsp::FileEdits>` straight to `Workbench::apply_user_file_edits`
(`crates/fe-workbench/src/workspace_edit.rs`). A rename that touches thirty files changes thirty files, some
closed and written straight to disk, before the user sees one of them.

The user (2026-11-01): before applying an LSP rename or a code action whose workspace edit touches more than the
current file, show a multi-file preview diff with per-file and per-hunk accept/reject, then apply it as one
undoable step per buffer, reusing the proposal diff UI and the changeset view rather than a new diff surface.

This card is the headless half: the preview model. A second card (the view) shows it in the AI Changes panel and
wires rename and code actions to it.

## Change

Add `crates/fe-workbench/src/refactor_preview.rs`, declared from `workspace_edit.rs` as
`#[cfg_attr(not(test), allow(dead_code))] #[path = "refactor_preview.rs"] pub(crate) mod refactor_preview;`
(no `lib.rs` change; the view card uses it), with its tests in `crates/fe-workbench/src/refactor_preview_tests.rs`
declared from it as the crate's other `*_tests.rs` are. No GPUI in this module: fe-buffer, fe-editor, fe-lsp and
fe-text only.

- **`RefactorPreview::needs_preview(edits: &[FileEdits], source: BufferId) -> bool`**: true when any text edit
  targets a file other than the `source` buffer (an `EditTarget::Open` of another buffer, any `Closed`, any
  `Depends`), or the edit has any file operation (`Create`, `Rename`, `Delete`). A rename inside one file applies
  as today.
- **`RefactorPreview::new(title, edits: Vec<FileEdits>, text_of)`**, where `text_of: impl FnMut(&Path,
  &EditTarget) -> Result<String, String>` gives each text-edited file's current text (the view passes an open
  buffer's text, or `loader::read` for a closed file). Each file gets a scratch `fe_buffer::Buffer` holding that
  text, owned by the preview, and one `ProposedChange` over it built with
  `ProposedChange::from_edits_keeping_noops`, so hunk *i* is the file's edit *i* in text order. `Closed` edits are
  decoded with `fe_lsp::decode_closed` for the scratch copy. Edits that insert at the same offset become one
  hunk, decided together (as `workspace_edit.rs` `merge_inserts` merges them), so `from_edits` never panics on
  LSP input.
- The proposals go into an `fe_editor::Changeset` (`Changeset::new(title, Origin::Lsp)`, `add_file`) over a
  `BufferSet` the preview implements on its scratch buffers. Listing, counts, summary, `accept_hunk`,
  `reject_hunk`, `accept_file`, `reject_file`, `accept_all` and `reject_all` are the changeset's, so the AI Changes
  panel's rows and counts work unchanged. Accepting in the preview only marks a hunk kept; it changes no real
  buffer.
- **`RefactorPreview::selected(&mut self) -> Vec<FileEdits>`**: the edit to apply. Every entry in the input order;
  a text entry keeps only the edits whose hunk isn't `Rejected` (pending and accepted are both kept, so the
  default is "apply everything"); an entry with every hunk rejected is dropped. `Open` keeps its `buffer` and
  `version`; `Closed` keeps the kept `TextEdit`s in their order and its `encoding`. With nothing rejected it is
  equal to the input.
- **File operations**: an edit with any `Create`, `Rename`, `Delete` or `Depends` entry is previewed whole-only:
  `is_decidable()` is false, every reject/accept call returns `Err(ChangesetError::NotDecidable)` (or the
  preview's own error type), and `selected()` returns the input unchanged. Per-hunk decisions over file
  operations are a later card.

Nothing applies here: the view card calls `Workbench::apply_user_file_edits(preview.selected(), cx)`, which
already checks versions, writes closed files and makes one user transaction per open buffer.

## Implementation notes

- `fe_lsp::FileEdits` is `#[non_exhaustive]` with no public constructor, so the tests decode real edits
  through the fake server (`Workbench::lsp_decode_edit`), as `workspace_edit_tests.rs` does; they're gpui
  tests, the module itself isn't.
- `ChangesetError` has no `NotDecidable` (fe-editor is out of scope): the preview's own `PreviewError`
  is `NotDecidable` or wraps the changeset's error.
- `new` returns `Result<Self, String>`: a file's text that can't be had, or edits that don't fit it (out
  of range, inside a character, overlapping), refuse the preview instead of panicking in `from_edits`.
- Whole-only edits are shown as they'd leave the files: a created file starts empty and isn't read, a
  renamed one is read by its old path, `Depends` shows the made branch (skipped if made can't apply).
  A file edited in two runs around an operation is one line diff (`ProposedChange::from_text`) of its
  text before and after both, an open buffer's later run moved past the earlier as applying does.
- Review round 1: the simulation keeps each path's live text through create (emptied), delete (gone) and
  rename (moved, replacing the target), apart from the listing records, and an open buffer's text and
  runs by `BufferId`, so a renamed buffer's later run is rebased past its earlier one as applying does.
  A closed file's inserts are normalized one by one before inserts at one place join, as `Buffer::edit`
  does; an open one's join first, as `merge_inserts` and `from_edits` do. Tests compare against a real
  apply.
- Review round 2: a rename to its own path (a file or a folder) changes nothing in the simulation, as
  applying makes nothing (`Made::Nothing`).
- fe-lsp hands `Closed` edits sorted by start already, so "in their order" is that order.
