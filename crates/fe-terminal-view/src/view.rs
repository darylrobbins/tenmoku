use std::cell::Cell;
use std::io;
use std::ops::Range;
use std::rc::Rc;

use fe_terminal::{
    GridSize, Terminal, TerminalEvent, TerminalMatch, TerminalOptions, TerminalQuery,
};
use gpui::{
    App, Bounds, ClipboardItem, Context, EventEmitter, FocusHandle, Focusable, InteractiveElement,
    IntoElement, KeyContext, ParentElement, Pixels, Render, Styled, Subscription, Task, Window,
    canvas, div, prelude::FluentBuilder as _, px,
};

use crate::TerminalStyle;
use crate::element::{PaintReport, TerminalElement};
use crate::mouse::{Button, Drag, WheelTo};
use crate::vi::ViKeys;

/// The key context a terminal view's bindings are scoped to.
pub const KEY_CONTEXT: &str = "terminal";

/// The key context identifier with which an element keeps keystrokes from
/// the app's keymap: `kiln_platform::keys::HOLDS_KEYS`. Vi mode reads its
/// keys itself, ctrl-b and the rest, whatever the keymap binds them to.
const HOLDS_KEYS: &str = "holds_keys";

/// What a [`TerminalView`] tells its owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalViewEvent {
    /// The shell exited, with its exit code (`None` when a signal ended it,
    /// the code is unknown, or the session couldn't start).
    Exited(Option<i32>),
}

/// A terminal session drawn in a window. The session starts on the view's
/// first layout, once its size in cells is known, and is resized whenever
/// that changes.
pub struct TerminalView {
    /// Taken when the session starts.
    options: Option<TerminalOptions>,
    pub(crate) terminal: Option<Terminal>,
    pub(crate) style: TerminalStyle,
    pub(crate) focus: FocusHandle,
    /// The size last given to the session.
    size: Option<GridSize>,
    title: Option<String>,
    /// Drains the session's events.
    events: Option<Task<()>>,
    /// A wakeup has notified since the last frame was laid out.
    notified: bool,
    /// The text an input method is composing, not yet written to the pty.
    pub(crate) marked: Option<String>,
    /// The input method's selection within `marked`, in UTF-16 units.
    pub(crate) marked_selection: Range<usize>,
    /// The selecting drag, while the left button holds it.
    pub(crate) drag: Option<Drag>,
    /// Buttons the program heard pressed and not yet released, oldest first.
    pub(crate) reported_buttons: Vec<Button>,
    /// The cell a press, drag or motion was last reported at.
    pub(crate) reported: Option<(usize, usize)>,
    /// Wheel motion, in lines, not yet scrolled or reported.
    pub(crate) wheel: f32,
    /// Where `wheel` was headed.
    pub(crate) wheel_to: WheelTo,
    /// The query whose matches are highlighted.
    pub(crate) find: Option<TerminalQuery>,
    /// The query's matches the last prepaint found on screen.
    pub(crate) find_matches: Vec<TerminalMatch>,
    /// Whether a selection is written to the Linux primary selection and a
    /// middle click pastes from it.
    pub(crate) primary_selection: bool,
    /// The text this view last wrote to the primary selection.
    pub(crate) last_primary: Option<String>,
    /// What the last prepaint laid out.
    pub(crate) painted: PaintReport,
    /// Whether prepaint fills `painted`: off outside tests.
    pub(crate) record_paint: bool,
    /// Vi copy mode's count, pending key and search prompt.
    pub(crate) vi: ViKeys,
    /// Where the last frame drew the vi search prompt.
    vi_prompt_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    _focus_reports: [Subscription; 2],
    /// Redraws the cursor when the window gains or loses focus.
    _activation: Subscription,
}

impl EventEmitter<TerminalViewEvent> for TerminalView {}

