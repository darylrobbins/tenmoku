//! A pty running a shell, and the VT grid its output draws.
//!
//! alacritty_terminal's `EventLoop` owns the pty on its own thread: it reads the
//! child's output into the shared `Term`, writes queued input, and applies
//! resizes. [`Terminal`] is the handle the app holds.

use std::borrow::Cow;
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Direction, Point};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{self, Term};
use alacritty_terminal::tty;
use rustix_openpty::rustix::termios::Winsize;

use crate::snapshot::{self, Snapshot};

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
}

impl TerminalOptions {
    pub const DEFAULT_SCROLLBACK: usize = 10_000;

    /// The login shell in `cwd`, with the default scrollback.
    pub fn new(cwd: impl Into<PathBuf>) -> TerminalOptions {
        TerminalOptions {
            cwd: cwd.into(),
            shell: None,
            scrollback: Self::DEFAULT_SCROLLBACK,
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
}

/// Which half of a cell a selection point is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
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
struct Listener {
    events: async_channel::Sender<TerminalEvent>,
    /// The event loop's input, set once it exists (the `Term` it drives is
    /// built first).
    pty: Arc<OnceLock<EventLoopSender>>,
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

/// The sequence that ends a synchronized update (DECSET 2026).
const END_SYNCHRONIZED_UPDATE: &[u8] = b"\x1b[?2026l";

/// A pty that ends an unfinished synchronized update once the child has exited
/// and its output is read.
///
/// The parser holds the output after `ESC[?2026h` back until the update ends
/// or its timeout fires, and the event loop stops on the child's exit without
/// doing either, so that output would never reach the grid. alacritty offers
/// no way to flush the parser from outside the loop, so this feeds it the end
/// sequence through the pty's reader, after the last real byte. Where no
/// update is open the sequence does nothing.
struct FlushingPty {
    pty: tty::Pty,
    reader: FlushingReader,
    child_exited: Arc<AtomicBool>,
}

struct FlushingReader {
    file: std::fs::File,
    child_exited: Arc<AtomicBool>,
    flushed: bool,
}

impl FlushingPty {
    fn new(pty: tty::Pty) -> io::Result<FlushingPty> {
        let child_exited = Arc::new(AtomicBool::new(false));
        // A duplicate shares the file description, so it reads what the
        // registered descriptor reports readable.
        let reader = FlushingReader {
            file: pty.file().try_clone()?,
            child_exited: child_exited.clone(),
            flushed: false,
        };
        Ok(FlushingPty {
            pty,
            reader,
            child_exited,
        })
    }
}

impl io::Read for FlushingReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.file.read(buf);
        // Nothing left to read from an exited child: end of its output.
        let drained = match &read {
            Ok(0) => true,
            Ok(_) => false,
            Err(error) => {
                error.kind() == io::ErrorKind::WouldBlock
                    || error.raw_os_error() == Some(rustix::io::Errno::IO.raw_os_error())
            }
        };
        if drained
            && !self.flushed
            && self.child_exited.load(Ordering::SeqCst)
            && buf.len() >= END_SYNCHRONIZED_UPDATE.len()
        {
            self.flushed = true;
            buf[..END_SYNCHRONIZED_UPDATE.len()].copy_from_slice(END_SYNCHRONIZED_UPDATE);
            return Ok(END_SYNCHRONIZED_UPDATE.len());
        }
        read
    }
}

impl tty::EventedReadWrite for FlushingPty {
    type Reader = FlushingReader;
    type Writer = <tty::Pty as tty::EventedReadWrite>::Writer;

    unsafe fn register(
        &mut self,
        poll: &Arc<polling::Poller>,
        interest: polling::Event,
        mode: polling::PollMode,
    ) -> io::Result<()> {
        unsafe { self.pty.register(poll, interest, mode) }
    }

    fn reregister(
        &mut self,
        poll: &Arc<polling::Poller>,
        interest: polling::Event,
        mode: polling::PollMode,
    ) -> io::Result<()> {
        self.pty.reregister(poll, interest, mode)
    }

    fn deregister(&mut self, poll: &Arc<polling::Poller>) -> io::Result<()> {
        self.pty.deregister(poll)
    }

    fn reader(&mut self) -> &mut FlushingReader {
        &mut self.reader
    }

    fn writer(&mut self) -> &mut Self::Writer {
        self.pty.writer()
    }
}

impl tty::EventedPty for FlushingPty {
    fn next_child_event(&mut self) -> Option<tty::ChildEvent> {
        let event = self.pty.next_child_event();
        if event.is_some() {
            self.child_exited.store(true, Ordering::SeqCst);
        }
        event
    }
}

impl alacritty_terminal::event::OnResize for FlushingPty {
    fn on_resize(&mut self, size: WindowSize) {
        self.pty.on_resize(size);
    }
}

/// A running terminal session. Dropping it hangs up the child.
pub struct Terminal {
    term: Arc<FairMutex<Term<Listener>>>,
    pty: EventLoopSender,
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

        let config = term::Config {
            scrolling_history: options.scrollback,
            ..Default::default()
        };
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
        let env = HashMap::from([
            ("TERM".to_owned(), "xterm-256color".to_owned()),
            ("COLORTERM".to_owned(), "truecolor".to_owned()),
            ("TERM_PROGRAM".to_owned(), "fastedit".to_owned()),
        ]);
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
        let pty = FlushingPty::new(pty)?;

        let event_loop = EventLoop::new(term.clone(), listener.clone(), pty, true, false)?;
        let sender = event_loop.channel();
        let _ = listener.pty.set(sender.clone());
        let handle = event_loop.spawn();
        // `EventLoop::spawn` hands the pty back through the join handle, so the
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

    /// Resizes the grid and tells the child (SIGWINCH).
    pub fn resize(&self, size: GridSize) {
        let window_size = size.window_size();
        *self.size.lock().unwrap_or_else(|e| e.into_inner()) = window_size;
        self.term.lock().resize(TermSize::from(size));
        let _ = self.pty.send(Msg::Resize(window_size));
    }

    /// Scrolls the view by `lines`: negative moves back into history, positive
    /// toward the bottom.
    pub fn scroll(&self, lines: i32) {
        self.term
            .lock()
            .scroll_display(Scroll::Delta(lines.saturating_neg()));
    }

    pub fn scroll_to_bottom(&self) {
        let mut term = self.term.lock();
        if term.grid().display_offset() != 0 {
            term.scroll_display(Scroll::Bottom);
        }
    }

    /// A copy of the visible grid.
    pub fn snapshot(&self) -> Snapshot {
        snapshot::take(&self.term.lock())
    }

    /// Starts a selection at viewport cell (`row`, `col`), replacing any other.
    pub fn select(&self, kind: SelectionKind, row: usize, col: usize, side: Side) {
        let mut term = self.term.lock();
        let point = viewport_point(&term, row, col);
        let ty = match kind {
            SelectionKind::Simple => SelectionType::Simple,
            SelectionKind::Word => SelectionType::Semantic,
            SelectionKind::Line => SelectionType::Lines,
        };
        term.selection = Some(Selection::new(ty, point, side.into()));
    }

    /// Moves the selection's moving end to viewport cell (`row`, `col`).
    pub fn extend_selection(&self, row: usize, col: usize, side: Side) {
        let mut term = self.term.lock();
        let point = viewport_point(&term, row, col);
        if let Some(selection) = term.selection.as_mut() {
            selection.update(point, side.into());
        }
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
