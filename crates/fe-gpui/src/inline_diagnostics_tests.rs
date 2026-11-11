//! Diagnostic messages after their rows in a headless window: which message
//! shows, where and how far, that nothing else moves for it, and what it
//! costs.

use std::{cell::RefCell, ops::Range, rc::Rc};

use fe_buffer::{Buffer, BufferSnapshot};
use fe_config::settings::InlineDiagnostics;
use fe_display::CharWidth as _;
use fe_editor::{
    EditorModel,
    decorations::{Decoration, DecorationKind, DecorationProvider, Tint},
    diagnostics::{Diagnostic, Severity},
};
use gpui::{
    Entity, Modifiers, MouseButton, Pixels, TestAppContext, VisualTestContext, point, px, size,
};

use fe_display::layout::MeasuredLine;

use super::{Kept, PaintedMessage, keep};
use crate::{hover::HOVER_DELAY, inlay_hints::InlayHint, *};

const ID: (u64, u64) = (1, 1);

fn editor<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&text), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, _| window.activate_window());
    cx.simulate_resize(size(px(800.), px(400.)));
    draw(cx);
    (view, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn level(view: &Entity<EditorView>, level: InlineDiagnostics, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        view.theme.inline_diagnostics = level;
        cx.notify();
    });
    draw(cx);
}

fn diagnose(
    view: &Entity<EditorView>,
    items: Vec<(Range<usize>, Diagnostic)>,
    cx: &mut VisualTestContext,
) {
    view.update(cx, |view, cx| {
        let version = view.model().snapshot().version();
        assert!(
            view.model_mut()
                .apply_diagnostics(1u32, ID, version, items, Some(ID))
        );
        cx.notify();
    });
    draw(cx);
}

