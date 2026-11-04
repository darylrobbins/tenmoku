---
task_uid: "01m6j5s4xzfyv02ezq3ttrz8bx"
task: "T436"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "ignore_check::unused_pattern_is_reported: 'nothing_here/' in a project with no such path yields one Unused finding on that line's range; 'src/' that exists yields none"
    required: true
    manual: false
  - id: "C2"
    text: "ignore_check::duplicate_and_shadowed_lines_are_reported: a repeated line is Duplicate{first_line}; '*.log' then '!keep.log' then '*.log' reports the middle line Shadowed{by_line: 3}"
    required: true
    manual: false
  - id: "C3"
    text: "ignore_check::invalid_patterns_are_reported: '[abc' is Invalid, and '!src/a.txt' after 'src/' is reported as unable to re-include"
    required: true
    manual: false
  - id: "C4"
    text: "ignore_check::nested_ignore_file_is_based_at_its_directory: 'build/' in sub/.gitignore matches sub/build and not build at the root"
    required: true
    manual: false
  - id: "C5"
    text: "ignore_check::truncated_or_cancelled_scan_reports_no_unused_lines: over the entry cap or with the cancel flag set no Unused finding is emitted"
    required: true
    manual: false
  - id: "C6"
    text: "ignore_check::dockerignore_is_not_checked: a file named .dockerignore returns an empty report"
    required: true
    manual: false
  - id: "C7"
    text: "cargo nextest run -p fe-project and cargo clippy -p fe-project --all-targets -- -D warnings pass; fe-project still has no GPUI dependency"
    required: true
    manual: false
---

# Check ignore-file patterns against the project: unused, duplicate, shadowed and invalid lines with git matching (Mac T1364)

## Problem
Ignore files give no feedback. JetBrains' .ignore plugin flags unused entries, duplicates and invalid patterns. This card is the engine only (no UI): given an ignore file's text and its location, it returns findings with byte ranges, using git's matching semantics.

## Seams found
- Matching: fe-project already uses the `ignore` crate (crates/fe-project/src/listing.rs: Gitignore, GitignoreBuilder, per-directory rules, global_excludes, parse(dir, file) with BOM handling). Reuse GitignoreBuilder::new(dir_of_the_ignore_file) so a nested file's patterns are based at its own directory. Do NOT use gix for this.
- The tree and finder hide ignored paths, so a pattern like node_modules/ would look unused. The scan must walk with ignore rules off (WalkBuilder standard_filters(false), skip .git) from the ignore file's directory, bounded (cap entries, e.g. 200k, report "scan truncated" and then emit no unused-pattern findings) and cancellable (AtomicBool checked per entry).

## Change
New crates/fe-project/src/ignore_check.rs (declare 'mod ignore_check;' and 'pub use ignore_check::{...}' in crates/fe-project/src/lib.rs; no GPUI). API: check_ignore_file(path: &Path, text: &str, cancel: &AtomicBool) -> IgnoreReport { findings: Vec<IgnoreFinding>, matches: per-line Vec<MatchedPaths> }, where IgnoreFinding { line, range: Range<usize> (bytes in text), kind: Unused | Duplicate{first_line} | Shadowed{by_line} | Invalid(String), severity }.
- Unused: the line's pattern, built alone, matches no walked path (hint severity).
- Duplicate: identical pattern text (after trailing-space trim) as an earlier line (warning).
- Shadowed/overridden: every path the line matches is decided by a later line (later ignore after a negation, later negation after an ignore); computed from the per-path last-match line over the walk (warning).
- Invalid: GitignoreBuilder::add_line errors (e.g. unclosed bracket), plus a negation whose parent directory is excluded by an earlier line ('!' cannot re-include; git docs) (error / warning).
- matches: for each line, the first N (cap 50) relative paths and the total count, for the hover card.
- .dockerignore decision for the first cut: checks run for .gitignore, .ignore, .npmignore and the other gitignore-syntax names, and are SKIPPED for .dockerignore (Docker patterns are relative to the build context, use Go filepath.Match, '**' differs, and a '!' re-includes inside an excluded dir). Return IgnoreReport::default() for it; a follow-up can model Docker semantics.
Tests in the new file crates/fe-project/tests/ignore_check.rs, built on temp dirs (fixture helpers in crates/fe-project/src/fixture.rs): unused, duplicate, shadowed, invalid, nested-base (a nested .gitignore's 'build/' matches only below its directory), truncation, cancellation, dockerignore returns nothing. Keep trees tiny. Headless; Mac or Linux.

## Implementation notes

- `check_ignore_file_capped` is the entry point with its own cap, for the truncation test; `IgnoreReport` also carries `truncated` (over the cap, or part of the tree unreadable) and `cancelled`. An incomplete walk drops every finding that depends on it (unused, shadowed, a negation blocked only by what it matched); duplicates, syntax errors and a blocked literal parent stand.
- A line gets at most one finding: invalid, then duplicate, then unused, then shadowed.
- Shadowed needs every matched path overridden by a later line of the opposite effect, so `a.log` then `*.log` isn't reported.
- "Excluded by an earlier line" is decided from the whole file, as git decides a directory: `!src/a.txt` then `src/` is reported, and `src/`, `!src/a.txt`, `!src/` isn't.
- Patterns are rewritten before `GitignoreBuilder` where globset departs from git: braces are literal (so `*.{log,tmp}` reads as unused), bracket expressions take escapes and POSIX classes, and a name-less `!` or `/` matches nothing. `single_patterns_match_what_git_matches` holds this to `git check-ignore`.
- `.git/info/exclude` is based at the worktree. `<name>.dockerignore` and `.containerignore` are skipped along with `.dockerignore`.
- Known limits: other ignore files' rules, nested repositories and `core.ignoreCase` aren't modelled.
