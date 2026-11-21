//! The transport seam (D § 1): a byte pipe each way to the peer, and the
//! child process behind it, if any. `ssh -T`, `wsl.exe -d` and
//! `docker exec -i` are all a child whose stdio is the pipe.

use std::io::{self, Read, Write};
use std::process::{Child, Command, Stdio};

pub struct Link {
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
    /// Reaped once the link ends, and killed when it's lost to missed
    /// pongs or a protocol error.
    pub child: Option<Child>,
}

impl Link {
    pub fn new(reader: impl Read + Send + 'static, writer: impl Write + Send + 'static) -> Link {
        Link {
            reader: Box::new(reader),
            writer: Box::new(writer),
            child: None,
        }
    }

    /// Spawns `command` with its stdin and stdout as the link. Its stderr
    /// is left as the command has it (the connection log sets its own).
    pub fn spawn(command: &mut Command) -> io::Result<Link> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other("the child's stdio wasn't piped"));
        };
        Ok(Link {
            reader: Box::new(stdout),
            writer: Box::new(stdin),
            child: Some(child),
        })
    }

    /// Two ends of an in-process link over OS pipes, which hold what the
    /// system's pipes hold (64 KiB on Linux) before a write blocks.
    pub fn pipe() -> io::Result<(Link, Link)> {
        let (a_reader, b_writer) = io::pipe()?;
        let (b_reader, a_writer) = io::pipe()?;
        Ok((Link::new(a_reader, a_writer), Link::new(b_reader, b_writer)))
    }
}
