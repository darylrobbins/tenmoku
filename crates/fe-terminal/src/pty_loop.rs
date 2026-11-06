//! The loop that moves bytes between the pty and the shared `Term`.
//!
//! This is alacritty_terminal's `EventLoop`, cut down to what a session needs
//! (it always drains on exit and never records), because that loop keeps its
//! VT parser private, and the parser is fed through a [`Feeder`] that watches
//! the bytes for the shell's escapes (`crate::shell`).
//!
//! The feeder, not the parser, holds a synchronized update's output (after
//! `ESC[?2026h`) back until the update ends or a timeout fires, so the held
//! bytes reach the grid through the same scanning and history bookkeeping as
//! any others. Every way out of the loop ends the update before the exit is
//! published.

use std::borrow::Cow;
use std::collections::VecDeque;
use std::io::{self, ErrorKind, Read, Write};
use std::num::NonZeroUsize;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event, EventListener, OnResize, WindowSize};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Term;
use alacritty_terminal::tty::{self, EventedPty, EventedReadWrite};
use alacritty_terminal::vte::ansi::{Processor, Timeout};

use crate::shell::{Found, MAX_ADVANCE, Scanner, ShellState};
use crate::vi::{self, Hold};

/// Max bytes to read from the pty before forced terminal synchronization.
const READ_BUFFER_SIZE: usize = 0x10_0000;

/// How long a synchronized update may hold output back, as alacritty's
/// parser allows.
const SYNC_TIMEOUT: Duration = Duration::from_millis(150);

/// The most output a synchronized update holds back before it's shown, as
/// alacritty's parser allows.
const MAX_SYNC_BYTES: usize = 0x20_0000;

/// Max bytes to parse while holding the terminal lock before letting go.
const MAX_LOCKED_READ: usize = u16::MAX as usize;

/// How many `MAX_LOCKED_READ` reads the final drain makes at most, so a
/// writer that outlives the child (one that inherited the pty) cannot keep
/// the loop from ending.
const MAX_FINAL_DRAIN_READS: usize = 64;

// alacritty's tokens for the pty's data and its child-exit notification. Its
// `Pty` registers with these keys, whatever key it's given; on Unix it does
// not export them.
#[cfg(unix)]
const PTY_READ_WRITE_TOKEN: usize = 0;
#[cfg(unix)]
const PTY_CHILD_EVENT_TOKEN: usize = 1;
#[cfg(windows)]
use alacritty_terminal::tty::{PTY_CHILD_EVENT_TOKEN, PTY_READ_WRITE_TOKEN};

