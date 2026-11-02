use std::fs;

use fe_project::fixture;
use gpui::TestAppContext;

use super::open;
use crate::resolve_launch;

#[test]
fn resolve_launch_picks_root_and_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fixture::generate(root, 1, 1).unwrap();
    let file = root.join(fixture::dir_name(0)).join(fixture::file_name(0));
    fs::write(&file, "text").unwrap();

    let launch = resolve_launch(Some(root.to_owned()));
    assert_eq!(launch.root.as_deref(), Some(root));
    assert!(launch.file.is_none());

    for file in [file.clone(), root.join("new.txt")] {
        let launch = resolve_launch(Some(file.clone()));
        assert_eq!(launch.root.as_deref(), file.parent());
        let (path, buffer) = launch.file.expect("a file to open");
        assert_eq!(path, file);
        assert!(buffer.is_none(), "read by the host");
    }

    let launch = resolve_launch(None);
    assert!(launch.root.is_none());
    assert!(launch.file.is_none());
}

#[gpui::test]
fn launch_with_no_argument_opens_no_tabs(cx: &mut TestAppContext) {
    let launch = resolve_launch(None);
    let (workbench, cx, _) = open(cx, launch);
    workbench.read_with(cx, |workbench, cx| {
        assert!(workbench.panel().read(cx).tree().is_none(), "no folder");
        let pane = workbench.pane().read(cx);
        assert!(pane.tabs().is_empty());
        assert_eq!(pane.title(cx), fe_config::APP_NAME);
        assert!(workbench.active_editor(cx).is_none());
    });
}
