//! Baseline of what assistive technology sees of a Workbench window.
//!
//! `Window::debug_a11y_tree_json` returns the last accessibility tree update
//! gpui-pre built for the window.
//!
//! **A headless test window yields no tree.** gpui-pre only builds one once the
//! platform's screen-reader activation callback fires (`A11y::is_active`), and
//! the test platform never fires it, so `is_a11y_active()` stays `false` and
//! `debug_a11y_tree_json()` returns `None` after any number of draws. The
//! first test therefore pins that state; if a gpui-pre upgrade starts giving a
//! tree, it checks that the dump parses and has a root. The tree can be read
//! only from the running app (see `docs/accessibility-audit.md`). Run
//! `cargo test -p fe-workbench --test a11y_tree_dump -- --ignored --nocapture`
//! to print the JSON.

use fe_buffer::Buffer;
use fe_gpui::{EditorSettings, EditorTheme};
use fe_workbench::{Launch, Workbench};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

fn opened(
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, Entity<Workbench>, &mut VisualTestContext) {
    // See palette_ghost_text.rs: no config directory, no watcher threads.
    for var in ["FASTEDIT_CONFIG_DIR", "XDG_CONFIG_HOME", "HOME"] {
        // SAFETY: no other thread is running yet to read the environment.
        unsafe { std::env::remove_var(var) };
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
    });
    let launch = Launch {
        root: None,
        file: Some((path, Some(Buffer::new("alpha\nbeta\n").into()))),
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    (dir, workbench, cx)
}

fn dump_json(cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|window, _| window.debug_a11y_tree_json())
}

#[gpui::test]
fn headless_has_no_tree_and_any_tree_has_a_root(cx: &mut TestAppContext) {
    let (_dir, _workbench, cx) = opened(cx);
    let active = cx.update(|window, _| window.is_a11y_active());
    let json = dump_json(cx);
    println!("a11y active: {active}, dump present: {}", json.is_some());
    let Some(json) = json else {
        assert!(!active, "an active window retains its tree update");
        return;
    };
    let value: serde_json::Value = serde_json::from_str(&json).expect("the dump is JSON");
    assert!(value["root"].is_string(), "a root node: {value}");
    assert!(value["nodes"].is_object());
}

#[gpui::test]
#[ignore = "prints the tree: run with --ignored --nocapture"]
fn dump(cx: &mut TestAppContext) {
    let (_dir, _workbench, cx) = opened(cx);
    match dump_json(cx) {
        Some(json) => println!("{json}"),
        None => println!("no a11y tree in this window"),
    }
}
