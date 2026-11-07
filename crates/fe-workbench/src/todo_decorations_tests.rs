//! Keyword badges in a real workbench, on a paused clock.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use fe_config::{Settings, SettingsEvent};
use fe_editor::decorations::{Decoration, DecorationKind, Tint};
use fe_gpui::{EditorSettings, EditorView};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use tempfile::TempDir;

use crate::{Launch, Workbench, pane_group::Direction, spell_squiggles::REST};

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Long enough for whatever is to be scanned to be, retries included.
fn settle(cx: &mut VisualTestContext) {
    draw(cx);
    for _ in 0..6 {
        cx.executor().advance_clock(REST);
        cx.run_until_parked();
        draw(cx);
    }
}

fn workbench(cx: &mut TestAppContext) -> (Entity<Workbench>, &mut VisualTestContext) {
    let launch = Launch {
        root: None,
        file: None,
    };
    let (workbench, cx, _) = crate::tests::open(cx, launch);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    (workbench, cx)
}

fn write(dir: &TempDir, name: &str, text: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, text).unwrap();
    path
}

fn open(
    workbench: &Entity<Workbench>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = path.to_owned();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("the file is shown")
}

/// `editor`'s keyword badges.
fn badges(
    workbench: &Entity<Workbench>,
    editor: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> Vec<Decoration> {
    workbench.read_with(cx, |workbench, cx| workbench.todo_decorations(editor, cx))
}

fn badged(
    workbench: &Entity<Workbench>,
    editor: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> Vec<String> {
    let found = badges(workbench, editor, cx);
    editor.read_with(cx, |view, _| {
        let text = view.model().text().to_string();
        found
            .iter()
            .map(|it| text[it.range.clone()].to_owned())
            .collect()
    })
}

#[gpui::test]
fn a_todo_comment_gets_a_decoration_after_the_rest_delay(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = write(
        &dir,
        "main.rs",
        "// TODO: later\nfn main() { let s = \"FIXME: no\"; }\n// FIXME(me): x\n",
    );
    let (workbench, cx) = workbench(cx);
    let editor = open(&workbench, &path, cx);
    draw(cx);
    assert_eq!(
        badges(&workbench, &editor, cx),
        [],
        "not before it has rested"
    );

    settle(cx);
    let found = badges(&workbench, &editor, cx);
    assert_eq!(badged(&workbench, &editor, cx), ["TODO:", "FIXME(me):"]);
    assert_eq!(found[0].kind, DecorationKind::Highlight);
    assert_eq!(found[0].tint, Tint::Information);
    assert_eq!(found[1].tint, Tint::Error);
    // They reach the editor's own decorations.
    let drawn = editor.read_with(cx, |view, _| {
        let model = view.model();
        model.decorations(0..model.text().len()).len()
    });
    assert_eq!(drawn, 2);
}

#[gpui::test]
fn setting_off_adds_no_provider(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, "main.rs", "// TODO: later\n");
    let (workbench, cx) = workbench(cx);
    let editor = open(&workbench, &path, cx);
    settle(cx);
    assert_eq!(badged(&workbench, &editor, cx), ["TODO:"]);

    reload(&workbench, false, cx);
    draw(cx);
    assert_eq!(workbench.read_with(cx, |w, _| w.todos.slot_count()), 0);
    assert_eq!(badges(&workbench, &editor, cx), []);
    let drawn = editor.read_with(cx, |view, _| {
        let model = view.model();
        model.decorations(0..model.text().len()).len()
    });
    assert_eq!(drawn, 0, "the provider is gone");

    settle(cx);
    assert_eq!(workbench.read_with(cx, |w, _| w.todos.slot_count()), 0);

    reload(&workbench, true, cx);
    settle(cx);
    assert_eq!(badged(&workbench, &editor, cx), ["TODO:"]);
}

#[gpui::test]
fn editing_the_comment_moves_the_decoration(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, "main.rs", "// TODO: later\n");
    let (workbench, cx) = workbench(cx);
    let editor = open(&workbench, &path, cx);
    settle(cx);
    assert_eq!(badges(&workbench, &editor, cx)[0].range, 3..8);

    // Text ahead of it: it follows, before the next scan.
    editor.update(cx, |view, cx| {
        view.model_mut().select_range(3..3, false);
        view.model_mut().insert("so ");
        cx.notify();
    });
    draw(cx);
    assert_eq!(badges(&workbench, &editor, cx)[0].range, 6..11);

    // Its colon removed: the next scan takes it away.
    editor.update(cx, |view, cx| {
        let colon = view.model().text().to_string().find(':').unwrap();
        view.model_mut().select_range(colon..colon + 1, false);
        view.model_mut().insert("");
        cx.notify();
    });
    settle(cx);
    assert_eq!(badges(&workbench, &editor, cx), []);
}

