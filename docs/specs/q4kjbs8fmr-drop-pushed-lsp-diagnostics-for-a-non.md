---
task_uid: "01m543dtr2941xn2q4kjbs8fmr"
task: "T740"
rev: 2
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-lsp test tests::diagnostics::uri_scheme::non_file_publications_for_an_open_files_path_are_dropped passes: custom:/// and untitled:/// publications whose decoded path is an open file's are dropped and a marker on the file's own URI comes next"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-lsp passes with no existing test edited; tests/diagnostics.rs changes only by the mod uri_scheme; line"
    required: true
    manual: false
---

# Drop pushed LSP diagnostics for a non-file URI instead of matching them to an open file by decoded path (T728, argon T106 review F1)

## Problem

argon T106 review F1 (codex, medium), on Mac T728's change (argon T106, commit 203332d, landing on the Mac
through host stack 14): `crates/fe-lsp/src/server.rs` `same_file` (around line 345) and `Actor::published`
(around line 1252) decode `params.uri.to_file_path()` without checking its scheme, and `same_file` checks
only the open document's scheme. `url::Url::to_file_path` doesn't look at the scheme (its docs say so), so
a publication for another scheme with the same path matches the open file and replaces its diagnostics.
T728's spec required both URIs to be `file` URIs.

Checked with url 2.5.8 (the locked version): `custom:///proj/app/%5Bslug%5D/page.txt`,
`untitled:///proj/app/%5Bslug%5D/page.txt` and `git:/proj/app/%5Bslug%5D/page.txt` all give
`Ok("/proj/app/[slug]/page.txt")`, equal to the open `file:///proj/app/[slug]/page.txt`'s path.

## Change

- `server.rs` `Actor::published`: decode the publication's path only when `params.uri.scheme() == "file"`
  (otherwise `None`), so `same_file` falls back to the decoded path only for two `file` URIs. Exact URI
  equality is unchanged.
- New test module `crates/fe-lsp/src/tests/diagnostics/uri_scheme.rs`, declared with `mod uri_scheme;` in
  `crates/fe-lsp/src/tests/diagnostics.rs` beside T728's `mod uri_spelling;` (the only change there), using
  that module's `publish`, `over`, `next` and `marker` helpers through `super::`. Test
  `non_file_publications_for_an_open_files_path_are_dropped`: open a synced buffer as `app/[slug]/page.txt`
  (`Started::open_synced`); publish version 0 with a diagnostic for the same path under `custom:///` and
  then under `untitled:///`, each spelled `%5Bslug%5D`; a `marker` on the buffer's own URI comes next (both
  were dropped). It fails at the base (T728's code).

Depends on Mac T728: `same_file` and tests/diagnostics/uri_spelling.rs land with it.
