//! Nerd Font icons painted by a terminal view: a private-use glyph wider than
//! its cell is fitted to one cell, or two when the next is blank.

use std::borrow::Cow;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use fe_terminal_view::{PaintReport, PaintedText, TerminalOptions, TerminalStyle, TerminalView};
use gpui::{
    Bounds, DevicePixels, Entity, Focusable, Font, FontId, FontMetrics, FontRun, GlyphId, Hsla,
    LineLayout, NoopTextSystem, Pixels, PlatformTextSystem, RenderGlyphParams, ShapedGlyph, Size,
    TestAppContext, TestDispatcher, TextRenderingMode, VisualTestContext, point, px, rgb, size,
};

fn style() -> TerminalStyle {
    let background: Hsla = rgb(0x101010).into();
    TerminalStyle {
        font_family: "Menlo".into(),
        font_size: px(14.),
        line_height: TerminalStyle::LINE_HEIGHT,
        background,
        foreground: rgb(0xd0d0d0).into(),
        cursor: rgb(0xffcc00).into(),
        selection: rgb(0x334466).into(),
        ansi: TerminalStyle::ansi_for(background),
    }
}

/// Draws until `done` holds. Panics after 5 s, naming `what`.
fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        if done(cx) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(10));
    }
}

/// A view whose shell prints `output` (shell `printf` syntax), then waits.
/// Returns once the grid shows `ready`.
fn painting<'a>(
    cx: &'a mut TestAppContext,
    output: &str,
    ready: &str,
) -> (Entity<TerminalView>, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let mut options = TerminalOptions::new(dir.keep());
    options.shell = Some((
        "/bin/sh".to_owned(),
        vec!["-c".to_owned(), format!("printf '{output}'; read line")],
    ));
    let (view, cx) =
        cx.add_window_view(|window, cx| TerminalView::new(options, style(), window, cx).unwrap());
    view.update(cx, |view, _| view.record_paint(true));
    cx.simulate_resize(size(px(400.), px(200.)));
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.read(cx).focus_handle(cx), cx)
    });
    until(cx, ready, |cx| {
        view.read_with(cx, |view, _| {
            view.terminal()
                .is_some_and(|terminal| terminal.snapshot().text().contains(ready))
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    (view, cx)
}

fn cell(cx: &mut VisualTestContext) -> Size<Pixels> {
    cx.update(|window, _| fe_terminal_view::cell_size(&style(), window))
}

/// Advances, in cells, of the controlled shaping below: a private-use icon
/// is 1.6 cells wide, a narrow one 0.5, Powerline U+E0B0 1.2; anything else is
/// one cell.
const CELL: f32 = 0.6;
const ICON: f32 = 1.6;
const NARROW_ICON: f32 = 0.5;
const POWERLINE: f32 = 1.2;

struct Shaping(NoopTextSystem);

impl PlatformTextSystem for Shaping {
    fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> gpui::Result<()> {
        self.0.add_fonts(fonts)
    }
    fn all_font_names(&self) -> Vec<String> {
        self.0.all_font_names()
    }
    fn font_id(&self, descriptor: &Font) -> gpui::Result<FontId> {
        self.0.font_id(descriptor)
    }
    fn font_metrics(&self, font_id: FontId) -> FontMetrics {
        self.0.font_metrics(font_id)
    }
    fn typographic_bounds(&self, font_id: FontId, glyph: GlyphId) -> gpui::Result<Bounds<f32>> {
        self.0.typographic_bounds(font_id, glyph)
    }
    fn advance(&self, font_id: FontId, glyph: GlyphId) -> gpui::Result<Size<f32>> {
        self.0.advance(font_id, glyph)
    }
    fn glyph_for_char(&self, font_id: FontId, ch: char) -> Option<GlyphId> {
        self.0.glyph_for_char(font_id, ch)
    }
    fn glyph_raster_bounds(
        &self,
        params: &RenderGlyphParams,
    ) -> gpui::Result<Bounds<DevicePixels>> {
        self.0.glyph_raster_bounds(params)
    }
    fn rasterize_glyph(
        &self,
        params: &RenderGlyphParams,
        bounds: Bounds<DevicePixels>,
    ) -> gpui::Result<(Size<DevicePixels>, Vec<u8>)> {
        self.0.rasterize_glyph(params, bounds)
    }
    fn layout_line(&self, text: &str, font_size: Pixels, runs: &[FontRun]) -> LineLayout {
        let mut layout = self.0.layout_line(text, font_size, runs);
        let mut glyphs = Vec::new();
        let mut x = px(0.);
        for (index, ch) in text.char_indices() {
            let em = match ch {
                '\u{E0B0}' => POWERLINE * CELL,
                '\u{F0001}' => NARROW_ICON * CELL,
                '\u{E000}'..='\u{F8FF}' | '\u{F0000}'..='\u{FFFFD}' => ICON * CELL,
                _ => CELL,
            };
            if em > 0. {
                glyphs.push(ShapedGlyph {
                    id: GlyphId(1),
                    position: point(x, px(0.)),
                    index,
                    is_emoji: false,
                });
            }
            x += font_size * em;
        }
        layout.width = x;
        if let Some(run) = layout.runs.first_mut() {
            run.glyphs = glyphs;
        }
        layout
    }
    fn recommended_rendering_mode(&self, font: FontId, size: Pixels) -> TextRenderingMode {
        self.0.recommended_rendering_mode(font, size)
    }
}

const FOLDER: &str = "\\357\\201\\273";
const POWERLINE_ARROW: &str = "\\356\\202\\260";
const NARROW_ICON_CHAR: &str = "\\363\\260\\200\\201";
const CJK: &str = "\\346\\274\\242";

fn painted(output: &str, ready: &str) -> (PaintReport, Size<Pixels>) {
    let mut cx = TestAppContext::build_with_text_system(
        TestDispatcher::new(0),
        None,
        Arc::new(Shaping(NoopTextSystem::new())),
    );
    let (view, cx) = painting(&mut cx, output, ready);
    let cell = cell(cx);
    (view.read_with(cx, |view, _| view.painted().clone()), cell)
}

/// The text painted at `row`, `col` (the origin's cell, before any centring).
fn at(texts: &[PaintedText], cell: Size<Pixels>, row: usize, col: usize) -> &PaintedText {
    texts
        .iter()
        .find(|t| {
            (f32::from(t.origin.y) / f32::from(cell.height)).round() as usize == row
                && (f32::from(t.origin.x) / f32::from(cell.width)).floor() as usize == col
        })
        .unwrap_or_else(|| panic!("nothing painted at {row},{col}: {texts:?}"))
}

/// The painted glyph's left and right edges.
fn extent(text: &PaintedText) -> (f32, f32) {
    let left = f32::from(text.origin.x);
    (left, left + f32::from(text.width))
}

fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 0.5, "{a} is not within 0.5 px of {b}");
}

fn icons(text: &PaintedText) -> bool {
    text.text
        .chars()
        .all(|c| matches!(c, '\u{E000}'..='\u{F8FF}' | '\u{F0000}'..='\u{FFFFD}'))
}

#[test]
fn an_icon_before_text_is_scaled_into_one_cell() {
    let (report, cell) = painted(
        &format!("{FOLDER}x\\r\\n{FOLDER}\\033[44m \\033[0m\\r\\ndone"),
        "done",
    );
    let em = style().font_size;
    let fitted = at(&report.text, cell, 0, 0);
    assert!(icons(fitted));
    assert!(
        fitted.font_size <= em / ICON + px(0.01),
        "{:?}",
        fitted.font_size
    );
    assert!(fitted.width <= cell.width + px(0.01));
    let (left, right) = extent(fitted);
    near((left + right) / 2., f32::from(cell.width) / 2.);
    // A coloured background next to it is a prompt segment: one cell too.
    let segment = at(&report.text, cell, 1, 0);
    assert!(segment.font_size <= em / ICON + px(0.01));
    assert!(segment.width <= cell.width + px(0.01));
    near(
        extent(segment).0 + f32::from(segment.width) / 2.,
        f32::from(cell.width) / 2.,
    );
}

#[test]
fn an_icon_before_a_blank_takes_two_cells() {
    let (report, cell) = painted(&format!("{FOLDER} \\r\\ndone"), "done");
    let icon = at(&report.text, cell, 0, 0);
    // 1.6 cells fit in two as they are, and are centred in them.
    assert_eq!(icon.font_size, style().font_size);
    assert!(icon.width <= cell.width * 2. + px(0.01));
    let (left, right) = extent(icon);
    near((left + right) / 2., f32::from(cell.width));
    // The blank cell's text isn't painted.
    assert_eq!(report.text.iter().filter(|t| icons(t)).count(), 1);
}

#[test]
fn powerline_symbols_take_one_cell_at_the_origin() {
    let (report, cell) = painted(&format!("{POWERLINE_ARROW} \\r\\ndone"), "done");
    let arrow = at(&report.text, cell, 0, 0);
    assert!(arrow.font_size < style().font_size);
    assert!(arrow.width <= cell.width + px(0.01));
    assert_eq!(arrow.origin.x, px(0.));
}

#[test]
fn glyphs_that_fit_are_painted_as_before() {
    let (report, cell) = painted(&format!("{NARROW_ICON_CHAR} Ab{CJK}\\r\\ndone"), "done");
    let em = style().font_size;
    for col in [0, 2, 3, 4] {
        let painted = at(&report.text, cell, 0, col);
        assert_eq!(painted.font_size, em);
        assert_eq!(painted.origin.x, cell.width * col as f32, "{painted:?}");
    }
    assert_eq!(at(&report.text, cell, 0, 2).text, "A");
    assert_eq!(at(&report.text, cell, 0, 4).text, "漢");
}

#[test]
fn a_block_cursor_draws_its_icon_fitted() {
    let (report, cell) = painted(&format!("{FOLDER}x\\r\\ndone\\033[1;1H"), "done");
    let [icon] = &report.cursor_text[..] else {
        panic!("{:?}", report.cursor_text);
    };
    assert!(icon.font_size <= style().font_size / ICON + px(0.01));
    assert!(icon.width <= cell.width + px(0.01));
    near(
        extent(icon).0 + f32::from(icon.width) / 2.,
        f32::from(cell.width) / 2.,
    );
}
