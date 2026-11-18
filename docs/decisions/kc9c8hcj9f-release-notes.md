# Release notes in the release (T1947): decisions

## The appcast's sparkle:format: markdown

`scripts/make-appcast.sh --notes-file` writes
`<description sparkle:format="markdown"><![CDATA[...]]></description>`.

The spec asked what Sparkle 2.10.1 (`scripts/fetch-sparkle.sh`) shows for markdown on an 11.x
Mac, since the app's minimum is 11.0 (`crates/fastedit/macos/Info.plist`). Read in Sparkle's
source at tag `2.10.1`:

- `SUUpdateAlert.m`: with no `releaseNotesLink`, the alert takes the item's description, maps
  `sparkle:format` `plain-text` to plain text, `markdown` to markdown and anything else to HTML,
  and gives both plain text and markdown to `SUTextViewReleaseNotesView`.
- `SUTextViewReleaseNotesView.m`, `_loadString`: for markdown it calls
  `-[NSAttributedString initWithMarkdownString:options:baseURL:error:]` (an API from macOS 12)
  with no `@available` check, and falls back to the plain string if that returns nil. Its
  formatting helpers are marked `API_AVAILABLE(macos(12.0))`.
- There is no check because Sparkle 2.10.1 itself needs macOS 12:
  `Configurations/ConfigCommon.xcconfig` sets `MACOSX_DEPLOYMENT_TARGET = 12.0`.
  `vtool -show-build` on the fetched framework gives `minos 12.0` for both slices of
  `Sparkle.framework/Versions/B/Sparkle` and for its `Updater.app`, and that `Updater.app`'s
  `LSMinimumSystemVersion` is `12.0`.

So on an 11.x Mac Sparkle 2.10.1 isn't a working updater at all, whatever the description's
format. That Mac never sees Sparkle's update alert, so `plain-text` would show it nothing more.
`crates/fastedit/src/sparkle.rs` logs a failed load or start and leaves Check for Updates…
asking GitHub. On 12 and later, every Mac Sparkle runs on, markdown draws as formatted text, with
headings and bullet lists, and plain text would show the raw `##` and `-` marks. That makes
markdown the right format. The notes' format rules (no raw HTML, no images) keep the text in
what `initWithMarkdownString` handles. If it ever fails to parse, Sparkle shows the text as is.

Not in this card's scope: the app's minimum (11.0) is below what Sparkle 2.10.1 supports (12.0).
On 11.x Check for Updates… falls back to asking GitHub. I haven't checked that on an 11.x
machine, and none is here. Either the app's minimum moves to 12.0, or 11.x stays a
GitHub-check-only platform. A person should decide which, as a card of its own.

## Other calls

- **One script checks the tag and its notes.** `check-release-version.sh vX.Y.Z` runs the
  version check first, then the notes check, so a wrong tag is refused for the tag. A manual run
  has no tag to check, so `--notes X.Y.Z` checks the file alone. The version job's step for
  manual runs (`if: github.ref_type != 'tag'`) runs it and turns a failure into a `::warning::`.
- **The Rust test owns the notes' grammar; the tag check has line rules only** (respec after
  review rounds 1 to 4). `check-release-version.sh` refuses a tag when the notes file is
  missing or empty, or when a line of it:
  - starts with `# ` (or is a lone `#`), an H1;
  - is a draft's `## Internal ...` or `## Unsorted` heading;
  - or holds `]]>`, which would end the appcast's CDATA.

  Each is named with the file, the line and the rule. The Markdown grammar is checked
  elsewhere: the three sections in order, bullets of text, nesting, no images, raw HTML or
  code blocks. That check is fe-workbench's
  `tests::whats_new::the_shipped_notes_are_well_formed`, which parses every
  `release-notes/*.md` with fe-markdown (pulldown-cmark), the parser the What's New tab draws
  with. Every gate runs it, so the commit that gets tagged has passed it.

  Rounds 1 to 4 tried the grammar in awk. Each round's review found notes it judged wrongly:
  too lax at first (setext headings, blocks after a bullet marker, unmatched backticks), then
  too strict (`*` bullets, ```` ```code``` ````, `| x`, `= =`, `\![`, nesting on one line).
  A second Markdown parser can't follow the real one exactly. So the shell check keeps to rules
  that hold whatever the parser does: every line it refuses, the Rust test refuses too, so it
  never refuses notes that test accepts. Its passing fixtures (in `test-release-workflow.sh`)
  are inline code spans of one and three backticks, a literal `|`, `=` and `= =` lines, a
  literal and an escaped `![`, runs of `#`, and nested lists with paragraphs, on separate
  lines and on one line (`- - x`). The shipped notes pass too.

  The cost is that a malformed file which keeps to the line rules passes the tag check. It is
  caught by the gate instead, so not before the build on a tag pushed from an unchecked
  commit. Running the Rust test in the version job, or a small fe-markdown checker there,
  would close that gap. Either needs `crates/` and a Rust build in that job, so it would be a
  card of its own.
- **--notes-file and --notes-url together are refused.** Sparkle shows a `releaseNotesLink`
  page instead of the description, so with both the notes would be ignored without a word.
- **The publish job's body** is `$RUNNER_TEMP/release-body.md`: the notes file, a blank line
  (with a newline added first when the file has none at its end, review F3), and the Downloads
  paragraph as before. A body that can't be written stops the step with an error. macOS's bash
  3.2 doesn't exit under `-e` when a `{ }` group's redirect fails. That is why review F2's
  direct call, with `RUNNER_TEMP` unset, still passed locally, its `gh` handed a body that
  didn't exist. The test now runs every publish through one helper that sets `RUNNER_TEMP`,
  `GH_REPO` and `GLIBC`, and it passes with all three unset in the caller. The job stops with an error before any `gh` call if
  the file is missing. The version job has already refused that tag, so this only guards a job
  run on its own. On a re-run, `gh release edit --notes-file` comes after the asset guards and
  the upload, so a refused re-run still changes nothing. The checkout uses
  `persist-credentials: false`: the job only reads the file, and its token can write releases.
- **The draft script.** The default `--since` is the highest `v*` tag by version
  (`git tag --sort=-v:refname`), not the nearest one `git describe` finds. That is the tag
  "latest" means on main. Merges are left out (`--no-merges`), lines come oldest first, and rgt
  is asked once per task. Subjects starting `Spec T<n>:` (the regatta spec commits) also go
  under Internal, as `Flake:` and version bumps do: like those, a user never needs to read
  them. With no rgt every line goes under Unsorted, as C4 says, those included. A type with no
  section of its own (a spike, say) and a task rgt doesn't know also go under Unsorted. `--out`
  refuses a path that exists, so a draft never overwrites notes already edited. One rgt call
  takes about 1.2 s here, so a release of 50 commits takes about a minute to draft.