/// Messages for the loop.
pub(crate) enum Msg {
    /// Bytes for the child.
    Input(Cow<'static, [u8]>),
    Resize(WindowSize),
    Shutdown,
}

/// Sends to a running loop and wakes it.
#[derive(Clone)]
pub(crate) struct LoopSender {
    sender: Sender<Msg>,
    poller: Arc<polling::Poller>,
}

impl LoopSender {
    pub(crate) fn send(&self, msg: Msg) -> io::Result<()> {
        self.sender
            .send(msg)
            .map_err(|_| io::Error::new(ErrorKind::BrokenPipe, "the pty loop has stopped"))?;
        self.poller.notify()
    }
}

pub(crate) struct PtyLoop<P, U: EventListener> {
    poll: Arc<polling::Poller>,
    pty: P,
    rx: Receiver<Msg>,
    terminal: Arc<FairMutex<Term<U>>>,
    /// Locked after `terminal`.
    shell: Arc<Mutex<ShellState>>,
    events: U,
}

impl<P, U> PtyLoop<P, U>
where
    P: EventedPty + OnResize + Send + 'static,
    U: EventListener + Send + 'static,
{
    pub(crate) fn new(
        terminal: Arc<FairMutex<Term<U>>>,
        shell: Arc<Mutex<ShellState>>,
        events: U,
        pty: P,
    ) -> io::Result<(PtyLoop<P, U>, LoopSender)> {
        let (tx, rx) = mpsc::channel();
        let poll = Arc::new(polling::Poller::new()?);
        let sender = LoopSender {
            sender: tx,
            poller: poll.clone(),
        };
        let pty_loop = PtyLoop {
            poll,
            pty,
            rx,
            terminal,
            shell,
            events,
        };
        Ok((pty_loop, sender))
    }

    /// Runs the loop on its own thread. The pty comes back through the join
    /// handle so the caller drops it (hangs up and reaps the child) off the
    /// app's thread.
    pub(crate) fn spawn(self) -> io::Result<JoinHandle<P>> {
        std::thread::Builder::new()
            .name("fe-terminal pty reader".to_owned())
            .spawn(move || self.run())
    }

    fn run(mut self) -> P {
        let mut feeder = Feeder::default();
        let mut writes = Writes::default();
        let mut buf = vec![0u8; READ_BUFFER_SIZE];
        let mut interest = polling::Event::readable(PTY_READ_WRITE_TOKEN);

        let registered = unsafe {
            self.pty
                .register(&self.poll, interest, polling::PollMode::Level)
        };
        if registered.is_err() {
            return self.pty;
        }
        // alacritty's Windows pty tells the poller of the child's exit only
        // once it is registered: a child that exited before then is found
        // by a look the loop asks itself for.
        #[cfg(windows)]
        {
            use polling::os::iocp::{CompletionPacket, PollerIocpExt as _};
            let look = polling::Event::readable(PTY_CHILD_EVENT_TOKEN);
            if self.poll.post(CompletionPacket::new(look)).is_err() {
                return self.pty;
            }
        }

        let mut child_exited = false;
        let mut events = polling::Events::with_capacity(NonZeroUsize::new(1024).unwrap());
        'event_loop: loop {
            // Wake for the synchronized update's timeout.
            let timeout = (feeder.deadline())
                .map(|deadline| deadline.saturating_duration_since(Instant::now()));
            events.clear();
            if let Err(error) = self.poll.wait(&mut events, timeout) {
                if error.kind() == ErrorKind::Interrupted {
                    continue;
                }
                break;
            }

            // End an update whose time is up, even while output keeps the
            // pty readable.
            if (feeder.deadline()).is_some_and(|deadline| deadline <= Instant::now()) {
                self.end_sync(&mut feeder);
                self.events.send_event(Event::Wakeup);
            }

            // Messages.
            while let Ok(msg) = self.rx.try_recv() {
                match msg {
                    Msg::Input(bytes) => writes.queue.push_back(bytes),
                    Msg::Resize(size) => self.pty.on_resize(size),
                    Msg::Shutdown => break 'event_loop,
                }
            }

            for event in events.iter() {
                match event.key {
                    PTY_CHILD_EVENT_TOKEN => {
                        if let Some(tty::ChildEvent::Exited(status)) = self.pty.next_child_event() {
                            if let Some(status) = status {
                                self.events.send_event(Event::ChildExit(status));
                            }
                            // The child's last output. One read call stops at
                            // `MAX_LOCKED_READ`, so read until caught up or
                            // the cap says a writer is still going.
                            for _ in 0..MAX_FINAL_DRAIN_READS {
                                match self.read(&mut feeder, &mut buf) {
                                    Ok(processed) if processed >= MAX_LOCKED_READ => {}
                                    _ => break,
                                }
                            }
                            child_exited = true;
                            break 'event_loop;
                        }
                    }
                    PTY_READ_WRITE_TOKEN => {
                        if event.is_interrupt() {
                            continue;
                        }
                        if event.readable {
                            match self.read(&mut feeder, &mut buf) {
                                Ok(_) => {}
                                // Linux reports a hung-up pty as EIO; the
                                // child-exit event follows.
                                #[cfg(target_os = "linux")]
                                Err(error) if error.raw_os_error() == Some(libc_eio()) => {
                                    continue;
                                }
                                Err(_) => break 'event_loop,
                            }
                        }
                        if event.writable && writes.flush(&mut self.pty).is_err() {
                            break 'event_loop;
                        }
                    }
                    _ => {}
                }
            }

            let needs_write = writes.pending();
            if needs_write != interest.writable {
                interest.writable = needs_write;
                if self
                    .pty
                    .reregister(&self.poll, interest, polling::PollMode::Level)
                    .is_err()
                {
                    break;
                }
            }
        }

        // Every way out of the loop ends here: output held back for an
        // unfinished synchronized update reaches the grid before the exit is
        // published or the parser goes. Ending the update only releases the
        // held bytes, so an ordinary escape sequence still unfinished stays
        // so.
        self.end_sync(&mut feeder);
        if child_exited {
            self.terminal.lock().exit();
        }
        self.events.send_event(Event::Wakeup);

        let _ = self.pty.deregister(&self.poll);
        self.pty
    }

