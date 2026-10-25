---
task_uid: "01m5qphz2f172y8jm13c2rm995"
task: "T226"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Integration test keymap_continuations_group_by_next_key (top level, kiln/crates/kiln-core/tests/keymap_continuations.rs): bindings longer than the held keystrokes whose context holds are listed once per next key with the command and a longer flag; a binding whose context fails is absent; the last binding for a key wins"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test vim::which_key_chord_tests::bracket_and_g_chords_list_their_keys: with the default keymap in an editor in normal mode, ] held then the delay shows d (diagnostics.next title); g shows b with the built-in g keys of T1134 merged; neither shows if the second key comes first"
    required: true
    manual: false
  - id: "C3"
    text: "vim::which_key_chord_tests::ctrl_w_lists_window_commands: ctrl-w held shows the pane focus, split and close keys with their command titles"
    required: true
    manual: false
  - id: "C4"
    text: "vim::which_key_chord_tests::chord_end_closes_the_popup: a completed chord, a failed one, CHORD_TIMEOUT, Esc and blur each close it, and the keys the matcher replays still reach vim unchanged"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p kiln-core -p kiln-platform -p fe-gpui and clippy for the three crates (kiln/ is its own workspace: run it there too) pass"
    required: true
    manual: false
---

# Which-key popup for prefixes the keymap matcher holds (g, [, ], ctrl-w): list the chord's bindings with their command titles (Mac T1141)

## Problem

The user asked on 2026-10-24 for the which-key popup after any prefix with continuations. In fastedit `g`, `[`, `]` and `<C-w>` are not held by vim at all: the keymap binds chords (`g b`, `] d`, `[ d` in `crates/fe-keymap/keymaps/fastedit.toml`, the `ctrl-w` window keys), so kiln's matcher swallows the first key until the second arrives or `CHORD_TIMEOUT` ends it. The popup of T1137 sees only what vim and the remap engine hold, so those prefixes (the commonest in vim) would show nothing.

## Current state

- `Matcher::pending()` (`kiln/crates/kiln-core/src/matcher.rs:62`) returns the held keystrokes; `kiln_platform::chord_pending` (`kiln/crates/kiln-platform/src/keys.rs:291`) says only whether one is held. `Keymap`/`Binding` are in `kiln/crates/kiln-core/src/keymap.rs:77,125`.
- The held chord belongs to the window and focused element it started in (keys.rs header); an interceptor registered later runs after the keymap (gpui-focus-key-ordering).
- Titles: the command catalog gives a `CommandMeta` title per command id.

## Change

- `kiln-core` keymap: `Keymap::continuations(&self, held: &[Keystroke], ctx: &ContextStack) -> Vec<Continuation { next: Keystroke, command: Option<String>, longer: bool }>`: the bindings whose context holds and whose keys start with `held` and are longer, grouped by next key, a later binding winning as in matching.
- `kiln-platform` keys.rs: `pending_continuations(cx) -> Option<PendingChord { held: Vec<Keystroke>, next: Vec<Continuation> }>` for the focused element, `None` with no chord held.
- fe-gpui `which_key.rs` (T1137's file): when a chord is held, arm the same delay timer; fire shows the popup with the chord's continuations merged with vim's (`g` shows both keymap chords such as `b` and the reducer's own `g`, `e`, `j`...: the reducer table of T1134 for the same prefix, which `which_key_for(&[VimKey])` answers), titled from the catalog. A key that ends the chord (resolved, failed, timed out, Esc, blur) closes it. The popup leaves the matcher alone.
- Scope: the vim popup only; chords outside an editor (cmd-k, cmd-k cmd-s) stay unshown (note it in the body of the tests).

## Tests

New `kiln/crates/kiln-core/tests/keymap_continuations.rs` (contexts, grouping, longer-binding flag, last-wins); new `crates/fe-gpui/src/vim/which_key_chord_tests.rs` registered in `vim.rs`: `]` held shows d, `[` shows d, `g` shows `b` merged with built-in g keys, `ctrl-w` lists the pane commands with titles, a fast `] d` never shows it, the chord timeout closes it.

## Implementation notes

- `Keymap::continuations` returns `Continuation { next, command: Option<CommandId>, longer }` (a typed id rather than a `String`), in the order of each next key's first binding; the command is the exact binding's by the matcher's precedence (deepest context match, then the later binding).
- `kiln_platform::keys::pending_continuations(window, cx)` takes the window too, to check the chord is held for that window's focused element (a chord belongs to the element it started in). `PendingChord` also carries `stack`, the context the chord's last keystroke was matched in, so the popup can count a group's keys; the internal chord state is renamed `HeldChord`.
- The editor learns of chord changes through a new `KeyOwner::chord_changed(window, cx)` hook (default: nothing), called after every keystroke the keymap sees, after a chord's timeout, and when focus leaving its element abandons it. fe-gpui's `VimKeys` owner (`crates/fe-gpui/src/keymap.rs`, a 5-line scope expansion) forwards it to `which_key::chord_changed`. An interceptor registered after the keymap's would see keystrokes but not the timeout or the abandon.
- After a chord times out or fails, its keys replay to vim; a prefix they leave pending there (`g`) waits the delay anew for vim's own popup, without the keymap's keys, rather than taking over the chord popup at once.
- A key going on from the chord that only starts longer bindings (`g r`) shows as a group, `+N` with N its next keys.
- A keymap hot reload that drops a held chord tells the owner too, in the chord's window, unless the reload runs inside that window's own update; then a popup shown for it stays until the next key.
- Carried T225 F6: the panel stops wheel events after its keys scroll, so the editor under it no longer scrolls (`which_key_chord_tests::the_wheel_scrolls_the_popup_not_the_text`; without the fix the document scrolled 2100px).
