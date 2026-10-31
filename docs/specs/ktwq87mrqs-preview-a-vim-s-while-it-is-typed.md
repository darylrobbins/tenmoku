---
task_uid: "01m5qpj03tpq1rxgktwq87mrqs"
task: "T238"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Integration test substitute_preview_matches_the_real_substitute (top level, crates/fe-vim/tests/substitute_preview.rs): for :s, :%s, :'\u003c,'\u003es, .,+3s, :\u0026\u0026 and the g/i/I flags and the replacement specials (\u0026 \\1 \\u ~ \\n), applying the preview's replacements to a clone of the text equals the text after running the real :s"
    required: true
    manual: false
  - id: "C2"
    text: "substitute_preview_has_no_side_effects: text, registers, the last substitute and search, history, marks and the jump list are equal before and after a preview; a half-typed command, an empty pattern, c and n flags and an invalid pattern give None. (The time-limit case moved to T256 per the user, 10-29.)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test vim::substitute_preview_tests::preview_follows_typing_and_restores: while :%s/a/b/ is typed the matches are highlighted and the rows' AfterLine text updates per keystroke; Esc leaves the buffer, decorations and view as before; Enter runs the real :s with no stale decoration"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test vim_line::substitute_preview_tests::colon_lines_reach_the_editor: the line forwards : text after each edit and not for non-substitute commands, and vim.inccommand = false (Integration test vim_inccommand_setting, crates/fe-config/tests/vim_inccommand_setting.rs: default true, schema golden) shows nothing"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-vim -p fe-gpui -p fe-workbench -p fe-config and clippy pass; the existing incsearch tests pass unchanged"
    required: true
    manual: false
---

# Preview a vim :s while it is typed (inccommand nosplit): highlight every match and show each affected row as it would read, restored on Esc (Mac T1168)

## Problem

The user asked on 2026-10-24 for the `:s` live preview, Neovim's 'inccommand' (default `nosplit`): while typing `:%s/old/new/g`, the matches are highlighted and the replacements shown, and Esc leaves the buffer untouched. Neovim users (and LazyVim, which relies on it) expect to see what a substitute will do before pressing Enter; a typo in the pattern shows at once.

## Current state

- `/` and `?` already preview ('incsearch'): the workbench's line calls `EditorView::preview_search` after each edit (`crates/fe-gpui/src/vim.rs:1292`), which shows the match through `SearchPreview` (:167), the line's pattern's matches as a replacement highlight, opens folds, reveals the match, and puts everything back on cancel (`undo_search_preview`, :1432). fe-vim answers from `VimState::search_preview` / `search_preview_pattern` (`crates/fe-vim/src/cmdline.rs:107`), under `SEARCH_PREVIEW_LIMIT`.
- The `:` line is skipped: `if *prefix == ':' { return; }` in `crates/fe-workbench/src/vim_line.rs:287-291`.
- `:s` edits in one transaction (`crates/fe-vim/src/substitute.rs:397`, `run`), so previewing by running it would change registers, the last substitute, history and marks. Decorations offer `Highlight` and `AfterLine(text)` (`crates/fe-editor/src/decorations.rs:53-78`), which an editor already paints for host providers (yank flash, `crates/fe-gpui/src/vim.rs:160`).

## Change

- fe-vim: `VimState::substitute_preview(&self, model, registers, line) -> Option<SubstitutePreview { matches: Vec<(Range<usize>, String)>, truncated: bool }>`: for an open `:` line that parses as `:s`/`:&`/`:&&` with a range (`:%s`, `:'<,'>s`, `.,+3s`), the first (up to 500) matches and what each becomes with the replacement's specials (`&`, `\1`, `\u`, `~`, `\n`) expanded, the `g`, `i`, `I` flags and the range honoured; it changes nothing (no buffer, registers, last substitute, history, marks or jump), under the same time limit as the search preview, `None` for a half-typed or invalid command, empty pattern, `c` and `n` flags. Factor the matching/expansion out of `run` rather than copying it.
- fe-gpui: `EditorView::preview_substitute` (new file `crates/fe-gpui/src/vim/substitute_preview.rs`) paints a `Highlight` over every match and, after each affected row's end, an `AfterLine` showing that row as it would read after the substitute (`→ …`, cut at 120 chars); a match in view is revealed as a search preview reveals; cancel and submit remove it all. `vim_line.rs` calls it for `:` lines; the preview is skipped for lines that are not a substitute and while `inccommand` is off.
- Setting `vim.inccommand` (bool, default true; false: no preview), like `vim.incsearch`.
- Not here: true in-place replacement text (nvim draws the replacement over the match; fastedit has no inline-replace decoration kind: a follow-up if wanted), the split window (`inccommand=split`), `:g`, `:sort`, `:normal` previews.

