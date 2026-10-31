//! `tenmoku --perf <file>`: measures the M2 large-file targets in a real
//! window, prints them and quits. See PERF.md for how to read the output.
//!
//! 1. Open: startup phases from process start (see [`Startup`]) to the
//!    first presented frame (the loading placeholder, unless the background
//!    read beat it) and the first frame that shows the file; memory then,
//!    and again once a background parse has had time to land.
//! 2. Scroll: one scroll-wheel event per frame, first smoothly and then in
//!    large jumps, timing every frame the editor paints.
//! 3. Soft wrap: alt-z, the window narrower by an em, wider by an em, and
//!    alt-z again, each followed by the frames that wrap the pending rows
//!    a slice at a time, timing every frame until none are pending.
//! 4. Idle: CPU used over a few seconds with nothing happening.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};

use fe_gpui::{EditorView, FrameTiming};
use fe_workbench::Workbench;

use gpui::{
    App, Entity, Keystroke, Modifiers, PlatformInput, ScrollDelta, ScrollWheelEvent, TouchPhase,
    Window, point, px, size,
};

/// Frames to let a background parse land before scrolling.
const SETTLE_FRAMES: u32 = 60;
const SCROLL_FRAMES: u32 = 600;
/// About two rows a frame: a brisk trackpad scroll.
const SMOOTH_STEP: f32 = 40.;
/// About a thousand rows a frame: dragging a scrollbar through a big file.
const JUMP_STEP: f32 = 21_000.;
/// GPUI keeps presenting for a second after fast input stops.
const IDLE_SETTLE: Duration = Duration::from_secs(2);
const IDLE: Duration = Duration::from_secs(5);
/// A frame interval longer than this missed a 120 Hz vsync.
const MISSED_VSYNC: Duration = Duration::from_micros(12_500);
/// Intervals longer than this are listed individually.
const STALL: Duration = Duration::from_millis(50);
/// Frames to wait for a resize to reach the editor.
const RESIZE_FRAMES: u32 = 120;

/// `println!` that ignores a closed stdout (`tenmoku --perf | head`): a
/// panic inside a platform callback aborts the app.
macro_rules! say {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stdout(), $($arg)*);
    }};
}

/// When startup reached each milestone up to the window opening. `main`
/// records them whether or not `--perf` is on: they're a few `Instant`s.
pub(crate) struct Startup {
    /// When the kernel started the process, before exec and dyld; `None` if
    /// it couldn't be read.
    process: Option<Instant>,
    main: Instant,
    /// When `gpui_platform::application()` returned: GPUI's platform and
    /// app state exist, AppKit hasn't launched.
    pub(crate) platform: Option<Instant>,
    /// When `Application::run` called back: AppKit finished launching.
    pub(crate) app: Option<Instant>,
    /// When `open_window` called back to build the root view: the native
    /// window and its renderer exist (shaders compiled).
    pub(crate) view: Option<Instant>,
    /// When `open_window` returned: the first frame is drawn, not yet
    /// presented.
    pub(crate) window: Option<Instant>,
}

impl Startup {
    /// Call first thing in `main`.
    pub(crate) fn new() -> Self {
        let main = Instant::now();
        Self {
            process: process_start(main),
            main,
            platform: None,
            app: None,
            view: None,
            window: None,
        }
    }

    /// Process start, or `main` if that's unknown.
    pub(crate) fn launched(&self) -> Instant {
        self.process.unwrap_or(self.main)
    }
}

/// The kernel's start time for this process, as an `Instant`. It's
/// wall-clock time to the microsecond, so it's converted through the wall
/// clock now, taken next to `now`.
#[cfg(target_os = "macos")]
fn process_start(now: Instant) -> Option<Instant> {
    use std::time::SystemTime;

    let wall = SystemTime::now();
    // SAFETY: `info` is a `proc_bsdinfo`, the flavour asked for, and the
    // size passed is its size.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let written = unsafe {
        libc::proc_pidinfo(
            libc::getpid(),
            libc::PROC_PIDTBSDINFO,
            0,
            (&raw mut info).cast(),
            size,
        )
    };
    if written != size {
        return None;
    }
    let started = SystemTime::UNIX_EPOCH
        + Duration::from_secs(info.pbi_start_tvsec)
        + Duration::from_micros(info.pbi_start_tvusec);
    now.checked_sub(wall.duration_since(started).ok()?)
}

