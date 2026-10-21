use std::{cell::RefCell, ops::Range, rc::Rc};

use fe_buffer::Buffer;
use fe_editor::{
    EditorModel,
    decorations::{Decoration, DecorationKind, DecorationProvider, Tint},
};
use gpui::{Entity, Hsla, TestAppContext, VisualTestContext};

use crate::*;

const TEXT: &str = "fn main() { let value = parse(input); }\n";

/// The ranges a provider is asked for.
type Asked = Rc<RefCell<Vec<Range<usize>>>>;

/// Decorations handed out as they are, whatever the range.
struct Fixed(Rc<RefCell<Vec<Decoration>>>, Asked);

impl DecorationProvider for Fixed {
    fn decorations(
        &self,
        _snapshot: &fe_buffer::BufferSnapshot,
        range: Range<usize>,
        out: &mut Vec<Decoration>,
    ) {
        self.1.borrow_mut().push(range);
        out.extend(self.0.borrow().iter().cloned());
    }
}

fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(init);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let model = EditorModel::new(Buffer::new(TEXT), Some("main.rs".into()));
        // The bundled theme leaves both without a colour of their own.
        let mut theme = EditorTheme::default_dark(cx);
        for (name, color) in [("variable.parameter", 0.3), ("function.method", 0.8)] {
            let index = fe_syntax::Highlight::from_capture(name)
                .expect("known")
                .index();
            theme.syntax[index] = Some(gpui::hsla(color, 0.7, 0.6, 1.));
        }
        EditorView::new(model, theme, cx)
    });
    cx.run_until_parked();
    (view, cx)
}

fn drawn(view: &Entity<EditorView>, word: &str, cx: &mut VisualTestContext) -> Hsla {
    cx.run_until_parked();
    let start = TEXT.find(word).expect("word in text");
    view.update(cx, |view, _| view.drawn_color(start).expect("drawn"))
}

fn named(view: &Entity<EditorView>, cx: &mut VisualTestContext, name: &str) -> Hsla {
    view.update(cx, |view, _| {
        let highlight = fe_syntax::Highlight::from_capture(name).expect("known");
        view.theme.syntax_color(highlight).expect("coloured")
    })
}

fn foreground(word: &str, from: usize, len: usize, name: &str) -> Decoration {
    let start = TEXT.find(word).expect("word in text") + from;
    Decoration::new(
        start..start + len,
        DecorationKind::Foreground(name.into()),
        Tint::Muted,
    )
}

#[gpui::test]
fn foreground_decorations_recolour_over_tree_sitter(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let before = [
        drawn(&view, "main", cx),
        drawn(&view, "value", cx),
        drawn(&view, "parse", cx),
        drawn(&view, "input", cx),
    ];
    let function = named(&view, cx, "function");
    assert_eq!(before[2], function);

    let decorations = Rc::new(RefCell::new(vec![foreground(
        "input",
        0,
        5,
        "variable.parameter",
    )]));
    let id = view.update(cx, |view, _| {
        view.model
            .add_decoration_provider(Rc::new(RefCell::new(Fixed(
                decorations.clone(),
                Default::default(),
            ))))
    });
    view.update(cx, |_, cx| cx.notify());
    let parameter = named(&view, cx, "variable.parameter");
    assert_ne!(parameter, before[3]);
    assert_eq!(drawn(&view, "input", cx), parameter);
    assert_eq!(drawn(&view, "parse", cx), function);

    // Part of `value`: the rest of its run keeps its colour.
    decorations
        .borrow_mut()
        .push(foreground("value", 1, 2, "function.method"));
    view.update(cx, |_, cx| cx.notify());
    let method = named(&view, cx, "function.method");
    let value = TEXT.find("value").unwrap();
    let at = |offset: usize, view: &Entity<EditorView>, cx: &mut VisualTestContext| {
        view.update(cx, |view, _| view.drawn_color(offset).expect("drawn"))
    };
    assert_eq!(at(value, &view, cx), before[1]);
    assert_eq!(at(value + 1, &view, cx), method);
    assert_eq!(at(value + 2, &view, cx), method);
    assert_eq!(at(value + 3, &view, cx), before[1]);

    // An unknown name changes nothing.
    decorations.borrow_mut().clear();
    decorations
        .borrow_mut()
        .push(foreground("main", 0, 4, "no.such.name"));
    view.update(cx, |_, cx| cx.notify());
    for (word, color) in ["main", "value", "parse", "input"].iter().zip(before) {
        assert_eq!(drawn(&view, word, cx), color, "{word}");
    }

    // Removing the provider restores every run.
    decorations
        .borrow_mut()
        .push(foreground("input", 0, 5, "variable.parameter"));
    view.update(cx, |_, cx| cx.notify());
    assert_eq!(drawn(&view, "input", cx), parameter);
    view.update(cx, |view, _| view.model.remove_decoration_provider(id));
    view.update(cx, |_, cx| cx.notify());
    for (word, color) in ["main", "value", "parse", "input"].iter().zip(before) {
        assert_eq!(drawn(&view, word, cx), color, "{word}");
    }
}

