use std::io;
use std::ops::Range;

use fe_terminal::{GridSize, Terminal, TerminalEvent, TerminalOptions};
use gpui::{
    App, ClipboardItem, Context, EventEmitter, FocusHandle, Focusable, InteractiveElement,
    IntoElement, ParentElement, Render, Styled, Subscription, Task, Window, div,
};

use crate::TerminalStyle;
use crate::element::TerminalElement;
use crate::mouse::Drag;

/// The key context a terminal view's bindings are scoped to.
pub const KEY_CONTEXT: &str = "terminal";

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
    /// What a held mouse button is doing.
    pub(crate) drag: Option<Drag>,
    /// The cell a press, drag or motion was last reported at.
    pub(crate) reported: Option<(usize, usize)>,
    /// Wheel motion, in lines, not yet scrolled or reported.
    pub(crate) wheel: f32,
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
            reported: None,
            wheel: 0.,
            _focus_reports: focus_reports,
            _activation: activation,
        })
    }

    /// Whether the view has focus in an active window: what decides a filled
    /// or hollow cursor.
    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus.is_focused(window) && window.is_window_active()
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

    /// Sends `CSI I` or `CSI O` when the program asked to hear of focus.
    fn report_focus(&self, focused: bool) {
        let Some(terminal) = &self.terminal else {
            return;
        };
        if terminal.snapshot().modes.focus_reporting {
            terminal.input(if focused { &b"\x1b[I"[..] } else { b"\x1b[O" });
        }
    }
}

impl Render for TerminalView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .size_full()
            .bg(self.style.background)
            .child(TerminalElement::new(cx.entity()))
    }
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
