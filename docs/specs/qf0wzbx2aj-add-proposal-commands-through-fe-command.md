---
task_uid: "01m4rqej6qzd5fj5qf0wzbx2aj"
task: "T51"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui test view::proposal_command_tests::commands_and_bindings: command_catalog() accepts the six proposal.* IDs; the macOS default keymap (default-macos.toml) binds alt-f5, alt-shift-f5, cmd-y, cmd-n, cmd-enter and cmd-backspace to them in context editor \u0026\u0026 proposal; the Linux default keymap (default-linux.toml) binds alt-f5, alt-shift-f5, ctrl-alt-y, ctrl-alt-n, ctrl-alt-shift-y and ctrl-alt-shift-n to them in the same context and leaves ctrl-y on editor.redo and ctrl-backspace on editor.delete_word_left; keymap_report finds no issue in either"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test view::proposal_command_tests::keys_need_a_proposal: with a proposal shown and the caret in a hunk's old rows, a simulated cmd-y accepts that hunk (ProposalEvent::Accepted) and cmd-z then reverts it; with no pending or stale hunk shown, cmd-y and cmd-backspace leave the text unchanged and emit nothing"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test view::proposal_command_tests::next_and_previous_wrap: with three hunks, one stale and one rejected, proposal.next_hunk from the document start visits the two pending-or-stale hunks' start rows in order then wraps, proposal.previous_hunk goes the other way, each leaves one caret, and both return Effect::Nothing with no hunk"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui test view::proposal_command_tests::accept_all_and_reject_all: with one stale hunk of three, proposal.accept_all applies the other two in one transaction, emits Accepted for each with that transaction, and one editor.undo reverts both; proposal.reject_all then rejects the stale hunk, leaves the text unchanged and emits Rejected"
    required: true
    manual: false
  - id: "C5"
    text: "fe-gpui test view::proposal_command_tests::host_and_failures: a RunCommand(proposal.reject_hunk) dispatched by the host with the view focused acts as cmd-n does, observe_invocations sees each successful proposal.* invocation, and proposal.accept_hunk on a stale hunk fails with the stale message and changes nothing"
    required: true
    manual: false
  - id: "C6"
    text: "Keymap files: the bindings are appended to crates/fe-keymap/keymaps/fastedit.toml: alt-f5 and alt-shift-f5 for every platform, cmd-y, cmd-n, cmd-enter and cmd-backspace with platform = \"macos\", and ctrl-alt-y, ctrl-alt-n, ctrl-alt-shift-y and ctrl-alt-shift-n with platform = \"!macos\"; default-macos.toml and default-linux.toml each gain their six; fe-keymap test one_keymap passes; the counts pinned in crates/fe-gpui/src/command_tests.rs and keymap_tests.rs go from 143 to 149, the workbench count 61 and OLD_TABLE are unchanged; crates/fe-workbench/src/tests/default_keymaps.rs is unchanged and every fe-workbench default_keymaps test passes"
    required: true
    manual: false
---

# Add proposal commands through fe-command with default keys: next and previous hunk, accept and reject the hunk at the caret, accept all and reject all (Mac T259)

Filed on helium from Mac T259 (~/coordination/helium-cards/T259.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-x.md). Title, type, complexity, scope, criteria and labels as on the Mac.

Helium notes (Mac note x, stream 1, the proposal/AI chain, run strictly one after another for crates/fe-gpui/src/view.rs): counts the card pins (keymap rows in default-macos.toml, keymap_tests.rs and similar) were taken on Mac main; on helium use the counts on your base and change them by exactly the deltas the card states. Screenshot goldens are Metal-only and do not run on helium: a golden criterion is met here by the Linux suite passing, and the Mac regenerates or checks goldens after the sync. Argon's Mac T427 (multi-cursor keys, alt-click) also changes view.rs and vim.rs but is not on helium: do not port it or code around it.

## Problem

After the accept/reject card, a proposal's hunks can be accepted or rejected only by clicking a block's buttons or
through `EditorView::accept_hunk`/`reject_hunk`. The user decided on 2026-10-09 to add keyboard commands: "next/previous
hunk, accept hunk, reject hunk, accept all, reject all, registered through fe-command so they get keybindings
(fe-keymap) and the host app can invoke them".

Commands are registered today as an fe-command `Registry` per subsystem, each a small op table
(`crates/fe-workbench/src/nav.rs` `registry()`, `crates/fe-gpui/src/commands.rs` `app_registry()`); keys reach them as
`RunCommand(Invocation)` (`crates/fe-gpui/src/commands.rs`), the focused `EditorView` runs editor IDs in
`EditorView::run_command` and propagates unknown ones, and `command_catalog()` is what keymap loading checks
bindings against. Default bindings live in `crates/fe-keymap/keymaps/fastedit.toml` (`secondary-` is cmd on macOS
and ctrl elsewhere; `platform = "macos"` / `"!macos"` limits a binding), resolved per platform into
`default-macos.toml` and `default-linux.toml`, which `crates/fe-keymap/tests/one_keymap.rs` checks agree with it.

## Change

