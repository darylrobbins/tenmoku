---
task_uid: "01m7dggygc7jz9atjtjfq3q4ge"
task: "T651"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "a_server_starting_later_enables_rename_symbol no longer hangs when the find bar asks before the fake answers initialize; the cause (the capability wait sleeping on the never-advanced ManualClock, with no later status change to ask again) is named in the decision file"
    required: true
    manual: false
  - id: "C2"
    text: "a new find_rename test holds the launch until the bar has asked, releases it and sees Rename symbol; it hangs without the fix (shown once, recorded in the decision file); Lsp::until and the other tests/lsp.rs helpers are unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "20 runs of the fe-workbench find_rename tests alongside a parallel cargo build all pass, recorded in the decision file"
    required: true
    manual: false
---

# Flake: fe-workbench find_rename a_server_starting_later_enables_rename_symbol hangs 60 s for Rename symbol when the bar asks before the server initializes (Mac T1604) (Mac T1873)

`fe-workbench tests::find_rename::a_server_starting_later_enables_rename_symbol` (landed with T1604, neon T345, 2ddef1f34) failed in a macOS stack gate on 2026-11-14 at load ~48: it panicked at crates/fe-workbench/src/tests/lsp.rs:530 "timed out waiting for Rename symbol" after 61 s. 1 of 5 targeted reruns at load ~20 hung the same way; passes take under 1 s (2.3 s at load 21).

Likely cause (test only, production is fine): the test trusts the root and waits for Rename symbol without waiting for the server to initialize. The find bar probes on every frame (find_bar.rs `rename_shown`), and once the document is placed on the starting server, `lsp_prepare_rename` (lsp.rs:2024) returns a future whose admission waits for the capabilities with `clock.sleep(POLL)` (fe-lsp manager.rs:2451-2456, POLL 20 ms). In this harness the manager's clock is the `TestClock` ManualClock (tests/lsp.rs:304), which this test never advances, so a probe sent before the fake answers `initialize` sleeps forever; RENAME_TIMEOUT is on the same clock. The bar keeps `(site, None)` and only asks again on a status change (`forget_find_renames`, lsp.rs:1842), but the status stays `Starting` after initialize (readiness waits for serverStatus or quiet time on the same clock), so no second probe comes. On an idle machine the fake answers initialize before the first probe is polled; under load it doesn't. The other find_rename tests wait for `didOpen` first, which is why only this one flakes.

Fix the test so the probe's capability wait is driven: for example wait for the fake's `didOpen` of src/main.rs after `set_trust(true)` (initialize answered), then advance `s.clock` by the 20 ms poll while a probe sleeps on it, inside the wait for Rename symbol. Do not change `Lsp::until` or other shared helpers in tests/lsp.rs: other tests rely on the manual clock standing still. Advance only as needed, so RENAME_TIMEOUT (10 s, same clock) is never reached while the fake is still answering.

Add a deterministic sibling in find_rename.rs: `hold_launches_late(true)`, `set_trust(true)`, `wait_launch_held()`, draw so the bar asks while the capabilities are unknown, `release_one_launch()`, then expect Rename symbol. Show once that it hangs without the clock drive, and record that in the decision file.

Prove it with 20 runs of the fe-workbench find_rename tests alongside a parallel `cargo build` of another crate, recorded in the decision file.
