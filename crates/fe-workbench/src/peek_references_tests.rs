//! Peek References in a headless workbench, over a fake provider's answer:
//! shift-f12 on `helper` in `main.rs` lists its 3 uses, in 2 files.

use std::{
    fs,
    ops::Range,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

use fe_editor::decorations::DecorationKind;
use fe_gpui::EditorView;
use fe_nav::{
    Definitions, FileReferences, Location, NavError, NavRequest, NavigationProvider, Precision,
    Readiness, Reference, ReferencesSummary, WorkspaceSymbol,
};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext, px, size};

use crate::{
    DockPanel, ReferencesPanel, Workbench, peek::PeekView, references_panel::ShownRow,
    tests::open_root,
};

const MAIN: &str = "mod util;\n\nfn main() {\n    util::helper();\n    util::helper();\n}\n";

/// `helper` on row 20, so the excerpt scrolls to it.
fn util() -> String {
    let mut text: String = (0..20).map(|n| format!("// line {n}\n")).collect();
    text.push_str("pub fn helper() {}\n");
    text.extend((21..40).map(|n| format!("// line {n}\n")));
    text
}

/// The uses of `helper` at `starts` in `text`, the file at `rel`.
fn file(rel: &str, text: &str, starts: &[usize]) -> FileReferences {
    let refs = (starts.iter())
        .map(|&start| {
            let row = text[..start].matches('\n').count();
            let line_start = text[..start].rfind('\n').map_or(0, |at| at + 1);
            let line_end = text[start..].find('\n').map_or(text.len(), |at| start + at);
            let column = start - line_start;
            Reference {
                location: Location {
                    rel: rel.into(),
                    range: start..start + "helper".len(),
                    row: row as u32,
                    snapshot: None,
                },
                line_number: row as u64 + 1,
                preview: text[line_start..line_end].into(),
                preview_ranges: std::iter::once(column..column + "helper".len()).collect(),
                verified: true,
                is_definition: false,
            }
        })
        .collect();
    FileReferences {
        rel: rel.into(),
        refs,
    }
}

/// `helper`'s uses at `n`th match in `text`.
fn nth(text: &str, n: usize) -> usize {
    text.match_indices("helper").nth(n).unwrap().0
}

/// Answers find references with `helper` in `util.rs`, then its two calls
/// in `main.rs`, in two batches.
struct Fake;

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
        _: &NavRequest,
        _: &AtomicBool,
        sink: &mut dyn FnMut(Vec<FileReferences>),
    ) -> Result<ReferencesSummary, NavError> {
        sink(vec![file("src/util.rs", &util(), &[nth(&util(), 0)])]);
        sink(vec![file(
            "src/main.rs",
            MAIN,
            &[nth(MAIN, 0), nth(MAIN, 1)],
        )]);
        Ok(ReferencesSummary {
            word: Some("helper".into()),
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

fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// A workbench on `src/main.rs` and `src/util.rs`, showing `main.rs`
/// focused with its caret in the first call of `helper`.
fn opened(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    PathBuf,
    Entity<Workbench>,
    &mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), MAIN).unwrap();
    fs::write(root.join("src/util.rs"), util()).unwrap();
    let (workbench, cx) = open_root(cx, &root);
    cx.simulate_resize(size(px(1000.), px(800.)));
    let path = root.join("src/main.rs");
    cx.update(|window, cx| {
        let pane = workbench.read(cx).pane().clone();
        pane.update(cx, |pane, cx| pane.open(path, window, cx));
    });
    settle(cx);
    let navigation = workbench.read_with(cx, |workbench, _| workbench.navigation().clone());
    navigation.add(Arc::new(Fake), 10);
    let editor = editor(&workbench, cx);
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&editor.focus_handle(cx), cx);
    });
    editor.update(cx, |editor, cx| {
        editor.model_mut().set_cursor(nth(MAIN, 0) + 2);
        cx.notify();
    });
    settle(cx);
    (dir, root, workbench, cx)
}

fn editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a file is shown")
}

fn peek(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<Entity<PeekView>> {
    workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        pane.peek.as_ref().map(|peek| peek.view.clone())
    })
}

fn list(peek: &Entity<PeekView>, cx: &mut VisualTestContext) -> Entity<ReferencesPanel> {
    peek.read_with(cx, |peek, _| peek.references().cloned())
        .expect("a references list")
}

/// The excerpt's text, its selection and highlights.
fn excerpt(
    peek: &Entity<PeekView>,
    cx: &mut VisualTestContext,
) -> (String, Range<usize>, Vec<Range<usize>>) {
    let excerpt = peek.read_with(cx, |peek, _| peek.excerpt().cloned());
    let excerpt = excerpt.expect("the excerpt is shown");
    excerpt.read_with(cx, |excerpt, _| {
        let model = excerpt.model();
        let text = model.text().to_string();
        let highlights = (model.decorations(0..text.len()).into_iter())
            .filter(|decoration| decoration.kind == DecorationKind::Highlight)
            .map(|decoration| decoration.range)
            .collect();
        (text, model.selections()[0].range.clone(), highlights)
    })
}

/// The active editor's file and its selection.
fn at(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (PathBuf, Range<usize>) {
    let editor = editor(workbench, cx);
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        (
            model.path().unwrap().to_owned(),
            model.selections()[0].range.clone(),
        )
    })
}

