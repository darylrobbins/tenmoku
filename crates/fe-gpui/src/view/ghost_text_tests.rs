//! Ghost text ([`super::ghost_text`]) in a headless window: its first line
//! dimmed inline after the caret, its other lines in rows below the caret's,
//! typing its next char consuming it, and anything else that ends it, or
//! vim leaving insert mode, dropping it.

use fe_buffer::{Buffer, Origin};
use fe_display::layout::Rect;
use fe_editor::{EditorModel, ghost_text::GhostText};
use gpui::{
    Bounds, Entity, Focusable as _, Modifiers, MouseButton, Pixels, Point, TestAppContext,
    VisualTestContext, px,
};

use crate::*;

/// The caret is before the `;`.
const TEXT: &str = "foo;\nnext\nlast\n";
const CARET: usize = 3;

/// A focused editor on [`TEXT`], vim off, the caret at [`CARET`].
fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(init);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let mut model = EditorModel::new(Buffer::new(TEXT), None);
        model.set_cursor(CARET);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.update(|_, cx| set_vim_enabled(cx, false));
    cx.run_until_parked();
    (view, cx)
}

fn ai() -> Origin {
    Origin::Ai {
        agent: "agent".into(),
        session: "session".into(),
    }
}

/// Suggests `text` at the view's caret.
fn suggest(view: &Entity<EditorView>, text: &str, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        let model = view.model();
        let at = model.primary_selection().head();
        let ghost = GhostText::new(model.snapshot(), at, text, ai());
        view.set_ghost_text(Some(ghost), cx);
    });
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

type Painted = (String, Vec<String>, Bounds<Pixels>);

/// The ghost text a fresh frame paints.
fn painted(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<Painted> {
    draw(cx);
    view.read_with(cx, |view, _| view.painted_ghost.clone())
}

/// Its inline part and block rows a fresh frame paints.
fn texts(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<(String, Vec<String>)> {
    painted(view, cx).map(|(inline, rows, _)| (inline, rows))
}

fn shown(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    view.read_with(cx, |view, _| view.ghost_text().is_some())
}

fn text(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.model().text().to_string())
}

fn head(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> usize {
    view.read_with(cx, |view, _| view.model().primary_selection().head())
}

/// The window bounds of a caret at `offset` in the last frame.
fn caret_at(
    view: &Entity<EditorView>,
    offset: usize,
    cx: &mut VisualTestContext,
) -> Bounds<Pixels> {
    view.update(cx, |view, _| {
        view.layout.as_mut().unwrap().caret_bounds(offset).unwrap()
    })
}

/// The window bounds of display row `row` across the text in the last
/// frame.
fn row_bounds(view: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        let frame = &layout.frame;
        layout.bounds(Rect {
            x: frame.text_left,
            y: frame.row_top(row),
            width: frame.width - frame.text_left,
            height: frame.line_height,
        })
    })
}

/// A little into the text on display row `row`.
fn on_row(view: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) -> Point<Pixels> {
    let bounds = row_bounds(view, row, cx);
    gpui::point(bounds.left() + px(3.), bounds.center().y)
}

fn click(at: Point<Pixels>, cx: &mut VisualTestContext) {
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
}

#[gpui::test]
fn inline_after_the_caret(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    assert_eq!(painted(&view, cx), None);
    let caret = caret_at(&view, CARET, cx);
    let semicolon_end = caret_at(&view, CARET + 1, cx);

    suggest(&view, "bar()", cx);
    let (inline, rows, bounds) = painted(&view, cx).expect("shown");
    assert_eq!((inline.as_str(), rows), ("bar()", vec![]));
    // After the caret, which stays where it was, on its row.
    assert_eq!(caret_at(&view, CARET, cx), caret);
    assert_eq!(bounds.origin, caret.origin);
    assert!(bounds.size.width > px(0.));
    let carets = view.read_with(cx, |view, _| view.painted_carets.clone());
    assert_eq!(carets.len(), 1);
    assert_eq!(carets[0].origin, caret.origin, "the caret paints before it");
    // `;` moved right by its width.
    let moved = caret_at(&view, CARET + 1, cx);
    let shift = moved.origin.x - semicolon_end.origin.x;
    assert!((shift - bounds.size.width).abs() < px(0.01), "{shift:?}");
    assert_eq!(moved.origin.y, semicolon_end.origin.y);
    // Dimmed: in the gutter's colour, the `;` after it in the text's.
    let colors = view.read_with(cx, |view, _| {
        let row = &view.layout.as_ref().unwrap().frame.rows[0];
        let inlay = row.expanded.inlay_range().unwrap();
        assert_eq!(&row.expanded.text()[inlay.clone()], "bar()");
        let theme = &view.theme;
        let (inlay, after) = (row.line.color_at(inlay.start), row.line.color_at(inlay.end));
        (
            inlay == Some(theme.gutter_foreground),
            after == Some(theme.foreground),
        )
    });
    assert_eq!(colors, (true, true));
    // Not in the buffer, and not underlined as composed text is.
    assert_eq!(text(&view, cx), TEXT);
    assert_eq!(view.read_with(cx, |view, _| view.painted_composition), None);

    view.update(cx, |view, cx| view.set_ghost_text(None, cx));
    assert_eq!(painted(&view, cx), None);
    assert_eq!(caret_at(&view, CARET + 1, cx), semicolon_end);
}

