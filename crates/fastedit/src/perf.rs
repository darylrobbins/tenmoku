//! `fastedit --perf <file>`: measures the M2 large-file targets in a real
//! window, prints them and quits. See PERF.md for how to read the output.
//!
//! 1. Open: time from launch to the first painted frame, and memory then.
//! 2. Scroll: one scroll-wheel event per frame, first smoothly and then in
//!    large jumps, timing every frame the editor paints.
//! 3. Idle: CPU used over a few seconds with nothing happening.

use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

use fe_gpui::{EditorView, FrameTiming};
use gpui::{
    App, Entity, Modifiers, PlatformInput, ScrollDelta, ScrollWheelEvent, TouchPhase, Window,
    point, px,
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

/// `println!` that ignores a closed stdout (`fastedit --perf | head`): a
/// panic inside a platform callback aborts the app.
macro_rules! say {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stdout(), $($arg)*);
    }};
}

struct Run {
    editor: Entity<EditorView>,
    launched: Instant,
    file_bytes: usize,
    frame: u32,
    frames: Vec<FrameTiming>,
}

pub(crate) fn start(
    editor: Entity<EditorView>,
    launched: Instant,
    window: &mut Window,
    cx: &mut App,
) {
    let file_bytes = editor.read(cx).model().text().len();
    let run = Rc::new(RefCell::new(Run {
        editor,
        launched,
        file_bytes,
        frame: 0,
        frames: Vec::new(),
    }));
    window.on_next_frame(move |window, cx| tick(run, window, cx));
}

/// Runs at the start of every frame until the scroll phases are over.
fn tick(run: Rc<RefCell<Run>>, window: &mut Window, cx: &mut App) {
    let mut state = run.borrow_mut();
    let painted = state.editor.read(cx).frame_timings().frames().copied();
    state.frames.extend(painted);
    state
        .editor
        .update(cx, |view, _| view.frame_timings_mut().clear());

    if state.frame == 0 {
        // Wait for the first painted frame.
        let Some(first) = state.frames.first() else {
            drop(state);
            window.on_next_frame(move |window, cx| tick(run, window, cx));
            return;
        };
        say!(
            "open: first frame painted {:.0?} after launch, window active: {}",
            first.painted_at - state.launched,
            window.is_window_active()
        );
        print_memory("after open", state.file_bytes);
    }

    let scroll_end = SETTLE_FRAMES + SCROLL_FRAMES;
    let step = match state.frame {
        frame if frame < SETTLE_FRAMES => None,
        frame if frame < scroll_end => Some(SMOOTH_STEP),
        frame if frame < scroll_end + SCROLL_FRAMES => Some(JUMP_STEP),
        _ => None,
    };
    if state.frame == SETTLE_FRAMES {
        state.frames.clear();
    }
    if state.frame == scroll_end {
        report("smooth scroll", &state.frames);
        state.frames.clear();
    }
    if state.frame == scroll_end + SCROLL_FRAMES {
        report("jump scroll", &state.frames);
        print_memory("after scrolling", state.file_bytes);
        drop(state);
        idle(run, cx);
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

fn idle(run: Rc<RefCell<Run>>, cx: &mut App) {
    let editor = run.borrow().editor.clone();
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
    let mb = |bytes: u64| bytes as f64 / (1024. * 1024.);
    say!(
        "memory {when}: footprint {:.0} MB, peak {:.0} MB, file {:.0} MB",
        mb(info.ri_phys_footprint),
        mb(info.ri_lifetime_max_phys_footprint),
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
