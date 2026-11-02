//! Quick-scope's underlines in a headless window: which targets each
//! `vim.quick_scope` mode marks, in which colour, where, and only on rows
//! the frame shows.

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_text::Bias;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext, px, size};

use super::*;
use crate::{EditorTheme, init, set_vim_enabled, set_vim_quick_scope};

/// A row with words either side of `baz`, where the tests put the caret.
const ROW: &str = "foo bar baz qux";

/// An editor on `text`, set up by `setup`, focused with vim on, drawn.
fn editor<'a>(
    text: &str,
    setup: impl FnOnce(&mut EditorModel) + 'static,
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let text = text.to_string();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), None);
        setup(&mut model);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();
    draw(cx);
    (view, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// Types `keys`. No explicit draw: a key has to ask for the frame itself.
fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
}

fn set(mode: VimQuickScope, cx: &mut VisualTestContext) {
    cx.update(|_, cx| set_vim_quick_scope(cx, mode));
    cx.run_until_parked();
}

/// The last frame's marks as laid out, after checking it painted just those.
fn marks(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<Mark> {
    let placed = view.read_with(cx, |view, _| {
        view.layout.as_ref().expect("drawn").quick_scope.clone()
    });
    assert_eq!(PAINTED.with(|painted| painted.borrow().clone()), placed);
    placed
}

/// Each mark's offset and whether it is in the primary colour, after
/// checking it is in one of the two.
fn targets(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<(usize, bool)> {
    let (primary, secondary) = view.read_with(cx, |view, _| {
        (
            view.theme.quick_scope_primary,
            view.theme.quick_scope_secondary,
        )
    });
    assert_ne!(primary, secondary);
    (marks(view, cx).into_iter())
        .map(|mark| {
            assert!(mark.color == primary || mark.color == secondary);
            (mark.offset, mark.color == primary)
        })
        .collect()
}

/// What the vim crate lists for the caret, `forward`, as marks would have it.
fn listed(
    view: &Entity<EditorView>,
    caret: usize,
    forward: bool,
    cx: &mut VisualTestContext,
) -> Vec<(usize, bool)> {
    view.read_with(cx, |view, _| {
        fe_vim::quick_scope::targets(view.model().text(), caret, forward)
            .into_iter()
            .map(|target| (target.offset, target.primary))
            .collect()
    })
}

fn caret_at(offset: usize) -> impl FnOnce(&mut EditorModel) {
    move |model| {
        let offset = model.text().clip_offset(offset, Bias::Left);
        model.set_selections([fe_buffer::ResolvedSelection {
            range: offset..offset,
            reversed: false,
        }]);
    }
}

#[gpui::test]
fn marks_follow_the_mode(cx: &mut TestAppContext) {
    // The caret on `b` of `baz`: `bar` is behind it, `qux` ahead.
    let (view, cx) = editor(ROW, caret_at(8), cx);
    let backward = listed(&view, 8, false, cx);
    let forward = listed(&view, 8, true, cx);
    assert!(!backward.is_empty() && !forward.is_empty());
    let both: Vec<_> = backward.iter().chain(&forward).copied().collect();

    // on_keys is the default: nothing until a find waits.
    assert_eq!(targets(&view, cx), []);
    press("f", cx);
    assert_eq!(targets(&view, cx), forward);
    press("escape", cx);
    assert_eq!(targets(&view, cx), []);
    press("shift-f", cx);
    assert_eq!(targets(&view, cx), backward);
    press("escape", cx);
    press("t", cx);
    assert_eq!(targets(&view, cx), forward);
    press("escape", cx);
    press("shift-t", cx);
    assert_eq!(targets(&view, cx), backward);
    press("escape", cx);
    // After an operator too.
    press("d f", cx);
    assert_eq!(targets(&view, cx), forward);
    press("escape", cx);

    // Gone once the char is typed: the caret is on `q`.
    press("f q", cx);
    assert_eq!(targets(&view, cx), []);
    press("0 w w", cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.model().primary_selection().head()),
        8
    );

    // always: both colours, both ways, and narrowed while a find waits.
    set(VimQuickScope::Always, cx);
    let marked = targets(&view, cx);
    assert_eq!(marked, both);
    assert!(marked.iter().any(|&(_, primary)| primary));
    press("f", cx);
    assert_eq!(targets(&view, cx), forward);
    press("escape", cx);
    press("shift-f", cx);
    assert_eq!(targets(&view, cx), backward);
    press("escape", cx);

    // In a visual mode, from the vim cursor, not the selection's head.
    press("v", cx);
    assert_eq!(targets(&view, cx), both);
    press("escape", cx);
    press("shift-v", cx);
    assert_eq!(targets(&view, cx), both);
    press("escape", cx);

    // Insert and replace mode: none, even with always.
    press("i", cx);
    assert_eq!(targets(&view, cx), []);
    press("escape", cx);
    press("shift-r", cx);
    assert_eq!(targets(&view, cx), []);
    press("escape", cx);
    press("l", cx);
    assert!(!targets(&view, cx).is_empty());

    // off: none, even while a find waits.
    set(VimQuickScope::Off, cx);
    assert_eq!(targets(&view, cx), []);
    press("f", cx);
    assert_eq!(targets(&view, cx), []);
    press("escape", cx);

    // Vim off: none.
    set(VimQuickScope::Always, cx);
    assert!(!targets(&view, cx).is_empty());
    cx.update(|_, cx| set_vim_enabled(cx, false));
    cx.run_until_parked();
    draw(cx);
    assert_eq!(targets(&view, cx), []);
}

#[gpui::test]
fn secondary_targets_take_the_secondary_colour(cx: &mut TestAppContext) {
    // Forward from the caret, `aa` has `a` seen once; `ab` then has `a` seen
    // twice and `b` once, so `b` is primary; `ab` again has `a` thrice but
    // `b` twice, so its `b` is secondary.
    let (view, cx) = editor("x aa ab ab", caret_at(0), cx);
    set(VimQuickScope::Always, cx);
    assert_eq!(targets(&view, cx), [(2, true), (6, true), (9, false)]);
}

#[gpui::test]
fn an_unfocused_editor_has_none(cx: &mut TestAppContext) {
    let (view, cx) = editor(ROW, caret_at(8), cx);
    set(VimQuickScope::Always, cx);
    assert!(!targets(&view, cx).is_empty());
    cx.update(|window, cx| window.blur(cx));
    cx.run_until_parked();
    assert_eq!(targets(&view, cx), []);
}

#[gpui::test]
fn the_visual_caret_is_the_vim_cursor(cx: &mut TestAppContext) {
    // The model's head is past the `b` the vim cursor is on: from the head
    // `b` would be counted as a target on the backward side.
    let (view, cx) = editor(ROW, caret_at(8), cx);
    set(VimQuickScope::Always, cx);
    press("v", cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.model().primary_selection().head()),
        9
    );
    let want: Vec<_> = (listed(&view, 8, false, cx).into_iter())
        .chain(listed(&view, 8, true, cx))
        .collect();
    assert_eq!(targets(&view, cx), want);
}

