//! Move to New Window's destination window (T1606 F3): kept when the user
//! has used it while the moved file's load was pending.

use std::fs;

use gpui::TestAppContext;

use crate::Workbench;

/// Runs `cx` until `done` says so, the files' reads being real ones.
fn wait_for(cx: &mut TestAppContext, mut done: impl FnMut(&mut TestAppContext) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "timed out");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

/// Each pane's files, in `window`.
fn window_files(
    window: gpui::WindowHandle<Workbench>,
    cx: &mut TestAppContext,
) -> Vec<Vec<std::path::PathBuf>> {
    window
        .read_with(cx, |workbench, cx| {
            (workbench.pane_group(cx).panes().iter())
                .map(|pane| {
                    (pane.read(cx).tabs().iter())
                        .filter_map(|tab| tab.path.clone())
                        .collect()
                })
                .collect()
        })
        .unwrap()
}

/// A destination window the user has used while the moved file's load was
/// pending stays when the load fails, with what they opened there
/// (T1606 F3); the failed tab shows its error as usual.
#[gpui::test]
fn a_destination_the_user_used_stays_when_the_load_fails(cx: &mut TestAppContext) {
    assert!(
        std::env::var_os("TENMOKU_STATE_DIR").is_some(),
        "needs the state dir nextest's setup script gives (scripts/nextest-isolate-state.sh)"
    );
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    let (a, missing) = (root.join("a.txt"), root.join("missing.txt"));
    fs::write(&a, "one\n").unwrap();
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
        cx.set_global(fe_workbench::ConfigDirOverride(None));
    });
    crate::tests::WATCH.with(|w| w.set(Some(|_, _| None)));

    let windows = cx.windows().len();
    let file = fe_workbench::WindowFile {
        path: missing.clone(),
        line: None,
        col: None,
    };
    let task = cx.update(|cx| crate::open_window_on(Some(root.clone()), Some(file), cx));
    // The user opens a file there before the load has ended.
    let window = (cx.windows().into_iter())
        .filter_map(|window| window.downcast::<Workbench>())
        .next_back()
        .expect("the destination window");
    window
        .update(cx, |workbench, window, cx| {
            workbench.open_document(a.clone(), window, cx)
        })
        .unwrap();
    let landed = std::rc::Rc::new(std::cell::Cell::new(None));
    let set = landed.clone();
    cx.spawn(async move |_| set.set(Some(task.await))).detach();
    wait_for(cx, |_| landed.get().is_some());

    // A missing file didn't load, but the window and the file stay.
    assert_eq!(landed.get(), Some(false));
    assert_eq!(cx.windows().len(), windows + 1);
    assert!(window_files(window, cx)[0].contains(&a));
    crate::tests::WATCH.with(|w| w.set(None));
}
