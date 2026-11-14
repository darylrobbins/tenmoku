---
task_uid: "01m79daatcapm8g3k2g3x3eje7"
task: "T342"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "modeline_languages::vim_set_and_bare_forms, vim_versioned_and_ex_prefixes, vim_only_first_and_last_five_lines, vim_ignores_other_options_and_takes_the_last_ft, emacs_mode_and_lone_token_forms, emacs_second_line_after_shebang, tenmoku_form, name_mapping_table, unknown_name_falls_through_to_path, modeline_beats_path_and_shebang pass"
    required: true
    manual: false
  - id: "C2"
    text: "open_uses_a_modeline_in_the_last_lines and hand_choice_beats_modeline (crates/fe-gpui/tests/modeline_detect.rs) pass"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-syntax -p fe-gpui passes"
    required: true
    manual: false
---

# Language modelines: Vim (ft=, filetype=, syntax=), Emacs (-*- mode: -*-) and 'tenmoku: language=' pick a file's language, beating its path (Mac T1776)

# Language modelines: Vim (ft=, filetype=, syntax=), Emacs (-*- mode: -*-) and `tenmoku: language=` pick a file's language

Detection today (detect.rs `detect_language_name(path, head)`): path, then shebang, then VS Code-style first-line patterns (the only Emacs form is `# -*- mode: shell-script -*-`). Callers pass the first 512 bytes only (fe-gpui view.rs DETECT_BYTES; pane_item.rs `detect_language` runs on open, after save-as and detect_language_from; diff_view.rs and commit_diff.rs also call it). A hand choice (`set_language`, `language_chosen`) already beats detection.

## Change
- New crates/fe-syntax/src/modeline.rs: `pub fn language_name_for_modeline(head: &str, tail: &str) -> Option<&'static str>`. Parse like the editors, first match wins, scanning head's first 5 lines then tail's last 5 (Vim's default `modelines=5`):
  - Vim: a line containing `vi:`, `vim:`, `Vim:`, `ex:` (also `vim700:`, `vim>600:` etc.) preceded by start-of-line or whitespace; options either `set ft=xml:` (up to the first unescaped `:`, `set`/`se` form) or the rest of the line split on spaces/colons; keys `ft`, `filetype`, `syntax`, `syn` with `=value`; the last such key on the line wins.
  - Emacs: first line, or the second if the first is a `#!` line, containing `-*- ... -*-`: `mode: xml`, `Mode: xml` (case-insensitive key, `;`-separated pairs) or a lone token `-*- xml -*-`. Ignore `coding:` etc. The end-of-file `Local Variables:` block is out of scope.
  - Tenmoku's own (editor-agnostic, any comment leader): `tenmoku: language=xml` (also `filetype=`/`ft=`), in the same first/last 5 lines. GitHub Linguist honours Vim and Emacs modelines too, so those are the compatible forms; ours is only a fallback for a language neither names.
  - Name mapping as data in modeline.rs: Vim ft and Emacs mode names to our names, case-insensitive: sh/bash/zsh/ksh/shell-script/sh-mode -> bash, c++/cpp -> cpp, cs/csharp -> csharp, js/javascript/javascriptreact/jsx -> javascript, ts/typescript -> typescript, typescriptreact -> tsx, py/python -> python, rb/ruby, md/markdown, yml/yaml, make/makefile, dockerfile, nxml/xml/xsd/xslt/svg -> xml, json/jsonc, toml, ini/dosini/conf -> ini, gitcommit, diff; anything else falls through to the same name/extension/fence-alias resolver `language::injected`-style lookup (language.rs ~line 437) so `rust`, `go`, `lua` just work. Unknown or uncompiled names return None (registered grammars are found by name too).
- Precedence: explicit user choice (unchanged) > modeline > path > shebang > first-line > content sniff. A modeline is an explicit statement in the file, as in Vim; `detect_language_name` gains `detect_language(path, head, tail)` and the old signature stays as `tail = ""`.
- Wiring: pane_item.rs `detect_language` also reads the last 1 KiB (clip to a line start) and passes it; diff_view.rs/commit_diff.rs keep the head-only call. Same triggers as today (open, save-as, detect_language_from): no re-detect while typing; a changed modeline takes effect on reopen or the next detect.
- Optional, NOT in this card: .gitattributes `linguist-language=`; Emacs Local Variables block.
## Scope
new crates/fe-syntax/src/modeline.rs, crates/fe-syntax/src/detect.rs, crates/fe-syntax/src/lib.rs, crates/fe-gpui/src/pane_item.rs, crates/fe-gpui/src/view.rs (a tail-bytes const), new tests crates/fe-syntax/tests/modeline_languages.rs and crates/fe-gpui/tests/modeline_detect.rs (or the nearest tests dir declared from its mod.rs).
## Criteria test ids
modeline_languages::vim_set_and_bare_forms, vim_versioned_and_ex_prefixes, vim_only_first_and_last_five_lines, vim_ignores_other_options_and_takes_the_last_ft, emacs_mode_and_lone_token_forms, emacs_second_line_after_shebang, tenmoku_form, name_mapping_table, unknown_name_falls_through_to_path, modeline_beats_path_and_shebang; modeline_detect::open_uses_a_modeline_in_the_last_lines, hand_choice_beats_modeline; `cargo nextest run -p fe-syntax -p fe-gpui` passes.
Conflicts: both fe-syntax detect cards touch `detect_language_name`: depend on the extension/sniff card. pane_item.rs: no ready card touches detect_language (T1764 is the editorconfig dirty logic, same file, different fn: rebase on finish).
Platform: linux-ok.

## Implementation notes

- Order inside `language_name_for_modeline`: the Emacs `-*-` line (first, or second after `#!`), then Vim modelines over head's first five and tail's last five lines, then `tenmoku:` over the same lines. A line whose name doesn't resolve is skipped, so `tenmoku:` stands in for a Vim/Emacs name we don't know.
- Vim, as `chk_modeline`: `ex:` not at a line's start; `Vim:` only with `set`; the `se[t] ...:` form needs its closing unescaped colon; dotted filetypes (`cpp.doxygen`) give their first part. Version prefixes are all accepted, whatever their number.
- Emacs trims spaces and tabs only, and a name containing whitespace resolves to nothing (`injected_language` trims, so this is checked first). `first_line_languages::near_misses_stay_undetected` had `#\u{85}-*- mode: shell-script -*-` as a near miss of VS Code's pattern; it is a real Emacs modeline, so it now detects as bash and moved to a positive assertion in that test.
- `tenmoku:` matches in any ASCII case, after anything but a letter, digit or `_` (so `--tenmoku:` and `<!--tenmoku:` work).
- pane_item `detect_language` reads the last `DETECT_TAIL_BYTES` (1024) from the first line start in them; if they're all one line, the tail is empty.
