//! Dispatches one keystroke into a focused [`EditorView`] in a headless GPUI
//! window and times each stage until the frame showing its effect has been
//! drawn and presented. Shared by the `keystroke` bench and the
//! `keystroke_latency` test, which each use part of it.
//!
//! The window is GPUI's test window under [`HeadlessAppContext`], with its
//! deterministic dispatcher: no `NSWindow`, so a locked screen doesn't
//! matter. The caller picks the text system and the renderer.
//!
//! Only public API is used. GPUI's own way to present a test window's frame
//! (`Window::present_if_needed`) is compiled into GPUI's own tests only, so
//! [`Presenter`] reaches the drawn scene through `Window::render_to_image`
//! and hands it to the renderer's present path instead.

use std::{
    cell::Cell,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use fe_buffer::{Buffer, ResolvedSelection};
use fe_editor::EditorModel;
use fe_gpui::{EditorTheme, EditorView, FrameTiming};
use gpui::{
    AppContext as _, DevicePixels, Entity, Focusable as _, HeadlessAppContext, Keystroke,
    PlatformAtlas, PlatformHeadlessRenderer, PlatformTextSystem, Scene, Size, Subscription,
    WindowHandle, px, size,
};
use image::RgbaImage;

/// The `--perf` window's size, in logical pixels.
const WINDOW: (f32, f32) = (1200., 800.);

/// Frames driven after a keystroke before giving up on one that shows it.
const MAX_FRAMES: usize = 8;

/// Rounds of background work and redraws allowed before the view must have
/// stopped notifying.
const MAX_SETTLE_ROUNDS: usize = 1000;

/// One editor setup and the keystroke timed in it.
#[derive(Clone, Copy)]
pub(crate) struct Case {
    pub(crate) name: &'static str,
    /// The text, and the caret's offset in it.
    pub(crate) corpus: fn() -> (String, usize),
    /// The language the view highlights as; `None` for plain text.
    pub(crate) language: Option<&'static str>,
    /// The keystroke timed, as `Keystroke::parse` reads it.
    pub(crate) keys: &'static str,
    /// Whether each keystroke lands while the previous edit's reparse is
    /// still pending in the background: [`Harness::reset`] doesn't settle
    /// it, and [`Harness::keystroke`] asserts it's pending.
    pub(crate) during_parse: bool,
}

/// Samples between full settles of a [`Case::during_parse`] case, which
/// bounds the background work left pending.
const SETTLE_EVERY: usize = 50;

/// The cases the bench runs. Corpora are generated, so there are no fixture
/// files.
pub(crate) fn cases() -> [Case; 6] {
    [
        Case {
            name: "type/small",
            corpus: || rust(10, 8),
            language: Some("rust"),
            keys: "x",
            during_parse: false,
        },
        Case {
            name: "type/10k-rust",
            corpus: || rust(1667, 833),
            language: Some("rust"),
            keys: "x",
            during_parse: false,
        },
        Case {
            name: "type/1m-plain",
            corpus: || plain(1_000_000, 500_000),
            language: None,
            keys: "x",
            during_parse: false,
        },
        Case {
            name: "move/10k-rust",
            corpus: || rust(1667, 833),
            language: Some("rust"),
            keys: "right",
            during_parse: false,
        },
        Case {
            name: "type/during-parse",
            corpus: || rust(30_000, 15_000),
            language: Some("rust"),
            keys: "x",
            during_parse: true,
        },
        Case {
            name: "move/long-row",
            corpus: || long_row(10_000_000, 5_000_000),
            language: None,
            keys: "right",
            during_parse: false,
        },
    ]
}

/// `blocks` six-line Rust functions, with the caret inside a word of block
/// `caret_block`. 1,667 blocks is 10,002 lines.
fn rust(blocks: usize, caret_block: usize) -> (String, usize) {
    let mut text = String::new();
    let mut caret = 0;
    for n in 0..blocks {
        text += &format!(
            "/// Doubles the input and adds {n}.\npub fn function_{n}(input: u32) -> u32 {{\n"
        );
        if n == caret_block {
            caret = text.len() + "    let dou".len();
        }
        text += &format!(
            "    let doubled = input.wrapping_mul(2) + {n};\n    \
             if doubled % 3 == 0 {{ doubled / 3 }} else {{ doubled + 1 }}\n}}\n\n"
        );
    }
    (text, caret)
}

/// One row of `len` bytes of plain ASCII words, with the caret inside a
/// word at `caret`.
fn long_row(len: usize, caret: usize) -> (String, usize) {
    const WORDS: &str = "lorem ipsum dolor sit amet ";
    let mut text = WORDS.repeat(len / WORDS.len() + 1);
    text.truncate(len);
    // Not at a space, so the caret is inside a word.
    let caret = caret - caret % WORDS.len() + 2;
    (text, caret)
}

/// `lines` lines of 33 bytes, with the caret inside a word of line
/// `caret_line`.
fn plain(lines: usize, caret_line: usize) -> (String, usize) {
    let mut text = String::with_capacity(lines * 33);
    let mut caret = 0;
    for n in 0..lines {
        if n == caret_line {
            caret = text.len() + "line 0000000 of pl".len();
        }
        text += &format!("line {n:07} of plain text, here\n");
    }
    (text, caret)
}

/// Why a keystroke gave no sample.
#[derive(Debug, PartialEq)]
pub(crate) enum NoFrame {
    /// The view wasn't notified: the keystroke changed nothing.
    NotNotified,
    /// No frame laid out the editor after the notification within
    /// [`MAX_FRAMES`] frames.
    NotDrawn,
}

/// The stages of one keystroke, in order. [`Sample::stages`] gives their
/// durations, which add up to [`Sample::total`].
pub(crate) const STAGES: [&str; 6] = [
    "dispatch",
    "pre_layout",
    "prepaint",
    "paint",
    "draw_rest",
    "present",
];

/// When each stage of one keystroke started and ended.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sample {
    /// Just before the keystroke was dispatched.
    pub(crate) dispatched: Instant,
    /// The view's first notification after it: keymap matching, the action
    /// or input handler and the model's edit are done.
    pub(crate) notified: Instant,
    /// The editor element's prepaint started, in the first frame after the
    /// notification. Before it, the view rendered (bringing the syntax tree
    /// up to date, within the in-frame reparse budget) and GPUI laid out the
    /// element tree.
    pub(crate) prepaint_started: Instant,
    pub(crate) prepaint_ended: Instant,
    pub(crate) paint_started: Instant,
    pub(crate) painted: Instant,
    /// The window's draw returned.
    pub(crate) drawn: Instant,
    /// The renderer committed the scene; `drawn` with no renderer.
    pub(crate) presented: Instant,
}

