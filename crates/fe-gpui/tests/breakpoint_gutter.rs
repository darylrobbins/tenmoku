//! A debugger's marks over the line numbers (`docs/design/design-system.md`,
//! Debugger; debugger card D6): the gutter stays 81 px and the marks draw
//! in the line-number column, the breakpoint's slot at x 14–26 and the
//! frame arrow's at x 29–41 at 13 px code, hiding their row's number; a
//! click on a row's number toggles a breakpoint there and leaves the
//! carets be; the problem dot and the host's lightbulb stay where they
//! are. Read back through `drawn_gutter`.

use std::{cell::RefCell, rc::Rc};

use fe_buffer::{Buffer, ResolvedSelection};
use fe_editor::{
    EditorModel,
    diagnostics::{Diagnostic, Severity},
};
use fe_gpui::{
    BreakpointGlyph, BreakpointHandlers, BreakpointMark, BreakpointMenuItem, BreakpointState,
    DrawnGutter, EditorTheme, EditorView, FrameArrow, GlyphKind, GutterGlyph,
};
use gpui::{
    Bounds, Entity, Focusable as _, Modifiers, MouseButton, Pixels, Point, TestAppContext,
    VisualTestContext, point, px,
};

/// Twelve rows: `fn f`, ten calls and a closing brace.
fn text() -> String {
    let mut text = String::from("fn f() {\n");
    for n in 0..10 {
        text.push_str(&format!("    call_{n}();\n"));
    }
    text.push_str("}\n");
    text
}

/// A focused Rust editor on [`text`] in the dark theme, the caret at the
/// end of row 1, drawn.
fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(fe_gpui::init);
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let text = text();
        let mut model = EditorModel::new(Buffer::new(&text), Some("main.rs".into()));
        let caret = text.find("();").unwrap() + 3;
        model.set_selections(vec![ResolvedSelection {
            range: caret..caret,
            reversed: false,
        }]);
        let mut theme = EditorTheme::default_dark(cx);
        theme.font_size = px(13.);
        EditorView::new(model, theme, cx)
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    draw(cx);
    (view, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn drawn(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> DrawnGutter {
    view.read_with(cx, |view, _| view.drawn_gutter().expect("drawn").clone())
}

fn mark(
    row: u32,
    breakpoint: Option<BreakpointState>,
    frame: Option<FrameArrow>,
) -> BreakpointMark {
    BreakpointMark {
        row,
        breakpoint,
        frame,
        target: None,
    }
}

fn set_marks(view: &Entity<EditorView>, marks: Vec<BreakpointMark>, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        let version = view.model().snapshot().version();
        view.set_breakpoints(version, marks, cx);
    });
    draw(cx);
}

