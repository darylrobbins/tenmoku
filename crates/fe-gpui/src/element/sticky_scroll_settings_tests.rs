//! Sticky scroll's fallbacks and settings in a headless window: plain text
//! and a file over its parse limit pin indented blocks, a language with a
//! tree but no outline pins its fold ranges, `default_model` picks where
//! the scopes start from, `max_line_count` caps the band, and
//! `scroll_with_editor` decides whether the band's text scrolls sideways,
//! each applied live and kept through a recolouring.

#![allow(clippy::single_range_in_vec_init)]

use fe_config::{
    EditorPalette, EditorSettings, Settings, StickyScrollModel, StickyScrollSettings,
    USER_SETTINGS_TEMPLATE, settings_schema,
};
use gpui::{Entity, Pixels, VisualTestContext, point, px};

use super::{
    Model, asked, asked_rows,
    tests::{band, draw, editor, line_height, pinned, scroll_to_row, unscrolled},
};
use crate::EditorView;

/// Rust with a 43-row function in an impl (rows 4 to 50), then 40 rows of
/// constants, as the band's other tests have it.
fn source() -> String {
    let mut text = String::from(
        "struct Point {\n    x: i32,\n}\n\nimpl Point {\n    fn new(x: i32) -> Self {\n",
    );
    for n in 0..40 {
        text.push_str(&format!("        let value_{n} = \"{n}\";\n"));
    }
    text.push_str("        Self { x }\n    }\n\n    fn other(&self) {}\n}\n");
    for n in 0..40 {
        text.push_str(&format!("const C_{n}: u32 = {n};\n"));
    }
    text
}

/// The rows pinned, without their tops.
fn rows(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<u32> {
    pinned(view, cx).into_iter().map(|(row, _)| row).collect()
}

/// Applies `sticky` as the editor's sticky scroll settings, and draws.
fn apply(view: &Entity<EditorView>, sticky: StickyScrollSettings, cx: &mut VisualTestContext) {
    let settings = EditorSettings {
        sticky_scroll: sticky,
        ..EditorSettings::default()
    };
    view.update(cx, |view, cx| view.apply_settings(&settings, cx));
    draw(cx);
}

/// Recolours the editor, and draws.
fn recolor(view: &Entity<EditorView>, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        view.set_colors(EditorPalette::fastedit_light(), cx)
    });
    draw(cx);
}

#[gpui::test]
fn plain_text_pins_its_indented_blocks(cx: &mut gpui::TestAppContext) {
    let (view, cx) = editor(&source(), "notes.txt", cx);
    scroll_to_row(&view, 10, cx);
    assert_eq!(asked(), Some(Model::Indentation(4)));
    // impl Point's block holds rows 5 to 49, and new's 6 to 46.
    let line_height = px(line_height(&view, cx));
    assert_eq!(pinned(&view, cx), [(4, px(0.)), (5, line_height)]);
    let band = band(&view, cx).unwrap();
    assert_eq!(band.rows[1].text, "    fn new(x: i32) -> Self {");
}

#[gpui::test]
fn a_file_over_its_parse_limit_pins_its_indented_blocks(cx: &mut gpui::TestAppContext) {
    let limit = fe_syntax::language_by_name("rust").unwrap().parse_limit();
    let mut text = String::from("mod m {\n    fn f() {\n");
    let row = "        g();\n";
    text.push_str(&row.repeat(limit / row.len() + 1));
    text.push_str("    }\n}\n");
    let (view, cx) = editor(&text, "main.rs", cx);
    let tree = view.read_with(cx, |view, _| {
        let syntax = view.syntax.as_ref().expect("Rust").syntax();
        syntax.tree().is_some()
    });
    assert!(!tree, "no tree over the parse limit");
    scroll_to_row(&view, 30_000, cx);
    assert_eq!(asked(), Some(Model::Indentation(4)));
    assert_eq!(rows(&view, cx), [0, 1]);
}

