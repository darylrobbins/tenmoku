---
task_uid: "01m5bbvbxm05w8e8jsfcdykvjy"
task: "T1012"
rev: 1
type: "docs"
complexity: "xs"
criteria:
  - id: "C1"
    text: "The root THIRD_PARTY.md Lucide row names exactly the .svg files in kiln/crates/kiln-ui/assets/icons/ (ls kiln/crates/kiln-ui/assets/icons/*.svg), matching kiln/THIRD_PARTY.md's row"
    required: true
    manual: false
  - id: "C2"
    text: "The root file's Feather (MIT) name lists match kiln/THIRD_PARTY.md's for the same icons"
    required: true
    manual: false
---

# Bring the root THIRD_PARTY.md Lucide icon list up to date with kiln/THIRD_PARTY.md and kiln-ui's icons directory

## Problem

The root `THIRD_PARTY.md` Lucide row (line ~178, `kiln/crates/kiln-ui/assets/icons/`) lists 41 icons, but
`kiln/THIRD_PARTY.md` (line ~27) and the icons directory now hold 59, including `code.svg`, `eye.svg` and
`panel-right-open.svg` added by T1007 (Markdown preview buttons, landed in Mac stack 40, main e6439604) and
earlier additions (`asterisk`, `box`, `braces`, `circle-dot`, `component`, `diamond`, `hash`, `heading`, `link`,
`list`, `plug`, `square-function`, `table`, `type`, `variable`). T1007's worker flagged it: the root file was out
of its scope.

## Change

Make the root `THIRD_PARTY.md` Lucide row's file list, and its Feather/MIT name lists below it, match
`kiln/THIRD_PARTY.md` and the SVGs actually present in `kiln/crates/kiln-ui/assets/icons/`. Docs only.