    /// Ends a synchronized update, putting its held bytes on the grid.
    fn end_sync(&self, feeder: &mut Feeder) {
        let mut terminal = self.terminal.lock();
        feeder.end_sync(&mut lock(&self.shell), &mut terminal);
    }

    /// Reads what the pty has and parses it. Returns the bytes read.
    fn read(&mut self, feeder: &mut Feeder, buf: &mut [u8]) -> io::Result<usize> {
        // Reserve the next terminal lock for reading.
        let _lease = self.terminal.lease();
        let mut terminal = None;
        let mut unprocessed = 0;
        let mut processed = 0;
        let mut parsed = false;

        loop {
            match self.pty.reader().read(&mut buf[unprocessed..]) {
                Ok(0) if unprocessed == 0 => break,
                Ok(got) => unprocessed += got,
                Err(error) => match error.kind() {
                    ErrorKind::Interrupted | ErrorKind::WouldBlock => {
                        if unprocessed == 0 {
                            break;
                        }
                    }
                    _ => return Err(error),
                },
            }

            let terminal = match &mut terminal {
                Some(terminal) => terminal,
                None => terminal.insert(match self.terminal.try_lock_unfair() {
                    // Block only at the buffer size limit.
                    None if unprocessed >= READ_BUFFER_SIZE => self.terminal.lock_unfair(),
                    None => continue,
                    Some(terminal) => terminal,
                }),
            };

            let bytes = &buf[..unprocessed];
            parsed |= feeder.feed(&mut lock(&self.shell), terminal, bytes, Instant::now());
            processed += unprocessed;
            unprocessed = 0;

            if processed >= MAX_LOCKED_READ {
                break;
            }
        }

        // Redraw unless everything read is held in a synchronized update.
        if parsed {
            self.events.send_event(Event::Wakeup);
        }
        Ok(processed)
    }
}

/// A synchronized-update timeout that never runs, so the parser never holds
/// output back: the [`Feeder`] does.
#[derive(Default)]
struct NoSync;

impl Timeout for NoSync {
    fn set_timeout(&mut self, _: Duration) {}

    fn clear_timeout(&mut self) {}

    fn pending_timeout(&self) -> bool {
        false
    }
}

/// A synchronized update's output, held back.
struct Held {
    bytes: Vec<u8>,
    deadline: Instant,
}

/// Feeds the pty's output to the parser at most [`MAX_ADVANCE`] bytes at a
/// time, telling the shell state of each call and, as the parser reaches
/// them, of the shell's escapes. A sequence that clears history or hides
/// the primary grid ends in a call of its own, its final byte in the next.
#[derive(Default)]
struct Feeder {
    parser: Processor<NoSync>,
    scanner: Scanner,
    /// The synchronized update under way.
    held: Option<Held>,
    /// How much of [`SYNC_PREFIX`] the held bytes end with.
    matched: usize,
}

