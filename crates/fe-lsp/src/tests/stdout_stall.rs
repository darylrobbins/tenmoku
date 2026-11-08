//! A server that reads its input only while its output isn't blocked, as
//! rust-analyzer's `lsp-server` does: its reader hands each message to its
//! main thread over a rendezvous channel, and the main thread writes each
//! reply before taking the next. With big messages both ways, the client
//! must keep reading the server's output while its own write waits, or
//! each waits for the other for ever. And what it holds of that output
//! while it waits is bounded: past the bound, the session ends.

use std::io::{self, BufRead, BufReader, Write};
use std::net::Shutdown;
#[cfg(unix)]
use std::os::unix::net::UnixStream;

/// A transport end: on Windows, which has no socket pairs in std, a loopback
/// TCP connection's.
#[cfg(windows)]
type UnixStream = std::net::TcpStream;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::mpsc::sync_channel;

use async_io::Async;
use futures::channel::oneshot;
use futures::future::{FutureExt, Shared};
use lsp_types::request::Request;
use serde_json::{Value, json};

use super::{ROOT, ServerStatus, answer, capture_logs, logs_from, manager};
use crate::{ExitInfo, LaunchCancel, ServerIo, ServerLauncher, ServerProcess, ServerSpec};

/// Each request's padding, and each reply's: more than a socket buffer.
const BIG: usize = 256 << 10;

enum Big {}

impl Request for Big {
    type Params = Value;
    type Result = Value;
    const METHOD: &'static str = "stall/big";
}

/// Asks the server for more output than the client holds unread
/// (`BACKLOG_MAX`, 64 MiB) before its answer.
enum Flood {}

impl Request for Flood {
    type Params = Value;
    type Result = Value;
    const METHOD: &'static str = "stall/flood";
}

/// How many [`BIG`] notifications answer a [`Flood`]: 66 MiB.
const FLOOD: usize = 264;

/// Launches [`serve`] on its own threads, over a `UnixStream` pair with
/// the OS's default buffers.
struct Coupled {
    /// Each launch's server end, shut when the test ends however it ends,
    /// so a stalled run doesn't leave the server's threads blocked.
    servers: std::sync::Mutex<Vec<UnixStream>>,
}

impl ServerLauncher for Coupled {
    fn launch(&self, _: &ServerSpec, _: &Path, _: &LaunchCancel) -> io::Result<ServerIo> {
        let (client, server) = stream_pair()?;
        self.servers.lock().unwrap().push(server.try_clone()?);
        let (exit, exited) = oneshot::channel();
        let shut = server.try_clone()?;
        std::thread::Builder::new()
            .name("stall server".into())
            .spawn(move || {
                serve(server);
                let _ = exit.send(());
            })?;
        Ok(ServerIo {
            input: Box::pin(Async::new(client.try_clone()?)?),
            output: Box::pin(Async::new(client)?),
            stderr: None,
            process: Box::new(Process {
                exited: exited.shared(),
                shut,
            }),
        })
    }
}

/// Shuts the server's end of the transport when dropped.
struct Guard(Arc<Coupled>);

impl Drop for Guard {
    fn drop(&mut self) {
        for server in &*self.0.servers.lock().unwrap() {
            let _ = server.shutdown(Shutdown::Both);
        }
    }
}

struct Process {
    exited: Shared<oneshot::Receiver<()>>,
    shut: UnixStream,
}

impl ServerProcess for Process {
    fn exited(&mut self) -> Pin<Box<dyn Future<Output = ExitInfo> + Send + '_>> {
        let exited = self.exited.clone();
        Box::pin(async move {
            let _ = exited.await;
            ExitInfo::default()
        })
    }

    fn kill(&mut self) {
        let _ = self.shut.shutdown(Shutdown::Both);
    }
}

