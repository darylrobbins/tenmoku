---
task_uid: "01m725cm8s17jf8kewhefnpbk1"
task: "T560"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
  - id: "C2"
    text: "The setup wizard's accessibility labels are keys, with en-US text byte-equal to before; cargo nextest run -p fe-workbench passes. The setup wizard goldens are macOS-only and can't be checked on helium: the Mac checks them when it lands the batch."
    required: true
    manual: false
---

# fe-workbench setup wizard, start page, shortcuts editor and small pickers strings (i18n W4, rest) (Mac T1559)

# fe-workbench setup wizard, start page, shortcuts and small pickers (W4, rest)

Spec: card **W4** of `docs/decisions/xte5e7hqsw-i18n-design.md` § 9, for the fe-workbench files the B3 surface
cards don't restyle (they move their own files' text). Sections 3–8 are the
design. en-US stays byte-equal to today's, so no golden and no behaviour test
changes except tests that construct removed literals.

Files: `setup_wizard.rs` (including its accessibility labels), `setup_wizard_model.rs`, `setup_wizard_preview.rs`, `setup_launch.rs`, `start_page.rs`, `start_page_model.rs`, `shortcuts_editor.rs`, `shortcuts_recorder.rs`, `encoding_picker.rs`, `line_ending_picker.rs`, `language_picker.rs`, `theme_picker.rs`, `config_problems.rs`.

Add `crates/fe-workbench/l10n/en-US/setup.ftl` (keys `workbench-setup-…`) to
`resources!` in `crates/fe-workbench/src/l10n.rs`, and these files to
`l10n-allow.toml`'s `migrated` list. English plural helpers become plural
messages; fragments become whole messages.

The start page and the setup wizard are not restyled (Welcome and empty window
is still to design), so their text moves as it is and the setup wizard goldens
do not change.

Platform: **Linux-OK**.

## Implementation notes

- **Keys.** Every key in `setup.ftl` starts `workbench-setup-`, as the card
  says, with a sub-area after it: `-start-` (the Start page), `-shortcuts-`,
  `-config-`, `-encoding-`, `-theme-`; the wizard's own have none.
  `workbench-setup-current` is the one `current` mark of the encoding, line
  ending and language pickers.
- **Stored text.** The Start page's rows are built off the main thread and
  kept, so `Row::label` is a `kiln_l10n::Text` (a key, or a recent entry's
  name verbatim) resolved as the row draws; the underlined dashboard letter
  is found in the label as drawn (`Row::key_in`), so a translation without
  the letter underlines nothing and the key still works. The wizard's
  controls, the pickers' rows, the shortcuts editor's notes and the config
  problems list are rebuilt on every render, so they take `tr!` output. The
  recorder's conflict preview is kept while recording, so it is a `Text`.
- **The config problems status item** is set when the diagnostics change, not
  per frame, so `follow_config_problems` also follows `ActiveLocale` and
  sets it again (`Subscription::join`). Its tooltip is the command's own
  title, `fe_command::l10n::command_label`.
- **Fragments made whole.** The font size stepper's VoiceOver labels were
  `"Decrease"`/`"Increase"` plus the control's title lowercased; they are
  whole messages now (`workbench-setup-font-size-decrease`, `-increase`), as
  a lowercased translated title is wrong in many languages. The Ready page's
  "Change any of them in Settings (keys)", each hint with its keys, the
  shortcuts editor's "Also bound to …; Starts the keys of …" (three messages:
  either list, or both) and the config problems detail are whole messages;
  command lists are passed pre-joined with `, `, as § 5 allows.
- **Counts** are plural messages: the Ready page's "You changed N settings"
  (with a `[0]` variant for "Tenmoku's defaults are set"), "N config
  problems", "N problems", and the size in "bigger than N KiB".
- **No more matching on English.** `label_family` read the default font's
  family back out of its "Default (…)" label; a `ControlOption` now carries
  the family it's drawn in, and `label_family` takes the option. The
  encoding picker's labels come from `EncodingChoice::label()`, by the
  encoding's WHATWG label, not from a table of English names.
- **Tests changed** only where they named removed literals or types:
  `NOTHING_FOUND`, `RESET_REFUSED_HINT` and `PROMPT` are functions now
  (`nothing_found()`, `reset_refused_hint()`, `prompt()`), and the tests
  assert their en-US text; `Row::label` and `EncodingChoice::label` are read
  resolved; `label_family` is checked on the Font page's options. The
  setup wizard's existing `labels_and_roles` and `ready_counts_and_shortcuts`
  tests, which pin its accessibility labels in English, pass unchanged.
- **Ids** built with `format!` (element ids and debug selectors) are
  allow-listed in `l10n-allow.toml`; plain id literals passed to
  `Button::new` are `verbatim!`, as is the wizard's Rust code sample.
- **Known limit.** kiln-ui's `TextInput` keeps its placeholder resolved and
  has no setter, so the shortcuts editor's search placeholder (and an open
  recorder's When placeholder) keep the language they were made in until
  the tab is reopened. Fixing it needs a `TextInput::set_placeholder` in
  kiln-ui, outside this card.
- **Carried T475 F1** (keyboard selection never scrolls into view) was fixed
  before this card by c8592543 (Mac T1425): the selected row is revealed
  after up/down and after a reload's selection reset, tested by
  `tests/start_page_selection_and_recents.rs`
  (`down_scrolls_the_selected_row_into_view`,
  `up_scrolls_back_to_the_first_row`). This card leaves that code as it is.
- **Scope.** The card's tests (the `*_tests.rs` of its files, `tests/l10n.rs`
  and `tests/start_page_selection_and_recents.rs`), `l10n.rs` and
  `l10n-allow.toml` were respecced in. The last two were held by T559 (W3)
  and then T557, so they joined after both landed; their lists keep those
  cards' lines (`navigation.ftl`, `find.ftl`) ahead of this card's.
