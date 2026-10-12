---
task_uid: "01m4ry9x0n2d45fagpkpg5a75s"
task: "T62"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "fastedit test third_party::copied_files_and_rows_agree passes: every row of both THIRD_PARTY.md copied-code tables names an existing file whose header gives the row's commit or version and licence, and every file with an adapted-from or copied-from header is named in one of them"
    required: true
    manual: false
  - id: "C2"
    text: "fastedit test third_party::queries_say_where_they_came_from passes, with crates/fe-syntax/queries/imports/javascript.scm and typescript.scm given provenance headers"
    required: true
    manual: false
  - id: "C3"
    text: "fastedit test third_party::no_zed_sources_beyond_gpui passes over the repository, applying rule 4 line by line as the spec words it"
    required: true
    manual: false
  - id: "C4"
    text: "fastedit test third_party::checks_catch_gaps: in-memory fixtures for an unlisted adapted file, a row whose file is missing, a header without the row's licence and a query without provenance are each reported, naming the file and the rule; and for rule 4, 'Copied from Zed editor; helper adapted from crates/gpui/src/x.rs; Apache-2.0', 'Copied from GPUI and Zed editor; Apache-2.0' and a header naming a Zed editor crate path are each reported, while 'Copied from Zed's GPUI examples; Apache-2.0' and kiln-ui text_field.rs's real header pass"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fastedit --test third_party and cargo clippy -p fastedit --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Check third-party provenance in a test: copied files and THIRD_PARTY.md rows match, every query says where it came from, and no source is Zed beyond GPUI (Mac T456)

Filed on helium from Mac T456 (~/coordination/helium-cards/T456.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-z.md). Title, type, complexity, scope, criteria and labels as on the Mac.

Helium note: lent in batch Z to fill idle lanes; independent of helium's T47-T55 chain.

## Problem

The plan's licence rules ("Copied code goes in files with its original license header. NOTICE and THIRD_PARTY.md
list each source, its commit and its license", plan line 32; risk 3, "a per-grammar and per-query audit, and
per-file headers on copied code", line 122) are checked only by reviewers reading diffs. Nothing stops a copied file
from missing its THIRD_PARTY.md row, a row from outliving its file, or a query from saying nothing about where it
came from. Today `crates/fe-syntax/queries/imports/javascript.scm` and `typescript.scm` (T168) have no provenance
line at all, while every other query says "ours" or names its source and licence. CLEAN_ROOM.md (lines 7-14) bans
Zed's GPL crates and Zed's highlight queries, again checked by hand.

## Change

New test file `crates/fastedit/tests/third_party.rs`. The checks are plain functions over `(path, text)` pairs, run
once over the repository (files under `crates/` and `kiln/`, excluding `target/`) and once over in-memory fixtures:

1. **Rows have files.** Each path in the "Code copied or adapted" table of `THIRD_PARTY.md` and of
   `kiln/THIRD_PARTY.md` (kiln's paths relative to `kiln/`) exists, and its first 15 lines contain that row's commit
   or version and its licence.
2. **Copied files have rows.** Each `.rs` or `.scm` file whose first 15 lines say `adapted from` or `copied from`
   (ignoring case), or a `.scm` whose header says `From <name> <version>`, is named in one of the two
   THIRD_PARTY.md files: its repository path, or its path from its crate (`queries/ruby.scm`), in backticks,
   anywhere in the file (rows and footnotes both count).
3. **Queries say where they came from.** Each `.scm` under `crates/fe-syntax/queries/` has, in its first 5 lines,
   either `ours` or a source with an `MIT` or `Apache-2.0` licence.
4. **No Zed beyond GPUI.** Strict and line-based, not a parser of source clauses (helium respec after 3 review
   rounds: each round found another way round free-text source parsing). Any of a file's first 15 lines that
   mentions `zed` in any case and form (`Zed`, `Zed's`, `zed@`, `zed-industries/zed`) passes only if both hold:
   - the line names a GPUI source: `gpui`, `gpui-pre`, `sum_tree` or `GPUI examples` (any case); and
   - the line names nothing else: no word `editor`, no `crates/<name>` path whose name is not `gpui`,
     `gpui-pre*` or `sum_tree`, and no second source (` and `, `&`, `+`, or more than one `from`). The one real
     header, kiln/crates/kiln-ui/src/text_field.rs ("Adapted from gpui-pre 0.3.7 examples/input.rs
     (zed@1a28cff); Apache-2.0"), passes.
   Anything else fails. False failures are acceptable: the fix is to reword the header so each source has a line
   of its own. CLEAN_ROOM.md allows GPUI crates and GPUI's examples only.

Fix what the checks find: add a provenance header to the two imports queries (they were written for T168; check
`git log` and say "ours" if so), and add or correct THIRD_PARTY.md rows for anything else the run reports. Each
failure message names the file and the rule.

## Tests

The new file holds both the repository checks and the fixture tests.
