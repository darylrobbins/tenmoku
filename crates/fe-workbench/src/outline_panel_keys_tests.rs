//! The outline's keys, pressed through the default keymap: `outline.show`
//! focusing the panel on the caret's symbol, the keys moving the
//! selection, and Enter and `o` jumping to it as a click does.

use std::{fs, ops::Range, path::PathBuf};

use fe_gpui::{EditorView, set_vim_enabled};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use crate::{DockPanel, OutlinePanel, Workbench, tests::open_root};

const RUST: &str = "\
struct Point;

impl Point {
    fn new() -> Self {
        Point
    }
}

fn main() {}
";

const OTHER: &str = "fn one() {}\n\nfn two() {}\n";

/// The rows of [`RUST`]'s outline: `Point`, `impl Point`, `new`, `main`.
const ROWS: usize = 4;

/// `files` (name, contents) in a temporary directory, and their paths.
fn project(files: &[(&str, &str)]) -> (tempfile::TempDir, Vec<PathBuf>) {
    let dir = tempfile::tempdir().unwrap();
    let paths = (files.iter())
        .map(|(name, text)| {
            let path = dir.path().join(name);
            fs::write(&path, text).unwrap();
            path
        })
        .collect();
    (dir, paths)
}

/// A workbench rooted at `dir` with `path` open, its outline landed, and
/// the editor focused.
fn opened<'a>(
    cx: &'a mut TestAppContext,
    dir: &tempfile::TempDir,
    path: &std::path::Path,
) -> (
    Entity<Workbench>,
    Entity<OutlinePanel>,
    &'a mut VisualTestContext,
) {
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let outline = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).outline().clone()
    });
    open(&workbench, path, cx);
    let editor = editor(&workbench, cx);
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);
    (workbench, outline, cx)
}

fn open(workbench: &Entity<Workbench>, path: &std::path::Path, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path.to_owned(), window, cx)));
    draw(cx);
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    (workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned()))
        .expect("a file is shown")
}

/// `outline.show`, from its key.
fn show(cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("cmd-shift-o");
    draw(cx);
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

fn panel_focused(outline: &Entity<OutlinePanel>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| outline.focus_handle(cx).is_focused(window))
}

fn editor_focused(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

fn selected(outline: &Entity<OutlinePanel>, cx: &mut VisualTestContext) -> Option<usize> {
    outline.read_with(cx, |outline, _| outline.selected())
}

fn set_caret(editor: &Entity<EditorView>, offset: usize, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, cx| {
        editor.select_and_reveal(offset..offset, cx)
    });
}

fn selections(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<Range<usize>> {
    editor.read_with(cx, |editor, _| {
        (editor.model().selections().into_iter())
            .map(|selection| selection.range)
            .collect()
    })
}

/// Inserts `text` at `offset`, as typing does, the caret after it, without
/// running the executor.
fn insert(editor: &Entity<EditorView>, offset: usize, text: &str, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, cx| {
        editor.model_mut().set_cursor(offset);
        editor.model_mut().insert(text);
        cx.notify();
    });
}

/// A caret at `at`, as [`selections`] has it.
fn caret(at: usize) -> Vec<Range<usize>> {
    std::iter::once(at..at).collect()
}

/// Item `ix`'s name, in the outline shown.
fn name_range(
    outline: &Entity<OutlinePanel>,
    ix: usize,
    cx: &mut VisualTestContext,
) -> Range<usize> {
    outline.read_with(cx, |outline, _| {
        outline.outline().unwrap().items[ix].name_range.clone()
    })
}

/// Whether the dock shows the Explorer with the outline expanded.
fn outline_shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    workbench.read_with(cx, |workbench, cx| {
        let dock = workbench.dock().read(cx);
        let stack = dock.explorer().read(cx).stack().read(cx);
        dock.is_visible()
            && dock.active() == DockPanel::Explorer
            && stack.is_collapsed(super::VIEW) == Some(false)
    })
}

mod focus {
    use super::*;

