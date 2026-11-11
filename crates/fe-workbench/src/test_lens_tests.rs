//! The Run Test lenses, in a headless workbench over a real folder whose
//! sessions are fake, so no process starts: `▷ Run test` in UI small sans
//! (T1550), read again when the editor's language changes (T1410 F1).

use std::{fs, io, path::Path, sync::Arc};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation, Source};
use fe_config::{Settings, SettingsEvent};
use fe_gpui::{EditorView, Lens, commands::RunCommand};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{Launch, Workbench, task_commands::TestFiles, task_defs::ProjectFiles, tests::open};

const LIB: &str = "\
pub fn add(a: u32, b: u32) -> u32 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds() {
        assert_eq!(add(1, 2), 3);
    }
}
";

struct Disk;

impl ProjectFiles for Disk {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        fs::read_to_string(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.is_file()
    }
}

struct Project<'a> {
    workbench: Entity<Workbench>,
    editor: Entity<EditorView>,
    cx: &'a mut VisualTestContext,
    _dir: tempfile::TempDir,
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// A workbench showing `text` as `name` in a package's folder, the root
/// trusted if `trusted`.
fn project<'a>(cx: &'a mut TestAppContext, name: &str, text: &str, trusted: bool) -> Project<'a> {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"p\"\n").unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    let path = dir.path().join(name);
    fs::write(&path, text).unwrap();
    let settings = fe_config::project_settings_path(dir.path());
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    // No server starts, the Python one either when an editor turns Python.
    let servers = "[lsp.servers.rust-analyzer]\nenabled = false\n\n\
                   [lsp.servers.pyright]\nenabled = false\n";
    fs::write(settings, servers).unwrap();
    cx.update(|cx| cx.set_global(TestFiles(Arc::new(Disk))));
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new(text).into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    if trusted {
        workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    }
    draw(cx);
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("an editor shows");
    Project {
        workbench,
        editor,
        cx,
        _dir: dir,
    }
}

impl Project<'_> {
    /// The lenses every provider of the editor answers for its text.
    fn lenses(&mut self) -> Vec<Lens> {
        draw(self.cx);
        self.editor.update(self.cx, |editor, _| {
            let snapshot = editor.model().snapshot().clone();
            (editor.lens_providers().iter())
                .flat_map(|provider| provider.borrow_mut().lenses(&snapshot))
                .collect()
        })
    }

    /// `(row, title)` of each lens, whose one item runs `tests.run_row`
    /// with its row.
    fn shown(&mut self) -> Vec<(u32, String)> {
        let lenses = self.lenses();
        lenses
            .into_iter()
            .map(|lens| {
                assert_eq!(lens.items.len(), 1);
                let item = &lens.items[0];
                assert_eq!(item.invocation.command.to_string(), "tests.run_row");
                assert_eq!(
                    item.invocation.args.get("row").and_then(|r| r.as_u64()),
                    Some(lens.row as u64)
                );
                (lens.row, item.title.to_string())
            })
            .collect()
    }
}

/// The providers every editor has without this one: the merge conflicts'.
const CONFLICTS: usize = 1;

fn providers(project: &mut Project) -> usize {
    project
        .editor
        .read_with(project.cx, |editor, _| editor.lens_providers().len())
}

fn row(row: u32, title: &str) -> (u32, String) {
    (row, title.to_owned())
}

#[gpui::test]
fn a_rust_test_gets_a_run_test_lens_on_its_attribute_row(cx: &mut TestAppContext) {
    let mut project = project(cx, "src/lib.rs", LIB, true);
    // The module's `#[cfg(test)]` and the test's `#[test]`.
    assert_eq!(
        project.shown(),
        [row(4, "▷ Run tests"), row(8, "▷ Run test")]
    );
}

#[gpui::test]
fn lens_follows_edits_by_version(cx: &mut TestAppContext) {
    let mut project = project(cx, "src/lib.rs", LIB, true);
    assert_eq!(project.shown()[1], row(8, "▷ Run test"));

    // A line above moves both lenses at once.
    project.editor.update(project.cx, |editor, cx| {
        editor.model_mut().set_cursor(0);
        editor.model_mut().insert("// hi\n");
        cx.notify();
    });
    assert_eq!(
        project.shown(),
        [row(5, "▷ Run tests"), row(9, "▷ Run test")]
    );

    // A new test below gets its own once the parse catches up.
    let end = project
        .editor
        .read_with(project.cx, |editor, _| editor.model().text().len());
    project.editor.update(project.cx, |editor, cx| {
        editor.model_mut().set_cursor(end);
        editor.model_mut().insert("\n#[test]\nfn more() {}\n");
        cx.notify();
    });
    let shown = project.shown();
    assert_eq!(shown.len(), 3, "{shown:?}");
    assert_eq!(shown[2], row(15, "▷ Run test"));
}

#[gpui::test]
fn code_lens_setting_off_adds_no_provider(cx: &mut TestAppContext) {
    let mut project = project(cx, "src/lib.rs", LIB, true);
    assert_eq!(project.shown().len(), 2);
    assert_eq!(providers(&mut project), CONFLICTS + 1);

    let set = |project: &mut Project, on: bool| {
        let mut settings = Settings::default();
        settings.editor.code_lens = on;
        let event = SettingsEvent {
            settings: Arc::new(settings),
            previous: Arc::new(Settings::default()),
            diagnostics: Vec::new(),
        };
        project.workbench.update(project.cx, |workbench, cx| {
            workbench.reload_settings(&event, cx)
        });
        draw(project.cx);
    };
    set(&mut project, false);
    assert_eq!(providers(&mut project), CONFLICTS);
    set(&mut project, true);
    assert_eq!(project.shown().len(), 2);
}