#[gpui::test]
fn a_language_with_a_tree_but_no_outline_pins_its_fold_ranges(cx: &mut gpui::TestAppContext) {
    assert!(!fe_syntax::outline_supported("xml"));
    let mut text = String::from("<root>\n  <list>\n    <item>\n");
    for n in 0..40 {
        text.push_str(&format!("      <value n=\"{n}\"/>\n"));
    }
    text.push_str("    </item>\n  </list>\n</root>\n");
    let (view, cx) = editor(&text, "page.xml", cx);
    scroll_to_row(&view, 10, cx);
    assert_eq!(asked(), Some(Model::Folding));
    assert_eq!(rows(&view, cx), [0, 1, 2]);
    // item's range ends on row 42, the row before its `</item>`, as the fold
    // gutter folds it: scrolled to row 41, that row is under the band's
    // third slot, so item's header is gone.
    scroll_to_row(&view, 40, cx);
    assert_eq!(rows(&view, cx), [0, 1, 2]);
    scroll_to_row(&view, 41, cx);
    assert_eq!(rows(&view, cx), [0, 1]);
}

#[gpui::test]
fn default_model_picks_where_the_scopes_start(cx: &mut gpui::TestAppContext) {
    let (view, cx) = editor(&source(), "main.rs", cx);
    scroll_to_row(&view, 10, cx);
    assert_eq!(asked(), Some(Model::Outline));
    assert_eq!(rows(&view, cx), [4, 5]);
    for (model, expected) in [
        (StickyScrollModel::Indentation, Model::Indentation(4)),
        (StickyScrollModel::Folding, Model::Folding),
        (StickyScrollModel::Outline, Model::Outline),
    ] {
        let sticky = StickyScrollSettings {
            default_model: model,
            ..StickyScrollSettings::default()
        };
        apply(&view, sticky, cx);
        assert_eq!(asked(), Some(expected), "{model:?}");
        assert_eq!(rows(&view, cx), [4, 5], "{model:?}");
        recolor(&view, cx);
        let kept = view.read_with(cx, |view, _| view.theme.sticky_scroll.default_model);
        assert_eq!(kept, model, "recoloured");
    }
    // The setting's three values, in the template and the schema.
    let parse = |value: &str| {
        let json = serde_json::json!({ "default_model": value });
        serde_json::from_value::<StickyScrollSettings>(json).map(|sticky| sticky.default_model)
    };
    assert_eq!(parse("outline").unwrap(), StickyScrollModel::Outline);
    assert_eq!(parse("folding").unwrap(), StickyScrollModel::Folding);
    assert_eq!(
        parse("indentation").unwrap(),
        StickyScrollModel::Indentation
    );
    assert!(parse("outlineModel").is_err());
    let schema = settings_schema();
    let values: Vec<_> = (schema["$defs"]["StickyScrollModel"]["oneOf"].as_array())
        .expect("one of")
        .iter()
        .map(|value| value["const"].as_str().unwrap())
        .collect();
    assert_eq!(values, ["outline", "folding", "indentation"]);
    let property = &schema["$defs"]["StickyScrollSettings"]["properties"]["default_model"];
    assert_eq!(property["default"], "outline");
    let table = template_table();
    assert!(
        table.contains("\n# default_model = \"outline\"\n"),
        "{table}"
    );
    for value in ["\"outline\"", "\"folding\"", "\"indentation\""] {
        assert!(table.contains(value), "{value}: {table}");
    }
}

/// The template's `[editor.sticky_scroll]` table.
fn template_table() -> &'static str {
    let (_, table) = USER_SETTINGS_TEMPLATE
        .split_once("[editor.sticky_scroll]\n")
        .unwrap();
    table.split("\n[").next().unwrap()
}

