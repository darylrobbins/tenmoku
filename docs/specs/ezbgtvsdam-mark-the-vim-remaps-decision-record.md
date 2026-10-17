---
task_uid: "01m55marck5xmk25ezbgtvsdam"
task: "T184"
rev: 1
type: "docs"
complexity: "xs"
criteria:
  - id: "C1"
    text: "docs/decisions/vim-remaps.md's status line says decided by the user on 2026-10-17, and each of the six questions has a 'Decided by the user, 2026-10-17: …' line giving the answer in the spec"
    required: true
    manual: false
  - id: "C2"
    text: "§1(a) and the Recommendation put the table in settings.toml's [vim] table ([[vim.remap]], vim.leader) with the reason; §5's macro-runner sentence matches main (replay_key types into an unseen line and drops its host command); CHORD_TIMEOUT is named as kiln_platform::keys::CHORD_TIMEOUT"
    required: true
    manual: false
  - id: "C3"
    text: "Only docs/decisions/vim-remaps.md changes, and no design decision in it other than the table's location changes"
    required: true
    manual: false
---

# Mark the vim remaps decision record decided, with the user's six answers of 2026-10-17 and the settings.toml table (Mac T867)

## Problem

`docs/decisions/vim-remaps.md` (T62, Mac T425) still says "Status: **proposed, waiting on
the user's answers**". T425 was abandoned after the record landed. The user answered its six
questions on 2026-10-17, each as the record recommends:

1. Both: the TOML table first, then the vimrc subset.
2. Recursive maps (`nmap`, `map`) supported, depth limit 100.
3. `<leader>` defaults to backslash `\`.
4. The first cut covers normal, visual and operator-pending modes only. Insert and
   cmdline come later.
5. Yes: `<Cmd>fe:<command id><CR>` right sides run fastedit commands, in a card after the
   core engine.
6. `CHORD_TIMEOUT` serves as 'timeoutlen'.

The record has also gone stale against main in three places:

- §1(a) puts the table "in the keymap TOML". The keymap file is kiln's (`kiln_core`'s
  loader, through `kiln::Config::keymap`), and vim's settings live in fe-config's
  `VimSettings` (`crates/fe-config/src/settings.rs`), the `[vim]` table of `settings.toml`.
- §5's "GPUI's macro runner stops when a key enters `cmdline`" is no longer true.
  `EditorView::replay_key` (`crates/fe-gpui/src/vim.rs`) now types a replay's keys into
  a line it doesn't show. It drops the host command (`:w`) that such a line leaves, with
  "a macro replay doesn't run …".
- `CHORD_TIMEOUT` is now `kiln_platform::keys::CHORD_TIMEOUT` (1 s), which
  `fe-gpui/src/keymap.rs` re-exports.

## Change

Edit only `docs/decisions/vim-remaps.md`:

- Status line: `Status: **decided** by the user, 2026-10-17 (see "Questions for the
  user"). Source card T62 (Mac T425).` Keep the clean-room sentence.
- Under each of the six questions, add one line: `Decided by the user, 2026-10-17: …`,
  giving the answer above.
- §1(a) and the Recommendation: the table is `[[vim.remap]]` (with `vim.leader`) in
  `settings.toml`'s `[vim]` table, read by fe-config's `VimSettings`, not the keymap file.
  Give the one-line reason: the keymap file is kiln's, and vim's settings are already in
  `VimSettings`.
- §5: replace the macro-runner sentence with what main does now (see above). Name
  `kiln_platform::keys::CHORD_TIMEOUT` where the record says `CHORD_TIMEOUT`.
- "Implementation cards (outline)": add one line saying the cards were filed on
  2026-10-17 under the label `vim-remaps`, and that card 8 (insert and cmdline) waits,
  as the user scoped the first cut to n/x/o.

Change nothing else in the record's design.