fn reload(workbench: &Entity<Workbench>, on: bool, cx: &mut VisualTestContext) {
    let event = SettingsEvent {
        settings: Arc::new(Settings {
            editor: EditorSettings {
                todo_comments: on,
                ..EditorSettings::default()
            },
            ..Settings::default()
        }),
        previous: Arc::new(Settings::default()),
        diagnostics: Vec::new(),
    };
    workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
    cx.run_until_parked();
}

fn drawn(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> usize {
    editor.read_with(cx, |view, _| {
        let model = view.model();
        model.decorations(0..model.text().len()).len()
    })
}

#[gpui::test]
fn what_a_split_copied_goes_with_the_setting_off(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, "a.rs", "// TODO: later\n");
    let (workbench, cx) = workbench(cx);
    open(&workbench, &path, cx);
    settle(cx);
    // A split, with no draw between it and the setting going off.
    workbench.update_in(cx, |workbench, window, cx| {
        let new = (workbench.workspace().clone()).update(cx, |workspace, cx| {
            workspace.split(Direction::Right, window, cx)
        });
        workbench.activate(new, window, cx);
    });
    let copy = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("the split shows it");
    reload(&workbench, false, cx);
    settle(cx);
    assert_eq!(drawn(&copy, cx), 0);
    reload(&workbench, true, cx);
    settle(cx);
    assert_eq!(drawn(&copy, cx), 1, "just the one, once it is back on");
}

#[gpui::test]
fn every_kind_is_a_tinted_background_on_its_tag(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let source =
        "// TODO: a\n// FIXME: b\n// HACK: c\n// WARN: d\n// PERF: e\n// NOTE: f\n// TEST: g\n";
    let path = write(&dir, "main.rs", source);
    let (workbench, cx) = workbench(cx);
    let editor = open(&workbench, &path, cx);
    settle(cx);
    let found = badges(&workbench, &editor, cx);
    let tags: Vec<_> = found.iter().map(|it| &source[it.range.clone()]).collect();
    assert_eq!(
        tags,
        [
            "TODO:", "FIXME:", "HACK:", "WARN:", "PERF:", "NOTE:", "TEST:"
        ]
    );
    let tints: Vec<_> = found.iter().map(|it| it.tint).collect();
    assert_eq!(
        tints,
        [
            Tint::Information,
            Tint::Error,
            Tint::Warning,
            Tint::Warning,
            Tint::Accent,
            Tint::Hint,
            Tint::Hint,
        ]
    );
    assert!(found.iter().all(|it| it.kind == DecorationKind::Highlight));
}

#[gpui::test]
fn an_edit_after_the_retries_run_out_leaves_no_stale_badge(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, "main.rs", "// TODO: later\n");
    let (workbench, cx) = workbench(cx);
    let editor = open(&workbench, &path, cx);
    settle(cx);
    assert_eq!(badged(&workbench, &editor, cx), ["TODO:"]);

    // Highlighting never finishes from here on.
    workbench.update(cx, |w, _| w.todos.incomplete = true);
    editor.update(cx, |view, cx| {
        view.model_mut().select_range(3..3, false);
        view.model_mut().insert("so ");
        cx.notify();
    });
    draw(cx);
    assert_eq!(
        badges(&workbench, &editor, cx).len(),
        1,
        "followed the edit"
    );
    for _ in 0..3 {
        settle(cx);
    }
    assert_eq!(badges(&workbench, &editor, cx), [], "no stale badge");

    // Not tried again on its own, and the next edit starts over.
    workbench.update(cx, |w, _| w.todos.incomplete = false);
    settle(cx);
    assert_eq!(badges(&workbench, &editor, cx), []);
    editor.update(cx, |view, cx| {
        view.model_mut().select_range(0..0, false);
        view.model_mut().insert(" ");
        cx.notify();
    });
    settle(cx);
    assert_eq!(badged(&workbench, &editor, cx), ["TODO:"]);
}
