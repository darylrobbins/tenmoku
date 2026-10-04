---
task_uid: "01m417z4eq6rmrt2fgczqqr375"
task: "T25"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-syntax: shebangs pick their interpreter's language, including env, env -S with options and versioned names like python3.12; unknown interpreters give None (test detect::tests::shebangs_pick_the_interpreters_language)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-syntax: the path wins over a shebang, and text with no shebang on its first line gives None (test detect::tests::the_path_wins_over_the_shebang)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-syntax builds warning-free with no language features and with each one alone (stage syntax_features)"
    required: true
    manual: false
---

# Detect a buffer's language from its shebang when the path names none

Source: Paddock T-13 (`b2c2ea4f-4f64-4e02-953d-20843cb10b79`), P3, draft (needs the user's approval).
Split from T-13: this is GPUI-free content detection in fe-syntax; the `EditorView` host API is T-13b.

## Problem

A buffer's language comes only from its path: `language_name_for_path`
(`crates/fe-syntax/src/language.rs`), called from `EditorView::new`. Extensionless scripts
(`#!/usr/bin/env python3` in `bin/deploy`) and untitled or AI scratch buffers get no highlighting.

## Change

New module `crates/fe-syntax/src/detect.rs`, exported from `crates/fe-syntax/src/lib.rs`:

- `pub fn language_name_for_shebang(first_line: &str) -> Option<&'static str>`: parses `#!`, takes the
  interpreter's file name, and for `env` the first argument that isn't an option (handles
  `env -S node --flag`). Strips a version suffix (`python3`, `python3.12`). Maps by a table in
  `detect.rs`, not a new field on every language definition:
  `python` → python; `sh`, `bash`, `zsh`, `dash`, `ksh` → bash; `node`, `nodejs` → javascript.
  Returns `None` for anything else, and for a language not compiled in (check against
  `language_names()`), so it respects the per-language Cargo features.
- `pub fn detect_language_name(path: Option<&Path>, text_start: &str) -> Option<&'static str>`:
  the path wins; otherwise the shebang on the first line of `text_start`. Callers pass the first few
  hundred bytes, never the whole buffer.
- Tests inline in `detect.rs` (`#[cfg(test)] mod tests`).

Detect by our own reading of the shebang convention; don't consult Zed's GPL `language` crate
(CLEAN_ROOM.md).

## Open questions

- Modelines (`# vim: ft=python`, `-*- mode: python -*-`) were in the source card's wish list but not
  its acceptance. Left out here; say if they belong.

## Implementation notes

- `env` is read the way GNU env reads a shebang. Its arguments are the line's whitespace-separated
  words until `-S`. Its operand (attached, or the rest of the line) is split by GNU's `-S` rules:
  quotes, backslash escapes including `\_` and `\c`, and `#` comments. The words it yields are parsed
  as further options, as GNU does.
- Options follow `getopt_long`. Each option takes a fixed kind of operand: none, required (attached
  or the next word), or, for the signal options, only an `=value`. Short options cluster
  (`-iS python3`), and long options may be abbreviated to any unambiguous prefix. BSD's `-P` is
  accepted too. Then a lone `-` and `NAME=value` assignments are skipped.
- Detection gives `None` where env would reject the line: an unknown or ambiguous option, a missing
  operand, an unterminated quote, an invalid escape, or `$` not in `${NAME}` form. It also gives
  `None` for `-0`, `--help` and `--version`, which run no command. `${NAME}` is left unexpanded, so
  `-S ${PY}` gives `None`, while `/opt/${V}/bin/python3` is still Python.
