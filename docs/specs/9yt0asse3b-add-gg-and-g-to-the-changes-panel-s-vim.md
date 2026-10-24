---
task_uid: "01m5qphvaa88fzeb9yt0asse3b"
task: "T323"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "git_panel_vim_nav_tests::first_last: with vim on and the Changes panel focused over three entries in two groups, G selects the last entry, g g the first; the selection scrolls into view"
    required: true
    manual: false
  - id: "C2"
    text: "git_panel_vim_nav_tests::pending_g: g then j selects the next entry from where it was (the g is dropped, as vim drops an unknown g-sequence); g g after an intervening focus change is not completed from the earlier g"
    required: true
    manual: false
  - id: "C3"
    text: "git_panel_vim_nav_tests::vim_off: with vim off g and shift-g do nothing and arrow keys still move; with the commit box focused g and shift-g type into it"
    required: true
    manual: false
  - id: "C4"
    text: "git_panel_vim_nav_tests::empty: gg and G over an empty list do nothing and do not panic; the existing git_panel tests pass unedited, and cargo clippy -p fe-workbench --all-targets -- -D warnings passes"
    required: true
    manual: false
---

# Add gg and G to the Changes panel's vim keys, beside its j, k, o, s, u, - and X, with a pending-g that any other key cancels (Mac T1131)

## Problem

User 2026-10-24: "Do we offer good vim control of the explorer/outline?" The Changes panel already has j, k, o, s, u, - and X in vim mode (git_panel.rs:923-990), but not gg and G, which the project tree and (after the sibling list-key card) the other lists have.

## Current state

git_panel.rs `on_key_down` (:923) matches single keys: up/down/enter, `k`/`j` under `vim`, `o s u - X`. A bare key only; it has no pending state, so `g g` cannot be a match arm. The panel reads vim with `fe_gpui::vim_enabled(cx)` (:951). The commit box is a separate editor focused inside the panel (key context git_panel && editor), so its typing is not in this handler.

## Change

- Add `pending_g: bool` to the panel; in `on_key_down` under `vim`: `g` sets it (and stops propagation), the next `g` selects the first entry, `shift-g` selects the last, any other key clears it and is then handled normally. Focus-out clears it.
- Selection goes through the existing `self.select(ix, cx)` so scrolling and the diff preview rules are unchanged.
- This is the neo-tree/nvim-tree gg/G, and the same keys as the tree (fastedit.toml:854-860).

## Tests

New file git_panel_vim_nav_tests.rs (declared in lib.rs with the other test modules), using simulate_keystrokes as git_panel_tests.rs does.

Lend note (helium lead): spec, scope and criteria are the Mac's full JSON (y57), not truncated.