impl Sample {
    fn new(dispatched: Instant, notified: Instant, frame: FrameTiming, drawn: Instant) -> Self {
        Self {
            dispatched,
            notified,
            prepaint_started: frame.prepaint_started,
            prepaint_ended: frame.prepaint_started + frame.prepaint,
            paint_started: frame.painted_at - frame.paint,
            painted: frame.painted_at,
            drawn,
            presented: drawn,
        }
    }

    /// Every instant, in the order they happen.
    pub(crate) fn instants(&self) -> [Instant; 8] {
        [
            self.dispatched,
            self.notified,
            self.prepaint_started,
            self.prepaint_ended,
            self.paint_started,
            self.painted,
            self.drawn,
            self.presented,
        ]
    }

    /// The duration of each of [`STAGES`]. `draw_rest` is all of the draw
    /// outside the editor's prepaint and paint after it: GPUI may prepaint
    /// other elements between the two, and finishes the frame after.
    pub(crate) fn stages(&self) -> [Duration; 6] {
        [
            self.notified - self.dispatched,
            self.prepaint_started - self.notified,
            self.prepaint_ended - self.prepaint_started,
            self.painted - self.paint_started,
            (self.paint_started - self.prepaint_ended) + (self.drawn - self.painted),
            self.presented - self.drawn,
        ]
    }

    /// From dispatch to present: the sum of [`Sample::stages`].
    pub(crate) fn total(&self) -> Duration {
        self.presented - self.dispatched
    }
}