/// What a synchronized update's start and end, exactly as alacritty's
/// parser matches them while one is under way, begin with: `h` after it
/// renews the update, `l` ends it.
const SYNC_PREFIX: &[u8] = b"\x1b[?2026";

impl Feeder {
    /// When the synchronized update under way must end.
    fn deadline(&self) -> Option<Instant> {
        self.held.as_ref().map(|held| held.deadline)
    }

    /// Feeds `bytes`, read at `now`, or holds them in a synchronized
    /// update. Whether any reached the parser.
    fn feed<U: EventListener>(
        &mut self,
        shell: &mut ShellState,
        terminal: &mut Term<U>,
        mut bytes: &[u8],
        now: Instant,
    ) -> bool {
        let mut parsed = false;
        while !bytes.is_empty() {
            let Some(held) = &mut self.held else {
                parsed = true;
                let Some(start) = self.parse(shell, terminal, bytes, false) else {
                    break;
                };
                self.held = Some(Held {
                    bytes: Vec::new(),
                    deadline: now + SYNC_TIMEOUT,
                });
                self.matched = 0;
                bytes = &bytes[start..];
                continue;
            };
            // Each byte is looked at once, whatever the reads' sizes.
            let mut end = None;
            for (ix, &byte) in bytes.iter().enumerate() {
                if self.matched == SYNC_PREFIX.len() {
                    self.matched = 0;
                    match byte {
                        b'h' => held.deadline = now + SYNC_TIMEOUT,
                        b'l' => {
                            end = Some(ix + 1);
                            break;
                        }
                        _ => {}
                    }
                }
                if byte == SYNC_PREFIX[self.matched] {
                    self.matched += 1;
                } else {
                    self.matched = usize::from(byte == SYNC_PREFIX[0]);
                }
            }
            let held_len = end.unwrap_or(bytes.len());
            held.bytes.extend_from_slice(&bytes[..held_len]);
            bytes = &bytes[held_len..];
            if end.is_some() || held.bytes.len() >= MAX_SYNC_BYTES {
                self.end_sync(shell, terminal);
                parsed = true;
            }
        }
        parsed
    }

    /// Ends the synchronized update under way, if any, feeding its bytes.
    fn end_sync<U: EventListener>(&mut self, shell: &mut ShellState, terminal: &mut Term<U>) {
        if let Some(held) = self.held.take() {
            self.parse(shell, terminal, &held.bytes, true);
        }
    }

    /// Feeds `bytes` to the parser. Unless `replaying` a synchronized
    /// update's, stops after a CSI that starts one: the offset of the bytes
    /// after it.
    fn parse<U: EventListener>(
        &mut self,
        shell: &mut ShellState,
        terminal: &mut Term<U>,
        mut bytes: &[u8],
        replaying: bool,
    ) -> Option<usize> {
        let len = bytes.len();
        let mut advance = |bytes: &[u8], terminal: &mut Term<U>, shell: &mut ShellState| {
            // Vi mode's view stays on its text as rows scroll into history.
            let hold = Hold::before(terminal, shell);
            self.parser.advance(terminal, bytes);
            shell.advanced(terminal);
            if let Some(hold) = hold {
                hold.after(terminal, shell);
            }
            vi::keep_in_view(terminal);
        };
        while !bytes.is_empty() {
            let chunk = &bytes[..bytes.len().min(MAX_ADVANCE)];
            let Some((end, found)) = self.scanner.next(chunk) else {
                advance(chunk, terminal, shell);
                bytes = &bytes[chunk.len()..];
                continue;
            };
            advance(&bytes[..end], terminal, shell);
            bytes = &bytes[end..];
            let starts_sync = match found {
                Found::Shell(event) => {
                    shell.event(event, terminal);
                    false
                }
                Found::ClearsHistory | Found::LeavesAlternateScreen => {
                    advance(&bytes[..1], terminal, shell);
                    bytes = &bytes[1..];
                    false
                }
                Found::EntersAlternateScreen { sync } => {
                    shell.freeze(terminal);
                    advance(&bytes[..1], terminal, shell);
                    bytes = &bytes[1..];
                    sync
                }
                Found::StartsSync => true,
            };
            if starts_sync && !replaying {
                return Some(len - bytes.len());
            }
        }
        None
    }
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(target_os = "linux")]
fn libc_eio() -> i32 {
    rustix::io::Errno::IO.raw_os_error()
}

