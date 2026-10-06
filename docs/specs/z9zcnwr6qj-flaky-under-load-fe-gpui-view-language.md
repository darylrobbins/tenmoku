---
task_uid: "01m4938tbf90e77rz9zcnwr6qj"
task: "T126"
rev: 2
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "switching_language_mid_parse_discards_the_old_parse sends the Rust parse to the background through an injected always-over-budget clock (SyntaxDriver::with_clock), not text size; its text is at most ~4 KB; it still asserts is_parsing() before the switch, and the python language and a 'module' root after it"
    required: true
    manual: false
  - id: "C2"
    text: "No file outside crates/fe-gpui/src/view/language_tests.rs changes, apart from Regatta's own spec file under docs/specs/ (written by the harness, not the worker); no production code changes"
    required: true
    manual: false
  - id: "C3"
    text: "Under 8×ncpu busy loops (own process group, killed after), the old test is shown failing and the fixed test passes at least 20 consecutive runs; commands, pass count and per-run wall times are recorded in the hand-off"
    required: true
    manual: false
  - id: "C4"
    text: "All of fe-gpui view::language_tests pass"
    required: true
    manual: false
---

# Flaky under load: fe-gpui view::language_tests::switching_language_mid_parse_discards_the_old_parse overruns fe-syntax's 5 s full-parse cap on its 200 KB Python parse

# A loaded machine pushes the test's 200 KB Python parse past fe-syntax's 5 s cap: fix the test

## Symptom

`fe-gpui view::language_tests::switching_language_mid_parse_discards_the_old_parse`
panicked at `crates/fe-gpui/src/view/language_tests.rs:78:34` ("parsed":
`syntax.tree()` was `None` after `run_until_parked`), after 5.144 s. This happened in
post_merge run `01m490xybdh1vaqd680qes4eyp` (3 lanes building), and earlier in T94's gate
at load ~50. The rerun on the same commit passed.

## Cause: the test's input, not production

- The test replaces the buffer with `"fn f() {}\n".repeat(20_000)` (200 KB), then
  switches to Python. The Python driver has no tree, so its first sync starts a
  from-scratch background `ParseJob`.
- `ParseJob::run` (`crates/fe-syntax/src/syntax.rs:690`) gives a full parse a
  wall-clock `FULL_PARSE_CAP` of 5 s (`syntax.rs:59-60`). On overrun it returns
  `Parsed { tree: None }`, and `Syntax::finish` (`syntax.rs:292-298`) gives up and
  shows plain text. That is the documented runaway-parse guard (T47) and is working
  as designed.
- In a debug test binary, Python's error recovery over 200 KB of Rust takes about
  0.4 s on an idle machine. The test takes 0.45 s; the same-size Rust parse in
  `tests::highlighting_follows_edits` takes 0.05 s. A busy machine stretches the
  parse past 5 s.
- GPUI's TestDispatcher runs `background_spawn` work on the test thread, so
  `run_until_parked` does wait for the parse. Nothing is lost or misordered. The
  `ParseSpawner` language check (`crates/fe-gpui/src/view.rs:458-466`) and the task
  dropped by `set_language` both behave correctly.
- Reproduced at main a7975bd with the prebuilt test binary:
  - idle (load ~60 from lanes): 5/5 pass in 0.44-0.59 s;
  - 56 busy loops (4×ncpu): 12/12 pass in 3.2-4.4 s;
  - 112 busy loops (8×ncpu): 17/17 FAIL at `language_tests.rs:78:34` in 5.07-5.48 s,
    which is the cap plus setup.

The text is only that big so that the Rust reparse overruns the view's 1 ms
`SYNC_PARSE_BUDGET` (`view.rs:22`) and goes to the background. The test asserts that
with `is_parsing()`.

## Fix (test only)

In `crates/fe-gpui/src/view/language_tests.rs`, make the Rust parse go to the
background without relying on size or wall-clock time, then shrink the text:

- Install the Rust driver with a clock that is always over budget. Build it with
  `syntax_driver(..)` (private in `view`, visible to this child module) and
  `.with_clock(..)`, moving it on by a second at each read as
  `crates/fe-syntax/src/driver/tests.rs:54-63` does. Assign it to `view.syntax` before
  the edit.
- Use a few hundred rows (for example `"fn f() {}\n".repeat(200)`, about 2 KB). That
  is enough for tree-sitter to consult the progress callback, so the overrun is
  deterministic. Keep the `is_parsing()` assert so a silent sync parse fails loudly.
- Keep what the test checks: after `set_language(Some("python"))` and
  `run_until_parked`, the language is Python and the tree's root is `module`.

No production code changes. If a fix is found to need one (for example lifting the
parse caps under a test feature, as `QUERY_BUDGET` does with `no-query-budget`), stop
and block. That would be a scope change, not part of this chore.

## Proof

There is no regression test: the cause is a timing assumption in the test. Prove the
fix with repeated runs under load, and record the commands and results in the
hand-off:

1. Build the fe-gpui test binary first (`cargo test -p fe-gpui --lib --no-run`). Then
   start the load: 8×ncpu busy loops in their own process group, killed as soon as the
   runs finish. Keep the whole window under about 3 minutes.
2. Show the old test fails under that load (at base, or by noting this spec's 17/17).
   Then run the fixed test at least 20 times under it: all must pass. Record each
   run's wall time.
3. Run all of `view::language_tests` once, idle, to show nothing else changed.
