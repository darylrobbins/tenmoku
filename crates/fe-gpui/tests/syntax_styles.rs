//! The editor theme resolves a capture to a colour and a font style.

use fe_config::{Color, EditorPalette, FontStyle};
use fe_gpui::{EditorSettings, EditorTheme};
use fe_syntax::Highlight;
use gpui::TestAppContext;

#[gpui::test]
fn the_editor_theme_resolves_a_capture_to_colour_and_style(cx: &mut TestAppContext) {
    let mut palette = EditorPalette::fastedit_dark().clone();
    palette.syntax = vec![("function".into(), "#112233".parse::<Color>().unwrap())];
    palette.styles = vec![
        (
            "comment".into(),
            FontStyle {
                italic: true,
                bold: false,
                strikethrough: false,
            },
        ),
        (
            "function".into(),
            FontStyle {
                italic: false,
                bold: true,
                strikethrough: false,
            },
        ),
    ];
    let theme = cx.update(|cx| EditorTheme::from_palette(&palette, &EditorSettings::default(), cx));

    let comment = Highlight::from_capture("comment.documentation").unwrap();
    let (color, style) = theme.capture_style(comment);
    assert_eq!(color, None);
    assert_eq!(
        style,
        Some(FontStyle {
            italic: true,
            bold: false,
            strikethrough: false,
        })
    );

    let method = Highlight::from_capture("function.method").unwrap();
    let (color, style) = theme.capture_style(method);
    assert!(color.is_some());
    assert_eq!(
        style,
        Some(FontStyle {
            italic: false,
            bold: true,
            strikethrough: false,
        })
    );

    assert_eq!(
        theme.capture_style(Highlight::from_capture("keyword").unwrap()),
        (None, None)
    );
}