fn name(start: usize) -> Range<usize> {
    start..start + "helper".len()
}

/// The rows the provider's answer lists, as the References panel shows
/// them.
fn rows() -> Vec<ShownRow> {
    let reference = |line: u64, preview: &str| ShownRow::Ref {
        line,
        preview: preview.into(),
        highlights: vec![name(preview.find("helper").unwrap())],
        dimmed: false,
        definition: false,
    };
    vec![
        ShownRow::File("src/util.rs".into(), 1),
        reference(21, "pub fn helper() {}"),
        ShownRow::File("src/main.rs".into(), 2),
        reference(4, "    util::helper();"),
        reference(5, "    util::helper();"),
    ]
}

#[gpui::test]
fn list_and_excerpt(cx: &mut TestAppContext) {
    let (_dir, _root, workbench, cx) = opened(cx);
    cx.simulate_keystrokes("shift-f12");
    settle(cx);
    let peek = peek(&workbench, cx).expect("a peek opened");
    let list = list(&peek, cx);
    // Grouped by file: 2 groups, 3 references.
    let shown = list.read_with(cx, |list, _| list.shown_rows());
    assert_eq!(shown, rows());
    let focused = cx.update(|window, cx| list.focus_handle(cx).is_focused(window));
    assert!(focused, "the list has focus");
    // The first reference's source, its use highlighted and selected.
    let util = util();
    let first = name(nth(&util, 0));
    let shown = excerpt(&peek, cx);
    assert_eq!(shown, (util, first.clone(), vec![first]));
    let header = peek.read_with(cx, |peek, cx| peek.header(cx));
    assert_eq!(header.as_deref(), Some("util.rs 21:8"));

    // Down selects the next, and the excerpt follows.
    cx.simulate_keystrokes("down");
    settle(cx);
    assert_eq!(
        list.read_with(cx, |list, _| list.selected_reference()),
        Some(1)
    );
    let call = name(nth(MAIN, 0));
    assert_eq!(excerpt(&peek, cx), (MAIN.into(), call.clone(), vec![call]));
    cx.simulate_keystrokes("down");
    settle(cx);
    let call = name(nth(MAIN, 1));
    assert_eq!(excerpt(&peek, cx), (MAIN.into(), call.clone(), vec![call]));
    let header = peek.read_with(cx, |peek, cx| peek.header(cx));
    assert_eq!(header.as_deref(), Some("main.rs 5:11"));
    // Its list is the peek's own: the dock's References panel is left as
    // it was.
    let dock = workbench.read_with(cx, |workbench, cx| {
        let dock = workbench.dock().read(cx);
        let rows = dock.references().read(cx).shown_rows();
        (dock.active() == DockPanel::References, rows)
    });
    assert_eq!(dock, (false, Vec::new()));
}

#[gpui::test]
fn enter_and_esc(cx: &mut TestAppContext) {
    let (_dir, root, workbench, cx) = opened(cx);
    let editor = editor(&workbench, cx);
    let tabs = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |workbench, cx| workbench.pane().read(cx).tabs().len())
    };
    let (was, open) = (at(&workbench, cx), tabs(cx));

    // Esc closes it, the caret where it was, nothing opened.
    cx.simulate_keystrokes("shift-f12");
    settle(cx);
    assert!(peek(&workbench, cx).is_some());
    cx.simulate_keystrokes("escape");
    settle(cx);
    assert!(peek(&workbench, cx).is_none());
    assert_eq!(at(&workbench, cx), was);
    assert_eq!(tabs(cx), open);
    let focused = cx.update(|window, cx| editor.focus_handle(cx).is_focused(window));
    assert!(focused, "the editor has focus back");

    // Enter opens the selected reference where it is, and closes it.
    cx.simulate_keystrokes("shift-f12");
    settle(cx);
    cx.simulate_keystrokes("down down");
    settle(cx);
    cx.simulate_keystrokes("enter");
    settle(cx);
    assert!(peek(&workbench, cx).is_none());
    assert_eq!(
        at(&workbench, cx),
        (root.join("src/main.rs"), name(nth(MAIN, 1)))
    );
    // And in a file of its own.
    cx.simulate_keystrokes("shift-f12");
    settle(cx);
    cx.simulate_keystrokes("enter");
    settle(cx);
    assert!(peek(&workbench, cx).is_none());
    let util = util();
    assert_eq!(
        at(&workbench, cx),
        (root.join("src/util.rs"), name(nth(&util, 0)))
    );
    assert_eq!(tabs(cx), open + 1);
}

#[gpui::test]
fn find_all_still_fills_panel(cx: &mut TestAppContext) {
    let (_dir, _root, workbench, cx) = opened(cx);
    cx.simulate_keystrokes("shift-alt-f12");
    settle(cx);
    assert!(peek(&workbench, cx).is_none(), "no peek");
    let dock = workbench.read_with(cx, |workbench, cx| {
        let dock = workbench.dock().read(cx);
        (dock.is_visible(), dock.active())
    });
    assert_eq!(dock, (true, DockPanel::References));
    let panel = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).references().clone()
    });
    assert_eq!(panel.read_with(cx, |panel, _| panel.shown_rows()), rows());
    let focused = cx.update(|window, cx| panel.focus_handle(cx).is_focused(window));
    assert!(focused, "the panel has focus");
}