    #[gpui::test]
    fn outline_show_selects_the_carets_symbol(cx: &mut TestAppContext) {
        let (dir, paths) = project(&[("a.rs", RUST)]);
        let (workbench, outline, cx) = opened(cx, &dir, &paths[0]);
        let editor = editor(&workbench, cx);
        assert_eq!(selected(&outline, cx), None);

        // Inside `new`'s body: `new`, not the `impl` around it.
        set_caret(&editor, RUST.find("Point\n    }").unwrap(), cx);
        show(cx);
        assert!(panel_focused(&outline, cx));
        assert_eq!(selected(&outline, cx), Some(2));

        // From the editor again, on `main`.
        cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
        set_caret(&editor, RUST.find("main").unwrap(), cx);
        show(cx);
        assert!(panel_focused(&outline, cx));
        assert_eq!(selected(&outline, cx), Some(3));

        // On the blank line between `impl Point` and `main`, in no symbol:
        // the first row.
        cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
        set_caret(&editor, RUST.find("\nfn main").unwrap(), cx);
        show(cx);
        assert!(panel_focused(&outline, cx));
        assert_eq!(selected(&outline, cx), Some(0));
    }

    #[gpui::test]
    fn outline_show_before_the_outline_lands(cx: &mut TestAppContext) {
        let (dir, paths) = project(&[("a.rs", RUST)]);
        let (workbench, cx) = open_root(cx, dir.path());
        cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
        let outline = workbench.read_with(cx, |workbench, cx| {
            workbench.dock().read(cx).outline().clone()
        });
        // Nothing open: focused, nothing to select.
        show(cx);
        assert!(panel_focused(&outline, cx));
        assert_eq!(selected(&outline, cx), None);

        // The file opens with its outline's job held; once it lands the
        // first row, the caret's symbol, is selected.
        outline.update(cx, |outline, _| outline.hold_jobs());
        open(&workbench, &paths[0], cx);
        assert!(outline.read_with(cx, |outline, _| outline.outline().is_none()));
        let editor = editor(&workbench, cx);
        set_caret(&editor, RUST.find("main").unwrap(), cx);
        show(cx);
        assert_eq!(selected(&outline, cx), None);
        outline.update(cx, |outline, _| outline.release_jobs());
        draw(cx);
        assert_eq!(selected(&outline, cx), Some(3));
        assert!(panel_focused(&outline, cx));
    }

    /// [`RUST`] with `fn added` appended, and the offset of its body.
    fn appended() -> (String, usize, usize) {
        let added = "fn added() {\n    1;\n}\n";
        let body = RUST.len() + added.find("1;").unwrap();
        (added.to_owned(), RUST.len(), body)
    }

    #[gpui::test]
    fn outline_show_over_a_stale_outline_waits_for_the_current_one(cx: &mut TestAppContext) {
        let (dir, paths) = project(&[("a.rs", RUST)]);
        let (workbench, outline, cx) = opened(cx, &dir, &paths[0]);
        let editor = editor(&workbench, cx);
        assert_eq!(
            outline.read_with(cx, |outline, _| outline.rows().len()),
            ROWS
        );

        // A function appended, its outline held: the caret is in it, past
        // every symbol of the outline shown.
        outline.update(cx, |outline, _| outline.hold_jobs());
        let (added, at, body) = appended();
        insert(&editor, at, &added, cx);
        set_caret(&editor, body, cx);
        show(cx);
        assert!(panel_focused(&outline, cx));
        assert_eq!(
            outline.read_with(cx, |outline, _| outline.rows().len()),
            ROWS
        );
        assert_eq!(
            selected(&outline, cx),
            Some(0),
            "the stale outline's answer"
        );

        // The current outline lands: the caret's symbol is selected, not the
        // first row kept.
        outline.update(cx, |outline, _| outline.release_jobs());
        draw(cx);
        let rows = outline.read_with(cx, |outline, _| outline.rows());
        assert_eq!(rows.len(), ROWS + 1);
        assert_eq!(selected(&outline, cx), Some(ROWS));
        assert_eq!(rows[ROWS], (0, "added".to_owned()));
    }

