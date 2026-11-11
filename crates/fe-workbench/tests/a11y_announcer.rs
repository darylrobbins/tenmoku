// SPDX-License-Identifier: Apache-2.0

//! An announcement made anywhere in a Workbench window reaches its live
//! region's `Status` node.

use fe_buffer::Buffer;
use fe_gpui::{EditorSettings, EditorTheme};
use fe_ui::{Politeness, Role, announce, nodes};
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

#[gpui::test]
fn an_announcement_reaches_the_status_node(cx: &mut TestAppContext) {
    let (_dir, _workbench, cx) = opened(cx);
    cx.update(|_, cx| announce(cx, "Saved a.txt", Politeness::Assertive));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let status = nodes(window, cx)
            .into_iter()
            .find(|n| n.role == Role::Status)
            .expect("the Status node");
        assert_eq!(status.states.value.as_deref(), Some("Saved a.txt"));
        assert_eq!(status.states.live, Some(Politeness::Assertive));
    });
}
