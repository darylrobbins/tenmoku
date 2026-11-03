//! A pty running a shell, and the VT grid its output draws.
//!
//! A `PtyLoop` owns the pty on its own thread: it reads the child's output
//! into the shared `Term`, writes queued input, and applies resizes. [`Terminal`] is the handle the app holds.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Direction, Point};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{self, Term, TermMode};
use alacritty_terminal::tty;
use rustix_openpty::rustix::termios::Winsize;

use crate::pty_loop::{LoopSender, Msg, PtyLoop, lock};
use crate::search::{self, QueryError, TerminalMatch, TerminalQuery};
use crate::shell::{ShellState, UNBOUNDED_HISTORY};
use crate::snapshot::{self, Snapshot};
use crate::vi;

/// What to run and how much history to keep.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalOptions {
    /// The shell's working directory.
    pub cwd: PathBuf,
    /// The program and its arguments. `None` runs the user's login shell:
    /// `/usr/bin/login -flp $USER` on macOS, the passwd shell elsewhere.
    pub shell: Option<(String, Vec<String>)>,
    /// Lines of scrollback kept above the screen.
    pub scrollback: usize,
    /// What the child's environment adds to the one it inherits, over the
    /// terminal's own variables (`TERM` and the rest).
    pub env: BTreeMap<String, String>,
}

impl TerminalOptions {
    pub const DEFAULT_SCROLLBACK: usize = 10_000;

    /// The login shell in `cwd`, with the default scrollback.
    pub fn new(cwd: impl Into<PathBuf>) -> TerminalOptions {
        TerminalOptions {
            cwd: cwd.into(),
            shell: None,
            scrollback: Self::DEFAULT_SCROLLBACK,
            env: BTreeMap::new(),
        }
    }
}

/// The grid size in cells, and a cell's size in pixels (programs can ask for
/// the window's pixel size).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridSize {
    pub cols: u16,
    pub rows: u16,
    pub cell_width: u16,
    pub cell_height: u16,
}

impl GridSize {
    fn window_size(self) -> WindowSize {
        WindowSize {
            num_lines: self.rows.max(1),
            num_cols: self.cols.max(2),
            cell_width: self.cell_width,
            cell_height: self.cell_height,
        }
    }
}

/// The grid dimensions `Term` sizes itself by.
struct TermSize {
    cols: usize,
    rows: usize,
}

impl From<GridSize> for TermSize {
    fn from(size: GridSize) -> TermSize {
        let size = size.window_size();
        TermSize {
            cols: usize::from(size.num_cols),
            rows: usize::from(size.num_lines),
        }
    }
}

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

/// What the session reports to the app, on the channel [`Terminal::spawn`]
/// returns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalEvent {
    /// The grid changed; take a new [`Terminal::snapshot`]. Several changes
    /// may arrive as one.
    Wakeup,
    /// The program set the window title.
    Title(String),
    Bell,
    /// The session ended, with the child's exit code (`None` when a signal
    /// ended it, the code is unknown, or the session stopped on an error or
    /// was dropped). Sent once, after the child's last output is in the grid;
    /// the session is dead after it.
    Exited(Option<i32>),
    /// The program asked to copy this text to the clipboard (OSC 52).
    ClipboardStore(String),
}

/// How a selection grows from where it starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SelectionKind {
    /// Exactly the cells between the two ends.
    Simple,
    /// Whole words, split on whitespace and shell punctuation.
    Word,
    /// Whole lines.
    Line,
    /// The rectangle of cells between the two ends.
    Block,
}

/// Which half of a cell a selection point is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
}

impl From<SelectionKind> for SelectionType {
    fn from(kind: SelectionKind) -> SelectionType {
        match kind {
            SelectionKind::Simple => SelectionType::Simple,
            SelectionKind::Word => SelectionType::Semantic,
            SelectionKind::Line => SelectionType::Lines,
            SelectionKind::Block => SelectionType::Block,
        }
    }
}

impl From<Side> for Direction {
    fn from(side: Side) -> Direction {
        match side {
            Side::Left => Direction::Left,
            Side::Right => Direction::Right,
        }
    }
}

/// Forwards `Term` and event-loop events to the app's channel, and answers
/// the ones the pty needs answered.
#[derive(Clone)]
pub(crate) struct Listener {
    events: async_channel::Sender<TerminalEvent>,
    /// The event loop's input, set once it exists (the `Term` it drives is
    /// built first).
    pty: Arc<OnceLock<LoopSender>>,
    size: Arc<Mutex<WindowSize>>,
    /// The child's exit code, from `ChildExit`, held until the loop has read
    /// the child's last output.
    exit_code: Arc<Mutex<Option<i32>>>,
    exited: Arc<AtomicBool>,
}

