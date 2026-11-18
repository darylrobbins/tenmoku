---
task_uid: "01m7p9e6hc7e180nkc9c8hcj9f"
task: "T1947"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "scripts/test-appcast.sh passes: --notes-file gives the item a CDATA description holding the file's text with the chosen sparkle:format and xmllint accepts it; a file holding ]]\u003e or an empty one is refused; no --notes-file gives no description, as now"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/test-release-workflow.sh passes with actionlint clean, checking: the version job's tag check covers the notes file, the macOS appcast step passes release-notes/$VERSION.md, the publish job checks out the tag and uses --notes-file on both gh release create and the re-run's gh release edit, and no --generate-notes is left"
    required: true
    manual: false
  - id: "C3"
    text: "check-release-version.sh fixture runs (in test-release-workflow.sh, on a temp copy of the layout): a well-formed release-notes/X.Y.Z.md passes, and so do bullets with inline code spans, literal |, =, ![ and runs of # in their text, and nested lists; a missing or empty file, a line starting with '# ' (H1), an Internal or Unsorted heading and ]]\u003e each fail, naming the file and the rule. The shell check covers only these line-level rules. Block-level Markdown grammar (images, HTML, blocks inside bullets, nesting) is checked by fe-workbench's tests::whats_new::the_shipped_notes_are_well_formed, which every gate runs on the commit that is tagged; the decision file says so, and the shell check must not refuse notes that test accepts"
    required: true
    manual: false
  - id: "C4"
    text: "scripts/test-draft-release-notes.sh passes: on a temp git repo with tagged history and a fake rgt (RGT), feature subjects land under New, bugfix under Fixed, refactor/chore/docs, Flake: and version bumps under Internal (delete before release), handle-less ones under Unsorted; (Mac T\u003cn\u003e) wins over a trailing (T\u003cn\u003e), trailing handles are stripped, --since and --out work, and with no rgt everything is Unsorted"
    required: true
    manual: false
  - id: "C5"
    text: "The decision file says which sparkle:format the appcast uses and why (what Sparkle 2.10.1 shows for markdown on macOS 11), and docs/release/macos-signing.md's Releasing section has the draft, edit, commit-with-the-bump steps and no longer says generated notes"
    required: true
    manual: false
---

# Release notes in the release: GitHub release body and Sparkle appcast description from release-notes/<version>.md (no --generate-notes), a tag refused without a well-formed notes file, and scripts/draft-release-notes.sh drafting a skeleton from git log grouped by task type

# Release notes in the release: the GitHub release body and the Sparkle appcast from release-notes/<version>.md, a tag refused without it, and a drafting script

User request (2026-11-17), with the card this one depends on: curated release notes instead of
git-cliff, one Markdown file per release (`release-notes/<X.Y.Z>.md`, format in that card's
spec and in its embedding module's doc comment: only `## New`, `## Improved`, `## Fixed`
bullet sections, no H1, no images or raw HTML, no `]]>`). The app shows them in its What's
New tab; this card makes the release use the same file, and makes writing it quick.

## Change

- **Tag check**: `scripts/check-release-version.sh` also refuses the tag when
  `release-notes/<version>.md` is missing, empty, or breaks the format (a heading other than
  the three, an H1, `]]>`), naming the file and the rule. It already runs first in
  `.github/workflows/release.yml`'s version job on tag pushes (:37-39), so the release fails
  there before any build. A manual run (workflow_dispatch) without the file warns and goes on.
- **Sparkle**: `scripts/make-appcast.sh` gains `--notes-file PATH`: the item's
  `<description sparkle:format="markdown"><![CDATA[...]]></description>`, refusing a file that
  holds `]]>` or is empty. The macOS job's appcast step (:111-126) passes
  `release-notes/$VERSION.md` when it exists. Sparkle 2.10.1 (`scripts/fetch-sparkle.sh`)
  reads `sparkle:format="markdown"` on macOS 12 and later, and the app's minimum is 11.0
  (`crates/fastedit/macos/Info.plist:60`): check in Sparkle's source what an 11.x Mac shows for
  it, and use `plain-text` instead if it would show nothing; say which in the decision file.
- **GitHub release**: the publish job (:407-450) checks out the tag (it has no checkout today)
  and writes the body as the notes file, a blank line, then today's Downloads paragraph, with
  `--notes-file`. Drop `--generate-notes`: its commit list is exactly what curating leaves
  out. A re-run on an existing release also sets the body (`gh release edit --notes-file`),
  keeping the appcast and `.sig` guards as they are.
- **Drafting**: new `scripts/draft-release-notes.sh [--since TAG] [--out PATH]` prints a
  skeleton for a person to edit, from `git log TAG..HEAD` (default TAG: the latest `v*` tag).
  Each subject's task is its `(Mac T<n>)`, else its last `(T<n>)`; its type comes from
  `rgt task show T<n> --json --fields type` when `rgt` is on PATH (read-only; `RGT` overrides
  the command, for the test). feature goes under `## New`, bugfix under `## Fixed`; refactor,
  chore, docs, subjects starting `Flake:` and version bumps under `## Internal (delete before
  release)`; no task or no rgt under `## Unsorted`. Each line is the subject with the trailing
  task handles removed. No network, no LLM call. The tag check rejects the Internal and
  Unsorted headings, so an unedited draft can't ship.
- **Docs**: the Releasing section of `docs/release/macos-signing.md` (:111-130) gets the step:
  draft with the script, edit, save as `release-notes/X.Y.Z.md` in the version bump commit, then
  tag. It also says the release body and the appcast come from that file, and no longer says
  "generated notes".

## Tests

- `scripts/test-appcast.sh`: `--notes-file` gives the CDATA description with the file's text
  and the chosen `sparkle:format`, xmllint passes; `]]>` or an empty file is refused; no
  `--notes-file` gives no description, as now.
- `scripts/test-release-workflow.sh`: the version job's tag check, the appcast step's
  `--notes-file`, the publish job's checkout and `--notes-file` on both create and edit, and
  no `--generate-notes`; plus fixture runs of `check-release-version.sh` in a temp copy of the
  repo layout (a good file passes; missing, empty, an H1, an Internal heading and `]]>` each
  fail with the file named). actionlint stays clean.
- New `scripts/test-draft-release-notes.sh`: a temp git repo with tagged history and a fake
  `rgt` printing types; checks the grouping, the handle stripping, `--since`, `--out`, and the
  Unsorted fallback with no rgt.

Not in this card: Velopack's own release notes (`vpk pack --releaseNotes`), notes on the
Windows/Linux update feeds, a releases page on a website.

`test-release-workflow.sh` needs macOS and actionlint: do not lend to Linux. Editing
`release.yml` needs a human risk approval (as T1309's workflow edit did). No open card touches
`release.yml` or these scripts. Do not widen --scope; if a file outside it is needed, stop and
say so.
