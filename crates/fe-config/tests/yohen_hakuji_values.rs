//! Yohen and Hakuji carry the design's colours: every row of the Signals,
//! Syntax and Surfaces tables in docs/design/design-system.md, and its three
//! text steps, resolves to the design's hex in each theme, wherever the
//! theme uses that role.

use std::{fs, path::Path};

use fe_config::{Color, EditorPalette, themes};
use kiln_core::theme::ThemeData;

/// Where each of the design's roles shows in a theme: a kiln token
/// (`accent.base`), an editor key (`editor.cursor`) or a capture's colour
/// (`syntax.keyword`). Keyed by the first cell of the design's row.
const ROLES: &[(&str, &[&str])] = &[
    // Signals.
    (
        "Hare's fur (accent)",
        &["accent.base", "editor.cursor", "editor.vim_mode_normal"],
    ),
    ("Oil spot", &["agent.base", "editor.agent_lane"]),
    (
        "Celadon",
        &[
            "status.success",
            "vcs.added",
            "editor.change_added",
            "editor.vim_mode_insert",
        ],
    ),
    (
        "Shinsha",
        &[
            "status.error",
            "vcs.deleted",
            "editor.error",
            "editor.change_deleted",
            "editor.vim_mode_replace",
        ],
    ),
    (
        "Kiseto",
        &[
            "status.warning",
            "editor.warning",
            "editor.vim_mode_command",
        ],
    ),
    (
        "Ruri",
        &["status.modified", "vcs.modified", "editor.change_modified"],
    ),
    ("Jun", &["editor.vim_mode_visual"]),
    // Syntax: brackets by depth are function, keyword and type.
    ("Keyword", &["syntax.keyword", "editor.bracket_2"]),
    ("Function", &["syntax.function", "editor.bracket_1"]),
    ("Type", &["syntax.type", "editor.bracket_3"]),
    ("String", &["syntax.string"]),
    ("Number, constant", &["syntax.number", "syntax.constant"]),
    (
        "Macro, attribute",
        &["syntax.function.macro", "syntax.attribute"],
    ),
    ("Property", &["syntax.property"]),
    ("Data-file key", &["syntax.string.special.key"]),
    ("Variable", &["editor.foreground"]),
    ("Punctuation", &["syntax.punctuation"]),
    ("Comment", &["syntax.comment"]),
    // Surfaces.
    ("Chrome", &["surface.chrome"]),
    ("Tool window", &["surface.panel"]),
    ("Editor", &["surface.background", "editor.background"]),
    ("Cursor line", &["editor.active_line"]),
    ("Hover", &["state.hover", "list.row_hover"]),
    (
        "Selected",
        &["state.selected_inactive", "list.row_selected_inactive"],
    ),
    (
        "Focused selection",
        &["state.selected", "list.row_selected"],
    ),
    ("Overlay", &["surface.raised", "editor.popover_background"]),
    ("Line", &["border.default"]),
    ("Control border", &["border.control"]),
    // The text steps.
    ("ink", &["text.primary", "editor.foreground"]),
    ("muted", &["text.muted"]),
    ("faint", &["text.faint"]),
];

/// The tables whose rows are roles.
const TABLES: [&str; 3] = ["### Signals", "### Syntax", "### Surfaces"];

/// The design's roles, each with its Yohen and Hakuji colours.
fn design() -> Vec<(String, Color, Color)> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/design/design-system.md");
    let doc = fs::read_to_string(&path).unwrap();
    let hex = |cell: &str| -> Color {
        let start = cell
            .find("`#")
            .unwrap_or_else(|| panic!("no colour in {cell:?}"));
        let code = &cell[start + 1..];
        code[..code.find('`').unwrap()].parse().unwrap()
    };
    let mut roles = Vec::new();
    for table in TABLES {
        let rows = (doc.lines())
            .skip_while(|line| *line != table)
            .skip(1)
            .skip_while(|line| !line.starts_with('|'))
            .take_while(|line| line.starts_with('|'))
            // The header and its rule.
            .skip(2);
        let before = roles.len();
        for row in rows {
            let cells: Vec<&str> = row.split('|').map(str::trim).collect();
            roles.push((cells[1].to_owned(), hex(cells[2]), hex(cells[3])));
        }
        assert!(roles.len() > before, "{table} has rows");
    }
    // "Text is three steps on every surface: ink `#…` / `#…`, muted …"
    let text = (doc.lines())
        .skip_while(|line| !line.starts_with("Text is three steps"))
        .take_while(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    for step in ["ink", "muted", "faint"] {
        let at = text
            .find(&format!("{step} `#"))
            .unwrap_or_else(|| panic!("{step}"));
        let (yohen, hakuji) = text[at..].split_once(" / ").unwrap();
        roles.push((step.to_owned(), hex(yohen), hex(hakuji)));
    }
    roles
}

/// The colour at `place` in `theme`: see [`ROLES`].
fn resolve(theme: &ThemeData, palette: &EditorPalette, place: &str) -> Option<Color> {
    if let Some(key) = place.strip_prefix("editor.") {
        palette.editor.get(key)
    } else if let Some(capture) = place.strip_prefix("syntax.") {
        palette.syntax_color(capture)
    } else {
        theme.tokens.get(place).copied()
    }
}

#[test]
fn every_design_role_has_the_design_value() {
    let design = design();
    let mut named: Vec<&str> = design.iter().map(|(role, ..)| role.as_str()).collect();
    let mut mapped: Vec<&str> = ROLES.iter().map(|&(role, _)| role).collect();
    named.sort_unstable();
    mapped.sort_unstable();
    assert_eq!(named, mapped, "every role of the design, and no other");
    assert_eq!(design.len(), 7 + 11 + 10 + 3);

    for (id, column) in [("yohen", 1), ("hakuji", 2)] {
        let bundled = themes::find(id).unwrap();
        let theme = ThemeData::from_file(&bundled.file());
        let (palette, warnings) = EditorPalette::for_theme(&theme);
        assert_eq!(warnings, Vec::<String>::new(), "{id}");
        for (role, yohen, hakuji) in &design {
            let want = if column == 1 { *yohen } else { *hakuji };
            let places = ROLES.iter().find(|(name, _)| name == role).unwrap().1;
            for place in places {
                assert_eq!(
                    resolve(&theme, &palette, place),
                    Some(want),
                    "{id}: {role} at {place}"
                );
            }
        }
    }
}

/// The design's other named colours: celadon-soft, the indent guides and
/// Yohen's find match.
#[test]
fn the_design_s_named_editor_colours() {
    let expect = [
        ("yohen", "status.success_soft", "#18291f"),
        ("hakuji", "status.success_soft", "#e3f3e8"),
        ("yohen", "editor.diff_added_row", "#18291f"),
        ("hakuji", "editor.diff_added_row", "#e3f3e8"),
        ("yohen", "editor.indent_guide", "#2c241e"),
        ("hakuji", "editor.indent_guide", "#eceef1"),
        ("yohen", "editor.find_match", "#5a4314"),
    ];
    for (id, place, hex) in expect {
        let theme = ThemeData::from_file(&themes::find(id).unwrap().file());
        let (palette, _) = EditorPalette::for_theme(&theme);
        let want: Color = hex.parse().unwrap();
        assert_eq!(
            resolve(&theme, &palette, place),
            Some(want),
            "{id}: {place}"
        );
    }
}