    #[gpui::test]
    fn outline_show_over_a_stale_first_outline_waits_for_the_current_one(cx: &mut TestAppContext) {
        let (dir, paths) = project(&[("a.rs", RUST)]);
        let (workbench, cx) = open_root(cx, dir.path());
        cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
        let outline = workbench.read_with(cx, |workbench, cx| {
            workbench.dock().read(cx).outline().clone()
        });
        // The first outline's job is held, then the text changes under it.
        outline.update(cx, |outline, _| outline.hold_jobs());
        open(&workbench, &paths[0], cx);
        assert_eq!(
            outline.read_with(cx, |outline, _| outline.jobs_started()),
            1
        );
        let editor = editor(&workbench, cx);
        let (added, at, body) = appended();
        insert(&editor, at, &added, cx);
        set_caret(&editor, body, cx);
        show(cx);
        assert_eq!(selected(&outline, cx), None);

        // The first job lands, of the text before the edit, then the
        // follow-up: the caret's symbol is selected from the last.
        outline.update(cx, |outline, _| outline.release_jobs());
        draw(cx);
        let rows = outline.read_with(cx, |outline, _| outline.rows());
        assert_eq!(rows.len(), ROWS + 1);
        assert!(outline.read_with(cx, |outline, _| outline.jobs_started()) >= 2);
        assert_eq!(selected(&outline, cx), Some(ROWS));
        assert!(panel_focused(&outline, cx));
    }

    #[gpui::test]
    fn a_panel_with_no_outline_ignores_the_keys(cx: &mut TestAppContext) {
        let (dir, paths) = project(&[
            ("notes.txt", "fn not_code() {}\n"),
            ("page.html", "<p>a</p>\n"),
        ]);
        let (workbench, cx) = open_root(cx, dir.path());
        cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
        let outline = workbench.read_with(cx, |workbench, cx| {
            workbench.dock().read(cx).outline().clone()
        });
        let keys = "down up enter j k g g shift-g o 3 j";
        for vim in [false, true] {
            cx.update(|_, cx| set_vim_enabled(cx, vim));
            // NoFile.
            show(cx);
            assert!(panel_focused(&outline, cx));
            press(keys, cx);
            assert_eq!(selected(&outline, cx), None);
            assert_eq!(
                outline
                    .read_with(cx, |outline, _| outline.message_text())
                    .as_deref(),
                Some("No file open")
            );
        }
        // PlainText, then Unsupported.
        for (path, message) in [
            (&paths[0], "No outline for plain text"),
            (&paths[1], "No outline for html"),
        ] {
            open(&workbench, path, cx);
            let editor = editor(&workbench, cx);
            for vim in [false, true] {
                cx.update(|_, cx| set_vim_enabled(cx, vim));
                set_caret(&editor, 1, cx);
                show(cx);
                assert!(panel_focused(&outline, cx));
                press(keys, cx);
                assert_eq!(selected(&outline, cx), None);
                assert!(panel_focused(&outline, cx), "nothing to jump to");
                assert_eq!(selections(&editor, cx), caret(1));
                assert_eq!(
                    outline
                        .read_with(cx, |outline, _| outline.message_text())
                        .as_deref(),
                    Some(message)
                );
            }
        }
    }
}

