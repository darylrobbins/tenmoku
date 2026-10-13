---
task_uid: "01m4synt8rzqbfxzffhbvz8ewe"
task: "T96"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "The new crates/fe-nav/tests/scip_references_memory.rs is its own test binary with one test named exactly references_drop_each_file_once_reported (no other name): 65 files delivered and peak live heap growth under 16 MiB during the call; it fails at base"
    required: true
    manual: false
  - id: "C2"
    text: "Results, their order, summary counts and cancellation are unchanged; every existing fe-nav test passes unchanged"
    required: true
    manual: false
---

# Drop each file's loaded text once its SCIP references are reported, instead of holding every referencing file until the request ends (Mac T628)

{
  "mode": "inline",
  "body": "## Problem\n\n`ScipNavigation::references` (crates/fe-nav/src/scip_provider.rs, around lines 338-380) sorts the\nsymbol's sites by file and handles them one file at a time with `chunk_by`, loading each file's text\nthrough `Texts::get` (around line 484). `Texts::loaded` is a `HashMap` that never evicts, and because each\nfile is visited exactly once it never gets a hit either: it only keeps every referencing file's `Text`\n(each up to 32 MiB) alive until the whole request returns. Find-references on a widely used symbol in a\nlarge repo indexed by SCIP holds hundreds of MB to GB at once, although each file's references were\nalready sent to the sink.\n\nA probe on a scratch copy of main (e6988aba) with 64 files of 1 MiB, each holding one reference, measured\n73 MiB peak extra heap with a counting `#[global_allocator]`; with the change below it was 3 MiB. `git log`\nshows no fix on main.\n\n## Change\n\nIn `ScipNavigation::references`, drop each file's loaded text once its references are built: after the\n`refs` vector is collected and before the `refs.is_empty()` check, remove `rel` from `texts.loaded` (add a\nsmall `Texts::forget(&mut self, rel: &Path)` if that reads better). The requesting file's text comes from\n`req.text`, not the map, so it is unaffected. Nothing else in the loop uses the dropped text: `accept`\nand `reference` take the `text` passed to them. Results, their order, the summary counts and cancellation\nare unchanged.\n\nRegression test: a new integration test file crates/fe-nav/tests/scip_references_memory.rs, public API\nonly (`ScipNavigation::new`, `NavigationProvider::references`, `NavRequest`), built the way\ncrates/fe-nav/tests/scip_provider.rs builds an index. It installs a counting `#[global_allocator]` (a\nwrapper over `std::alloc::System` tracking live and peak bytes in atomics), so it must be its own test\nbinary with that single test, named exactly `references_drop_each_file_once_reported`. It writes\n`main.js` (`\"foo;\\n\"`, holding the definition of `npm pkg 1.0 m/foo.`) and 64 files of `\"foo;\\n\"`\nfollowed by 1 MiB of filler, each with a reference occurrence `[0, 0, 3]` of that symbol; runs\nreferences from the definition with a sink that drops each batch after counting it; and asserts 65 files\nwere delivered and the peak live heap grew by less than 16 MiB during the call (73 MiB at base).",
  "ref": "",
  "ref_blob": ""
}
