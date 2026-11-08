# Tenmoku stops drawing until deactivated and reactivated (T1507)

Read-only investigation: no build, no launch, no reproduction.

- **First freeze:** Tenmoku 0.1.1 release build, launched as `tenmoku .` from a terminal, under load average 40-90. It
  "froze" while the user scrolled, and they "couldn't change files or scroll". It "came back to life" after they
  switched away and back. There was no crash or hang report and nothing in the unified log.
- **Second freeze (2026-11-07):** pid 62551, the same build, launched 17:39. It froze with no scroll and no file switch.
  The lead took a 5 s `sample` while it was frozen, kept at
  `~/.local/share/fastedit-lead/T1507/tenmoku-freeze-62551.txt`. See "Evidence: the sample of pid 62551" below.

Paths: `macos/` is `~/.cargo/registry/src/index.crates.io-*/gpui-pre-macos-0.3.9/src/`, `gpui/` is `gpui-pre-0.3.9/src/`
and `apple/` is `gpui-pre-apple-0.3.9/src/`. GPUI is the unforked crates.io snapshot (`Cargo.toml:57`).

Each claim below is one of two kinds:
- **source-proven:** control flow read from these files;
- **assumed:** runtime behaviour the source can't show (CoreVideo, AppKit, WindowServer, the user's machine).

Everything about what happened on the user's machine before the sample is a hypothesis. A third kind of claim covers
the sample:
- **observed:** read from the sample of the second freeze.

## Verdict

- **Cause 1's end state is observed in the second freeze: no display link running, in gpui-pre-macos 0.3.9's frame
  source.** During the 5 s the sample covers, the process had no `CVDisplayLink` running at all, and its main thread sat
  idle and responsive. So the window got no display-link frame requests in that interval.
- **Which of 1a, 1b and 1c happened is still unknown.** All three end in that same state, and the sample can't tell
  them apart.
- **The first freeze probably had the same cause, but that is assumed.** Its symptoms match, and so does the recovery:
  activation re-subscribes the frame source and forces a frame (source-proven). Nothing was captured for it.
- **Scrolling is not the trigger.** The second freeze happened with no scroll and no file switch.
- **Ruled out by the sample, for the second freeze's sampled interval only:**
  - main-thread starvation (6);
  - drawn-but-not-presented (2) as the explanation;
  - lost invalidation (4b).

  4a stays refuted by the source.
- **No cause in this repo is confirmed.**
  - The fe-gpui frame chains (5) don't stop, start or subscribe the display link. Nothing else in the repo does either.
    That rules out only our code stopping the link directly.
  - For the first freeze, which was never captured, a cause of ours is not excluded: for example synchronous main-thread
    work under load that happened to end around the time the user switched (6).
- **Recommended next:**
  1. **A log file for detached launches.** A detached launch, which is what `tenmoku .` from a terminal is, has stdout
     and stderr on `/dev/null`, so even the start failures gpui does log are lost.
  2. **A resubscribe watchdog.** When the window should be getting frame requests and none arrive, it restarts the frame
     source through `displayLayer:` and logs which case it found.

  Both are workarounds or evidence gathering on our side. GPUI stays unforked, and this card changes no code.

## Evidence: the sample of pid 62551 (observed)