#[gpui::test]
fn extra_lines_as_rows_below(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    draw(cx);
    let line_height = row_bounds(&view, 0, cx).size.height;
    let next_top = row_bounds(&view, 1, cx).top();

    suggest(&view, "bar(\n\tx\n)", cx);
    assert_eq!(
        texts(&view, cx),
        Some(("bar(".into(), vec!["    x".into(), ")".into()]))
    );
    // The rows go under the caret's, pushing the rows below down.
    assert_eq!(row_bounds(&view, 0, cx).bottom(), next_top);
    assert_eq!(
        row_bounds(&view, 1, cx).top(),
        next_top + line_height * 2.,
        "row 1 is under the suggestion's 2 rows"
    );
    let blocks = view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        let blocks = layout.frame.blocks.iter();
        blocks
            .map(|block| (block.rows, layout.bounds(block.rect).top()))
            .collect::<Vec<_>>()
    });
    assert_eq!(blocks, [(2, next_top)]);
    assert_eq!(text(&view, cx), TEXT);

    // A click on the row below them puts the caret on that row.
    click(on_row(&view, 1, cx), cx);
    assert_eq!(head(&view, cx), TEXT.find("next").unwrap());
    assert!(!shown(&view, cx), "the caret moved away");
    assert_eq!(painted(&view, cx), None);
    assert_eq!(row_bounds(&view, 1, cx).top(), next_top);

    // At the end of the text, the rows go after the last row.
    view.update(cx, |view, _| view.model_mut().set_cursor(TEXT.len()));
    suggest(&view, "a\nb", cx);
    assert_eq!(texts(&view, cx), Some(("a".into(), vec!["b".into()])));
    let last = row_bounds(&view, 3, cx).bottom();
    let block = view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        layout.bounds(layout.frame.blocks[0].rect)
    });
    assert_eq!(block.top(), last);
}

#[gpui::test]
fn typing_consumes_and_stale_drops(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    suggest(&view, "bar()", cx);
    assert_eq!(texts(&view, cx), Some(("bar()".into(), vec![])));

    // Typing its next char: one char shorter.
    cx.simulate_input("b");
    assert_eq!(texts(&view, cx), Some(("ar()".into(), vec![])));
    assert_eq!(text(&view, cx), "foob;\nnext\nlast\n");

    // A char that doesn't match.
    cx.simulate_input("x");
    assert!(!shown(&view, cx));
    assert_eq!(painted(&view, cx), None);

    // A click elsewhere.
    suggest(&view, "bar()", cx);
    assert_eq!(texts(&view, cx), Some(("bar()".into(), vec![])));
    click(on_row(&view, 2, cx), cx);
    assert!(!shown(&view, cx));
    assert_eq!(painted(&view, cx), None);

    // Vim's normal mode, even with the caret where it was offered.
    cx.update(|_, cx| set_vim_enabled(cx, true));
    view.update(cx, |view, _| view.model_mut().set_cursor(CARET));
    cx.simulate_keystrokes("i");
    cx.run_until_parked();
    suggest(&view, "bar()", cx);
    assert_eq!(texts(&view, cx), Some(("bar()".into(), vec![])));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.vim_mode()),
        Some("normal")
    );
    assert!(!shown(&view, cx));
    assert_eq!(painted(&view, cx), None);
    // Set in normal mode, it's dropped before it's drawn.
    suggest(&view, "bar()", cx);
    assert_eq!(painted(&view, cx), None);
    assert!(!shown(&view, cx));
}

#[gpui::test]
fn other_buffer_dropped(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let other = Buffer::new(TEXT);
    let ghost = GhostText::new(&other.snapshot(), CARET, "bar()", ai());
    view.update(cx, |view, cx| view.set_ghost_text(Some(ghost), cx));
    assert!(!shown(&view, cx));
    assert_eq!(painted(&view, cx), None);
    assert_eq!(text(&view, cx), TEXT);
    cx.simulate_input("b");
    assert_eq!(painted(&view, cx), None);
}

