// SPDX-License-Identifier: Apache-2.0

//! A child program's stdio, fed and drained by one thread without blocking: on Unix through
//! socket pairs made non-blocking; on Windows, whose anonymous pipes can't be made
//! non-blocking, through a thread per stream. And a program started without a console window.

use std::io::{self, Read as _, Write as _};
#[cfg(unix)]
use std::os::fd::OwnedFd;
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};
#[cfg(windows)]
use std::thread;

/// Has `command`'s program start with no console window of its own, as one run from a GUI app
/// should (`CREATE_NO_WINDOW` on Windows; elsewhere there's nothing to do).
pub fn without_console_window(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(command, 0x0800_0000);
    command
}

/// Our end of one of the program's output streams, from [`OutputDrain::pair`].
#[cfg(unix)]
pub struct OutputSource(UnixStream);

/// Our end of one of the program's output streams, from [`OutputDrain::pair`].
#[cfg(windows)]
pub struct OutputSource(io::PipeReader);

impl OutputDrain {
    /// Whether it went past its cap.
    pub fn over(&self) -> bool {
        self.over
    }

    /// The bytes it kept.
    pub fn into_kept(self) -> Vec<u8> {
        self.kept
    }
}

/// Bytes for a program's stdin, on their way into it.
#[cfg(unix)]
pub struct StdinFeed<'a> {
    socket: UnixStream,
    bytes: &'a [u8],
}

#[cfg(unix)]
impl<'a> StdinFeed<'a> {
    /// A feed of `bytes`, and the program's end of it.
    pub fn pair(bytes: &'a [u8]) -> io::Result<(Self, Stdio)> {
        let (ours, theirs) = UnixStream::pair()?;
        Ok((
            Self {
                socket: ours,
                bytes,
            },
            OwnedFd::from(theirs).into(),
        ))
    }

    /// Readies the feed once the program has started.
    pub fn start(&mut self) -> io::Result<()> {
        self.socket.set_nonblocking(true)
    }

    /// Writes what the socket takes: whether any went, or `None` once there's nothing more to
    /// write or the program won't read it.
    pub fn write(&mut self) -> Option<bool> {
        if self.bytes.is_empty() {
            return None;
        }
        match self.socket.write(self.bytes) {
            Ok(written) => {
                self.bytes = &self.bytes[written..];
                Some(written > 0)
            }
            Err(error) if retry(&error) => Some(false),
            Err(_) => None,
        }
    }
}

/// One of the program's output streams, read as far as it goes without blocking.
#[cfg(unix)]
pub struct OutputDrain {
    /// None once it ended, or went past the cap.
    socket: Option<UnixStream>,
    kept: Vec<u8>,
    over: bool,
}

#[cfg(unix)]
impl OutputDrain {
    /// Our end of a new output stream, and the program's.
    pub fn pair() -> io::Result<(OutputSource, Stdio)> {
        let (ours, theirs) = UnixStream::pair()?;
        Ok((OutputSource(ours), OwnedFd::from(theirs).into()))
    }

    pub fn new(OutputSource(socket): OutputSource) -> io::Result<Self> {
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket: Some(socket),
            kept: Vec::new(),
            over: false,
        })
    }

    pub fn ended(&self) -> bool {
        self.socket.is_none()
    }

    /// Reads what is there, keeping `cap` bytes; at the first byte past them it stops for
    /// good. Whether anything was read.
    pub fn read(&mut self, cap: usize) -> io::Result<bool> {
        let Some(socket) = &mut self.socket else {
            return Ok(false);
        };
        let mut chunk = [0; 16 << 10];
        let mut moved = false;
        loop {
            match socket.read(&mut chunk) {
                Ok(0) => {
                    self.socket = None;
                    return Ok(true);
                }
                Ok(read) => {
                    moved = true;
                    let room = cap - self.kept.len();
                    self.kept.extend_from_slice(&chunk[..read.min(room)]);
                    if read > room {
                        self.over = true;
                        self.socket = None;
                        return Ok(true);
                    }
                }
                Err(error) if retry(&error) => return Ok(moved),
                Err(error) => return Err(error),
            }
        }
    }
}