impl Listener {
    fn write(&self, text: String) {
        // An empty write would stall alacritty's writer for good.
        if text.is_empty() {
            return;
        }
        if let Some(pty) = self.pty.get() {
            let _ = pty.send(Msg::Input(Cow::Owned(text.into_bytes())));
        }
    }

    fn send(&self, event: TerminalEvent) {
        // Unbounded, so this fails only once the app dropped the receiver.
        let _ = self.events.try_send(event);
    }

    /// Sends `Exited` once, with the code `ChildExit` reported, if any.
    fn exit(&self) {
        if !self.exited.swap(true, Ordering::SeqCst) {
            let code = *self.exit_code.lock().unwrap_or_else(|e| e.into_inner());
            self.send(TerminalEvent::Exited(code));
        }
    }
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        match event {
            Event::Wakeup => self.send(TerminalEvent::Wakeup),
            Event::Title(title) => self.send(TerminalEvent::Title(title)),
            Event::ResetTitle => self.send(TerminalEvent::Title(String::new())),
            Event::Bell => self.send(TerminalEvent::Bell),
            Event::ClipboardStore(_, text) => self.send(TerminalEvent::ClipboardStore(text)),
            Event::PtyWrite(text) => self.write(text),
            Event::TextAreaSizeRequest(format) => {
                let size = *self.size.lock().unwrap_or_else(|e| e.into_inner());
                self.write(format(size));
            }
            // The loop sends `ChildExit`, then reads the child's remaining
            // output, then sends `Exit`; `Exited` waits for the output.
            Event::ChildExit(status) => {
                *self.exit_code.lock().unwrap_or_else(|e| e.into_inner()) = status.code();
            }
            Event::Exit => self.exit(),
            // Clipboard reads and palette queries go unanswered for now: the
            // core has no clipboard or theme.
            Event::ClipboardLoad(..)
            | Event::ColorRequest(..)
            | Event::MouseCursorDirty
            | Event::CursorBlinkingChange => {}
        }
    }
}

/// A running terminal session. Dropping it hangs up the child.
pub struct Terminal {
    pub(crate) term: Arc<FairMutex<Term<Listener>>>,
    /// What the shell reported. Locked after `term`.
    pub(crate) shell: Arc<Mutex<ShellState>>,
    /// The `Term`'s options with a history too long to fill, set while it
    /// resizes on the alternate screen: the only way to the hidden primary
    /// grid's limit.
    unbounded: term::Config,
    pty: LoopSender,
    size: Arc<Mutex<WindowSize>>,
    pid: u32,
    event_loop: Option<JoinHandle<()>>,
}

