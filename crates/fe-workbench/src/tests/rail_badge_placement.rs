//! The Changes count badge sits in its own button, not over the Chat
//! button below it.

use std::{fs, process::Command};

use gpui::{Bounds, Pixels, TestAppContext, VisualTestContext};
use kiln_shell::PanelId;

use super::open;
use crate::Launch;

fn git(dir: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("run git");
    assert!(output.status.success());
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn overlap(a: Bounds<Pixels>, b: Bounds<Pixels>) -> bool {
    a.left() < b.right() && b.left() < a.right() && a.top() < b.bottom() && b.top() < a.bottom()
}

#[gpui::test]
fn changes_badge_stays_inside_its_own_button(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    for file in ["a.rs", "b.rs", "c.rs"] {
        fs::write(root.join(file), format!("{file}\n")).unwrap();
    }
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    for file in ["a.rs", "b.rs", "c.rs"] {
        fs::write(root.join(file), "changed\n").unwrap();
    }
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let (workbench, cx, _) = open(cx, launch);
    draw(cx);
    let dock = workbench.read_with(cx, |workbench, _| workbench.dock().clone());
    workbench.update(cx, |workbench, cx| {
        let changes = workbench.dock().read(cx).changes().clone();
        changes.update(cx, crate::GitPanel::refresh);
    });
    draw(cx);
    let changes = dock.read_with(cx, |dock, _| {
        dock.state()
            .drawn()
            .rail_button_bounds(PanelId("scm"))
            .expect("drawn")
    });
    let badge = cx.debug_bounds("badge").expect("drawn");
    let chat_icon = cx
        .debug_bounds("rail-icon:dock-rail-3")
        .expect("Chat's icon");
    let chat = Bounds::new(gpui::point(changes.left(), changes.bottom()), changes.size);
    assert!(changes.contains(&badge.origin));
    assert!(badge.bottom() <= changes.bottom());
    assert!(!overlap(badge, chat));
    assert!(!overlap(badge, chat_icon));
}
