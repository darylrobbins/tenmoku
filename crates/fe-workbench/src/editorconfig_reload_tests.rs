//! Open editors follow `.editorconfig` changes: a save of one in the
//! editor, and changes on disk the project watcher reports.

use std::path::{Path, PathBuf};

use fe_buffer::{BufferEvent, LineEnding};
use fe_editor::IndentStyle;
use fe_gpui::{DiffView, EditorSettings, EditorView, SaveFixes};
use fe_project::FsChange;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{
    Workbench,
    tests::{
        find_bar::{draw, run},
        open_root,
    },
};

const SPACED: &str = "a {\n    b\n}\n";

fn open(
    workbench: &Entity<Workbench>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    let pane = workbench.read_with(cx, |w, _| w.pane().clone());
    let path = path.to_owned();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    workbench.read_with(cx, |w, cx| w.active_editor(cx).cloned().expect("a tab"))
}

/// Indent style and tab size.
fn indent(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> (Option<IndentStyle>, usize) {
    editor.read_with(cx, |e, _| (e.model().indent_style(), e.model().tab_size()))
}

fn line_ending(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> LineEnding {
    editor.read_with(cx, |e, _| e.model().buffer().line_ending())
}

fn fixes(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> SaveFixes {
    editor.read_with(cx, |e, _| e.save_fixes())
}

fn changed(changes: Vec<FsChange>, cx: &mut VisualTestContext) {
    cx.update(|_, cx| crate::pane::editorconfigs_changed(&changes, cx));
    draw(cx);
}

fn spaces(n: u8) -> Option<IndentStyle> {
    Some(IndentStyle::Spaces(n))
}

#[gpui::test]
fn saving_an_editorconfig_in_the_editor_reapplies(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let config = dir.path().join(".editorconfig");
    std::fs::write(&config, "root = true\n").unwrap();
    let a = dir.path().join("a.go");
    let c = other.path().join("c.go");
    std::fs::write(&a, SPACED.repeat(5)).unwrap();
    std::fs::write(&c, SPACED.repeat(5)).unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    let default_tab = EditorSettings::default().tab_size;

    let outside = open(&workbench, &c, cx);
    let go = open(&workbench, &a, cx);
    assert_eq!(indent(&go, cx), (spaces(4), default_tab));
    let ec = open(&workbench, &config, cx);

    cx.update(|window, cx| window.focus(&ec.focus_handle(cx), cx));
    ec.update(cx, |e, _| {
        e.model_mut().select_all();
        e.model_mut()
            .insert("root = true\n[*.go]\nindent_style = tab\ntab_width = 8\n");
    });
    cx.simulate_keystrokes("cmd-s");
    draw(cx);
    assert_eq!(
        std::fs::read_to_string(&config).unwrap(),
        "root = true\n[*.go]\nindent_style = tab\ntab_width = 8\n"
    );
    assert_eq!(indent(&go, cx), (Some(IndentStyle::Tabs), 8));
    assert_eq!(indent(&outside, cx), (spaces(4), default_tab));

    // Set by hand since, and kept over a save that changes nothing.
    go.update(cx, |e, _| e.model_mut().set_indent_style(spaces(3)));
    let saves = ec.update(cx, |e, _| e.model_mut().buffer_mut().subscribe());
    let saved = cx.update(|window, cx| ec.update(cx, |e, cx| e.save(window, cx)));
    draw(cx);
    drop(saved);
    assert!(
        (saves.drain().iter()).any(|event| matches!(event, BufferEvent::Saved { .. })),
        "the .editorconfig was saved again"
    );
    assert_eq!(indent(&go, cx), (spaces(3), 8));
}

#[gpui::test]
fn disk_changes_reapply(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join(".editorconfig");
    let b = dir.path().join("b.js");
    std::fs::write(&b, SPACED.repeat(5)).unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    let default_tab = EditorSettings::default().tab_size;
    let js = open(&workbench, &b, cx);
    // A diff view's working side follows too.
    run("diff.compare_saved", cx);
    let diff = workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        (pane.active_item())
            .and_then(|item| item.downcast_ref::<DiffView>().cloned())
            .expect("a diff view is shown")
    });
    let working = diff.read_with(cx, |diff, _| diff.right().clone());
    assert_ne!(working, js);
    assert_eq!(indent(&js, cx), (spaces(4), default_tab));
    assert_eq!(line_ending(&js, cx), LineEnding::Lf);
    let created = || vec![FsChange::Path(dir.path().join(".editorconfig"))];

    std::fs::write(
        &config,
        "[*.js]\nindent_size = 2\nend_of_line = crlf\ntrim_trailing_whitespace = true\n",
    )
    .unwrap();
    changed(created(), cx);
    assert_eq!(indent(&js, cx), (spaces(2), 2));
    assert_eq!(indent(&working, cx), (spaces(2), 2));
    assert_eq!(line_ending(&js, cx), LineEnding::CrLf);
    let trim = SaveFixes {
        trim_trailing_whitespace: true,
        insert_final_newline: false,
    };
    assert_eq!(fixes(&js, cx), trim);

    std::fs::write(
        &config,
        "[*.js]\nindent_size = 3\nend_of_line = crlf\ntrim_trailing_whitespace = true\ninsert_final_newline = true\n",
    )
    .unwrap();
    changed(created(), cx);
    assert_eq!(indent(&js, cx), (spaces(3), 3));
    assert_eq!(indent(&working, cx), (spaces(3), 3));
    assert_eq!(
        fixes(&js, cx),
        SaveFixes {
            trim_trailing_whitespace: true,
            insert_final_newline: true,
        }
    );

    std::fs::remove_file(&config).unwrap();
    changed(created(), cx);
    assert_eq!(indent(&js, cx), (spaces(4), default_tab));
    assert_eq!(indent(&working, cx), (spaces(4), default_tab));
    assert_eq!(fixes(&js, cx), SaveFixes::default());
    assert_eq!(line_ending(&js, cx), LineEnding::CrLf);

    // A rescan reads it again.
    std::fs::write(&config, "[*.js]\nindent_size = 2\n").unwrap();
    changed(vec![FsChange::Rescan], cx);
    assert_eq!(indent(&js, cx), (spaces(2), 2));

    // Other files are not .editorconfigs.
    std::fs::write(&config, "[*.js]\nindent_size = 5\n").unwrap();
    let others: Vec<PathBuf> = ["x.editorconfig", ".editorconfig.bak", "b.js"]
        .iter()
        .map(|name| dir.path().join(name))
        .collect();
    changed(others.into_iter().map(FsChange::Path).collect(), cx);
    assert_eq!(indent(&js, cx), (spaces(2), 2));
}

#[gpui::test]
fn saving_an_editorconfig_from_a_diff_reapplies(cx: &mut TestAppContext) {
    // Outside the root, so no watcher sees it.
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let config = other.path().join(".editorconfig");
    std::fs::write(&config, "root = true\n").unwrap();
    let c = other.path().join("c.go");
    std::fs::write(&c, SPACED.repeat(5)).unwrap();
    let (workbench, cx) = open_root(cx, root.path());
    let go = open(&workbench, &c, cx);
    let tab = open(&workbench, &config, cx);
    tab.update(cx, |e, _| e.model_mut().insert("# edited\n"));
    run("diff.compare_saved", cx);
    let diff = workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        (pane.active_item())
            .and_then(|item| item.downcast_ref::<DiffView>().cloned())
            .expect("a diff view is shown")
    });
    let working = diff.read_with(cx, |diff, _| diff.right().clone());
    // Only the diff is left over the .editorconfig.
    let pane = workbench.read_with(cx, |w, _| w.pane().clone());
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            let picked = |item: &dyn kiln_shell::ItemHandle, _: &gpui::App| {
                item.downcast_ref::<EditorView>() == Some(&tab)
            };
            pane.close_matching(true, picked, window, cx)
        })
    });
    draw(cx);
    let tabs = workbench.read_with(cx, |w, cx| w.open_editors(cx));
    assert!(!tabs.contains(&tab), "the tab closed");
    assert!(working.read_with(cx, |e, _| e.model().path() == Some(config.as_path())));

    working.update(cx, |e, _| {
        e.model_mut().select_all();
        e.model_mut()
            .insert("root = true\n[*.go]\nindent_style = tab\n");
    });
    let saved = cx.update(|window, cx| working.update(cx, |e, cx| e.save(window, cx)));
    draw(cx);
    drop(saved);
    assert_eq!(
        std::fs::read_to_string(&config).unwrap(),
        "root = true\n[*.go]\nindent_style = tab\n"
    );
    assert_eq!(indent(&go, cx).0, Some(IndentStyle::Tabs));
}

