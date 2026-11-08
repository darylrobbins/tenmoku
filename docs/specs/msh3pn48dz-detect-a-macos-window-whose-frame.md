---
task_uid: "01m6w99eb8xsjsjpmsh3pn48dz"
task: "T1609"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "On macOS, while a window is visible or key, an on_next_frame probe left unacknowledged for 500 ms across two 250 ms polls is a stall, and triggers setNeedsDisplay on the window's view (displayLayer:). A probe pending at a nudge is consumed by it, and the episode recovers only after two consecutive fresh probes acknowledged without a nudge. Test: frame_watch::tests::stall_policy"
    required: true
    manual: false
  - id: "C2"
    text: "Nudges alternate with probe-only polls (at most 2 per second) and back off to one every 2 s after 8 in an episode. A begin record (unacknowledged time, visible, key, occlusionState Visible, screen present, display id) is logged at warn once per episode, and an end record only on recovery. Covered by stall_policy and the never-recovering test"
    required: true
    manual: false
  - id: "C3"
    text: "Headless tests with an injected nudge hook and simulate_next_frame: a healthy clean window never nudges; no input and no frame requests nudges within 600 ms; key-event draws without frame requests nudge; a key window reported Hidden still nudges; nudges that only acknowledge their own probe never end the episode; and resumed delivery ends it"
    required: true
    manual: false
  - id: "C4"
    text: "Compiled out on other platforms; GPUI stays unforked"
    required: true
    manual: false
---

# Detect a macOS window whose frame requests stop arriving, log it, and restart its display link through displayLayer: (T1507 follow-up 2)

Follow-up 2 of T1507 (docs/decisions/h9n3r1vpgs-freeze.md, "Card 2"). The design is in that decision file, which lands
with T1507. Read its "Card 2" and "What can still tell 1a, 1b and 1c apart" sections first.

**Why.** In the lead's sample of a frozen window (pid 62551, 2026-11-07), the main thread was idle and no `CVDisplayLink`
was running anywhere in the process. So the window got no frame requests.

gpui-pre-macos 0.3.9's `start_display_link` (`macos/window.rs:836-856`) can leave a window unsubscribed in three ways,
and nothing retries until the next occlusion change, screen change, `displayLayer:` or activation:
- a stale occlusion state;
- no screen;
- a failed or lost `CVDisplayLinkStart` on the shared per-display link (`macos/display_link.rs:191-294`).

This card is a narrow workaround and evidence gatherer. GPUI stays unforked.

**Behaviour (macOS only; compiled out elsewhere).**
- **When to watch.** While the window is visible or key. A key window is watched even when GPUI says it is hidden,
  because GPUI's visibility reads the same `occlusionState` bit that makes the stale-occlusion case give up. A window that
  is hidden and not key isn't watched. Input doesn't gate the watchdog, since the second freeze happened with no input.
- **Heartbeat.** A probe posted with `window.on_next_frame`. Only the platform frame callback runs next-frame callbacks:
  `step`, `displayLayer:` or activation, after any throttling (`gpui/window.rs:1864-1874`). Paints aren't counted, since
  key events draw synchronously without presenting (`gpui/window.rs:6009-6012`).
- **Stall.** A probe unacknowledged for 500 ms and across two consecutive 250 ms polls, timed with `dispatch_after` on
  the main queue.
- **Nudge.** `[ns_view setNeedsDisplay:YES]` on the window's view, from `HasWindowHandle for Window`. AppKit then calls
  GPUI's `displayLayer:`, which draws and presents synchronously, then runs `start_display_link`
  (`macos/window.rs:3321-3340`).
- **A nudge's own frame is not recovery.** `displayLayer:` acknowledges the pending probe before it tries to subscribe.
  - So a nudge consumes the probe pending at the time.
  - Recovery counts only fresh probes, posted after the nudge's frame, and acknowledged with no nudge in between.
  - The episode ends after two such probes in a row. Until then the episode, its nudge count and its backoff state are
    kept.
- **Pacing.** A nudge, then a poll with a fresh probe and no nudge, then a nudge again if that probe went
  unacknowledged. That gives up to 2 frames a second in a stall that won't clear. After 8 nudges in one episode, it backs
  off to one nudge every 2 s.
- **Log (warn).** One record when an episode begins and one only on recovery.
  - Begin: the unacknowledged time, visible and key, `occlusionState` `Visible`, whether `screen()` exists, and the
    display id.
  - End: the nudge count and how long the stall lasted.

  The state values are context, not attribution.
- **Limit.** If a dead shared link is still marked running and a second window is subscribed on that display, the
  restart doesn't stop the link. The nudge then only presents frames, and the log shows no recovery.
- **Scope extras.** `objc2` and `raw-window-handle`, macOS-only, both already in the lock.

