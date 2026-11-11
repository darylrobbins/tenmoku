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
