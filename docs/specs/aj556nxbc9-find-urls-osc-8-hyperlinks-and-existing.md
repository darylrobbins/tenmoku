---
task_uid: "01m4s5kb292j0ewbaj556nxbc9"
task: "T61"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-terminal test links::urls: in a row \"see https://example.com/a?b=1).\" link_at on any URL cell gives LinkTarget::Url(\"https://example.com/a?b=1\") with cols covering exactly it; \"(https://x.test/z)\" excludes both parens; \"https://en.wikipedia.org/wiki/Rust_(language)\" keeps its balanced paren; a cell outside any link is None"
    required: true
    manual: false
  - id: "C2"
    text: "fe-terminal test links::paths: with a temp cwd holding src/main.rs, \"src/main.rs:12:5: error\" gives File { path: \u003ccwd\u003e/src/main.rs, line: Some(12), column: Some(5) }, \"src/main.rs(3,7)\" line 3 column 7, \"./src/main.rs\" and its absolute path no line; \"missing.rs:1\" (no such file) is None"
    required: true
    manual: false
  - id: "C3"
    text: "fe-terminal test links::osc8: after printf of an OSC 8 link to https://a.test/x around \"label\", every cell of label has the same Some(link) index, Snapshot::links holds the URI, and link_at on it gives Url(\"https://a.test/x\") with cols over label; an OSC 8 file:// URI gives LinkTarget::File"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-terminal -p fe-terminal-view passes (layout.rs only gains the new Cell and Snapshot fields) and scripts/check-no-gpui.sh prints ok for fe-terminal; no Cargo.toml or Cargo.lock change; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Find URLs, OSC 8 hyperlinks and existing file paths with line and column under a terminal cell in fe-terminal (Mac T574)

Filed on argon from Mac T574 (card ~/coordination/argon-cards/T574.json rev 1). Mac task uid 01m4s5c8326yrv8s7nbgbxz310. Depends on argon T60: both change crates/fe-terminal/src/lib.rs. Title the host commit "... (Mac T574)".

## Problem

Output in the terminal panel names URLs and `src/foo.rs:12:5` locations (compiler errors, test failures, grep), and
programs emit OSC 8 hyperlinks, but nothing in fastedit can find them: VS Code, Zed and iTerm open them on
cmd-click. fe-terminal's `Snapshot` (`crates/fe-terminal/src/snapshot.rs`) drops alacritty's per-cell hyperlink.

## Change

GPUI-free, in fe-terminal (the cmd-click and underline in fe-terminal-view are a later card):

- `crates/fe-terminal/src/snapshot.rs`: `Cell` gains `link: Option<u32>`, an index into a new
  `Snapshot::links: Vec<String>` of the OSC 8 URIs on screen (cells of one hyperlink share an index; alacritty's
  `Cell::hyperlink()`).
- New `crates/fe-terminal/src/links.rs` (`mod links;` and `pub use links::{Link, LinkTarget, link_at};` in
  `crates/fe-terminal/src/lib.rs`): `link_at(snapshot: &Snapshot, row: usize, col: usize, cwd: &Path) ->
  Option<Link>`, `Link { row, cols: Range<usize>, target }` (the run of cells to underline, on that row) and
  `LinkTarget::{Url(String), File { path: PathBuf, line: Option<u32>, column: Option<u32> }}`. In order:
  1. An OSC 8 link on the cell: its URI (`file://` URIs become `File`).
  2. A URL under the cell: `https?://`, `file://`, `mailto:` up to whitespace or a closing bracket/quote that has no
     opener inside it, trailing `.,;:!?` dropped.
  3. A path under the cell: a run of path characters with an optional `:line[:column]` or `(line[,column])`
     suffix, resolved against `cwd` when relative (`~/` expanded), kept only if the file or directory **exists**
     (one `metadata` call per candidate; no other I/O).
  Links don't continue across rows (a soft-wrapped link is a known limit, said in the module doc).

## Tests

New file `crates/fe-terminal/tests/links.rs`: snapshots from `/bin/sh -c 'printf …'` sessions as in
`crates/fe-terminal/tests/session.rs`, including an OSC 8 sequence, against a temp dir as `cwd`.

## Implementation notes

- A quote ends a URL when the same quote sits just before the URL (`"http://x/p"`); otherwise quotes pair up inside it. `<` and `>` also end a URL.
- A text `file://` URL becomes `LinkTarget::File`, like an OSC 8 one, with `%XX` decoded and the host dropped.
- Path characters are alphanumerics and `/._-~+@%`. A run needs at least one alphanumeric, so `.` and `..` alone never link. With no line suffix, if a run that ends in dots doesn't exist, it is retried without them (`see src/main.rs.`), which costs a second `metadata` call.
- OSC 8 links share an index in `Snapshot::links` when alacritty's hyperlink (id and URI) is equal.
