---
task_uid: "01m5gg6byjfshjfgy269x1qwez"
task: "T217"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "quitting_waits_for_slow_servers no longer asserts a wall-clock bound that load can break: it proves 'quitting waits through the shutdown timeouts without real sleeping' from the test clock (s.clock) / QuitDriver steps instead of Instant::now(), keeping the shutdown, exit and kill assertions"
    required: true
    manual: false
  - id: "C2"
    text: "lsp_tone_follows_a_same_text_message waits until the workbench has admitted a server for a.rs (advancing the manual and executor clocks as the stack 46 fix did for lsp_semantic_tokens setup, crates/fe-workbench/src/tests/lsp_semantic_tokens.rs) before force_lsp_status, so lsp.rs:1844 expect(\"the buffer has a server\") cannot fire"
    required: true
    manual: false
  - id: "C3"
    text: "Proof under load: both tests pass 30 runs with --retries 0 while the machine is loaded (e.g. 24 yes burners), via cargo nextest run -p fe-workbench -E 'test(quitting_waits_for_slow_servers) | test(lsp_tone_follows_a_same_text_message)'; cargo nextest run -p fe-workbench passes"
    required: true
    manual: false
---

# Flake: fe-workbench tests::lsp::quitting_waits_for_slow_servers (4 s wall-clock bound hit 6.06 s under load) and tests::status_bar_icons::lsp_tone_follows_a_same_text_message (asks for the buffer's server before it is admitted) (Mac T1082)

Test-only flake fix (no regression test: a test fix can't fail at base on demand; prove it with repeated runs under load).

Seen 2026-10-22 00:2xZ on the daemon's main verify of Mac main 2277f74c (run 01m5gea5rgbf8pn699wajxf0mq, test stage, 4736/4738 passed, load ~60-150, fresh build):
- tests::lsp::quitting_waits_for_slow_servers (crates/fe-workbench/src/tests/lsp.rs:948): `assert!(started.elapsed() < Duration::from_secs(4))` failed at 6.06 s. The timeouts it waits through run on the test's clock and QuitDriver, so wall-clock time only measures machine load. Assert on the clock/driver instead (e.g. that the driver advanced through both shutdown timeouts and no real sleep happened), or drop the bound if the driver already proves it.
- tests::status_bar_icons::lsp_tone_follows_a_same_text_message (crates/fe-workbench/src/tests/status_bar_icons.rs:163): panicked in 0.56 s at src/lsp.rs:1844 `expect("the buffer has a server")`. It waits for the fake server's didOpen, but the workbench admits the buffer's server through admit_once_known's poll on the manual/executor clocks, which the test never advances: the same class as the stack 46 fix (2277f74c) for lsp_semantic_tokens / lsp_inlay_hints. Wait for admission (advance both clocks in an until-loop) before force_lsp_status.

Pure fe-workbench GPUI test code; runs on Linux.