/// The kernel's start time for this process, as an `Instant`, from
/// `/proc/self/stat`: clock ticks since boot (10 ms each, typically),
/// converted through the boot clock now, taken next to `now`.
#[cfg(target_os = "linux")]
fn process_start(now: Instant) -> Option<Instant> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    // Fields after the command, which is in parentheses and may hold
    // spaces; the start time is field 22, the 20th after it.
    let after_command = &stat[stat.rfind(')')? + 2..];
    let ticks: u64 = after_command.split(' ').nth(19)?.parse().ok()?;
    // SAFETY: `sysconf` reads a constant; `clock_gettime` fills in the
    // struct it's given.
    let per_second = u64::try_from(unsafe { libc::sysconf(libc::_SC_CLK_TCK) }).ok()?;
    let mut boot: libc::timespec = unsafe { std::mem::zeroed() };
    if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut boot) } != 0 {
        return None;
    }
    let since_boot = Duration::new(boot.tv_sec as u64, boot.tv_nsec as u32);
    let started = Duration::from_nanos(ticks * 1_000_000_000 / per_second);
    now.checked_sub(since_boot.checked_sub(started)?)
}

/// Elsewhere the kernel's start time isn't read; launch falls back to `main`.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn process_start(_now: Instant) -> Option<Instant> {
    None
}

thread_local! {
    /// When the background read finished, and when the main thread picked
    /// its result up.
    static LOADED: Cell<Option<(Instant, Instant)>> = const { Cell::new(None) };
}

/// Records that the file read, finished at `read`, reached the main thread.
pub(crate) fn file_loaded(read: Instant) {
    LOADED.set(Some((read, Instant::now())));
}

struct Run {
    workbench: Entity<Workbench>,
    /// Set once the file is read and the editor replaces the placeholder.
    editor: Option<Entity<EditorView>>,
    startup: Startup,
    file_bytes: usize,
    /// Whether the first frame presented was the loading placeholder.
    placeholder_first: Option<bool>,
    /// The frame requests until the file is shown.
    requests: Vec<FrameRequest>,
    frame: u32,
    frames: Vec<FrameTiming>,
}

/// One of the window's frame requests: it runs `on_next_frame` callbacks,
/// then draws if anything changed, then presents.
#[derive(Clone, Copy, Debug)]
struct FrameRequest {
    started: Instant,
    /// When the request returned, after presenting; `None` until then.
    returned: Option<Instant>,
}

pub(crate) fn start(workbench: Entity<Workbench>, startup: Startup, window: &mut Window) {
    let run = Rc::new(RefCell::new(Run {
        workbench,
        editor: None,
        startup,
        file_bytes: 0,
        placeholder_first: None,
        requests: Vec::new(),
        frame: 0,
        frames: Vec::new(),
    }));
    window.on_next_frame(move |window, cx| tick(run, window, cx));
}

