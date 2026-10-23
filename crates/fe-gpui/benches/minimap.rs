//! Minimap frame time, headless: the scroll bench's cases that stress the
//! minimap most, and typing in a big highlighted file, each with the minimap
//! off and on. Frames are timed as the scroll and keystroke benches time
//! them, from dispatching the event until the frame is drawn and presented,
//! with the editor's own prepaint and paint. The frame callbacks the
//! minimap schedules (its build continuations and decoration sweep) are
//! delivered before each sampled draw, as the platform does at the start of
//! a frame, and their time is part of the frame and reported on its own.
//!
//! The two are measured alternately in one window, [`ROUNDS`] times over,
//! so load on the machine falls on both alike; compare them by the fastest
//! sample, and by p50 (PERF.md, "Measuring"). The budget is a frame p99 of
//! at most 8.33 ms (120 fps), and a jump's p50 within 0.5 ms of the minimap
//! off. Nothing asserts either; the results go in PERF.md.
//!
//! Run with `cargo bench -p fe-gpui --bench minimap`; `-- jump/7mb-rust`
//! runs just one case.

use std::time::{Duration, Instant};

use gpui::{
    AppContext as _, Modifiers, PlatformInput, ScrollDelta, ScrollWheelEvent, TouchPhase, point, px,
};
use harness::{Case, Harness};

// Shared with the keystroke bench and test, which use the rest.
#[allow(dead_code)]
#[path = "keystroke/harness.rs"]
mod harness;

/// Rounds of the minimap off then on: short ones, so a change in the
/// machine's load falls on both alike.
const ROUNDS: usize = 6;
/// Frames scrolled per case, each round and setting.
const SCROLL_FRAMES: usize = 100;
/// Keystrokes timed per case, each round and setting.
const KEYSTROKES: usize = 25;
/// Rounds of delivering frame callbacks after which a settle gives up.
const MAX_DRAIN_FRAMES: usize = 10_000;
/// A 120 Hz frame.
const BUDGET: Duration = Duration::from_micros(8_333);
/// About two rows a frame: a brisk trackpad scroll.
const SMOOTH_STEP: f32 = 40.;
/// About a thousand rows a frame: dragging a scrollbar through a big file.
const JUMP_STEP: f32 = 21_000.;

enum Drive {
    /// Scrolls this many pixels a frame, soft wrapped or not.
    Scroll { step: f32, wrap: bool },
    /// Types the case's keystroke, undone between samples.
    Type,
}

struct Bench {
    case: Case,
    drive: Drive,
}

fn benches() -> [Bench; 4] {
    let plain = harness::cases()[2];
    let rust = Case {
        corpus: || {
            let text = rust(7 << 20);
            // Inside a word of a body row in the middle.
            let caret = text[text.len() / 2..].find("let doubled").unwrap() + text.len() / 2;
            (text, caret + "let dou".len())
        },
        language: Some("rust"),
        ..plain
    };
    [
        Bench {
            case: Case {
                name: "smooth/1m-plain",
                ..plain
            },
            drive: Drive::Scroll {
                step: SMOOTH_STEP,
                wrap: false,
            },
        },
        Bench {
            case: Case {
                name: "jump/7mb-rust",
                ..rust
            },
            drive: Drive::Scroll {
                step: JUMP_STEP,
                wrap: false,
            },
        },
        Bench {
            case: Case {
                name: "jump/1m-plain-wrapped",
                ..plain
            },
            drive: Drive::Scroll {
                step: JUMP_STEP,
                wrap: true,
            },
        },
        Bench {
            case: Case {
                name: "type/7mb-rust",
                ..rust
            },
            drive: Drive::Type,
        },
    ]
}

/// Six-line Rust functions up to at least `bytes` bytes, as the scroll
/// bench writes them: under Rust's 7.5 MiB parse limit for 7 MiB.
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

/// One frame.
#[derive(Clone, Copy)]
struct Frame {
    /// From the event to the presented scene, with the callbacks.
    total: Duration,
    /// The frame callbacks delivered before the draw.
    sweep: Duration,
    /// Whether any ran: a continuation frame.
    continued: bool,
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
    for bench in benches() {
        let name = bench.case.name;
        if !filter.is_empty() && !filter.iter().any(|f| name.contains(f.as_str())) {
            continue;
        }
        let mut harness = Harness::new(
            text_system.clone(),
            gpui_platform::current_headless_renderer,
            &bench.case,
        );
        if let Drive::Scroll { wrap: true, .. } = bench.drive {
            harness.cx.update_entity(&harness.view, |view, cx| {
                view.model_mut().set_soft_wrap(true);
                cx.notify();
            });
        }
        let mut frames: [Vec<Frame>; 2] = Default::default();
        let mut settle_frames = [0; 2];
        for _ in 0..ROUNDS {
            for ((minimap, frames), settled) in [false, true]
                .into_iter()
                .zip(&mut frames)
                .zip(&mut settle_frames)
            {
                harness.set_minimap(minimap);
                settle(&mut harness, name, settled);
                frames.extend(match bench.drive {
                    Drive::Scroll { step, .. } => scroll(&mut harness, step, name, settled),
                    Drive::Type => type_keys(&mut harness, name, settled),
                });
            }
        }
        let [off, on] = frames;
        print(name, "off", &off, settle_frames[0]);
        print(name, "on", &on, settle_frames[1]);
    }
}

/// Delivers the frame callbacks scheduled so far, as the start of a frame
/// does, and returns how many ran. Untimed: the window draws after.
fn pump(harness: &mut Harness) -> usize {
    harness
        .cx
        .update_window(harness.window.into(), |_, window, cx| {
            window.simulate_next_frame(cx)
        })
        .expect("the window is open")
}

