---
task_uid: "01m6vmg6jeg3xc0yh9n3r1vpgs"
task: "T1507"
rev: 1
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "docs/decisions/\u003clane\u003e-freeze.md ranks the causes with file:line evidence, each said to be upstream gpui-pre 0.3.9 / gpui-pre-macos 0.3.9 or ours, and explains why a deactivate/reactivate unsticks it: macOS display link start/stop (gpui-pre-macos window.rs start_display_link/stop_display_link, window_did_change_occlusion_state, window_did_change_key_status, request_frame_callback take/restore), on_request_frame throttling (inactive, thermal) in gpui-pre window.rs, and the fe-gpui frame chains (minimap.rs request_animation_frame and sweep_asked, element.rs wrap_pending, view.rs press_when_settled)"
    required: true
    manual: false
  - id: "C2"
    text: "If a cause in this repo is confirmed, the decision file says so and proposes the fix as a bugfix card with its regression test; if upstream, it proposes a narrow workaround (e.g. restarting the display link on a stalled frame request) as a card. No speculative code change on this card."
    required: true
    manual: false
---

# Investigate: Tenmoku stops drawing and taking input while scrolling until its window is deactivated and reactivated

User report (2026-11-07), Tenmoku 0.1.1 release build at main 18700000 launched as `tenmoku .` from a terminal, machine under heavy build load (load avg 40-90): "The editor is frozen", while scrolling; "couldn't change files or scroll"; it "came back to life when I switched away from the window and back". No crash or hang report, nothing in the unified log.

Lead's static read (unconfirmed): most likely the macOS CVDisplayLink was stopped (stale occlusion state, or no screen during a display change: gpui-pre-macos-0.3.9 window.rs:836-851, :2993-3005) or request_frame_callback was left None (window.rs:3325-3362), and key-window activation (window.rs:3148-3217) forces a frame and restarts the link. GPUI is unforked crates.io gpui-pre 0.3.9 (Cargo.toml:57); its sources are under ~/.cargo/registry/src. Confirm or refute; do not assume. If the user captures a `sample` of a frozen window, it will be attached to this card.

Read code only: ask the lead before any cargo build, and do not launch the app.