/// Runs at the start of every frame until the scroll phases are over.
fn tick(run: Rc<RefCell<Run>>, window: &mut Window, cx: &mut App) {
    let mut state = run.borrow_mut();
    if state.frame == 0 {
        if state.placeholder_first.is_none() {
            // `open_window` draws a frame but leaves presenting it to the
            // first frame request, which runs this callback first. Nothing
            // else swaps the placeholder for the editor in between, so if
            // it's still here, it's what gets presented.
            state.placeholder_first = Some(state.workbench.read(cx).active_editor(cx).is_none());
        }
        note_frame_request(&run, &mut state, cx);
    }
    if state.editor.is_none() {
        state.editor = state.workbench.read(cx).active_editor(cx).cloned();
        if let Some(editor) = &state.editor {
            state.file_bytes = editor.read(cx).model().text().len();
        }
    }
    if let Some(editor) = state.editor.clone() {
        let painted = editor.read(cx).frame_timings().frames().copied();
        state.frames.extend(painted);
        editor.update(cx, |view, _| view.frame_timings_mut().clear());
    }

    if state.frame == 0 {
        // Wait until the first frame that shows the file is presented, and
        // report after that, so reporting doesn't delay what it measures.
        let shown = state.frames.first().and_then(|&painted| {
            presented_after(painted.painted_at, &state.requests).map(|shown| (painted, shown))
        });
        let Some((painted, shown)) = shown else {
            drop(state);
            window.on_next_frame(move |window, cx| tick(run, window, cx));
            return;
        };
        let request = state.requests[0];
        let first = request.returned.unwrap_or(shown);
        let placeholder = state.placeholder_first.expect("set above");
        let launched = state.startup.launched();
        if state.startup.process.is_none() {
            say!("open: process start unavailable; times are from main");
        }
        let phases = startup_phases(
            &state.startup,
            request.started,
            first,
            placeholder,
            painted,
            shown,
        );
        for (phase, at) in phases {
            say!("{}", phase_line(&phase, at - launched));
        }
        say!(
            "open: first frame {:.0?} after launch ({}), window active: {}",
            first - launched,
            if placeholder { "loading" } else { "the file" },
            window.is_window_active()
        );
        say!("open: file shown {:.0?} after launch", shown - launched);
        print_memory("at file shown", state.file_bytes);
    }

    let scroll_end = SETTLE_FRAMES + SCROLL_FRAMES;
    let step = match state.frame {
        frame if frame < SETTLE_FRAMES => None,
        frame if frame < scroll_end => Some(SMOOTH_STEP),
        frame if frame < scroll_end + SCROLL_FRAMES => Some(JUMP_STEP),
        _ => None,
    };
    if state.frame == SETTLE_FRAMES {
        // Once a background parse has had time to land.
        print_memory("after settling", state.file_bytes);
        state.frames.clear();
    }
    if state.frame == scroll_end {
        report("smooth scroll", &state.frames);
        state.frames.clear();
    }
    if state.frame == scroll_end + SCROLL_FRAMES {
        report("jump scroll", &state.frames);
        print_memory("after scrolling", state.file_bytes);
        state.frames.clear();
        drop(state);
        wrap_step(run, 0, window, cx);
        return;
    }
    state.frame += 1;
    drop(state);

    if let Some(step) = step {
        let size = window.viewport_size();
        window.dispatch_event(
            PlatformInput::ScrollWheel(ScrollWheelEvent {
                // Over the editor, left of the project panel.
                position: point(size.width / 3., size.height / 2.),
                delta: ScrollDelta::Pixels(point(px(0.), px(-step))),
                modifiers: Modifiers::default(),
                touch_phase: TouchPhase::Moved,
            }),
            cx,
        );
    }
    window.on_next_frame(move |window, cx| tick(run, window, cx));
}

/// Records a frame request starting now, and when it returns: GPUI has no
/// hook after presenting, but a task spawned now runs on the main thread
/// once the request, including its present, is done.
fn note_frame_request(run: &Rc<RefCell<Run>>, state: &mut Run, cx: &mut App) {
    let index = state.requests.len();
    state.requests.push(FrameRequest {
        started: Instant::now(),
        returned: None,
    });
    let run = run.clone();
    cx.spawn(async move |_| {
        run.borrow_mut().requests[index].returned = Some(Instant::now());
    })
    .detach();
}

/// When a frame painted at `painted` was presented: the end of the first
/// frame request that returned after it. A frame drawn when the window
/// opened waits for the first request; later frames are drawn and presented
/// in the same request.
fn presented_after(painted: Instant, requests: &[FrameRequest]) -> Option<Instant> {
    requests
        .iter()
        .filter_map(|request| request.returned)
        .find(|&returned| returned >= painted)
}

