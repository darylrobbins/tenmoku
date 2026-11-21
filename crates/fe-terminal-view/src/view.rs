use std::cell::Cell;
use std::io;
use std::ops::Range;
use std::rc::Rc;

use fe_terminal::history::History;
use fe_terminal::{
    CommandBlock, CommandStatus, GridSize, OutputTap, Terminal, TerminalEvent, TerminalMatch,
    TerminalOptions, TerminalQuery,
};
use gpui::{
    App, Bounds, ClipboardItem, Context, EventEmitter, FocusHandle, Focusable, Hsla,
    InteractiveElement, IntoElement, KeyContext, ParentElement, Pixels, Render, SharedString,
    Styled, Subscription, Task, Window, canvas, div, prelude::FluentBuilder as _, px,
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
    /// The shell's foreground command started or ended, as its OSC 133
    /// marks say ([`Terminal::command_status`]): told as the session's
    /// output is read, whether or not the view is drawn.
    CommandStatus(CommandStatus),
}

/// How a view marks its session's command blocks (OSC 133;A to D,
/// [`Terminal::command_blocks`]): an edge along each finished block's rows,
/// [`TerminalView::BLOCK_EDGE`] wide down the grid's left edge and painted
/// under the text, and a label at the right of its prompt row. The host gives the colours and the
/// text, so this crate holds no UI text.
#[derive(Clone)]
pub struct CommandBlockLook {
    /// The edge of a block whose command exited 0.
    pub passed: Hsla,
    /// The edge of a block whose command exited with another code.
    pub failed: Hsla,
    /// The label's colour.
    pub label_color: Hsla,
    /// The label a block's prompt row shows, if any: asked of blocks that
    /// ended with an exit code.
    pub label: BlockLabel,
}

/// What gives a command block's label: [`CommandBlockLook::label`].
pub type BlockLabel = Rc<dyn Fn(&CommandBlock) -> Option<SharedString>>;

impl std::fmt::Debug for CommandBlockLook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandBlockLook")
            .field("passed", &self.passed)
            .field("failed", &self.failed)
            .field("label_color", &self.label_color)
            .finish_non_exhaustive()
    }
}

/// The history suggestion at a live prompt
/// ([`TerminalView::set_history_suggestions`]).
#[derive(Clone, Debug)]
pub struct HistorySuggestions {
    /// The command lines every terminal of the app ran: this view adds its
    /// own as they finish.
    pub history: History,
    /// The suggestion's colour: the theme's `text.faint`.
    pub color: Hsla,
}

/// A terminal session drawn in a window. The session starts on the view's
/// first layout, once its size in cells is known, and is resized whenever
/// that changes.
pub struct TerminalView {
    /// Taken when the session starts.
    options: Option<TerminalOptions>,
    /// Hears the session's output from its first byte
    /// ([`TerminalView::tap_output`]); taken when it starts.
    tap: Option<OutputTap>,
    pub(crate) terminal: Option<Terminal>,
    pub(crate) style: TerminalStyle,
    pub(crate) focus: FocusHandle,
    /// The size last given to the session.
    size: Option<GridSize>,
    title: Option<String>,
    /// The foreground command's status last told
    /// ([`TerminalViewEvent::CommandStatus`]).
    command_status: CommandStatus,
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
    /// Whether the view drops everything meant for the pty: typed keys,
    /// pastes, composed text and the reports of focus and the mouse.
    pub(crate) read_only: bool,
    /// How command blocks are marked: not at all while `None`.
    pub(crate) blocks: Option<CommandBlockLook>,
    /// The history suggestion at the live prompt: none while `None`.
    pub(crate) suggest: Option<HistorySuggestions>,
    /// The newest command block whose line went into the history.
    recorded: Option<u64>,
    _focus_reports: [Subscription; 2],
    /// Redraws the cursor when the window gains or loses focus.
    _activation: Subscription,
}

impl EventEmitter<TerminalViewEvent> for TerminalView {}

