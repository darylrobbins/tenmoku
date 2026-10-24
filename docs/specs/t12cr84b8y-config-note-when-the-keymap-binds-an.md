---
task_uid: "01m5n8y590jf2bmst12cr84b8y"
task: "T209"
rev: 2
type: "feature"
complexity: ""
criteria:
  - id: "C1"
    text: "remap_keymap_conflicts(keymap, remaps, leader) returns a conflict for an Insert map whose left side or a prefix of it the keymap binds in an editor stack with vim.mode == insert (and replace), and for a CmdLine map against the vim_line context; none for a binding scoped to other modes or contexts (tests in keymap_remap_conflicts_tests.rs against the default keymap: inoremap jk \u003cEsc\u003e has no conflict, inoremap \u003cC-t\u003e x does)"
    required: true
    manual: false
  - id: "C2"
    text: "VimRemaps::diagnostics() reports one Warning per conflict at the map's vimrc line or [[vim.remap]] entry, naming the left side, the binding's keys and command and saying the keymap wins; it is recomputed when the vimrc, settings or keymap reload; documented limit: a keymap swapped by SharedKeymap::update (extension bindings) is not recomputed until the next reload (follow-up card)"
    required: true
    manual: false
  - id: "C3"
    text: "n/x/o maps raise no such note (out of scope); the other vim_remaps tests and config_problems tests pass unchanged"
    required: true
    manual: false
---

# Config note when the keymap binds an insert or cmdline remap's left side or a prefix of it (the keymap wins there) (T877 E) (Mac T1093)

## Problem

T877 split, card E. Record section 4 rule 1: the keymap wins where it matches, so an
`i`/`c` map whose left side (or a prefix of it) the keymap binds in the same
context can never run. The record asks for a load note in the config problems list
naming the binding, saying the keymap wins there, raised only for modes and
contexts that actually bind it. No such note exists for any mode yet:
`fastedit/src/vim_remaps.rs` (the config source that builds the set and reports
problems) never reads the keymap, and `RemapSet::hides_builtin` has no non-test
caller.

## Change

- `crates/fe-gpui/src/keymap.rs` (or a new `crates/fe-gpui/src/keymap_remap_conflicts.rs`
  re-exported from `lib.rs`): `pub fn remap_keymap_conflicts(keymap: &Keymap,
  remaps: &[Remap], leader: &str) -> Vec<RemapConflict>` for `Insert` maps (context
  `editor` with `vim.mode == insert`/`replace`) and `CmdLine` maps (context
  `vim_line`). A binding conflicts when its `KeySequence` equals the left side or is
  a prefix of it, and its `ContextPredicate` is satisfied by a `ContextStack`
  stamped for that mode (see `stamp`/`context_stack` in `fe-gpui/src/vim.rs` and
  `keymap.rs`). Left sides use the same `<leader>` and notation fe-vim parses.
- `crates/fastedit/src/vim_remaps.rs`: `VimRemaps::install`/`reload` read the keymap
  in use (`fe_gpui::keymap` current keymap) and add one Warning per conflict to
  `diagnostics()`, at the vimrc line or the `[[vim.remap]]` entry that wrote the
  map (the code already places problems that way: `Problem::Entry`, `at_line`). A
  keymap reload re-runs it. Message: ``"{from}" is bound in the keymap in {mode};
  the keymap wins there``, naming the binding's keys and command.
- New tests in NEW file `crates/fe-gpui/src/keymap_remap_conflicts_tests.rs`
  (pure keymap + stack tests) and a case in `crates/fastedit/src/vim_remaps.rs`'s
  existing test module only if the shared `Files` helper makes it a few lines.

## Notes

- Needs card B (the loaded `i`/`c` records); touches `fastedit/src/vim_remaps.rs`,
  which B also edits, so run after B.
- Unsure: whether the keymap's reload order guarantees the new keymap is in place
  when `VimRemaps::reload` runs (settings-then-remaps ordering is documented there;
  keymaps are not). If not, expose the check as a separate kiln config source.
## Revision 2 (lead decision on review F2, medium): documented limit, follow-up card

The conflict warnings refresh whenever `ConfigSource::reload` runs (the vimrc, settings or keymap file changed). A keymap swapped by `SharedKeymap::update` (extension bindings) does not call the sources or republish diagnostics, so those warnings are not recomputed until the next reload. Fixing that needs a keymap-change observer in kiln-platform (keys.rs, outside this scope) and a way to republish `ConfigDiagnostics` from `VimRemaps`: a follow-up card. C2 now states this limit.

## Implementation notes

- Known limit, tracked in T223: keymap conflict warnings are recomputed in `VimRemaps::reload` only; a `SharedKeymap::update` (extension bindings) doesn't trigger it. Noted on `reload` in `crates/fastedit/src/vim_remaps.rs`.
