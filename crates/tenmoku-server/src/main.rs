//! `tenmoku-server --stdio | daemon | --version`: see the library's docs.

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    use fe_remote_proto::BUILD;
    use std::process::ExitCode;
    use tenmoku_server::{Config, Daemon, Paths, log_line, stdio};

    let fail = |what: &str, e: std::io::Error| -> ExitCode {
        log_line(format_args!("{what}: {e}"));
        ExitCode::FAILURE
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["--stdio"] => match (Paths::from_env(BUILD), std::env::current_exe()) {
            (Ok(paths), Ok(exe)) => match stdio::attach(&paths, &exe) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => fail("can't reach the daemon", e),
            },
            (Err(e), _) | (_, Err(e)) => fail("can't attach", e),
        },
        ["daemon"] => {
            match Paths::from_env(BUILD).and_then(|paths| Daemon::claim(paths, Config::default())) {
                Ok(Some(daemon)) => {
                    daemon.run();
                    ExitCode::SUCCESS
                }
                // Another daemon took the lock first, and serves.
                Ok(None) => ExitCode::SUCCESS,
                Err(e) => fail("can't start the daemon", e),
            }
        }
        ["--version"] => {
            println!("tenmoku-server {BUILD}");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("usage: tenmoku-server --stdio | daemon | --version");
            ExitCode::from(2)
        }
    }
}

#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    eprintln!("tenmoku-server runs on Linux and macOS hosts (and in WSL)");
    std::process::ExitCode::FAILURE
}
