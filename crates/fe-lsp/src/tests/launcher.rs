//! The launch seam: `LocalLauncher`, and launchers that refuse.

use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::{AsyncReadExt, AsyncWriteExt};

use super::{manager, wait_until};
use crate::{
    LaunchCancel, LocalLauncher, LspError, ServerIo, ServerLauncher, ServerSpec, ServerStatus,
};

#[test]
fn local_launcher_kills_the_process_group() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut spec = ServerSpec::new("sh", "/bin/sh");
    spec.args = vec![
        "-c".into(),
        "sleep 60 & echo $! > sleep.pid; printf %s \"$FE_LSP_TEST\" > env.txt; pwd -P > cwd.txt; cat"
            .into(),
    ];
    spec.env = vec![("FE_LSP_TEST".into(), "from the spec".into())];
    let ServerIo {
        mut input,
        mut output,
        mut process,
        ..
    } = LocalLauncher
        .launch(&spec, &root, &LaunchCancel::new())
        .unwrap();

    async_io::block_on(async {
        input.write_all(b"ping\n").await.unwrap();
        input.flush().await.unwrap();
        let mut echoed = [0; 5];
        output.read_exact(&mut echoed).await.unwrap();
        assert_eq!(&echoed, b"ping\n");
    });
    let read = |name: &str| std::fs::read_to_string(root.join(name)).unwrap();
    assert_eq!(read("cwd.txt").trim_end(), root.to_str().unwrap());
    assert_eq!(read("env.txt"), "from the spec");
    let sleep: i32 = read("sleep.pid").trim().parse().unwrap();
    let sleep = rustix::process::Pid::from_raw(sleep).unwrap();
    assert!(rustix::process::test_kill_process(sleep).is_ok());

    process.kill();
    let exit = async_io::block_on(process.exited());
    assert_eq!(exit.signal, Some(9), "{exit:?}");
    let deadline = Instant::now() + Duration::from_secs(1);
    while rustix::process::test_kill_process(sleep).is_ok() {
        assert!(
            Instant::now() < deadline,
            "the background sleep survived the kill"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    let missing = ServerSpec::new("missing", "/nonexistent/fe-lsp-test-server");
    match LocalLauncher.launch(&missing, &root, &LaunchCancel::new()) {
        Err(err) => assert_eq!(err.kind(), io::ErrorKind::NotFound),
        Ok(_) => panic!("launched a command that doesn't exist"),
    }
}

/// Refuses every launch, noting the thread each ran on.
#[derive(Default)]
struct Refuse {
    threads: Mutex<Vec<Option<String>>>,
}

impl ServerLauncher for Refuse {
    fn launch(&self, spec: &ServerSpec, _root: &Path, _: &LaunchCancel) -> io::Result<ServerIo> {
        self.threads
            .lock()
            .unwrap()
            .push(std::thread::current().name().map(str::to_owned));
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("the host doesn't run {}", spec.name),
        ))
    }
}

#[test]
fn launcher_errors_stop_the_server() {
    let root = tempfile::tempdir().unwrap();
    let launcher = Arc::new(Refuse::default());
    let (mut manager, events) = manager(launcher.clone(), None);
    let id = manager
        .start(ServerSpec::new("refused", "anything"), root.path())
        .unwrap();
    assert_eq!(events.status(id), ServerStatus::Starting);
    assert_eq!(
        events.status(id),
        ServerStatus::Stopped(Some("the host doesn't run refused".into()))
    );
    wait_until("the server thread to end", || manager.thread_finished(id));
    events.assert_quiet();
    let caller = std::thread::current().name().map(str::to_owned);
    let threads = launcher.threads.lock().unwrap().clone();
    assert_eq!(threads, [Some("fe-lsp refused".to_owned())]);
    assert_ne!(threads[0], caller);

    // No session: requests find nothing to answer them.
    let reply = manager.request::<lsp_types::request::Shutdown>(id, ());
    assert_eq!(async_io::block_on(reply), Err(LspError::ServerDown));
    assert_eq!(manager.position_encoding(id), None);
}