#[cfg(unix)]
#[gpui::test]
fn an_editor_through_a_link_follows_the_real_path(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real");
    let link = dir.path().join("link");
    std::fs::create_dir(&real).unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();
    std::fs::write(real.join("b.js"), SPACED.repeat(5)).unwrap();
    std::fs::write(real.join("c.js"), SPACED.repeat(5)).unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    let linked = open(&workbench, &link.join("b.js"), cx);
    let direct = open(&workbench, &real.join("c.js"), cx);
    assert!(linked.read_with(cx, |e, _| e.model().path()
        == Some(link.join("b.js").as_path())));

    // Reported by the real path, and by the link's.
    std::fs::write(real.join(".editorconfig"), "[*.js]\nindent_size = 2\n").unwrap();
    changed(vec![FsChange::Path(real.join(".editorconfig"))], cx);
    assert_eq!(indent(&linked, cx), (spaces(2), 2));
    std::fs::write(real.join(".editorconfig"), "[*.js]\nindent_size = 3\n").unwrap();
    changed(vec![FsChange::Path(link.join(".editorconfig"))], cx);
    assert_eq!(indent(&direct, cx), (spaces(3), 3));
    assert_eq!(indent(&linked, cx), (spaces(3), 3));
}

fn reload_tab_size(workbench: &Entity<Workbench>, size: usize, cx: &mut VisualTestContext) {
    let mut settings = fe_config::Settings::default();
    settings.editor.tab_size = size;
    let event = fe_config::SettingsEvent {
        settings: std::sync::Arc::new(settings),
        previous: std::sync::Arc::new(fe_config::Settings::default()),
        diagnostics: Vec::new(),
    };
    workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
}

