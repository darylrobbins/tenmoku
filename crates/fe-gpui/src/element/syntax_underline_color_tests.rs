//! A syntax underline is painted in the colour of the text it underlines:
//! the name's own colour when the theme has one, else the colour the text
//! under it is painted in, run by run.

use std::{cell::RefCell, ops::Range, rc::Rc};

use fe_buffer::Buffer;
use fe_config::{EditorPalette, themes::find};
use fe_editor::{
    EditorModel,
    decorations::{Decoration, DecorationKind, DecorationProvider, Tint, overlaps},
};
use gpui::{Entity, Focusable as _, Hsla, TestAppContext, VisualTestContext};
use kiln_core::theme::ThemeData;

use crate::{EditorSettings, EditorTheme, EditorView, init};

const TEXT: &str = "impl S {\n    fn bump(&mut self) {}\n}\n";

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

fn word(word: &str) -> Range<usize> {
    let start = TEXT.find(word).unwrap();
    start..start + word.len()
}

/// A mutable underline as the semantic tokens make it: `self` is named
/// nothing by rust-analyzer's mapping at base, `bump` is a function.
fn underline(range: Range<usize>, name: &str) -> Decoration {
    Decoration::new(
        range,
        DecorationKind::SyntaxUnderline(name.into()),
        Tint::Muted,
    )
}

fn check(name: &str, cx: &mut TestAppContext) {
    cx.update(init);
    let palette = match name {
        "tenmoku-dark" => EditorPalette::fastedit_dark().clone(),
        other => {
            let data = ThemeData::from_file(&find(other).expect("bundled").file());
            EditorPalette::for_theme(&data).0
        }
    };
    let provider = Rc::new(RefCell::new(Fixed(vec![
        underline(word("self"), ""),
        underline(word("bump"), "function"),
    ])));
    let theme = cx.update(|cx| EditorTheme::from_palette(&palette, &EditorSettings::default(), cx));
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(TEXT), None);
        model.add_decoration_provider(provider);
        EditorView::new(model, theme, cx)
    });
    view.update(cx, |view, cx| view.set_language(Some("rust"), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let self_color = painted(&view, word("self").start, cx);
    let function_color = painted(&view, word("bump").start, cx);
    let background = view.read_with(cx, |view, _| view.theme.foreground);
    assert_ne!(
        self_color, background,
        "{name}: self has a colour of its own"
    );
    let placed = view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().decorations.placed.clone()
    });
    assert_eq!(placed.len(), 2, "{name}");
    assert_eq!(placed[0].color, function_color, "{name}: bump");
    assert_eq!(placed[1].color, self_color, "{name}: self");
}

fn painted(view: &Entity<EditorView>, offset: usize, cx: &mut VisualTestContext) -> Hsla {
    view.read_with(cx, |view, _| view.drawn_color(offset).unwrap())
}

#[gpui::test]
fn a_mutable_self_underlines_in_the_colour_of_self_in_fastedit_dark(cx: &mut TestAppContext) {
    check("tenmoku-dark", cx);
}

#[gpui::test]
fn a_mutable_self_underlines_in_the_colour_of_self_in_vscode_2026_dark(cx: &mut TestAppContext) {
    check("vscode-2026-dark", cx);
}

/// Ghost text inserted inside an unnamed underline that crosses several
/// syntax colours: the underline is split around it, no piece lends the
/// ghost text its colour, and none of them covers it.
#[gpui::test]
fn an_inlay_inside_the_underline_is_left_out_of_it(cx: &mut TestAppContext) {
    use fe_buffer::Origin;
    use fe_editor::ghost_text::GhostText;

    cx.update(init);
    let at = word("self").start;
    let range = word("bump").start..word("self").end;
    let provider = Rc::new(RefCell::new(Fixed(vec![underline(range, "")])));
    let theme = cx.update(|cx| EditorTheme::default_dark(cx));
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(TEXT), None);
        model.add_decoration_provider(provider);
        model.set_cursor(at);
        EditorView::new(model, theme, cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    view.update(cx, |view, cx| {
        view.set_language(Some("rust"), cx);
        let origin = Origin::Ai {
            agent: "agent".into(),
            session: "session".into(),
        };
        let ghost = GhostText::new(view.model().snapshot(), at, "xyz", origin);
        view.set_ghost_text(Some(ghost), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let (inlay, text, ghost_width, mut placed) = view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        let row = layout
            .frame
            .rows
            .iter()
            .find(|row| row.contains(at))
            .unwrap();
        let inlay = row.expanded.inlay_range().expect("ghost text is inline");
        (
            row.line.color_at(inlay.start).unwrap(),
            row.line.color_at(inlay.end).unwrap(),
            row.ghost_width() as f32,
            layout.decorations.placed.clone(),
        )
    });
    assert_ne!(inlay, text, "the ghost text is painted apart from self");
    placed.sort_by(|a, b| a.bounds.origin.x.partial_cmp(&b.bounds.origin.x).unwrap());
    let colors: Vec<_> = placed.iter().map(|p| p.color).collect();
    assert!(
        !colors.contains(&inlay),
        "no piece takes the ghost text's colour"
    );
    assert_eq!(colors.last(), Some(&text), "self's piece is last");
    let mut distinct = colors.clone();
    distinct.dedup();
    assert!(distinct.len() >= 3, "several syntax colours: {colors:?}");
    // The pieces cover the underlined text but not the ghost text between
    // them: what's left of the extent is its width.
    let left = placed[0].bounds.origin.x;
    let last = placed.last().unwrap();
    let extent: f32 = (last.bounds.origin.x + last.bounds.size.width - left).into();
    let covered: f32 = placed.iter().map(|p| f32::from(p.bounds.size.width)).sum();
    assert!(ghost_width > 0.);
    assert!(
        (extent - covered - ghost_width).abs() < 1.,
        "{extent} - {covered} should leave the ghost text's {ghost_width}"
    );
}