#[gpui::test]
fn each_bar_sits_along_its_cell_s_bottom(cx: &mut TestAppContext) {
    let (view, cx) = editor(ROW, caret_at(8), cx);
    set(VimQuickScope::Always, cx);
    let marks = marks(&view, cx);
    assert!(!marks.is_empty());
    for mark in marks {
        let cell = view.update(cx, |view, _| {
            let text = view.model.text().clone();
            let layout = view.layout.as_mut().unwrap();
            let cell = block_rect(layout, &text, mark.offset).unwrap();
            layout.bounds(cell)
        });
        assert_eq!(mark.bounds.size.height, px(HEIGHT));
        assert_eq!(mark.bounds.size.width, cell.size.width);
        assert_eq!(mark.bounds.left(), cell.left());
        assert_eq!(mark.bounds.bottom(), cell.bottom());
    }
}

#[gpui::test]
fn a_wrapped_row_s_targets_land_on_their_display_rows(cx: &mut TestAppContext) {
    // Distinct words, so each piece of the wrapped row has some targets.
    let words: Vec<String> = (0..120).map(|i| format!("w{i:03}z")).collect();
    let text = words.join(" ");
    let (view, cx) = editor(&text, |model| model.set_soft_wrap(true), cx);
    cx.simulate_resize(size(px(400.), px(400.)));
    cx.run_until_parked();
    draw(cx);
    set(VimQuickScope::Always, cx);
    let marks = marks(&view, cx);
    let rows = view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        let frame = &layout.frame;
        assert!(frame.rows.len() > 2, "the row wraps");
        let mut seen = std::collections::BTreeSet::new();
        for mark in &marks {
            let row = frame.row_containing(mark.offset).expect("a shown row");
            let top = layout.bounds(Rect {
                x: 0.,
                y: frame.row_top(row.row),
                width: 1.,
                height: frame.line_height,
            });
            assert_eq!(mark.bounds.bottom(), top.bottom(), "{}", mark.offset);
            seen.insert(row.row);
        }
        seen.len()
    });
    assert!(rows > 1, "marks on {rows} display rows");
}

