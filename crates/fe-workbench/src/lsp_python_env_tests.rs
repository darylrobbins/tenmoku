//! A Python file's status item names the Python environment its server
//! runs in, with the environment's path in its tooltip, until a launch
//! without it; a Rust file's is as it was.

use std::fs;
use std::time::Duration;

use gpui::TestAppContext;
use kiln_shell::StatusSide;

use crate::tests::lsp::Lsp;

/// The status item's tooltip.
fn tooltip(s: &mut Lsp) -> Option<String> {
    let bar = s.workbench.read_with(&s.cx, |w, _| w.status_bar().clone());
    let items = bar.read_with(&s.cx, |bar, cx| bar.items(cx));
    let item = items
        .into_iter()
        .find(|item| item.side == StatusSide::Right && item.id == crate::lsp::ITEM)?;
    item.tooltip.map(|tooltip| tooltip.to_string())
}

#[gpui::test]
fn status_item_names_the_env(cx: &mut TestAppContext) {
    let mut s = Lsp::untrusted(cx);
    let venv = s.root.join(".venv");
    let python = if cfg!(windows) {
        venv.join("Scripts").join("python.exe")
    } else {
        venv.join("bin").join("python")
    };
    fs::create_dir_all(python.parent().unwrap()).unwrap();
    fs::write(&python, "").unwrap();
    // The test's own VIRTUAL_ENV, if it has one, mustn't win.
    s.write_user_settings("[lsp.servers.pyright]\nenv = { VIRTUAL_ENV = \"\" }\n");
    s.set_trust(true);

    s.open("tool.py");
    s.wait_item(Some("pyright (.venv): starting…"));
    s.python.wait_initialized();
    let clock = s.clock.clone();
    s.until("pyright to be ready", |s| {
        clock.advance(Duration::from_millis(100));
        s.item().as_deref() == Some("pyright (.venv): ready")
    });
    let tip = tooltip(&mut s).unwrap();
    assert!(
        tip.contains(&venv.display().to_string()),
        "{tip:?} names {}",
        venv.display()
    );
    // A click still shows the log.
    let bar = s.workbench.read_with(&s.cx, |w, _| w.status_bar().clone());
    let items = bar.read_with(&s.cx, |bar, cx| bar.items(cx));
    let item = items
        .iter()
        .find(|item| item.id == crate::lsp::ITEM)
        .unwrap();
    assert_eq!(
        item.command
            .as_ref()
            .map(|command| command.command.as_str()),
        Some("lsp.show_log")
    );

    // The item names the environment the server runs in: one removed
    // since counts from the next launch.
    fs::remove_file(&python).unwrap();
    s.draw();
    assert_eq!(s.item().as_deref(), Some("pyright (.venv): ready"));
    s.run("lsp.restart");
    s.python.wait_initialized_at(2);
    s.wait_item(Some("pyright: starting…"));
    assert_eq!(tooltip(&mut s).as_deref(), Some("Show Language Server Log"));

    // A Rust file's item is as it was: no environment, and none once
    // ready.
    s.open("src/main.rs");
    s.wait_item(Some("rust-analyzer: starting…"));
    assert_eq!(tooltip(&mut s).as_deref(), Some("Show Language Server Log"));
}

#[gpui::test]
fn a_server_of_python_and_rust_names_the_env_only_for_python(cx: &mut TestAppContext) {
    let mut s = Lsp::untrusted(cx);
    let python = if cfg!(windows) {
        s.root.join(".venv").join("Scripts").join("python.exe")
    } else {
        s.root.join(".venv").join("bin").join("python")
    };
    fs::create_dir_all(python.parent().unwrap()).unwrap();
    fs::write(&python, "").unwrap();
    // One server for both, on rust-analyzer's fake.
    s.write_user_settings(
        "[lsp.servers.rust-analyzer]\nenabled = false\n\
         [lsp.servers.pyright]\nenabled = false\n\
         [lsp.servers.both]\ncommand = \"both\"\nlanguages = [\"python\", \"rust\"]\n\
         env = { VIRTUAL_ENV = \"\" }\n",
    );
    s.set_trust(true);

    s.open("tool.py");
    s.wait_item(Some("both (.venv): starting…"));
    s.fake.wait_initialized();
    let clock = s.clock.clone();
    s.until("the server to be ready", |s| {
        clock.advance(Duration::from_millis(100));
        s.item().as_deref() == Some("both (.venv): ready")
    });

    // The same server, for a Rust file: no environment, so no item once
    // it's ready.
    s.open("src/main.rs");
    assert_eq!(s.fake.launches(), 1);
    s.wait_item(None);
    s.open("tool.py");
    s.wait_item(Some("both (.venv): ready"));
}
