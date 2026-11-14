// SPDX-License-Identifier: Apache-2.0

//! The fake debug adapter (`fe_dap::fake`) as a process, for tests of
//! `LocalLauncher` with a real child.
//!
//! ```text
//! fe-dap-fake --port <port> [--script <json>]   listen on 127.0.0.1:<port>, serve one connection
//! fe-dap-fake --stdio [--script <json>]         serve stdin and stdout
//! fe-dap-fake --never-listen [--port <port>]    run until killed, accepting nothing
//! ```
//!
//! `--pid-file <path>` first writes its process id to `<path>`.
//!
//! `--script` is a [`Script`] as JSON (every field optional), inline when it
//! starts with `{`, else the path of a file. It exits with the code the fake
//! ends with.

use std::net::{Ipv4Addr, TcpListener};
use std::process::ExitCode;

use async_io::Async;
use fe_dap::fake::{Script, serve};
use futures::AsyncReadExt as _;
use futures::io::AllowStdIo;

enum Mode {
    Port(u16),
    Stdio,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(why) => {
            eprintln!("fe-dap-fake: {why}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let mut mode = None;
    let mut never_listen = false;
    let mut script = Script::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => {
                let port = args.next().ok_or("--port needs a value")?;
                mode = Some(Mode::Port(
                    port.parse()
                        .map_err(|err| format!("bad port {port}: {err}"))?,
                ));
            }
            "--stdio" => mode = Some(Mode::Stdio),
            "--never-listen" => never_listen = true,
            "--pid-file" => {
                let path = args.next().ok_or("--pid-file needs a value")?;
                std::fs::write(&path, std::process::id().to_string())
                    .map_err(|err| format!("writing {path}: {err}"))?;
            }
            "--script" => {
                let given = args.next().ok_or("--script needs a value")?;
                let json = if given.starts_with('{') {
                    given
                } else {
                    std::fs::read_to_string(&given)
                        .map_err(|err| format!("reading {given}: {err}"))?
                };
                script = serde_json::from_str(&json).map_err(|err| format!("bad script: {err}"))?;
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if never_listen {
        loop {
            std::thread::park();
        }
    }
    let fake = match mode.ok_or("give --port, --stdio or --never-listen")? {
        Mode::Stdio => serve(
            script,
            AllowStdIo::new(std::io::stdin()),
            AllowStdIo::new(std::io::stdout()),
        ),
        Mode::Port(port) => {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
                .map_err(|err| format!("listening on {port}: {err}"))?;
            let (stream, _) = listener
                .accept()
                .map_err(|err| format!("accepting: {err}"))?;
            let (reader, writer) = Async::new(stream).map_err(|err| err.to_string())?.split();
            serve(script, reader, writer)
        }
    };
    let exit = async_io::block_on(fake.exited());
    Ok(ExitCode::from(exit.code.unwrap_or(1).clamp(0, 255) as u8))
}
