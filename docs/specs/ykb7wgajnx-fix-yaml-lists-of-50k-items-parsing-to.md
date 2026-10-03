---
task_uid: "01m417z42a1q99ynykb7wgajnx"
task: "T15"
rev: 2
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-syntax test a_long_yaml_list_parses_without_an_error_root (integration binary yaml_long_list) passes: a 100k-item '- item N' list parses with no ERROR root and no error nodes"
    required: true
    manual: false
  - id: "C2"
    text: "fe-syntax test tests::golden::highlights_match_golden_files passes"
    required: true
    manual: false
  - id: "C3"
    text: "the deny gate stage passes"
    required: true
    manual: false
  - id: "C4"
    text: "tree-sitter-yaml 0.7.2 with upstream PR #35 applied is vendored under vendor/tree-sitter-yaml/ and wired in through [patch.crates-io] in Cargo.toml, with a README there naming the upstream version, the PR, the commit applied and when to drop the patch (once a release includes the fix, issue #49); THIRD_PARTY.md records it as MIT and CLEAN_ROOM.md lists it as a patched MIT grammar"
    required: true
    manual: false
---

# Fix YAML lists of 50k+ items parsing to an ERROR root

## Problem

Found while measuring flat files for T-15 (merged as `bec5dd8`). A YAML file of `- item N` lines parses correctly at 10k items, but at 50k items or more the root is an ERROR node. Highlighting then mostly stops, with 0 spans past the first screen. The bug is probably in tree-sitter-yaml's external scanner (0.7.2, `THIRD_PARTY.md`), not in our wrapper. The source card's acceptance asks for a 1M-item list "under MAX_PARSE_BYTES". Since T-15, `MAX_PARSE_BYTES` (`crates/fe-syntax/src/syntax.rs`) is 8 MB, and 1M items is about 13 MB, so the largest list that still gets parsed is about 600k items.

## Change

1. **Find the threshold.** Bisect the item count between 10k and 50k. Check whether it depends on item count, byte size or nesting, and whether `- a` lines with a constant item fail the same way.
2. **Check upstream.** Look at tree-sitter-grammars/tree-sitter-yaml issues and newer releases. If a newer release fixes it, bump the workspace dependency in `Cargo.toml` and `Cargo.lock`, and the version in `THIRD_PARTY.md`, re-auditing the licence and query provenance as for any grammar bump. Regenerate `crates/fe-syntax/tests/golden/yaml.yaml.golden` and review its diff.
3. **Otherwise find the cause.** Read the scanner source, which is MIT and in the cargo registry; don't open Zed's code (`CLEAN_ROOM.md`). If a workaround on our side exists without patching the grammar, do it in `crates/fe-syntax`. Patching or forking the grammar is a decision for a person; don't do it.
4. **Regression test.** Add `crates/fe-syntax/tests/yaml_long_list.rs`, gated `#![cfg(feature = "yaml")]` for the feature matrix. It builds a 100k-item `- item N` list (twice the observed threshold, about 1.3 MB), runs `Syntax::parse_job().run()` with no time limit, and asserts the root is not an error and has no error nodes. Keep it at 100k, not 600k: trees cost 30–85× file size (PERF.md, "Tree memory"), and the test suite must stay light. Check the largest list under `MAX_PARSE_BYTES` once by hand, and record the result in the decision file.
5. **Write it down.** Note the threshold and outcome in `PERF.md` under the flat-files section, with a link to any upstream issue.

## Open questions (draft card: needs the user's approval)

- If no newer grammar fixes it and there's no wrapper-side workaround, the regression test can't pass. The outcome is then an upstream bug report plus a `PERF.md` note, which the source card accepts ("or the cause is reported upstream and documented"). The person must then retype this card as `docs` and drop the regression test, or approve patching or forking the grammar, which is an MIT patch but a maintenance burden.
- Who files the upstream issue: the worker drafts it in the decision file, and a person posts it.

## Implementation notes

