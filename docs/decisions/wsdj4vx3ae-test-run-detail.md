# T1576: Test run detail in the Runs panel

Spec: `docs/specs/wsdj4vx3ae-test-run-detail-split-bar-test-tree-by.md`;
design: `docs/design/design-system.md#runs-builds-tests-and-agent-tasks-in-one-list`
("a split bar (green passed, red failed, track remaining), a test tree, and
each test's last 8 results as small bars, so flaky tests stand out"; keys
`␣tf` rerun failed).

## What changed

- **`crates/fe-workbench/src/test_run_view.rs`** (new). The detail of the
  test run selected in the Runs list. The Runs panel hosts it: beside the
  list, which keeps two fifths of the width, when the panel was last drawn
  600 px wide or more, else under a list three rows tall. A canvas measures
  the panel each frame and redraws it when it crosses the line.
  - A title row (the run's title in code small, its counts muted), then the
    **split bar**, 4 px: `status.success` (celadon) for passed,
    `status.error` (shinsha) for failed, and the track for the rest, in
    proportion to the summary's `total`. Until the total's known it uses
    the tests ended so far. Ignored tests fall in the track: they neither
    passed nor failed.
  - The **test tree**, a kiln-ui `List` (arrow keys, Enter, screen-reader
    options), 24 px rows. Modules are the test's binary plus its name's path
    before the last `::`, joined by `::` (`p` + `tests::adds` → `p::tests`;
    an integration test `more` + `subtracts` → `more`). A doc test's name is
    a file and an item, not a path, so it sits in `Doc-tests p` whole.
    Modules come in the order their first tests did. One with a failed test
    starts expanded, the others collapsed. A click on a module's row, Enter,
    or right and left open and close it. The toggles last while the run
    stays selected.
  - A test row: indent, glyph (✓ `Check` celadon, ✕ `X` shinsha, ▲
    `Triangle` kiseto for a flaky test, a dash `Minus` muted for ignored),
    the test's name within its module in code small, its duration at the
    right (`4 ms` under a second, `1.3 s` above), then eight 3 x 10 px bars
    1 px apart. They're the last 8 results, oldest left, newest right. The
    runs a test hasn't had yet are empty at the left, so every row's newest
    bar lines up. Passed bars are celadon, failed shinsha, empty the track.
    A flaky test (T1574's `History::flaky`, both a pass and a fail in its
    window) has a 1 px `status.warning` (kiseto) outline round its bars.
    Others have a transparent one, so the bars line up.
  - A failed test's glyph is ✕ even when it's flaky; ▲ is for a flaky
    test that passed. T1574's `flaky` is a pass and a fail in the window,
    so a test that has just broken after seven passes counts as flaky. A
    ▲ there would hide a fresh regression (the first audit capture showed
    one). Its kiseto-outlined bars still say flaky, and so does its
    accessible name ("tests::x, failed, flaky, …").
  - **Detail slot.** Selecting a test's row shows a 24 px strip under the
    tree with its glyph, full name and binary (`TestRunView::selected_test`).
    The failure detail is the next card's.
- **The track** is `border.control`, kiln-ui's off-switch track.
  `border.subtle` is a 50% mix of `border.default` into the background,
  nearly invisible on Yohen's panel (#251d18 on #17120f).
- **Last results** come from the root's history, read through the same
  queue the writes go through (`test_runner::test_tracks`). A read queued
  after a run ended sees that run's results. The view reads once when a run
  is shown, and again once it's seen the run end. It starts the read from a
  spawned task, after the update that ended the run, by which time
  `TestRun` has queued its write.
- **`runs.rerun_failed`** ("Runs: Rerun Failed Tests", `␣tf`). It reruns
  the run selected in the Runs list if that's a test run, else the newest
  test run. The failed tests' names go each once, in the order they failed:
  the run's command up to its `--`, then `-- --exact <names>`. `cargo test`
  takes one `TESTNAME` before `--`, and a test binary takes any number of
  filters after it. The filters the first run had after `--` (a file run's
  module path) are dropped: libtest ORs filters, so they'd run every test
  under the module again. A rerun keeps the base command, so rerunning a
  rerun's failures uses the original `cargo test -p … --lib`, not
  `--exact`ed twice. Its title is "Rerun failed: <title>". Files are saved
  and trust is checked as for Test File / Test Project (`Scope::Failed`
  through `start_test_run`). A toast says when there's no test run, or no
  test failed in it.
- **`␣tf`.** `space t f` in `fastedit.toml`, the same context as `␣r`
  (`editor && vim.mode == normal && !vim.pending`). Space `t` then any
  other key comes back `Unmatched` with all three keys, for vim.
- **`test_runner.rs`.** A `TestRun` keeps its plan's base spec and whether
  it ran under nextest, for reruns. `Workbench::record_test_run` adds a
  finished test run with given results and last results, as a runner other
  than the workbench's ran it: no process, no history write, no rerun. The
  audit scene and the view's tests use it. `write_history` became
  `queue_history_job`, as reads go through it too.
- **`Workbench::select_run`** (runs_panel.rs) selects a run in the Runs list
  as a click would. The audit scene needs it, since the panel is private.

## Review round 1 (d4hxes)

- **F1, filters kept in a rerun.** A run's rerun base (`Plan::base`) is
  now its command with no tests picked (`unfiltered`): the subcommand
  (`test`, or `nextest run`) and its options. The name filters before
  `--`, such as a Test File run's `util::`, are dropped, along with
  everything after `--`. Nextest applies `--exact` only after `--`, so a
  kept filter would pick every test under the module.
- **F2, names across binaries.** A rerun picks each test by its binary as
  well as its name.
  - **Under nextest:** one run with `-E`, each test as
    `(binary_id(=…) & test(=…))`, joined by `|` (`filterset`). The
    matchers escape `\`, `)`, `,` and `/` (nextest's filterset
    reference). `TestId.binary` is nextest's binary id already.
  - **Under `cargo test`:** cargo can't tie a name to a binary, so each
    failing binary gets a run of its own (`reruns`). Its title names the
    binary when there are several ("Rerun failed: <title> (more)").
  - **Picking the target:** the test sink records each `Running` line's
    binary and source file (`running_source`). The file picks the cargo
    target (`source_target`):
    - `src/main.rs` or `src/bin/…` → `--bins`; other `src/…` → `--lib`;
    - `tests/x.rs` or `tests/x/main.rs` → `--test x`, likewise
      `--bench` and `--example`. A name declared in the manifest is the
      artifact's name, used when it differs from the file's stem.
    - Doc tests → `--doc`.
  - **Picking the package:** the source file's folder up to `src`/`tests`/…
    has the manifest whose `[package] name` is passed with `-p`. It
    replaces the run's `-p`/`--workspace`.
  - **Fallback:** a binary no `Running` line named keeps the base's
    selection.
  - **Limit:** a library and a binary of one package with one name are one
    binary in the output (`TestId.binary` has no hash), so both are rerun
    (`--lib --bins`).
- **F3, another folder's run.** `runs.rerun_failed` on a run whose root
  isn't the one open is refused with a toast: "This run is from another
  folder: open it again to rerun its failed tests". Its results would
  otherwise be recorded in the open folder's history.
- **F4, history for tests that come later.** The view asks the history
  about each test as it appears (`requested`), one read at a time. When a
  read ends it asks about those that came meanwhile. Once the run has
  ended it asks about all of them again, with the run's own results in.
  Reads come back in the order they were queued, so a later read is
  newer. A read for a run no longer shown is dropped.
- **F5, opened modules.** Expansion is an explicit choice per module
  (`opened: HashMap<String, bool>`), the failure default only when there's
  none. A module opened by hand stays open when its first test fails, and
  its selection is kept.

New tests: `rerun_failed_runs_exactly_the_failed_names` now runs a fake
`cargo` that filters like cargo (`FILTERING` in the test file). It covers
`p`'s and `more`'s identically named `util::tests::shared`, the failing one
rerun alone with `-p p --lib`, failures in two binaries as two runs, and
the filterset's escapes. Also new: `a_filtered_nextest_run_reruns_only_its_failed_test`
(F1), `a_run_from_another_folder_isnt_rerun` (F3), and
`a_live_run_reads_history_as_tests_come_and_keeps_open_modules`, which
covers F4 and F5 with a gated run.

## Review round 2

Round 1's `cargo test` targeting guessed each binary's target from its
`Running` line's source path. Round 2 showed where that fails:
- F2: `--bins` runs every binary, and same-named libraries, binaries or
  integration targets merge.
- F6: the paths are package-relative, so the package isn't found from the
  command's folder.
- F7: a target with a custom path or a renamed target is misread.
- F8: coloured status lines weren't parsed.

The rerun now uses cargo's own record of what it built.

- **`cargo test` runs add `--message-format json-render-diagnostics`.**
  Cargo then prints a `compiler-artifact` JSON message on stdout for each
  unit it builds; diagnostics stay human-readable on stderr. For each test
  executable the message gives `manifest_path`, `target.kind`,
  `target.name` (spelled as the manifest spells it) and `executable` (the
  file name, cargo's hash included). The sink records them, along with
  each library's crate name → manifest for doc tests (`CargoBinaries`).
  The JSON lines also land in the run's kept output; nothing shows that
  raw today.
- **Each result is tied to the executable it ran in.** The sink follows
  `Running` and `Doc-tests` lines and stamps each result with the current
  executable's file name, hash kept (`CargoBinaries::of_result`). The
  binary a `Running` line ends gets the events the parser gives for that
  line. ANSI escapes are stripped before matching (F8).
- **Reruns group failures by that executable**, so a library and a binary
  named `p`, or `p`'s and `q`'s `my-more` tests, are rerun apart. Each
  group runs as `-p <package>` plus exactly one target:
  - `--lib`, `--bin <name>`, `--test <name>`, `--bench <name>` or
    `--example <name>` from the artifact message;
  - `--doc` for `Doc-tests <crate>`, with the package found from the
    library's manifest.

  The `-p` replaces the run's `-p`/`--workspace`, and the package name is
  read from `manifest_path`. A failure whose executable cargo named no
  target for refuses the whole rerun with a toast ("Can't tell which
  target … was built from"). A broader selection would run more than the
  failed tests.
- A labelled rerun's title names its package and target the way cargo
  picks it: "Rerun failed: <title> (q --test my-more)".
- The other-folder check now runs before the target check.
- T1575's tests in `src/tests/test_runner.rs` assert the cargo command
  line, so they now expect the new flag (respecced into scope).

Tests run a fake `cargo` (`FILTERING` in `src/tests/test_run_view.rs`). It
models a workspace:
- `p`'s library and its namesake binary at `src/cli.rs`;
- `p`'s integration test `my-more` at `checks/more.rs`;
- `q`'s `my-more` at `tests/more.rs`;
- optionally a root package whose library is also `src/lib.rs`.

The fake picks packages, targets and tests as cargo does, prints artifact
messages, and can colour its `Running` lines.
- `rerun_failed_runs_exactly_the_failed_names` covers coloured output and
  three failures in three same-named-or-renamed targets: three reruns,
  `-p p --bin p`, `-p p --test my-more` and `-p q --test my-more`, each
  executing only its failed test. It also checks a library failure
  (`--lib`) and the doc-test mapping.
- `a_member_run_beside_a_root_package_reruns_in_the_member` covers F6.
- `a_rerun_that_cant_be_exact_is_refused` covers unknown targets and
  another folder.

## Review round 3

- **F2, doc tests of same-named libraries.** `CargoBinaries::libraries`
  maps a crate name to its manifest, or to none once a second package's
  library claims the name. Cargo's `Doc-tests <crate>` line can't say whose
  doc tests ran, so such a failure refuses the rerun with the
  unknown-target toast.
- **F9, one identity for display, history and reruns.** A `cargo test`
  result whose executable cargo described gets the binary name nextest
  gives it, in place of cargo's file name without its hash:
  - `p` for a library;
  - `p::bin/p` for a binary;
  - `p::my-more` for an integration test;
  - `p::bench/x` and `p::example/x` for benches and examples.

  The package comes from the artifact message's `package_id`, in either
  spec form, or cargo's pre-1.77 `name version (url)`. Rows, modules,
  selection and the history's `TestId`s are therefore separate for a
  library and a binary named `p`, and for two packages' `my-more`. They
  also match what a nextest run of the same tests records. Doc tests keep
  `Doc-tests <crate>`. Output that names no target (an old cargo, or the
  T1575 fixture) keeps cargo's name, as before.
- **F10, status lines in test output.** A `Running` line counts only when
  its status word is right-aligned in cargo's 12 columns, as
  `LibtestParser`'s `cargo_status` reads it. A result's executable is
  found through the parser's own binary for it: the latest executable
  whose name without its hash is that binary, or the `Doc-tests` name.
  So a test that prints `Running …` can't move its own or later results
  to another target. Where the line is aligned, the parser ends its
  failure block and switches binary too, and both agree.

Tests:
- `rerun_failed_runs_exactly_the_failed_names` now also checks the
  workspace's tree: separate modules for `p`, `p::bin/p`, `p::my-more`
  and `q::my-more`. It checks that selecting the binary's `shared`
  selects that result, and that the history has `p`'s `shared` passing
  and `p::bin/p`'s failing.
- It also covers a failure whose captured output has unaligned
  `Running`/`Doc-tests` lines and a misaligned `Running` line naming
  another `p` executable. That failure stays the library's and reruns
  with `--lib`.
- Two libraries named `my_lib` refuse a doc-test rerun (`reruns` returns
  the unknown-target message).

## Scope added by respec

`lib.rs` (the module and its `init`), `test_runner.rs` (rerun plans, kept
specs, recorded runs, history reads), and `examples/screenshots/audit_scenes.rs`
(the scene's `mod` and `SURFACES` lines), `src/tests/test_runner.rs` (round
2's cargo flag in T1575's asserted command lines). `src/tests/mod.rs` declares the
tests module; T1829 held it while this card was built, so it was respecced
in once T1829 released it.

## Audit scene (C4)

`examples/screenshots/audit/runs_tests.rs` records a passed build and a
test run of ten tests: eight passed, one failed, one ignored. It runs
`runs.show` and selects the test run. Its probes check the split bar's
three parts, an empty and a passed bar, a failed newest bar, the flaky
test's kiseto outline, and the panel under the tree.

Recorded on the Mac, 2026-11-14, at commit b075fd9f3 (with the one-line
`src/tests/mod.rs` declaration applied locally), and again after review
round 1, 2026-11-15, at 241c43ce0, with the same output:
`cargo run -p fe-workbench --example screenshots -- audit target/ui-audit`
wrote `runs_tests_dark.png` and `runs_tests_light.png` (Yohen and Hakuji)
and printed `ok runs_tests_dark probes (8)` and `ok runs_tests_light probes
(8)`. T1571's `runs` scene still passes its 3 probes in both.

Both captures show the list at two fifths with the test run selected, and
the detail beside it. The title row reads `cargo test -p demo` and "8
passed, 1 failed of 10". Below it the split bar runs celadon to eight
tenths, shinsha for a tenth, and the track for the ignored tenth.
`demo::count` is open with ✕. Its tests have durations (4 ms, 12 ms, 2 ms,
31 ms) and eight bars each:
- `counts_words`: six empty, then two passes.
- `counts_lines`: ✕, seven passes then a failure, outlined.
- `empty_input`: ▲ in kiseto, outlined.
- `unicode`: three empty, then five passes.

`demo::split` and `demo::parse` are closed with ✓ and "3 tests".

A first capture, before the glyph change above, showed `counts_lines` as ▲.
That's how the ✕-for-failures rule came about.

The run's other failures are the 22 that T1571's decision file recorded on
main: chrome, settings, shortcuts_editor, changes, git_log, git_log_card,
problems_panel, problems, terminal_session, terminal, and the yohen/hakuji
chrome probe. None is in the Runs panel.
