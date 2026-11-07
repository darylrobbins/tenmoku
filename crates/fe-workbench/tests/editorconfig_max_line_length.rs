//! `.editorconfig` `max_line_length` is the file's wrap column where
//! `editor.wrap_mode` is `column` or `bounded`, and draws a ruler.

use std::{
    path::Path,
    sync::{Mutex, MutexGuard},
};

use fe_config::settings::WrapMode;
use fe_gpui::{EditorSettings, EditorTheme, EditorView};
use fe_workbench::{Launch, Workbench};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

static SERIAL: Mutex<()> = Mutex::new(());

/// Runs the rest of a test with no other test of this binary running, and
/// with nothing naming a config directory (see `hot_exit_rootless.rs`).
fn serially() -> MutexGuard<'static, ()> {
    let guard = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for var in ["TENMOKU_CONFIG_DIR", "XDG_CONFIG_HOME", "HOME", "APPDATA"] {
        // SAFETY: every test takes the lock before it starts gpui, so no
        // other thread is running to read the environment.
        unsafe { std::env::remove_var(var) };
    }
    guard
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// A project with `editorconfig` (if any) and `a.txt` of one 200-column
/// line, opened in a wide window with `mode` and `column` as settings.
fn open(
    editorconfig: Option<&str>,
    mode: WrapMode,
    column: u32,
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, Entity<Workbench>, VisualTestContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
    });
    let dir = tempfile::tempdir().unwrap();
    if let Some(text) = editorconfig {
        std::fs::write(dir.path().join(".editorconfig"), text).unwrap();
    }
    let file = dir.path().join("a.txt");
    std::fs::write(&file, "x".repeat(200)).unwrap();
    let launch = Launch {
        root: None,
        file: None,
    };
    let settings = EditorSettings {
        soft_wrap: true,
        wrap_mode: mode,
        wrap_column: column,
        font_size: 8.,
        ..EditorSettings::default()
    };
    let window = cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, settings, window, cx)
    });
    let workbench = window.root(cx).unwrap();
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    vcx.simulate_resize(gpui::size(gpui::px(3000.), gpui::px(600.)));
    draw(&mut vcx);
    open_file(&workbench, &file, &mut vcx);
    (dir, workbench, vcx)
}

fn open_file(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |w, _| w.pane().clone());
    let path = path.to_owned();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    draw(cx);
}

fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench.read_with(cx, |w, cx| w.active_editor(cx).cloned().expect("a tab"))
}

fn em(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> f32 {
    cx.update(|window, cx| {
        let view = editor.read(cx);
        let text_system = window.text_system();
        let font_id = text_system.resolve_font(&gpui::font(view.theme().font_family.clone()));
        text_system
            .em_advance(font_id, view.theme().font_size)
            .unwrap()
            .into()
    })
}

/// The wrap width in columns, if it wraps at all, and the row count.
fn wrap(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> (Option<f32>, u32) {
    let em = em(editor, cx);
    editor.update(cx, |editor, _| {
        let display = editor.model().display();
        (display.wrap_width().map(|w| w / em), display.row_count())
    })
}

#[gpui::test]
fn max_line_length_sets_the_wrap_column_in_column_mode(app: &mut TestAppContext) {
    let _guard = serially();
    let (_dir, workbench, mut cx) = open(
        Some("[*]\nmax_line_length = 60\n"),
        WrapMode::Column,
        0,
        app,
    );
    let editor = active(&workbench, &mut cx);
    assert_eq!(wrap(&editor, &mut cx), (Some(60.), 4));
    drop((_dir, workbench));

    let (_dir, workbench, mut cx) = open(
        Some("[*]\nmax_line_length = 60\n"),
        WrapMode::Viewport,
        0,
        app,
    );
    let editor = active(&workbench, &mut cx);
    assert_eq!(wrap(&editor, &mut cx).1, 1);
}

#[gpui::test]
fn max_line_length_off_or_unset_keeps_the_setting(app: &mut TestAppContext) {
    let _guard = serially();
    for config in [Some("[*]\nmax_line_length = off\n"), None] {
        let (_dir, workbench, mut cx) = open(config, WrapMode::Column, 40, app);
        let editor = active(&workbench, &mut cx);
        assert_eq!(wrap(&editor, &mut cx).0, Some(40.), "{config:?}");
    }
}

#[gpui::test]
fn editing_the_editorconfig_updates_and_clears_the_wrap_column(app: &mut TestAppContext) {
    let _guard = serially();
    let (dir, workbench, mut cx) = open(
        Some("[*]\nmax_line_length = 60\n"),
        WrapMode::Column,
        0,
        app,
    );
    let editor = active(&workbench, &mut cx);
    assert_eq!(wrap(&editor, &mut cx).0, Some(60.));

    // Saved in the editor, as the editor's own tests do: the editors under
    // it follow.
    let config = dir.path().join(".editorconfig");
    for (text, expected) in [("[*]\nmax_line_length = 80\n", 3), ("[*]\n", 1)] {
        open_file(&workbench, &config, &mut cx);
        let ec = active(&workbench, &mut cx);
        cx.update(|window, cx| window.focus(&ec.focus_handle(cx), cx));
        ec.update(&mut cx, |e, _| {
            e.model_mut().select_all();
            e.model_mut().insert(text);
        });
        cx.simulate_keystrokes("cmd-s");
        draw(&mut cx);
        draw(&mut cx);
        assert_eq!(std::fs::read_to_string(&config).unwrap(), text);
        // Shown again, as the tab the file is in.
        let workbench = workbench.clone();
        let a = dir.path().join("a.txt");
        open_file(&workbench, &a, &mut cx);
        let editor = active(&workbench, &mut cx);
        assert_eq!(wrap(&editor, &mut cx).1, expected, "{text:?}");
    }
}
