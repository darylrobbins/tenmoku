//! Lens rows in a headless window: a provider's items on a row of their own
//! above a line, following the text and folds, a click on one running its
//! command.

use std::{cell::RefCell, rc::Rc};

use fe_buffer::BufferSnapshot;
use fe_command::{Dispatched, Effect, Invocation, Source};
use gpui::{Bounds, Entity, Pixels, TestAppContext, VisualTestContext, point, px};

use super::{caret, click, editor_on};
use crate::{
    EditorView, Lens, LensItem, LensProvider, commands::observe_invocations, lens::PaintedRow,
};

/// Five rows, the third indented.
const TEXT: &str = "fn a() {\nfn b() {\n    target();\n}\n}";

/// A lens over each row that has `target`, with two items.
struct Targets;

impl LensProvider for Targets {
    fn lenses(&mut self, snapshot: &BufferSnapshot) -> Vec<Lens> {
        let text = snapshot.text();
        let item = |title: &'static str, command| LensItem {
            title: title.into(),
            invocation: Invocation::of(command).unwrap(),
        };
        (0..text.row_count())
            .filter(|&row| text.row_to_string(row).contains("target"))
            .map(|row| Lens {
                row,
                items: vec![
                    item("3 references", "test.lens_first"),
                    item("Run", "test.lens_second"),
                ],
            })
            .collect()
    }
}

fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (view, cx, _) = editor_on(TEXT, TEXT, cx);
    (view, cx)
}

fn add_lenses(view: &Entity<EditorView>, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        view.add_lens_provider(Rc::new(RefCell::new(Targets)));
        cx.notify();
    });
    draw(cx);
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// The window y each buffer row shown starts at.
fn row_tops(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<(u32, Pixels)> {
    view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        let frame = &layout.frame;
        (frame.rows.iter())
            .map(|row| (row.buffer_row, layout.origin.y + px(frame.row_top(row.row))))
            .collect()
    })
}

fn line_height(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Pixels {
    view.read_with(cx, |view, _| {
        px(view.layout.as_ref().unwrap().frame.line_height)
    })
}

fn lens_rows(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<PaintedRow> {
    view.read_with(cx, |view, _| view.lens_rows.clone())
}

/// The caret's box at `offset` in the last frame.
fn caret_at(
    view: &Entity<EditorView>,
    offset: usize,
    cx: &mut VisualTestContext,
) -> Bounds<Pixels> {
    view.update(cx, |view, _| {
        view.layout.as_mut().unwrap().caret_bounds(offset).unwrap()
    })
}

#[gpui::test]
fn a_lens_row_sits_above_its_row(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let before = row_tops(&view, cx);
    add_lenses(&view, cx);
    let after = row_tops(&view, cx);
    let line_height = line_height(&view, cx);
    assert_eq!(after[..2], before[..2]);
    assert_eq!(after[2], (2, before[2].1 + line_height));
    assert_eq!(after[3], (3, before[3].1 + line_height));

    let rows = lens_rows(&view, cx);
    assert_eq!(rows.len(), 1);
    let titles: Vec<_> = rows[0]
        .items
        .iter()
        .map(|(item, _)| item.title.as_ref())
        .collect();
    assert_eq!(titles, ["3 references", "Run"]);
    let indent = caret_at(&view, TEXT.find("target").unwrap(), cx).left();
    let (first, second) = (rows[0].items[0].1, rows[0].items[1].1);
    assert!(
        (first.left() - indent).abs() < px(0.01),
        "{first:?} at {indent:?}"
    );
    assert!(second.left() > first.right(), "the separator between");
    for (_, bounds) in &rows[0].items {
        assert_eq!(bounds.top(), before[2].1);
        assert_eq!(bounds.size.height, line_height);
    }
}

#[gpui::test]
fn clicking_an_item_runs_its_command(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    cx.update(|_, cx| {
        kiln_platform::commands::register_app_commands(cx, |registry| {
            for id in ["test.lens_first", "test.lens_second"] {
                let ran = |_: &mut &mut gpui::App, _: &_| Ok(Dispatched::new(Effect::Nothing));
                registry.register(id, id, ran).unwrap();
            }
        });
    });
    let seen = Rc::new(RefCell::new(Vec::<Invocation>::new()));
    let sink = seen.clone();
    cx.update(|_, cx| {
        observe_invocations(cx, move |invocation| {
            sink.borrow_mut().push(invocation.clone())
        })
    });
    add_lenses(&view, cx);
    let before = caret(&view, cx);
    let rows = lens_rows(&view, cx);
    let (first, second) = (rows[0].items[0].1, rows[0].items[1].1);

    click(second.center(), cx);
    let ran = std::mem::take(&mut *seen.borrow_mut());
    assert_eq!(ran.len(), 1, "{ran:?}");
    assert_eq!(ran[0].command.as_str(), "test.lens_second");
    assert_eq!(ran[0].source, Source::Mouse);
    assert_eq!(caret(&view, cx), before);
    assert!(view.read_with(cx, |view, _| view.drag.is_none()));

    let between = point((first.right() + second.left()) / 2., first.center().y);
    click(between, cx);
    assert_eq!(*seen.borrow(), []);
    assert_eq!(caret(&view, cx), before);
}

#[gpui::test]
fn lenses_follow_the_text_and_folds(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    add_lenses(&view, cx);
    let row_of_target = |view: &Entity<EditorView>, cx: &mut VisualTestContext| {
        let tops = row_tops(view, cx);
        let rows = lens_rows(view, cx);
        let top = rows[0].items[0].1.top();
        let line_height = line_height(view, cx);
        (tops.iter())
            .find(|(_, at)| *at == top + line_height)
            .map(|&(row, _)| row)
    };
    assert_eq!(row_of_target(&view, cx), Some(2));

    view.update(cx, |view, cx| {
        view.model.set_cursor(0);
        view.model.insert("new\n");
        view.changed(cx);
    });
    draw(cx);
    assert_eq!(row_of_target(&view, cx), Some(3));

    view.update(cx, |view, cx| {
        view.model.fold_rows(2..=4);
        view.changed(cx);
    });
    draw(cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.shows_buffer_row(3)),
        Some(false)
    );
    assert!(lens_rows(&view, cx).is_empty());
    let tops = row_tops(&view, cx);
    let line_height = line_height(&view, cx);
    let gaps = tops
        .windows(2)
        .all(|pair| pair[1].1 - pair[0].1 == line_height);
    assert!(gaps, "no lens row's room left: {tops:?}");
}