#[gpui::test]
fn the_tab_size_falls_back_to_the_settings_as_they_are_when_read(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join(".editorconfig");
    std::fs::write(&config, "[*.js]\nindent_size = 2\n").unwrap();
    let b = dir.path().join("b.js");
    std::fs::write(&b, SPACED.repeat(5)).unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    let js = open(&workbench, &b, cx);
    assert_eq!(indent(&js, cx), (spaces(2), 2));

    // Removed; the settings change while the read runs.
    std::fs::remove_file(&config).unwrap();
    cx.update(|_, cx| crate::pane::editorconfigs_changed(&[FsChange::Path(config.clone())], cx));
    reload_tab_size(&workbench, 8, cx);
    draw(cx);
    assert_eq!(indent(&js, cx), (spaces(4), 8));
}

#[gpui::test]
fn a_diff_saved_as_an_editorconfig_reapplies(cx: &mut TestAppContext) {
    // Outside the root, so no watcher sees it.
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let x = other.path().join("x.txt");
    std::fs::write(&x, "x\n").unwrap();
    let c = other.path().join("c.go");
    std::fs::write(&c, SPACED.repeat(5)).unwrap();
    let (workbench, cx) = open_root(cx, root.path());
    let go = open(&workbench, &c, cx);
    let tab = open(&workbench, &x, cx);
    tab.update(cx, |e, _| e.model_mut().insert("# edited\n"));
    run("diff.compare_saved", cx);
    let diff = workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        (pane.active_item())
            .and_then(|item| item.downcast_ref::<DiffView>().cloned())
            .expect("a diff view is shown")
    });
    let working = diff.read_with(cx, |diff, _| diff.right().clone());
    let pane = workbench.read_with(cx, |w, _| w.pane().clone());
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            let picked = |item: &dyn kiln_shell::ItemHandle, _: &gpui::App| {
                item.downcast_ref::<EditorView>() == Some(&tab)
            };
            pane.close_matching(true, picked, window, cx)
        })
    });
    draw(cx);
    assert!(
        !workbench
            .read_with(cx, |w, cx| w.open_editors(cx))
            .contains(&tab)
    );

    // Saved as the folder's .editorconfig, as vim's :sav does.
    working.update(cx, |e, _| {
        e.model_mut().select_all();
        e.model_mut()
            .insert("root = true\n[*.go]\nindent_style = tab\n");
    });
    let config = other.path().join(".editorconfig");
    let to = config.clone();
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.save_editor_as_with(&working, to, false, window, cx)
    });
    draw(cx);
    assert!(working.read_with(cx, |e, _| e.model().path() == Some(config.as_path())));
    assert_eq!(indent(&go, cx).0, Some(IndentStyle::Tabs));

    // And heard for its saves under the new name.
    working.update(cx, |e, _| {
        e.model_mut().select_all();
        e.model_mut()
            .insert("root = true\n[*.go]\nindent_style = tab\ntab_width = 8\n");
    });
    let saved = cx.update(|window, cx| working.update(cx, |e, cx| e.save(window, cx)));
    draw(cx);
    drop(saved);
    assert_eq!(indent(&go, cx), (Some(IndentStyle::Tabs), 8));
}

#[gpui::test]
fn a_removed_editorconfig_detects_indentation_from_the_shared_text(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join(".editorconfig");
    std::fs::write(&config, "[*.js]\nindent_size = 2\n").unwrap();
    let b = dir.path().join("b.js");
    std::fs::write(&b, SPACED.repeat(5)).unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    let js = open(&workbench, &b, cx);
    run("diff.compare_saved", cx);
    let diff = workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        (pane.active_item())
            .and_then(|item| item.downcast_ref::<DiffView>().cloned())
            .expect("a diff view is shown")
    });
    let working = diff.read_with(cx, |diff, _| diff.right().clone());
    assert_eq!(indent(&js, cx), (spaces(2), 2));

    // Tab-indented through the diff, while the file's tab isn't shown.
    working.update(cx, |e, cx| {
        e.model_mut().select_all();
        e.model_mut().insert(&"a {\n\tb\n}\n".repeat(5));
        cx.notify();
    });
    draw(cx);
    std::fs::remove_file(&config).unwrap();
    changed(vec![FsChange::Path(config.clone())], cx);
    let default_tab = EditorSettings::default().tab_size;
    assert_eq!(indent(&js, cx), (Some(IndentStyle::Tabs), default_tab));
    assert_eq!(indent(&working, cx), (Some(IndentStyle::Tabs), default_tab));
}