/// Handlers that keep a set of enabled breakpoints as a host's store
/// would, showing it on each toggle; the rows toggled, in order.
fn host(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Rc<RefCell<Vec<u32>>> {
    let toggled = Rc::new(RefCell::new(Vec::new()));
    let set: Rc<RefCell<Vec<u32>>> = Rc::default();
    let handle = view.downgrade();
    let handlers = BreakpointHandlers {
        on_toggle: Rc::new({
            let toggled = toggled.clone();
            move |row, _, cx| {
                toggled.borrow_mut().push(row);
                let mut rows = set.borrow_mut();
                match rows.iter().position(|r| *r == row) {
                    Some(at) => {
                        rows.remove(at);
                    }
                    None => rows.push(row),
                }
                let marks = (rows.iter())
                    .map(|&row| mark(row, Some(BreakpointState::Enabled), None))
                    .collect();
                (handle.update(cx, |view, cx| {
                    let version = view.model().snapshot().version();
                    view.set_breakpoints(version, marks, cx);
                }))
                .ok();
            }
        }),
        menu: Rc::new(|_, _| Vec::<BreakpointMenuItem>::new()),
    };
    view.update(cx, |view, cx| {
        view.set_breakpoint_handlers(Some(handlers), cx)
    });
    draw(cx);
    toggled
}

/// The middle of buffer row `row`'s line-number column.
fn number_at(view: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) -> Point<Pixels> {
    let drawn = drawn(view, cx);
    let column = drawn.columns[0].union(&drawn.columns[1]);
    let y = view.update(cx, |view, _| {
        let offset = view.model().text().row_start(row);
        view.caret_bounds_at(offset)
            .expect("row in view")
            .center()
            .y
    });
    point(column.center().x, y)
}

fn caret(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> usize {
    view.read_with(cx, |view, _| view.model().primary_selection().head())
}

fn click(at: Point<Pixels>, cx: &mut VisualTestContext) {
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    draw(cx);
}

/// The glyphs drawn, as `(row, glyph)`.
fn glyphs(drawn: &DrawnGutter) -> Vec<(u32, BreakpointGlyph)> {
    (drawn.breakpoints.iter())
        .map(|drawn| (drawn.row, drawn.glyph))
        .collect()
}

/// C1: a click on row 7's number toggles a breakpoint on row 7, whatever
/// row the caret is on, and doesn't move the caret; a second click removes
/// it; a press that moves first is a drag, not a toggle.
#[gpui::test]
fn a_click_on_row_7s_number_toggles_a_breakpoint_at_row_7_not_at_the_carets(
    cx: &mut TestAppContext,
) {
    let (view, cx) = editor(cx);
    let toggled = host(&view, cx);
    let before = caret(&view, cx);
    assert_eq!(glyphs(&drawn(&view, cx)), []);

    click(number_at(&view, 7, cx), cx);
    assert_eq!(*toggled.borrow(), [7]);
    assert_eq!(caret(&view, cx), before, "the caret stays on row 1");
    let drawn = drawn(&view, cx);
    assert_eq!(glyphs(&drawn), [(7, BreakpointGlyph::Dot)]);
    let column = drawn.columns[0].union(&drawn.columns[1]);
    assert!(column.contains(&drawn.breakpoints[0].bounds.center()));
    assert!(!drawn.numbers.contains(&7), "row 7 draws no number");
    assert!(drawn.numbers.contains(&6) && drawn.numbers.contains(&8));

    click(number_at(&view, 7, cx), cx);
    assert_eq!(*toggled.borrow(), [7, 7]);
    assert_eq!(glyphs(&self::drawn(&view, cx)), []);
    assert!(self::drawn(&view, cx).numbers.contains(&7));

    // Pressed on row 2's number and dragged down: a selection, no toggle.
    let (from, to) = (number_at(&view, 2, cx), number_at(&view, 5, cx));
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    draw(cx);
    assert_eq!(*toggled.borrow(), [7, 7]);
    let selected = view.read_with(cx, |view, _| view.model().primary_selection().range);
    assert!(!selected.is_empty(), "a drag selects");
}

/// C2: every state draws in its slot over the number column, its row's
/// number hidden, and the gutter keeps its six columns and 81 px.
#[gpui::test]
fn breakpoints_draw_over_the_number_column_and_the_gutter_stays_81_px(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let widths = |drawn: &DrawnGutter| -> Vec<f32> {
        (drawn.columns.iter())
            .map(|column| f32::from(column.size.width))
            .collect()
    };
    let before = drawn(&view, cx);
    assert_eq!(widths(&before), [4., 38., 10., 16., 3., 10.]);
    set_marks(
        &view,
        vec![
            mark(1, Some(BreakpointState::Enabled), None),
            mark(
                2,
                Some(BreakpointState::Unverified {
                    message: Some("no code".into()),
                }),
                None,
            ),
            mark(3, Some(BreakpointState::Disabled), None),
            mark(4, None, Some(FrameArrow::Current)),
            mark(5, Some(BreakpointState::Enabled), Some(FrameArrow::Caller)),
        ],
        cx,
    );
    let drawn = drawn(&view, cx);
    assert_eq!(drawn.columns, before.columns, "the gutter doesn't move");
    assert_eq!(drawn.text_left, before.text_left);
    let left = drawn.columns[0].left();
    assert_eq!(f32::from(drawn.text_left - left), 81.);
    assert_eq!(
        glyphs(&drawn),
        [
            (1, BreakpointGlyph::Dot),
            (2, BreakpointGlyph::Ring),
            (3, BreakpointGlyph::Disabled),
            (4, BreakpointGlyph::CurrentArrow),
            (5, BreakpointGlyph::Dot),
            (5, BreakpointGlyph::CallerArrow),
        ]
    );
    let theme = view.read_with(cx, |view, _| view.theme().clone());
    let slot = |bounds: Bounds<Pixels>| {
        let x = f32::from(bounds.left() - left);
        (
            x,
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
        )
    };
    for drawn in &drawn.breakpoints {
        let arrow = matches!(
            drawn.glyph,
            BreakpointGlyph::CurrentArrow | BreakpointGlyph::CallerArrow
        );
        let (x, width, height) = slot(drawn.bounds);
        match arrow {
            true => assert_eq!((x, width, height), (29., 12., 10.), "{drawn:?}"),
            false => assert_eq!((x, width, height), (14., 12., 12.), "{drawn:?}"),
        }
        let color = match drawn.glyph {
            BreakpointGlyph::Dot | BreakpointGlyph::Ring => theme.debug_breakpoint,
            BreakpointGlyph::Disabled => theme.debug_breakpoint_disabled,
            _ => theme.debug_frame_arrow,
        };
        assert_eq!(drawn.color, color, "{drawn:?}");
        // Inside the number column, centred on its row.
        let column = drawn_column(&self::drawn(&view, cx));
        assert!(column.contains(&drawn.bounds.origin), "{drawn:?}");
        let row = view.update(cx, |view, _| {
            let offset = view.model().text().row_start(drawn.row);
            view.caret_bounds_at(offset).unwrap()
        });
        let middle = f32::from(drawn.bounds.center().y - row.center().y);
        assert!(middle.abs() <= 0.5, "{drawn:?} on {row:?}");
    }
    for row in 1..=5 {
        assert!(!drawn.numbers.contains(&row), "row {row} draws no number");
    }
    assert!(drawn.numbers.contains(&0) && drawn.numbers.contains(&6));
    assert_ne!(theme.debug_breakpoint, theme.debug_breakpoint_disabled);

    // Without a gutter, nothing over the numbers.
    view.update(cx, |view, cx| view.set_gutter(false, cx));
    draw(cx);
    assert_eq!(self::drawn(&view, cx).breakpoints, []);
}

/// The number column: the room before the numbers and the numbers.
fn drawn_column(drawn: &DrawnGutter) -> Bounds<Pixels> {
    drawn.columns[0].union(&drawn.columns[1])
}

/// C2: a breakpoint shares its row with a problem dot, and another with
/// the host's lightbulb, each in its own place.
#[gpui::test]
fn they_coexist_with_the_diagnostic_dot_and_the_lightbulb(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    view.update(cx, |view, cx| {
        let start = view.model().text().row_start(2) + 4;
        let version = view.model().snapshot().version();
        let items = vec![(start..start + 4, Diagnostic::new(Severity::Error, "bad"))];
        assert!(
            view.model_mut()
                .apply_diagnostics(1u32, (1, 1), version, items, Some((1, 1)))
        );
        view.set_gutter_glyph(
            Some(GutterGlyph {
                row: 3,
                kind: GlyphKind::Lightbulb,
            }),
            None,
        );
        cx.notify();
    });
    draw(cx);
    let before = drawn(&view, cx);
    assert_eq!(
        (before.dots.iter()).map(|dot| dot.0).collect::<Vec<_>>(),
        [2]
    );
    let lightbulb = before.glyph.expect("the lightbulb");
    assert_eq!(lightbulb.0, 3);
    set_marks(
        &view,
        vec![
            mark(2, Some(BreakpointState::Enabled), None),
            mark(
                3,
                Some(BreakpointState::Disabled),
                Some(FrameArrow::Current),
            ),
        ],
        cx,
    );
    let drawn = drawn(&view, cx);
    assert_eq!(drawn.dots, before.dots, "the problem dot stays");
    assert_eq!(drawn.glyph, Some(lightbulb), "the lightbulb stays");
    assert_eq!(
        glyphs(&drawn),
        [
            (2, BreakpointGlyph::Dot),
            (3, BreakpointGlyph::Disabled),
            (3, BreakpointGlyph::CurrentArrow),
        ]
    );
    let problem = drawn.columns[2];
    for glyph in &drawn.breakpoints {
        assert!(glyph.bounds.right() <= problem.left(), "{glyph:?}");
        assert!(!glyph.bounds.intersects(&drawn.dots[0].1));
        assert!(!glyph.bounds.intersects(&lightbulb.1));
    }
}