**Tests (headless).** The test platform has no frame loop, so `Window::simulate_next_frame` stands for frame-request
delivery, and the nudge is an injected hook.
- `frame_watch::tests::stall_policy`, the pure policy:
  - not acknowledged for 500 ms but within one poll → no stall;
  - across two polls → stall;
  - acknowledged → no stall;
  - hidden and not key → no probe;
  - visible, or key → probe;
  - a probe pending at a nudge is consumed and not counted as recovery;
  - nudge, then a fresh probe with no nudge, then a nudge again;
  - back off after 8 nudges;
  - recovery only after two consecutive fresh probes acknowledged without nudges;
  - one begin record per episode, and an end record only on recovery.
- Healthy clean window: `simulate_next_frame` at each poll for 2 s → no nudge, no log.
- No input and no frame requests (the second freeze's shape): advance 600 ms → a nudge and a begin record.
- Key-event draws without frame requests: make the window dirty and dispatch keys, with no `simulate_next_frame`. Advance
  600 ms → a nudge.
- Stale hidden state: `TestWindow::simulate_visibility_change(Hidden)` on the key window, with no frame requests → a
  nudge.
- Nudges that never restart delivery: the hook calls `simulate_next_frame` once per nudge, and nothing else delivers a
  frame. Advance 10 s. Assert:
  - one begin record and no end record;
  - the nudge count rising at 2 per second, then backing off after 8;
  - no reset of the episode.
- Recovery: after a nudge, `simulate_next_frame` at each later poll → an end record after the second fresh probe, and no
  more nudges.

**Where the code lives (rev 3).** `scripts/check_kiln_boundary.py` allows OS cfgs only in `kiln/crates/kiln-core/src/platform`
and `kiln/crates/kiln-platform/src`, so the watchdog lives in kiln-platform:
- `kiln/crates/kiln-platform/src/frame_watch.rs` and `frame_watch/tests.rs` (tests stay `frame_watch::tests::*`, as
  kiln-platform lib tests);
- the macOS-only `objc2` and `raw-window-handle` deps in `kiln/crates/kiln-platform/Cargo.toml`, dropped from kiln-shell's;
- exported as `kiln_platform::frame_watch::watch_frame_requests`, re-exported with no cfg as
  `kiln_shell::watch_frame_requests` from kiln-shell's `lib.rs`, and called by `Root`.

kiln-shell keeps no OS cfg. Don't evade the boundary regex (for example with `target_vendor`). T1610 calls
`kiln_shell::watch_frame_requests`.

## Implementation notes

- **Entry point for T1610: `kiln_shell::watch_frame_requests(window: &mut Window, cx: &mut App) -> Option<Task<()>>`.**
  It's a re-export of `kiln_platform::frame_watch::watch_frame_requests`, and it works for any GPUI window, not only a
  kiln `Root`. Call it once per window as the window opens: in Tenmoku's `new_workbench` (`crates/fastedit/src/main.rs`),
  next to `window.follow_system_appearance(cx)`. Keep the task or `.detach()` it. The watch ends when the task is dropped
  or when the window closes (`frame_watch::tests::a_detached_watch_ends_when_its_window_closes`). It exists on every
  platform, so the call site needs no `cfg`. It returns `None` where the watchdog is compiled out (anything but macOS) and
  for a window with no AppKit view, such as GPUI's test windows.
- `Root::new` calls the same function and keeps the task. Rev 1 found that Tenmoku's windows aren't `Root`s, which is why
  T1610 exists.
- **Where it lives (rev 3).** `kiln_platform::frame_watch` (`kiln/crates/kiln-platform/src/frame_watch.rs`) holds only
  the public entry point. The implementation is the macOS-only private submodule `frame_watch/macos.rs`: the policy, the
  hooks and the AppKit calls. The tests are in `frame_watch/tests.rs`, compiled on macOS only, as
  `frame_watch::tests::*` (kiln-platform lib tests). kiln-shell has no OS `cfg`, and `objc2` and `raw-window-handle` are
  macOS-only dependencies of kiln-platform. `scripts/check_kiln_boundary.py` passes.
- Polls use GPUI's timer (`dispatch_after` under the macOS dispatcher) and run on the main thread through
  `window.spawn`. The test clock drives them headless.
- A fresh probe is posted only after the nudge's consumed probe has been acknowledged. Otherwise a late `displayLayer:`
  could acknowledge a fresh probe and look like recovery.
- **The AppKit path hasn't run in a real window** (accepted for rev 2). That covers `setNeedsDisplay:`,
  `occlusionState`, `screen` and `deviceDescription[NSScreenNumber]`. From Claude's shell no window reached `Root::new`,
  sandboxed or not, with the screen locked, so these calls are checked only by the compiler. The headless tests replace
  them with hooks.
- A key window counts as on screen even when GPUI says it's hidden (as specified). So a key window with a locked screen,
  if AppKit stops its frames, may log a begin record and get one nudge every 2 s until the screen is unlocked.