#[gpui::test]
fn a_folded_row_has_none(cx: &mut TestAppContext) {
    // Rows 2 to 3 fold under row 1; the caret goes on row 2.
    let body = "fn f() {\n  foo bar\n  baz qux\n}\n";
    let at = body.find("baz").unwrap();
    let tail = "p q\n".repeat(500);
    let text = format!("{body}{tail}");
    let (view, cx) = editor(
        &text,
        move |model| {
            model.fold_rows(0..=3);
            caret_at(at)(model);
        },
        cx,
    );
    set(VimQuickScope::Always, cx);
    let shown = view.read_with(cx, |view, _| {
        let frame = &view.layout.as_ref().unwrap().frame;
        frame
            .rows
            .iter()
            .map(|row| row.buffer_row)
            .collect::<Vec<_>>()
    });
    assert!(!shown.contains(&2), "{shown:?}");
    assert_eq!(targets(&view, cx), []);
}

#[gpui::test]
fn a_row_scrolled_off_has_none(cx: &mut TestAppContext) {
    // The caret left behind.
    let (view, cx) = editor(&"p q\n".repeat(500), caret_at(0), cx);
    set(VimQuickScope::Always, cx);
    assert!(!targets(&view, cx).is_empty());
    let line_height = view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().frame.line_height
    });
    view.update(cx, |view, _| {
        view.scroll.y = px(line_height * 100.);
        view.autoscroll = false;
    });
    draw(cx);
    assert_eq!(targets(&view, cx), []);
}

#[gpui::test]
fn a_target_scrolled_off_to_the_side_has_none(cx: &mut TestAppContext) {
    // A row wider than the window, targets past its right edge and, once
    // scrolled, past its left.
    let words: Vec<String> = (0..40).map(|i| format!("w{i:03}z")).collect();
    let (view, cx) = editor(&words.join(" "), caret_at(0), cx);
    cx.simulate_resize(size(px(400.), px(400.)));
    cx.run_until_parked();
    set(VimQuickScope::Always, cx);
    let listed = listed(&view, 0, true, cx);
    let shown = targets(&view, cx);
    let (first, last) = (shown[0], *shown.last().unwrap());
    assert_eq!(first, listed[0]);
    assert_ne!(
        last,
        *listed.last().unwrap(),
        "the right edge cuts them off"
    );

    let past_first = view.update(cx, |view, _| {
        let text = view.model.text().clone();
        let layout = view.layout.as_mut().unwrap();
        let cell = block_rect(layout, &text, first.0).unwrap();
        let visible = layout.frame.text_bounds();
        cell.x + cell.width - visible.x + cell.width
    });
    view.update(cx, |view, _| {
        view.scroll.x = px(past_first);
        view.autoscroll = false;
    });
    draw(cx);
    let scrolled = targets(&view, cx);
    assert!(!scrolled.is_empty());
    assert!(!scrolled.contains(&first), "{scrolled:?}");
    assert!(
        scrolled.len() > 1 && scrolled.contains(&last),
        "{scrolled:?}"
    );
}

#[gpui::test]
fn a_pending_jump_takes_the_row(cx: &mut TestAppContext) {
    let (view, cx) = editor(ROW, caret_at(8), cx);
    set(VimQuickScope::Always, cx);
    assert!(!targets(&view, cx).is_empty());
    press("g s", cx);
    assert!(view.read_with(cx, |view, _| view.layout.as_ref().unwrap().jump.is_some()));
    assert_eq!(targets(&view, cx), []);
}

#[gpui::test]
fn an_open_command_line_has_none(cx: &mut TestAppContext) {
    let (view, cx) = editor(ROW, caret_at(8), cx);
    set(VimQuickScope::Always, cx);
    press("/", cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.vim_mode()),
        Some("cmdline")
    );
    assert_eq!(targets(&view, cx), []);
}
