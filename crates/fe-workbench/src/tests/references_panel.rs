//! Find references (shift-F12) in the References panel, over a fake
//! provider whose batches the test releases one at a time, and over the
//! search provider end to end.

use std::{
    collections::HashMap,
    fs,
    ops::Range,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use fe_buffer::Buffer;
use fe_nav::{
    Definitions, FileReferences, Location, NavError, NavRequest, NavigationProvider, Precision,
    Readiness, Reference, ReferencesSummary, WorkspaceSymbol,
};
use fe_ui::Icon;
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use super::{open, open_root};
use crate::{
    DockPanel, Launch, ReferencesPanel, Workbench,
    references_panel::ShownRow::{self, File},
};

/// A project of `files`, and its root, resolved as the watcher reports it.
pub(super) fn project(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    for (rel, text) in files {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    (dir, root)
}

const MAIN: &str = "mod util;\n\nfn main() {\n    util::helper();\n    other();\n}\n";

/// A workbench rooted at `root`, focused, showing `file`.
pub(super) fn opened<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
    file: &str,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let (workbench, cx) = open_root(cx, root);
    let path = root.join(file);
    cx.update(|window, cx| {
        let pane = workbench.read(cx).pane().clone();
        pane.update(cx, |pane, cx| pane.open(path, window, cx));
    });
    settle(cx);
    focus_editor(&workbench, cx);
    (workbench, cx)
}

pub(super) fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

pub(super) fn focus_editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
}

/// Puts the caret inside the first `word` of the active editor, returning
/// its offset.
pub(super) fn caret_on(
    workbench: &Entity<Workbench>,
    word: &str,
    cx: &mut VisualTestContext,
) -> usize {
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a file is shown");
    editor.update(cx, |editor, cx| {
        let offset = editor.model().text().to_string().find(word).unwrap() + 1;
        editor.model_mut().set_cursor(offset);
        cx.notify();
        offset
    })
}

/// The active editor's file, its selection and whether it's focused.
pub(super) fn at(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (PathBuf, Range<usize>, bool) {
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a file is shown");
    let focused = cx.update(|window, cx| editor.focus_handle(cx).is_focused(window));
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        (
            model.path().unwrap().to_owned(),
            model.selections()[0].range.clone(),
            focused,
        )
    })
}

pub(super) fn references(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Entity<ReferencesPanel> {
    workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).references().clone()
    })
}

pub(super) fn shown(panel: &Entity<ReferencesPanel>, cx: &mut VisualTestContext) -> Vec<ShownRow> {
    panel.read_with(cx, |panel, _| panel.shown_rows())
}

fn status(panel: &Entity<ReferencesPanel>, cx: &mut VisualTestContext) -> Option<String> {
    panel.read_with(cx, |panel, _| panel.status_text())
}

fn header(panel: &Entity<ReferencesPanel>, cx: &mut VisualTestContext) -> Option<String> {
    panel.read_with(cx, |panel, _| panel.header_text())
}

/// Holds the next run's batches, and its summary, each until a send.
fn hold(panel: &Entity<ReferencesPanel>, cx: &mut VisualTestContext) -> async_channel::Sender<()> {
    let (release, hold) = async_channel::unbounded();
    panel.update(cx, |panel, _| panel.test.hold = Some(hold));
    release
}

fn release(release: &async_channel::Sender<()>, cx: &mut VisualTestContext) {
    release.try_send(()).unwrap();
    settle(cx);
}

