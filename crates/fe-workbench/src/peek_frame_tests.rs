//! The peek's frame: a 1 px hairline in `ui.border`, never the accent.

use fe_config::themes::find;
use fe_ui::UiTheme;
use gpui::{TestAppContext, px};
use kiln::core::theme::ThemeData;
use kiln::ui::Theme;

use std::sync::{Arc, atomic::AtomicBool};

use fe_nav::{
    Definitions, FileReferences, Location, NavError, NavRequest, NavigationProvider, Precision,
    Readiness, Reference, ReferencesSummary, WorkspaceSymbol,
};

use super::peek_tests::{
    MAIN, Opened, caret_box, editor, excerpt, opened, peek, peek_definition, settle,
};
use gpui::VisualTestContext;

use super::{BORDER, HEADER_HEIGHT, ROWS};

fn ui_of(title: &str) -> UiTheme {
    let data = ThemeData::from_file(&find(title).expect("bundled").file());
    Theme::new(data).ui_theme()
}

#[test]
fn the_frame_constants_are_a_hairline() {
    assert_eq!(BORDER, px(1.));
}

/// The painted quad with exactly the bounds of `selector`'s element.
fn painted(selector: &'static str, cx: &mut VisualTestContext) -> gpui::Quad {
    let bounds = cx.debug_bounds(selector).expect("drawn");
    cx.update(|window, _| {
        let scale = window.scale_factor();
        let want = bounds.scale(scale);
        window
            .painted_quads()
            .into_iter()
            .find(|quad| {
                let edges = quad.border_widths;
                quad.bounds == want
                    && (edges.top != gpui::ScaledPixels::from(0.)
                        || edges.bottom != gpui::ScaledPixels::from(0.))
            })
            .unwrap_or_else(|| panic!("no bordered quad painted at {bounds:?}"))
    })
}

/// With the peek open: under each theme its top and bottom edges are
/// painted 1 px wide in `ui.border`, and the sides have no border.
fn assert_painted_hairline(cx: &mut VisualTestContext) {
    for title in ["Tenmoku Ember", "Monokai"] {
        let ui = ui_of(title);
        assert_ne!(ui.border, ui.accent, "{title}");
        cx.update(|_, cx| kiln::ui::set_ui_theme(cx, ui));
        settle(cx);
        let quad = painted("peek", cx);
        let scale = cx.update(|window, _| window.scale_factor());
        let one = gpui::ScaledPixels::from(scale);
        assert_eq!(quad.border_widths.top, one, "{title} top");
        assert_eq!(quad.border_widths.bottom, one, "{title} bottom");
        assert_eq!(
            quad.border_widths.left,
            gpui::ScaledPixels::from(0.),
            "{title}"
        );
        assert_eq!(
            quad.border_widths.right,
            gpui::ScaledPixels::from(0.),
            "{title}"
        );
        assert_eq!(quad.border_color, ui.border, "{title}");
    }
}

#[gpui::test]
fn peek_frame_is_a_one_pixel_hairline_in_the_border_colour(cx: &mut TestAppContext) {
    let Opened {
        _dir,
        workbench,
        cx,
        ..
    } = opened(cx);
    peek_definition(cx);
    assert!(peek(&workbench, cx).is_some());
    assert_painted_hairline(cx);
}

/// Answers find references with the one call of `helper` in `main.rs`.
struct Refs;

impl NavigationProvider for Refs {
    fn name(&self) -> &str {
        "refs"
    }

    fn precision(&self) -> Precision {
        Precision::Exact
    }

    fn readiness(&self, _: Option<&str>) -> Readiness {
        Readiness::Ready
    }

    fn definitions(&self, _: &NavRequest, _: &AtomicBool) -> Result<Definitions, NavError> {
        Err(NavError::Unsupported)
    }

    fn references(
        &self,
        _: &NavRequest,
        _: &AtomicBool,
        sink: &mut dyn FnMut(Vec<FileReferences>),
    ) -> Result<ReferencesSummary, NavError> {
        let start = MAIN.find("helper").unwrap();
        sink(vec![FileReferences {
            rel: "src/main.rs".into(),
            refs: vec![Reference {
                location: Location {
                    rel: "src/main.rs".into(),
                    range: start..start + 6,
                    row: 3,
                    snapshot: None,
                },
                line_number: 4,
                preview: "    util::helper();".into(),
                preview_ranges: std::iter::once(10..16).collect(),
                verified: true,
                is_definition: false,
            }],
        }]);
        Ok(ReferencesSummary {
            word: Some("helper".into()),
            ..ReferencesSummary::default()
        })
    }

    fn workspace_symbols(
        &self,
        _: &str,
        _: usize,
        _: &AtomicBool,
    ) -> Result<Vec<WorkspaceSymbol>, NavError> {
        Ok(Vec::new())
    }
}

#[gpui::test]
fn peek_references_frame_is_the_same_hairline(cx: &mut TestAppContext) {
    let Opened {
        _dir,
        workbench,
        cx,
        ..
    } = opened(cx);
    let navigation = workbench.read_with(cx, |workbench, _| workbench.navigation().clone());
    navigation.add(Arc::new(Refs), 20);
    cx.simulate_keystrokes("shift-f12");
    settle(cx);
    let peek = peek(&workbench, cx).expect("a references peek opened");
    assert!(peek.read_with(cx, |peek, _| peek.references().is_some()));
    assert_painted_hairline(cx);
}

#[gpui::test]
fn excerpt_height_follows_the_one_pixel_frame(cx: &mut TestAppContext) {
    let Opened {
        _dir,
        workbench,
        cx,
        ..
    } = opened(cx);
    let editor = editor(&workbench, cx);
    let row = caret_box(&editor, 0, cx).size.height;
    peek_definition(cx);
    let peek = peek(&workbench, cx).expect("a peek opened");
    let _ = excerpt(&peek, cx);
    let reserved = editor.read_with(cx, |editor, _| editor.reserved_blocks().to_vec());
    assert_eq!(reserved[0].bounds.size.height, row * ROWS as f32);
    let block = reserved[0].bounds;
    let frame = cx.debug_bounds("peek").expect("peek drawn");
    let header = cx.debug_bounds("peek-header").expect("header drawn");
    let body = cx.debug_bounds("peek-excerpt").expect("excerpt drawn");
    // The frame fills the reserved rows exactly.
    assert_eq!((frame.top(), frame.bottom()), (block.top(), block.bottom()));
    // Hairline, header, excerpt, hairline: no gap and no overflow.
    assert_eq!(header.top(), frame.top() + BORDER);
    assert_eq!(header.size.height, HEADER_HEIGHT);
    assert_eq!(body.top(), header.bottom());
    assert_eq!(body.bottom(), frame.bottom() - BORDER);
    assert_eq!(
        body.size.height,
        block.size.height - HEADER_HEIGHT - BORDER * 2.
    );
}
