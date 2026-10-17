//! Scrolling frame time, headless: scroll-wheel events go through the real
//! window into a focused `EditorView` over a big file, and each frame is
//! timed from the event's dispatch until the frame is drawn and presented.
//! Prepaint and paint are the editor's own `frame_timings()`.
//!
//! Like the keystroke bench, it uses GPUI's test window with the real macOS
//! text system and the headless Metal renderer, so no screen is needed and
//! a locked one doesn't matter. The numbers leave out vsync, the compositor
//! and the GPU's own time.
//!
//! The budget is a frame p99 of at most 8.33 ms (120 fps) in every case.
//! Nothing asserts it; the results go in PERF.md.
//!
//! Run with `cargo bench -p fe-gpui --bench scroll`; `-- jump/7mb-rust` runs
//! just one case.

use std::time::{Duration, Instant};

use gpui::{
    AppContext as _, Modifiers, PlatformInput, ScrollDelta, ScrollWheelEvent, TouchPhase, point, px,
};
use harness::{Case, Harness};

// Shared with the keystroke bench and test, which use the rest.
#[allow(dead_code)]
#[path = "keystroke/harness.rs"]
mod harness;

/// Frames scrolled per case.
const FRAMES: usize = 600;
/// A 120 Hz frame.
const BUDGET: Duration = Duration::from_micros(8_333);
/// About two rows a frame: a brisk trackpad scroll.
const SMOOTH_STEP: f32 = 40.;
/// About a thousand rows a frame: dragging a scrollbar through a big file.
const JUMP_STEP: f32 = 21_000.;

struct Scroll {
    case: Case,
    /// Pixels scrolled a frame.
    step: f32,
    wrap: bool,
}

fn scrolls() -> [Scroll; 4] {
    let plain = harness::cases()[2];
    [
        Scroll {
            case: Case {
                name: "smooth/1m-plain",
                ..plain
            },
            step: SMOOTH_STEP,
            wrap: false,
        },
        Scroll {
            case: Case {
                name: "jump/1m-plain",
                ..plain
            },
            step: JUMP_STEP,
            wrap: false,
        },
        Scroll {
            case: Case {
                name: "jump/7mb-rust",
                corpus: || (rust(7 << 20), 0),
                language: Some("rust"),
                ..plain
            },
            step: JUMP_STEP,
            wrap: false,
        },
        Scroll {
            case: Case {
                name: "jump/1m-plain-wrapped",
                ..plain
            },
            step: JUMP_STEP,
            wrap: true,
        },
    ]
}

/// Six-line Rust functions up to at least `bytes` bytes, which is under
/// Rust's 7.5 MiB parse limit for 7 MiB.
fn rust(bytes: usize) -> String {
    let mut text = String::with_capacity(bytes + 256);
    let mut n = 0;
    while text.len() < bytes {
        text += &format!(
            "/// Doubles the input and adds {n}.\npub fn function_{n}(input: u32) -> u32 {{\n    \
             let doubled = input.wrapping_mul(2) + {n};\n    \
             if doubled % 3 == 0 {{ doubled / 3 }} else {{ doubled + 1 }}\n}}\n\n"
        );
        n += 1;
    }
    text
}

/// One scroll frame.
struct Frame {
    total: Duration,
    prepaint: Duration,
    paint: Duration,
}

fn main() {
    let filter: Vec<String> = std::env::args()
        .skip(1)
        .filter(|arg| !arg.starts_with('-'))
        .collect();
    // Text shaping needs the main thread, which a `harness = false` bench has.
    let text_system = gpui_platform::current_platform(true).text_system();
    for scroll in scrolls() {
        if !filter.is_empty() && !filter.iter().any(|f| scroll.case.name.contains(f.as_str())) {
            continue;
        }
        let frames = run(&scroll, &text_system);
        print(scroll.case.name, &frames);
    }
}