#[gpui::test]
fn composition_hides_it(cx: &mut TestAppContext) {
    use gpui::EntityInputHandler as _;
    let (view, cx) = editor(cx);
    suggest(&view, "bar()", cx);
    let mark = |text: &str, cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.replace_and_mark_text_in_range(None, text, Some(0..0), window, cx)
            })
        });
    };
    // The composition takes the inlay, underlined, while it lasts.
    mark("b", cx);
    assert_eq!(painted(&view, cx), None);
    let underlined = view.read_with(cx, |view, _| view.painted_composition.is_some());
    assert!(underlined);
    // Cancelled, the suggestion shows again.
    mark("", cx);
    assert_eq!(texts(&view, cx), Some(("bar()".into(), vec![])));
}

/// Scrolls the view right to `x`, as far as it goes, and draws a frame.
fn scroll_right(view: &Entity<EditorView>, x: f32, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        view.scroll.x = px(x);
        view.autoscroll = false;
        cx.notify();
    });
    draw(cx);
}

#[gpui::test]
fn wide_rows_scroll_into_view(cx: &mut TestAppContext) {
    use fe_display::layout::MAX_SHAPED_ROW_BYTES;
    let (view, cx) = editor(cx);
    draw(cx);
    let text_width = f32::from(row_bounds(&view, 0, cx).size.width);
    // Monospaced, as wide as the `;`.
    let char_width =
        f32::from(caret_at(&view, CARET + 1, cx).origin.x - caret_at(&view, CARET, cx).origin.x);
    // A second line twice as wide as the view, on rows that all fit in it:
    // the view scrolls right as far as its end.
    let chars = (2. * text_width / char_width) as usize;
    let wide = format!("{}end", "x".repeat(chars));
    suggest(&view, &format!("bar(\n{wide}\n)"), cx);
    scroll_right(&view, 1_000_000., cx);
    let scrolled = view.read_with(cx, |view, _| f32::from(view.scroll.x));
    let wide_width = char_width * wide.len() as f32;
    assert!(scrolled > 0., "the view scrolled");
    assert!(
        scrolled + text_width >= wide_width,
        "{scrolled} + {text_width} reaches {wide_width}"
    );
    let (_, rows) = texts(&view, cx).expect("shown");
    assert_eq!(rows, [wide.as_str(), ")"]);

    // A second line longer than is shaped at once, scrolled to its end: a
    // slice around the view is painted, its last chars with it.
    view.update(cx, |view, cx| view.set_ghost_text(None, cx));
    let long = format!("{}tail!", "y".repeat(2 * MAX_SHAPED_ROW_BYTES));
    suggest(&view, &format!("bar(\n{long}\n)"), cx);
    scroll_right(&view, 10_000_000., cx);
    let (_, rows) = texts(&view, cx).expect("shown");
    assert!(
        rows[0].len() <= MAX_SHAPED_ROW_BYTES && rows[0].ends_with("yyytail!"),
        "{} bytes, ending {:?}",
        rows[0].len(),
        &rows[0][rows[0].len().saturating_sub(10)..]
    );
    let scrolled = view.read_with(cx, |view, _| f32::from(view.scroll.x));
    let long_width = char_width * long.len() as f32;
    assert!(
        scrolled + text_width >= long_width,
        "{scrolled} + {text_width} reaches {long_width}"
    );
}

#[gpui::test]
fn replace_caret_ignores_the_inlay(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    view.update(cx, |view, _| view.model_mut().set_cursor(CARET));
    cx.simulate_keystrokes("shift-r");
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.vim_mode()),
        Some("replace")
    );
    let carets = |cx: &mut VisualTestContext| {
        draw(cx);
        view.read_with(cx, |view, _| view.painted_carets.clone())
    };
    // An underline under the `;`.
    let before = carets(cx);
    assert_eq!(before.len(), 1);
    let semicolon = caret_at(&view, CARET + 1, cx).origin.x - caret_at(&view, CARET, cx).origin.x;
    assert!((before[0].size.width - semicolon).abs() < px(0.01));

    suggest(&view, "bar()", cx);
    assert_eq!(texts(&view, cx), Some(("bar()".into(), vec![])));
    let after = carets(cx);
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].origin, before[0].origin);
    assert!(
        (after[0].size.width - before[0].size.width).abs() < px(0.01),
        "{:?} is one `;` wide, as {:?} is",
        after[0].size.width,
        before[0].size.width
    );
}
