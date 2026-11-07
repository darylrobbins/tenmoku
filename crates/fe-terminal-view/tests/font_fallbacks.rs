//! The terminal's font fallbacks: every font the element asks the text system
//! for carries them.

// Unix only: the emulator is fed `/bin/sh` scripts. It is the same code on
// Windows, where ConPTY re-renders the stream (see fe-terminal's conpty.rs).
#![cfg(unix)]

use std::borrow::Cow;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use fe_terminal_view::{TerminalOptions, TerminalStyle, TerminalView};
use gpui::{
    Bounds, DevicePixels, EntityInputHandler as _, Focusable, Font, FontId, FontMetrics, FontStyle,
    FontWeight, GlyphId, Hsla, LineLayout, NoopTextSystem, Pixels, PlatformTextSystem,
    RenderGlyphParams, Size, TestAppContext, TestDispatcher, TextRenderingMode, px, rgb, size,
};

fn style(fallbacks: &[&str]) -> TerminalStyle {
    let background: Hsla = rgb(0x101010).into();
    TerminalStyle {
        font_family: "Menlo".into(),
        font_fallbacks: fallbacks.iter().map(|&name| name.into()).collect(),
        font_size: px(14.),
        line_height: TerminalStyle::LINE_HEIGHT,
        background,
        foreground: rgb(0xd0d0d0).into(),
        cursor: rgb(0xffcc00).into(),
        selection: rgb(0x334466).into(),
        ansi: TerminalStyle::ansi_for(background),
    }
}

/// A text system recording each `Font` descriptor `font_id` is asked for.
struct Recording {
    inner: NoopTextSystem,
    fonts: Arc<Mutex<Vec<Font>>>,
}

impl PlatformTextSystem for Recording {
    fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> gpui::Result<()> {
        self.inner.add_fonts(fonts)
    }
    fn all_font_names(&self) -> Vec<String> {
        self.inner.all_font_names()
    }
    fn font_id(&self, descriptor: &Font) -> gpui::Result<FontId> {
        self.fonts.lock().unwrap().push(descriptor.clone());
        self.inner.font_id(descriptor)
    }
    fn font_metrics(&self, font_id: FontId) -> FontMetrics {
        self.inner.font_metrics(font_id)
    }
    fn typographic_bounds(&self, font_id: FontId, glyph: GlyphId) -> gpui::Result<Bounds<f32>> {
        self.inner.typographic_bounds(font_id, glyph)
    }
    fn advance(&self, font_id: FontId, glyph: GlyphId) -> gpui::Result<Size<f32>> {
        self.inner.advance(font_id, glyph)
    }
    fn glyph_for_char(&self, font_id: FontId, ch: char) -> Option<GlyphId> {
        self.inner.glyph_for_char(font_id, ch)
    }
    fn glyph_raster_bounds(
        &self,
        params: &RenderGlyphParams,
    ) -> gpui::Result<Bounds<DevicePixels>> {
        self.inner.glyph_raster_bounds(params)
    }
    fn rasterize_glyph(
        &self,
        params: &RenderGlyphParams,
        bounds: Bounds<DevicePixels>,
    ) -> gpui::Result<(Size<DevicePixels>, Vec<u8>)> {
        self.inner.rasterize_glyph(params, bounds)
    }
    fn layout_line(&self, text: &str, font_size: Pixels, runs: &[gpui::FontRun]) -> LineLayout {
        self.inner.layout_line(text, font_size, runs)
    }
    fn recommended_rendering_mode(&self, font: FontId, size: Pixels) -> TextRenderingMode {
        self.inner.recommended_rendering_mode(font, size)
    }
}

/// Every font the element asks for while showing plain, bold and italic
/// cells and marked text, with `fallbacks` set.
fn requested(fallbacks: &[&str]) -> Vec<Font> {
    let fonts = Arc::new(Mutex::new(Vec::new()));
    let mut cx = TestAppContext::build_with_text_system(
        TestDispatcher::new(0),
        None,
        Arc::new(Recording {
            inner: NoopTextSystem::new(),
            fonts: fonts.clone(),
        }),
    );
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let mut options = TerminalOptions::new(dir.keep());
    options.shell = Some((
        "/bin/sh".to_owned(),
        vec![
            "-c".to_owned(),
            "printf 'plain \\033[1mbold\\033[0m \\033[3mitalic\\033[0m done'; read line".to_owned(),
        ],
    ));
    let style = style(fallbacks);
    let (view, cx) =
        cx.add_window_view(|window, cx| TerminalView::new(options, style, window, cx).unwrap());
    cx.simulate_resize(size(px(400.), px(200.)));
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.read(cx).focus_handle(cx), cx)
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let done = view.read_with(cx, |view, _| {
            view.terminal()
                .is_some_and(|terminal| terminal.snapshot().text().contains("done"))
        });
        if done {
            break;
        }
        assert!(Instant::now() < deadline, "timed out waiting for output");
        thread::sleep(Duration::from_millis(10));
    }
    // Marked text shapes with the same font.
    let marked = cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.replace_and_mark_text_in_range(None, "é", None, window, cx);
            view.marked_text_range(window, cx)
        })
    });
    assert!(marked.is_some(), "no marked text");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    fonts.lock().unwrap().clone()
}

#[test]
fn every_font_asked_for_has_the_fallbacks() {
    let fonts = requested(&["Symbols Nerd Font Mono"]);
    assert!(
        fonts.iter().any(|font| font.weight == FontWeight::BOLD),
        "{fonts:?}"
    );
    assert!(
        fonts.iter().any(|font| font.style == FontStyle::Italic),
        "{fonts:?}"
    );
    let menlo: Vec<_> = fonts.iter().filter(|font| font.family == "Menlo").collect();
    assert!(menlo.len() >= 3, "{fonts:?}");
    for font in menlo {
        let fallbacks = font.fallbacks.as_ref().expect("fallbacks set");
        assert_eq!(fallbacks.fallback_list(), ["Symbols Nerd Font Mono"]);
    }
}

#[test]
fn an_empty_list_leaves_fallbacks_unset() {
    let fonts = requested(&[]);
    assert!(!fonts.is_empty());
    for font in fonts {
        assert_eq!(font.fallbacks, None, "{font:?}");
    }
}

#[test]
fn style_font_has_the_fallbacks() {
    let font = style(&["A", "B"]).font();
    assert_eq!(font.family, "Menlo");
    assert_eq!(font.fallbacks.unwrap().fallback_list(), ["A", "B"]);
    assert_eq!(style(&[]).font().fallbacks, None);
}
