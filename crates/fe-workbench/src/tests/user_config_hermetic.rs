//! A test with no config directory of its own has no user config: a
//! developer's `settings.toml` that configures a language server must not
//! start that server in a test.

use std::{
    fs, io,
    path::Path,
    sync::{Arc, Mutex},
};

use fe_lsp::{LaunchCancel, ServerIo, ServerLauncher, ServerSpec};
use gpui::TestAppContext;

use super::open_root;
use crate::{set_server_launcher, set_trust_file};

/// Records the command of every server it is asked to start, and starts none.
struct Recorder(Arc<Mutex<Vec<String>>>);

impl ServerLauncher for Recorder {
    fn launch(
        &self,
        spec: &ServerSpec,
        _root: &Path,
        _cancel: &LaunchCancel,
    ) -> io::Result<ServerIo> {
        self.0
            .lock()
            .unwrap()
            .push(spec.command.display().to_string());
        Err(io::Error::from(io::ErrorKind::Other))
    }
}

#[gpui::test]
fn a_user_settings_file_in_the_environment_starts_no_server(cx: &mut TestAppContext) {
    let config = tempfile::tempdir().unwrap();
    fs::write(
        config.path().join("settings.toml"),
        "[lsp.servers.bogus]\ncommand = \"/nonexistent-should-not-run\"\nlanguages = [\"go\"]\nroot_markers = [\"go.mod\"]\n",
    )
    .unwrap();
    // SAFETY: the test runs alone in its own process, on one thread.
    unsafe { std::env::set_var("TENMOKU_CONFIG_DIR", config.path()) };

    let project = tempfile::tempdir().unwrap();
    let root = project.path().canonicalize().unwrap();
    fs::write(root.join("go.mod"), "module m\n").unwrap();
    fs::write(root.join("main.go"), "package main\n").unwrap();

    let recorded = Arc::new(Mutex::new(Vec::new()));
    let state = tempfile::tempdir().unwrap();
    cx.update(|cx| {
        set_server_launcher(cx, Arc::new(Recorder(recorded.clone())));
        set_trust_file(cx, state.path().join("trust.toml"));
    });
    let (workbench, cx) = open_root(cx, &root);
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    let pane = workbench.read_with(cx, |w, _| w.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(root.join("main.go"), window, cx)));
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    assert!(recorded.lock().unwrap().is_empty());
    assert_eq!(*recorded.lock().unwrap(), Vec::<String>::new());
}