/// Answers every request on `stream` until `exit`, or until its transport
/// fails: a `stall/big` with [`BIG`] bytes of padding, a `stall/flood`
/// after [`FLOOD`] notifications as big, anything else with what
/// `initialize` and `shutdown` need.
fn serve(stream: UnixStream) {
    let (handed, taken) = sync_channel::<Value>(0);
    let reader = stream.try_clone().expect("clone the server's end");
    std::thread::Builder::new()
        .name("stall server reader".into())
        .spawn(move || {
            let mut reader = BufReader::new(reader);
            while let Some(message) = read_message(&mut reader) {
                if handed.send(message).is_err() {
                    return;
                }
            }
        })
        .expect("spawn the server's reader");
    let mut writer = stream;
    let frame = |message: Value| {
        let message = message.to_string();
        format!("Content-Length: {}\r\n\r\n{message}", message.len())
    };
    for message in taken {
        if message["method"] == "exit" {
            return;
        }
        let Some(id) = message.get("id") else {
            continue;
        };
        if message["method"] == Flood::METHOD {
            let note = frame(json!({
                "jsonrpc": "2.0",
                "method": "stall/flooding",
                "params": { "pad": "f".repeat(BIG) },
            }));
            for _ in 0..FLOOD {
                if writer.write_all(note.as_bytes()).is_err() {
                    return;
                }
            }
        }
        let result = match message["method"].as_str() {
            Some("initialize") => json!({ "capabilities": {} }),
            Some(Big::METHOD) => json!({ "pad": "r".repeat(BIG) }),
            _ => Value::Null,
        };
        let reply = frame(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
        if writer.write_all(reply.as_bytes()).is_err() {
            return;
        }
    }
}

/// The next message on `reader`, or `None` at its end or on an error.
fn read_message(reader: &mut impl BufRead) -> Option<Value> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(n) = line.strip_prefix("Content-Length: ") {
            length = Some(n.parse::<usize>().ok()?);
        }
    }
    let mut body = vec![0; length?];
    reader.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

#[test]
fn big_requests_and_replies_cross_without_a_stall() {
    let launcher = Arc::new(Coupled {
        servers: std::sync::Mutex::default(),
    });
    let (mut manager, events) = manager(launcher.clone(), None);
    // Dropped before the manager: a stalled run's threads then end.
    let _guard = Guard(launcher);
    let server = manager
        .start(ServerSpec::new("stall", "stall-server"), Path::new(ROOT))
        .unwrap();
    assert_eq!(events.status(server), ServerStatus::Starting);

    let replies: Vec<_> = (0..8)
        .map(|n| manager.request::<Big>(server, json!({ "n": n, "pad": "q".repeat(BIG) })))
        .collect();
    let replies = answer(futures::future::join_all(replies));
    for reply in replies {
        let reply = reply.expect("an answer");
        assert_eq!(reply.result["pad"].as_str().map(str::len), Some(BIG));
    }
}

#[test]
fn an_output_backlog_past_its_bound_ends_the_session_while_a_write_waits() {
    capture_logs("fe-lsp stall-flood");
    let launcher = Arc::new(Coupled {
        servers: std::sync::Mutex::default(),
    });
    let (mut manager, events) = manager(launcher.clone(), None);
    let _guard = Guard(launcher);
    let server = manager
        .start(
            ServerSpec::new("stall-flood", "stall-server"),
            Path::new(ROOT),
        )
        .unwrap();
    assert_eq!(events.status(server), ServerStatus::Starting);

    // The server floods its output, reading nothing more meanwhile, while
    // the client's writes behind the flood's request outgrow the socket's
    // buffers: the client's loop waits for a write.
    let flood = manager.request::<Flood>(server, json!({}));
    let big: Vec<_> = (0..8)
        .map(|n| manager.request::<Big>(server, json!({ "n": n, "pad": "q".repeat(BIG) })))
        .collect();
    // Past the bound, the session ends as a crash: the server is killed,
    // and starts again.
    events.until("a restart", |event| match event {
        crate::LspEvent::Status {
            status: ServerStatus::Restarting(_),
            ..
        } => Some(()),
        _ => None,
    });
    let logs = logs_from("fe-lsp stall-flood");
    assert!(
        logs.iter()
            .any(|line| line.contains("server output backlog over 64 MiB")),
        "{logs:?}"
    );
    drop((flood, big));
}

/// A connected pair of transport ends.
#[cfg(unix)]
fn stream_pair() -> io::Result<(UnixStream, UnixStream)> {
    UnixStream::pair()
}

/// A connected pair of transport ends: a loopback connection's.
#[cfg(windows)]
fn stream_pair() -> io::Result<(UnixStream, UnixStream)> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let near = UnixStream::connect(listener.local_addr()?)?;
    let (far, _) = listener.accept()?;
    Ok((near, far))
}
