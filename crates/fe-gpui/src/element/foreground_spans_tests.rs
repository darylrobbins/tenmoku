//! A `Foreground` decoration whose name the theme leaves uncoloured paints
//! the theme foreground over what is under it.

use fe_buffer::Buffer;
use fe_config::EditorPalette;
use fe_editor::decorations::{Decoration, DecorationKind, Tint};
use gpui::TestAppContext;

use super::foreground_spans;
use super::styled_highlights;
use crate::{EditorSettings, EditorTheme};

#[gpui::test]
fn an_uncoloured_name_paints_the_foreground(cx: &mut TestAppContext) {
    let theme = cx.update(|cx| EditorTheme::default_dark(cx));
    let decoration =
        |name: &str| Decoration::new(0..5, DecorationKind::Foreground(name.into()), Tint::Accent);
    // `variable` is one name the theme leaves to the foreground.
    let spans = foreground_spans(&[decoration("variable")], &theme);
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].style, theme.foreground);
    // A name outside the vocabulary is still left out.
    assert!(foreground_spans(&[decoration("no.such.name")], &theme).is_empty());
}

/// A log line: `{}` and `{error}` placeholders, `error` inlined, a `\n`.
const LINE: &str = r#"log::warn!("a {} {error}\n", x);"#;

/// The string around format specifiers, as drawn: the specifier tokens lie
/// over the string's highlight, `error` (uncoloured) in the foreground.
#[gpui::test]
fn specifiers_in_a_string_draw_over_it_in_every_theme(cx: &mut TestAppContext) {
    let ember = {
        let theme = fe_config::themes::find("ember").expect("a bundled theme");
        let data = kiln_core::theme::ThemeData::from_file(&theme.file());
        EditorPalette::for_theme(&data).0
    };
    let palettes = [
        ("dark", EditorPalette::fastedit_dark().clone()),
        ("light", EditorPalette::fastedit_light().clone()),
        ("ember", ember),
    ];
    let buffer = Buffer::new(LINE);
    let mut syntax = fe_syntax::Syntax::new(
        fe_syntax::language_by_name("rust").unwrap(),
        buffer.current(),
    );
    assert!(syntax.parse_until(std::time::Duration::MAX, || false));
    let at = |needle: &str, nth: usize| LINE.match_indices(needle).nth(nth).unwrap().0;
    let token = |needle: &str, nth: usize, name: &str| {
        let start = at(needle, nth);
        Decoration::new(
            start..start + needle.len(),
            DecorationKind::Foreground(name.into()),
            Tint::Accent,
        )
    };
    let decorations = [
        token("{", 0, "string.placeholder"),
        token("}", 0, "string.placeholder"),
        token("{", 1, "string.placeholder"),
        token("error", 0, "variable"),
        token("}", 1, "string.placeholder"),
        token("\\n", 0, "escape"),
    ];
    for (name, palette) in palettes {
        let theme =
            cx.update(|cx| EditorTheme::from_palette(&palette, &EditorSettings::default(), cx));
        let colour = |capture: &str| {
            let highlight = fe_syntax::Highlight::from_capture(capture).unwrap();
            theme.syntax_color(highlight).unwrap_or(theme.foreground)
        };
        let (string, placeholder, escape) = (
            colour("string"),
            colour("string.placeholder"),
            colour("escape"),
        );
        assert_ne!(string, theme.foreground, "{name}: strings are coloured");
        assert_ne!(placeholder, string, "{name}: placeholders stand apart");
        assert_ne!(escape, string, "{name}: escapes stand apart");

        let foreground = foreground_spans(&decorations, &theme);
        let spans = styled_highlights(Some(&syntax), &theme, &[], &foreground, 0..LINE.len());
        let drawn = |offset: usize| {
            spans
                .iter()
                .find(|span| span.range.contains(&offset))
                .map(|span| span.style)
                .unwrap_or(theme.foreground)
        };
        let text = LINE.find("\"a ").unwrap() + 1;
        assert_eq!(drawn(text), string, "{name}: the string");
        assert_eq!(drawn(at(" ", 1)), string, "{name}: between tokens");
        for (needle, nth) in [("{", 0), ("}", 0), ("{", 1), ("}", 1)] {
            assert_eq!(
                drawn(at(needle, nth)),
                placeholder,
                "{name}: {needle} #{nth}"
            );
        }
        let error = at("error", 0);
        for offset in error..error + 5 {
            assert_eq!(drawn(offset), theme.foreground, "{name}: error at {offset}");
        }
        let escape_at = at("\\n", 0);
        for offset in escape_at..escape_at + 2 {
            assert_eq!(drawn(offset), escape, "{name}: escape at {offset}");
        }
    }
}
