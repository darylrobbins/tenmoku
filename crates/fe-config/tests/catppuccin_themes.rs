//! Catppuccin's four flavours, bundled themes none of which is the default:
//! Latte (light), Frappe, Macchiato and Mocha (dark).

use std::path::Path;

use fe_config::{EditorPalette, Settings, theme::EXTENSIONS, themes::find};
use fe_syntax::Highlight;
use kiln_core::theme::{Appearance, Color, ThemeData, ThemeFile};

/// Each flavour's title, ID and appearance.
const FLAVOURS: [(&str, &str, Appearance); 4] = [
    ("Catppuccin Latte", "catppuccin-latte", Appearance::Light),
    ("Catppuccin Frappe", "catppuccin-frappe", Appearance::Dark),
    (
        "Catppuccin Macchiato",
        "catppuccin-macchiato",
        Appearance::Dark,
    ),
    ("Catppuccin Mocha", "catppuccin-mocha", Appearance::Dark),
];

fn parse(id: &str) -> ThemeFile {
    let theme = find(id).unwrap_or_else(|| panic!("{id} is bundled"));
    let path = format!("{id}.toml");
    let (file, diagnostics) = ThemeFile::parse(Path::new(&path), theme.source, EXTENSIONS);
    assert_eq!(diagnostics, [], "{id}");
    file.unwrap()
}

fn palette(id: &str) -> EditorPalette {
    let (palette, warnings) = EditorPalette::for_theme(&ThemeData::from_file(&parse(id)));
    assert_eq!(warnings, Vec::<String>::new(), "{id}");
    palette
}

#[test]
fn each_flavour_loads() {
    assert_eq!(Settings::default().theme.name, None);
    for (title, id, appearance) in FLAVOURS {
        let by_title = find(title).unwrap_or_else(|| panic!("{title:?} is found"));
        assert_eq!(by_title.id, id);
        assert_eq!(by_title.appearance, appearance, "{id}");
        assert_eq!(find(id), Some(by_title));
        let file = parse(id);
        assert_eq!(file.name.as_deref(), Some(title));
        assert_eq!(file.appearance, Some(appearance), "{id}");
        palette(id);
    }
}

/// catppuccin/vscode's built `themes/<flavour>.json` at commit befc9e6
/// (Catppuccin.catppuccin-vsc 3.19.0): editor.background, .foreground,
/// .selectionBackground and editorCursor.foreground, then the token colours
/// of comment, keyword, entity.name.function, string, entity.name.type and
/// constant.numeric.
#[test]
fn matches_upstream() {
    let upstream: [(&str, [u32; 4], [u32; 6]); 4] = [
        (
            "catppuccin-latte",
            [0xEFF1F5FF, 0x4C4F69FF, 0x7C7F934D, 0xDC8A78FF],
            [0x7C7F93, 0x8839EF, 0x1E66F5, 0x40A02B, 0xDF8E1D, 0xFE640B],
        ),
        (
            "catppuccin-frappe",
            [0x303446FF, 0xC6D0F5FF, 0x949CBB40, 0xF2D5CFFF],
            [0x949CBB, 0xCA9EE6, 0x8CAAEE, 0xA6D189, 0xE5C890, 0xEF9F76],
        ),
        (
            "catppuccin-macchiato",
            [0x24273AFF, 0xCAD3F5FF, 0x939AB740, 0xF4DBD6FF],
            [0x939AB7, 0xC6A0F6, 0x8AADF4, 0xA6DA95, 0xEED49F, 0xF5A97F],
        ),
        (
            "catppuccin-mocha",
            [0x1E1E2EFF, 0xCDD6F4FF, 0x9399B240, 0xF5E0DCFF],
            [0x9399B2, 0xCBA6F7, 0x89B4FA, 0xA6E3A1, 0xF9E2AF, 0xFAB387],
        ),
    ];
    for (id, editor, syntax) in upstream {
        let data = ThemeData::from_file(&parse(id));
        let (palette, _) = EditorPalette::for_theme(&data);
        let colors = &palette.editor;
        assert_eq!(
            [
                colors.background,
                colors.foreground,
                colors.selection,
                colors.cursor
            ],
            editor.map(Color::rgba),
            "{id}"
        );
        assert_eq!(data.tokens.surface.background, colors.background, "{id}");
        let names = ["comment", "keyword", "function", "string", "type", "number"];
        for (name, rgb) in names.into_iter().zip(syntax) {
            assert_eq!(
                palette.syntax_color(name),
                Some(Color::rgb(rgb)),
                "{id}: {name}"
            );
        }
    }
    assert_eq!(
        palette("catppuccin-mocha").editor.background,
        Color::rgb(0x1E1E2E)
    );
    assert_eq!(
        palette("catppuccin-latte").editor.background,
        Color::rgb(0xEFF1F5)
    );
}

#[test]
fn syntax_keys_are_known() {
    for (_, id, _) in FLAVOURS {
        let syntax = &parse(id).extensions["syntax"];
        for key in syntax.keys() {
            assert!(Highlight::from_capture(key).is_some(), "{id}: {key}");
        }
        for key in [
            "variable.parameter",
            "type.parameter",
            "constant.enum_member",
        ] {
            assert!(syntax.contains_key(key), "{id} sets {key}");
        }
    }
}

/// WCAG relative luminance.
fn luminance(color: Color) -> f64 {
    let [r, g, b] = [color.r(), color.g(), color.b()].map(|c| {
        let c = f64::from(c) / 255.;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    });
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// WCAG contrast ratio of `a` against `b`.
fn contrast(a: Color, b: Color) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[test]
fn readable() {
    for (_, id, _) in FLAVOURS {
        let editor = palette(id).editor;
        let fills = [
            (
                "jump_label",
                editor.jump_label,
                editor.jump_label_foreground,
            ),
            (
                "jump_label_preview",
                editor.jump_label_preview,
                editor.jump_label_foreground,
            ),
            (
                "vim_mode_normal",
                editor.vim_mode_normal,
                editor.vim_mode_foreground,
            ),
            (
                "vim_mode_insert",
                editor.vim_mode_insert,
                editor.vim_mode_foreground,
            ),
            (
                "vim_mode_visual",
                editor.vim_mode_visual,
                editor.vim_mode_foreground,
            ),
            (
                "vim_mode_replace",
                editor.vim_mode_replace,
                editor.vim_mode_foreground,
            ),
            (
                "vim_mode_command",
                editor.vim_mode_command,
                editor.vim_mode_foreground,
            ),
        ];
        for (key, fill, label) in fills {
            assert_eq!(fill.a(), 0xff, "{id}: {key} {fill} isn't opaque");
            assert_eq!(label.a(), 0xff, "{id}: {key}'s label {label} isn't opaque");
            let ratio = contrast(label, fill);
            assert!(ratio >= 3.0, "{id}: label on {key} {ratio:.2}");
        }
        let modes = &fills[2..];
        for (i, (key, fill, _)) in modes.iter().enumerate() {
            for (other, other_fill, _) in &modes[i + 1..] {
                assert_ne!(fill, other_fill, "{id}: {key} is {other}");
            }
        }

        let brackets = [editor.bracket_1, editor.bracket_2, editor.bracket_3];
        for (i, a) in brackets.iter().enumerate() {
            for b in &brackets[i + 1..] {
                assert_ne!(a, b, "{id}: {brackets:?}");
            }
        }
    }
}
