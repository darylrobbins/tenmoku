---
task_uid: "01m7wj30g9sx06trtak878xqx9"
task: "T715"
rev: 5
type: "docs"
complexity: "s"
criteria:
  - id: "C1"
    text: "docs/user/ is an mdBook (book.toml, `src/SUMMARY.md`) with these top-level chapters, each a stub page with a one-paragraph description and a visible `\u003e Status: stub` line: Getting started, Editing, Vim mode, Languages and LSP, Git, Terminal, Debugging, Plugins, Settings reference, Keyboard shortcuts (the last two as `reference/settings.md` and `reference/keyboard-shortcuts.md` placeholders that HD2 replaces with generated pages; plus `reference/commands.md`). No real content is written."
    required: true
    manual: false
  - id: "C2"
    text: "scripts/check-user-docs.sh (POSIX sh, no mdbook needed) fails when: a SUMMARY entry has no file, a file under src/ is not in SUMMARY, the file part of a relative link does not resolve (fragments after # are not checked: a documented Known limit), a page lacks an H1, a non-generated page has no Status line, any stage of its own file-listing pipeline fails, or a page or SUMMARY.md uses a link form outside the supported subset (reference-style definitions, angle-bracketed destinations, a destination not closed on its line). Lines in ``` or ~~~ fenced code are skipped for the H1, Status and link checks. scripts/test-check-user-docs.sh feeds it a good tree and one broken tree per rule, including the failing listing sed, each unsupported link form, and a page whose only H1 or Status line is inside a fence. Relative links in SUMMARY.md are checked only in its chapter entries; other SUMMARY links are a Known limit carried to T721. The tests include a page under src/ whose only SUMMARY entry is inside a fence, reported as missing from SUMMARY."
    required: true
    manual: false
  - id: "C3"
    text: "docs/user/README.md: how to preview (`mdbook serve docs/user`), the stub convention, and that reference/ pages are generated (do not edit). The repo's README.md links to https://tenmoku.dev/docs. It also says in one line that link fragments (#…) are not checked yet. It also says in one line which link forms the checker accepts."
    required: true
    manual: false
  - id: "C4"
    text: "Choice recorded in docs/user/README.md: mdBook over a plain Markdown tree or a site generator, because it is one static binary with no Node, builds the SUMMARY into a navigable book with search, the sources stay plain Markdown that renders on GitHub, and CI installs it in seconds."
    required: true
    manual: false
---

# User docs scaffold: docs/user as an mdBook with the ten stub pages and a structure check (help 1) (Mac T2010)

# User docs scaffold

The user: "help documentation would probably be good. That's a bigger project, but maybe start with the scaffolding." Today docs/ is internal (decisions, designs, release, specs, plugins/authoring.md); there is no user-facing documentation. tenmoku.dev is a separate site (no source in this repo; only README.md links to it).

The book is published at https://tenmoku.dev/docs by HD3's GitHub Actions pipeline (user decision 2026-11-20), so book.toml sets `[output.html] site-url = "/docs/"` (mdBook needs it for the 404 page under a subpath).

Out of scope: real content (later cards), publishing (HD3), generated pages (HD2). Do not run mdbook in the gate; the check script is the gate. Files read first: README.md, docs/plugins/authoring.md (the Plugins chapter will link to it), docs/vim.md (the Vim chapter's future source).


Read the files named before starting. Do not widen --scope; if a file outside it is needed, stop and respec. New tests go in new files.

## Helium filing note

- Lent from the Mac as y153, the user's request (structural search SS1-SS9 on a user-approved design: one
  language per search, injected languages skipped, `inside` the only relational rule; user docs help 1-2).
- linux-ok: headless tests. The macOS gate runs when the batch lands on the Mac.
- Builds and loops: CARGO_TARGET_DIR inside your worktree, never /tmp.
- Windows: imports and helpers that only `#[cfg(unix)]` tests use go under `#[cfg(unix)]` too, or Windows Clippy
  fails with -D warnings (Mac T2028). Linux gates can't catch this.
- Golden and pin tests: a new command changes typed_command_ids_tests.rs and command_catalog_tests.rs; a new
  setting changes the settings schema/template goldens, settings_model.rs and the l10n files; a new binding can
  break binding-count tests in fe-gpui. If one fails that isn't in scope, widen it with `rgt task respec --scope`
  (it replaces the list, so pass the whole list) before you finish, and say why in the spec's Implementation notes.
- Criteria tests need full cargo test paths (`module::tests::name`, or `fe-x::binary name` for an integration
  test whose name exists in several binaries).
- Ask the lead (`herdr agent prompt wR7:p1 ...`) before rgt finish if anything in the criteria can't be met as
  written. Run rgt finish once, and wait for its result before you report.

## Known limits (lead respec after review round 3, review 2e4nfz)

- **Fragments are not validated.** The checker resolves the file part of a relative link (`page.md#part`
  checks `page.md` exists) and ignores the `#part`. Checking fragments needs mdBook's heading-anchor rules
  (duplicate headings, Unicode, punctuation), which is a card of its own: review finding F2 goes to a
  follow-up card on the Mac board. `docs/user/README.md` says so in one line, and the checker's comment says so.
- **F4 is still this card's to fix:** every stage of the file-listing pipeline in check-user-docs.sh has its
  exit status checked (run the stages separately, e.g. through a temp file), and
  test-check-user-docs.sh has a case where the listing `sed` fails while `sort` succeeds and the checker fails.

## Supported Markdown subset, fail-closed (lead respec after review round 4, review 31s3vd)

The checker is not a Markdown parser. It checks a small subset that docs/user is written in, and **fails on any
link form outside it** rather than guessing, so nothing it can't read slips past it:

- **Links:** inline links whose destination is on the same line as `](` and is not wrapped in `<...>`.
  The checker fails, naming the file and line, on a reference-style link definition (a line like
  `[label]: target`), an angle-bracketed destination (`](<`), or a `](` with no closing `)` on its line.
  This covers SUMMARY.md too, so a reference-style chapter entry fails rather than being missed (F5), and so do
  multi-line or angle-bracketed destinations (F6). The tests have one broken tree for each.
- **Code:** lines inside fenced code blocks (``` or ~~~ fences) are skipped for the H1, Status and link checks
  (F7): a page whose only `# ` or `> Status:` line is in a fence fails. Indented code blocks aren't recognised,
  and that's a known limit.
- docs/user/README.md says in one line which link forms the checker accepts.

## Lead respec after review 5dsknj (rev 5)

- **F11 moves to helium T721** (Mac T2038, the fragment follow-up, which depends on this card): relative links in
  SUMMARY.md outside its chapter entries aren't resolved by this card's checker. That's a Known limit here, and
  the README's link-limits line says so.
- **F10 stays here:** add a test tree with an existing page under src/ whose only SUMMARY entry is inside a fence,
  and assert the checker reports that page as missing from SUMMARY.
