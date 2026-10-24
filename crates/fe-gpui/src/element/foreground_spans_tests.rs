//! A `Foreground` decoration whose name the theme leaves uncoloured paints
//! the theme foreground over what is under it.

use fe_editor::decorations::{Decoration, DecorationKind, Tint};
use gpui::TestAppContext;

use super::foreground_spans;
use crate::EditorTheme;

#[gpui::test]
fn an_uncoloured_name_paints_the_foreground(cx: &mut TestAppContext) {
    let theme = cx.update(|cx| EditorTheme::default_dark(cx));
    let decoration =
        |name: &str| Decoration::new(0..5, DecorationKind::Foreground(name.into()), Tint::Accent);
    let uncoloured = fe_syntax::Highlight::all()
        .find(|h| theme.syntax_color(*h).is_none())
        .expect("the dark theme leaves a name to the foreground");
    let spans = foreground_spans(&[decoration(uncoloured.name())], &theme);
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].style, theme.foreground);
    // A name outside the vocabulary is still left out.
    assert!(foreground_spans(&[decoration("no.such.name")], &theme).is_empty());
}
