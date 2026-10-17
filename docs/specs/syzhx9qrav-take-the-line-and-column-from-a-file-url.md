---
task_uid: "01m54hwzjxnjb4grsyzhx9qrav"
task: "T141"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-terminal test file_urls_take_a_line_and_column_suffix (new crates/fe-terminal/tests/file_url_lines.rs, top-level fn, base API only) passes: on a one-row snapshot of '    at main (file://\u003cexisting temp index.mjs\u003e{suffix})', link_at over the URL covers exactly the URL and its suffix and gives LinkTarget::File { path: the file, line, column } with (Some(3), Some(9)) for ':3:9', (Some(3), None) for ':3' and (None, None) for no suffix; it fails at the base (the path keeps ':3:9')"
    required: true
    manual: false
  - id: "C2"
    text: "Only a file:// URL found in the text (url_at) has a trailing :N or :N:M split off its decoded path, parsed as location parses a path's; an OSC 8 hyperlink's URI, http(s):// and mailto: URLs and file:// URLs with no such suffix give the targets they give at the base; no I/O is added; the module doc lists the new form"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-terminal passes with no existing test edited (tests/links.rs, link_marks.rs and link_line_words.rs among them); cargo clippy -p fe-terminal --all-targets -- -D warnings is clean; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Take the line and column from a file:// URL's :N or :N:M suffix in terminal links, so a Node.js ES module stack frame opens at its row (Mac T760)

## Problem

A `file://` URL in terminal text that ends in `:line` or `:line:column` links to a path with the
suffix in it, which doesn't exist, so clicking it opens nothing (or an empty new file) instead of the
file at that row. Node.js prints ES module stack frames this way, and so do Deno, Vite and Vitest:
`    at main (file:///Users/me/proj/index.mjs:3:9)`. A plain path with the same suffix
(`/Users/me/proj/index.mjs:3:9`) already opens at 3:9.

Cause: crates/fe-terminal/src/links.rs `url_at` runs `url_end` (which keeps `:3:9`; only a trailing
`.,;:!?` is dropped) and hands the whole URL to `uri_target`, which percent-decodes everything after
the host as the path and always sets `line: None, column: None`.

Reproduced at Mac main b7bd02cf: an existing `<tmp>/index.mjs`, a one-row snapshot of
`    at main (file://<tmp>/index.mjs:3:9)`, `link_at` on the URL gives
`File { path: "<tmp>/index.mjs:3:9", line: None, column: None }`; expected
`File { path: "<tmp>/index.mjs", line: Some(3), column: Some(9) }`.

## Change

In crates/fe-terminal/src/links.rs, for a `file://` URL found in the text (`url_at`), split a trailing
`:N` or `:N:M` (ASCII digits) off the decoded path and return it as the target's `line` and `column`,
parsed as `location` parses a path's `:N:M`. The link's cells still cover the whole URL including the
suffix. Nothing else changes: an OSC 8 hyperlink's `file://` URI (`link_at`'s cell-link branch) keeps
its path as given, a URL with no such suffix is unchanged, `http(s)://` and `mailto:` URLs are
unchanged, and no I/O is added. Update the module doc's list of forms to include
`file:///src/main.rs:12:5`.

Regression test: new integration test crates/fe-terminal/tests/file_url_lines.rs (base API only; it
builds a one-row `Snapshot` as tests/link_marks.rs does and leaves that file alone). Top-level fn
`file_urls_take_a_line_and_column_suffix`: with an existing temp `index.mjs`, the text
`    at main (file://<path>{suffix})` for suffix `:3:9`, `:3` and none gives a link over exactly the URL
and its suffix, to `File { path, line, column }` = (3, 9), (3, None) and (None, None). Run at b7bd02cf it
fails on `:3:9`; with the change prototyped it and tests/links.rs, link_marks.rs and link_line_words.rs
pass.