/// Clicks row `ix` where the last frame laid it out.
pub(super) fn click(panel: &Entity<ReferencesPanel>, ix: usize, cx: &mut VisualTestContext) {
    let bounds = panel.read_with(cx, |panel, _| panel.row_bounds(ix).expect("drawn"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    settle(cx);
}

/// Clicks at `position` without running anything else, so a file it opens
/// is still read when the update ends.
pub(super) fn click_now(
    position: gpui::Point<gpui::Pixels>,
    window: &mut gpui::Window,
    cx: &mut gpui::App,
) {
    let (modifiers, button) = (Modifiers::none(), gpui::MouseButton::Left);
    let down = gpui::MouseDownEvent {
        position,
        modifiers,
        button,
        click_count: 1,
        first_mouse: false,
    };
    let up = gpui::MouseUpEvent {
        position,
        modifiers,
        button,
        click_count: 1,
    };
    window.dispatch_event(gpui::PlatformInput::MouseDown(down), cx);
    window.dispatch_event(gpui::PlatformInput::MouseUp(up), cx);
}

/// A verified use of `word` on line `line` of `rel`, shown as `preview`.
fn reference(rel: &str, line: u64, preview: &str, word: &str) -> Reference {
    let start = preview.find(word).unwrap();
    Reference {
        location: Location {
            rel: rel.into(),
            range: start..start + word.len(),
            row: line as u32 - 1,
            snapshot: None,
        },
        line_number: line,
        preview: preview.into(),
        preview_ranges: std::iter::once(start..start + word.len()).collect(),
        verified: true,
        is_definition: false,
    }
}

fn file(rel: &str, refs: Vec<Reference>) -> FileReferences {
    FileReferences {
        rel: rel.into(),
        refs,
    }
}

/// Answers find references with the batches set for the word at the
/// caret, and nothing for any other word.
#[derive(Default)]
struct Fake {
    /// Per word: its batches, and whether the summary is truncated.
    answers: HashMap<String, (Vec<Vec<FileReferences>>, bool)>,
    calls: AtomicUsize,
}

impl Fake {
    fn answer(mut self, word: &str, batches: Vec<Vec<FileReferences>>, truncated: bool) -> Self {
        self.answers.insert(word.into(), (batches, truncated));
        self
    }

    /// Asked before the search provider.
    fn add(self, workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Arc<Fake> {
        let fake = Arc::new(self);
        let navigation = workbench.read_with(cx, |workbench, _| workbench.navigation().clone());
        navigation.add(fake.clone(), 10);
        fake
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }
}

/// The ASCII word around `offset` in `text`.
fn word_around(text: &str, offset: usize) -> String {
    let is_word = |ch: char| ch.is_ascii_alphanumeric() || ch == '_';
    let start = text[..offset]
        .rfind(|ch| !is_word(ch))
        .map_or(0, |at| at + 1);
    let end = text[offset..]
        .find(|ch| !is_word(ch))
        .map_or(text.len(), |at| offset + at);
    text[start..end].to_owned()
}

impl NavigationProvider for Fake {
    fn name(&self) -> &str {
        "fake"
    }

    fn precision(&self) -> Precision {
        Precision::Exact
    }

    fn readiness(&self, _: Option<&str>) -> Readiness {
        Readiness::Ready
    }

    fn definitions(&self, _: &NavRequest, _: &AtomicBool) -> Result<Definitions, NavError> {
        Err(NavError::Unsupported)
    }

    fn references(
        &self,
        req: &NavRequest,
        _: &AtomicBool,
        sink: &mut dyn FnMut(Vec<FileReferences>),
    ) -> Result<ReferencesSummary, NavError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let word = word_around(&req.text.to_string(), req.offset);
        let (batches, truncated) = self.answers.get(&word).cloned().unwrap_or_default();
        for batch in batches {
            sink(batch);
        }
        Ok(ReferencesSummary {
            word: Some(word),
            truncated,
            ..ReferencesSummary::default()
        })
    }

    fn workspace_symbols(
        &self,
        _: &str,
        _: usize,
        _: &AtomicBool,
    ) -> Result<Vec<WorkspaceSymbol>, NavError> {
        Ok(Vec::new())
    }
}

#[gpui::test]
fn shift_f12_shows_the_panel(cx: &mut TestAppContext) {
    let util = "pub fn helper() {}\n";
    let (_dir, root) = project(&[("src/main.rs", MAIN), ("src/util.rs", util)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    caret_on(&workbench, "helper", cx);
    let dock = workbench.read_with(cx, |workbench, _| workbench.dock().clone());
    cx.simulate_keystrokes("cmd-b");
    settle(cx);
    let tabs = |cx: &mut VisualTestContext| {
        dock.read_with(cx, |dock, _| {
            let button = dock.rail_button_bounds(DockPanel::References);
            (dock.rail_icons().to_vec(), button.is_some())
        })
    };
    assert!(!dock.read_with(cx, |dock, _| dock.is_visible()));
    let before = (vec![Icon::Files, Icon::Search, Icon::ListTree], false);
    assert_eq!(tabs(cx), before, "no References tab yet");

    cx.simulate_keystrokes("shift-f12");
    settle(cx);
    let state = dock.read_with(cx, |dock, _| {
        (dock.is_visible(), dock.active(), dock.title())
    });
    assert_eq!(state, (true, DockPanel::References, "References".into()));
    let panel = references(&workbench, cx);
    assert_eq!(
        header(&panel, cx).as_deref(),
        Some("References to `helper`")
    );
    let after = vec![Icon::Files, Icon::Search, Icon::ListTree, Icon::FileSymlink];
    assert_eq!(tabs(cx), (after, true));

    let focused = cx.update(|window, cx| panel.focus_handle(cx).is_focused(window));
    assert!(focused, "the panel's list has focus");
    let rows = shown(&panel, cx);
    assert!(matches!(rows[0], File(..)), "{rows:?}");
    assert!(matches!(rows[1], ShownRow::Ref { .. }), "{rows:?}");
    assert_eq!(panel.read_with(cx, |panel, _| panel.selected()), Some(1));
}

#[gpui::test]
fn results_stream_grouped_by_file(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("src/main.rs", MAIN)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    let call = "    util::helper();";
    let unverified = Reference {
        verified: false,
        ..reference("src/a.rs", 7, "// helper, maybe", "helper")
    };
    let definition = Reference {
        is_definition: true,
        ..reference("src/util.rs", 1, "pub fn helper() {}", "helper")
    };
    let batches = vec![
        vec![file(
            "src/main.rs",
            vec![
                reference("src/main.rs", 4, call, "helper"),
                reference("src/main.rs", 9, "helper(helper)", "helper"),
            ],
        )],
        vec![file(
            "src/a.rs",
            vec![reference("src/a.rs", 2, call, "helper"), unverified],
        )],
        vec![file("src/util.rs", vec![definition])],
    ];
    let fake = Fake::default()
        .answer("helper", batches, false)
        .add(&workbench, cx);
    let panel = references(&workbench, cx);
    let release_one = hold(&panel, cx);
    caret_on(&workbench, "helper", cx);

    cx.simulate_keystrokes("shift-f12");
    settle(cx);
    let finding = Some("Finding references to `helper`…".to_owned());
    assert_eq!(shown(&panel, cx), []);
    assert_eq!(status(&panel, cx), finding);

    let line = |line, preview: &str, dimmed, definition| {
        let start = preview.find("helper").unwrap();
        ShownRow::Ref {
            line,
            preview: preview.into(),
            highlights: std::iter::once(start..start + 6).collect(),
            dimmed,
            definition,
        }
    };
    let expected = [
        File("src/main.rs".into(), 2),
        line(4, call, false, false),
        line(9, "helper(helper)", false, false),
        File("src/a.rs".into(), 2),
        line(2, call, false, false),
        line(7, "// helper, maybe", true, false),
        File("src/util.rs".into(), 1),
        line(1, "pub fn helper() {}", false, true),
    ];
    // Each release adds its file below the rows already shown.
    for shows in [3, 6, 8] {
        release(&release_one, cx);
        assert_eq!(shown(&panel, cx), expected[..shows]);
        assert_eq!(status(&panel, cx), finding);
    }
    release(&release_one, cx);
    assert_eq!(shown(&panel, cx), expected);
    let done = "5 references in 3 files";
    assert_eq!(status(&panel, cx).as_deref(), Some(done));
    assert_eq!(fake.calls(), 1);
    // As the frame drew them: (row, dimmed, tagged a definition).
    let drawn = panel.read_with(cx, |panel, _| panel.drawn());
    let styles = [
        (1, false, false),
        (2, false, false),
        (4, false, false),
        (5, true, false),
        (7, false, true),
    ];
    assert_eq!(drawn, styles);
}

#[gpui::test]
fn search_provider_end_to_end(cx: &mut TestAppContext) {
    let main = "mod a;\nmod util;\n\nfn main() {\n    util::helper();\n    // calls helper\n}\n";
    let a = "pub fn run() {\n    crate::util::helper();\n}\n";
    let util = "pub fn helper() {}\n";
    let (_dir, root) = project(&[
        ("src/a.rs", a),
        ("src/main.rs", main),
        ("src/util.rs", util),
    ]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    caret_on(&workbench, "helper", cx);
    cx.simulate_keystrokes("shift-f12");
    settle(cx);

    let panel = references(&workbench, cx);
    let rows = shown(&panel, cx);
    let mut groups: Vec<(String, Vec<(u64, bool)>)> = Vec::new();
    for row in rows {
        match row {
            File(rel, _) => groups.push((rel, Vec::new())),
            ShownRow::Ref {
                line, definition, ..
            } => groups.last_mut().unwrap().1.push((line, definition)),
        }
    }
    assert_eq!(
        groups[0],
        ("src/main.rs".into(), vec![(5, false)]),
        "{groups:?}"
    );
    groups.sort();
    assert_eq!(
        groups,
        [
            ("src/a.rs".into(), vec![(2, false)]),
            ("src/main.rs".into(), vec![(5, false)]),
            ("src/util.rs".into(), vec![(1, true)]),
        ],
        "the definition and both calls, not the comment"
    );
    let status = status(&panel, cx).unwrap();
    assert_eq!(status, "3 references in 3 files (by search)");
}

#[gpui::test]
fn click_and_enter_open_a_reference(cx: &mut TestAppContext) {
    let main = "mod a;\nmod util;\n\nfn main() {\n    util::helper();\n}\n";
    let a = "pub fn run() {\n    crate::util::helper();\n}\n";
    let util = "pub fn other() {}\r\n\r\npub fn helper() {}\r\n";
    let (_dir, root) = project(&[
        ("src/a.rs", a),
        ("src/main.rs", main),
        ("src/util.rs", util),
    ]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    let caret = caret_on(&workbench, "helper", cx);
    cx.simulate_keystrokes("shift-f12");
    settle(cx);
    let panel = references(&workbench, cx);
    let rows = shown(&panel, cx);
    assert_eq!(rows.len(), 6, "{rows:?}");
    let locations = panel.read_with(cx, |panel, _| panel.locations());
    // Reference `n`'s row, and its location.
    let row = |n: usize| {
        let refs = rows.iter().enumerate();
        let mut refs = refs.filter(|(_, row)| matches!(row, ShownRow::Ref { .. }));
        (refs.nth(n).unwrap().0, locations[n].clone())
    };
    let opens = |location: &Location, cx: &mut VisualTestContext| {
        let (path, selection, focused) = at(&workbench, cx);
        assert_eq!(path, root.join(&location.rel));
        assert_eq!(selection, location.range);
        assert!(focused, "the editor has focus");
        let text = workbench.read_with(cx, |workbench, cx| {
            let editor = workbench.active_editor(cx).unwrap().read(cx);
            editor.model().text().to_string()
        });
        assert_eq!(&text[selection], "helper");
    };

    // The CRLF file's reference, by a click.
    let crlf = (0..3).find(|&n| row(n).1.rel == Path::new("src/util.rs"));
    let (ix, location) = row(crlf.unwrap());
    click(&panel, ix, cx);
    opens(&location, cx);

    // The request's own file's, first, by a click; then down, past the next
    // file's header, and enter.
    let (ix, location) = row(0);
    assert_eq!(location.rel, Path::new("src/main.rs"));
    click(&panel, ix, cx);
    opens(&location, cx);
    cx.update(|window, cx| window.focus(&panel.focus_handle(cx), cx));
    cx.simulate_keystrokes("down");
    let (next, location) = row(1);
    assert_eq!(next, ix + 2, "the header row is skipped");
    assert_eq!(panel.read_with(cx, |panel, _| panel.selected()), Some(next));
    cx.simulate_keystrokes("enter");
    settle(cx);
    opens(&location, cx);

    // Back goes to where shift-F12 was pressed.
    cx.simulate_keystrokes("ctrl--");
    settle(cx);
    let (path, selection, _) = at(&workbench, cx);
    assert_eq!((path, selection), (root.join("src/main.rs"), caret..caret));
}

#[gpui::test]
fn a_new_run_cancels_the_old(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("src/main.rs", MAIN)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    let helpers = vec![vec![file(
        "src/main.rs",
        vec![reference("src/main.rs", 4, "    util::helper();", "helper")],
    )]];
    let others = vec![
        vec![file(
            "src/main.rs",
            vec![reference("src/main.rs", 5, "    other();", "other")],
        )],
        vec![file(
            "src/b.rs",
            vec![reference("src/b.rs", 1, "fn other() {}", "other")],
        )],
    ];
    let fake = Fake::default()
        .answer("helper", helpers, false)
        .answer("other", others, false)
        .add(&workbench, cx);
    let panel = references(&workbench, cx);
    let _held = hold(&panel, cx);
    caret_on(&workbench, "helper", cx);
    cx.simulate_keystrokes("shift-f12");
    settle(cx);
    assert_eq!(fake.calls(), 1);
    let first = panel.read_with(cx, |panel, _| panel.cancel_flag());
    assert!(!first.load(Ordering::Relaxed));

    focus_editor(&workbench, cx);
    caret_on(&workbench, "other", cx);
    cx.simulate_keystrokes("shift-f12");
    assert!(first.load(Ordering::Relaxed), "the first run is cancelled");
    settle(cx);
    let rows = shown(&panel, cx);
    let previews: Vec<_> = (rows.iter())
        .filter_map(|row| match row {
            ShownRow::Ref { preview, .. } => Some(preview.as_str()),
            File(..) => None,
        })
        .collect();
    assert_eq!(previews, ["    other();", "fn other() {}"]);
    assert_eq!(header(&panel, cx).as_deref(), Some("References to `other`"));
    assert_eq!(
        status(&panel, cx).as_deref(),
        Some("2 references in 2 files")
    );

    // Showing another panel and then References keeps the rows.
    cx.simulate_keystrokes("cmd-shift-e");
    settle(cx);
    let dock = workbench.read_with(cx, |workbench, _| workbench.dock().clone());
    assert_eq!(
        dock.read_with(cx, |dock, _| dock.active()),
        DockPanel::Project
    );
    let button = dock.read_with(cx, |dock, _| {
        dock.rail_button_bounds(DockPanel::References)
            .expect("drawn")
    });
    cx.simulate_click(button.center(), Modifiers::none());
    settle(cx);
    assert_eq!(
        dock.read_with(cx, |dock, _| dock.active()),
        DockPanel::References
    );
    assert_eq!(shown(&panel, cx), rows);
    assert_eq!(fake.calls(), 2, "no new run");
    assert_eq!(panel.read_with(cx, |panel, _| panel.runs()), 2);

    // Their paths are the old root's: a new root drops them.
    let other = tempfile::tempdir().unwrap();
    let new_root = fs::canonicalize(other.path()).unwrap();
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.set_root(new_root, window, cx)
    });
    settle(cx);
    assert_eq!(shown(&panel, cx), []);
    assert_eq!((header(&panel, cx), status(&panel, cx)), (None, None));
}

#[gpui::test]
fn opening_records_where_the_caret_moved_to(cx: &mut TestAppContext) {
    let main = "mod a;\n\nfn main() {\n    a::helper();\n}\n";
    let a = "pub fn helper() {}\n\npub fn run() {\n    helper();\n}\n";
    let (_dir, root) = project(&[("src/a.rs", a), ("src/main.rs", main)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    let caret = caret_on(&workbench, "helper", cx);
    cx.simulate_keystrokes("shift-f12");
    settle(cx);
    let panel = references(&workbench, cx);
    let rows = shown(&panel, cx);
    assert_eq!(rows.len(), 5, "{rows:?}");
    let place = |cx: &mut VisualTestContext| {
        let (path, selection, _) = at(&workbench, cx);
        (path, selection.end)
    };

    // The first opens from where shift-F12 was pressed, the second from
    // where the caret was moved to since.
    click(&panel, 3, cx);
    let first = place(cx);
    assert_eq!(first.0, root.join("src/a.rs"));
    let moved = a.find("run").unwrap();
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    editor
        .unwrap()
        .update(cx, |editor, _| editor.model_mut().set_cursor(moved));
    click(&panel, 4, cx);
    assert_ne!(place(cx), first);

    focus_editor(&workbench, cx);
    cx.simulate_keystrokes("ctrl--");
    settle(cx);
    assert_eq!(place(cx), (root.join("src/a.rs"), moved));
    cx.simulate_keystrokes("ctrl--");
    settle(cx);
    assert_eq!(place(cx), (root.join("src/main.rs"), caret));
}

#[gpui::test]
fn moving_to_an_unopened_reference_end_is_recorded(cx: &mut TestAppContext) {
    let main = "mod a;\n\nfn main() {\n    a::helper();\n}\n";
    let a = "pub fn helper() {}\n\npub fn run() {\n    helper();\n}\n";
    let (_dir, root) = project(&[("src/a.rs", a), ("src/main.rs", main)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    let caret = caret_on(&workbench, "helper", cx);
    cx.simulate_keystrokes("shift-f12");
    settle(cx);
    let panel = references(&workbench, cx);
    let locations = panel.read_with(cx, |panel, _| panel.locations());
    let end_of = |rel: &str, nth: usize| {
        let all = locations
            .iter()
            .filter(|l| l.rel.as_path() == std::path::Path::new(rel));
        all.map(|l| l.range.end).nth(nth).unwrap()
    };
    let place = |cx: &mut VisualTestContext| {
        let (path, selection, _) = at(&workbench, cx);
        (path, selection.end)
    };
    let move_to = |at: usize, cx: &mut VisualTestContext| {
        let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
        editor
            .unwrap()
            .update(cx, |editor, _| editor.model_mut().set_cursor(at));
    };

    // Before the first open: the end of a reference never opened.
    let first = end_of("src/main.rs", 0);
    assert_ne!(first, caret);
    move_to(first, cx);
    click(&panel, 3, cx);
    assert_eq!(place(cx).0, root.join("src/a.rs"));

    // Between opens: the end of another unopened one.
    let other = end_of("src/a.rs", 1);
    move_to(other, cx);
    click(&panel, 4, cx);

    focus_editor(&workbench, cx);
    cx.simulate_keystrokes("ctrl--");
    settle(cx);
    assert_eq!(place(cx), (root.join("src/a.rs"), other));
    cx.simulate_keystrokes("ctrl--");
    settle(cx);
    assert_eq!(place(cx), (root.join("src/main.rs"), first));
}

#[gpui::test]
fn empty_states(cx: &mut TestAppContext) {
    // No root: nothing runs.
    let (_dir, root) = project(&[("src/main.rs", MAIN)]);
    let path = root.join("src/main.rs");
    let launch = Launch {
        root: None,
        file: Some((path, Some(Buffer::new(MAIN)))),
    };
    let (workbench, cx_, _) = open(cx, launch);
    let fake = Fake::default().add(&workbench, cx_);
    focus_editor(&workbench, cx_);
    caret_on(&workbench, "helper", cx_);
    cx_.simulate_keystrokes("shift-f12");
    settle(cx_);
    let panel = references(&workbench, cx_);
    assert_eq!(status(&panel, cx_).as_deref(), Some("No folder open"));
    assert_eq!(header(&panel, cx_), None);
    assert_eq!(fake.calls(), 0);

    let many = vec![vec![file(
        "src/main.rs",
        vec![
            reference("src/main.rs", 1, "mod util;", "util"),
            reference("src/main.rs", 4, "    util::helper();", "util"),
        ],
    )]];
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    let fake = Fake::default()
        .answer("util", many, true)
        .add(&workbench, cx);
    let panel = references(&workbench, cx);
    let find = |at: usize, cx: &mut VisualTestContext| {
        focus_editor(&workbench, cx);
        let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
        editor
            .unwrap()
            .update(cx, |editor, _| editor.model_mut().set_cursor(at));
        cx.simulate_keystrokes("shift-f12");
        settle(cx);
        (header(&panel, cx), status(&panel, cx).unwrap())
    };

    // The caret between blanks.
    let blank = MAIN.find("    util").unwrap() + 2;
    assert_eq!(find(blank, cx), (None, "No symbol at the caret".into()));
    assert_eq!(fake.calls(), 0);

    let other = MAIN.find("other").unwrap();
    let none = (
        Some("References to `other`".into()),
        "No references to `other`".into(),
    );
    assert_eq!(find(other, cx), none);
    assert_eq!(shown(&panel, cx), []);

    let util = MAIN.find("util").unwrap();
    let truncated = find(util, cx).1;
    assert_eq!(truncated, "Showing the first 2 references");
    assert_eq!(fake.calls(), 2);
}

#[gpui::test]
fn only_visible_rows_are_built(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("src/main.rs", MAIN)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    let files = (0..10)
        .map(|n| {
            let rel = format!("src/f{n}.rs");
            let refs = (1..=100)
                .map(|line| reference(&rel, line, "helper();", "helper"))
                .collect();
            file(&rel, refs)
        })
        .collect();
    Fake::default()
        .answer("helper", vec![files], false)
        .add(&workbench, cx);
    caret_on(&workbench, "helper", cx);
    cx.simulate_keystrokes("shift-f12");
    settle(cx);
    let panel = references(&workbench, cx);
    let count = shown(&panel, cx).len();
    assert_eq!(count, 1_010, "10 headers and 1,000 references");

    // The rows that fit, one more for a partly scrolled one, and the first
    // row measured twice (the list's request_layout and prepaint).
    let (fit, built) = panel.read_with(cx, |panel, _| {
        let row = panel.row_bounds(0).unwrap();
        let fit = (panel.list_bounds().size.height / row.size.height).ceil() as usize;
        (fit, panel.built())
    });
    assert!(fit > 10 && fit < 1_000, "{fit} rows fit");
    let total: usize = built.iter().map(Range::len).sum();
    assert!(
        total > 0 && total <= fit + 1 + 2,
        "{built:?} for {fit} rows"
    );

    // Moving the selection past the rows that fit keeps it in view, past
    // the second file's header, building only the rows around it.
    for _ in 0..100 {
        cx.simulate_keystrokes("down");
    }
    settle(cx);
    let selected = panel.read_with(cx, |panel, _| panel.selected()).unwrap();
    assert_eq!(selected, 1 + 100 + 1);
    let built = panel.read_with(cx, |panel, _| panel.built());
    let total: usize = built.iter().map(Range::len).sum();
    assert!(total <= fit + 1 + 2, "{built:?} for {fit} rows");
    assert_eq!(built.last().unwrap().end, selected + 1, "{built:?}");
}

#[test]
fn the_default_keymap_binds_the_references_commands() {
    let store = fe_keymap::KeymapStore::load(
        fe_keymap::DEFAULT_MACOS,
        None,
        Some(crate::command_catalog()),
    );
    assert_eq!(store.diagnostics(), []);
    let bound = |command: &str| {
        let id = fe_command::CommandId::parse(command).unwrap();
        let bindings = store.keymap().bindings_for(&id);
        (bindings.iter())
            .map(|binding| {
                let context = binding.context.as_ref().map(ToString::to_string);
                (binding.keys.to_string(), context.unwrap_or_default())
            })
            .collect::<Vec<_>>()
    };
    let one = |keys: &str, context: &str| vec![(keys.to_owned(), context.to_owned())];
    assert_eq!(bound("nav.find_references"), one("shift-f12", "workbench"));
    let panel = "ReferencesPanel";
    assert_eq!(bound("references_panel.select_prev"), one("up", panel));
    assert_eq!(bound("references_panel.select_next"), one("down", panel));
    assert_eq!(bound("references_panel.open"), one("enter", panel));
}

#[gpui::test]
fn shift_f12_drops_a_definition_still_to_land(cx: &mut TestAppContext) {
    let util = "pub fn helper() {}\n";
    let (_dir, root) = project(&[("src/main.rs", MAIN), ("src/util.rs", util)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    let (release, held) = async_channel::bounded::<()>(1);
    workbench.update(cx, |workbench, _| {
        workbench.nav_state.hold_answer = Some(held)
    });
    caret_on(&workbench, "helper", cx);
    cx.simulate_keystrokes("f12");
    settle(cx);
    cx.simulate_keystrokes("shift-f12");
    settle(cx);

    // The definition's answer lands after shift-F12: it neither jumps nor
    // takes focus from the panel.
    drop(release);
    settle(cx);
    let panel = references(&workbench, cx);
    let focused = cx.update(|window, cx| panel.focus_handle(cx).is_focused(window));
    assert!(focused, "References keeps focus");
    assert_eq!(at(&workbench, cx).0, root.join("src/main.rs"));
    assert_eq!(
        workbench.read_with(cx, |workbench, _| workbench.palette),
        None
    );
}

#[gpui::test]
fn a_clamped_reference_counts_as_where_the_caret_landed(cx: &mut TestAppContext) {
    // `src/a.rs` has shrunk since its references were found: both land
    // clamped to its end.
    let a = "fn a() {}\n";
    let (_dir, root) = project(&[("src/main.rs", MAIN), ("src/a.rs", a)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    let gone = |line, start: usize| Reference {
        location: Location {
            rel: "src/a.rs".into(),
            range: start..start + 6,
            row: line as u32 - 1,
            snapshot: None,
        },
        ..reference("src/a.rs", line, "helper();", "helper")
    };
    let batches = vec![vec![
        file(
            "src/main.rs",
            vec![reference("src/main.rs", 4, "    util::helper();", "helper")],
        ),
        file("src/a.rs", vec![gone(20, 200), gone(30, 300)]),
    ]];
    Fake::default()
        .answer("helper", batches, false)
        .add(&workbench, cx);
    let caret = caret_on(&workbench, "helper", cx);
    cx.simulate_keystrokes("shift-f12");
    settle(cx);
    let panel = references(&workbench, cx);
    assert_eq!(shown(&panel, cx).len(), 5);
    let place = |cx: &mut VisualTestContext| {
        let (path, selection, _) = at(&workbench, cx);
        (path, selection.end)
    };

    // The first loads `src/a.rs`, the second finds it open; the main.rs one
    // then opens from where the second left the caret.
    click(&panel, 3, cx);
    assert_eq!(place(cx), (root.join("src/a.rs"), a.len()));
    click(&panel, 4, cx);
    assert_eq!(place(cx), (root.join("src/a.rs"), a.len()));
    click(&panel, 1, cx);
    assert_eq!(place(cx).0, root.join("src/main.rs"));

    // Neither clamped landing was recorded: back returns to where shift-F12
    // was pressed.
    focus_editor(&workbench, cx);
    cx.simulate_keystrokes("ctrl--");
    settle(cx);
    assert_eq!(place(cx), (root.join("src/main.rs"), caret));
}

#[gpui::test]
fn a_landing_is_followed_in_its_own_pane(cx: &mut TestAppContext) {
    let main = "mod a;\n\nfn main() {\n    a::helper();\n}\n";
    let a = "pub fn helper() {}\n";
    let (_dir, root) = project(&[("src/a.rs", a), ("src/main.rs", main)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    // Two panes on main.rs; the right one, active, is where references open.
    cx.simulate_keystrokes("cmd-\\");
    settle(cx);
    let panes = workbench.read_with(cx, |workbench, _| {
        let panes = workbench.pane_group().panes();
        panes.into_iter().cloned().collect::<Vec<_>>()
    });
    let [left, right] = [panes[0].clone(), panes[1].clone()];
    assert_eq!(
        workbench.read_with(cx, |workbench, _| workbench.pane().clone()),
        right
    );
    let caret = caret_on(&workbench, "helper", cx);
    cx.simulate_keystrokes("shift-f12");
    settle(cx);
    let panel = references(&workbench, cx);
    let rows = shown(&panel, cx);
    let row = |rel: &str| {
        let header = rows.iter().position(|row| *row == File(rel.into(), 1));
        header.expect("a header") + 1
    };
    let (a_row, main_row) = (row("src/a.rs"), row("src/main.rs"));

    // a.rs opens in the right pane, and the left one is made active while
    // it's still read: nothing runs the read until the update ends.
    let bounds = panel.read_with(cx, |panel, _| panel.row_bounds(a_row).expect("drawn"));
    cx.update(|window, cx| {
        click_now(bounds.center(), window, cx);
        assert!(right.read(cx).is_opening(&root.join("src/a.rs")));
        left.update(cx, |_, cx| cx.emit(crate::PaneEvent::Activated));
    });
    settle(cx);
    // Read, a.rs's tab takes the right pane active again.
    let (path, selection, _) = at(&workbench, cx);
    assert_eq!(path, root.join("src/a.rs"));
    let landed = selection.end;

    // From where a.rs's reference left the caret, main.rs's opens, then
    // back returns to where shift-F12 was pressed.
    click(&panel, main_row, cx);
    assert_eq!(at(&workbench, cx).0, root.join("src/main.rs"));
    focus_editor(&workbench, cx);
    cx.simulate_keystrokes("ctrl--");
    settle(cx);
    let (path, selection, _) = at(&workbench, cx);
    assert_ne!((&path, selection.end), (&root.join("src/a.rs"), landed));
    assert_eq!((path, selection), (root.join("src/main.rs"), caret..caret));
}

#[gpui::test]
fn back_and_forward_while_a_reference_is_read(cx: &mut TestAppContext) {
    let main = "mod a;\n\nfn main() {\n    a::helper();\n}\n";
    let a = "pub fn helper() {}\n";
    let (_dir, root) = project(&[("src/a.rs", a), ("src/main.rs", main)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    let caret = caret_on(&workbench, "helper", cx);
    cx.simulate_keystrokes("shift-f12");
    settle(cx);
    let panel = references(&workbench, cx);
    let rows = shown(&panel, cx);
    let row = |rel: &str| {
        rows.iter()
            .position(|row| *row == File(rel.into(), 1))
            .unwrap()
            + 1
    };
    let (a_row, main_row) = (row("src/a.rs"), row("src/main.rs"));

    // Back while a.rs is still read, then forward to it once it is.
    let bounds = panel.read_with(cx, |panel, _| panel.row_bounds(a_row).expect("drawn"));
    cx.update(|window, cx| {
        click_now(bounds.center(), window, cx);
        window.dispatch_keystroke(gpui::Keystroke::parse("ctrl--").unwrap(), cx);
    });
    settle(cx);
    assert_eq!(at(&workbench, cx).0, root.join("src/main.rs"));
    focus_editor(&workbench, cx);
    cx.simulate_keystrokes("ctrl-shift--");
    settle(cx);
    assert_eq!(at(&workbench, cx).0, root.join("src/a.rs"));

    // Back dropped a.rs's reveal, so where forward left the caret, on its
    // reference's end, isn't a panel landing (T196's decision): recorded.
    let end = at(&workbench, cx).1.end;
    click(&panel, main_row, cx);
    focus_editor(&workbench, cx);
    cx.simulate_keystrokes("ctrl--");
    settle(cx);
    let (path, selection, _) = at(&workbench, cx);
    assert_eq!((path, selection), (root.join("src/a.rs"), end..end));
    cx.simulate_keystrokes("ctrl--");
    settle(cx);
    let (path, selection, _) = at(&workbench, cx);
    assert_eq!((path, selection), (root.join("src/main.rs"), caret..caret));
}

#[gpui::test]
fn a_reference_landing_in_another_pane_meanwhile(cx: &mut TestAppContext) {
    let main = "mod a;\n\nfn main() {\n    a::helper();\n}\n";
    let a = "pub fn helper() {}\n";
    let (_dir, root) = project(&[("src/a.rs", a), ("src/main.rs", main)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    cx.simulate_keystrokes("cmd-\\");
    settle(cx);
    let left = workbench.read_with(cx, |workbench, _| workbench.pane_group().panes()[0].clone());
    caret_on(&workbench, "helper", cx);
    cx.simulate_keystrokes("shift-f12");
    settle(cx);
    let panel = references(&workbench, cx);
    let rows = shown(&panel, cx);
    let row = |rel: &str| {
        rows.iter()
            .position(|row| *row == File(rel.into(), 1))
            .unwrap()
            + 1
    };
    let bounds = |ix: usize, cx: &mut VisualTestContext| {
        panel.read_with(cx, |panel, _| panel.row_bounds(ix).expect("drawn"))
    };
    let (a_at, main_at) = (bounds(row("src/a.rs"), cx), bounds(row("src/main.rs"), cx));

    // a.rs opens in the right pane; while it's read the left pane is made
    // active and main.rs's reference opens there, from the left editor's
    // caret, at 0. The right pane's read then lands, and takes it active.
    // Each update ends with its effects applied, but no read run.
    cx.update(|window, cx| {
        click_now(a_at.center(), window, cx);
        left.update(cx, |_, cx| cx.emit(crate::PaneEvent::Activated));
    });
    cx.update(|window, cx| {
        let workbench = workbench.read(cx);
        assert_eq!(*workbench.pane(), left);
        let right = workbench.pane_group().panes()[1].clone();
        assert!(right.read(cx).is_opening(&root.join("src/a.rs")));
        click_now(main_at.center(), window, cx);
    });
    settle(cx);
    assert_eq!(at(&workbench, cx).0, root.join("src/a.rs"));

    // From where a.rs's reference left the caret, nothing is recorded: back
    // goes to the left editor's caret, not a.rs.
    click(&panel, row("src/main.rs"), cx);
    focus_editor(&workbench, cx);
    cx.simulate_keystrokes("ctrl--");
    settle(cx);
    let (path, selection, _) = at(&workbench, cx);
    assert_eq!((path, selection), (root.join("src/main.rs"), 0..0));
}

#[gpui::test]
fn asked_for_at_the_end_of_a_reference(cx: &mut TestAppContext) {
    let main = "mod a;\n\nfn main() {\n    a::helper();\n}\n";
    let a = "pub fn helper() {}\n";
    let (_dir, root) = project(&[("src/a.rs", a), ("src/main.rs", main)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    // The caret just after the word, where its own reference ends.
    let end = main.find("helper").unwrap() + "helper".len();
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    editor
        .unwrap()
        .update(cx, |editor, _| editor.model_mut().set_cursor(end));
    cx.simulate_keystrokes("shift-f12");
    settle(cx);
    let panel = references(&workbench, cx);
    let rows = shown(&panel, cx);
    let row = |rel: &str| {
        rows.iter()
            .position(|row| *row == File(rel.into(), 1))
            .unwrap()
            + 1
    };
    click(&panel, row("src/a.rs"), cx);
    click(&panel, row("src/main.rs"), cx);
    focus_editor(&workbench, cx);
    cx.simulate_keystrokes("ctrl--");
    settle(cx);
    let (path, selection, _) = at(&workbench, cx);
    assert_eq!((path, selection), (root.join("src/main.rs"), end..end));
}

#[gpui::test]
fn another_reference_opened_while_one_is_read(cx: &mut TestAppContext) {
    let main = "mod a;\n\nfn main() {\n    a::helper();\n}\n";
    let a = "pub fn helper() {}\n";
    let (_dir, root) = project(&[("src/a.rs", a), ("src/main.rs", main)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    let caret = caret_on(&workbench, "helper", cx);
    cx.simulate_keystrokes("shift-f12");
    settle(cx);
    let panel = references(&workbench, cx);
    let rows = shown(&panel, cx);
    let row = |rel: &str| {
        rows.iter()
            .position(|row| *row == File(rel.into(), 1))
            .unwrap()
            + 1
    };
    let bounds = |ix: usize, cx: &mut VisualTestContext| {
        panel.read_with(cx, |panel, _| panel.row_bounds(ix).expect("drawn"))
    };
    let (a_at, main_at) = (bounds(row("src/a.rs"), cx), bounds(row("src/main.rs"), cx));

    // main.rs's reference opens while a.rs is still read, the caret taken
    // to be where a.rs's reference goes: not recorded.
    cx.update(|window, cx| click_now(a_at.center(), window, cx));
    cx.update(|window, cx| {
        let pane = workbench.read(cx).pane().clone();
        assert!(pane.read(cx).is_opening(&root.join("src/a.rs")));
        click_now(main_at.center(), window, cx);
    });
    settle(cx);
    assert_eq!(at(&workbench, cx).0, root.join("src/main.rs"));
    focus_editor(&workbench, cx);
    cx.simulate_keystrokes("ctrl--");
    settle(cx);
    let (path, selection, _) = at(&workbench, cx);
    assert_eq!((path, selection), (root.join("src/main.rs"), caret..caret));
}

#[gpui::test]
fn moving_back_to_an_earlier_opened_end_is_recorded(cx: &mut TestAppContext) {
    let main = "mod a;\n\nfn main() {\n    a::helper();\n}\n";
    let a = "pub fn helper() {}\n\npub fn run() {\n    helper();\n}\n";
    let (_dir, root) = project(&[("src/a.rs", a), ("src/main.rs", main)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    let caret = caret_on(&workbench, "helper", cx);
    cx.simulate_keystrokes("shift-f12");
    settle(cx);
    let panel = references(&workbench, cx);
    let locations = panel.read_with(cx, |panel, _| panel.locations());
    let ends: Vec<usize> = (locations.iter())
        .filter(|l| l.rel.as_path() == std::path::Path::new("src/a.rs"))
        .map(|l| l.range.end)
        .collect();
    let place = |cx: &mut VisualTestContext| {
        let (path, selection, _) = at(&workbench, cx);
        (path, selection.end)
    };

    // Two references opened in a row; the caret then moved back to the end
    // of the first, which is no longer where one left it.
    click(&panel, 3, cx);
    click(&panel, 4, cx);
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    editor
        .unwrap()
        .update(cx, |editor, _| editor.model_mut().set_cursor(ends[0]));
    click(&panel, 1, cx);
    assert_eq!(place(cx).0, root.join("src/main.rs"));

    focus_editor(&workbench, cx);
    cx.simulate_keystrokes("ctrl--");
    settle(cx);
    assert_eq!(place(cx), (root.join("src/a.rs"), ends[0]));
    cx.simulate_keystrokes("ctrl--");
    settle(cx);
    assert_eq!(place(cx), (root.join("src/main.rs"), caret));
}

#[gpui::test]
fn moving_to_the_end_of_a_superseded_reference_is_recorded(cx: &mut TestAppContext) {
    let main = "mod a;\n\nfn main() {\n    a::helper();\n}\n";
    let a = "pub fn helper() {}\n\npub fn run() {\n    helper();\n}\n";
    let (_dir, root) = project(&[("src/a.rs", a), ("src/main.rs", main)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    let caret = caret_on(&workbench, "helper", cx);
    cx.simulate_keystrokes("shift-f12");
    settle(cx);
    let panel = references(&workbench, cx);
    let locations = panel.read_with(cx, |panel, _| panel.locations());
    let ends: Vec<usize> = (locations.iter())
        .filter(|l| l.rel.as_path() == std::path::Path::new("src/a.rs"))
        .map(|l| l.range.end)
        .collect();
    let place = |cx: &mut VisualTestContext| {
        let (path, selection, _) = at(&workbench, cx);
        (path, selection.end)
    };
    let bounds = |ix: usize, cx: &mut VisualTestContext| {
        panel.read_with(cx, |panel, _| panel.row_bounds(ix).expect("drawn"))
    };
    let (second, first) = (bounds(4, cx), bounds(3, cx));

    // The second reference is superseded by the first while a.rs is still
    // read: it never lands.
    cx.update(|window, cx| click_now(second.center(), window, cx));
    cx.update(|window, cx| click_now(first.center(), window, cx));
    settle(cx);
    assert_eq!(place(cx), (root.join("src/a.rs"), ends[0]));
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    editor
        .unwrap()
        .update(cx, |editor, _| editor.model_mut().set_cursor(ends[1]));
    click(&panel, 1, cx);
    assert_eq!(place(cx).0, root.join("src/main.rs"));

    focus_editor(&workbench, cx);
    cx.simulate_keystrokes("ctrl--");
    settle(cx);
    assert_eq!(place(cx), (root.join("src/a.rs"), ends[1]));
    cx.simulate_keystrokes("ctrl--");
    settle(cx);
    assert_eq!(place(cx), (root.join("src/main.rs"), caret));
}