/// Input waiting to be written to the pty.
#[derive(Default)]
struct Writes {
    queue: VecDeque<Cow<'static, [u8]>>,
    current: Option<(Cow<'static, [u8]>, usize)>,
}

impl Writes {
    fn pending(&self) -> bool {
        self.current.is_some() || !self.queue.is_empty()
    }

    fn flush<P: EventedReadWrite>(&mut self, pty: &mut P) -> io::Result<()> {
        loop {
            if self.current.is_none() {
                self.current = self.queue.pop_front().map(|bytes| (bytes, 0));
            }
            let Some((bytes, written)) = self.current.as_mut() else {
                return Ok(());
            };
            match pty.writer().write(&bytes[*written..]) {
                Ok(0) => return Ok(()),
                Ok(n) => {
                    *written += n;
                    if *written >= bytes.len() {
                        self.current = None;
                    }
                }
                Err(error) => match error.kind() {
                    ErrorKind::Interrupted | ErrorKind::WouldBlock => return Ok(()),
                    _ => return Err(error),
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::Event;
    use alacritty_terminal::grid::Dimensions;
    use alacritty_terminal::term::{Config, TermMode};
    use alacritty_terminal::tty::ChildEvent;
    use std::sync::Mutex;

    /// A connected pair of sockets, for wake-ups the poller sees.
    #[cfg(unix)]
    type WakeStream = std::os::unix::net::UnixStream;
    #[cfg(windows)]
    type WakeStream = std::net::TcpStream;

    #[cfg(unix)]
    fn wake_pair() -> (WakeStream, WakeStream) {
        WakeStream::pair().unwrap()
    }

    /// Windows' poller takes sockets alone, and std has no socket pair there:
    /// a loopback connection is one.
    #[cfg(windows)]
    fn wake_pair() -> (WakeStream, WakeStream) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let near = WakeStream::connect(listener.local_addr().unwrap()).unwrap();
        let (far, _) = listener.accept().unwrap();
        (near, far)
    }

    struct Size;

    impl Dimensions for Size {
        fn total_lines(&self) -> usize {
            10
        }
        fn screen_lines(&self) -> usize {
            10
        }
        fn columns(&self) -> usize {
            80
        }
    }

    /// Records the events the loop publishes.
    #[derive(Clone, Default)]
    struct Recorder(Arc<Mutex<Vec<String>>>);

    impl EventListener for Recorder {
        fn send_event(&self, event: Event) {
            self.0.lock().unwrap().push(format!("{event:?}"));
        }
    }

    /// What a read returns once the scripted chunks are used up.
    enum Then {
        /// Nothing more for now.
        Idle,
        /// The same byte, as fast as it is read: a writer that never stops.
        Endless(u8),
        /// A failing pty.
        Fail,
        /// The byte every other read, a millisecond apart, until the time
        /// given: a slow writer that keeps the pty readable.
        Trickle(u8, Instant),
    }

    struct Script {
        chunks: VecDeque<Vec<u8>>,
        then: Then,
        /// Sent after the chunks are read, like a shutdown racing the output.
        shutdown_after: Option<LoopSender>,
        served: usize,
        /// Whether the last trickled read had the byte.
        trickled: bool,
    }

    impl Read for Script {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if let Some(chunk) = self.chunks.pop_front() {
                let n = chunk.len().min(buf.len());
                buf[..n].copy_from_slice(&chunk[..n]);
                self.served += n;
                return Ok(n);
            }
            if let Then::Trickle(byte, until) = self.then
                && Instant::now() < until
            {
                // Every other read finds nothing, so the loop polls again.
                self.trickled = !self.trickled;
                if !self.trickled {
                    return Err(ErrorKind::WouldBlock.into());
                }
                std::thread::sleep(Duration::from_millis(1));
                buf[0] = byte;
                self.served += 1;
                return Ok(1);
            }
            if let Some(sender) = self.shutdown_after.take() {
                sender.send(Msg::Shutdown).unwrap();
            }
            match self.then {
                Then::Idle | Then::Trickle(..) => Err(ErrorKind::WouldBlock.into()),
                Then::Endless(byte) => {
                    let n = buf.len().min(4096);
                    buf[..n].fill(byte);
                    self.served += n;
                    Ok(n)
                }
                Then::Fail => Err(io::Error::other("injected")),
            }
        }
    }

    /// A pty whose wake-ups are a socket that is always readable, under the
    /// data token or (for a child that exited) the child token.
    struct FakePty {
        script: Script,
        wake: WakeStream,
        _peer: WakeStream,
        token: usize,
        child_exited: bool,
    }

    impl FakePty {
        fn new(script: Script, token: usize) -> FakePty {
            let (wake, peer) = wake_pair();
            (&peer).write_all(b"x").unwrap();
            FakePty {
                script,
                wake,
                _peer: peer,
                token,
                child_exited: token == PTY_CHILD_EVENT_TOKEN,
            }
        }
    }

    impl EventedReadWrite for FakePty {
        type Reader = Script;
        type Writer = io::Sink;

        unsafe fn register(
            &mut self,
            poll: &Arc<polling::Poller>,
            _: polling::Event,
            mode: polling::PollMode,
        ) -> io::Result<()> {
            unsafe { poll.add_with_mode(&self.wake, polling::Event::readable(self.token), mode) }
        }

        fn reregister(
            &mut self,
            poll: &Arc<polling::Poller>,
            _: polling::Event,
            mode: polling::PollMode,
        ) -> io::Result<()> {
            poll.modify_with_mode(&self.wake, polling::Event::readable(self.token), mode)
        }

        fn deregister(&mut self, poll: &Arc<polling::Poller>) -> io::Result<()> {
            poll.delete(&self.wake)
        }

        fn reader(&mut self) -> &mut Script {
            &mut self.script
        }

        fn writer(&mut self) -> &mut io::Sink {
            unreachable!("no input is sent")
        }
    }

    impl EventedPty for FakePty {
        fn next_child_event(&mut self) -> Option<ChildEvent> {
            std::mem::take(&mut self.child_exited).then_some(ChildEvent::Exited(None))
        }
    }

    impl OnResize for FakePty {
        fn on_resize(&mut self, _: WindowSize) {}
    }

    struct Run {
        text: String,
        events: Vec<String>,
        pty: FakePty,
        /// The prompt marks' lines.
        prompts: Vec<i32>,
    }

    /// The grid's text, history and screen.
    fn text<U>(terminal: &Term<U>) -> String {
        (terminal.topmost_line().0..=terminal.bottommost_line().0)
            .map(|line| {
                (0..80)
                    .map(|col| {
                        terminal.grid()[alacritty_terminal::index::Line(line)]
                            [alacritty_terminal::index::Column(col)]
                        .c
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Runs the loop to its end on `script`, which `make` may wire to the
    /// loop's sender.
    fn run(token: usize, chunks: &[&[u8]], then: Then, shutdown: bool) -> Run {
        let events = Recorder::default();
        let term = Arc::new(FairMutex::new(Term::new(
            Config {
                // Room for everything an endless writer is read for.
                scrolling_history: 100_000,
                ..Config::default()
            },
            &Size,
            events.clone(),
        )));
        let script = Script {
            chunks: chunks.iter().map(|chunk| chunk.to_vec()).collect(),
            then,
            shutdown_after: None,
            served: 0,
            trickled: false,
        };
        let shell = Arc::new(Mutex::new(ShellState::new(100_000)));
        let (mut pty_loop, sender) = PtyLoop::new(
            term.clone(),
            shell.clone(),
            events.clone(),
            FakePty::new(script, token),
        )
        .unwrap();
        if shutdown {
            pty_loop.pty.script.shutdown_after = Some(sender);
        }
        let pty = pty_loop.run();
        let terminal = term.lock();
        let text = text(&terminal);
        let prompts = lock(&shell).prompt_lines(&terminal);
        Run {
            text,
            events: events.0.lock().unwrap().clone(),
            pty,
            prompts,
        }
    }

    const HELD: &[u8] = b"\x1b[?2026hhello";

    #[test]
    fn shutdown_flushes_an_unfinished_synchronized_update() {
        // Run returns long before the parser's 150 ms synchronization timeout.
        let start = Instant::now();
        let run = run(PTY_READ_WRITE_TOKEN, &[HELD], Then::Idle, true);
        assert!(start.elapsed() < std::time::Duration::from_millis(150));
        assert!(run.text.contains("hello"), "{}", run.text);
        assert!(!run.events.iter().any(|event| event == "Exit"));
    }

    #[test]
    fn an_io_error_flushes_an_unfinished_synchronized_update() {
        let start = Instant::now();
        let run = run(PTY_READ_WRITE_TOKEN, &[HELD], Then::Fail, false);
        assert!(start.elapsed() < std::time::Duration::from_millis(150));
        assert!(run.text.contains("hello"), "{}", run.text);
        assert!(!run.events.iter().any(|event| event == "Exit"));
    }

    #[test]
    fn the_final_drain_stops_at_its_cap_when_the_writer_continues() {
        // Only the child-exit event is registered, so every byte is read by
        // the final drain.
        let run = run(PTY_CHILD_EVENT_TOKEN, &[HELD], Then::Endless(b'x'), false);
        assert!(run.text.contains("hello"), "{}", run.text);
        let served = run.pty.script.served;
        assert!(served >= MAX_LOCKED_READ, "drain stopped early: {served}");
        // One read per `MAX_LOCKED_READ`, at most `MAX_FINAL_DRAIN_READS`.
        assert!(
            served <= (MAX_FINAL_DRAIN_READS + 1) * (MAX_LOCKED_READ + 4096),
            "drain did not stop: {served}"
        );
        // Completion is published, after the held text reached the grid.
        assert!(
            run.events.iter().any(|event| event == "Exit"),
            "{:?}",
            run.events
        );
    }

    #[test]
    fn a_prompt_is_marked_where_the_cursor_is_left() {
        let prompt = b"a\r\nb\r\n\x1b]133;A\x07$ \r\n";
        let run = run(PTY_READ_WRITE_TOKEN, &[prompt], Then::Idle, true);
        assert_eq!(run.prompts, vec![2]);
    }

    #[test]
    fn a_prompt_in_a_synchronized_update_is_marked_where_it_was_printed() {
        let chunks: &[&[u8]] = &[
            b"\x1b[?2026ha\r\n\x1b]133;A\x07",
            b"$ \r\nb\r\n\x1b[?20",
            b"26l\x1b[?2026h\r\n\x1b]133;A\x07\r\n",
        ];
        // The second update ends as the loop shuts down.
        let run = run(PTY_READ_WRITE_TOKEN, chunks, Then::Idle, true);
        assert_eq!(run.prompts, vec![1, 4]);
        assert!(run.text.contains("$ "), "{}", run.text);
    }

    #[test]
    fn a_prompt_on_the_alternate_screen_is_not_marked_even_in_an_update() {
        let chunks: &[&[u8]] = &[
            b"\x1b[?2026h\x1b[?1049h\x1b]133;A\x07x\x1b[?1049l\x1b[?2026l",
            b"\x1b[?1049h\x1b]133;A\x07\x1b[?2026h\x1b[?1049l\x1b[?2026l",
        ];
        let run = run(PTY_READ_WRITE_TOKEN, chunks, Then::Idle, true);
        assert_eq!(run.prompts, Vec::<i32>::new());
    }

    /// A feeder and what it feeds.
    fn feeder() -> (Feeder, ShellState, Term<Recorder>) {
        let config = Config {
            scrolling_history: ShellState::term_history(100),
            ..Config::default()
        };
        let term = Term::new(config, &Size, Recorder::default());
        (Feeder::default(), ShellState::new(100), term)
    }

    #[test]
    fn a_repeated_start_renews_a_synchronized_update() {
        let (mut feeder, mut shell, mut term) = feeder();
        let start = Instant::now();
        feeder.feed(&mut shell, &mut term, b"\x1b[?2026hframe", start);
        assert_eq!(feeder.deadline(), Some(start + SYNC_TIMEOUT));
        // At 100 ms, a start split across reads.
        let renewed = start + Duration::from_millis(100);
        feeder.feed(&mut shell, &mut term, b" one\x1b[?20", renewed);
        feeder.feed(&mut shell, &mut term, b"26h two", renewed);
        assert_eq!(feeder.deadline(), Some(renewed + SYNC_TIMEOUT));
        assert!(!text(&term).contains("frame"));
        // The timeout ends it, all of it.
        feeder.end_sync(&mut shell, &mut term);
        assert_eq!(feeder.deadline(), None);
        assert!(text(&term).contains("frame one two"), "{}", text(&term));
    }

    #[test]
    fn a_screen_switch_can_start_a_synchronized_update() {
        for start in [&b"\x1b[?1049;2026h"[..], b"\x1b[?2026;1049h"] {
            let (mut feeder, mut shell, mut term) = feeder();
            let mut bytes = b"\x1b]133;A\x07$ ".to_vec();
            bytes.extend(start);
            bytes.extend(b"held\x1b[?2026lshown");
            // A byte at a time.
            let now = Instant::now();
            let (before_end, end) = bytes.split_at(bytes.len() - 13);
            for byte in before_end {
                feeder.feed(&mut shell, &mut term, &[*byte], now);
            }
            assert!(feeder.deadline().is_some());
            assert!(term.mode().contains(TermMode::ALT_SCREEN));
            assert!(!text(&term).contains("held"));
            for byte in end {
                feeder.feed(&mut shell, &mut term, &[*byte], now);
            }
            assert_eq!(feeder.deadline(), None);
            assert!(text(&term).contains("heldshown"), "{}", text(&term));
            // The prompt before the switch is still marked.
            term.swap_alt();
            assert_eq!(shell.prompt_lines(&term), vec![0]);
        }
    }

    #[test]
    fn adjacent_short_updates_are_fed_in_one_pass() {
        // A quadratic copy of what follows each update would take minutes.
        let (mut feeder, mut shell, mut term) = feeder();
        let mut bytes = b"\x1b[?2026h\x1b[?2026l".repeat(0x10_0000 / 16);
        bytes.extend(b"end");
        assert!(feeder.feed(&mut shell, &mut term, &bytes, Instant::now()));
        assert_eq!(feeder.deadline(), None);
        assert!(text(&term).contains("end"));
    }

    #[test]
    fn an_update_times_out_while_output_keeps_coming() {
        // The pty stays readable for 400 ms, well past the update's 150 ms.
        let until = Instant::now() + Duration::from_millis(400);
        let run = run(
            PTY_READ_WRITE_TOKEN,
            &[HELD],
            Then::Trickle(b'x', until),
            true,
        );
        assert!(run.text.contains("hellox"), "{}", run.text);
        // Wakeups for the timeout and the trickle after it, not only the
        // one as the loop ends.
        let wakeups = run.events.iter().filter(|event| *event == "Wakeup").count();
        assert!(wakeups > 2, "{:?}", run.events);
    }
}