#[gpui::test]
fn no_lens_in_an_untrusted_folder(cx: &mut TestAppContext) {
    let mut project = project(cx, "src/lib.rs", LIB, false);
    assert_eq!(project.shown(), []);
    assert_eq!(providers(&mut project), CONFLICTS);
}

#[gpui::test]
fn no_lens_in_a_plain_text_file(cx: &mut TestAppContext) {
    let mut project = project(cx, "notes.txt", "#[test]\nfn adds() {}\n", true);
    assert_eq!(project.shown(), []);
    assert_eq!(providers(&mut project), CONFLICTS);
}

#[gpui::test]
fn clicking_the_lens_runs_that_test_not_the_one_at_the_caret(cx: &mut TestAppContext) {
    let two = LIB.replace(
        "    #[test]\n    fn adds",
        "    #[test]\n    fn first() {}\n\n    #[test]\n    fn adds",
    );
    let mut project = project(cx, "src/lib.rs", &two, true);
    let terminal = project
        .workbench
        .read_with(project.cx, |workbench, _| workbench.terminal().clone());
    terminal.update(project.cx, |terminal, _| terminal.fake_sessions = true);

    // The caret is in `first`; the click is on `adds`'s lens.
    let at = two.find("fn first").unwrap();
    project
        .editor
        .update(project.cx, |view, _| view.model_mut().set_cursor(at));
    let lens = project
        .lenses()
        .into_iter()
        .find(|lens| lens.row == 11)
        .expect("a lens over adds");
    let context = ContextStack::new()
        .with(ContextFrame::new("workbench"))
        .with(ContextFrame::new("editor"));
    let invocation: Invocation = lens.items[0].invocation.clone();
    let run = RunCommand(invocation.context(context).source(Source::Mouse));
    project
        .cx
        .update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(project.cx);

    let started = terminal.read_with(project.cx, |terminal, _| {
        (terminal.started_options.iter())
            .map(|options| {
                let (program, args) = options.shell.clone().expect("a task's command");
                [vec![program], args].concat().join(" ")
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(
        started,
        ["cargo test -p p tests::adds -- --exact --nocapture"]
    );
}

#[gpui::test]
fn the_test_lens_reads_run_test_in_ui_small_sans(cx: &mut TestAppContext) {
    let mut project = project(cx, "src/lib.rs", LIB, true);
    assert_eq!(
        project.shown(),
        [row(4, "▷ Run tests"), row(8, "▷ Run test")]
    );
    draw(project.cx);
    let (drawn, theme) = project.editor.read_with(project.cx, |editor, _| {
        let drawn = editor.drawn_annotations().expect("drawn").lenses.clone();
        (drawn, editor.theme().clone())
    });
    let texts: Vec<&str> = drawn.iter().map(|text| text.text.as_ref()).collect();
    assert_eq!(texts, ["▷ Run tests", "▷ Run test"]);
    let ui = project.cx.update(|_, cx| fe_gpui::ui_font_family(cx));
    for text in &drawn {
        // The design's UI small: sans, 500, 11 px at the default 13.
        assert_eq!(text.font.family, ui, "{text:?}");
        assert_eq!(text.font.weight, gpui::FontWeight::MEDIUM, "{text:?}");
        assert_eq!(text.font_size, theme.font_size - gpui::px(2.), "{text:?}");
        assert_eq!(text.color, theme.gutter_foreground, "{text:?}");
    }
}

/// The editor's language set to `name`, its parse caught up.
fn set_language(project: &mut Project, name: &str) {
    project.editor.update(project.cx, |editor, cx| {
        editor.set_language(Some(name), cx);
    });
    for _ in 0..3 {
        draw(project.cx);
    }
    let language = project.editor.read_with(project.cx, |editor, _| {
        editor.syntax().map(|syntax| syntax.language().name())
    });
    assert_eq!(language, Some(name));
}

#[gpui::test]
fn switching_the_language_without_an_edit_reads_the_lenses_again(cx: &mut TestAppContext) {
    // Rust's tests, and a Python test function under them.
    let text = format!("{LIB}def test_adds():\n    pass\n");
    let mut project = project(cx, "src/lib.rs", &text, true);
    assert_eq!(
        project.shown(),
        [row(4, "▷ Run tests"), row(8, "▷ Run test")]
    );

    // The same text as Python: Python's test, not Rust's.
    set_language(&mut project, "python");
    let python = project.shown();
    assert_eq!(python, [row(13, "▷ Run test")]);

    // And back, with no edit between: Rust's again.
    set_language(&mut project, "rust");
    assert_eq!(
        project.shown(),
        [row(4, "▷ Run tests"), row(8, "▷ Run test")]
    );

    // A language with no runnables shows none, and Rust's come back after.
    set_language(&mut project, "toml");
    assert_eq!(project.shown(), []);
    set_language(&mut project, "rust");
    assert_eq!(
        project.shown(),
        [row(4, "▷ Run tests"), row(8, "▷ Run test")]
    );
}
