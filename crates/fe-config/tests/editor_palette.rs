//! The editor's colours: the bundled dark and light palettes, Yohen's and
//! Hakuji's, and a kiln theme's `[editor]` and `[syntax]` tables laid over
//! them.

use std::path::Path;

use fe_config::{Color, EditorColors, EditorPalette, theme::EXTENSIONS};
use fe_syntax::Highlight;
use kiln_core::theme::{ThemeData, ThemeFile};

/// The highlights the bundled themes don't colour: a dotted parent's colour
/// covers each, or the foreground does (`embedded`, `variable`).
const UNCOLOURED: &[&str] = &[
    "comment.documentation",
    "comment.documentation.italic",
    "comment.documentation.list",
    "comment.documentation.quote",
    "comment.documentation.strikethrough",
    "comment.documentation.strong",
    "constant.builtin",
    "embedded",
    "function.builtin",
    "function.definition",
    "function.method",
    "punctuation.bracket",
    "punctuation.delimiter",
    "type.builtin",
    "variable",
];

/// The TODO-comment badges' names.
const BADGES: [&str; 6] = [
    "comment.fixme",
    "comment.hack",
    "comment.note",
    "comment.perf",
    "comment.todo",
    "comment.warn",
];

fn color(text: &str) -> Color {
    text.parse().unwrap()
}

fn theme(source: &str) -> (ThemeData, usize) {
    let (file, diagnostics) = ThemeFile::parse(Path::new("theme.toml"), source, EXTENSIONS);
    (ThemeData::from_file(&file.unwrap()), diagnostics.len())
}

#[test]
fn bundled_palettes() {
    let dark = EditorPalette::fastedit_dark();
    let light = EditorPalette::fastedit_light();
    // Font styles are a theme's own, so a theme without the table has the
    // markup's alone, every theme's.
    let unstyled = |palette: &EditorPalette| EditorPalette {
        styles: (fe_config::theme::MARKUP_STYLES.iter())
            .map(|&(name, style)| (name.to_owned(), style))
            .collect(),
        ..palette.clone()
    };
    assert_eq!(
        EditorPalette::for_theme(ThemeData::dark()),
        (unstyled(dark), vec![])
    );
    assert_eq!(
        EditorPalette::for_theme(ThemeData::light()),
        (unstyled(light), vec![])
    );

    // Yohen's and Hakuji's: Tenmoku's default themes.
    assert_eq!(dark.editor.background, color("#1c1613"));
    assert_eq!(dark.editor.cursor, color("#e0874c"));
    assert_eq!(dark.editor.popover_background, color("#2a211c"));
    assert_eq!(dark.syntax_color("keyword"), Some(color("#c7a5e6")));
    assert_eq!(light.editor.background, color("#ffffff"));
    assert_eq!(light.editor.cursor, color("#a84a16"));
    assert_eq!(light.syntax_color("keyword"), Some(color("#7b36a8")));
    for (palette, source) in [
        (dark, include_str!("../themes/yohen.toml")),
        (light, include_str!("../themes/hakuji.toml")),
    ] {
        let (data, diagnostics) = theme(source);
        assert_eq!(diagnostics, 0);
        assert_eq!(EditorPalette::for_theme(&data), (palette.clone(), vec![]));
    }
    // Tenmoku Dark keeps its own colours over the new base.
    let (tenmoku_dark, _) = theme(include_str!("../themes/tenmoku-dark.toml"));
    let (tenmoku_dark, _) = EditorPalette::for_theme(&tenmoku_dark);
    assert_eq!(tenmoku_dark.editor.background, color("#16181d"));
    assert_eq!(tenmoku_dark.syntax_color("keyword"), Some(color("#c678dd")));

    let sources = [
        include_str!("../themes/yohen.toml"),
        include_str!("../themes/hakuji.toml"),
        include_str!("../themes/tenmoku-dark.toml"),
        include_str!("../themes/tenmoku-light.toml"),
        include_str!("../themes/tenmoku-ember.toml"),
    ];
    let mut captures = Vec::new();
    for source in sources {
        let (file, diagnostics) = ThemeFile::parse(Path::new("bundled.toml"), source, EXTENSIONS);
        assert_eq!(diagnostics, []);
        let file = file.unwrap();
        let editor: Vec<_> = file.extensions["editor"]
            .keys()
            .map(String::as_str)
            .collect();
        // Each colour key, once: a table's keys are unique.
        let mut keys = EditorColors::KEYS.to_vec();
        keys.sort();
        assert_eq!(editor, keys);
        captures.push(
            file.extensions["syntax"]
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
        );
    }
    // Each colours every highlight but those it leaves to a dotted parent's
    // colour, or to the foreground.
    let mut expected: Vec<_> = Highlight::all()
        .map(Highlight::name)
        .filter(|name| !UNCOLOURED.contains(name))
        .collect();
    expected.sort();
    for mut captures in captures {
        captures.sort();
        assert_eq!(captures, expected);
    }
    let names = |palette: &EditorPalette| -> Vec<String> {
        palette
            .syntax
            .iter()
            .map(|(name, _)| name.clone())
            .collect()
    };
    assert_eq!(names(dark), names(light));
}

#[test]
fn extension_tables_override() {
    let (theme, diagnostics) = theme(
        r##"
appearance = "light"

[editor]
background = "#fafafa"
cursor = 3

[syntax]
keyword = "#d00000"
"function.method" = "#123456"
"##,
    );
    assert_eq!(diagnostics, 0);
    let (palette, warnings) = EditorPalette::for_theme(&theme);
    assert_eq!(
        warnings,
        ["editor.cursor: expected a colour string, found integer"]
    );

    let light = EditorPalette::fastedit_light();
    let mut expected = light.clone();
    expected.styles = (fe_config::theme::MARKUP_STYLES.iter())
        .map(|&(name, style)| (name.to_owned(), style))
        .collect();
    expected.editor.background = color("#fafafa");
    let keyword = expected
        .syntax
        .iter_mut()
        .find(|(name, _)| name == "keyword")
        .unwrap();
    keyword.1 = color("#d00000");
    expected
        .syntax
        .push(("function.method".to_owned(), color("#123456")));
    // With an `[editor]` table of its own, the debugger's colours follow
    // its cursor line, error and gutter, and the TODO badges it doesn't
    // name take their kinds' tints (crates/fe-config/tests/debug_tints.rs).
    expected.editor.debug_current_line = light.editor.active_line;
    expected.editor.debug_frame_line = light.editor.active_line;
    expected.editor.debug_breakpoint = light.editor.error;
    expected.editor.debug_breakpoint_disabled = light.editor.gutter_foreground;
    expected.editor.debug_frame_arrow = light.editor.cursor;
    expected
        .syntax
        .retain(|(name, _)| !BADGES.contains(&name.as_str()));
    assert_eq!(palette, expected);
    assert_eq!(palette.editor.cursor, light.editor.cursor);
    assert_eq!(
        palette.syntax.last(),
        Some(&("function.method".to_owned(), color("#123456")))
    );
}