mod r#move {
    use super::*;

    #[gpui::test]
    fn the_arrows_move_and_clamp(cx: &mut TestAppContext) {
        let (dir, paths) = project(&[("a.rs", RUST)]);
        let (workbench, outline, cx) = opened(cx, &dir, &paths[0]);
        let rows = outline.read_with(cx, |outline, _| outline.rows());
        assert_eq!(rows.len(), ROWS);
        set_caret(&editor(&workbench, cx), 0, cx);
        show(cx);
        assert_eq!(selected(&outline, cx), Some(0));
        for expected in [1, 2, 3, 3] {
            press("down", cx);
            assert_eq!(selected(&outline, cx), Some(expected));
        }
        for expected in [2, 1, 0, 0] {
            press("up", cx);
            assert_eq!(selected(&outline, cx), Some(expected));
        }
        // With vim off, vim's keys aren't the panel's.
        press("j shift-g", cx);
        assert_eq!(selected(&outline, cx), Some(0));
        assert!(panel_focused(&outline, cx));
    }

    #[gpui::test]
    fn vims_keys_move_and_take_counts(cx: &mut TestAppContext) {
        let (dir, paths) = project(&[("a.rs", RUST)]);
        let (workbench, outline, cx) = opened(cx, &dir, &paths[0]);
        cx.update(|_, cx| set_vim_enabled(cx, true));
        set_caret(&editor(&workbench, cx), 0, cx);
        show(cx);
        assert_eq!(selected(&outline, cx), Some(0));
        for (keys, expected) in [
            ("j", 1),
            ("k", 0),
            ("k", 0),
            ("shift-g", 3),
            ("j", 3),
            ("g g", 0),
            ("down", 1),
            ("up", 0),
            // Counts.
            ("3 j", 3),
            ("2 k", 1),
            ("9 j", 3),
            ("1 0 k", 0),
            ("2 g g", 1),
            ("3 shift-g", 2),
            ("9 shift-g", 3),
            ("1 g g", 0),
            // A count is dropped by a key that isn't the panel's.
            ("3 x j", 1),
            ("0 j", 2),
        ] {
            press(keys, cx);
            assert_eq!(selected(&outline, cx), Some(expected), "{keys}");
            assert_eq!(
                outline.read_with(cx, |outline, _| outline.count()),
                None,
                "{keys}"
            );
        }
        assert!(panel_focused(&outline, cx));
    }

    #[gpui::test]
    fn the_selection_scrolls_into_view(cx: &mut TestAppContext) {
        let text: String = (0..200).map(|ix| format!("fn f{ix}() {{}}\n")).collect();
        let (dir, paths) = project(&[("long.rs", &text)]);
        let (workbench, outline, cx) = opened(cx, &dir, &paths[0]);
        cx.update(|_, cx| set_vim_enabled(cx, true));
        assert_eq!(
            outline.read_with(cx, |outline, _| outline.rows().len()),
            200
        );
        set_caret(&editor(&workbench, cx), 0, cx);
        show(cx);
        let visible = |ix: usize, cx: &mut VisualTestContext| {
            outline.read_with(cx, |outline, _| {
                let list = outline.list_bounds();
                let row = outline.row_bounds(ix).expect("drawn");
                row.top() >= list.top() && row.bottom() <= list.bottom()
            })
        };
        assert!(visible(0, cx));
        assert!(!visible(199, cx), "the list is shorter than the outline");
        for (keys, row) in [
            ("shift-g", 199),
            ("g g", 0),
            ("1 0 0 j", 100),
            ("down", 101),
        ] {
            press(keys, cx);
            assert_eq!(selected(&outline, cx), Some(row), "{keys}");
            assert!(visible(row, cx), "{keys}: row {row} scrolled into view");
        }
        assert!(!visible(0, cx));
    }

    #[gpui::test]
    fn the_selection_stays_on_its_symbol_across_refreshes(cx: &mut TestAppContext) {
        let (dir, paths) = project(&[("a.rs", RUST)]);
        let (workbench, outline, cx) = opened(cx, &dir, &paths[0]);
        let editor = editor(&workbench, cx);
        set_caret(&editor, RUST.find("main").unwrap(), cx);
        show(cx);
        assert_eq!(selected(&outline, cx), Some(3));
        // A symbol added above `main` moves it down a row.
        editor.update(cx, |editor, cx| {
            editor.model_mut().set_cursor(0);
            editor.model_mut().insert("fn first() {}\n");
            cx.notify();
        });
        draw(cx);
        let rows = outline.read_with(cx, |outline, _| outline.rows());
        assert_eq!(rows[0], (0, "first".to_owned()));
        assert_eq!(selected(&outline, cx), Some(4));
        assert_eq!(rows[4], (0, "main".to_owned()));
    }

    #[gpui::test]
    fn a_symbol_of_a_shared_name_keeps_the_selection(cx: &mut TestAppContext) {
        let text = "\
struct A;

impl A {
    fn new() {}
}

struct B;

impl B {
    fn new() {}
}
";
        let (dir, paths) = project(&[("a.rs", text)]);
        let (workbench, outline, cx) = opened(cx, &dir, &paths[0]);
        let editor = editor(&workbench, cx);
        let a_new = text.find("new").unwrap();
        let b_new = text.rfind("new").unwrap();
        set_caret(&editor, b_new, cx);
        show(cx);
        assert_eq!(selected(&outline, cx), Some(5));
        assert_eq!(name_range(&outline, 5, cx).start, b_new);

        // A comment above both as long as the distance between them puts
        // `A::new` where `B::new` was: the selection stays on `B::new`.
        let gap = b_new - a_new;
        let comment = format!("//{}\n", "x".repeat(gap - 3));
        assert_eq!(comment.len(), gap);
        insert(&editor, 0, &comment, cx);
        draw(cx);
        assert_eq!(
            name_range(&outline, 2, cx).start,
            b_new,
            "A::new moved there"
        );
        assert_eq!(selected(&outline, cx), Some(5));
        press("enter", cx);
        assert_eq!(selections(&editor, cx), caret(b_new + gap));
    }
}

mod jump {
    use super::*;

