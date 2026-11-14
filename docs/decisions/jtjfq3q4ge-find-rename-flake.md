# T651: find_rename a_server_starting_later_enables_rename_symbol flake

**Cause (C1).** The test trusted the root and waited for Rename symbol without waiting for the server to initialize. The find bar probes every frame; a probe sent before the fake answers `initialize` waits for capabilities with `clock.sleep(POLL)` (fe-lsp manager.rs), and the harness manager clock is the never-advanced `ManualClock`, so it sleeps forever. The bar keeps `(site, None)` and only asks again on a status change, and the status stays `Starting`, so no second probe comes. Production is fine.

**Fix.** The test now waits for the fake's `didOpen`, then waits for Rename symbol through `wait_rename_symbol`, which advances `s.clock` by 20 ms only while something sleeps on it (RENAME_TIMEOUT is never reached). `Lsp::until` and the tests/lsp.rs helpers are unchanged.

**C2.** New test `a_probe_before_the_server_initializes_is_driven_to_rename_symbol`: `hold_launches_late(true)`, `set_trust(true)`, `wait_launch_held()`, draw, `release_one_launch()`, then wait for Rename symbol. With the clock advance removed (and the 60 s bound temporarily cut to 8 s) it fails with "timed out waiting for Rename symbol"; restored afterwards.

**C3.** 20 runs of the fe-workbench `find_rename` tests (10 tests) with a parallel `cargo build -p fe-markdown` in a separate target dir, at load ~40: 20/20 passed.
