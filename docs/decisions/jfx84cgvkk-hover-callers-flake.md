# hover_callers flake: the footer test blocked the thread that sends the request (T382)

`the_footer_fills_late_without_moving_the_card` waited for `callHierarchy/incomingCalls`
with `fake.wait_for`, which blocks the test thread. The request is sent from a
foreground task that runs only while the test draws or pumps the executor. Once
the hover was up, whether the request had already gone out depended on the
prepare reply landing before the last `until` frame. Under load it did not, and
the blocking wait then sat until the fake's 60 s timeout with nothing driving
the executor. This was a test bug, not a race in `lsp.rs` or `hierarchy.rs`.

Fix: the test now waits with `s.until(..., requests(s, INCOMING) > 0)`, which
draws each iteration. It was the only `wait_for` in hover_callers.rs.

Proof: 20 consecutive runs of `cargo test -p fe-workbench --lib hover_callers`
(8 tests each) alongside a `cargo build -p fe-config` in a separate target dir
all passed, each in about 0.6 to 1.2 s.
