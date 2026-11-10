---
task_uid: "01m725cggjy1v78g03ttzbww43"
task: "T557"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in tests::find_bar_b3 (cargo nextest run -p fe-workbench) pass: the find field is a bordered input whose focus ring is the accent; case, whole word and regex are toggle buttons inside it with accessible names; 'n of m' is a plural message; in selection toggles"
    required: true
    manual: false
  - id: "C2"
    text: "tests::find_bar_b3::vim_equivalent_matches_the_query passes: dirty gives /dirty; with replace needs_paint, case and regex on, :%s/dirty/needs_paint/gI (flags from the toggles), and running that command in fe-vim finds the same matches"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
---

# Find and replace row in B3: toggles inside the field, the accent focus ring, n of m, in selection, and the equivalent Vim command shown faintly (Mac T1545)

# Find and replace row in B3

Spec: `docs/design/design-system.md#editor` (Find and replace). Audit findings
FB1, FB2 (the held card P6, find half).

## Change (`crates/fe-workbench/src/find_bar.rs`)

- Stays a row under the breadcrumbs. The find field is the controls card's
  input with the accent focus ring; match case, whole word and regex are
  toggle buttons inside it (icons `case-sensitive`, `whole-word`, `regex`; on
  = accent-soft tint); then "2 of 3" (a plural message), previous, next, in
  selection (`text-select`).
- Replace row: Replace and Replace all as secondary buttons.
- The equivalent Vim command shown faintly at the right end, built from the
  query and toggles: `/dirty` for find, `:%s/dirty/needs_paint/gI` with
  replace (flags from the toggles; `\V` / very-magic as fe-vim's search
  takes it). Copyable, not editable.
- Rename symbol in place of Replace waits (it needs a symbol check per match).

## i18n

This card is the one that moves these files' text into the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5 and 10): every
user-visible string in `find_bar.rs`, the new ones and the old, goes to
`crates/fe-workbench/l10n/en-US/find.ftl` (keys `workbench-find-…`), through `tr!` / `text!`, whole messages, plural
selectors for counts; add the file to `resources!` in the crate's
`src/l10n.rs` and the files to `l10n-allow.toml`'s `migrated` list. Text the
design rewrites follows the design; text it keeps stays byte-equal. Tooltips
and accessible names count.

Platform: **Linux-OK**.

## Implementation notes

- The scope grew (respecs) by `l10n.rs` and `l10n-allow.toml` (named by the
  spec), `tests/mod.rs` (declares `find_bar_b3`), `tests/find_bar/toggle_focus.rs`
  (one test clicked the bar's right end, now the Vim command), kiln-ui's
  `toggle_button.rs` (`ToggleButton::icon_only`, so the field's toggles are
  kiln's toggles with the accent-soft tint, in the tab order, with names) and
  fe-vim's `search/replace.rs` (the Vim command, and T410 F3's
  `replacement_at` bound).
- The Vim command is `fe_vim::search::replace::vim_command`: `/` with the
  replace row hidden, `:%s/…/…/g` plus `I` or `i` with it shown. For `/`,
  `\c` or `\C` is added only where vim's own 'ignorecase' and 'smartcase'
  (the shared registers) would get the case wrong, so with LazyVim's
  settings `dirty` is `/dirty`. It's shown only with vim on, and is `None`
  (nothing shown) wherever Vim can't say the same thing exactly: Find in
  Selection; Preserve Case while replacing; a pattern that can match nothing
  (`:s` skips the empty match after a non-empty one, Replace All doesn't);
  Whole Word on a regex, or on literal text that doesn't start and end with
  a word character; regex classes and escapes whose meaning differs
  (`\d`, `\w`, `\s`, `\b` are Unicode in the `regex` crate, ASCII in Vim),
  flags, named groups, nested classes and set operations; named groups and
  groups past 9 in the replacement. fe-vim's tests run each translated
  command in `HeadlessVim` against Replace All on the same text.
- Known difference: Whole Word's `\<`/`\>` follow Vim's word classes, so
  next to CJK text they can take a match the find bar's Unicode `\w` check
  rejects.
- The placeholders are resolved when the bar is made, as a picker's are;
  the replace-all status is stored as a `Text` and resolved when drawn.