/// Draws frames, delivering their callbacks first, until none is scheduled:
/// the minimap's tiles and sweep are complete. Adds the frames it took to
/// `frames`.
fn drain(harness: &mut Harness, name: &str, frames: &mut usize) {
    let window = harness.window;
    for _ in 0..MAX_DRAIN_FRAMES {
        harness.cx.run_until_parked();
        let ran = pump(harness);
        harness
            .cx
            .update_window(window.into(), |_, _, _| {})
            .expect("the window is open");
        if ran == 0 {
            return;
        }
        *frames += 1;
    }
    panic!("{name}: frame callbacks kept being scheduled after {MAX_DRAIN_FRAMES} frames");
}

/// Lets the background parse land and the wrap finish, then drains the
/// minimap's continuations, counting their frames in `drained`.
fn settle(harness: &mut Harness, name: &str, drained: &mut usize) {
    let window = harness.window;
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
    let highlighted = harness.cx.read_entity(&harness.view, |view, _| {
        view.syntax().is_none_or(|syntax| {
            !syntax.needs_parse() && !syntax.failed() && syntax.tree().is_some()
        })
    });
    assert!(highlighted, "{name}: the file isn't highlighted");
    drain(harness, name, drained);
}

/// Scrolls [`SCROLL_FRAMES`] frames of `step` pixels from the top, as the
/// scroll bench does, wrapping to the top at the end of the file.
fn scroll(harness: &mut Harness, step: f32, name: &str, drained: &mut usize) -> Vec<Frame> {
    let window = harness.window;
    let viewport = harness
        .cx
        .update_window(window.into(), |_, window, _| window.viewport_size())
        .expect("the window is open");
    // The rendered row height: the theme's multiplier on its font size.
    let row_px = harness.cx.update(|cx| {
        let theme = fe_gpui::EditorTheme::default_dark(cx);
        f32::from((theme.font_size * theme.line_height).round()).max(1.)
    });
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
    draw(harness);
    drain(harness, name, drained);

    let mut frames = Vec::with_capacity(SCROLL_FRAMES);
    let mut before = scrolled_px(harness);
    for _ in 0..SCROLL_FRAMES {
        if !frames.is_empty() && scrolled_px(harness) - before < step - 1. {
            harness
                .cx
                .update_entity(&harness.view, |view, cx| view.set_scroll_top(0., cx));
            draw(harness);
            drain(harness, name, drained);
        }
        before = scrolled_px(harness);
        harness
            .cx
            .update_entity(&harness.view, |view, _| view.frame_timings_mut().clear());
        let dispatched = Instant::now();
        // The callbacks and the event in one update, so effects flush into
        // one draw.
        let (ran, sweep) = harness
            .cx
            .update_window(window.into(), |_, window, cx| {
                let started = Instant::now();
                let ran = window.simulate_next_frame(cx);
                let sweep = started.elapsed();
                window.dispatch_event(
                    PlatformInput::ScrollWheel(ScrollWheelEvent {
                        // Over the text.
                        position: point(viewport.width / 3., viewport.height / 2.),
                        delta: ScrollDelta::Pixels(point(px(0.), px(-step))),
                        modifiers: Modifiers::default(),
                        touch_phase: TouchPhase::Moved,
                    }),
                    cx,
                );
                (ran, sweep)
            })
            .expect("the window is open");
        // Draws the window if the event didn't.
        draw(harness);
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
        let painted = painted.unwrap_or_else(|| panic!("{name}: no frame drawn"));
        frames.push(Frame {
            total: presented - dispatched,
            sweep,
            continued: ran > 0,
            prepaint: painted.prepaint,
            paint: painted.paint,
        });
    }
    frames
}

/// Types the case's keystroke [`KEYSTROKES`] times, each undone and
/// settled before the next.
fn type_keys(harness: &mut Harness, name: &str, drained: &mut usize) -> Vec<Frame> {
    harness.pump_frames = true;
    (0..KEYSTROKES)
        .map(|_| {
            let sample = harness
                .keystroke("x")
                .unwrap_or_else(|err| panic!("{name}: {err:?}"));
            let (ran, sweep) = harness.pumped;
            harness.reset();
            drain(harness, name, drained);
            let stages = sample.stages();
            Frame {
                total: sample.total(),
                sweep,
                continued: ran > 0,
                prepaint: stages[2],
                paint: stages[3],
            }
        })
        .collect()
}

fn print(name: &str, minimap: &str, frames: &[Frame], settle_frames: usize) {
    let percentiles = |what: &str, mut values: Vec<Duration>| {
        values.sort();
        let ms = |value: Duration| value.as_secs_f64() * 1e3;
        let at = |p: usize| ms(values[(values.len() - 1) * p / 100]);
        let (fastest, max) = (ms(values[0]), ms(*values.last().expect("frames")));
        eprintln!(
            "  {what:<12}{fastest:>8.3} {:>8.3} {:>8.3} {max:>8.3}",
            at(50),
            at(99)
        );
    };
    let over = frames.iter().filter(|frame| frame.total > BUDGET).count();
    eprintln!(
        "{name}, minimap {minimap}: {} frames, ms  fastest      p50      p99      max",
        frames.len()
    );
    percentiles("frame", frames.iter().map(|f| f.total).collect());
    percentiles("prepaint", frames.iter().map(|f| f.prepaint).collect());
    percentiles("paint", frames.iter().map(|f| f.paint).collect());
    percentiles("callbacks", frames.iter().map(|f| f.sweep).collect());
    let continued = frames.iter().filter(|frame| frame.continued).count();
    eprintln!(
        "  continuation frames: {continued} of {} sampled, {settle_frames} more in settles and between samples",
        frames.len()
    );
    eprintln!("  frames over 8.33 ms: {over} of {}", frames.len());
}
