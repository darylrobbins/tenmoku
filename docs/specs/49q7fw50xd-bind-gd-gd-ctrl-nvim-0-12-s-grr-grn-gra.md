---
task_uid: "01m55qj2qnbe849t49q7fw50xd"
task: "T191"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration test vim_ide_keys_resolve_in_normal_mode (top level, in crates/fe-keymap/tests/vim_ide_keys.rs) passes: on macOS and Linux, g d, ctrl-], g shift-d, g r r, g r n, g r a, g r i, g r t, g shift-o and shift-k resolve to nav.goto_definition (twice), nav.goto_declaration, nav.find_references, lsp.rename, lsp.code_actions, nav.goto_implementation, nav.goto_type_definition, outline.show and editor.show_hover in an editor in vim normal mode with nothing pending, and g r a also in vim visual mode."
    required: true
    manual: false
  - id: "C2"
    text: "The integration test vim_ide_keys_leave_vims_keys (top level, same file) passes: none of them resolves in insert, replace or operator-pending mode, with a count or register pending, or with vim off; g g, g r x and g u match no binding in normal mode; and cargo test -p fe-gpui command_tests passes with the new binding count."
    required: true
    manual: false
---

# Bind gd, gD, ctrl-], nvim 0.12's grr grn gra gri grt, gO and K in vim normal mode to fastedit's language-server commands (Mac T900)

## Problem

At Mac main 6f032b12 vim's normal mode has no keys for fastedit's language-server commands: `gd`, `K` and
nvim 0.12's `gr` keys cancel, and the VS Code keys (`f12`, `shift-f12`, `f2`, `cmd-.`, the hover chord) are the
only way in. nvim 0.12.6 maps by default (verified with `maparg`): `grr` references, `grn` rename, `gra` code
action (normal and visual), `gri` implementation, `grt` type definition, `gO` document symbols, and `K` hover
once a server attaches; VSCodeVim and IdeaVim map `gd` to go to definition. The user approved IDE keys in
vim mode in the "Vim parity" track on 2026-10-17. `]d`/`[d` (diagnostics) are already in T523.

## Change

In `crates/fe-keymap/keymaps/fastedit.toml`, bindings in `editor && vim.mode == normal && !vim.pending`
(as T523 scopes `]d`), after the vim layer's other bindings:

- `g d` → `nav.goto_definition` and `ctrl-]` → `nav.goto_definition` (nvim's tag jump goes to the
  definition through the server; this also takes `ctrl-]` from `editor.indent` in vim's normal mode off
  macOS); `g shift-d` → `nav.goto_declaration` (from T852).
- `g r r` → `nav.find_references`, `g r n` → `lsp.rename`, `g r a` → `lsp.code_actions` (also in
  `editor && vim.visual`, where the model's selection is the visual one), `g r i` →
  `nav.goto_implementation`, `g r t` → `nav.goto_type_definition` (both from T852).
- `g shift-o` → `outline.show`; `shift-k` → `editor.show_hover`.

`gd` thus goes to the language server's definition, as VSCodeVim's does, not nvim's search for a local
declaration. As in nvim 0.12, `gr` followed by any other key still reaches vim (the keymap replays a prefix it
can't complete), as do `g g` and every other `g` key. Update `crates/fe-gpui/src/command_tests.rs`'s binding
count as T523 does.

Test: a new `crates/fe-keymap/tests/vim_ide_keys.rs` modelled on `tests/vim_window_keys.rs`: each binding
resolves to its command on macOS and Linux in vim normal mode with nothing pending, and to nothing in insert,
operator-pending and (but for `g r a`) visual mode or with vim off; `g g`, `g r x` and `g u` match no binding.

fastedit.toml is also changed by T523, T820, T848, T852 and T855: this card depends on T523 and T852.