impl Terminal {
    /// Starts `options`' shell on a new pty of `size`.
    pub fn spawn(
        options: TerminalOptions,
        size: GridSize,
    ) -> io::Result<(Terminal, async_channel::Receiver<TerminalEvent>)> {
        let (events, receiver) = async_channel::unbounded();
        let window_size = size.window_size();
        let listener = Listener {
            events,
            pty: Arc::new(OnceLock::new()),
            size: Arc::new(Mutex::new(window_size)),
            exit_code: Arc::new(Mutex::new(None)),
            exited: Arc::new(AtomicBool::new(false)),
        };

        // The loop trims history back to the scrollback (`ShellState`).
        let config = term::Config {
            scrolling_history: ShellState::term_history(options.scrollback),
            ..Default::default()
        };
        let unbounded = term::Config {
            scrolling_history: UNBOUNDED_HISTORY,
            ..config.clone()
        };
        let shell = Arc::new(Mutex::new(ShellState::new(options.scrollback)));
        let term = Term::new(config, &TermSize::from(size), listener.clone());
        let term = Arc::new(FairMutex::new(term));

        // The child's environment is set here, never with `tty::setup_env`,
        // which calls `env::set_var` in a multi-threaded process.
        // alacritty's spawn ignores a failed `chdir`, which would run the shell
        // in the app's own directory.
        if !std::fs::metadata(&options.cwd)?.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                format!("{} is not a directory", options.cwd.display()),
            ));
        }
        // A directory the user cannot enter fails the same way. `access` asks
        // the OS without changing this process's cwd.
        rustix::fs::access(&options.cwd, rustix::fs::Access::EXEC_OK)
            .map_err(|errno| io::Error::from_raw_os_error(errno.raw_os_error()))?;
        let mut env = HashMap::from([
            ("TERM".to_owned(), "xterm-256color".to_owned()),
            ("COLORTERM".to_owned(), "truecolor".to_owned()),
            ("TERM_PROGRAM".to_owned(), "tenmoku".to_owned()),
        ]);
        env.extend(options.env);
        let tty_options = tty::Options {
            shell: options
                .shell
                .map(|(program, args)| tty::Shell::new(program, args)),
            working_directory: Some(options.cwd),
            drain_on_exit: true,
            env,
        };
        // The pty is opened here rather than by `tty::new` so this side keeps a
        // handle on its user end until the event loop stops. Otherwise, on
        // Linux, a child that exits at once hangs up the pty before the loop
        // reads its output: the loop skips a readable event flagged HUP, and
        // its drain on exit drops bytes it read when the next read is EIO.
        let winsize = Winsize {
            ws_row: window_size.num_lines,
            ws_col: window_size.num_cols,
            ws_xpixel: window_size.num_cols.saturating_mul(window_size.cell_width),
            ws_ypixel: window_size
                .num_lines
                .saturating_mul(window_size.cell_height),
        };
        let pair = rustix_openpty::openpty(None, Some(&winsize))?;
        let user = pair.user.try_clone()?;
        let pty = tty::from_fd(&tty_options, 0, pair.controller, pair.user)?;
        let pid = pty.child().id();

        let (event_loop, sender) =
            PtyLoop::new(term.clone(), shell.clone(), listener.clone(), pty)?;
        let _ = listener.pty.set(sender.clone());
        let handle = event_loop.spawn()?;
        // `PtyLoop::spawn` hands the pty back through the join handle, so the
        // pty is dropped (hanging up and waiting for the child) by whichever
        // thread joins it. This thread does, so the app's never waits. If the
        // loop stopped on an error rather than the child's exit, `Exited` is
        // sent here, so the app never waits for it either.
        let reaper = listener.clone();
        let event_loop = std::thread::Builder::new()
            .name("fe-terminal pty".to_owned())
            .spawn(move || {
                drop(handle.join());
                drop(user);
                reaper.exit();
            })
            .inspect_err(|_| {
                let _ = sender.send(Msg::Shutdown);
            })?;

        let terminal = Terminal {
            term,
            shell,
            unbounded,
            pty: sender,
            size: listener.size,
            pid,
            event_loop: Some(event_loop),
        };
        Ok((terminal, receiver))
    }

    /// The child's process id.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Writes `bytes` to the pty, as typed or pasted, and scrolls to the
    /// bottom. Empty input does nothing.
    pub fn input(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        let bytes = bytes.into();
        // An empty write would stall alacritty's writer for good.
        if bytes.is_empty() {
            return;
        }
        self.scroll_to_bottom();
        let _ = self.pty.send(Msg::Input(bytes));
    }

    /// Writes `bytes` to the pty without moving the view: for what the
    /// program asked for rather than what the user typed, such as mouse
    /// reports. Empty input does nothing.
    pub fn input_in_place(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        let bytes = bytes.into();
        // An empty write would stall alacritty's writer for good.
        if bytes.is_empty() {
            return;
        }
        let _ = self.pty.send(Msg::Input(bytes));
    }

    /// Resizes the grid and tells the child (SIGWINCH). Prompt marks follow
    /// their lines as a column change reflows them.
    pub fn resize(&self, size: GridSize) {
        let window_size = size.window_size();
        *self.size.lock().unwrap_or_else(|e| e.into_inner()) = window_size;
        let mut term = self.term.lock();
        let mut shell = lock(&self.shell);
        let offset = term.grid().display_offset();
        let size = TermSize::from(size);
        // Rows, then columns, as alacritty does them, but one at a time: a
        // row change may drop blank rows at the bottom, and the marks follow
        // each. The history's limit is lifted for each, so a reflow keeps
        // every row: trimming it back counts the rows that go.
        let rows_first = TermSize {
            cols: term.columns(),
            rows: size.rows,
        };
        for step in [rows_first, size] {
            if (step.cols, step.rows) == (term.columns(), term.screen_lines()) {
                continue;
            }
            let anchors = shell.before_resize(&term);
            if term.mode().contains(TermMode::ALT_SCREEN) {
                // Until the primary grid shows again and is trimmed.
                term.set_options(self.unbounded.clone());
            } else {
                term.grid_mut().update_history(UNBOUNDED_HISTORY);
            }
            term.resize(step);
            shell.after_resize(&mut term, anchors);
        }
        shell.view_moved(offset, &term);
        // Trimming history back may have taken the vi cursor's text.
        vi::keep_in_view(&mut term);
        drop((shell, term));
        let _ = self.pty.send(Msg::Resize(window_size));
    }

    /// The working directory the shell last reported with OSC 7, if any
    /// (docs/terminal-shell-integration.md). It may not exist here: a shell
    /// over ssh reports the remote one.
    pub fn cwd(&self) -> Option<PathBuf> {
        lock(&self.shell).cwd()
    }

    /// Scrolls the view to the previous or next prompt (`forward`) the shell
    /// marked with OSC 133;A, putting its row at the top of the view as far
    /// as history allows. Back goes from the prompt last scrolled to, or
    /// with none from the shell's current one; forward from the one last
    /// scrolled to, and past the last to the bottom (with none, it does
    /// nothing). The view returning to
    /// the bottom forgets the prompt. Whether there was a prompt to go to.
    pub fn scroll_to_prompt(&self, forward: bool) -> bool {
        let mut term = self.term.lock();
        lock(&self.shell).scroll_to_prompt(&mut term, forward)
    }

    /// Scrolls the view by `lines`: negative moves back into history, positive
    /// toward the bottom.
    pub fn scroll(&self, lines: i32) {
        let mut term = self.term.lock();
        let offset = term.grid().display_offset();
        term.scroll_display(Scroll::Delta(lines.saturating_neg()));
        lock(&self.shell).view_moved(offset, &term);
    }

    /// Scrolls the view to the bottom, forgetting the prompt
    /// [`Terminal::scroll_to_prompt`] went to.
    pub fn scroll_to_bottom(&self) {
        let mut term = self.term.lock();
        if term.grid().display_offset() != 0 {
            term.scroll_display(Scroll::Bottom);
        }
        lock(&self.shell).forget();
    }

    /// A copy of the visible grid.
    pub fn snapshot(&self) -> Snapshot {
        snapshot::take(&self.term.lock())
    }

    /// Starts a selection at viewport cell (`row`, `col`), replacing any other.
    pub fn select(&self, kind: SelectionKind, row: usize, col: usize, side: Side) {
        let mut term = self.term.lock();
        let point = viewport_point(&term, row, col);
        term.selection = Some(Selection::new(kind.into(), point, side.into()));
    }

    /// Moves the selection's moving end to viewport cell (`row`, `col`).
    pub fn extend_selection(&self, row: usize, col: usize, side: Side) {
        let mut term = self.term.lock();
        let point = viewport_point(&term, row, col);
        if let Some(selection) = term.selection.as_mut() {
            selection.update(point, side.into());
        }
    }

    /// Whether anything is selected, even blank cells.
    pub fn has_selection(&self) -> bool {
        (self.term.lock().selection.as_ref()).is_some_and(|selection| !selection.is_empty())
    }

    pub fn clear_selection(&self) {
        self.term.lock().selection = None;
    }

    /// The selected text, or `None` with nothing selected.
    pub fn selected_text(&self) -> Option<String> {
        self.term
            .lock()
            .selection_to_string()
            .filter(|text| !text.is_empty())
    }

    /// Finds `query`'s next match after the selection (or before it, when not
    /// `forward`), else from the cursor, wrapping through the whole history
    /// once. The match becomes the selection, and the view scrolls to its
    /// first line (to the bottom when that's on screen). With no match the
    /// selection and view stay as they were.
    pub fn find(
        &self,
        query: &TerminalQuery,
        forward: bool,
    ) -> Result<Option<TerminalMatch>, QueryError> {
        let mut term = self.term.lock();
        let offset = term.grid().display_offset();
        let found = search::find(&mut term, query, forward);
        lock(&self.shell).view_moved(offset, &term);
        found
    }

    /// `query`'s matches on the visible rows at the current display offset,
    /// top to bottom, at most 1,000: for highlighting them all.
    pub fn find_all_visible(
        &self,
        query: &TerminalQuery,
    ) -> Result<Vec<TerminalMatch>, QueryError> {
        search::find_all_visible(&self.term.lock(), query)
    }
}

/// The grid point at viewport cell (`row`, `col`), clamped to the viewport.
fn viewport_point<T>(term: &Term<T>, row: usize, col: usize) -> Point {
    let row = row.min(term.screen_lines() - 1);
    let col = col.min(term.columns() - 1);
    term::viewport_to_point(term.grid().display_offset(), Point::new(row, Column(col)))
}

impl Drop for Terminal {
    fn drop(&mut self) {
        // The event loop stops, and the pty thread drops the pty, which sends
        // SIGHUP and reaps the child. Nothing here waits for either.
        let _ = self.pty.send(Msg::Shutdown);
        drop(self.event_loop.take());
    }
}