/// The startup milestones, in order, each named with how long the phase
/// that ends at it took. `first_request` is when the first frame request
/// started and `first_frame` when it returned, having presented.
fn startup_phases(
    startup: &Startup,
    first_request: Instant,
    first_frame: Instant,
    placeholder: bool,
    editor: FrameTiming,
    shown: Instant,
) -> Vec<(String, Instant)> {
    let mut phases = vec![(startup.main, "to main".to_string())];
    phases.extend(startup.platform.map(|at| (at, "platform created".into())));
    phases.extend(startup.app.map(|at| {
        let label = if cfg!(target_os = "macos") {
            "app callback (AppKit launched)"
        } else {
            "app callback"
        };
        (at, label.into())
    }));
    phases.extend(
        startup
            .view
            .map(|at| (at, "window and renderer created".into())),
    );
    phases.extend(
        startup
            .window
            .map(|at| (at, "window open (first frame drawn)".into())),
    );
    let what = if placeholder { "loading" } else { "the file" };
    phases.push((first_request, "first frame requested".into()));
    phases.push((first_frame, format!("first frame presented ({what})")));
    if let Some((read, loaded)) = LOADED.get() {
        phases.push((read, "file read (background thread)".into()));
        phases.push((loaded, "file loaded (main thread)".into()));
    }
    phases.push((
        editor.prepaint_started,
        "editor's first prepaint starts".into(),
    ));
    phases.push((
        editor.painted_at,
        format!(
            "editor's first paint done (prepaint {:.1?}, paint {:.1?})",
            editor.prepaint, editor.paint
        ),
    ));
    phases.push((shown, "file shown (presented)".into()));
    // Stable, so ties keep the order above.
    phases.sort_by_key(|(at, _)| *at);
    let mut previous = startup.launched();
    phases
        .into_iter()
        .map(|(at, name)| {
            let phase = at.saturating_duration_since(previous);
            previous = previous.max(at);
            (format!("{name}, +{}", millis(phase)), at)
        })
        .collect()
}

fn phase_line(phase: &str, since_launch: Duration) -> String {
    format!("open: {:>7} {phase}", millis(since_launch))
}

fn millis(duration: Duration) -> String {
    format!("{:.1} ms", duration.as_secs_f64() * 1000.)
}

/// The soft-wrap phase's steps, in order.
#[derive(Clone, Copy, Debug)]
enum WrapStep {
    /// alt-z, with soft wrap off.
    On,
    Narrower,
    Wider,
    /// alt-z, with soft wrap on.
    Off,
}

const WRAP_STEPS: [WrapStep; 4] = [
    WrapStep::On,
    WrapStep::Narrower,
    WrapStep::Wider,
    WrapStep::Off,
];

/// Starts step `step` of the soft-wrap phase, then idles after the last.
fn wrap_step(run: Rc<RefCell<Run>>, step: usize, window: &mut Window, cx: &mut App) {
    let Some(&what) = WRAP_STEPS.get(step) else {
        idle(run, cx);
        return;
    };
    let editor = run
        .borrow()
        .editor
        .clone()
        .expect("wrapping needs the editor");
    let width_before = editor.read(cx).model().display().wrap_width();
    let started = Instant::now();
    match what {
        WrapStep::On | WrapStep::Off => {
            let wrapping = editor.read(cx).model().soft_wrap();
            let alt_z = Keystroke::parse("alt-z").expect("a keystroke");
            window.dispatch_keystroke(alt_z, cx);
            assert_ne!(
                editor.read(cx).model().soft_wrap(),
                wrapping,
                "alt-z didn't reach the editor"
            );
        }
        WrapStep::Narrower | WrapStep::Wider => {
            let workbench = run.borrow().workbench.clone();
            let theme = workbench.read(cx).pane().read(cx).theme();
            let text_system = window.text_system();
            let font_id = text_system.resolve_font(&gpui::font(theme.font_family.clone()));
            let em = text_system
                .em_advance(font_id, theme.font_size)
                .expect("the editor's font");
            let viewport = window.viewport_size();
            let delta = if matches!(what, WrapStep::Wider) {
                em
            } else {
                -em
            };
            window.resize(size(viewport.width + delta, viewport.height));
        }
    }
    let command = started.elapsed();
    let name = format!("wrap {what:?}");
    window.on_next_frame(move |window, cx| {
        wrap_tick(
            run,
            step,
            name,
            (started, command, width_before),
            0,
            window,
            cx,
        );
    });
}

