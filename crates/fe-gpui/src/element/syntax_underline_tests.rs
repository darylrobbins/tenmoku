//! A syntax-coloured underline: placed straight on the underlines layer, in
//! the theme's colour for its highlight name, or the foreground without one.

use std::{cell::RefCell, ops::Range, rc::Rc};

use fe_buffer::Buffer;
use fe_editor::{
    EditorModel,
    decorations::{Decoration, DecorationKind, DecorationProvider, Tint, overlaps},
};
use gpui::{Entity, TestAppContext, VisualTestContext, hsla};

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
                .filter(|item| overlaps(&item.range, &range))
                .cloned(),
        );
    }
}

fn index(name: &str) -> usize {
    fe_syntax::Highlight::from_capture(name).unwrap().index()
}

/// An editor on `one two` whose theme colours `variable` but not `function`.
fn editor(
    items: Vec<Decoration>,
    cx: &mut TestAppContext,
) -> (Entity<EditorView>, &mut VisualTestContext, EditorTheme) {
    cx.update(init);
    let provider = Rc::new(RefCell::new(Fixed(items)));
    let mut theme = cx.update(|cx| EditorTheme::default_dark(cx));
    let len = theme
        .syntax
        .len()
        .max(index("variable") + 1)
        .max(index("function") + 1);
    theme.syntax.resize(len, None);
    theme.syntax[index("variable")] = Some(hsla(0.3, 0.8, 0.5, 1.));
    theme.syntax[index("function")] = None;
    let shown = theme.clone();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new("one two"), None);
        model.add_decoration_provider(provider);
        EditorView::new(model, theme, cx)
    });
    cx.run_until_parked();
    PAINTED.with(|painted| painted.borrow_mut().clear());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    (view, cx, shown)
}

fn underline(range: Range<usize>, name: &str) -> Decoration {
    Decoration::new(
        range,
        DecorationKind::SyntaxUnderline(name.into()),
        Tint::Muted,
    )
}

#[gpui::test]
fn it_is_straight_in_the_names_colour_on_the_underlines_layer(cx: &mut TestAppContext) {
    let (view, cx, theme) = editor(vec![underline(0..3, "variable")], cx);
    let all = view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().decorations.placed.clone()
    });
    assert_eq!(all.len(), 1);
    assert_eq!(
        all[0].color,
        theme
            .syntax_color(fe_syntax::Highlight::from_capture("variable").unwrap())
            .unwrap()
    );
    let style = underline_style(&all[0]);
    assert!(!style.wavy);
    assert_eq!(style.color, Some(all[0].color));
    assert!(
        PAINTED
            .with(|painted| painted.take())
            .contains(&"underlines")
    );
}

#[gpui::test]
fn a_name_without_a_colour_underlines_in_the_foreground(cx: &mut TestAppContext) {
    let (view, cx, theme) = editor(
        vec![underline(0..3, "function"), underline(4..7, "no.such.name")],
        cx,
    );
    let all = view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().decorations.placed.clone()
    });
    assert_eq!(all.len(), 2);
    for placed in &all {
        assert_eq!(placed.color, theme.foreground);
    }
}
