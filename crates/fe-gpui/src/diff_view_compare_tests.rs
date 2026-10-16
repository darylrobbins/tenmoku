//! A [`DiffView`]'s sides compare against each other: each side's row
//! changes, change bars and vim's `]c` / `[c` follow the diff, an editable
//! right side's too, while the file's editor keeps comparing against
//! `HEAD`; its commands are in the app's catalog and palette; and an
//! editor and a diff view over one buffer each know the other shows it.

use fe_buffer::Buffer;
use fe_command::CommandId;
use fe_editor::row_changes::{RowChange, RowChangeKind};
use gpui::{TestAppContext, VisualTestContext};
use kiln_shell::{ItemPane, PaneView as _};

use super::*;
use crate::{EditorTheme, commands, init};

/// `count` rows `r0` on, but row 5 `<prefix>5` and, after row 7, rows
/// `extra` when given.
fn rows(count: usize, prefix: &str, row2: &str, extra: bool) -> String {
    let mut rows: Vec<String> = (0..count).map(|row| format!("r{row}")).collect();
    rows[2] = row2.into();
    rows[5] = format!("{prefix}5");
    if extra {
        rows.insert(8, "extra".into());
    }
    rows.join("\n")
}

/// `HEAD`'s text, the working tree's and the index's: the working tree
/// changes row 2 from `HEAD`, the index row 5 and has a row the working
/// tree lacks after row 7.
fn texts(count: usize) -> (String, String, String) {
    (
        rows(count, "r", "old2", false),
        rows(count, "r", "r2", false),
        rows(count, "idx", "r2", true),
    )
}

