// SPDX-License-Identifier: Apache-2.0

//! A Workbench window's landmarks, on the render path Tenmoku draws
//! (`Workbench::render` around kiln-shell's `Shell` and docks): the
//! activity rail, the side panel, the editor area, the bottom dock and the
//! status bar, with the roles and labels a screen reader lists; and every
//! control the recorder sees has a label.

use fe_buffer::Buffer;
use fe_config::TerminalSettings;
use fe_gpui::{EditorSettings, EditorTheme};
use fe_ui::{Node, Role, nodes};
use fe_workbench::{Launch, Workbench};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_shell::DockView as _;

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
    std::fs::write(&path, "alpha\nbeta\n").unwrap();
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
    });
    let launch = Launch {
        root: Some(dir.path().to_path_buf()),
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
    (dir, workbench, cx)
}

/// What the recorder saw in a frame that drew every view afresh.
fn drawn(cx: &mut VisualTestContext) -> Vec<Node> {
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
        nodes(window, cx)
    })
}

fn landmarks(nodes: &[Node]) -> Vec<(Role, String)> {
    nodes
        .iter()
        .filter(|n| {
            matches!(
                n.role,
                Role::Navigation | Role::Complementary | Role::Main | Role::Region
            )
        })
        .map(|n| (n.role, n.label.to_string()))
        .collect()
}

#[gpui::test]
fn the_window_lists_its_five_landmarks(cx: &mut TestAppContext) {
    // Showing the terminal dock starts its shell: a quiet one, and its pty
    // thread may park the executor.
    cx.executor().allow_parking();
    let (_dir, workbench, cx) = opened(cx);
    let quiet = TerminalSettings {
        shell: "/bin/sh".into(),
        shell_args: vec!["-c".into(), "read x".into()],
        ..TerminalSettings::default()
    };
    workbench.update(cx, |workbench, cx| {
        workbench.set_terminal_settings(&quiet, cx);
        workbench.set_status_bar_visible(true, cx);
    });

    // Without the bottom dock: the rail, the side panel, the editor area
    // and the status bar.
    let side = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).dock().title(cx)
    });
    let side = side.expect("a side panel shown").to_string();
    let got = landmarks(&drawn(cx));
    for landmark in [
        (Role::Navigation, "Activity bar".to_string()),
        (Role::Complementary, side.clone()),
        (Role::Main, "Editor".to_string()),
        (Role::Region, "Status bar".to_string()),
    ] {
        assert!(got.contains(&landmark), "{landmark:?} not in {got:?}");
    }
    assert!(!got.contains(&(Role::Region, "Panel".into())), "{got:?}");

    // The bottom dock shown: its panel is the Region "Panel".
    let dock = workbench.read_with(cx, |workbench, _| workbench.terminal_dock().clone());
    dock.update(cx, |dock, cx| {
        if !dock.is_visible() {
            dock.toggle(cx);
        }
    });
    cx.run_until_parked();
    let got = landmarks(&drawn(cx));
    assert!(got.contains(&(Role::Region, "Panel".into())), "{got:?}");
    let named: Vec<_> = [
        "Activity bar",
        side.as_str(),
        "Editor",
        "Panel",
        "Status bar",
    ]
    .into_iter()
    .filter(|label| got.iter().any(|(_, l)| l == label))
    .collect();
    assert_eq!(named.len(), 5, "{got:?}");
}

#[gpui::test]
fn every_workbench_control_has_a_label(cx: &mut TestAppContext) {
    let (_dir, _workbench, cx) = opened(cx);
    let got = drawn(cx);
    let controls: Vec<_> = got
        .iter()
        .filter(|n| matches!(n.role, Role::Button | Role::CheckBox | Role::Switch))
        .collect();
    assert!(!controls.is_empty(), "{got:#?}");
    let unnamed: Vec<_> = controls
        .iter()
        .filter(|n| n.label.trim().is_empty())
        .collect();
    assert!(unnamed.is_empty(), "controls without a label: {unnamed:#?}");
    // The rail's buttons are among them, one toggled on for the shown
    // panel.
    let rail_on: Vec<_> = controls
        .iter()
        .filter(|n| n.states.toggled == Some(true))
        .collect();
    assert!(!rail_on.is_empty(), "{controls:#?}");
}