- New file `crates/fe-gpui/src/view/proposal_commands.rs` (declared from `view.rs`): `enum ProposalOp`, a
  `ProposalCommands` `CommandTarget` whose target is `Option<ProposalOp>` (as `NavCommands` does), and `registry()`
  with six commands, each in context `editor`, `mutates_buffer` true for the accepts:

  | ID | Title |
  |---|---|
  | `proposal.next_hunk` | Next Proposed Change |
  | `proposal.previous_hunk` | Previous Proposed Change |
  | `proposal.accept_hunk` | Accept Proposed Change |
  | `proposal.reject_hunk` | Reject Proposed Change |
  | `proposal.accept_all` | Accept All Proposed Changes |
  | `proposal.reject_all` | Reject All Proposed Changes |

- `EditorView::run_command` (`crates/fe-gpui/src/commands.rs`): an ID the editor's registry doesn't know is tried
  against the proposal registry before it propagates; the op runs on the view and the result goes through
  `dispatched` (so `observe_invocations` sees it). `command_catalog()`'s `Catalog` checks the proposal registry
  after the editor's.
- What each op does, on the view's shown proposals (all hunks in text order, `Pending` or `Stale` only):
  - the hunk *at the caret*: the one whose old rows contain the primary caret's buffer row, or whose block sits
    directly below that row (a pure insertion);
  - `next_hunk`/`previous_hunk`: one caret at the start of the first hunk starting on a row after (before) the
    primary caret's row, wrapping around the document; autoscroll so its block is in view. `Effect::Nothing` with
    no hunk.
  - `accept_hunk`/`reject_hunk`: `EditorView::accept_hunk`/`reject_hunk` on the hunk at the caret; the caret
    doesn't move. No hunk at the caret is `Effect::Nothing`. Accepting a stale hunk fails with
    `CommandError` "the text under the hunk has changed since it was proposed" and changes nothing.
  - `accept_all`: for each shown proposal, `ProposedChange::accept_all_as(…, Origin::User)` through
    `model_mut().buffer_mut()`, one transaction per proposal (one cmd-z undoes it); stale hunks are skipped and
    stay. Emits `ProposalEvent::Accepted` for each applied hunk with its proposal's transaction.
  - `reject_all`: rejects every pending and stale hunk, emitting `ProposalEvent::Rejected` for each.
  - Host API: `EditorView::{next_hunk, previous_hunk, accept_all, reject_all}` as `pub` methods in the same file;
    the commands call them.
- Key context: `view.rs`'s render adds the flag `proposal` to the editor's `KeyContext` while it shows at least one
  pending or stale hunk.
- Bindings appended at the end of `crates/fe-keymap/keymaps/fastedit.toml`, all in context `editor && proposal`:
  - every platform: `alt-f5` next, `alt-shift-f5` previous (VS Code's next/previous change);
  - `platform = "macos"`, keys written as `cmd-…` (as the file's other macOS-only bindings are): `cmd-y` accept
    hunk, `cmd-n` reject hunk, `cmd-enter` accept all, `cmd-backspace` reject all (Cursor's);
  - `platform = "!macos"` (as the file's other Linux-only bindings are): `ctrl-alt-y` accept hunk, `ctrl-alt-n`
    reject hunk, `ctrl-alt-shift-y` accept all, `ctrl-alt-shift-n` reject all.

  Regenerate the resolved files: `default-macos.toml` gains the two `alt-f5` ones and the four `cmd-` ones,
  `default-linux.toml` the two `alt-f5` ones and the four `ctrl-alt-` ones. fe-workbench's
  `tests::default_keymaps::linux_keymap_binds_the_macos_commands_with_ctrl` (Linux binds the same command set as
  macOS) then passes unchanged.

Not in this card: menu items, a palette, auto-advancing to the next hunk after an accept.

## Tests

New file `crates/fe-gpui/src/view/proposal_command_tests.rs` (declared `#[cfg(test)]` from `view.rs`). The default
binding count pinned in `crates/fe-gpui/src/command_tests.rs` and `crates/fe-gpui/src/keymap_tests.rs` (143 on
main, counted on `default-macos.toml`) rises by exactly these six bindings to 149; the workbench count in
`keymap_tests.rs` (61) is unchanged, `OLD_TABLE` (the first 45 bindings) is unchanged since the new ones are
appended, and any pinned context list gains only `proposal`; nothing else in those files changes. In
`crates/fe-workbench/src/tests/default_keymaps.rs` nothing changes.

## Decided (the user, 2026-10-11): Linux keys for accept/reject

These bindings are live whenever a proposal is shown, while the user keeps typing, so off macOS they must not take
a key editing or vim uses: `ctrl-y` (redo; vim's scroll in T230), `ctrl-n` (vim's down), `ctrl-backspace` (delete
word). Linux uses one chord family instead: `ctrl-alt-y` / `ctrl-alt-n` accept / reject the hunk, plus `shift`
for all. Not `ctrl-alt-backspace` (X11's zap) and not `ctrl-enter` (it would break the pattern). `alt-f5` stays on
every platform for VS Code parity, though GNOME's mutter may take it for unmaximize; users can rebind it.