/// Rust nested `depth` deep, a header a row, the innermost holding 60 rows.
fn nested(depth: usize) -> String {
    let mut text = String::new();
    for level in 0..depth {
        text.push_str(&format!("{}mod m{level} {{\n", "    ".repeat(level)));
    }
    for n in 0..60 {
        text.push_str(&format!("{}const C{n}: u32 = {n};\n", "    ".repeat(depth)));
    }
    for level in (0..depth).rev() {
        text.push_str(&format!("{}}}\n", "    ".repeat(level)));
    }
    text
}

#[gpui::test]
fn max_line_count_caps_the_band_live(cx: &mut gpui::TestAppContext) {
    assert_eq!(Settings::default().editor.sticky_scroll.max_line_count, 5);
    let schema = settings_schema();
    let property = &schema["$defs"]["StickyScrollSettings"]["properties"]["max_line_count"];
    assert_eq!(
        (
            &property["default"],
            &property["minimum"],
            &property["maximum"]
        ),
        (&5.into(), &1.into(), &20.into())
    );
    assert!(template_table().contains("\n# max_line_count = 5\n"));

    let (view, cx) = editor(&nested(8), "main.rs", cx);
    scroll_to_row(&view, 30, cx);
    assert_eq!(rows(&view, cx), [0, 1, 2, 3, 4]);
    for (count, expected) in [
        (2, vec![0, 1]),
        (1, vec![0]),
        (8, (0..8).collect()),
        (20, (0..8).collect()),
        // Out of bounds, as the schema rejects: 1 or 20.
        (0, vec![0]),
        (40, (0..8).collect()),
    ] {
        let sticky = StickyScrollSettings {
            max_line_count: count,
            ..StickyScrollSettings::default()
        };
        apply(&view, sticky, cx);
        assert_eq!(rows(&view, cx), expected, "{count}");
    }
    let sticky = StickyScrollSettings {
        max_line_count: 2,
        ..StickyScrollSettings::default()
    };
    apply(&view, sticky, cx);
    recolor(&view, cx);
    assert_eq!(rows(&view, cx), [0, 1], "recoloured");
}

