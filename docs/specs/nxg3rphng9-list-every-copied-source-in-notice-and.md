---
task_uid: "01m4sfx2zserj2ktnxg3rphng9"
task: "T68"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "fastedit test notice::sources_are_in_notice passes on the repository: NOTICE names every source of THIRD_PARTY.md's \"Code copied or adapted\" and \"Icons\" rows (gpui-pre, gpui-kit, nvim-treesitter, tree-sitter-ruby, tree-sitter-php, tree-sitter-kotlin-sg, tree-sitter-scala, tree-sitter-lua, Lucide), each entry with a copyright line and its license, and kiln/NOTICE names every source of kiln/THIRD_PARTY.md's rows"
    required: true
    manual: false
  - id: "C2"
    text: "fastedit test notice::license_texts_ship passes: every license identifier in those rows (Apache-2.0, MIT, ISC) has a text: LICENSES/Apache-2.0.txt and LICENSES/MIT.txt at the root, kiln/LICENSES/Apache-2.0.txt, and Lucide's existing icons LICENSE for ISC; the added texts are the verbatim SPDX texts"
    required: true
    manual: false
  - id: "C3"
    text: "fastedit test notice::checks_catch_gaps passes: in-memory fixtures for a row whose source NOTICE doesn't name, a row whose license has no text, and a multi-license cell with one text missing each fail with a message naming the table, the row's path and the rule; a complete fixture passes"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fastedit --test notice, cargo clippy -p fastedit --all-targets -- -D warnings and python3 scripts/check_kiln_boundary.py pass; THIRD_PARTY.md and kiln/THIRD_PARTY.md are unchanged"
    required: true
    manual: false
---

# List every copied source in NOTICE and ship the Apache-2.0 and MIT texts they need, checked against THIRD_PARTY.md by a test (Mac T592)

Filed on argon from Mac T592 (card ~/coordination/argon-cards/T592.json rev 1). Mac task uid 01m4sdx07s870jt010wq524t1n. No dependencies. Agent-only. Title the host commit "... (Mac T592)".

## Problem

The plan's reuse rules say "Copied code goes in files with its original license header. `NOTICE` and
`THIRD_PARTY.md` list each source, its commit and its license" (plan, "Reuse map"), and risk 3 lists "per-file
headers on copied code" against license contamination. T456 (landed as helium T62, `crates/fastedit/tests/third_party.rs`)
now checks that THIRD_PARTY.md rows and file headers agree, but nothing checks `NOTICE`, and it has fallen behind:

- `NOTICE` names three sources: GPUI, gpui-kit "(soft wrap and folding, adapted in crates/fe-display)" and Lucide.
  THIRD_PARTY.md's "Code copied or adapted into this repository" table also has gpui-kit's `diagnostics.rs` in
  `crates/fe-editor/src/diagnostics.rs`, **nvim-treesitter** (Apache-2.0: `crates/fe-syntax/queries/toml.scm`,
  `kotlin.scm`), and five grammar crates whose `queries/tags.scm` we adapted under MIT (tree-sitter-ruby,
  tree-sitter-php, tree-sitter-kotlin-sg, tree-sitter-scala, tree-sitter-lua: the `queries/outline/` and
  `queries/definitions/` rows). None of those six sources is in NOTICE, and MIT asks that its copyright and
  permission notice go with every copy.
- THIRD_PARTY.md says, for gpui-kit, nvim-treesitter and the adapted queries, "Ship the Apache-2.0 license text
  with them", but the repository has no Apache-2.0 or MIT license text to ship (only Lucide's
  `kiln/crates/kiln-ui/assets/icons/LICENSE` and the vendored `vendor/tree-sitter-yaml/LICENSE`). kiln, which is
  to leave this repository with its own NOTICE, adapts GPUI's Apache-2.0 `examples/input.rs` and has no license
  text either.

## Change

1. **`NOTICE`:** one entry per source in THIRD_PARTY.md's "Code copied or adapted into this repository" and
   "Icons" tables, in the existing entry format (name and what we took, copyright line, license line). Widen
   gpui-kit's entry to say "soft wrap, folding and diagnostics, adapted in crates/fe-display and
   crates/fe-editor". Add nvim-treesitter (Apache-2.0; "Copyright The nvim-treesitter contributors" or the line its
   LICENSE carries) and the five grammar crates, each with the copyright line from that crate's own `LICENSE` file
   at the version THIRD_PARTY.md names (read it from the cargo registry; if a crate ships no LICENSE file, as
   THIRD_PARTY.md says of Ruby and Lua, take the line from its repository's LICENSE at that version and say so).
   Each entry says where the license text is (point 2).
2. **License texts:** add `LICENSES/Apache-2.0.txt` and `LICENSES/MIT.txt` at the repository root, the verbatim
   texts from spdx.org/licenses (MIT's with the `<year> <copyright holders>` line left as the template; the
   holders are in NOTICE), and `kiln/LICENSES/Apache-2.0.txt`, the same Apache text, for kiln's GPUI adaptation.
   Add a line to `kiln/NOTICE`'s GPUI entry pointing at it. No other kiln file changes.
3. **The check:** a new test file `crates/fastedit/tests/notice.rs` (do not edit `third_party.rs`). Plain functions
   over `(path, text)` pairs, run once over the repository and once over in-memory fixtures:
   - **Sources are in NOTICE.** For each row of the "Code copied or adapted" and "Icons" tables of
     `THIRD_PARTY.md` (checked against `NOTICE`) and of `kiln/THIRD_PARTY.md` (against `kiln/NOTICE`), the row's
     source name appears in the NOTICE file. The source name is the Source cell's first word, or the text of its
     first markdown link when the cell starts with one (`[gpui-kit](…)` gives `gpui-kit`, `nvim-treesitter
     \`runtime/…\`` gives `nvim-treesitter`, `tree-sitter-ruby crate …` gives `tree-sitter-ruby`,
     `[lucide-icons/lucide](…)` gives `lucide-icons/lucide`, matched case-insensitively against NOTICE with a
     `Lucide` alias for it).
   - **License texts ship.** Each License cell's identifier (`Apache-2.0`, `MIT`, `ISC`; a cell with more than one,
     such as "ISC (Feather-derived icons: MIT)", needs each) has a text file: `LICENSES/<id>.txt` beside that
     NOTICE, or, for ISC, the Lucide `LICENSE` beside the icons, which the Icons table already names.
   - Each failure names the table, the row's path and the rule.

THIRD_PARTY.md and kiln/THIRD_PARTY.md don't change (THIRD_PARTY.md is append-only for tasks under `regatta.toml`
`shared_append_only`); the NOTICE entries say where the texts are. Generating a full `THIRD_PARTY_LICENSES` for every dependency stays the release-checklist item
THIRD_PARTY.md describes; it is not this card.