/// Runs at the start of every frame of a soft-wrap step, until the wrap
/// width has changed and no rows are pending.
fn wrap_tick(
    run: Rc<RefCell<Run>>,
    step: usize,
    name: String,
    (started, command, width_before): (Instant, Duration, Option<f32>),
    frame: u32,
    window: &mut Window,
    cx: &mut App,
) {
    let mut state = run.borrow_mut();
    let editor = state.editor.clone().expect("wrapping needs the editor");
    state
        .frames
        .extend(editor.read(cx).frame_timings().frames().copied());
    editor.update(cx, |view, _| view.frame_timings_mut().clear());
    let display = editor.read(cx).model().display();
    let width = display.wrap_width();
    let changed = width != width_before;
    if !changed && frame < RESIZE_FRAMES || display.wrap_is_pending() {
        drop(state);
        window.on_next_frame(move |window, cx| {
            wrap_tick(
                run,
                step,
                name,
                (started, command, width_before),
                frame + 1,
                window,
                cx,
            );
        });
        return;
    }
    if changed {
        say!(
            "{name}: wrap width {width_before:?} -> {width:?}, {} display rows, \
             wrapped {:.0?} after the command (which took {})",
            display.row_count(),
            started.elapsed(),
            millis(command),
        );
        report(&name, &state.frames);
    } else {
        say!("{name}: the wrap width didn't change in {RESIZE_FRAMES} frames");
    }
    state.frames.clear();
    drop(state);
    wrap_step(run, step + 1, window, cx);
}

fn idle(run: Rc<RefCell<Run>>, cx: &mut App) {
    let editor = run
        .borrow()
        .editor
        .clone()
        .expect("scrolling needs the editor");
    cx.spawn(async move |cx| {
        cx.background_executor().timer(IDLE_SETTLE).await;
        cx.update(|cx| editor.update(cx, |view, _| view.frame_timings_mut().clear()));
        let (cpu, started) = (cpu_time(), Instant::now());
        cx.background_executor().timer(IDLE).await;
        let used = cpu_time() - cpu;
        let painted = cx.update(|cx| editor.read(cx).frame_timings().frames().len());
        say!(
            "idle: {:.2?} CPU in {:.1?} ({:.2}% of a core), {painted} frames painted",
            used,
            started.elapsed(),
            100. * used.as_secs_f64() / started.elapsed().as_secs_f64()
        );
        cx.update(|cx| cx.quit());
    })
    .detach();
}

fn report(phase: &str, frames: &[FrameTiming]) {
    let mut prepaint: Vec<_> = frames.iter().map(|frame| frame.prepaint).collect();
    let mut paint: Vec<_> = frames.iter().map(|frame| frame.paint).collect();
    let mut intervals: Vec<_> = frames
        .windows(2)
        .map(|pair| pair[1].painted_at - pair[0].painted_at)
        .collect();
    let missed = intervals.iter().filter(|gap| **gap > MISSED_VSYNC).count();
    let span = match (frames.first(), frames.last()) {
        (Some(first), Some(last)) => last.painted_at - first.painted_at,
        _ => Duration::ZERO,
    };
    let fps = intervals.len() as f64 / span.as_secs_f64().max(f64::EPSILON);
    say!(
        "{phase}: {} frames painted at {fps:.0} fps, {missed} intervals over {MISSED_VSYNC:.1?}",
        frames.len(),
    );
    say!("  prepaint  {}", percentiles(&mut prepaint));
    say!("  paint     {}", percentiles(&mut paint));
    for (pair, gap) in frames.windows(2).zip(&intervals) {
        if *gap > STALL {
            let at = pair[0].painted_at - frames[0].painted_at;
            say!("  stall: {gap:.0?} after frame at {at:.2?}");
        }
    }
    say!("  interval  {}", percentiles(&mut intervals));
}

fn percentiles(samples: &mut [Duration]) -> String {
    if samples.is_empty() {
        return "no samples".into();
    }
    samples.sort_unstable();
    let at = |q: f64| samples[((samples.len() - 1) as f64 * q).round() as usize];
    format!(
        "p50 {:>8.2?}  p99 {:>8.2?}  max {:>8.2?}",
        at(0.5),
        at(0.99),
        at(1.)
    )
}

