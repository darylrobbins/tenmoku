//! The editor gutter in B3 (`docs/design/design-system.md#editor`, Gutter,
//! Folding and Indent guides; `#agent-provenance`): 81 px at 13 px code
//! (line numbers 38, problem dot 10, fold column 16, change bar 3, 10 to the
//! code, after a 4 px inset), scaled with the font; a fold column always
//! reserved, so the code never moves; chevrons only while the pointer is in
//! the gutter, folded rows' always, the hovered one on a tile with an
//! up-chevron on its closing row; a notch for removed lines and a dotted
//! lane for an agent's; indent guides in leading whitespace.

use std::{cell::RefCell, ops::Range, rc::Rc};

use fe_buffer::{Buffer, BufferSnapshot};
use fe_editor::{
    EditorModel,
    decorations::{Decoration, DecorationKind, DecorationProvider, GutterMark, Tint, overlaps},
};
use fe_gpui::{ChevronKind, DrawnGutter, EditorTheme, EditorView, GutterAnnotations};
use gpui::{
    Bounds, Entity, Focusable as _, Modifiers, Pixels, Point, TestAppContext, VisualTestContext,
    point, px,
};

/// Rows 0 (`fn f`) and 4 (`fn g`) head folds; `fn f`'s closes on row 3.
const TEXT: &str =
    "fn f() {\n    one();\n    if x {\n        two();\n    }\n}\nfn g() {\n    three();\n}\n";

/// The rows of [`TEXT`]: `fn f` heads 0..=4, closed by row 5's `}`.
const F_CLOSE: u32 = 5;

/// Fixed decorations, as a git provider would hand out.
struct Fixed(Vec<Decoration>);

impl DecorationProvider for Fixed {
    fn decorations(&self, _: &BufferSnapshot, range: Range<usize>, out: &mut Vec<Decoration>) {
        out.extend(
            (self.0.iter())
                .filter(|item| overlaps(&item.range, &range))
                .cloned(),
        );
    }
}

/// A focused Rust editor on `text` in the dark theme at `font_size`, with
/// `decorations`, drawn once its tree is parsed.
fn editor_with<'a>(
    text: &str,
    font_size: f32,
    decorations: Vec<Decoration>,
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    editor_set_up(text, font_size, decorations, |_| {}, cx)
}

/// [`editor_with`], the model set up by `setup` first.
fn editor_set_up<'a>(
    text: &str,
    font_size: f32,
    decorations: Vec<Decoration>,
    setup: impl FnOnce(&mut EditorModel) + 'static,
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    let text = text.to_owned();
    cx.update(fe_gpui::init);
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), Some("main.rs".into()));
        setup(&mut model);
        model.add_decoration_provider(Rc::new(RefCell::new(Fixed(decorations))));
        let mut theme = EditorTheme::default_dark(cx);
        theme.font_size = px(font_size);
        EditorView::new(model, theme, cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.run_until_parked();
    let parsed = view.read_with(cx, |view, _| {
        view.syntax().and_then(fe_syntax::Syntax::tree).is_some()
    });
    assert!(parsed);
    draw(cx);
    (view, cx)
}

fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    editor_with(TEXT, 13., Vec::new(), cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn drawn(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> DrawnGutter {
    view.read_with(cx, |view, _| view.drawn_gutter().expect("drawn").clone())
}

/// Where the last frame drew a caret at the start of buffer row `row`.
fn row_bounds(view: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    view.update(cx, |view, _| {
        let offset = view.model().text().row_start(row);
        view.caret_bounds_at(offset).expect("row in view")
    })
}

/// The middle of the fold column on buffer row `row`.
fn fold_column_at(
    view: &Entity<EditorView>,
    row: u32,
    cx: &mut VisualTestContext,
) -> Point<Pixels> {
    let column = drawn(view, cx).columns[3];
    point(column.center().x, row_bounds(view, row, cx).center().y)
}

fn move_to(position: Point<Pixels>, cx: &mut VisualTestContext) {
    cx.simulate_mouse_move(position, None, Modifiers::none());
    cx.run_until_parked();
    draw(cx);
}

/// The chevrons drawn, as `(row, kind, hovered)`.
fn chevrons(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> Vec<(u32, ChevronKind, bool)> {
    let drawn = drawn(view, cx);
    (drawn.chevrons.iter())
        .map(|chevron| (chevron.row, chevron.kind, chevron.hovered))
        .collect()
}

fn widths(drawn: &DrawnGutter) -> Vec<f32> {
    (drawn.columns.iter())
        .map(|column| f32::from(column.size.width))
        .collect()
}

#[gpui::test]
fn the_gutter_is_81_px_at_13_px_and_scales_with_the_font(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let drawn = drawn(&view, cx);
    // Inset, numbers, problem dot, fold column, change bar, gap.
    assert_eq!(widths(&drawn), [4., 38., 10., 16., 3., 10.]);
    let left = drawn.columns[0].left();
    assert_eq!(drawn.text_left - left, px(81.));
    // Side by side, left to right, the last ending where the code starts.
    for pair in drawn.columns.windows(2) {
        assert_eq!(pair[0].right(), pair[1].left(), "{pair:?}");
    }
    assert_eq!(drawn.columns[5].right(), drawn.text_left);
    assert_eq!(row_bounds(&view, 0, cx).left(), drawn.text_left);
}

#[gpui::test]
fn at_26_px_every_column_is_twice_as_wide(cx: &mut TestAppContext) {
    let (view, cx) = editor_with(TEXT, 26., Vec::new(), cx);
    let drawn = drawn(&view, cx);
    assert_eq!(widths(&drawn), [8., 76., 20., 32., 6., 20.]);
    assert_eq!(drawn.text_left - drawn.columns[0].left(), px(162.));
}

#[gpui::test]
fn the_line_numbers_column_grows_past_four_digits(cx: &mut TestAppContext) {
    let text = format!("{TEXT}{}", "\n".repeat(20_000));
    let (view, cx) = editor_with(&text, 13., Vec::new(), cx);
    let drawn = drawn(&view, cx);
    let widths = widths(&drawn);
    assert!(widths[1] > 38., "{widths:?}");
    assert_eq!(widths[2..], [10., 16., 3., 10.]);
}

#[gpui::test]
fn the_code_does_not_move_when_the_pointer_enters_or_a_fold_toggles(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let text_left = drawn(&view, cx).text_left;
    let code = row_bounds(&view, 6, cx).left();
    assert_eq!(code, text_left);

    move_to(fold_column_at(&view, 0, cx), cx);
    assert!(
        !chevrons(&view, cx).is_empty(),
        "the pointer is in the gutter"
    );
    assert_eq!(drawn(&view, cx).text_left, text_left);
    assert_eq!(row_bounds(&view, 6, cx).left(), code);

    // Folding `fn f` at its chevron, and unfolding it.
    for _ in 0..2 {
        let at = fold_column_at(&view, 0, cx);
        cx.simulate_click(at, Modifiers::none());
        draw(cx);
        assert_eq!(drawn(&view, cx).text_left, text_left);
        assert_eq!(row_bounds(&view, 6, cx).left(), code);
    }
    let folded = view.read_with(cx, |view, _| {
        view.model().display().hidden_rows(1).is_some()
    });
    assert!(!folded, "folded and unfolded again");
}

#[gpui::test]
fn chevrons_show_only_while_the_pointer_is_in_the_gutter(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let open = |row| (row, ChevronKind::Open, false);
    assert_eq!(chevrons(&view, cx), [], "the pointer hasn't moved");
    let drawn = drawn(&view, cx);
    let y = row_bounds(&view, 1, cx).center().y;

    move_to(point(drawn.text_left + px(30.), y), cx);
    assert_eq!(chevrons(&view, cx), [], "over the code");
    // Over the line numbers, the dot's column, the bar and the gap: all
    // the gutter.
    for column in [1, 2, 4, 5] {
        move_to(point(drawn.columns[column].center().x, y), cx);
        assert_eq!(
            chevrons(&view, cx),
            [open(0), open(2), open(6)],
            "column {column}"
        );
    }
    move_to(point(drawn.text_left + px(30.), y), cx);
    assert_eq!(chevrons(&view, cx), []);

    // A folded row's chevron shows wherever the pointer is.
    view.update(cx, |view, cx| {
        view.model_mut().fold_rows(6..=7);
        cx.notify();
    });
    draw(cx);
    assert_eq!(chevrons(&view, cx), [(6, ChevronKind::Folded, false)]);
    move_to(point(drawn.columns[1].center().x, y), cx);
    assert_eq!(
        chevrons(&view, cx),
        [open(0), open(2), (6, ChevronKind::Folded, false)]
    );
}

#[gpui::test]
fn the_hovered_fold_is_on_a_tile_and_its_closing_row_has_the_up_chevron(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let theme = view.read_with(cx, |view, _| view.theme().clone());
    // The caret on the last, empty row, whose block guide there's none.
    view.update(cx, |view, cx| {
        let end = view.model().text().len();
        view.model_mut().set_cursor(end);
        cx.notify();
    });
    move_to(fold_column_at(&view, 0, cx), cx);
    assert_eq!(
        chevrons(&view, cx),
        [
            (0, ChevronKind::Open, true),
            (2, ChevronKind::Open, false),
            (6, ChevronKind::Open, false),
            (F_CLOSE, ChevronKind::Closing, false),
        ]
    );
    let drawn = drawn(&view, cx);
    let fold = drawn.columns[3];
    for chevron in &drawn.chevrons {
        // In the fold column, centred on its row.
        let row = row_bounds(&view, chevron.row, cx);
        assert!(fold.left() <= chevron.bounds.left() && chevron.bounds.right() <= fold.right());
        assert_eq!(chevron.bounds.center().y, row.center().y);
        let strong = chevron.hovered || chevron.kind == ChevronKind::Closing;
        let color = match strong {
            true => theme.gutter_active_foreground,
            false => theme.gutter_foreground,
        };
        assert_eq!(chevron.color, color, "{chevron:?}");
    }
    let [header, closing] = [0, 3].map(|ix| drawn.chevrons[ix].bounds);
    assert!(closing.size.width < header.size.width, "small: {closing:?}");

    // Its indent guide, down the rows it folds, is the bright one.
    let bright: Vec<_> = (drawn.indent_guides.iter())
        .filter(|(_, bright)| *bright)
        .map(|(bounds, _)| *bounds)
        .collect();
    assert_eq!(bright.len(), 1, "{:?}", drawn.indent_guides);
    assert_eq!(bright[0].top(), row_bounds(&view, 1, cx).top());
    assert_eq!(bright[0].bottom(), row_bounds(&view, 4, cx).bottom());
    assert_eq!(bright[0].left(), drawn.text_left);

    // Onto the inner fold: its closing row is row 4.
    move_to(fold_column_at(&view, 2, cx), cx);
    let kinds = chevrons(&view, cx);
    assert!(kinds.contains(&(2, ChevronKind::Open, true)), "{kinds:?}");
    assert!(
        kinds.contains(&(4, ChevronKind::Closing, false)),
        "{kinds:?}"
    );
    // A row that heads nothing hovers nothing.
    move_to(fold_column_at(&view, 1, cx), cx);
    let kinds = chevrons(&view, cx);
    assert!(
        kinds
            .iter()
            .all(|&(_, kind, hovered)| !hovered && kind != ChevronKind::Closing)
    );
}

/// A whole-row decoration of `kind` over `rows` of [`TEXT`].
fn rows_decoration(rows: Range<u32>, kind: DecorationKind, tint: Tint) -> Decoration {
    let starts: Vec<usize> = std::iter::once(0)
        .chain(TEXT.match_indices('\n').map(|(at, _)| at + 1))
        .collect();
    Decoration::new(
        starts[rows.start as usize]..starts[rows.end as usize],
        kind,
        tint,
    )
}

#[gpui::test]
fn agent_lines_draw_a_dotted_agent_lane(cx: &mut TestAppContext) {
    // Rows 1 through 3 changed; an agent wrote rows 2 and 3.
    let bar = rows_decoration(
        1..4,
        DecorationKind::Gutter(GutterMark::Bar),
        Tint::Modified,
    );
    let (view, cx) = editor_with(TEXT, 13., vec![bar], cx);
    let theme = view.read_with(cx, |view, _| view.theme().clone());
    let before = drawn(&view, cx);
    assert_eq!(before.agent_dots, []);
    assert_eq!(before.bars.len(), 1);

    view.update(cx, |view, cx| {
        let snapshot = view.model().snapshot().clone();
        let text = snapshot.text();
        let rows =
            snapshot.anchor_before(text.row_start(2))..snapshot.anchor_before(text.row_start(4));
        let annotations = GutterAnnotations {
            version: snapshot.version(),
            entries: Vec::new(),
            hover: Rc::new(|_, _| None),
            on_click: Rc::new(|_, _, _, _| {}),
        };
        view.set_gutter_annotations_with_agent_lane(annotations, vec![rows], cx);
    });
    draw(cx);
    let after = drawn(&view, cx);
    let drawn = &after;
    // No annotation column: the gutter didn't move.
    assert_eq!(drawn.columns, before.columns);
    let lane = drawn.columns[4];
    let (top, bottom) = (
        row_bounds(&view, 2, cx).top(),
        row_bounds(&view, 3, cx).bottom(),
    );
    assert!(drawn.agent_dots.len() >= 4, "{:?}", drawn.agent_dots);
    for dot in &drawn.agent_dots {
        // Bar-wide circles in the bar's place, down the agent's rows.
        assert_eq!(
            (dot.left(), dot.size.width, dot.size.height),
            (lane.left(), px(3.), px(3.))
        );
        assert!(top <= dot.top() && dot.bottom() <= bottom, "{dot:?}");
    }
    for pair in drawn.agent_dots.windows(2) {
        assert_eq!(pair[1].top() - pair[0].top(), px(6.), "dotted: {pair:?}");
    }
    // The bar keeps row 1, the person's.
    assert_eq!(drawn.bars.len(), 1, "{:?}", drawn.bars);
    let (bar, color) = drawn.bars[0];
    assert_eq!(color, theme.change_modified.opacity(0.6));
    assert!(bar.top() >= row_bounds(&view, 1, cx).top() && bar.bottom() <= top);
    assert!(theme.agent_lane.a > 0.);

    // Taking the column away takes the lane.
    view.update(cx, |view, cx| view.set_gutter_annotations(None, cx));
    draw(cx);
    assert_eq!(self::drawn(&view, cx).agent_dots, []);
}

#[gpui::test]
fn removed_lines_draw_a_notch(cx: &mut TestAppContext) {
    let deletion = rows_decoration(
        2..2,
        DecorationKind::Gutter(GutterMark::Deletion),
        Tint::Removed,
    );
    let (view, cx) = editor_with(TEXT, 13., vec![deletion], cx);
    let theme = view.read_with(cx, |view, _| view.theme().clone());
    let drawn = drawn(&view, cx);
    assert_eq!(drawn.notches.len(), 1, "{:?}", drawn.notches);
    let (notch, color) = drawn.notches[0];
    assert_eq!(color, theme.change_deleted);
    // On the edge above row 2, from the bar's left edge, twice its width.
    let lane = drawn.columns[4];
    assert_eq!(notch.left(), lane.left());
    assert_eq!(notch.size.width, lane.size.width * 2.);
    assert!(notch.size.height < notch.size.width);
    assert_eq!(notch.center().y, row_bounds(&view, 2, cx).top());
}

#[gpui::test]
fn indent_guides_stay_in_leading_whitespace_every_4_columns_the_caret_s_brighter(
    cx: &mut TestAppContext,
) {
    let (view, cx) = editor(cx);
    // The caret into `two();`, in the `if`'s block.
    view.update(cx, |view, cx| {
        let at = view.model().text().row_start(3) + 8;
        view.model_mut().set_cursor(at);
        cx.notify();
    });
    draw(cx);
    let drawn = drawn(&view, cx);
    let space = (row_bounds(&view, 1, cx).left(), {
        let offset = view.read_with(cx, |view, _| view.model().text().row_start(1) + 4);
        view.update(cx, |view, _| view.caret_bounds_at(offset).unwrap().left())
    });
    let column = (space.1 - space.0) / 4.;
    let guides: Vec<(f32, u32, u32, bool)> = (drawn.indent_guides.iter())
        .map(|(bounds, bright)| {
            let columns = ((bounds.left() - drawn.text_left) / column).round();
            let first = (0..9).find(|&row| row_bounds(&view, row, cx).top() == bounds.top());
            let last = (0..9).find(|&row| row_bounds(&view, row, cx).bottom() == bounds.bottom());
            assert_eq!(bounds.size.width, px(1.));
            (columns, first.unwrap(), last.unwrap(), *bright)
        })
        .collect();
    // Column 0 down `fn f`'s and `fn g`'s bodies, column 4 down the `if`'s,
    // the caret's: never past a row's indent.
    assert_eq!(
        guides,
        [(0., 1, 4, false), (0., 7, 7, false), (4., 3, 3, true),]
    );
}

/// The chevrons drawn closing a fold, by row.
fn closing_rows(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<u32> {
    (chevrons(view, cx).into_iter())
        .filter(|&(_, kind, _)| kind == ChevronKind::Closing)
        .map(|(row, _, _)| row)
        .collect()
}

#[gpui::test]
fn the_closing_row_is_the_fold_s_own_terminator(cx: &mut TestAppContext) {
    // A block comment closes on its `*/` row; a call whose `)` ends a row
    // closes on that row, not on the enclosing `}` below it.
    let text = "/*\n  a\n  b\n*/\nfn h() {\n    call(\n        a,\n        b);\n}\n";
    let (view, cx) = editor_with(text, 13., Vec::new(), cx);
    move_to(fold_column_at(&view, 0, cx), cx);
    assert_eq!(closing_rows(&view, cx), [3], "the comment's */");
    move_to(fold_column_at(&view, 5, cx), cx);
    assert_eq!(closing_rows(&view, cx), [7], "the call's last row");
    move_to(fold_column_at(&view, 4, cx), cx);
    assert_eq!(closing_rows(&view, cx), [8], "fn h's }}");

    // A server's range closes on its own last row, whatever follows it.
    view.update(cx, |view, cx| {
        view.set_fold_ranges(Some(std::iter::once(4..=6).collect()));
        cx.notify();
    });
    draw(cx);
    move_to(fold_column_at(&view, 0, cx), cx);
    move_to(fold_column_at(&view, 4, cx), cx);
    assert_eq!(closing_rows(&view, cx), [6], "the server's range");
}

/// The width of a column of `view`'s code, from its first row's first char.
fn column_width(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Pixels {
    view.update(cx, |view, _| {
        view.caret_bounds_at(1).unwrap().left() - view.caret_bounds_at(0).unwrap().left()
    })
}

/// The guides drawn, as (column, first row, last row).
fn guide_spans(
    view: &Entity<EditorView>,
    rows: u32,
    cx: &mut VisualTestContext,
) -> Vec<(f32, u32, u32)> {
    let drawn = drawn(view, cx);
    let column = column_width(view, cx);
    let tops: Vec<_> = (0..rows).map(|row| row_bounds(view, row, cx)).collect();
    (drawn.indent_guides.iter())
        .map(|(bounds, _)| {
            let columns = ((bounds.left() - drawn.text_left) / column).round();
            let first = tops
                .iter()
                .position(|row| row.top() == bounds.top())
                .unwrap() as u32;
            let last = tops
                .iter()
                .position(|row| row.bottom() == bounds.bottom())
                .unwrap() as u32;
            (columns, first, last)
        })
        .collect()
}

#[gpui::test]
fn guides_are_4_columns_apart_whatever_the_indent(cx: &mut TestAppContext) {
    // Two-space indents: a guide at columns 0 and 4, none at 2 or 6.
    let text = "fn f() {\n  if x {\n    if y {\n      z();\n    }\n  }\n}\n";
    let (view, cx) = editor_with(text, 13., Vec::new(), cx);
    assert_eq!(guide_spans(&view, 7, cx), [(0., 1, 5), (4., 3, 3)]);
}

#[gpui::test]
fn guides_are_4_columns_apart_in_8_wide_tabs(cx: &mut TestAppContext) {
    // Guides at 0 and 4 inside one tab, 8 and 12 inside two.
    let text = "fn f() {\n\tif x {\n\t\ty();\n\t}\n}\n";
    let (view, cx) = editor_set_up(text, 13., Vec::new(), |model| model.set_tab_size(8), cx);
    assert_eq!(
        guide_spans(&view, 5, cx),
        [(0., 1, 3), (4., 1, 3), (8., 2, 2), (12., 2, 2)]
    );
}

#[gpui::test]
fn a_wrapped_indented_row_keeps_its_guides_off_the_continuation(cx: &mut TestAppContext) {
    // Row 3 is indented 8 and wraps: its continuations start at the code's
    // left edge with code, which no guide crosses.
    let long = "word ".repeat(80);
    let text =
        format!("fn f() {{\n    if x {{\n        y();\n        {long}\n        z();\n    }}\n}}\n");
    let (view, cx) = editor_set_up(
        &text,
        13.,
        Vec::new(),
        |model| model.set_soft_wrap(true),
        cx,
    );
    let (row_3, row_4) = (row_bounds(&view, 3, cx), row_bounds(&view, 4, cx));
    assert!(row_4.top() > row_3.bottom(), "row 3 wraps");
    let drawn = drawn(&view, cx);
    for (guide, _) in &drawn.indent_guides {
        let over_continuations = guide.top() < row_4.top() && row_3.bottom() < guide.bottom();
        assert!(
            !over_continuations,
            "{guide:?} crosses row 3's continuations"
        );
    }
    // Each guide goes on below them.
    assert_eq!(
        guide_spans(&view, 7, cx),
        [(0., 1, 3), (0., 4, 5), (4., 2, 3), (4., 4, 4)]
    );
}
