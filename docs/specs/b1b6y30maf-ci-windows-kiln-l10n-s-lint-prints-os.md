---
task_uid: "01m6ygp2n62vjh90b1b6y30maf"
task: "T309"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "catalogue.rs crate_dir and keys.rs render the paths in their messages with '/' separators on every OS"
    required: true
    manual: false
  - id: "C2"
    text: "new test kiln-l10n lint_paths passes on Linux with backslash-separated components fed to the helper; the existing lint_catalogue and lint_keys tests are unchanged and pass"
    required: true
    manual: false
  - id: "C3"
    text: "Only the three scope files change"
    required: true
    manual: false
---

# CI (windows): kiln-l10n's lint prints OS-native path separators, so lint_catalogue and lint_keys tests fail on Windows (Mac T1622)

# CI (windows): kiln-l10n's lint prints OS-native path separators, so three lint tests fail on Windows

## Evidence

GitHub CI's `windows` job (run 42520105964, main 7358064f) fails three kiln-l10n tests, none of them on Linux or macOS:

    lint_catalogue::a_root_reports_unused_keys_stale_translations_and_missing_counts  (tests/lint_catalogue.rs:242)
    lint_catalogue::a_library_root_exempts_its_public_keys                            (tests/lint_catalogue.rs:286)
      left:  "en-US: alpha/l10n/en-US\\alpha.ftl: `alpha-unused` is unused"
      right: "en-US: alpha/l10n/en-US/alpha.ftl: `alpha-unused` is unused"
    lint_keys::each_problem_is_reported                                                (tests/lint_keys.rs:63)
      "l10n\en-US\unlisted.ftl isn't listed in the crate's resources!"

The "doesn't format: pl: ... Unknown message: alpha-referenced" and the "doesn't parse ... ExpectedCharRange" lines in the log
are the `{report}` dump of fixtures that are meant to be broken (the bad fixture and the pl reference), not extra failures.
The repo's .gitattributes already forces `eol=lf`, so CRLF is not the cause.

## Cause

Two places build a user-facing name from a `Path` with `.display()`, which prints `\` on Windows:
- `kiln/crates/kiln-l10n/src/lint/catalogue.rs` `crate_dir`: `name: format!("{crate_name}/l10n/{}", rel.display())`
- `kiln/crates/kiln-l10n/src/lint/keys.rs` (~line 111): `found.strip_prefix(&self.dir)...display()` for the "isn't listed" message.
`lint/source.rs` already normalises its paths (`.replace('\\', "/")` in `rel`); these two do not.

## Fix

Render both with `/` separators (join the path's components with `/`, or `.to_string_lossy().replace('\\', "/")`, matching source.rs).
Leave the tests' expected strings as they are. Linux-testable: add a unit test in a new file that feeds the helper a path with
backslash components, so the regression is checked without Windows.