`sample 62551 5` (1 ms interval, 3122 samples), taken at 18:34:37 while the window was frozen. The process was
launched at 17:39:58, and its parent is `launchd` (the file's header).

- **The main thread was idle and responsive.**
  - 3120 of 3122 samples are in `-[NSApplication run]` → `nextEventMatchingMask:` → `__CFRunLoopServiceMachPort` →
    `mach_msg2_trap`.
  - 1 sample is draining the main dispatch queue, through `_dispatch_main_queue_callback_4CF` into Tenmoku code.

  So the app was not hung, and the main queue (and with it any frame source's dispatch source) was being serviced.
  This rules out 6 for this freeze.
- **No `CVDisplayLink` io thread exists.**
  - No thread in the call graph runs CoreVideo code or is named for a display link. CoreVideo is only listed under
    Binary Images.
  - The threads present are the main thread, `ipc` and `ipc keeper`, `com.apple.NSEventThread`, file and config
    watchers, `chat-store`, `fe-lsp rust-analyzer`, `async-io` and `async-process`, gix status workers, and unnamed
    dispatch workers.
  - A running `CVDisplayLink` keeps its io thread alive for as long as it runs, blocked between vsyncs, so a 5 s sample
    would show it even if no callback landed in a sample (assumed CoreVideo behaviour, but long-standing). Its absence
    means no link in the process was running.
  - gpui-pre-macos runs a display's link exactly while the registry holds a subscriber for it and believes it running
    (`macos/display_link.rs:191-294`), and only a running link calls `step` (source-proven). So **no window in the
    process was getting display-link frame requests.**
- **What the absent io thread rules out, for the sampled interval:**
  - 2 as the explanation: with no frame requests nothing was drawn to present. A drawable or GPU problem could coexist,
    but it can't cause this state.
  - 4b: a subscribed window that is merely clean still has a running link and an io thread.
  - "Ticks arrive but the handler doesn't run" (end of 1): there were no ticks.
  - 3: throttling needs frame requests to throttle.
- **The machine had two displays:** an external Dell AW3225QF 4K as main, and the built-in XDR (the lead).
  - Each display has its own link entry (`macos/display_link.rs:84-88`), and neither link was running.
  - The displays last woke at 16:27, before launch, so display sleep and wake (the usual case of 1c) is unlikely.
  - Other reconfiguration isn't ruled out: a mode change, or the window crossing between the two displays.
- **No log could have shown it.**
  - A terminal launch detaches: the server is the same binary respawned with stdin, stdout and stderr on `/dev/null`,
    in its own process group (`crates/fastedit/src/ipc.rs:787-804`; `ipc_tests.rs:216`).
  - The parent becomes `launchd` once the client exits.
  - So `could not start display link, code: …` (`macos/display_link.rs:523`) and `failed to retrieve next drawable`
    (`apple/metal_renderer.rs:509-513`) would both have been discarded. The unified log has nothing, because Tenmoku
    doesn't log there.
  - The same applies to the first freeze, also a `tenmoku .` launch. The earlier advice to check its terminal was wrong.
- **The busy gix status workers** (`gix::status::*` threads) are git status background work, not drawing.
- **What the sample can't tell apart.** 1a, 1b and 1c all end with no link running:

  | Mechanism | How it ends with no link running | Registry state after |
  |---|---|---|
  | 1a, failed start | the window unsubscribed and the link stopped (`macos/display_link.rs:249-264`) | `running = false`, no subscriber |
  | 1a, start lost in a stop race | the link stopped although the start succeeded | `running = true`, a subscriber |
  | 1b, stale occlusion | `start_display_link` gave up after unsubscribing (`macos/window.rs:837-845`), and the last unsubscribe stopped the link (`macos/display_link.rs:279-292`) | `running = false`, no subscriber |
  | 1c, no screen | the same give-up (`macos/window.rs:847-850`) | `running = false`, no subscriber |

  A sample shows threads, not the registry. It also can't show the window's `occlusionState` or `screen()` at the
  transition where the start failed.

  Two cheap user-side checks during a future freeze. They are suggestive only and never attribute a mechanism on their
  own:
  - **Open a second Tenmoku window on the same display.**
    - Opening a window can draw once without a working link, through `displayLayer:` (`macos/window.rs:3321-3340`). So
      only sustained drawing counts: for example, scrolling the new window for a few seconds.
    - If it keeps drawing, a start succeeded for it now. That fits the failed-start case of 1a, 1b or 1c for the old
      window.
    - If it stays frozen, that fits a dead link still marked running (stop-race 1a). It also fits the new window's own
      start failing again, through a repeated `CVDisplayLinkStart` failure or the same occlusion or screen give-up.
  - **Drag the frozen window to the other display.** `windowDidChangeScreen:` calls `start_display_link`
    (`macos/window.rs:3124`).
    - Sustained drawing there means a start can succeed now, which argues against a 1b that is still in effect.
    - Staying frozen fits a repeated start failure as well as 1b.

## How macOS frames are driven (gpui-pre-macos 0.3.9, source-proven)

- **Presented frames have two sources.**
  - The window's `request_frame_callback` (`macos/window.rs:670`) is GPUI's frame callback (`gpui/window.rs:1760-1934`).
    It runs next-frame callbacks (`:1866-1874`), draws if the window is dirty (`:1884-1902`), presents (`:1899`), or only
    presents (`:1904-1907`).
  - Key events can also draw outside that callback: `dispatch_key_event` draws a dirty window synchronously before
    dispatching (`gpui/window.rs:6009-6012`), but doesn't present. Typing during a lost subscription therefore runs
    draws that never reach the screen.
- **Nothing else wakes the window.** macOS has no frame waker or fallback timer: `frame_waker` is the trait default
  `None` (`gpui/platform.rs:1254-1256`), and `schedule_frame` is the no-op default (`:1277`). `Window::on_next_frame`
  (`gpui/window.rs:2710-2716`) and `refresh` (`:2376-2381`) only record demand.
- **`request_frame_callback` has three callers:**
  - `step`, the display-link tick (`macos/window.rs:3342-3364`);
  - `display_layer`, AppKit's `displayLayer:` (`:3321-3340`, selector registered at `:232`);
  - the activation path in `window_did_change_key_status` (`:3196-3216`).
- **`step` only runs while the window's `WindowFrameSource` is subscribed** to its display's link
  (`macos/display_link.rs:299-357`):
  - one immortal `CVDisplayLink` per display, shared by every window on that display;
  - the output callback merges `1` into each subscriber's main-queue dispatch source (`:128-163`);
  - a link runs while it has subscribers (`subscribe` `:191-268`, `unsubscribe` `:270-294`).
- **`start_display_link` (`macos/window.rs:836-856`) unsubscribes first (`:837`), then subscribes again.** It **returns
  without logging and leaves the window unsubscribed** in two cases:
  - `occlusionState` lacks `Visible` at that instant (`:839-845`);
  - `screen()` gives no display id (`:847-850`).

  A failed `CVDisplayLinkStart` is logged (`:851-855` through `log_err`, message at `macos/display_link.rs:523`).
- **What subscribes or unsubscribes** (each start is an unsubscribe followed by a subscribe):

  | Event | Code | Stops | Starts |
  |---|---|---|---|
  | `windowDidChangeOcclusionState:` | `macos/window.rs:2993-3010` | `:3005` when hidden | `:3003` when visible |
  | `windowDidChangeScreen:` | `macos/window.rs:3121-3127` | — | `:3124` |
  | `displayLayer:` | `macos/window.rs:3321-3340` | `:3327` | `:3338`, after a synchronous frame |
  | `windowDidBecomeKey:` (2nd activation on) | `macos/window.rs:3196-3216` | `:3201` | `:3212`, after a synchronous frame |

  `windowDidResignKey:` leaves the link alone: it only queues the activation callback (`:3219-3230`). Scrolling, typing
  and clicking subscribe and unsubscribe nothing.

## Why switching away and back unsticks it (source-proven paths)

- **Switching back makes the window key.** `windowDidBecomeKey:` takes `request_frame_callback`, stops the link, and
  runs one frame synchronously with `presents_with_transaction` (`macos/window.rs:3199-3209`). It then puts the callback
  back and calls `start_display_link` (`:3210-3212`), which subscribes again unless 1b or 1c applies at that moment.
- **The queued activation callback** (`:3219-3230`) reaches GPUI's `on_active_status_change`
  (`gpui/window.rs:1999-2018`), which calls `window.refresh()` (`:2013`), so the next frame request draws.
- **Switching away** queues the same callback with `false`, so it refreshes but doesn't subscribe again.
- **If the other app's window covered Tenmoku,** the occlusion notifications also stop the link on the way out (`:3005`)
  and start it on the way back (`:3003`).
- **What that repairs:** every route back gives a fresh subscription and a refreshed window, which repairs a lost
  subscription (1). The refresh alone would also repair lost invalidation (4b). A starved main thread (6) would have
  caught up whether or not the user switched.

## Causes, ranked

### 1. Frame source lost: upstream, gpui-pre-macos 0.3.9 (observed in the second freeze; mechanism unknown)

All three ways end with the window unsubscribed, or subscribed to a link that isn't running. Then `step` never runs, and
nothing retries until the next occlusion change, screen change, `displayLayer:` or activation (source-proven). The
sample of the second freeze shows that end state, with no link running in the process (observed). The freeze began at
the last such transition, the user's interaction then was only what revealed it (assumed), and the second freeze had
no scroll at all (observed).

- **1a. Shared link not running after a stop and start.** With one Tenmoku window on the display, every
  `start_display_link` stops the display's link (`macos/display_link.rs:279-292`) and starts it again at once
  (`:240-259`) (source-proven).
  - The module's note says `CVDisplayLinkStop` "merely flags the link's io thread to exit 'soon' and returns
    immediately" (`:3-4`). This is upstream's claim about CoreVideo and isn't checked here.
  - If a start races the old io thread winding down, two outcomes are possible (assumed):
    - the start fails, which logs `could not start display link, code: …` and leaves the window unsubscribed
      (`:249-264`). A detached launch sends that line to `/dev/null` (see the evidence section);
    - the start succeeds but the link still stops, and the registry's `running = true` describes a dead link. This logs
      nothing.
  - This is the only mechanism here where CPU load plausibly matters: a starved io thread would wind down more slowly
    (assumed).
  - gpui-pre-macos 0.3.7 has the same registry and `start_display_link`, so the 0.3.9 bump didn't bring it in.
- **1b. Occlusion read as not `Visible` at a transition.** `start_display_link` gives up (`macos/window.rs:839-845`).
  The callers are activation (`:3212`), `displayLayer:` (`:3338`) and screen change (`:3124`) (source-proven).
  - Only a later `windowDidChangeOcclusionState:` that reports `Visible` re-subscribes (`:3003`) (source-proven).
  - If WindowServer's occlusion state was briefly hidden at a transition and no later change was posted, the window
    stays unsubscribed and nothing is logged (assumed).
- **1c. No screen at a transition.** The same give-up when `screen()` is nil (`macos/window.rs:847-850`), which the code
  says AppKit does while displays are reconfigured.
  - It recovers on the next screen change, occlusion change, `displayLayer:` or activation (source-proven).
  - It needs a display change, such as sleep or a monitor change, and the report doesn't mention one.
- **Not covered by 1: ticks reaching a subscribed window but not running.** The dispatch source is created and resumed
  once, and never suspended (`macos/display_link.rs:306-326`). GPUI's foreground tasks use the same main queue
  (`apple/dispatcher.rs:51-56`). In the second freeze there were no ticks to deliver, and the main queue was draining
  (observed).

### 2. Frames drawn but not presented: upstream, gpui-pre-apple 0.3.9 (ruled out for the second freeze)

- `MetalRenderer::draw` returns without presenting when `nextDrawable` gives nil (`apple/metal_renderer.rs:507-514`),
  with three drawables (`:183`). That path logs `failed to retrieve next drawable` at error on every frame, and the
  logger passes it (`crates/fastedit/src/main.rs:431-465`: other crates at Warn). In a detached launch it goes to
  `/dev/null` (source-proven).
- GPU command-buffer errors are silent: the completed handler only returns the instance buffer and never reads the
  buffer's status (`apple/metal_renderer.rs:566-576`) (source-proven).
- Whether one `presents_with_transaction` frame at activation would clear a GPU-side fault is unknown.
- In the second freeze no link ran, so no frames were requested and nothing was drawn to present (observed). A
  presentation fault couldn't have caused that state.

### 3. on_request_frame throttling (inactive, thermal): upstream, gpui-pre 0.3.9 (cannot stop frames alone)

- The callback (`gpui/window.rs:1760-1934`) caps frames in two cases:
  - inactive windows, at `inactive_frame_interval`, by default 33.3 ms (`gpui/platform.rs:2676`; `gpui/window.rs:1821-1822`);
  - Serious or Critical thermal state, at 16.7 ms (`:1823-1824`).
- The caps only apply to frames that carry next-frame callbacks or a forced render (`:1816-1818`).
- A throttled request returns before `last_frame_time.set` (`:1830-1864`), so as long as requests keep arriving it only
  delays frames (source-proven).
- Throttling can't produce a stall without the requests themselves stopping, which is cause 1. Under build load the
  thermal cap may have applied (assumed). Our scroll frames carry next-frame callbacks (see 5), so scrolling would then
  cap at 60 fps.
- The re-entrancy skip (`:1796-1800`) only fires while a draw is on the stack, so `step`, which runs from the main queue
  with no draw in progress, never reaches it.

### 4. GPUI frame state: upstream, gpui-pre / gpui-pre-macos

- **4a. `request_frame_callback` left `None`: refuted.**
  - Each `take()` is restored on its only non-panicking path (`macos/window.rs:3199`/`:3210`, `:3325`/`:3336`,
    `:3347`/`:3362`).
  - A panic out of these `extern "C"` callbacks aborts the process, and there was no crash.
  - If it had stayed `None`, activation couldn't recover: its synchronous frame and its restart both sit behind the
    same `take()` (`:3199`).
- **4b. Window not dirty when it should be: ruled out for the second freeze.**
  - A draw phase stuck at Prepaint or Paint is excluded: it would drop `invalidate_view` (`gpui/window.rs:182`) and the
    activation refresh alike (`:2377`, `not_drawing` `:279-281`), so the recovery couldn't have happened.
  - Notifies skipped elsewhere are not excluded: the activation refresh (`:2013`) would repaint the window. But it would
    take both the editor's scroll handling and the file tree's clicks skipping `cx.notify` at once. One missing notify
    would freeze one view, not the window, so this is ranked low.
  - The second freeze rules it out as the explanation: a subscribed window that is merely clean keeps its link and io
    thread running, and the sample has none (observed).

### 5. fe-gpui frame chains: ours (don't touch the frame source; other effects not excluded)

- `element.rs:807-828` (`wrap_pending`):
  - while soft wrap is pending, each frame wraps a byte budget around the top row on the main thread
    (`fe-display/src/wrap_map.rs:323-360`, at least one row per call) and asks for another frame (`:826-828`);
  - `wrap_pending_at` (`fe-editor/src/lib.rs:751-758`) keeps the top's buffer position.
- `element/minimap.rs:226-234` and `:829-845`:
  - an unfinished minimap asks for another frame (`request_animation_frame`, `:226-228`);
  - `sweep_asked` (`:414`) keeps one sweep step queued at a time with `on_next_frame` (`:230-234`, re-armed in
    `sweep_step` `:834-843`).
- `view.rs:1081-1107` (`press_when_settled`):
  - re-queues a deferred mouse press each frame until no vim replay is suspended and the layout has the current
    version, and refreshes the window (`:1106`);
  - a replay that stayed suspended would hold that editor's mouse-downs but not scrolling, which doesn't pass through
    it.
- **What the source proves:**
  - every chain only pushes next-frame callbacks (`gpui/window.rs:2710-2716`) or notifies;
  - none stops, starts or subscribes the display link, or calls `displayLayer:` or anything to do with occlusion.
    Nothing else in `crates/` or `kiln/` does either.

  That excludes only our code stopping the frame source directly.
- **What it doesn't prove:** that their main-thread work stays short under heavy load. Wrapping and minimap work are
  budgeted per frame, but a slow frame is possible. A stall like that would end by itself (6), and if it ended around the
  time the user switched, it would look like activation cleared it. So for the uncaptured first freeze it isn't excluded.
  In the second freeze's sampled interval the main thread was idle (observed).

### 6. Main thread starved or blocked for a time: ours or upstream (ruled out for the second freeze)

- While the main thread is busy, neither frames nor input are handled.
- The recovery only shows the main thread was responsive by the time the user reactivated. A stall that ended on its own
  around then, through CPU starvation at load 40-90 or long synchronous work, fits the report if switching was a
  coincidence.
- macOS shows the spinning cursor when events go unhandled for a few seconds. The report doesn't mention it, and the
  absence of a hang report doesn't rule a hang out.
- This is ranked below 1 because "came back when I switched away and back" points to the switch itself.
- In the second freeze the main thread was idle in its run loop for 3120 of 3122 samples (observed). That rules this out
  there. The first freeze isn't covered by the sample.

## What can still tell 1a, 1b and 1c apart

The sample shows cause 1's end state but not which mechanism led to it (see its table). Three sources can narrow it
down, and only the last can attribute it:

- **The user-side checks** in the evidence section: open a second window, or drag the frozen window to the other
  display. They're free and can be done at the next freeze.
- **The watchdog's log (card 2 below).** It measures per-window callback progress: whether frame requests reach this
  window, independently of the watchdog's own nudges. For each stall it records:
  - `occlusionState` `Visible`, whether `screen()` exists, and the display id at the first nudge;
  - whether delivery resumed afterwards.

  Card 1's log file also catches any `could not start display link` line. The table is suggestive only:

  | After the nudges | State at the nudge | Fits, among others |
  |---|---|---|
  | delivery resumes | any | failed-start 1a; transient 1b or 1c; stop-race 1a with this window the display's only subscriber |
  | no delivery, with a `could not start display link` line | any | a repeated start failure (1a) |
  | no delivery | not `Visible` | 1b still in effect, or a repeated start failure |
  | no delivery | no screen | 1c still in effect, or a repeated start failure |
  | no delivery | `Visible`, has a screen | stop-race 1a with another subscriber on that display; a start that succeeded but whose callbacks never arrive; or an unlogged repeated failure |

  The state values are read at the nudge, not at the original failed start, so they are context, not attribution.
- **A patched gpui-pre-macos for dogfooding:** a local `[patch.crates-io]` build that logs every `start_display_link`
  outcome, the registry's `running` flag and subscriber count, and each `step` delivered to the window.
  - Attributing a mechanism takes both: the start and subscription outcomes, and per-window callback progress. A start
    that succeeds and is followed by no `step` is only visible through the second.
  - Take this step only if the watchdog's log leaves it ambiguous: it means building against a modified GPUI.

All of this needs a log that survives a detached launch, so card 1 comes first.

## Recommended follow-up cards (C2)

No cause in this repo is confirmed, so neither card is a bugfix of ours. The second freeze's observed end state, no
display link running, is in gpui-pre-macos 0.3.9's frame source. What triggered it is unknown. Card 1 makes the next freeze leave evidence. Card 2 is the narrow workaround and leaves
GPUI unforked. They are independent, but card 2's log is only useful once card 1 has landed.

### Card 1: a log file for detached and LaunchServices launches

- **Title:** "Write Tenmoku's stderr to a log file when it would go to /dev/null, so a detached launch leaves evidence."
  Type: bugfix. Complexity: s.
- **Scope:**
  - `crates/fastedit/src/main.rs`, where `init_logging` is;
  - a new `crates/fastedit/src/log_file.rs` and its tests;
  - the decision file.
- **Why:** `tenmoku .` from a terminal respawns itself detached with stdout and stderr on `/dev/null`
  (`crates/fastedit/src/ipc.rs:787-804`). A launch from the Dock or Finder also gets `/dev/null`. Everything the
  logger prints (`crates/fastedit/src/main.rs:450-454`, `eprintln!`) is lost, including gpui's
  `could not start display link` and `failed to retrieve next drawable`, and so are panic messages.
- **Behaviour:**
  1. At startup, before `init_logging`, check what fd 2 is.
  2. If it is `/dev/null` (same device and inode as `/dev/null`), point fd 2 at the log file with `dup2`, opened for
     append:
     - macOS: `~/Library/Logs/Tenmoku/tenmoku.log`;
     - Linux: `$XDG_STATE_HOME/tenmoku/tenmoku.log`, default `~/.local/state`.

     This catches the logger's lines, panics, and anything a library writes to stderr, through one path.
  3. A terminal, a pipe, or a file the user redirected to (`TENMOKU_FOREGROUND=1 … 2>log`, `ipc_tests.rs:338`) is left
     as it is.
  4. Before opening the file, rotate it: if it's over 5 MB, rename it to `tenmoku.log.1`, replacing the old one. Only one
     old file is kept.
  5. The first line of each launch gives the version, pid, launch kind (detached, LaunchServices or foreground) and the
     display count.
  6. Log levels don't change: gpui errors already pass at Warn.
- **Regression tests:**
  - `log_file::tests::redirects_only_a_null_stderr`: the pure decision, by what fd 2 is. Null → redirect. Terminal, pipe
    or regular file → leave it.
  - `log_file::tests::rotates_past_the_size_limit`, in a temporary directory: a file over the limit becomes `.1`, a
    second rotation replaces `.1`, and a file under the limit is appended to.
  - `log_file::tests::launch_line_is_first`: opening the log in a temporary directory writes the launch line before any
    record.

### Card 2: resubscribe watchdog for a window that gets no frame requests

- **Title:** "Detect a macOS window whose frame requests stop arriving, log it, and restart its display link through
  `displayLayer:`." Type: bugfix. Complexity: m.
- **Scope:**
  - `kiln/crates/kiln-shell/src/root.rs`;
  - a new `kiln/crates/kiln-shell/src/frame_watch.rs`;
  - `kiln-shell`'s `Cargo.toml`: macOS-only `objc2` and `raw-window-handle`, both already in the lock;
  - its test module list;
  - the decision file.
- **When a window should be getting frame requests:** while it is visible (GPUI visibility `Visible`), or while it is
  the key window.
  - While subscribed, a window's link ticks every vsync and each tick runs the frame callback, dirty or not
    (`macos/window.rs:3342-3364`). So such a window always gets frame requests, and their absence is the stall
    (source-proven).
  - Visibility alone isn't enough. GPUI's visibility reads the same `occlusionState` `Visible` bit that makes 1b give
    up (`macos/window.rs:2947-2958` vs `:839-845`), so a stale "hidden" would also hide the window from the watchdog.
    The key window is taken as being on screen.
  - A window that is hidden and not key is skipped: its link is stopped by design.
  - Input doesn't gate the watchdog, since the second freeze happened with no input.
- **The heartbeat is a frame-request probe, not a paint or an input event.**
  - The watchdog posts `window.on_next_frame(ack)`. GPUI runs next-frame callbacks only inside the platform frame
    callback, after any throttling (`gpui/window.rs:1864-1874`), and never from `dispatch_key_event`'s synchronous draw
    (`:6009-6012`).
  - So an acknowledged probe means `step` (or `displayLayer:`, or activation) delivered a frame request, and an
    unacknowledged probe means none did.
  - Paints are not counted, since key events draw without presenting.
  - On a healthy window the probe adds nothing: it rides a frame callback that runs every vsync anyway.
- **Stall:**
  - a probe unacknowledged for 500 ms and across two consecutive 250 ms polls;
  - the polls run on the main queue through `dispatch_after`, independent of the display link;
  - requiring two polls stops a busy main thread, whose queued tick hasn't run yet, from counting as a stall.
- **Nudge:**
  - `[ns_view setNeedsDisplay:YES]` on the window's view, from `HasWindowHandle for Window` (`gpui/window.rs:7572`;
    view handle at `macos/window.rs:2422-2427`);
  - AppKit then calls GPUI's `displayLayer:`, which draws and presents synchronously, then calls `start_display_link`,
    which unsubscribes and subscribes again (`macos/window.rs:3321-3340`, `:836-856`). Calling this as described is
    assumed AppKit behaviour. GPUI's `activate` relies on the same call to force a frame (`:1801-1804`).
  - **A nudge's own frame is not recovery.** `displayLayer:` runs the frame callback, and with it the pending probe's
    acknowledgement, before `start_display_link` tries to subscribe (`macos/window.rs:3325-3338`). Even if the restart
    fails, the probe is acknowledged.
    - So each nudge consumes the probe pending at the time.
    - Recovery counts only fresh probes, posted at a poll after the nudge's frame has run, and acknowledged with no
      nudge in between.
    - The episode ends only after two such probes in a row, about 1 s of delivery the watchdog didn't cause. AppKit can
      also call `displayLayer:` for other reasons, such as a resize, so one acknowledgement isn't enough.
    - Until then, the episode, its nudge count and its backoff state are kept.
  - **Pacing.** A nudge, then a poll with a fresh probe and no nudge, then the next poll nudges again if that probe went
    unacknowledged. So a 1b or 1c that stays in effect still presents up to 2 frames a second. After 8 nudges in one
    episode, it backs off to one nudge every 2 s, so a stuck idle window doesn't keep redrawing.
  - Limit: in stop-race 1a with a second Tenmoku window on the same display, the restart doesn't stop the shared link,
    because another subscriber remains. The nudge then only presents frames, and the log says so (see the table above).
- **Log, through card 1's file:** one warn line when a stall episode begins and one when it ends, not one per nudge.
  - Begin: how long the probe went unacknowledged, visible and key at the time, `occlusionState` `Visible`, whether
    `screen()` exists, and the display id.
  - End, written only when delivery resumes by the rule above: the nudge count, and how long the stall lasted.
- **Other platforms:** compiled out.
- **Regression tests (headless).** The test platform has no frame loop, so frame-request delivery is
  `Window::simulate_next_frame` (`gpui/window.rs:2740-2747`). The nudge is an injected hook.
  - `frame_watch::tests::stall_policy`, the pure policy:
    - not acknowledged for 500 ms but within one poll → no stall;
    - across two polls → stall;
    - acknowledged → no stall;
    - hidden and not key → no probe;
    - visible, or key → probe;
    - a probe pending at a nudge is consumed by it, and not counted as recovery;
    - a nudge, then a fresh probe with no nudge, then a nudge again while unacknowledged;
    - back off after 8 nudges;
    - recovery only after two consecutive fresh probes acknowledged without nudges;
    - one begin record per episode, and an end record only on recovery.
  - Healthy window, nothing to draw: a visible, clean window. Call `simulate_next_frame` each poll and advance the clock
    2 s. Assert no nudge and no log.
  - No input at all, no frame requests: a visible window with no events, where `simulate_next_frame` is never called.
    Advance 600 ms and assert a nudge and a begin record. This is the second freeze's shape.
  - Input-triggered draws without frame requests: make the window dirty and dispatch key events, so
    `dispatch_key_event` draws synchronously. Never call `simulate_next_frame`, advance 600 ms, and assert a nudge.
  - Stale hidden state: `TestWindow::simulate_visibility_change(Hidden)` (`gpui/platform/test/window.rs:161-170`) on the
    key window, with no frame requests. Assert a nudge.
  - Nudges that never restart delivery: the injected nudge models `displayLayer:` by calling `simulate_next_frame`
    once, so each nudge acknowledges the probe pending at the time, but nothing else ever delivers a frame. Advance
    10 s. Assert:
    - one begin record and no end record;
    - the nudge count rising at the 2-per-second pace, then backing off after 8;
    - no reset of the episode.
  - Recovery: after a nudge, call `simulate_next_frame` at each later poll as well, which models `step` resuming.
    Assert an end record after the second fresh probe is acknowledged, and no nudge after it.
- **Out of scope:** forking gpui-pre-macos or changing `start_display_link`.

### Upstream report

Worth sending separately to the Zed repo, with the sample's finding: no `CVDisplayLink` thread in a frozen, idle,
visible window.
- `start_display_link` gives up without logging or retrying (`macos/window.rs:839-850`).
- Each transition stops the shared link and starts it again at once (`macos/display_link.rs:240-292`).
- An upstream fix would log and retry a skipped or failed start, or check frame requests against `step` deliveries.
