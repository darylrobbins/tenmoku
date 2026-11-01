//! `editor.toggle_provenance`: who wrote each row, in the annotation column.

use std::{path::Path, process::Command, time::Duration};

use fe_buffer::Origin;
use fe_gpui::EditorView;
use gpui::{Entity, TestAppContext, VisualTestContext};

use crate::{
    Workbench,
    tests::find_bar::{draw, editor, opened, pane, run},
};

const TOGGLE: &str = "editor.toggle_provenance";
const BLAME: &str = "git.toggle_blame_annotations";
const TEXT: &str = "one\ntwo\nthree\nfour\n";

fn ai(agent: &str) -> Origin {
    Origin::Ai {
        agent: agent.into(),
        session: "s1".into(),
    }
}

fn edit(
    tab: &Entity<EditorView>,
    at: usize,
    text: &str,
    origin: Origin,
    cx: &mut VisualTestContext,
) {
    tab.update(cx, |tab, cx| {
        let text = text.to_owned();
        tab.model_mut().buffer_mut().edit([(at..at, text)], origin);
        cx.notify();
    });
    draw(cx);
}

/// The column's entries: text and heat.
fn entries(
    tab: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> Option<Vec<(String, Option<f32>)>> {
    tab.read_with(cx, |tab, _| {
        tab.gutter_annotations().map(|shown| {
            (shown.entries.iter())
                .map(|entry| (entry.text.to_string(), entry.heat))
                .collect()
        })
    })
}

fn texts(tab: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<String> {
    entries(tab, cx)
        .expect("a column is shown")
        .into_iter()
        .map(|(text, _)| text)
        .collect()
}

fn hover(tab: &Entity<EditorView>, ix: usize, cx: &mut VisualTestContext) -> Option<String> {
    tab.read_with(cx, |tab, _| {
        let shown = tab.gutter_annotations()?;
        (shown.hover)(ix, 0).map(|content| content.text)
    })
}

fn open_text(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    Entity<EditorView>,
    &mut VisualTestContext,
) {
    let (dir, workbench, cx) = opened(cx, &[("a.txt", TEXT)]);
    let tab = editor(&pane(&workbench, cx), cx);
    (dir, workbench, tab, cx)
}

#[gpui::test]
fn toggle_shows_authors_by_row_run(cx: &mut TestAppContext) {
    let (_dir, _workbench, tab, cx) = open_text(cx);
    edit(&tab, 4, "x", Origin::User, cx);
    edit(&tab, 9, "y", Origin::Format, cx);
    run(TOGGLE, cx);
    assert_eq!(texts(&tab, cx), ["You", "Format"]);
    assert_eq!(hover(&tab, 0, cx).as_deref(), Some("Written by you"));
    assert_eq!(entries(&tab, cx).unwrap()[0].1, None);
    // Click does nothing.
    let click = tab.read_with(cx, |tab, _| {
        tab.gutter_annotations().unwrap().on_click.clone()
    });
    cx.update(|window, cx| click(0, 1, window, cx));
    assert_eq!(texts(&tab, cx), ["You", "Format"]);
}

#[gpui::test]
fn an_accepted_ai_hunk_shows_the_agent(cx: &mut TestAppContext) {
    let (_dir, _workbench, tab, cx) = open_text(cx);
    edit(&tab, 4, "x", Origin::User, cx);
    edit(&tab, 10, "z", ai("claude"), cx);
    run(TOGGLE, cx);
    assert_eq!(
        entries(&tab, cx).unwrap(),
        [
            ("You".to_owned(), None),
            ("AI · claude".to_owned(), Some(1.0))
        ]
    );
    assert_eq!(hover(&tab, 1, cx).as_deref(), Some("Written by claude"));
}

#[gpui::test]
fn edits_update_the_column(cx: &mut TestAppContext) {
    let (_dir, _workbench, tab, cx) = open_text(cx);
    edit(&tab, 4, "x", Origin::User, cx);
    run(TOGGLE, cx);
    assert_eq!(texts(&tab, cx), ["You"]);
    edit(&tab, 10, "z", ai("claude"), cx);
    // Rebuilt once the edit has rested.
    assert_eq!(texts(&tab, cx), ["You"]);
    cx.executor().advance_clock(Duration::from_millis(400));
    draw(cx);
    assert_eq!(texts(&tab, cx), ["You", "AI · claude"]);
}

#[gpui::test]
fn toggle_twice_hides_it(cx: &mut TestAppContext) {
    let (_dir, _workbench, tab, cx) = open_text(cx);
    edit(&tab, 4, "x", Origin::User, cx);
    run(TOGGLE, cx);
    assert!(entries(&tab, cx).is_some());
    run(TOGGLE, cx);
    assert_eq!(entries(&tab, cx), None);
    // An edit doesn't bring it back.
    edit(&tab, 0, "q", Origin::User, cx);
    cx.executor().advance_clock(Duration::from_millis(400));
    draw(cx);
    assert_eq!(entries(&tab, cx), None);
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[gpui::test]
fn provenance_and_blame_share_the_column(cx: &mut TestAppContext) {
    let (dir, _workbench, tab, cx) = open_text(cx);
    cx.executor().allow_parking();
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-q", "-m", "first"]);
    edit(&tab, 4, "x", Origin::User, cx);

    run(TOGGLE, cx);
    assert_eq!(texts(&tab, cx), ["You"]);

    // Blame on: provenance goes, blame's rows land.
    run(BLAME, cx);
    for _ in 0..200 {
        if entries(&tab, cx).is_some_and(|shown| shown.iter().all(|(text, _)| text != "You")) {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
        draw(cx);
    }
    let blamed = texts(&tab, cx);
    assert!(
        !blamed.is_empty() && blamed.iter().all(|text| text.ends_with(" t")),
        "{blamed:?}"
    );

    // Provenance on: blame goes, and stays gone.
    run(TOGGLE, cx);
    assert_eq!(texts(&tab, cx), ["You"]);
    cx.executor().advance_clock(Duration::from_secs(2));
    draw(cx);
    assert_eq!(texts(&tab, cx), ["You"]);

    // Blame off by its own toggle remembers nothing of provenance: pressing
    // it again turns blame on, and provenance off.
    run(BLAME, cx);
    assert_ne!(entries(&tab, cx).map(|shown| shown.len()), Some(1));
    run(TOGGLE, cx);
    assert_eq!(texts(&tab, cx), ["You"]);
}