/// The editor with `decorations` handed out by one provider, and the ranges
/// the provider is asked for.
fn with_provider<'a>(
    text: &str,
    brackets: bool,
    decorations: Vec<Decoration>,
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, Asked, &'a mut VisualTestContext) {
    cx.update(init);
    let text = text.to_string();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&text), Some("main.rs".into()));
        let mut theme = EditorTheme::default_dark(cx);
        theme.bracket_pair_colors = brackets;
        let index = fe_syntax::Highlight::from_capture("variable.parameter")
            .expect("known")
            .index();
        theme.syntax[index] = Some(gpui::hsla(0.3, 0.7, 0.6, 1.));
        EditorView::new(model, theme, cx)
    });
    let asked = Asked::default();
    let provider = Fixed(Rc::new(RefCell::new(decorations)), asked.clone());
    view.update(cx, |view, cx| {
        view.model
            .add_decoration_provider(Rc::new(RefCell::new(provider)));
        cx.notify();
    });
    cx.run_until_parked();
    (view, asked, cx)
}

fn at(view: &Entity<EditorView>, offset: usize, cx: &mut VisualTestContext) -> Hsla {
    view.update(cx, |view, _| view.drawn_color(offset).expect("drawn"))
}

#[gpui::test]
fn later_foreground_decorations_win_where_they_overlap(cx: &mut TestAppContext) {
    let start = TEXT.find("input").unwrap();
    let over = |from: usize, len: usize, name: &str, priority: i32| {
        foreground("input", from, len, name).with_priority(priority)
    };
    let (view, _, cx) = with_provider(
        TEXT,
        false,
        vec![
            over(0, 2, "variable.parameter", 0),
            over(3, 2, "variable.parameter", 0),
            over(0, 5, "string", 1),
            over(2, 1, "number", 2),
        ],
        cx,
    );
    let colour =
        |name: &str, view: &Entity<EditorView>, cx: &mut VisualTestContext| named(view, cx, name);
    let (string, number) = (colour("string", &view, cx), colour("number", &view, cx));
    let drawn: Vec<Hsla> = (0..5).map(|ix| at(&view, start + ix, cx)).collect();
    assert_eq!(drawn, [string, string, number, string, string]);
}

#[gpui::test]
fn foreground_decorations_win_over_bracket_colours(cx: &mut TestAppContext) {
    let open = TEXT.find("(input)").unwrap();
    let wanted = Decoration::new(
        open..open + "(input)".len(),
        DecorationKind::Foreground("variable.parameter".into()),
        Tint::Muted,
    );
    let (view, _, cx) = with_provider(TEXT, true, vec![wanted], cx);
    let parameter = named(&view, cx, "variable.parameter");
    for offset in [open, open + 1, open + "(input)".len() - 1] {
        assert_eq!(at(&view, offset, cx), parameter, "{offset}");
    }
}

#[gpui::test]
fn providers_are_asked_once_for_a_folded_frame(cx: &mut TestAppContext) {
    let text: String = (0..300)
        .map(|row| format!("let r{row} = {row};\n"))
        .collect();
    let (view, asked, cx) = with_provider(&text, false, Vec::new(), cx);
    view.update(cx, |view, _| view.model.fold_rows(10..=40));
    cx.run_until_parked();
    asked.borrow_mut().clear();
    view.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    let shown = view.update(cx, |view, _| {
        let rows = &view.layout.as_ref().expect("drawn").frame.rows;
        rows.first().unwrap().start..rows.last().unwrap().end
    });
    // Each frame drawn asks once, for the rows' range alone, not for the
    // folded gap's two sides.
    let asked = asked.borrow();
    assert!(!asked.is_empty() && asked.iter().all(|range| *range == shown));
}