## Tests

New `crates/fe-vim/tests/substitute_preview.rs`: matches and expansions for the forms above against the real `:s` (a differential check: applying the preview's replacements to the text equals running `:s` on a clone), flags, ranges, multi-row patterns, no side effects (text, registers, `&`, history, marks equal before and after), invalid/partial lines, the cap and the time limit. New `crates/fe-gpui/src/vim/substitute_preview_tests.rs` (declared in vim.rs): appears while typing, updates per keystroke, Esc restores, Enter leaves the real edit, setting off. A `crates/fe-workbench/src/vim_line/substitute_preview_tests.rs` case (declared at the end of vim_line.rs) that the line forwards `:` text.

## Implementation notes

- `\n` in a *replacement* is refused by the real `:s` (`NulInReplacement`, a NUL in Vim), so the preview gives `None` for it too and the differential test checks both refuse. The C1 `\n` cases are patterns over rows (`%s/o\n/_/g`, `%s/\n//g`, `:s/\n//`'s join), and a line break put in by a replacement is `\r`.
- Half-typed means the command name alone (`:%s`, `:%subst`, which would repeat the last substitute) or a pattern with no closing delimiter yet (`:%s/a`): nothing shows until the second delimiter, unlike nvim, which highlights the bare pattern's matches. `:&`, `:&&` and `:~` preview.
- The range is read by the same code that runs it: `ex.rs`'s `lines`/`resolve` now take a `Lines` trait, implemented by `VimTarget` and by `Peek`, which reads marks and the cursor from the editor and sends an address pattern's side effects (the last search) to a copy-on-write copy of the registers, so `:/pat/,$s//x/`-style lines read as they would run. `:s`'s pattern/replacement/flags resolution (`flags_of`, `resolve`) and its matching (`find`, `find_across`, now over a text and a deadline, with an optional per-match collector) are shared with `run`.
- Past the time limit, the preview is `None` (not a partial result).
- The view's `SearchPreview` holds the substitute's decoration provider; it starts at a `:` line's first preview (not when the line opens), so `:` lines that never preview keep today's cancel behaviour. The reveal is the first match from the caret's row on, else the first. Highlights and after-row text use `Tint::Added`; rows the replacement leaves unchanged get no after-row text; a line break in a row's preview shows as `⏎`.
- The workbench forwards a `:` line when `Ex::parse` reads it as a substitute, and once more on the edit that makes it something else, so the preview comes down.
- Review round 1: a preview's replacements together stop at `SUBSTITUTE_PREVIEW_BYTES` (1 MiB, `truncated`), so a replacement that grows each match can't hold up typing; a row's after-text is read from the text's chunks only as far as its 120 characters. The decorations come down when the buffer's version moves on under them (an edit through another view; the line's next edit previews afresh) and at once when `vim.inccommand` goes off, the view put back. A match of line breaks alone highlights as an empty range at its start, which paints a space wide at the row's end.
- Review round 2: the preview's budget counts work, not only bytes: each replacement piece (a case operator, an empty group) costs a unit as each byte written does, and the deadline is checked as it's spent (`Budget`), so expansion that writes nothing is bounded too; past the deadline the preview is `None`. A `~` is measured (`tilde_len`) and refused past `SUBSTITUTE_PREVIEW_BYTES` before it is expanded or compiled. A buffer change takes the whole preview down (decorations, view, folds), as a line that previews nothing does. A run of rows counts as changed only if the rows read with the replacements differ from the rows as they are, compared as streams of the text's chunks.
