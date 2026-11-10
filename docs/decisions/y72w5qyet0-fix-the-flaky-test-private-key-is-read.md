# T568: private_key_is_read_from_stdin_only flake

The test helper `Fixture::run` in `crates/fe-registry/tests/signing.rs`
unwrapped the `write_all` to the child's stdin. If the binary exits before
reading all of stdin, the write fails with `BrokenPipe` under load. The helper
now tolerates `BrokenPipe`, still panics on other errors, and drops stdin
before waiting. Callers still assert on the exit status and output as before.
Only test code changed (C2).

## Evidence (C1)

`cargo test -p fe-registry --test signing private_key_is_read_from_stdin_only`
run 20 times in a row while 3 concurrent `cargo test -p fe-registry
--no-fail-fast` suites ran alongside, load average ~19-25: 20/20 passed.