impl TerminalView {
    /// A command block's edge's width.
    pub const BLOCK_EDGE: Pixels = px(2.);

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
            tap: None,
            terminal: None,
            style,
            focus,
            size: None,
            title: None,
            command_status: CommandStatus::Idle,
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
            read_only: false,
            blocks: None,
            suggest: None,
            recorded: None,
            _focus_reports: focus_reports,
            _activation: activation,
        })
    }

    /// Hands all the session's output, every byte as the pty reads it, to
    /// `tap`, independent of the grid and its scrollback: set before the
    /// session starts (on the view's first layout), else it hears nothing.
    pub fn tap_output(&mut self, tap: OutputTap) {
        if self.terminal.is_none() {
            self.tap = Some(tap);
        }
    }

    /// Turns the Linux primary selection on or off: while on, a selection
    /// made with the mouse is written to it and a middle click pastes from it.
    pub fn set_primary_selection(&mut self, on: bool) {
        self.primary_selection = on;
    }

    /// Makes the view read-only, or writable again: while read-only no key,
    /// paste or composed text reaches the pty. Selection, copy, scrolling,
    /// vi copy mode and find are the view's own, and still work.
    pub fn set_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        if self.read_only != read_only {
            self.read_only = read_only;
            // Gestures begun the other way end with it.
            self.reported_buttons.clear();
            self.reported = None;
            self.drag = None;
            self.wheel = 0.;
            cx.notify();
        }
    }

    /// Whether the view drops input meant for the pty.
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Marks the session's command blocks as `look` says, or, with `None`
    /// (the default), not at all. Neither moves the grid or changes its size:
    /// the edges are painted under the first column's text, and a label
    /// only where its row is blank.
    pub fn set_command_blocks(&mut self, look: Option<CommandBlockLook>, cx: &mut Context<Self>) {
        self.blocks = look;
        cx.notify();
    }

    /// Shows a faint suggestion after what is typed at the live prompt, the
    /// rest of the newest line of `suggestions.history` that starts with it,
    /// and writes the lines of the commands that finish to that history; or,
    /// with `None` (the default), neither. Right or End at the end of the
    /// line accepts it. Painted over the grid, not as cells.
    pub fn set_history_suggestions(
        &mut self,
        suggestions: Option<HistorySuggestions>,
        cx: &mut Context<Self>,
    ) {
        self.suggest = suggestions;
        cx.notify();
    }

    /// The history suggestion's setup, if it is on.
    pub fn history_suggestions(&self) -> Option<&HistorySuggestions> {
        self.suggest.as_ref()
    }

    /// What follows the typed command at the live prompt in the history's
    /// newest line that starts with it. `None` when suggestions are off, the
    /// view is read-only, text is being composed, vi mode is on, the program
    /// runs, the shell marks no prompt, the cursor isn't at the end of the
    /// line, or nothing matches.
    pub(crate) fn suggestion(&self) -> Option<String> {
        let suggest = self.suggest.as_ref()?;
        if self.read_only || self.marked.is_some() {
            return None;
        }
        let typed = self.terminal.as_ref()?.typed_command()?;
        suggest.history.remainder(&typed)
    }

    /// Adds the lines of the commands that have finished since the last
    /// call to the history, if suggestions are on.
    fn record_history(&mut self) {
        let (Some(suggest), Some(terminal)) = (&self.suggest, &self.terminal) else {
            return;
        };
        for (id, line) in terminal.finished_commands(self.recorded) {
            suggest.history.record(&line);
            self.recorded = Some(id);
        }
    }

    /// How command blocks are marked, if they are.
    pub fn command_blocks(&self) -> Option<&CommandBlockLook> {
        self.blocks.as_ref()
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
        match Terminal::spawn_tapped(options, size, self.tap.take()) {
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
                self.record_history();
                let status = (self.terminal.as_ref())
                    .map_or(CommandStatus::Idle, |terminal| terminal.command_status());
                if status != self.command_status {
                    self.command_status = status;
                    cx.emit(TerminalViewEvent::CommandStatus(status));
                }
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
        if self.read_only {
            return;
        }
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
