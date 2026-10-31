//! A test build's trash never reaches the user's Trash: the project panel's
//! Move to Trash goes, through `fe_project::fs_ops::trash`, to the test
//! trash of the process's own (`fe_project::fs_ops::testing`), as every
//! fe-workbench test build has fe-project's `test-support` on.

use std::{fs, os::unix::fs::MetadataExt as _};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use fe_project::fs_ops::testing;
use gpui::{Focusable as _, TestAppContext};

use super::{open_root, panel, rows};

#[gpui::test]
fn project_panel_delete_goes_to_the_test_trash(cx: &mut TestAppContext) {
    let dir = tempfile::Builder::new()
        .prefix("fe-workbench-isolation-")
        .tempdir()
        .unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    // A name of the test's own, to find it in the test trash by.
    let name = format!("{}.txt", root.file_name().unwrap().to_str().unwrap());
    let path = root.join(&name);
    fs::write(&path, "gone\n").unwrap();
    fs::write(root.join("kept.txt"), "").unwrap();
    let ino = fs::metadata(&path).unwrap().ino();
    let (workbench, cx) = open_root(cx, &root);
    // The panel focused, with the file selected.
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    cx.simulate_keystrokes("cmd-shift-e");
    let panel = panel(&workbench, cx);
    panel.update(cx, |panel, cx| panel.reveal(path.clone(), cx));
    cx.run_until_parked();
    let context = ContextStack::new()
        .with(ContextFrame::new("workbench"))
        .with(ContextFrame::new("project_panel"));
    let delete = RunCommand(
        Invocation::of("project_panel.delete")
            .unwrap()
            .context(context),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(delete), cx));
    cx.run_until_parked();
    cx.simulate_prompt_answer("Move to Trash");
    cx.run_until_parked();

    assert!(!path.exists(), "{} is still there", path.display());
    assert_eq!(rows(&panel, cx), [(0, "kept.txt".to_owned())]);
    let found = testing::trashed(&name);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with(testing::trash_dir()), "{found:?}");
    assert_eq!(fs::metadata(&found[0]).unwrap().ino(), ino);
    assert_eq!(fs::read_to_string(&found[0]).unwrap(), "gone\n");
}
