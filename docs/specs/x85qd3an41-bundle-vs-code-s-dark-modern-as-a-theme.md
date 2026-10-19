---
task_uid: "01m57yaecyncyp8px85qd3an41"
task: "T200"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-config test vscode_dark_modern_theme::parses_cleanly: the file parses with no diagnostics under EXTENSIONS and EditorPalette::for_theme returns no warnings"
    required: true
    manual: false
  - id: "C2"
    text: "vscode_dark_modern_theme::matches_upstream: the editor background, foreground and selection, and the colours of keyword, function, string, comment, type and module, equal Dark Modern's values"
    required: true
    manual: false
  - id: "C3"
    text: "vscode_dark_modern_theme::syntax_keys_are_known: every [syntax] key is a name Highlight::from_capture accepts"
    required: true
    manual: false
  - id: "C4"
    text: "NOTICE has a Microsoft Corporation MIT entry and THIRD_PARTY.md a row with the upstream path and commit; cargo nextest run -p fastedit --test notice --test third_party passes"
    required: true
    manual: false
---

# Bundle VS Code's Dark Modern as a theme on fastedit's current colour tables (Mac T994)

# Bundle VS Code's Dark Modern as a theme, on fastedit's current colour tables

## Problem

The user approved (2026-10-18, "fastedit: VS Code Parity Notes", section Requested changes > Theme) shipping a VS Code
Dark Modern-equivalent theme as an option. fastedit bundles three themes in `crates/fe-config/themes/`
(`fastedit-dark.toml`, `fastedit-light.toml`, `fastedit-ember.toml`); each is a kiln theme file that carries
fastedit's `[editor]` colours and a `[syntax]` table of colours by dotted highlight name (`EXTENSIONS` in
`crates/fe-config/src/theme.rs:32`, read by `EditorPalette::for_theme` and `syntax_color`). Ember is reachable only
as a user's `theme.toml`; the `[theme] dark = "<id>"` setting names installed themes (extension record T839), not
bundled ones.

This card is the colours VS Code's Dark Modern gives editor and tokens, on the model that exists (one colour per
highlight name). Semantic-token rules (`*.mutable` underline), style roles, bracket-pair, inlay-hint and
deprecated styling need the styling record's follow-ups and the keys T932/T988 add; they are a later card on this file.

Licence: VS Code's `extensions/theme-defaults/themes/dark_modern.json` (and the `dark_plus.json`/`dark_vs.json` it
includes) are in microsoft/vscode, MIT, Copyright (c) Microsoft Corporation. MIT needs the notice kept with the
copied or adapted values. `CLEAN_ROOM.md` allows permissive sources; this theme carries the colour values, not code.

## Change

1. New `crates/fe-config/themes/vscode-dark-modern.toml` (`name = "VS Code Dark Modern"`, `appearance = "dark"`): a
   header comment naming the upstream files, the commit read (pin it) and the MIT notice; `[editor]` set from the
   workbench colours (background, foreground, gutter, selection, active line, cursor, diagnostics, popover, indent
   guides, git change colours, vim mode segments chosen to sit in the palette); `[syntax]` mapping Dark Modern's token
   colours (TextMate scopes) onto the names of `fe_syntax::Highlight::all()`, longest-prefix as today. A scope with
   no highlight name is left out, and the header says which.
2. Register it beside the other bundled themes in `crates/fe-config/src/theme.rs` only if a bundled theme list
   exists by then; otherwise it is, like Ember, a file users copy to `theme.toml` and the finish notes say so.
3. `NOTICE` gets a VS Code entry (Copyright (c) Microsoft Corporation, MIT) and `THIRD_PARTY.md` a row for the file
   with the upstream path and commit, so `crates/fastedit/tests/notice.rs` and `third_party.rs` pass.
4. New test `crates/fe-config/tests/vscode_dark_modern_theme.rs`: the file parses with no diagnostics under
   `EXTENSIONS`; `EditorPalette::for_theme` returns no warnings; the background, foreground, selection, and the
   colours of `keyword`, `function`, `string`, `comment`, `type`, `module` equal Dark Modern's values; every
   `[syntax]` key is a name `Highlight::from_capture` accepts.

## Implementation notes

- Upstream pinned at microsoft/vscode `ab8f31f0c0b1a4dd86d8dbee1248542f5806574d`. No bundled theme list exists, so like Ember the file is one users copy to `theme.toml`.
- Selection, cursor, warning, information and hint are VS Code's built-in defaults; none of the three theme files sets them.
- A theme's `[syntax]` is laid over fastedit Dark's, so every name Dark lists is listed here (e.g. `function.macro`, `punctuation`) or Dark's colour would leak through.
- `fe-syntax` is a dev-dependency of fe-config for the `from_capture` check. `third_party.rs` now collects `.toml` files so the new row's file is found.
