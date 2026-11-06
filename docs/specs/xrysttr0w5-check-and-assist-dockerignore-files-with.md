---
task_uid: "01m6rjkrba1tbhb2xrysttr0w5"
task: "T473"
rev: 1
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "patterns_are_anchored_and_cleaned: '*.log' matches a/top.log never, only top-level logs; '**/*.log' matches at any depth; '/dist', './dist' and 'dist/' are Duplicate of each other"
    required: true
    manual: false
  - id: "C2"
    text: "go_match_syntax_and_errors: '[^a-z]' negates, '[abc' and a trailing backslash are Invalid, a lone '!' is Invalid, '**' matches zero or more directories"
    required: true
    manual: false
  - id: "C3"
    text: "exception_reincludes_inside_excluded_directory: 'docs' then '!docs/README.md' reports no 'unable to re-include' finding and README.md counts as included"
    required: true
    manual: false
  - id: "C4"
    text: "unused_and_shadowed_use_docker_order: last match wins; 'build' with nothing named build is Unused; a line fully overridden by a later '!' line is Shadowed"
    required: true
    manual: false
  - id: "C5"
    text: "container_files_share_the_rules: .dockerignore, Dockerfile.dockerignore and .containerignore are checked with the Docker pass, .gitignore still with git's"
    required: true
    manual: false
  - id: "C6"
    text: "ignore_check::dockerignore_is_checked_with_docker_rules: the updated former dockerignore_is_not_checked test passes"
    required: true
    manual: false
  - id: "C7"
    text: "ignore_diagnostics::dockerignore_tests::diagnostics_and_completion_cover_dockerignore: .dockerignore gets diagnostics and a no-leading-slash path completion; a Rust buffer gets none"
    required: true
    manual: false
  - id: "C8"
    text: "cargo nextest run -p fe-project -p fe-workbench and cargo clippy -p fe-project -p fe-workbench --all-targets -- -D warnings pass; fe-project still has no GPUI dependency and no new crate is added"
    required: true
    manual: false
---

# Check and assist .dockerignore files with Docker's own matching: build-context-relative anchored patterns, Go filepath.Match plus **, ! re-includes inside excluded directories (Mac T1393)

Lent from Mac T1393 (lend y41, 2026-11-06). Cut it back as-is with no Mac numbers in a helium batch.

## Problem
T1363-T1366 (highlighting, engine, diagnostics, assist; landed as T435-T438) cover .gitignore-style files and deliberately skip .dockerignore (and Dockerfile.dockerignore, .containerignore): crates/fe-project/src/ignore_check.rs `has_docker_syntax` returns an empty report, and crates/fe-workbench/src/ignore_diagnostics.rs `is_ignore_file` / `has_container_syntax` turn the diagnostics and the assist (ignore_assist.rs asks `is_ignore_file`) off. The user wants the same help in .dockerignore with Docker's own rules, because applying git's would mislead.

## How Docker matches (moby/patternmatcher, as `docker build` reads the file)
- Paths are relative to the BUILD CONTEXT, not to the file's directory per git's nested files. There are no per-directory .dockerignore files: one file per context. The context is the directory holding the file for `.dockerignore`, and for `<name>.dockerignore` (read beside the Dockerfile named `<name>`) the same directory as the best guess (the real context is a `docker build` argument we cannot see; say so in the report doc comment).
- Each line is trimmed of surrounding whitespace (git keeps escaped trailing spaces), blank lines and lines starting with `#` are skipped, then the pattern is cleaned with Go's filepath.Clean: a leading `/` and `./` are removed (`/dist` == `dist`), a trailing `/` is dropped (`build/` also matches a file called build), `a//b` collapses.
- EVERY pattern is anchored at the context root: `*.log` matches only top-level logs, `**/*.log` matches at any depth. There is no "no slash means any depth" rule.
- Matching is Go's filepath.Match per path segment: `*` any run of non-`/`, `?` one non-`/` char, `[a-z]` classes with `^` for negation (git uses `!`), `\c` escapes; plus `**` matching any number of directories including none. A malformed pattern (unclosed `[`, trailing `\`) is an error at build time ("syntax error in pattern"); a lone `!` is an error too ("illegal exclusion pattern").
- A path is excluded if it, or any parent directory, matches; the LAST matching pattern wins, and a `!` exception re-includes a path even inside an excluded directory (git cannot; Docker walks into the directory when an exception could re-include something). So the git engine's "unable to re-include below an excluded directory" error and its `Blocked` logic must not apply.
- Matches are case-sensitive. `Dockerfile` and `.dockerignore` stay sent to the daemon even when excluded (a line excluding them still "matches" files; it is reported unused only if nothing matches).

