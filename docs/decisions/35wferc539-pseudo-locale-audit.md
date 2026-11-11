# T1720: pseudo-locale audit capture

`cargo run -p fe-workbench --example screenshots -- audit --pseudo <out-dir>` writes every audit
scene (51 captures: each surface dark and light, plus every bundled theme) in `en-XA`. Without
`--pseudo` the audit is unchanged (same 51 captures, en-US).

## How it selects en-XA

`main.rs`'s launcher sets `TENMOKU_PSEUDO_LOCALE=1` in the isolated child when the arguments are
`audit … --pseudo`; `audit_scenes::main` then calls `locale::set_locale(cx, &locale::preferred())`.
`preferred()` is where the variable puts `en-XA` first, so this is the one selection from T1564, not
a second path. The example never installed a locale before (only the `fastedit` binary does), so
the variable alone changed nothing.

The 20 token-probe failures the audit prints are the same with and without `--pseudo`: they predate
this card.

## Run on the Mac, 2026-11-11: plain English still showing (findings)

Reviewed the dark capture of each surface; light captures share the same strings. Each is a string
the catalogue missed. Fixing them is not this card; a follow-up is filed (tsk desk card "Catalogue the strings the pseudo-locale audit found in English (from T1720)").

- **Command palette** (`command_palette_*`): the first entry's label "Hide Tenmoku" (`app.hide`).
- **Keyboard shortcuts** (`shortcuts_editor_*`): the "Hide Tenmoku" label; the raw command ids
  (`picker.confirm`, `proposal.accept_all`, …) in the Command column and the context expressions in
  the When column (`editor && proposal`, `text_input`, `(settings editor || find_bar)`). The ids and
  expressions may be meant to stay raw; the label may not.
- **Search panel** (`search_panel_*`): the "Search" panel title, and the "5 results in 1 file"
  summary. The find-in-files query box shows no placeholder.
- **File finder** (`file_finder_*`): the input placeholder "Go to file…".
- **Git log hover card** (`git_log_card_*`): the "Co-authored-by:" trailer label (commit-message
  text may be data), and the date "(2026-11-01 10:00 +0000)" in a fixed English format.
- **Toast** (`toast_*`): the whole toast, "Couldn't save src/main.rs" and "Permission denied (os
  error 13)".
- **Trust prompt** (`prompt_*`): the title "Do you trust the authors of the files in this
  folder?" and the whole body paragraph, including "File > Trust This Folder or Untrust This
  Folder".
- **Start page** (`start_page_*`): the "Tenmoku" heading (product name, probably intended).
- **Title bar** (every scene): "cargo build" run-configuration label, and the branch chip
  "main*+" (data).
- **Status bar** (every scene): "UTF-8", "LF", "Rust" (encoding, line ending and language names).
- **Terminal**: shell output is program output, not UI text.
- **Hover** (`hover_*`): the rust-analyzer text is server data.

Scenes with no English UI text found: `chrome`, `settings` (all of the strings are en-XA; the
`workbench.rail` key names, the `left` value and the `13.0` number are data), `problems`,
`problems_panel`, `changes`, `find_bar`, `project_tree_git`, `git_log`.

Also reviewed (dark captures): `terminal_session` (tab titles are en-XA; the terminal body is program
output), `problems` (tabs, scope switch and gutter are en-XA; the diagnostic messages "variable does
not need to be mutable", "mismatched types…", "\"small\" is a weasel word" and their sources are
language-server/linter data, not application text), and `yohen`, which stands for the bundled-theme
captures: they render the same workbench, so they show the same en-XA chrome and the same status-bar
and title-bar English listed above. Light captures use the same strings as their dark pair.