fn run(scroll: &Scroll, text_system: &std::sync::Arc<dyn gpui::PlatformTextSystem>) -> Vec<Frame> {
    let mut harness = Harness::new(
        text_system.clone(),
        gpui_platform::current_headless_renderer,
        &scroll.case,
    );
    let window = harness.window;
    if scroll.wrap {
        harness.cx.update_entity(&harness.view, |view, cx| {
            view.model_mut().set_soft_wrap(true);
            cx.notify();
        });
    }
    // Lets the background parse land and the wrap finish.
    for _ in 0..10_000 {
        harness.cx.run_until_parked();
        harness
            .cx
            .update_window(window.into(), |_, _, _| {})
            .expect("the window is open");
        let settled = harness.cx.read_entity(&harness.view, |view, _| {
            !view.model().display().wrap_is_pending()
                && view.syntax().is_none_or(|syntax| !syntax.needs_parse())
        });
        if settled {
            break;
        }
    }
    if scroll.case.language.is_some() {
        let parsed = harness.cx.read_entity(&harness.view, |view, _| {
            view.syntax().is_some_and(|syntax| {
                !syntax.needs_parse() && !syntax.failed() && syntax.tree().is_some()
            })
        });
        assert!(parsed, "{}: the file isn't highlighted", scroll.case.name);
    }

    let viewport = harness
        .cx
        .update_window(window.into(), |_, window, _| window.viewport_size())
        .expect("the window is open");
    // The rendered row height: the theme's multiplier on its font size.
    let row_px = harness.cx.update(|cx| {
        let theme = fe_gpui::EditorTheme::default_dark(cx);
        f32::from((theme.font_size * theme.line_height).round()).max(1.)
    });
    // Starts at the top: the inherited plain corpus puts the caret mid-file.
    let draw = |harness: &mut Harness| {
        harness
            .cx
            .update_window(window.into(), |_, _, _| {})
            .expect("the window is open");
    };
    let scrolled_px = |harness: &Harness| {
        harness
            .cx
            .read_entity(&harness.view, |view, _| view.scroll_top() * row_px)
    };
    harness
        .cx
        .update_entity(&harness.view, |view, cx| view.set_scroll_top(0., cx));
    draw(&mut harness);

    let mut frames = Vec::with_capacity(FRAMES);
    let mut before = scrolled_px(&harness);
    for _ in 0..FRAMES {
        // A jump case wraps to the top once the last frame stopped short of
        // its step, which is the end of the file.
        if !frames.is_empty() && scrolled_px(&harness) - before < scroll.step - 1. {
            harness
                .cx
                .update_entity(&harness.view, |view, cx| view.set_scroll_top(0., cx));
            draw(&mut harness);
        }
        before = scrolled_px(&harness);
        let delta = scroll.step;
        harness
            .cx
            .update_entity(&harness.view, |view, _| view.frame_timings_mut().clear());
        let dispatched = Instant::now();
        harness
            .cx
            .update_window(window.into(), |_, window, cx| {
                window.dispatch_event(
                    PlatformInput::ScrollWheel(ScrollWheelEvent {
                        // Over the editor.
                        position: point(viewport.width / 3., viewport.height / 2.),
                        delta: ScrollDelta::Pixels(point(px(0.), px(-delta))),
                        modifiers: Modifiers::default(),
                        touch_phase: TouchPhase::Moved,
                    }),
                    cx,
                );
            })
            .expect("the window is open");
        // Draws the window if the event didn't.
        harness
            .cx
            .update_window(window.into(), |_, _, _| {})
            .expect("the window is open");
        let presented = harness
            .cx
            .update_window(window.into(), |_, window, _| {
                window.render_to_image().expect("the scene is presented");
                Instant::now()
            })
            .expect("the window is open");
        let painted = harness.cx.read_entity(&harness.view, |view, _| {
            view.frame_timings().frames().last().copied()
        });
        let painted = painted.unwrap_or_else(|| panic!("{}: no frame drawn", scroll.case.name));
        frames.push(Frame {
            total: presented - dispatched,
            prepaint: painted.prepaint,
            paint: painted.paint,
        });
    }
    frames
}

fn print(name: &str, frames: &[Frame]) {
    let percentiles = |what: &str, mut values: Vec<Duration>| {
        values.sort();
        let at = |p: usize| values[(values.len() - 1) * p / 100].as_secs_f64() * 1e3;
        let max = values.last().expect("frames").as_secs_f64() * 1e3;
        eprintln!("  {what:<26}{:>8.3} {:>8.3} {:>8.3}", at(50), at(99), max);
    };
    let over = frames.iter().filter(|frame| frame.total > BUDGET).count();
    eprintln!(
        "{name}: {} frames, ms        p50      p99      max",
        frames.len()
    );
    percentiles("frame", frames.iter().map(|f| f.total).collect());
    percentiles("prepaint", frames.iter().map(|f| f.prepaint).collect());
    percentiles("paint", frames.iter().map(|f| f.paint).collect());
    eprintln!("  frames over 8.33 ms: {over} of {}", frames.len());
}