## Change
- crates/fe-project/src/docker_ignore.rs (new; declared `mod docker_ignore;` in lib.rs): a Docker matcher: `clean(pattern)`, `matches(pattern, rel_path)` implemented as Go's Match plus `**`, hand written or with the `regex` crate if fe-project already depends on it (it does not today: do not add a dependency for it; a segment-wise matcher with `**` backtracking is enough), and the Docker walk order (last match wins, parent match excludes, `!` re-includes).
- ignore_check.rs: `check_ignore_file_capped` drops the early empty-report return for container names and runs a Docker pass producing the same IgnoreReport: Invalid for the malformed patterns above and a lone `!`; Duplicate after cleaning (`/dist`, `dist/` and `./dist` are the same line); Unused (nothing in the context matches; same entry cap, cancel flag and truncated rules as git's pass); Shadowed (a later line decides every path this one matches, the other way); a new Hint-level finding is NOT added. Keep `matches` (the hover counts) in Docker terms. Walk from the context root with ignore rules off as the git pass does, skipping `.git`.
- fe-workbench: ignore_diagnostics.rs `is_ignore_file` stops excluding container names (delete `has_container_syntax` and its doc), so diagnostics and ignore_assist.rs follow; ignore_assist.rs: path completion offers entries relative to the context and never inserts a leading `/` for Docker files (it is stripped, not an anchor); hover says "matches N files" with Docker's counting.
- Highlighting (crates/fe-syntax) needs no change; the `[^a-z]` negation class already colours as a class. No change to language.rs.

## Tests
- crates/fe-project/tests/dockerignore_check.rs (new file): Docker's rules above, one test per bullet.
- Update the three existing tests that pin the old behaviour, in place: crates/fe-project/tests/ignore_check.rs `dockerignore_is_not_checked` (now: a .dockerignore with `nothing_here/` reports Unused, and `x` twice a Duplicate), crates/fe-workbench/src/ignore_diagnostics_tests.rs `dockerignore_and_other_languages_get_none` (the Rust-buffer half stays), crates/fe-workbench/src/ignore_assist_tests.rs `comments_dockerignore_and_other_buffers_are_untouched` (the comment-line and Rust-buffer halves stay).
- New workbench test file crates/fe-workbench/src/dockerignore_tests.rs declared from ignore_diagnostics.rs (add that file to scope) as `#[cfg(test)] #[path = "dockerignore_tests.rs"] mod dockerignore_tests;`, as ignore_diagnostics_tests is: diagnostics appear for .dockerignore and not for a Rust buffer; completion from a Dockerfile.dockerignore offers the context's children without a leading slash.

## Implementation notes

- Docker pass lives in `docker_ignore.rs` (own matcher, no new dependency; `**` follows moby's regexp translation). Container names are matched case-insensitively.
- `Dockerfile.dockerignore` and `.containerignore` are checked by fe-project, but fe-syntax's language table (language.rs, out of scope) only detects `.dockerignore`, so the others get diagnostics in the editor only once their language is picked by hand.
- Fixed carried T438 F1 (escaped `/` in typed paths). The T436 F1-F5 findings concern the git pass and were not changed here.