    /// Presses `keys` from the outline with row `ix` selected, and returns
    /// the editor's selections after.
    fn jump_with(
        keys: &str,
        ix: usize,
        workbench: &Entity<Workbench>,
        outline: &Entity<OutlinePanel>,
        cx: &mut VisualTestContext,
    ) -> Vec<Range<usize>> {
        let editor = editor(workbench, cx);
        cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
        set_caret(&editor, 0, cx);
        show(cx);
        for _ in 0..ix {
            press("down", cx);
        }
        assert_eq!(selected(outline, cx), Some(ix));
        press(keys, cx);
        assert!(editor_focused(&editor, cx), "{keys}");
        assert!(!panel_focused(outline, cx), "{keys}");
        assert!(outline_shown(workbench, cx), "{keys}: the panel stays");
        selections(&editor, cx)
    }

    #[gpui::test]
    fn enter_and_o_jump_as_a_click_does(cx: &mut TestAppContext) {
        let (dir, paths) = project(&[("a.rs", RUST)]);
        let (workbench, outline, cx) = opened(cx, &dir, &paths[0]);
        let editor = editor(&workbench, cx);
        show(cx);
        let name = name_range(&outline, 2, cx);
        assert_eq!(&RUST[name.clone()], "new");

        // The click, the third row.
        set_caret(&editor, 0, cx);
        cx.update(|window, cx| window.blur(cx));
        let bounds = outline.read_with(cx, |outline, _| outline.row_bounds(2).expect("drawn"));
        cx.simulate_click(
            bounds.origin + gpui::point(gpui::px(30.), bounds.size.height / 2.),
            Modifiers::none(),
        );
        draw(cx);
        assert!(editor_focused(&editor, cx));
        let clicked = selections(&editor, cx);
        assert_eq!(clicked, caret(name.start));

        assert_eq!(jump_with("enter", 2, &workbench, &outline, cx), clicked);
        cx.update(|_, cx| set_vim_enabled(cx, true));
        assert_eq!(jump_with("enter", 2, &workbench, &outline, cx), clicked);
        assert_eq!(jump_with("o", 2, &workbench, &outline, cx), clicked);
        let main = name_range(&outline, 3, cx);
        assert_eq!(
            jump_with("o", 3, &workbench, &outline, cx),
            caret(main.start)
        );
    }

    #[gpui::test]
    fn a_key_after_the_outline_is_replaced_does_not_land(cx: &mut TestAppContext) {
        let (dir, paths) = project(&[("a.rs", RUST), ("b.rs", OTHER)]);
        let (workbench, outline, cx) = opened(cx, &dir, &paths[0]);
        let a = editor(&workbench, cx);
        open(&workbench, &paths[1], cx);
        let b = editor(&workbench, cx);
        let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
        let activate = |ix: usize, cx: &mut VisualTestContext| {
            cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(ix, window, cx)));
        };
        activate(0, cx);
        draw(cx);
        set_caret(&a, 0, cx);
        set_caret(&b, 0, cx);
        show(cx);
        press("down down", cx);
        assert_eq!(selected(&outline, cx), Some(2));

        // The other tab becomes active, its outline not landed yet, while
        // the panel keeps focus: Enter lands in neither editor.
        outline.update(cx, |outline, _| outline.hold_jobs());
        activate(1, cx);
        let focus = outline.read_with(cx, |outline, cx| outline.focus_handle(cx));
        cx.update(|window, cx| window.focus(&focus, cx));
        press("enter", cx);
        assert!(panel_focused(&outline, cx));
        assert_eq!(selected(&outline, cx), None);
        assert_eq!(selections(&a, cx), caret(0));
        assert_eq!(selections(&b, cx), caret(0));

        // Once it lands, nothing is selected in it until a key moves.
        outline.update(cx, |outline, _| outline.release_jobs());
        draw(cx);
        assert_eq!(outline.read_with(cx, |outline, _| outline.rows().len()), 2);
        press("enter", cx);
        assert_eq!(selections(&b, cx), caret(0));
        assert!(panel_focused(&outline, cx));
        press("down down enter", cx);
        let two = name_range(&outline, 1, cx);
        assert_eq!(&OTHER[two.clone()], "two");
        assert_eq!(selections(&b, cx), caret(two.start));
        assert!(editor_focused(&b, cx));
        assert_eq!(selections(&a, cx), caret(0));
    }
}
