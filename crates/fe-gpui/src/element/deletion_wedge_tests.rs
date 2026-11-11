//! A deletion's notch paints in the gutter layer.

use std::{cell::RefCell, ops::Range, rc::Rc};

use fe_buffer::Buffer;
use fe_editor::{
    EditorModel,
    decorations::{Decoration, DecorationProvider, GutterMark, Tint, overlaps},
};
use gpui::TestAppContext;

use super::*;
use crate::{EditorTheme, EditorView, init};

struct Fixed(Vec<Decoration>);

impl DecorationProvider for Fixed {
    fn decorations(
        &self,
        _: &fe_buffer::BufferSnapshot,
        range: Range<usize>,
        out: &mut Vec<Decoration>,
    ) {
        out.extend(
            self.0
                .iter()
                .filter(|d| overlaps(&d.range, &range))
                .cloned(),
        );
    }
}

#[gpui::test]
fn a_drawn_deletion_paints_in_the_gutter_layer(cx: &mut TestAppContext) {
    cx.update(init);
    let text = "one\ntwo\nthree\n";
    let deletion = Decoration::new(
        4..4,
        DecorationKind::Gutter(GutterMark::Deletion),
        Tint::Removed,
    );
    let provider = Rc::new(RefCell::new(Fixed(vec![deletion])));
    let (_view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(text), None);
        model.add_decoration_provider(provider);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.run_until_parked();
    PAINTED.with(|painted| painted.borrow_mut().clear());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(PAINTED.with(|painted| painted.take()).contains(&"gutter"));
}