/// Bytes for a program's stdin, on their way into it. Windows' anonymous pipes can't be
/// made non-blocking, so a thread of its own writes them; it ends once all are written, or the
/// program's end closes (it exited, or was killed).
#[cfg(windows)]
pub struct StdinFeed<'a> {
    pipe: Option<io::PipeWriter>,
    bytes: &'a [u8],
}

#[cfg(windows)]
impl<'a> StdinFeed<'a> {
    /// A feed of `bytes`, and the program's end of it.
    pub fn pair(bytes: &'a [u8]) -> io::Result<(Self, Stdio)> {
        let (theirs, ours) = io::pipe()?;
        Ok((
            Self {
                pipe: Some(ours),
                bytes,
            },
            theirs.into(),
        ))
    }

    /// Starts the thread that writes the bytes, once the program has started.
    pub fn start(&mut self) -> io::Result<()> {
        let Some(mut pipe) = self.pipe.take() else {
            return Ok(());
        };
        let bytes = self.bytes.to_vec();
        self.bytes = &[];
        thread::Builder::new()
            .name("plugin-process-stdin".into())
            .spawn(move || drop(pipe.write_all(&bytes)))?;
        Ok(())
    }

    /// The thread writes everything: nothing is left to write here.
    pub fn write(&mut self) -> Option<bool> {
        None
    }
}

/// One of the program's output streams. Windows' anonymous pipes can't be made non-blocking,
/// so a thread of its own reads it into a channel, which is read as far as it goes without
/// blocking. The thread ends at the stream's end: when the program, and anything it started
/// that holds the stream, exits.
#[cfg(windows)]
pub struct OutputDrain {
    /// None once it ended, or went past the cap.
    chunks: Option<std::sync::mpsc::Receiver<io::Result<Vec<u8>>>>,
    kept: Vec<u8>,
    over: bool,
}

#[cfg(windows)]
impl OutputDrain {
    /// Our end of a new output stream, and the program's.
    pub fn pair() -> io::Result<(OutputSource, Stdio)> {
        let (ours, theirs) = io::pipe()?;
        Ok((OutputSource(ours), theirs.into()))
    }

    pub fn new(OutputSource(mut pipe): OutputSource) -> io::Result<Self> {
        let (send, chunks) = std::sync::mpsc::channel();
        thread::Builder::new()
            .name("plugin-process-output".into())
            .spawn(move || {
                let mut chunk = vec![0; 16 << 10];
                loop {
                    let sent = match pipe.read(&mut chunk) {
                        Ok(0) => return,
                        Ok(read) => send.send(Ok(chunk[..read].to_vec())),
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                        // A broken pipe is its end, as a socket's EOF is.
                        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => return,
                        Err(error) => send.send(Err(error)),
                    };
                    // The run is over: nothing reads what follows.
                    if sent.is_err() {
                        return;
                    }
                }
            })?;
        Ok(Self {
            chunks: Some(chunks),
            kept: Vec::new(),
            over: false,
        })
    }

    pub fn ended(&self) -> bool {
        self.chunks.is_none()
    }

    /// Takes what the thread has read, keeping `cap` bytes; at the first byte past them it stops
    /// for good. Whether anything was read.
    pub fn read(&mut self, cap: usize) -> io::Result<bool> {
        use std::sync::mpsc::TryRecvError;

        let Some(chunks) = &self.chunks else {
            return Ok(false);
        };
        let mut moved = false;
        loop {
            match chunks.try_recv() {
                Ok(Ok(chunk)) => {
                    moved = true;
                    let room = cap - self.kept.len();
                    self.kept.extend_from_slice(&chunk[..chunk.len().min(room)]);
                    if chunk.len() > room {
                        self.over = true;
                        self.chunks = None;
                        return Ok(true);
                    }
                }
                Ok(Err(error)) => return Err(error),
                Err(TryRecvError::Empty) => return Ok(moved),
                Err(TryRecvError::Disconnected) => {
                    self.chunks = None;
                    return Ok(true);
                }
            }
        }
    }
}

/// Whether `error` only says to try again later.
#[cfg(unix)]
fn retry(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
    )
}
