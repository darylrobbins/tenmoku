//! An expanded Graph commit lists only its file rows: its message body,
//! trailers and files-changed totals are left to its hover card.

use std::{path::Path, process::Command};

use gpui::TestAppContext;

use super::git_log_open_tests::{draw, lines, panel, press, row_of, window};

fn git(dir: &Path, args: &[&str]) {
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
    assert!(output.status.success(), "git {args:?} failed");
}

#[gpui::test]
fn file_rows_only(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    std::fs::write(root.join("a.rs"), "one\n").unwrap();
    std::fs::write(root.join("b.rs"), "one\n").unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "base"]);
    std::fs::write(root.join("a.rs"), "two\n").unwrap();
    std::fs::write(root.join("b.rs"), "two\n").unwrap();
    git(root, &["add", "-A"]);
    let message = "tall\n\nfirst line\nsecond line\n\nmore\n\n\
                   Regatta-Task: T1\n(cherry picked from commit abcdef0)\n\
                   Co-authored-by: Pat <pat@example.com>\n";
    git(root, &["commit", "-q", "-m", message]);

    let (workbench, mut cx) = window(cx, root);
    let cx = &mut cx;
    let all = lines(&workbench, cx);
    let tall = row_of(&all, "tall");
    let panel = panel(&workbench, cx);
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.open_row(tall, window, cx)));
    draw(cx);
    press("G", cx);

    let listed = lines(&workbench, cx);
    assert_eq!(listed[tall + 1..tall + 3], ["M a.rs", "M b.rs"]);
    assert_eq!(listed.len(), all.len() + 2, "{listed:?}");
    for text in ["first line", "Regatta-Task", "cherry picked", "changed"] {
        assert!(listed.iter().all(|line| !line.contains(text)), "{listed:?}");
    }
}