fn messages(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<PaintedMessage> {
    view.read_with(cx, |view, _| {
        view.painted_inline_diagnostics.messages.clone()
    })
}

/// Each row with a message, and its text.
fn shown(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<(u32, String)> {
    (messages(view, cx).into_iter())
        .map(|message| (message.buffer_row, message.text))
        .collect()
}

fn owned(rows: &[(u32, &str)]) -> Vec<(u32, String)> {
    rows.iter()
        .map(|(row, text)| (*row, text.to_string()))
        .collect()
}

/// The window x of the caret at `offset`.
fn caret_x(view: &Entity<EditorView>, offset: usize, cx: &mut VisualTestContext) -> Pixels {
    view.update(cx, |view, _| {
        view.layout
            .as_mut()
            .unwrap()
            .caret_bounds(offset)
            .unwrap()
            .left()
    })
}

/// The width of a space in the editor font, which is an em in it.
fn em(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Pixels {
    view.update(cx, |view, _| {
        px(view.layout.as_mut().unwrap().glyph_widths.char_width(' '))
    })
}

fn assert_near(left: Pixels, right: Pixels) {
    assert!((left - right).abs() < px(0.01), "{left:?} != {right:?}");
}

#[gpui::test]
fn the_most_severe_message_per_row_shows_and_the_level_filters(cx: &mut TestAppContext) {
    let text = "one two\nthree\nfour\nfive\nsix\nseven\neight\nnine\n";
    let (view, cx) = editor(cx, text);
    let at = |word: &str| {
        let start = text.find(word).unwrap();
        start..start + word.len()
    };
    diagnose(
        &view,
        vec![
            // The hint comes first, and an error beats it on the row.
            (at("one"), Diagnostic::new(Severity::Hint, "a hint")),
            (
                at("two"),
                Diagnostic::new(Severity::Error, "an error\nsecond line"),
            ),
            // The earliest of equals, given later.
            (
                at("three").start + 2..at("three").end,
                Diagnostic::new(Severity::Warning, "later"),
            ),
            (
                at("three").start..at("three").start + 2,
                Diagnostic::new(Severity::Warning, "earlier"),
            ),
            (
                at("four"),
                Diagnostic::new(Severity::Information, "an info"),
            ),
            (at("five"), Diagnostic::new(Severity::Hint, "five hint")),
            // A blank first line has nothing to show, so gives way.
            (at("six"), Diagnostic::new(Severity::Error, "\nnot this")),
            (at("six"), Diagnostic::new(Severity::Warning, "a tab\there")),
            // Through to the start of a row, but not on it.
            (
                at("seven").start..at("eight").start,
                Diagnostic::new(Severity::Warning, "whole row"),
            ),
            // Empty at a row's start: on that row.
            (
                at("nine").start..at("nine").start,
                Diagnostic::new(Severity::Warning, "empty"),
            ),
        ],
        cx,
    );

    // Warning is the default.
    let warning = owned(&[
        (0, "an error"),
        (1, "earlier"),
        (4, "a tab here"),
        (5, "whole row"),
        (7, "empty"),
    ]);
    assert_eq!(shown(&view, cx), warning);
    let colors = messages(&view, cx);
    let theme = view.read_with(cx, |view, _| view.theme.clone());
    // An error's at full strength, a warning's at 70% (T1550).
    assert_eq!(colors[0].color, theme.diagnostic_color(Severity::Error));
    assert_eq!(
        colors[1].color,
        theme.diagnostic_color(Severity::Warning).opacity(0.7)
    );

    level(&view, InlineDiagnostics::Error, cx);
    assert_eq!(shown(&view, cx), owned(&[(0, "an error")]));

    level(&view, InlineDiagnostics::All, cx);
    assert_eq!(
        shown(&view, cx),
        owned(&[
            (0, "an error"),
            (1, "earlier"),
            (2, "an info"),
            (3, "five hint"),
            (4, "a tab here"),
            (5, "whole row"),
            (7, "empty"),
        ])
    );
    let info = messages(&view, cx);
    assert_eq!(
        info[2].color,
        theme.diagnostic_color(Severity::Information).opacity(0.6)
    );

    level(&view, InlineDiagnostics::Off, cx);
    assert_eq!(shown(&view, cx), owned(&[]));
    assert_eq!(
        view.read_with(cx, |view, _| view.painted_inline_diagnostics.shapes),
        0
    );

    // Through the settings, as a reload brings them.
    view.update(cx, |view, cx| {
        let settings = EditorSettings {
            inline_diagnostics: InlineDiagnostics::Error,
            ..EditorSettings::default()
        };
        view.apply_settings(&settings, cx);
    });
    draw(cx);
    assert_eq!(shown(&view, cx), owned(&[(0, "an error")]));

    // A change of palette keeps it.
    view.update(cx, |view, cx| {
        let settings = EditorSettings {
            inline_diagnostics: InlineDiagnostics::Off,
            ..EditorSettings::default()
        };
        view.apply_settings(&settings, cx);
        view.set_colors(EditorPalette::fastedit_light(), cx);
        assert_eq!(view.theme.inline_diagnostics, InlineDiagnostics::Off);
    });
}

#[gpui::test]
fn a_wrapped_row_draws_on_its_last_display_row_and_is_cut_at_the_viewport(cx: &mut TestAppContext) {
    let long = "x".repeat(300);
    let message = "m".repeat(400);
    let text = format!("short\n{long}\nfolded\n  body\nend\n");
    let (view, cx) = editor(cx, &text);
    let long_end = 6 + long.len();
    diagnose(
        &view,
        vec![
            (0..5, Diagnostic::new(Severity::Error, "fits")),
            (
                6..long_end,
                Diagnostic::new(Severity::Error, "on the wrapped row"),
            ),
        ],
        cx,
    );
    let em = em(&view, cx);

    // One em after the last glyph, on the row's own top.
    let found = messages(&view, cx);
    assert_eq!(found[0].buffer_row, 0);
    assert_near(found[0].bounds.left(), caret_x(&view, 5, cx) + em);
    let caret = view.update(cx, |view, _| {
        view.layout.as_mut().unwrap().caret_bounds(5).unwrap()
    });
    assert_near(found[0].bounds.top(), caret.top());

    // Off the end of the view when not wrapped, so not drawn.
    assert_eq!(found.iter().filter(|m| m.buffer_row == 1).count(), 0);

    // Wrapped: on the last display row, where the row ends.
    view.update(cx, |view, cx| {
        view.model_mut().set_soft_wrap(true);
        cx.notify();
    });
    draw(cx);
    let found = messages(&view, cx);
    let wrapped = found.iter().find(|m| m.buffer_row == 1).unwrap();
    let caret = view.update(cx, |view, _| {
        view.layout
            .as_mut()
            .unwrap()
            .caret_bounds(long_end)
            .unwrap()
    });
    assert_near(wrapped.bounds.left(), caret.left() + em);
    assert_near(wrapped.bounds.top(), caret.top());
    assert_eq!(wrapped.text, "on the wrapped row");
    view.update(cx, |view, cx| {
        view.model_mut().set_soft_wrap(false);
        cx.notify();
    });
    draw(cx);

    // A long message ends in an ellipsis inside the right edge, and with a
    // minimap there, inside that.
    diagnose(
        &view,
        vec![(0..5, Diagnostic::new(Severity::Error, message.clone()))],
        cx,
    );
    let right = |view: &Entity<EditorView>, cx: &mut VisualTestContext| {
        view.update(cx, |view, _| {
            let layout = view.layout.as_ref().unwrap();
            let visible = layout.frame.text_bounds();
            layout.bounds(visible).right()
        })
    };
    let cut = messages(&view, cx).remove(0);
    assert!(cut.text.ends_with('\u{2026}'), "{}", cut.text);
    assert!(cut.text.len() < message.len());
    let edge = right(&view, cx);
    assert!(
        cut.bounds.right() <= edge + px(0.01),
        "{:?} > {edge:?}",
        cut.bounds
    );
    assert!(
        edge - cut.bounds.right() < em * 2.,
        "{:?} far from {edge:?}",
        cut.bounds
    );

    view.update(cx, |view, cx| {
        view.set_minimap(Some(MinimapOptions::default()), cx);
    });
    draw(cx);
    let narrowed = messages(&view, cx).remove(0);
    let (narrower, minimap_left) = (
        right(&view, cx),
        view.read_with(cx, |view, _| view.minimap_bounds().unwrap().left()),
    );
    assert!(narrower < edge);
    assert!(narrowed.bounds.right() <= minimap_left + px(0.01));
    assert!(narrowed.text.len() < cut.text.len());
    view.update(cx, |view, cx| view.set_minimap(None, cx));
    draw(cx);

    // Under eight chars of room, drawn at nine, not at seven.
    let width = |cx: &mut VisualTestContext, view: &Entity<EditorView>, chars: f32| {
        let at = caret_x(view, 5, cx) + em;
        let wide = cx.update(|window, _| window.bounds().size.width);
        let edge = right(view, cx);
        cx.simulate_resize(size(wide - (edge - at) + em * chars, px(400.)));
        draw(cx);
    };
    width(cx, &view, 9.);
    assert_eq!(messages(&view, cx).len(), 1);
    width(cx, &view, 7.);
    assert_eq!(messages(&view, cx), []);
    cx.simulate_resize(size(px(800.), px(400.)));
    draw(cx);
    assert_eq!(messages(&view, cx).len(), 1);

    // A folded row draws nothing.
    diagnose(
        &view,
        vec![(
            text.find("folded").unwrap()..text.find("  body").unwrap() - 1,
            Diagnostic::new(Severity::Error, "folded row"),
        )],
        cx,
    );
    assert_eq!(shown(&view, cx), owned(&[(2, "folded row")]));
    let fold_row = view.update(cx, |view, cx| {
        let model = view.model_mut();
        model.fold(&|_| Some(2..=3));
        cx.notify();
        model.display().folds().len()
    });
    assert_eq!(fold_row, 1);
    draw(cx);
    assert_eq!(shown(&view, cx), owned(&[]));
    view.update(cx, |view, cx| {
        view.model_mut().unfold_rows(2..=3);
        cx.notify();
    });
    draw(cx);
    assert_eq!(shown(&view, cx), owned(&[(2, "folded row")]));

    // Past the horizontal scroll: the row's end is left of the text.
    diagnose(
        &view,
        vec![
            (0..5, Diagnostic::new(Severity::Error, "short row")),
            (6..long_end, Diagnostic::new(Severity::Error, "long row")),
        ],
        cx,
    );
    view.update(cx, |view, cx| {
        view.scroll.x = px(1000.);
        view.autoscroll = false;
        cx.notify();
    });
    draw(cx);
    assert_eq!(
        shown(&view, cx).iter().filter(|(row, _)| *row == 0).count(),
        0
    );
}

#[gpui::test]
fn the_overlay_changes_neither_the_layout_nor_the_minimap(cx: &mut TestAppContext) {
    let text = "let a = 1;\nlet b = 2;\nlet c = 3;\n".repeat(10);
    let (view, cx) = editor(cx, &text);
    view.update(cx, |view, cx| {
        view.set_minimap(Some(MinimapOptions::default()), cx);
    });
    let items: Vec<_> = (0..30)
        .map(|row| {
            let start = text
                .lines()
                .take(row)
                .map(|line| line.len() + 1)
                .sum::<usize>();
            (
                start + 4..start + 5,
                Diagnostic::new(Severity::Error, format!("unused variable {row}")),
            )
        })
        .collect();
    diagnose(&view, items, cx);

    let snapshot = |view: &Entity<EditorView>, cx: &mut VisualTestContext| {
        view.update(cx, |view, _| {
            let minimap = view.painted_minimap.clone().expect("the minimap painted");
            let layout = view.layout.as_mut().unwrap();
            let rows: Vec<_> = (layout.frame.rows.iter())
                .map(|row| (row.row, row.buffer_row, row.start, row.end))
                .collect();
            let caret = layout.caret_bounds(4).unwrap();
            (rows, caret, minimap.runs, minimap.tiles)
        })
    };
    let on = snapshot(&view, cx);
    let found = messages(&view, cx);
    assert!(!found.is_empty());
    let over = found[0].bounds.center();

    level(&view, InlineDiagnostics::Off, cx);
    assert_eq!(messages(&view, cx), []);
    let off = snapshot(&view, cx);
    assert_eq!(on.0, off.0);
    assert_eq!(on.1, off.1);
    assert_eq!(on.2, off.2);
    assert_eq!(on.3, off.3);

    level(&view, InlineDiagnostics::Warning, cx);

    // A click on the message puts the caret at the row's end, as without it.
    cx.simulate_mouse_move(over, None, Modifiers::none());
    cx.simulate_mouse_down(over, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(over, MouseButton::Left, Modifiers::none());
    draw(cx);
    let end = text.find('\n').unwrap();
    assert_eq!(
        view.read_with(cx, |view, _| view.model().primary_selection().head()),
        end
    );

    // Hovering on it shows no popover, there being no text under it; hovering
    // on the squiggle shows its message, as ever.
    cx.executor().advance_clock(HOVER_DELAY * 2);
    draw(cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.painted_popover.clone()),
        None
    );
    let squiggle = view.update(cx, |view, _| {
        let caret = view.layout.as_mut().unwrap().caret_bounds(4).unwrap();
        point(caret.left() + px(1.), caret.center().y)
    });
    cx.simulate_mouse_move(squiggle, None, Modifiers::none());
    cx.executor().advance_clock(HOVER_DELAY * 2);
    draw(cx);
    let popover = view.read_with(cx, |view, _| view.painted_popover.clone());
    assert!(popover.is_some_and(|text| text.contains("unused variable 0")));
}

#[gpui::test]
fn only_visible_diagnostic_rows_are_shaped(cx: &mut TestAppContext) {
    let text = "let value = 1;\n".repeat(10_000);
    let (view, cx) = editor(cx, &text);
    let line_height = view.update(cx, |view, _| {
        view.layout.as_ref().unwrap().frame.line_height
    });
    cx.simulate_resize(size(px(800.), px(line_height * 50.)));
    draw(cx);
    let items: Vec<_> = (0..10_000)
        .map(|row| {
            let start = row * "let value = 1;\n".len();
            (
                start + 4..start + 9,
                Diagnostic::new(Severity::Warning, format!("unused value on row {row}")),
            )
        })
        .collect();
    diagnose(&view, items, cx);
    let rows = view.update(cx, |view, _| view.layout.as_ref().unwrap().frame.rows.len());
    assert!((50..=52).contains(&rows), "{rows} rows");
    let painted = view.read_with(cx, |view, _| view.painted_inline_diagnostics.clone());
    assert_eq!(painted.messages.len(), rows);
    assert!(
        painted.shapes <= rows,
        "{} shapes for {rows} rows",
        painted.shapes
    );

    // The same text in the same colour is shaped once.
    let same: Vec<_> = (0..10_000)
        .map(|row| {
            let start = row * "let value = 1;\n".len();
            (
                start + 4..start + 9,
                Diagnostic::new(Severity::Warning, "unused"),
            )
        })
        .collect();
    diagnose(&view, same, cx);
    let painted = view.read_with(cx, |view, _| view.painted_inline_diagnostics.clone());
    assert_eq!(painted.messages.len(), rows);
    assert_eq!(painted.shapes, 1);
}

#[gpui::test]
fn a_long_rows_slice_draws_nothing(cx: &mut TestAppContext) {
    let long = "x".repeat(fe_display::layout::MAX_SHAPED_ROW_BYTES * 2);
    let text = format!("{long}\nshort\n");
    let (view, cx) = editor(cx, &text);
    diagnose(
        &view,
        vec![
            (0..4, Diagnostic::new(Severity::Error, "long row")),
            (
                long.len() + 1..long.len() + 6,
                Diagnostic::new(Severity::Error, "short row"),
            ),
        ],
        cx,
    );
    // Scrolled so the row's end is in view, it's still a slice.
    view.update(cx, |view, cx| {
        view.scroll.x = px(f32::MAX);
        view.autoscroll = false;
        cx.notify();
    });
    draw(cx);
    let sliced = view.update(cx, |view, _| {
        let row = &view.layout.as_ref().unwrap().frame.rows[0];
        row.slice.is_some()
    });
    assert!(sliced);
    assert_eq!(
        messages(&view, cx)
            .iter()
            .filter(|m| m.buffer_row == 0)
            .count(),
        0
    );
}

/// Text after the first row.
struct After;

impl DecorationProvider for After {
    fn decorations(&self, snapshot: &BufferSnapshot, _: Range<usize>, out: &mut Vec<Decoration>) {
        let row = 0..snapshot.text().row_len(0) as usize;
        out.push(Decoration::new(
            row,
            DecorationKind::AfterLine("blame".into()),
            Tint::Warning,
        ));
    }
}

#[gpui::test]
fn a_message_follows_what_else_is_drawn_after_the_line(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "first\nsecond\n");
    diagnose(
        &view,
        vec![
            (0..5, Diagnostic::new(Severity::Error, "on first")),
            (6..12, Diagnostic::new(Severity::Error, "on second")),
        ],
        cx,
    );
    let em = em(&view, cx);

    // After a hint at the row's end, which the row's line holds.
    let version = view.read_with(cx, |view, _| view.model().snapshot().version());
    view.update(cx, |view, cx| {
        let hint = InlayHint {
            offset: 12,
            label: ": hint".to_string(),
            kind: None,
            padding_left: false,
            padding_right: false,
            data: None,
            tooltip: None,
            edits: Vec::new(),
        };
        view.set_inlay_hints(version, vec![hint]);
        cx.notify();
    });
    draw(cx);
    let hint = view.read_with(cx, |view, _| view.painted_inlay_hints[0].1);
    let second = messages(&view, cx).remove(1);
    assert!(second.bounds.left() >= hint.right() + em - px(0.01));

    // After a decoration's text, which starts a row later than the line ends.
    view.update(cx, |view, cx| {
        view.model_mut()
            .add_decoration_provider(Rc::new(RefCell::new(After)));
        cx.notify();
    });
    draw(cx);
    let label = view.update(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        (layout.decorations.placed.iter())
            .find(|placed| matches!(placed.kind, DecorationKind::AfterLine(_)))
            .unwrap()
            .bounds
    });
    let first = messages(&view, cx).remove(0);
    assert_near(first.bounds.left(), label.right() + em);
    assert_near(first.bounds.top(), label.top());
}

/// A line as a proportional font shapes it: `pair_width` a pair of chars when
/// kerned together, which is less than the chars measured one by one.
struct Kerned {
    pair_width: f32,
    len: usize,
    ellipsis: f32,
}

impl MeasuredLine for Kerned {
    fn width(&self) -> f32 {
        self.x_for_index(self.len) + self.ellipsis
    }

    fn x_for_index(&self, index: usize) -> f32 {
        index.min(self.len) as f32 * self.pair_width / 2.
    }

    fn index_for_x(&self, x: f32) -> usize {
        ((x / (self.pair_width / 2.)).round().max(0.) as usize).min(self.len)
    }
}

#[test]
fn cuts_come_from_the_shaped_line_so_kerned_text_that_fits_stays_whole() {
    // 50 chars, 18.68 apiece alone but 16.5 kerned: 934 by the sum of chars,
    // 825 shaped, in 880 of room.
    let text = "AV".repeat(25);
    let line = Kerned {
        pair_width: 33.,
        len: text.len(),
        ellipsis: 10.,
    };
    assert_eq!(
        keep(&line, &text, true, 880.),
        Kept {
            len: 50,
            end: 825.,
            ellipsis: None,
        }
    );

    // Cut, the ellipsis ends within a char of the edge, not short of it by
    // the kerning of every char before it.
    let kept = keep(&line, &text, true, 500.);
    assert_eq!(kept.ellipsis, Some((825., 10.)));
    assert!(kept.width() <= 500.);
    assert!(500. - kept.width() < 16.5, "{kept:?}");
    // A prefix that isn't all of the message has an ellipsis though it fits.
    let kept = keep(&line, &text, false, 880.);
    assert_eq!(kept.len, 50);
    assert_eq!(kept.ellipsis, Some((825., 10.)));
    // Never within a grapheme, whichever byte the x is nearest.
    let accents = "e\u{301}".repeat(10);
    let line = Kerned {
        pair_width: 10.,
        len: accents.len(),
        ellipsis: 5.,
    };
    let kept = keep(&line, &accents, true, 28.);
    assert_eq!(kept.len % 3, 0, "{kept:?}");
}

/// Text after every row.
struct AfterEvery;

impl DecorationProvider for AfterEvery {
    fn decorations(&self, snapshot: &BufferSnapshot, _: Range<usize>, out: &mut Vec<Decoration>) {
        let text = snapshot.text();
        for row in 0..text.row_count() {
            let start = text.row_start(row);
            out.push(Decoration::new(
                start..start + text.row_len(row) as usize,
                DecorationKind::AfterLine("n".into()),
                Tint::Warning,
            ));
        }
    }
}

#[gpui::test]
fn overlapping_diagnostics_and_dense_folds_take_one_pass(cx: &mut TestAppContext) {
    const ROWS: usize = 100;
    const MANY: usize = 2_000;
    let text = "line\n".repeat(ROWS);
    let (view, cx) = editor(cx, &text);
    // All over the whole file, the worst last, so each row sees them all.
    let mut items: Vec<_> = (0..MANY)
        .map(|n| {
            (
                0..text.len() - 1,
                Diagnostic::new(Severity::Warning, format!("spanning {n}")),
            )
        })
        .collect();
    items.push((0..text.len() - 1, Diagnostic::new(Severity::Error, "worst")));
    diagnose(&view, items, cx);
    let painted = view.read_with(cx, |view, _| view.painted_inline_diagnostics.clone());
    let rows = view.update(cx, |view, _| view.layout.as_ref().unwrap().frame.rows.len());
    assert_eq!(painted.messages.len(), rows);
    assert!(
        painted
            .messages
            .iter()
            .all(|message| message.text == "worst")
    );
    assert_eq!(painted.shapes, 1);
    // Each diagnostic looked at once and joining once, each row once, where
    // a scan of the rows for each would take MANY times the rows.
    assert!(
        painted.steps <= (MANY + 1) * 2 + rows,
        "{} steps for {rows} rows",
        painted.steps
    );

    // Every other row folded, with text after every row.
    view.update(cx, |view, cx| {
        view.model_mut()
            .add_decoration_provider(Rc::new(RefCell::new(AfterEvery)));
        view.model_mut()
            .fold_all_exact((0..ROWS as u32 - 1).step_by(2).map(|row| row..=row + 1));
        cx.notify();
    });
    draw(cx);
    let painted = view.read_with(cx, |view, _| view.painted_inline_diagnostics.clone());
    let (rows, markers) = view.update(cx, |view, _| {
        let frame = &view.layout.as_ref().unwrap().frame;
        (frame.rows.len(), frame.fold_markers.len())
    });
    assert!(markers > 10, "{markers} folds");
    assert_eq!(painted.messages, []);
    assert!(
        painted.steps <= (MANY + 1) * 2 + rows + markers + rows * 2,
        "{} steps for {rows} rows and {markers} folds",
        painted.steps
    );
}

#[gpui::test]
fn what_paints_is_the_shaped_glyphs_kept_and_the_ellipsis_moved_up_to_them(
    cx: &mut TestAppContext,
) {
    let (view, cx) = editor(cx, "short\nother\n");
    let long = "m".repeat(400);
    diagnose(
        &view,
        vec![
            (0..5, Diagnostic::new(Severity::Error, "wolf")),
            (6..11, Diagnostic::new(Severity::Error, long.clone())),
        ],
        cx,
    );
    let found = messages(&view, cx);

    // Fits: its glyphs where they were shaped, and no ellipsis, whose glyph
    // would be at the message's length.
    let whole = &found[0];
    assert_eq!(
        (whole.glyphs.iter())
            .map(|(index, _)| *index)
            .collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
    assert_eq!(whole.glyphs[0].1, px(0.));

    // Cut: only the kept glyphs, then the ellipsis, at the kept text's end,
    // not at the end of the message it was shaped after.
    let cut = &found[1];
    let (&(ellipsis, at), kept) = cut.glyphs.split_last().unwrap();
    assert!(ellipsis > kept.len(), "{ellipsis}: past the text left out");
    assert_eq!(kept.len(), cut.text.len() - '\u{2026}'.len_utf8());
    assert!(kept.iter().enumerate().all(|(ix, (index, _))| ix == *index));
    let advance = kept[1].1 - kept[0].1;
    assert_near(at, kept.last().unwrap().1 + advance);
    assert!(at < cut.bounds.size.width);
}