fn changes(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> Vec<(Range<u32>, RowChangeKind)> {
    view.update(cx, |view, _| {
        let model = view.model_mut();
        model.sync_display();
        (model.row_changes().iter())
            .map(|RowChange { rows, kind }| (rows.clone(), *kind))
            .collect()
    })
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn caret(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> u32 {
    view.read_with(cx, |view, _| caret_row(view.model()))
}

/// The file's editor over `working`, its base `head`, and a focused diff
/// view of `index`, read-only, against a split of it, drawn.
fn opened<'a>(
    head: &str,
    working: &str,
    index: &str,
    cx: &'a mut TestAppContext,
) -> (
    Entity<EditorView>,
    Entity<DiffView>,
    &'a mut VisualTestContext,
) {
    cx.update(init);
    let mut file = EditorModel::new(Buffer::new(working), Some("a.rs".into()));
    file.set_diff_base(Some(head.into()));
    let right = file.split();
    let index = EditorModel::new(Buffer::new(index), None);
    let (diff, cx) = cx.add_window_view(move |_, cx| {
        let left = DiffSide::new("Index", index).read_only();
        DiffView::new("a.rs", left, DiffSide::new("Working Tree", right), cx)
    });
    let editor =
        cx.update(|_, cx| cx.new(|cx| EditorView::new(file, EditorTheme::default_dark(cx), cx)));
    cx.update(|window, cx| window.focus(&diff.focus_handle(cx), cx));
    draw(cx);
    (editor, diff, cx)
}

fn sides(diff: &Entity<DiffView>, cx: &mut VisualTestContext) -> [Entity<EditorView>; 2] {
    diff.read_with(cx, |diff, _| diff.views.clone())
}

#[gpui::test]
fn editable_side_compares(app: &mut TestAppContext) {
    use RowChangeKind::*;
    // Small, then over 1.5 MiB, diffed off the main thread.
    for count in [20, 250_000] {
        let (head, working, index) = texts(count);
        assert_eq!(working.len() > ASYNC_BYTES * 3 / 2, count > 20);
        let (editor, diff, cx) = opened(&head, &working, &index, app);
        let [left, right] = sides(&diff, cx);
        // Each side against the other's text.
        assert_eq!(changes(&right, cx), [(5..6, Modified), (8..8, Deleted)]);
        assert_eq!(changes(&left, cx), [(5..6, Modified), (8..9, Added)]);
        // The file's editor still against `HEAD`.
        assert_eq!(changes(&editor, cx), [(2..3, Modified)]);

        // A row typed at the top of the right side.
        right.update(cx, |view, cx| {
            let model = view.model_mut();
            model.set_cursor(0);
            model.insert("N\n");
            cx.notify();
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        if count > 20 {
            // The last changes until the new diff lands.
            assert_eq!(changes(&right, cx), [(5..6, Modified), (8..8, Deleted)]);
        }
        cx.run_until_parked();
        let right_changes = [(0..1, Added), (6..7, Modified), (9..9, Deleted)];
        assert_eq!(changes(&right, cx), right_changes);
        let left_changes = [(0..0, Deleted), (5..6, Modified), (8..9, Added)];
        assert_eq!(changes(&left, cx), left_changes);
        assert_eq!(changes(&editor, cx), [(0..1, Added), (3..4, Modified)]);
    }

    // Vim's change navigation on the right goes to the diff's hunks, not
    // `HEAD`'s, side by side and inline.
    let (head, working, index) = texts(20);
    let (_, diff, cx) = opened(&head, &working, &index, app);
    let right = sides(&diff, cx)[1].clone();
    cx.update(|_, cx| crate::set_vim_enabled(cx, true));
    for inline in [false, true] {
        cx.update(|window, cx| diff.update(cx, |diff, cx| diff.set_inline(inline, window, cx)));
        draw(cx);
        right.update(cx, |view, _| view.model_mut().set_cursor(0));
        cx.simulate_keystrokes("] c");
        assert_eq!(caret(&right, cx), 5, "inline: {inline}");
        cx.simulate_keystrokes("] c");
        assert_eq!(caret(&right, cx), 8, "inline: {inline}");
        cx.simulate_keystrokes("[ c");
        assert_eq!(caret(&right, cx), 5, "inline: {inline}");
    }
}

#[gpui::test]
fn catalog(app: &mut TestAppContext) {
    let catalog = commands::command_catalog();
    for id in [NEXT_CHANGE, PREVIOUS_CHANGE, TOGGLE_INLINE] {
        let command = CommandId::parse(id).unwrap();
        assert_eq!(catalog.check(&command, &Value::Null), Ok(()), "{id}");
    }
    let list = commands::command_list();
    let title = |id: &str| {
        (list.iter())
            .find(|meta| meta.id.as_str() == id)
            .map(|meta| meta.title.clone())
    };
    assert_eq!(title(NEXT_CHANGE).as_deref(), Some("Go to Next Difference"));
    assert_eq!(
        title(PREVIOUS_CHANGE).as_deref(),
        Some("Go to Previous Difference")
    );
    assert_eq!(title(TOGGLE_INLINE).as_deref(), Some("Toggle Inline Diff"));

    // A user keymap binding f7 to the next change, checked against it.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keymap.toml");
    let user = format!(
        r#"
        [[bindings]]
        keys = "f7"
        command = "{NEXT_CHANGE}"
        context = "{KEY_CONTEXT}"
        "#
    );
    std::fs::write(&path, user).unwrap();
    let store = fe_keymap::KeymapStore::load(
        fe_keymap::DEFAULT_MACOS,
        Some(path.clone()),
        Some(commands::command_catalog()),
    );
    let theirs: Vec<_> = (store.diagnostics().iter())
        .filter(|diagnostic| diagnostic.file == path)
        .collect();
    assert!(theirs.is_empty(), "{theirs:?}");
    let id = CommandId::parse(NEXT_CHANGE).unwrap();
    assert_eq!(store.keymap().bindings_for(&id).len(), 1);

    let (head, working, index) = texts(20);
    let (_, diff, cx) = opened(&head, &working, &index, app);
    let keymap = fe_keymap::Keymap::clone(store.keymap());
    cx.update(|_, cx| crate::set_user_keymap(cx, keymap));
    let right = sides(&diff, cx)[1].clone();
    cx.simulate_keystrokes("f7");
    draw(cx);
    assert_eq!(caret(&right, cx), 5);
    cx.simulate_keystrokes("f7");
    draw(cx);
    assert_eq!(caret(&right, cx), 8);
}

/// Whether `a` says it shows `b`'s document.
fn shares(a: &AnyView, b: &AnyView, cx: &mut VisualTestContext) -> bool {
    cx.update(|_, cx| {
        if let Ok(editor) = a.clone().downcast::<EditorView>() {
            return editor.read(cx).shares_document(b, cx);
        }
        let diff = a
            .clone()
            .downcast::<DiffView>()
            .expect("an editor or a diff view");
        diff.read(cx).shares_document(b, cx)
    })
}

#[gpui::test]
fn shares_both_ways(app: &mut TestAppContext) {
    let (head, working, index) = texts(20);
    let (editor, diff, cx) = opened(&head, &working, &index, app);
    let other = cx.update(|_, cx| {
        let left = EditorModel::new(Buffer::new("x"), None);
        let right = EditorModel::new(Buffer::new("y"), Some("b.rs".into()));
        cx.new(|cx| {
            let left = DiffSide::new("Index", left).read_only();
            DiffView::new("b.rs", left, DiffSide::new("Working Tree", right), cx)
        })
    });
    let (editor_view, diff_view, other_view): (AnyView, AnyView, AnyView) = (
        editor.clone().into(),
        diff.clone().into(),
        other.clone().into(),
    );
    assert!(shares(&editor_view, &diff_view, cx));
    assert!(shares(&diff_view, &editor_view, cx));
    assert!(!shares(&editor_view, &other_view, cx));
    assert!(!shares(&other_view, &editor_view, cx));

    // The buffer dirty, either closes without asking while the other
    // shows it; the survivor asks once.
    editor.update(cx, |view, cx| {
        view.model_mut().insert("dirty ");
        cx.notify();
    });
    assert!(diff.read_with(cx, |diff, cx| diff.is_dirty(cx)));
    for first in [0, 1] {
        let pane = cx.update(|_, cx| cx.new(|cx| ItemPane::new(Default::default(), cx)));
        pane.update(cx, |pane, cx| {
            pane.add_item(Box::new(editor.clone()), cx);
            pane.add_item(Box::new(diff.clone()), cx);
        });
        let close = |ix: usize, cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                pane.update(cx, |pane, cx| {
                    let at = (pane.pane().items().iter())
                        .position(|item| {
                            item.to_any_view().entity_id()
                                == [editor.entity_id(), diff.entity_id()][ix]
                        })
                        .expect("in the pane");
                    pane.activate_item(at, window, cx);
                    pane.close_active_item(window, cx);
                })
            });
            cx.run_until_parked();
        };
        close(first, cx);
        assert!(!cx.has_pending_prompt(), "closing {first} first");
        assert_eq!(pane.read_with(cx, |pane, _| pane.pane().items().len()), 1);
        close(1 - first, cx);
        assert!(cx.has_pending_prompt(), "closing {} last", 1 - first);
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert!(!cx.has_pending_prompt(), "asks once");
        assert_eq!(pane.read_with(cx, |pane, _| pane.pane().items().len()), 1);
    }
}

/// T118 F3: navigating inline to the second of three hunks, then back to
/// side by side, the left side's untouched caret at offset 0 counts from
/// itself, not from the hunk inline navigation went to.
#[gpui::test]
fn inline_navigation_leaves_no_left_caret(app: &mut TestAppContext) {
    let left: Vec<String> = (0..30).map(|row| format!("r{row}")).collect();
    let mut right = left.clone();
    for row in [5, 15, 25] {
        right[row] = format!("changed{row}");
    }
    let (_, diff, cx) = opened(&left.join("\n"), &right.join("\n"), &left.join("\n"), app);
    let [left, right] = sides(&diff, cx);
    let call = |forward: bool, cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            diff.update(cx, |diff, cx| match forward {
                true => diff.next_change(window, cx),
                false => diff.previous_change(window, cx),
            })
        });
        draw(cx);
    };
    for forward in [true, false] {
        cx.update(|window, cx| diff.update(cx, |diff, cx| diff.set_inline(true, window, cx)));
        right.update(cx, |view, _| view.model_mut().set_cursor(0));
        call(true, cx);
        call(true, cx);
        assert_eq!(caret(&right, cx), 15);
        cx.update(|window, cx| diff.update(cx, |diff, cx| diff.set_inline(false, window, cx)));
        left.update(cx, |view, _| view.model_mut().set_cursor(0));
        cx.update(|window, cx| window.focus(&left.focus_handle(cx), cx));
        draw(cx);
        call(forward, cx);
        // From row 0: on to the first hunk, or back, wrapping, to the last.
        let expected = if forward { 5 } else { 25 };
        assert_eq!(caret(&left, cx), expected, "forward: {forward}");
        let wrapped = diff.read_with(cx, |diff, _| diff.message().cloned());
        assert_eq!(wrapped.is_some(), !forward);
    }
}