/// Where the band's first row's text and line number are painted.
fn first_row(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> (Pixels, Pixels) {
    let band = band(view, cx).expect("pinned");
    let row = &band.rows[0];
    (row.text_origin.x, row.number.unwrap().0.origin.x)
}

/// Scrolls `x` pixels right with row 10 at the top, and draws.
fn scroll_right(view: &Entity<EditorView>, x: f32, cx: &mut VisualTestContext) {
    let line_height = line_height(view, cx);
    view.update(cx, |view, cx| {
        view.scroll = point(px(x), px(10. * line_height));
        view.autoscroll = false;
        cx.notify();
    });
    draw(cx);
}

#[gpui::test]
fn scroll_with_editor_scrolls_the_bands_text_but_never_its_numbers(cx: &mut gpui::TestAppContext) {
    let defaults = Settings::default().editor.sticky_scroll;
    assert!(defaults.scroll_with_editor);
    assert!(template_table().contains("\n# scroll_with_editor = true\n"));
    let schema = settings_schema();
    let property = &schema["$defs"]["StickyScrollSettings"]["properties"]["scroll_with_editor"];
    assert_eq!(
        (&property["type"], &property["default"]),
        (&"boolean".into(), &true.into())
    );

    // The impl's header, and the rows in view, wide enough to scroll
    // across.
    let wide = format!("; // {}\n", "-".repeat(400));
    let long = format!("impl Point {{ // {}", "x".repeat(400));
    let source =
        (source().replacen("impl Point {", &long, 1)).replace("\";\n", &format!("\"{wide}"));
    let (view, cx) = editor(&source, "main.rs", cx);
    let (origin, _, _) = unscrolled(&view, 4, cx);
    scroll_right(&view, 0., cx);
    let (text, number) = first_row(&view, cx);
    assert_eq!(text, origin.x);

    // Scrolled 120 px right, the text by as much as the editor's.
    scroll_right(&view, 120., cx);
    let scrolled = view.read_with(cx, |view, _| view.scroll.x);
    assert_eq!(scrolled, px(120.), "scrolled");
    assert_eq!(first_row(&view, cx), (origin.x - px(120.), number));

    // Off, it stays at the first column, through a recolouring too.
    let sticky = StickyScrollSettings {
        scroll_with_editor: false,
        ..StickyScrollSettings::default()
    };
    apply(&view, sticky, cx);
    scroll_right(&view, 120., cx);
    assert_eq!(first_row(&view, cx), (origin.x, number));
    recolor(&view, cx);
    assert_eq!(first_row(&view, cx), (origin.x, number));

    apply(&view, StickyScrollSettings::default(), cx);
    scroll_right(&view, 120., cx);
    assert_eq!(first_row(&view, cx), (origin.x - px(120.), number));
}

/// Folds buffer rows `rows` behind their first, and draws.
fn fold(
    view: &Entity<EditorView>,
    rows: std::ops::RangeInclusive<u32>,
    cx: &mut VisualTestContext,
) {
    view.update(cx, |view, cx| {
        view.model.fold_rows(rows);
        cx.notify();
    });
    draw(cx);
}

#[gpui::test]
fn a_closed_fold_in_the_band_rows_isnt_read_for_indentation(cx: &mut gpui::TestAppContext) {
    // outer holds a, whose million rows are folded, then b's 60.
    let hidden = 1_000_000;
    let mut text = String::from("outer:\n    a:\n");
    text.push_str(&"        x\n".repeat(hidden));
    text.push_str("    b:\n");
    text.push_str(&"        y\n".repeat(60));
    let (view, cx) = editor(&text, "notes.txt", cx);
    let b = hidden as u32 + 2;
    fold(&view, 1..=b - 1, cx);
    // a's folded row at the top: outer is pinned, and the model is asked
    // for the rows shown, either side of the fold, not the million under it.
    scroll_to_row(&view, 1, cx);
    assert_eq!(asked(), Some(Model::Indentation(4)));
    assert_eq!(asked_rows(), [1..2, b..b + 6]);
    assert_eq!(rows(&view, cx), [0]);
    // Scrolled a pixel, outer is found open past the fold too: both headers
    // pin, outer at the top and b a line below.
    let line_height = line_height(&view, cx);
    super::tests::scroll_to(&view, line_height + 1., cx);
    assert_eq!(asked_rows(), [1..2, b..b + 6]);
    let band = pinned(&view, cx);
    assert_eq!(band.iter().map(|(row, _)| *row).collect::<Vec<_>>(), [0, b]);
    assert_eq!(band[0].1, px(0.));
    assert_eq!(band[1].1, px(line_height));
}

#[gpui::test]
fn a_closed_fold_in_the_band_rows_isnt_read_for_folding(cx: &mut gpui::TestAppContext) {
    // root holds big, whose 100,000 rows are folded, then list's 60.
    let hidden = 100_000;
    let mut text = String::from("<root>\n  <big>\n");
    text.push_str(&"    <v/>\n".repeat(hidden));
    text.push_str("  </big>\n  <list>\n");
    text.push_str(&"    <v/>\n".repeat(60));
    text.push_str("  </list>\n</root>\n");
    let (view, cx) = editor(&text, "page.xml", cx);
    let end = hidden as u32 + 2;
    fold(&view, 1..=end - 1, cx);
    scroll_to_row(&view, 1, cx);
    assert_eq!(asked(), Some(Model::Folding));
    assert_eq!(asked_rows(), [1..2, end..end + 6]);
    assert_eq!(rows(&view, cx), [0]);
    // Scrolled into list, past the fold: root and list.
    scroll_to_row(&view, 4, cx);
    assert_eq!(asked_rows(), [end + 2..end + 9]);
    assert_eq!(rows(&view, cx), [0, end + 1]);
}
