//! The project tree's text kept on screen follows a switch to a fake
//! locale, `qps-ploc`, and back, while it shows (T1541): the message in
//! place of the rows of a folder that couldn't be opened, Expand All's word
//! under the rows, and the name field's placeholder and error and the
//! filter box's placeholder, each stored as a key and resolved as drawn.
//!
//! The catalogue's locale is the process's, so this is the only test in
//! its binary.

use std::{fs, path::Path};

use fe_command::l10n::catalogue::{
    __private::inventory, LanguageIdentifier, Registered, ResourceFile, Resources,
};
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use fe_workbench::{Launch, ProjectPanel, Workbench};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

/// The fake locale's words for the messages this test shows, each naming
/// its arguments.
static FAKE_LOCALE: Resources = Resources {
    crate_name: "project-text-fake-locale",
    manifest_dir: env!("CARGO_MANIFEST_DIR"),
    declared_in: file!(),
    files: &[ResourceFile {
        locale: "qps-ploc",
        path: "qps-ploc/project.ftl",
        source: "\
workbench-project-cannot-open = ⟦cännöt öpen⟧ { $path }
workbench-project-expand-all-capped = ⟦cäpped { $count }⟧
workbench-project-file-name = ⟦file nämë⟧
workbench-project-name-taken = ⟦täken { $name }⟧
workbench-project-filter-placeholder = ⟦filtër⟧
",
    }],
    dynamic_prefixes: &[],
};

inventory::submit! { Registered(&FAKE_LOCALE) }

fn set_locale(tag: &str, cx: &mut VisualTestContext) {
    let tag: LanguageIdentifier = tag.parse().unwrap();
    cx.update(|_, cx| kiln::platform::locale::set_locale(cx, &[tag]));
    draw(cx);
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// A workbench on `root`, its project tree focused.
fn open<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (Entity<ProjectPanel>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
    });
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    cx.update(|window, cx| window.focus(&panel.focus_handle(cx), cx));
    draw(cx);
    (panel, cx)
}

/// Runs the project tree's command `id`, as its key would.
fn run(id: &str, cx: &mut VisualTestContext) {
    let mut context = ContextStack::new()
        .with(ContextFrame::new("workbench"))
        .with(ContextFrame::new("project_panel"));
    if id.starts_with("text_input.") {
        context = context.with(ContextFrame::new("text_input"));
    }
    let run = RunCommand(Invocation::of(id).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// The words the tree shows now, by what shows them.
fn words(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Vec<(&'static str, String)> {
    panel.read_with(cx, |panel, cx| panel.shown_words(cx))
}

#[gpui::test]
fn the_trees_text_follows_the_locale_while_it_shows(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    // More folders than Expand All opens, and a file to name again.
    for ix in 0..301 {
        fs::create_dir_all(root.join(format!("d{ix:03}"))).unwrap();
    }
    fs::write(root.join("taken.rs"), "").unwrap();
    let missing = dir.path().join("missing");

    let (panel, cx) = open(cx, &root);
    run("project_panel.expand_all", cx);
    run("project_panel.new_file", cx);
    cx.simulate_input("taken.rs");
    cx.simulate_keystrokes("enter");
    draw(cx);
    let english = [
        (
            "status",
            "Expanded the first 300 folders: Expand All stops there".to_owned(),
        ),
        ("name-placeholder", "File name".to_owned()),
        (
            "name-error",
            "A file or folder “taken.rs” already exists here. Choose another name.".to_owned(),
        ),
    ];
    assert_eq!(words(&panel, cx), english);

    // In the fake locale, as they stay on screen.
    set_locale("qps-ploc", cx);
    assert_eq!(
        words(&panel, cx),
        [
            ("status", "⟦cäpped 300⟧".to_owned()),
            ("name-placeholder", "⟦file nämë⟧".to_owned()),
            ("name-error", "⟦täken taken.rs⟧".to_owned()),
        ]
    );
    set_locale("en-US", cx);
    assert_eq!(words(&panel, cx), english);

    // The filter's box, opened in English, then in the fake locale.
    cx.simulate_keystrokes("escape");
    draw(cx);
    run("project_panel.filter", cx);
    let filter = |words: Vec<(&'static str, String)>| {
        (words.into_iter()).find(|(what, _)| *what == "filter-placeholder")
    };
    assert_eq!(
        filter(words(&panel, cx)),
        Some(("filter-placeholder", "Filter by name".to_owned()))
    );
    set_locale("qps-ploc", cx);
    assert_eq!(
        filter(words(&panel, cx)),
        Some(("filter-placeholder", "⟦filtër⟧".to_owned()))
    );
    set_locale("en-US", cx);

    // A folder that couldn't be opened: its message.
    let (panel, cx) = open(cx, &missing);
    let message = words(&panel, cx);
    let shown = &message.first().expect("a message").1;
    assert!(shown.starts_with("Cannot open "), "{shown}");
    set_locale("qps-ploc", cx);
    let message = words(&panel, cx);
    let shown = &message.first().expect("a message").1;
    assert!(shown.starts_with("⟦cännöt öpen⟧ "), "{shown}");
    set_locale("en-US", cx);
}