#[cfg(target_os = "macos")]
fn print_memory(when: &str, file_bytes: usize) {
    // SAFETY: `info` is a `rusage_info_v4`, the flavour asked for.
    let mut info: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
    let result = unsafe {
        libc::proc_pid_rusage(
            libc::getpid(),
            libc::RUSAGE_INFO_V4,
            (&raw mut info).cast::<libc::rusage_info_t>(),
        )
    };
    if result != 0 {
        say!("memory {when}: unavailable");
        return;
    }
    // SAFETY: `getrusage` fills in the struct it's given.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    let mb = |bytes: u64| bytes as f64 / (1024. * 1024.);
    say!(
        "memory {when}: footprint {:.0} MB, peak {:.0} MB; resident {:.0} MB, peak {:.0} MB; file {:.0} MB",
        mb(info.ri_phys_footprint),
        mb(info.ri_lifetime_max_phys_footprint),
        mb(info.ri_resident_size),
        // Bytes on macOS.
        mb(usage.ru_maxrss as u64),
        mb(file_bytes as u64),
    );
}

/// On Linux, the resident set and its peak from `/proc/self/status`; there
/// is no physical footprint as macOS counts it.
#[cfg(target_os = "linux")]
fn print_memory(when: &str, file_bytes: usize) {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    // `VmRSS:     123456 kB`
    let kilobytes = |field: &str| {
        status
            .lines()
            .find_map(|line| line.strip_prefix(field))
            .and_then(|rest| rest.trim().strip_suffix(" kB"))
            .and_then(|value| value.trim().parse::<u64>().ok())
    };
    let (Some(resident), Some(peak)) = (kilobytes("VmRSS:"), kilobytes("VmHWM:")) else {
        say!("memory {when}: unavailable");
        return;
    };
    let mb = |bytes: u64| bytes as f64 / (1024. * 1024.);
    say!(
        "memory {when}: resident {:.0} MB, peak {:.0} MB; file {:.0} MB",
        mb(resident * 1024),
        mb(peak * 1024),
        mb(file_bytes as u64),
    );
}

/// Elsewhere only the resident peak from `getrusage` is reported.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn print_memory(when: &str, file_bytes: usize) {
    // SAFETY: `getrusage` fills in the struct it's given.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    let mb = |bytes: u64| bytes as f64 / (1024. * 1024.);
    say!(
        "memory {when}: resident peak {:.0} MB; file {:.0} MB",
        // Kilobytes on the BSDs.
        mb(usage.ru_maxrss as u64 * 1024),
        mb(file_bytes as u64),
    );
}

/// User plus system CPU time used by this process so far.
fn cpu_time() -> Duration {
    // SAFETY: `getrusage` fills in the struct it's given.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    let duration = |time: libc::timeval| {
        Duration::from_secs(time.tv_sec as u64) + Duration::from_micros(time.tv_usec as u64)
    };
    duration(usage.ru_utime) + duration(usage.ru_stime)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    #[test]
    fn file_is_shown_when_the_first_request_after_its_paint_returns() {
        let t = Instant::now();
        let request = |started, returned: Option<u64>| FrameRequest {
            started: at(t, started),
            returned: returned.map(|ms| at(t, ms)),
        };
        let requests = [
            request(10, Some(14)),
            request(18, Some(22)),
            request(26, None),
        ];
        // Drawn when the window opened: the first request presents it.
        assert_eq!(presented_after(at(t, 5), &requests), Some(at(t, 14)));
        // Drawn during the second request, which presents it.
        assert_eq!(presented_after(at(t, 20), &requests), Some(at(t, 22)));
        // Drawn during a request that hasn't returned yet.
        assert_eq!(presented_after(at(t, 27), &requests), None);
    }

    #[test]
    fn first_prepaint_is_when_the_editor_started_it() {
        // Other elements prepaint for 5 ms between the editor's prepaint
        // and paint, so durations back from the paint would put it at 107.
        let t = Instant::now();
        let editor = FrameTiming {
            prepaint_started: at(t, 100),
            prepaint: Duration::from_millis(2),
            paint: Duration::from_millis(1),
            painted_at: at(t, 108),
        };
        let startup = Startup {
            process: Some(t),
            main: at(t, 5),
            platform: None,
            app: None,
            view: None,
            window: None,
        };
        let phases = startup_phases(&startup, at(t, 112), at(t, 115), false, editor, at(t, 115));
        let (name, prepaint) = &phases[1];
        assert!(name.starts_with("editor's first prepaint starts"), "{name}");
        assert_eq!(*prepaint, at(t, 100));
    }
}
