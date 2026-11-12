//! A buffer no language server serves highlights its word's occurrences
//! in any locale (T329): fe-lsp's reason for it, resolved in the fake
//! locale `qps-ploc`, is matched by kind, not compared with its en-US
//! text.
//!
//! The catalogue's locale is the process's, so this is the only test in
//! its binary.

use std::{fs, time::Duration};

use fe_command::l10n::catalogue::{
    __private::inventory, LanguageIdentifier, Registered, ResourceFile, Resources,
};
use fe_editor::decorations::DecorationKind;
use fe_gpui::{EditorSettings, EditorTheme};
use fe_workbench::{Launch, Workbench};
use gpui::{Focusable as _, TestAppContext, VisualTestContext};

/// The fake locale's words for fe-lsp's reasons a document request isn't
/// served.
static FAKE_LOCALE: Resources = Resources {
    crate_name: "lsp-reasons-fake-locale",
    manifest_dir: env!("CARGO_MANIFEST_DIR"),
    declared_in: file!(),
    files: &[ResourceFile {
        locale: "qps-ploc",
        path: "qps-ploc/lsp.ftl",
        source: "\
lsp-unavailable-buffer-changed = ⟦büffer chängëd⟧
lsp-unavailable-too-large = ⟦töö lärgë⟧
lsp-unavailable-not-open = ⟦nöt öpen⟧
",
    }],
    dynamic_prefixes: &[],
};

inventory::submit! { Registered(&FAKE_LOCALE) }

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

#[gpui::test]
fn an_unserved_buffer_shows_its_words_in_a_fake_locale(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("notes.txt");
    fs::write(&path, "foo bar foo").unwrap();

    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
        let tag: LanguageIdentifier = "qps-ploc".parse().unwrap();
        kiln::platform::locale::set_locale(cx, &[tag]);
    });
    // Rooted, so it starts its language server manager; the root isn't
    // trusted, so no server runs and the buffer is open on none.
    let launch = Launch {
        root: Some(root),
        file: Some((
            path.clone(),
            Some(fe_workbench::loader::open(path).unwrap()),
        )),
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    draw(cx);
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("notes.txt is shown");
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);
    assert_eq!(
        editor.read_with(cx, |view, _| view.model().text().to_string()),
        "foo bar foo"
    );

    // The caret rests on the first `foo`.
    editor.update(cx, |view, cx| {
        view.model_mut().set_cursor(1);
        cx.notify();
    });
    draw(cx);
    cx.executor().advance_clock(Duration::from_millis(260));
    draw(cx);

    let highlights: Vec<_> = editor.read_with(cx, |view, _| {
        let model = view.model();
        (model.decorations(0..model.text().len()).into_iter())
            .filter(|item| item.kind == DecorationKind::Highlight)
            .map(|item| item.range)
            .collect()
    });
    assert_eq!(highlights.len(), 2, "{highlights:?}");
    assert_eq!(highlights, [0..3, 8..11]);
}