/// A focused editor in a headless window, ready to time keystrokes.
pub(crate) struct Harness {
    // Dropped before `cx`, which shuts the app down.
    _observer: Subscription,
    pub(crate) view: Entity<EditorView>,
    pub(crate) window: WindowHandle<EditorView>,
    /// When the view was first notified since this was last cleared.
    notified: Rc<Cell<Option<Instant>>>,
    /// Whether the window got a renderer to present with.
    presents: Rc<Cell<bool>>,
    /// The keystroke [`Harness::reset`] undoes.
    last: Option<&'static str>,
    /// See [`Case::during_parse`].
    during_parse: bool,
    /// Resets since the last full settle, for [`Case::during_parse`].
    resets: usize,
    pub(crate) cx: HeadlessAppContext,
}

impl Harness {
    /// Opens `case`'s editor, focused, in a window the size of `--perf`'s,
    /// and waits for it to settle: its language loaded and parsed, and
    /// nothing left to redraw. `renderer` returns `None` for no renderer, so
    /// nothing is presented.
    pub(crate) fn new(
        text_system: Arc<dyn PlatformTextSystem>,
        renderer: impl Fn() -> gpui::Result<Option<Box<dyn PlatformHeadlessRenderer>>> + 'static,
        case: &Case,
    ) -> Self {
        let presents = Rc::new(Cell::new(false));
        let mut cx = HeadlessAppContext::with_platform(text_system, Arc::new(()), {
            let presents = presents.clone();
            move || {
                let renderer = renderer()?;
                presents.set(renderer.is_some());
                Ok(renderer.map(|renderer| Box::new(Presenter(renderer)) as _))
            }
        });
        cx.update(fe_gpui::init);
        let (text, caret) = (case.corpus)();
        let language = case.language;
        let window = cx
            .open_window(size(px(WINDOW.0), px(WINDOW.1)), move |window, cx| {
                let view = cx.new(|cx| {
                    let mut model = EditorModel::new(Buffer::new(&text), None);
                    model.set_selections([ResolvedSelection {
                        range: caret..caret,
                        reversed: false,
                    }]);
                    let mut view = EditorView::new(model, EditorTheme::default_dark(cx), cx);
                    view.set_language(language, cx);
                    view
                });
                window.focus(&view.focus_handle(cx), cx);
                view
            })
            .expect("the headless window opens");
        let view = cx
            .read_window(&window, |view, _| view)
            .expect("the window is open");
        let notified = Rc::new(Cell::new(None));
        let observer = cx.update(|cx| {
            let notified = notified.clone();
            cx.observe(&view, move |_, _| {
                if notified.get().is_none() {
                    notified.set(Some(Instant::now()));
                }
            })
        });
        let mut harness = Self {
            _observer: observer,
            view,
            window,
            notified,
            presents,
            last: None,
            during_parse: case.during_parse,
            resets: 0,
            cx,
        };
        harness.settle();
        if harness.during_parse {
            // Leaves a reparse pending for the first keystroke, as `reset`
            // does for the rest.
            harness.last = Some("x");
            harness.dispatch_untimed("x");
            harness.reset();
        }
        harness
    }

    fn dispatch_untimed(&mut self, keys: &str) {
        let keystroke = Keystroke::parse(keys).expect("a valid keystroke");
        self.cx
            .update_window(self.window.into(), |_, window, cx| {
                window.dispatch_keystroke(keystroke, cx);
            })
            .expect("the window is open");
    }

