//! Lens rows measure and shape only what can show (T858 F2), and an item's
//! underline goes when the pointer leaves the window (T858 F3).

use std::{cell::RefCell, rc::Rc};

use fe_buffer::{Buffer, BufferSnapshot};
use fe_command::Invocation;
use fe_display::layout::MAX_SHAPED_ROW_BYTES;
use fe_editor::EditorModel;
use gpui::{
    Entity, Focusable as _, Modifiers, MouseExitEvent, SharedString, TestAppContext,
    VisualTestContext, px, size,
};

use crate::{EditorTheme, EditorView, Lens, LensItem, LensProvider, init, lens::PaintedRow};

/// A lens with `titles` over the row that has `target`.
struct Titled(Vec<SharedString>);

impl LensProvider for Titled {
    fn lenses(&mut self, snapshot: &BufferSnapshot) -> Vec<Lens> {
        let text = snapshot.text();
        let item = |title: &SharedString| LensItem {
            title: title.clone(),
            invocation: Invocation::of("test.lens").unwrap(),
        };
        (0..text.row_count())
            .filter(|&row| text.row_to_string(row).contains("target"))
            .map(|row| Lens {
                row,
                items: self.0.iter().map(item).collect(),
            })
            .collect()
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// An 800×400 editor on `text` with a lens of `titles` over its `target` row.
fn editor<'a>(
    text: String,
    titles: &[&str],
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let titles: Vec<SharedString> = titles
        .iter()
        .map(|&title| title.to_owned().into())
        .collect();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&text), Some("main.rs".into()));
        let mut view = EditorView::new(model, EditorTheme::default_dark(cx), cx);
        view.add_lens_provider(Rc::new(RefCell::new(Titled(titles))));
        view
    });
    cx.simulate_resize(size(px(800.), px(400.)));
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    draw(cx);
    (view, cx)
}

fn lens_rows(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<PaintedRow> {
    view.read_with(cx, |view, _| view.lens_rows.clone())
}

#[gpui::test]
fn a_lens_past_a_huge_indent_shapes_nothing(cx: &mut TestAppContext) {
    let text = format!("fn a() {{\n{}target();\n}}\n", " ".repeat(500_000));
    let (view, cx) = editor(text, &["3 references", "Run"], cx);
    let rows = lens_rows(&view, cx);
    assert_eq!(rows.len(), 1);
    // Past the view's right edge: neither item is shown nor shaped.
    assert!(rows[0].items.is_empty(), "{:?}", rows[0].items);
    assert_eq!(rows[0].shaped, Vec::<usize>::new());
}

#[gpui::test]
fn a_long_lens_title_is_shaped_around_the_view(cx: &mut TestAppContext) {
    let long = "x".repeat(200_000);
    let text = "fn a() {\n    target();\n}\n".to_owned();
    let (view, cx) = editor(text, &[&long, "Run"], cx);
    let rows = lens_rows(&view, cx);
    assert_eq!(rows.len(), 1);
    // The long title runs past the view: only it, cut short, is shaped.
    assert_eq!(rows[0].shaped.len(), 1);
    assert!(
        rows[0].shaped[0] <= MAX_SHAPED_ROW_BYTES,
        "{:?}",
        rows[0].shaped
    );
    assert!(rows[0].shaped[0] > 0);
    let (item, shown) = &rows[0].items[0];
    assert_eq!(item.title.len(), long.len());
    assert_eq!(shown.right(), rows[0].bounds.right(), "to the view's edge");
}

#[gpui::test]
fn an_underline_goes_when_the_pointer_leaves_the_window(cx: &mut TestAppContext) {
    let text = "fn a() {\n    target();\n}\n".to_owned();
    let (view, cx) = editor(text, &["3 references", "Run"], cx);
    let first = lens_rows(&view, cx)[0].items[0].1;
    cx.simulate_mouse_move(first.center(), None, Modifiers::none());
    draw(cx);
    assert_eq!(lens_rows(&view, cx)[0].underlined, [true, false]);

    cx.simulate_event(MouseExitEvent {
        position: first.center(),
        pressed_button: None,
        modifiers: Modifiers::none(),
    });
    draw(cx);
    assert_eq!(lens_rows(&view, cx)[0].underlined, [false, false]);
}
