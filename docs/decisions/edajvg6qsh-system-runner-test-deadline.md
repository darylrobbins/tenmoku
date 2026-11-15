# T661: the_system_runner_stops_a_descendant_holding_its_stdout deadlines

The printing and failing cases now use a 10 s `SystemRunner`; only the lingering
and hanging cases keep the 500 ms one (each still asserted to return < 5 s).
`SystemRunner` is unchanged.

The printing case also retries up to 3 times: a run under load still returned
None with the 10 s deadline (1 of 20), so the cause there was not the deadline
but a failed spawn (a sibling test forking while the script was open for
writing, i.e. ETXTBSY).

## Proof (C2)

20 runs of `fe_dap registry` tests (11 tests) with a `cargo build -p fe-lsp`
running in parallel, load average ~20 on 24 cores: 20/20 passed, twice in a row
after the retry (before it: 18/20 and 19/20).
