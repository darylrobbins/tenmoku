# fe-workbench's code lenses (i18n design, T1550): `workbench-lens-…`.

## The test lenses (test_lens.rs): a row above each test, set in the
## design's UI small. The triangle is the run glyph.

# Above a test function: a click runs it.
workbench-lens-run-test = ▷ Run test
# Above a test module or class: a click runs its tests.
workbench-lens-run-tests = ▷ Run tests

## A language server's lenses (lsp_lenses.rs): their titles are the
## server's. What an info toast says when a click can't run one.

# $title is the lens's title, as the server gave it.
workbench-lens-unsupported = { $title }: this command isn't supported
# $title is the lens's title; $error is what went wrong, as the server or
# the client said it.
workbench-lens-failed = { $title }: { $error }

## The test history on a test's lens (test_lens.rs): plain text after
## Run test, from the test's last 8 results.

# $count is how many of its last results there are (at most 8); all passed.
workbench-lens-passed = { $count ->
    [one] passed in the last run
   *[other] passed in the last { $count } runs
}
# $count is how many of its last results there are; all failed.
workbench-lens-failed-all = { $count ->
    [one] failed in the last run
   *[other] failed in the last { $count } runs
}
# It has failed in its last results and its last run passed: set in the
# warning colour.
workbench-lens-flaky = flaky
# Some of its last results failed and some passed, the last one failed:
# $failed of $count (at least two) failed.
workbench-lens-failed-some = failed in { $failed } of the last { $count } runs
