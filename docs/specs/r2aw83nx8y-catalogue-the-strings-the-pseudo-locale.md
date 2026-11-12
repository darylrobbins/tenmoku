---
task_uid: "01m76fnhfzpqbc5xr2aw83nx8y"
task: "T588"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Under en-XA the palette and shortcuts-editor title of app.hide (and app.hide_others, app.show_all, app.quit) comes from the catalogue (kiln.ftl command-app-hide etc.), not the English registry fallback; en-US stays byte-equal ('Hide Tenmoku'); proven by the headless test in crates/fe-workbench/tests/app_command_titles_pseudo.rs (cargo nextest run -p fe-workbench --test app_command_titles_pseudo)"
    required: true
    manual: false
  - id: "C2"
    text: "The status bar's LF and CRLF in crates/fe-gpui/src/pane_item.rs go through kiln_l10n::verbatim!; cargo nextest run -p fe-gpui passes (pane_item_tests still see UTF-8, LF)"
    required: true
    manual: false
  - id: "C3"
    text: "docs/decisions/35wferc539-pseudo-locale-audit.md gains a Follow-up section naming, per T1720 finding, its owner card (T1546, T1556, T1560, T1563) or its verbatim/data verdict with the code evidence"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-workbench --test l10n and cargo nextest run -p kiln-platform pass"
    required: true
    manual: false
---

# Catalogue the strings the pseudo-locale audit found in English: app.hide title, status-bar line endings, per-string verdicts (from T1720) (Mac T1726)

# Catalogue the strings the pseudo-locale audit found in English (from T1720)

Findings: `docs/decisions/35wferc539-pseudo-locale-audit.md` ("Run on the Mac" list). Most of that list is already
owned by a card; this one takes what is left and records the verdict per string.

## Already owned (do not touch here)

- Search panel title and "5 results in 1 file": **T1546** (Search panel in B3) migrates `search_panel.rs` and
  `search_replace.rs` to `search.ftl`.
- Toast "Couldn't save src/main.rs" / "Permission denied (os error 13)": `toast.rs` and the audit fixture
  `examples/screenshots/audit/toast.rs` (the English is that fixture's own literal) are in **T1556**'s scope;
  the real save-failure toasts come from `save_commands.rs` (**T1563**).
- Trust prompt title and body (`crates/fe-workbench/src/trust.rs`): **T1560**.
- File finder placeholder "Go to file…" (`crates/fe-workbench/src/workbench_palettes.rs`, `Picker::new`): **T1563**.

## Change

1. **"Hide Tenmoku"** (`app.hide`) shows in English in the command palette and the shortcuts editor under en-XA.
   `kiln/crates/kiln/l10n/en-US/kiln.ftl` has `command-app-hide = Hide { $app }`, but the title shown is the English
   fallback `format!("Hide {app_name}")` registered by `kiln_platform::commands::app_commands`
   (`kiln/crates/kiln-platform/src/commands.rs`). Find why the label does not resolve through
   `fe_command::l10n::command_label` (suspect: `kiln_command::set_label_arg("app", ...)` is only called from
   kiln's own setup, not on fastedit's path through `crates/fe-gpui/src/commands.rs`; or kiln.ftl is not in the
   resources the palette resolves against) and fix it so the palette and shortcuts editor take the title,
   and `app.hide_others`, `app.show_all`, `app.quit` and the `window.*` titles, from the catalogue.
2. **Status bar "LF" / "CRLF"** (`crates/fe-gpui/src/pane_item.rs`, the `LineEnding` match): technical names, kept
   verbatim in every locale; wrap them in `kiln_l10n::verbatim!` so the choice is explicit like the language
   names. **"UTF-8"** is `Encoding::label()` from fe-text (a WHATWG name, verbatim; no change). **"Rust"** already
   goes through `status::language_label`'s `verbatim!` table (no change).
3. **Kept as is, no change** (record each in the decision file with its reason): the git log card's
   "Co-authored-by" is a git trailer key, already `kiln_l10n::verbatim!` in `crates/fe-workbench/src/git_log.rs`;
   the card date "2026-11-01 10:00 +0000" is `git_blame::date`, a numeric ISO-like stamp already in
   `crates/fe-workbench/l10n-allow.toml` ("not words"); the shortcuts editor's command ids and When expressions are
   keymap syntax; "Tenmoku", the branch chip, the "cargo build" run label, terminal output and language-server
   text are names or program data.

## Tests

New file `crates/fe-workbench/tests/app_command_titles_pseudo.rs` (headless; a fake or `en-XA` locale as
`tests/command_labels_fake_locale.rs` does): under the pseudo-locale `app.hide`'s palette title differs from
the English and contains the app name; en-US stays "Hide Tenmoku". Add the decision-file update
(`docs/decisions/35wferc539-pseudo-locale-audit.md`, a "Follow-up" section listing owner or verdict per finding).

Platform: **Linux-OK** (the Mac audit re-run is optional confirmation, not a criterion).