Outcome: **blocked on a decision.** No newer grammar fixes this, and no workaround is possible on our side. C1 can't pass without patching the grammar.

- **Threshold:** 32,768 **lines**, independent of item count, bytes or line shape. `- item N`, `- a`, `- aaaaaaaaaaaa` and `kN: N` all parse clean at 32,768 lines and get error nodes at 32,769, with or without a final newline. 20k items separated by blank lines (40k lines) fail. With 33k–70k lines the root is fine but has error nodes. At 100k lines the root is ERROR.
- **Largest list under `MAX_PARSE_BYTES`**, checked by hand: 607,122 `- item N` lines (8,388,598 B) give an ERROR root.
- **Cause:** `src/scanner.c` in tree-sitter-yaml 0.7.2 stores `row`, `cur_row`, `end_row` and `blk_imp_row` as `int16_t`. At row 32,768, `cur_row` wraps to −32,768, so `has_nwl = scanner->cur_row > scanner->row` (line 900) is false across that newline, and the line is scanned as a continuation of the previous one. The same wrap recurs every 65,536 rows after that.
- **Upstream:** [tree-sitter-yaml#49](https://github.com/tree-sitter-grammars/tree-sitter-yaml/issues/49), "Documents longer than 32768 lines fail to parse: int16_t row overflow in scanner.c", is open and already diagnoses this, so there's no new issue to draft. The fix, [PR #35](https://github.com/tree-sitter-grammars/tree-sitter-yaml/pull/35) (closes #23), widens the four row fields to `int32_t` and leaves `col` and the indent stacks at 16 bits. It's open and unmerged. 0.7.2 (2025-10-07) is the latest release, and master `a1c4812` still has `int16_t row`. A person could add our numbers to #49, or ask for #35 to be merged and released.
- **No workaround in our code:** the row is private scanner state, carried between tokens only through serialize/deserialize. Included ranges would hide newlines from the scanner and break indentation. Parsing in chunks of fewer than 32k lines would mean stitching several trees together in `Syntax`, which is a redesign, not a workaround.
- **PR #35 verified by hand** in a scratch crate (tree-sitter 0.27.0, 0.7.2 plus the #35 diff): 32,769, 100k and 607,122 items, a 40k-key mapping and a mapping nested 200 deep all parse with no error nodes. It adds 4 bytes of serialized state, and the stacks are unchanged, so the nesting headroom in the 1024-byte buffer is about the same as before. Issue #48, an out-of-bounds write on very deep nesting, predates it.

Options for the person:
1. Retype this card as `docs` and drop C1. The PERF.md note is committed, and we pick up the fix when upstream releases it.
2. Approve vendoring tree-sitter-yaml 0.7.2 with PR #35 applied, through `[patch.crates-io]` and a path or git dependency. That's MIT, a 13-line diff, and dropped once upstream releases. C1–C3 can then be done as written: the regression test, the golden file (which shouldn't change) and the deny gate (the new source has to be allowed).

### Revision 2: grammar patched (option 2 approved)

- `vendor/tree-sitter-yaml/` is the crates.io 0.7.2 package, from release commit `7708026`, with PR #35's commits `fa76593` and `8025f83` (head `8025f83a294d829769d9a99132105bcd68fbaee6`) applied to `src/scanner.c` with `patch -p1`. The vendored scanner is byte-identical to the one verified by hand above. The manifest is the package's `Cargo.toml.orig` without its `tree-sitter` dev-dependency. Upstream's README is replaced by ours, which says when to drop the patch.
- Root `Cargo.toml` adds `[patch.crates-io] tree-sitter-yaml = { path = "vendor/tree-sitter-yaml" }` and `exclude = ["vendor"]`, so the crate is never a workspace member and its unit test, clippy and so on don't run in the gate. In `Cargo.lock`, only the `source` and `checksum` lines of tree-sitter-yaml are dropped.
- `cargo deny check` passes: the crate is still MIT, and a path source needs no entry in `[sources]`. The YAML golden didn't change.
