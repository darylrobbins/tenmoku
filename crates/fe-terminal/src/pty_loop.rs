//! The loop that moves bytes between the pty and the shared `Term`.
//!
//! This is alacritty_terminal's `EventLoop`, cut down to what a session needs
//! (it always drains on exit and never records), because that loop keeps its
//! VT parser private: the parser holds the output after `ESC[?2026h` back
//! until the update ends or a timeout fires, and only the loop can end the
//! update. Owning the loop lets every way out of it flush the parser through
//! `Processor::stop_sync` before the exit is published.

use std::borrow::Cow;
use std::collections::VecDeque;
use std::io::{self, ErrorKind, Read, Write};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::Instant;

use alacritty_terminal::event::{Event, EventListener, OnResize, WindowSize};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Term;
use alacritty_terminal::tty::{self, EventedPty, EventedReadWrite};
use alacritty_terminal::vte::ansi::Processor;

/// Max bytes to read from the pty before forced terminal synchronization.
const READ_BUFFER_SIZE: usize = 0x10_0000;

/// Max bytes to parse while holding the terminal lock before letting go.
const MAX_LOCKED_READ: usize = u16::MAX as usize;

/// How many `MAX_LOCKED_READ` reads the final drain makes at most, so a
/// writer that outlives the child (one that inherited the pty) cannot keep
/// the loop from ending.
const MAX_FINAL_DRAIN_READS: usize = 64;

// alacritty's tokens for the pty's data and its child-exit notification. Its
// `Pty` registers with these keys and does not export them.
const PTY_READ_WRITE_TOKEN: usize = 0;
const PTY_CHILD_EVENT_TOKEN: usize = 1;

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
    events: U,
}

impl<P, U> PtyLoop<P, U>
where
    P: EventedPty + OnResize + Send + 'static,
    U: EventListener + Send + 'static,
{
    pub(crate) fn new(
        terminal: Arc<FairMutex<Term<U>>>,
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
        let mut parser: Processor = Processor::default();
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

        let mut child_exited = false;
        let mut events = polling::Events::with_capacity(NonZeroUsize::new(1024).unwrap());
        'event_loop: loop {
            // Wake for the synchronized update's timeout.
            let timeout = parser
                .sync_timeout()
                .sync_timeout()
                .map(|deadline| deadline.saturating_duration_since(Instant::now()));
            events.clear();
            if let Err(error) = self.poll.wait(&mut events, timeout) {
                if error.kind() == ErrorKind::Interrupted {
                    continue;
                }
                break;
            }

            if events.is_empty() {
                // Timed out, or a message arrived: handle messages below, and
                // end an update whose time is up.
                if parser
                    .sync_timeout()
                    .sync_timeout()
                    .is_some_and(|deadline| deadline <= Instant::now())
                {
                    parser.stop_sync(&mut *self.terminal.lock());
                    self.events.send_event(Event::Wakeup);
                }
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
                                match self.read(&mut parser, &mut buf) {
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
                            match self.read(&mut parser, &mut buf) {
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
        // published or the parser goes. `stop_sync` only releases the held
        // bytes, so an ordinary escape sequence still unfinished stays so.
        parser.stop_sync(&mut *self.terminal.lock());
        if child_exited {
            self.terminal.lock().exit();
        }
        self.events.send_event(Event::Wakeup);

        let _ = self.pty.deregister(&self.poll);
        self.pty
    }

    /// Reads what the pty has and parses it. Returns the bytes parsed.
    fn read(&mut self, parser: &mut Processor, buf: &mut [u8]) -> io::Result<usize> {
        // Reserve the next terminal lock for reading.
        let _lease = self.terminal.lease();
        let mut terminal = None;
        let mut unprocessed = 0;
        let mut processed = 0;

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

            parser.advance(&mut **terminal, &buf[..unprocessed]);
            processed += unprocessed;
            unprocessed = 0;

            if processed >= MAX_LOCKED_READ {
                break;
            }
        }

        // Redraw unless everything parsed is held in a synchronized update.
        if parser.sync_bytes_count() < processed && processed > 0 {
            self.events.send_event(Event::Wakeup);
        }
        Ok(processed)
    }
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
    use alacritty_terminal::term::Config;
    use alacritty_terminal::tty::ChildEvent;
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;
    use std::sync::Mutex;

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
    }

    struct Script {
        chunks: VecDeque<Vec<u8>>,
        then: Then,
        /// Sent after the chunks are read, like a shutdown racing the output.
        shutdown_after: Option<LoopSender>,
        served: usize,
    }

    impl Read for Script {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if let Some(chunk) = self.chunks.pop_front() {
                let n = chunk.len().min(buf.len());
                buf[..n].copy_from_slice(&chunk[..n]);
                self.served += n;
                return Ok(n);
            }
            if let Some(sender) = self.shutdown_after.take() {
                sender.send(Msg::Shutdown).unwrap();
            }
            match self.then {
                Then::Idle => Err(ErrorKind::WouldBlock.into()),
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
        wake: UnixStream,
        _peer: UnixStream,
        token: usize,
        child_exited: bool,
    }

    impl FakePty {
        fn new(script: Script, token: usize) -> FakePty {
            let (wake, peer) = UnixStream::pair().unwrap();
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
            unsafe {
                poll.add_with_mode(
                    self.wake.as_raw_fd(),
                    polling::Event::readable(self.token),
                    mode,
                )
            }
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
        };
        let (mut pty_loop, sender) =
            PtyLoop::new(term.clone(), events.clone(), FakePty::new(script, token)).unwrap();
        if shutdown {
            pty_loop.pty.script.shutdown_after = Some(sender);
        }
        let pty = pty_loop.run();
        let terminal = term.lock();
        let text = (terminal.topmost_line().0..=terminal.bottommost_line().0)
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
            .join("\n");
        Run {
            text,
            events: events.0.lock().unwrap().clone(),
            pty,
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
}
