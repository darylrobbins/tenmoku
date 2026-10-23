//! The outline's rows: each symbol's detail, its signature or type, dimmed
//! after its name.

use std::fs;

use fe_ui::{ActiveTheme as _, TreeRow};
use gpui::{
    Context, Focusable as _, IntoElement, ParentElement as _, Render, Styled as _, TestAppContext,
    VisualTestContext, Window, div, hsla, px,
};

use super::open_root;

const RUST: &str = "\
struct Point;

impl Point {
    fn new(x: f32) -> Self {
        Point
    }
}

const MAX: usize = 3;
";

#[gpui::test]
fn rows_carry_their_items_detail_in_the_muted_colour(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.rs");
    fs::write(&path, RUST).unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    cx.simulate_keystrokes("cmd-shift-o");
    let outline = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).outline().clone()
    });
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    cx.run_until_parked();

    let muted = cx.update(|_, cx| cx.theme().colors.text.muted);
    let built: Vec<(String, Option<String>)> = outline.update(cx, |outline, cx| {
        let items: Vec<_> = (outline.outline().unwrap().items.iter())
            .map(|item| (item.name.clone(), item.detail.clone()))
            .collect();
        let rows = outline.render_rows(0..items.len(), cx);
        assert_eq!(rows.len(), items.len());
        (items.into_iter().zip(rows))
            .map(|((name, detail), row)| {
                let shown = row.detail_text().map(|(text, color)| {
                    assert_eq!(color, muted, "{name}");
                    assert_eq!(text.as_ref(), detail, "{name}");
                    text.to_string()
                });
                (name, shown)
            })
            .collect()
    });
    assert_eq!(
        built,
        [
            ("Point", None),
            ("impl Point", None),
            ("new", Some("(x: f32) -> Self")),
            ("MAX", Some(": usize")),
        ]
        .map(|(name, detail)| (name.to_string(), detail.map(str::to_string)))
    );

    // Drawn right after the name; an empty detail draws nothing.
    cx.update(|window, cx| window.draw(cx).clear(cx));
    for name in ["new", "MAX"] {
        let label = cx.debug_bounds(format!("tree-label:{name}").leak());
        let detail = cx.debug_bounds(format!("tree-detail:{name}").leak());
        let (label, detail) = (label.expect("label drawn"), detail.expect("detail drawn"));
        assert_eq!(label.right() + px(6.), detail.left(), "{name}");
    }
    assert!(cx.debug_bounds("tree-detail:Point").is_none());
}

const WIDTH: f32 = 200.;
const NAME: &str = "name";
const LONG_NAME: &str = "a_name_far_too_long_to_fit_in_the_row_on_its_own";
const DETAIL: &str = "(first: u32, second: u32, third: u32) -> Result<(), Error>";

/// Rows `WIDTH` wide: a short name with a long detail and a badge, the same
/// without a badge, a name too long for the row with a detail, and a short
/// name with a short detail.
struct Rows;

impl Render for Rows {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = fe_ui::ui_theme(cx);
        let (dim, badge) = (hsla(0., 0., 0.5, 1.), hsla(0.3, 0.7, 0.5, 1.));
        div()
            .w(px(WIDTH))
            .text_size(px(12.))
            .child(
                TreeRow::new("badged", NAME, &theme)
                    .detail(DETAIL, dim)
                    .badge("M", badge),
            )
            .child(TreeRow::new("plain", "other", &theme).detail(DETAIL, dim))
            .child(TreeRow::new("long", LONG_NAME, &theme).detail(DETAIL, dim))
            .child(TreeRow::new("short", "s", &theme).detail("()", dim))
    }
}

/// The width `text` takes at the rows' font size.
fn text_width(text: &str, cx: &mut VisualTestContext) -> gpui::Pixels {
    cx.update(|window, _| {
        let style = window.text_style();
        let run = gpui::TextRun {
            len: text.len(),
            font: style.font(),
            color: style.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line = window.text_system().shape_line(
            text.to_string().into(),
            px(12.),
            std::slice::from_ref(&run),
            None,
        );
        line.width()
    })
}

#[gpui::test]
fn a_detail_follows_the_whole_label_and_ellipsizes_before_the_badge(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| Rows);
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let bounds = |selector: String, cx: &mut VisualTestContext| {
        cx.debug_bounds(selector.leak()).expect("drawn")
    };
    // The row's right padding is 8 px.
    let right_end = px(WIDTH - 8.);

    // The label keeps its whole text; the detail starts after it and gives
    // way before the badge.
    let label = bounds(format!("tree-label:{NAME}"), cx);
    let detail = bounds(format!("tree-detail:{NAME}"), cx);
    let badge = bounds(format!("tree-badge:{NAME}"), cx);
    assert!(text_width(NAME, cx) > px(0.), "the test font has widths");
    assert!(label.size.width >= text_width(NAME, cx));
    assert_eq!(label.right() + px(6.), detail.left());
    assert_eq!(detail.right() + px(6.), badge.left());
    assert_eq!(badge.right(), right_end);
    assert!(detail.size.width < text_width(DETAIL, cx), "ellipsized");

    // Without a badge it runs to the row's right end.
    let detail = bounds("tree-detail:other".into(), cx);
    assert_eq!(detail.right(), right_end);

    // A name too long for the row ellipsizes, leaving the detail no room.
    let label = bounds(format!("tree-label:{LONG_NAME}"), cx);
    let detail = bounds(format!("tree-detail:{LONG_NAME}"), cx);
    assert!(label.size.width < text_width(LONG_NAME, cx));
    assert_eq!(label.right() + px(6.), detail.left());
    assert_eq!(detail.size.width, px(0.));

    // A short detail takes its text's width after the label's.
    let label = bounds("tree-label:s".into(), cx);
    let detail = bounds("tree-detail:s".into(), cx);
    assert!(label.size.width >= text_width("s", cx));
    assert!(label.size.width < text_width("s", cx) + px(1.));
    assert_eq!(label.right() + px(6.), detail.left());
}