impl TerminalView {
    /// A view that will run `options`' shell, drawn in `style`. Fails when
    /// the working directory isn't one.
    pub fn new(
        options: TerminalOptions,
        style: TerminalStyle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> io::Result<Self> {
        if !std::fs::metadata(&options.cwd)?.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                format!("{} is not a directory", options.cwd.display()),
            ));
        }
        let focus = cx.focus_handle();
        let focus_reports = [
            cx.on_focus_in(&focus, window, |view, _, _| view.report_focus(true)),
            cx.on_focus_out(&focus, window, |view, _, _, _| view.report_focus(false)),
        ];
        let activation = cx.observe_window_activation(window, |_, _, cx| cx.notify());
        Ok(Self {
            options: Some(options),
            terminal: None,
            style,
            focus,
            size: None,
            title: None,
            events: None,
            notified: false,
            marked: None,
            marked_selection: 0..0,
            drag: None,
            reported_buttons: Vec::new(),
            reported: None,
            wheel: 0.,
            wheel_to: WheelTo::default(),
            find: None,
            find_matches: Vec::new(),
            primary_selection: cfg!(any(target_os = "linux", target_os = "freebsd")),
            last_primary: None,
            painted: PaintReport::default(),
            record_paint: false,
            vi: ViKeys::default(),
            vi_prompt_bounds: Rc::default(),
            _focus_reports: focus_reports,
            _activation: activation,
        })
    }

    /// Turns the Linux primary selection on or off: while on, a selection
    /// made with the mouse is written to it and a middle click pastes from it.
    pub fn set_primary_selection(&mut self, on: bool) {
        self.primary_selection = on;
    }

    /// Whether the view has focus in an active window: what decides a filled
    /// or hollow cursor.
    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus.is_focused(window) && window.is_window_active()
    }

    /// Starts or stops recording what each frame lays out, for
    /// [`painted`](Self::painted). Off by default: recording allocates on
    /// every prepaint. For tests.
    #[doc(hidden)]
    pub fn record_paint(&mut self, on: bool) {
        self.record_paint = on;
        if !on {
            self.painted = PaintReport::default();
        }
    }

    /// What the last frame laid out, once recording. For tests.
    #[doc(hidden)]
    pub fn painted(&self) -> &PaintReport {
        &self.painted
    }

    /// Where the last frame drew the vi search prompt, if it drew one. For
    /// tests.
    #[doc(hidden)]
    pub fn vi_prompt_bounds(&self) -> Option<Bounds<Pixels>> {
        self.vi_prompt_bounds.get()
    }

    pub fn style(&self) -> &TerminalStyle {
        &self.style
    }

    pub fn set_style(&mut self, style: TerminalStyle, cx: &mut Context<Self>) {
        if self.style != style {
            self.style = style;
            cx.notify();
        }
    }

    /// The title the program last set, if any.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// The session, once the view has been laid out.
    pub fn terminal(&self) -> Option<&Terminal> {
        self.terminal.as_ref()
    }

    /// The size last given to the session.
    pub fn grid_size(&self) -> Option<GridSize> {
        self.size
    }

    /// Starts the session at `size`, or resizes it when that changed. Called
    /// from the element's prepaint.
    pub(crate) fn fit(&mut self, size: GridSize, cx: &mut Context<Self>) {
        self.notified = false;
        if self.size == Some(size) {
            return;
        }
        self.size = Some(size);
        if let Some(terminal) = &self.terminal {
            terminal.resize(size);
            return;
        }
        let Some(options) = self.options.take() else {
            return;
        };
        match Terminal::spawn(options, size) {
            Ok((terminal, events)) => {
                self.terminal = Some(terminal);
                self.events = Some(cx.spawn(async move |this, cx| {
                    while let Ok(event) = events.recv().await {
                        if this.update(cx, |view, cx| view.handle(event, cx)).is_err() {
                            break;
                        }
                    }
                }));
            }
            Err(err) => {
                log::error!("couldn't start the terminal: {err}");
                cx.emit(TerminalViewEvent::Exited(None));
            }
        }
    }

    fn handle(&mut self, event: TerminalEvent, cx: &mut Context<Self>) {
        match event {
            TerminalEvent::Wakeup => {
                if !self.notified {
                    self.notified = true;
                    cx.notify();
                }
            }
            TerminalEvent::Title(title) => {
                self.title = (!title.is_empty()).then_some(title);
                cx.notify();
            }
            TerminalEvent::Exited(code) => {
                cx.notify();
                cx.emit(TerminalViewEvent::Exited(code));
            }
            TerminalEvent::ClipboardStore(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
            // Bells wait for the card that handles them.
            TerminalEvent::Bell => {}
        }
    }

    /// Sends `CSI I` or `CSI O` when the program asked to hear of focus,
    /// leaving the view where it is.
    fn report_focus(&self, focused: bool) {
        let Some(terminal) = &self.terminal else {
            return;
        };
        if terminal.snapshot().modes.focus_reporting {
            terminal.input_in_place(if focused { &b"\x1b[I"[..] } else { b"\x1b[O" });
        }
    }

    /// The vi search prompt, a row across the bottom of the view drawn over
    /// the grid: `/` or `?`, the query and a caret.
    fn vi_prompt_row(&self, text: String) -> impl IntoElement {
        let bounds = self.vi_prompt_bounds.clone();
        let height = (self.style.font_size * self.style.line_height).round();
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .h(height)
            .flex()
            .items_center()
            .bg(self.style.background)
            .text_color(self.style.foreground)
            .font(self.style.font())
            .text_size(self.style.font_size)
            .line_height(height)
            .child(text)
            .child(
                div()
                    .w(px(2.))
                    .h(self.style.font_size)
                    .bg(self.style.cursor),
            )
            .child(
                canvas(move |drawn, _, _| bounds.set(Some(drawn)), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
    }
}

impl Render for TerminalView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let prompt = self.vi.prompt();
        if prompt.is_none() {
            self.vi_prompt_bounds.set(None);
        }
        let mut context = KeyContext::default();
        context.add(KEY_CONTEXT);
        if self.is_vi_mode() {
            context.add(HOLDS_KEYS);
        }
        div()
            .key_context(context)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .relative()
            .size_full()
            .bg(self.style.background)
            .child(TerminalElement::new(cx.entity()))
            .when_some(prompt, |view, prompt| {
                view.child(self.vi_prompt_row(prompt))
            })
    }
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
