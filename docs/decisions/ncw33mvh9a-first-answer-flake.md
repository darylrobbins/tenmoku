# T641: first_answer flake — decision

## Cause

`primary_non_empty_answer_wins_even_when_add_on_is_first` delayed the
primary's reply by 500 ms on the manual clock and called
`clock.wait_for_sleep(500ms)` to know the delay was armed. 500 ms is also
`readiness::SETTLE`, which each server arms after `initialize`. Under load
the wait could match a settle timer instead; `advance(500ms)` then fired it
before the fake's reply delay was armed. The delay armed afterwards, at
now+500 ms, nothing advanced the clock again, and the test failed with
"no answer in 10s".

## Fix (test only)

The delay is now 700 ms, with `assert_ne!(late, SETTLE)`. No production code
changed.

## Evidence

Run on helium, load average 17-28:

- Before: with three copies of the fe-lsp unit suite running alongside, 1
  failure in 100 runs of the test (`no answer in 10s`, tests/mod.rs `answer`).
- After, 200 runs, 0 failures, alongside three copies of the fe-lsp unit
  suite. (An earlier version of this note also named `cargo test -p fe-lsp
  --test real_server`; that skips all seven tests, which are `#[ignore]`d,
  so it counted for nothing. The next run replaces it.)
- After, with the integration suite really executing: servers installed with
  `rustup component add rust-analyzer`, `mise install` and `npm ci` under
  tools/lsp (as scripts/lsp-real.sh does). The suite ran in the background:
  `FE_LSP_REQUIRE_SERVERS=1 target/debug/deps/real_server-* --ignored`
  (7 tests: rust-analyzer x4, pyright, typescript-language-server,
  typescript-go; all 7 passed, so no server was missing). At the same time,
  one more copy of the fe-lsp unit suite ran, and the test was run 20
  consecutive times with `fe_lsp-* --exact <test>`: 20 runs, 0 failures. The 20
  runs ended while the integration suite was still running. Load average
  27-37.