    /// Dispatches `keys` and times it until the first frame that lays out
    /// the editor after the view's notification has been drawn and
    /// presented. Frames are drawn as GPUI's test windows draw them, at the
    /// end of the update that dirtied the window; up to [`MAX_FRAMES`] more
    /// are driven if that one doesn't qualify.
    pub(crate) fn keystroke(&mut self, keys: &'static str) -> Result<Sample, NoFrame> {
        let keystroke = Keystroke::parse(keys).expect("a valid keystroke");
        if self.during_parse {
            let pending = self.cx.read_entity(&self.view, |view, _| {
                view.syntax().is_some_and(|syntax| syntax.needs_parse())
            });
            assert!(pending, "no background parse is pending to type during");
        }
        self.notified.set(None);
        let dispatched = Instant::now();
        self.cx
            .update_window(self.window.into(), |_, window, cx| {
                window.dispatch_keystroke(keystroke, cx);
            })
            .expect("the window is open");
        let mut drawn = Instant::now();
        let notified = self.notified.get().ok_or(NoFrame::NotNotified)?;
        self.last = Some(keys);
        let mut frame = self.frame_after(notified);
        for _ in 0..MAX_FRAMES {
            if frame.is_some() {
                break;
            }
            // Draws the window if it's dirty.
            self.cx
                .update_window(self.window.into(), |_, _, _| {})
                .expect("the window is open");
            drawn = Instant::now();
            frame = self.frame_after(notified);
        }
        let mut sample = Sample::new(dispatched, notified, frame.ok_or(NoFrame::NotDrawn)?, drawn);
        if self.presents.get() {
            sample.presented = self
                .cx
                .update_window(self.window.into(), |_, window, _| {
                    window.render_to_image().expect("the scene is presented");
                    Instant::now()
                })
                .expect("the window is open");
        }
        Ok(sample)
    }

    /// Undoes the last sampled keystroke, untimed, with `backspace` after a
    /// typed character or the opposite movement, then settles. The next
    /// keystroke starts from the same text, selections, scroll position and
    /// highlights. A [`Case::during_parse`] case doesn't settle, leaving the
    /// reparse pending, except every [`SETTLE_EVERY`] resets.
    pub(crate) fn reset(&mut self) {
        let Some(keys) = self.last.take() else {
            return;
        };
        let undo = match keys {
            "left" => "right",
            "right" => "left",
            "up" => "down",
            "down" => "up",
            _ => "backspace",
        };
        if self.during_parse {
            self.resets += 1;
            if self.resets.is_multiple_of(SETTLE_EVERY) {
                self.settle();
            }
        }
        self.cx
            .update_window(self.window.into(), |_, window, cx| {
                window.dispatch_keystroke(Keystroke::parse(undo).expect("a valid keystroke"), cx);
            })
            .expect("the window is open");
        if self.during_parse {
            // One frame: the reparse is too big for the frame's budget, so
            // it's moved to the background and left pending.
            self.cx
                .update_window(self.window.into(), |_, _, _| {})
                .expect("the window is open");
        } else {
            self.settle();
        }
    }

    /// Runs background work (language loads, parses moved off the frame)
    /// and draws until the view stops notifying.
    fn settle(&mut self) {
        for _ in 0..MAX_SETTLE_ROUNDS {
            self.notified.set(None);
            self.cx.run_until_parked();
            self.cx
                .update_window(self.window.into(), |_, _, _| {})
                .expect("the window is open");
            if self.notified.get().is_none() {
                return;
            }
        }
        panic!("the editor kept notifying after {MAX_SETTLE_ROUNDS} rounds");
    }

    /// The first editor frame whose prepaint started after `notified`.
    fn frame_after(&mut self, notified: Instant) -> Option<FrameTiming> {
        self.cx.read_entity(&self.view, |view, _| {
            view.frame_timings()
                .frames()
                .find(|frame| frame.prepaint_started > notified)
                .copied()
        })
    }
}

/// Presents through `Window::render_to_image`: encodes the scene and commits
/// it to the GPU, as presenting to a window does, without waiting for the
/// GPU or reading pixels back. The image returned is empty.
struct Presenter(Box<dyn PlatformHeadlessRenderer>);

impl PlatformHeadlessRenderer for Presenter {
    fn render_scene_to_image(
        &mut self,
        scene: &Scene,
        size: Size<DevicePixels>,
    ) -> gpui::Result<RgbaImage> {
        self.0.render_scene(scene, size)?;
        Ok(RgbaImage::new(0, 0))
    }

    fn render_scene(&mut self, scene: &Scene, size: Size<DevicePixels>) -> gpui::Result<()> {
        self.0.render_scene(scene, size)
    }

    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.0.sprite_atlas()
    }
}
