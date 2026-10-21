//! A hover's Markdown is drawn rich in the popover, and its links open
//! (T242).

use fe_lsp::fake::{Reply, Scripted};
use fe_syntax::Highlight;
use gpui::{Focusable as _, Modifiers, TestAppContext, point, px};
use lsp_types::{LSPAny, LSPObject, request::HoverRequest, request::Request as _};

use super::super::lsp::Lsp;

const HOVER: &str = "```rust\nfn f()\n```\n---\nSee [docs](https://example.com).";

#[gpui::test]
fn hover_markdown_renders_rich(cx: &mut TestAppContext) {
    let markup = LSPObject::from_iter([
        ("kind".to_owned(), LSPAny::from("markdown")),
        ("value".to_owned(), LSPAny::from(HOVER)),
    ]);
    let hover = LSPObject::from_iter([("contents".to_owned(), LSPAny::Object(markup))]);
    let mut s = Lsp::new(cx);
    s.fake.set_reply(
        HoverRequest::METHOD,
        Scripted::now(Reply::Result(LSPAny::Object(hover))),
    );
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake
        .wait_for_document(&main, "didOpen", |r| r.is_some_and(|r| r.open));
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("main.rs is shown");
    s.cx.update(|window, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().set_cursor("fn ma".len());
            cx.notify();
        });
        window.focus(&editor.focus_handle(cx), cx);
    });
    s.draw();
    let tabs = |s: &Lsp| {
        s.workbench
            .read_with(&s.cx, |w, cx| w.pane().read(cx).tabs().len())
    };
    let before = tabs(&s);

    s.cx.simulate_keystrokes("cmd-k cmd-i");
    s.until("the hover", |s| {
        editor
            .read_with(&s.cx, |view, _| view.popover_text())
            .is_some()
    });

    // The fence is block 0, the rule 1, the paragraph 2.
    let fence = s.cx.debug_bounds("md-block-0");
    let paragraph = s.cx.debug_bounds("md-block-2").expect("the paragraph");
    assert!(fence.is_some(), "the fence is drawn");

    // The fence's `fn` is painted in the theme's keyword colour.
    let (renderer, theme) = editor.read_with(&s.cx, |view, _| {
        (
            view.markdown_renderer().cloned().expect("a renderer"),
            view.theme().clone(),
        )
    });
    let keyword = Highlight::from_capture("keyword").expect("a highlight");
    let keyword = theme.syntax_color(keyword).expect("a keyword colour");
    assert_ne!(keyword, theme.foreground);
    let glyphs = painted::glyphs(renderer, theme, "```rust\nfn f()\n```");
    let keywords = glyphs.iter().filter(|c| **c == keyword).count();
    // `fn`, a glyph each.
    assert!(keywords >= 2, "{keywords} keyword glyphs of {glyphs:?}");
    assert!(keywords < glyphs.len(), "the rest isn't all keyword colour");

    // The link is the paragraph's last words.
    let y = paragraph.origin.y + px(8.);
    let mut x = paragraph.origin.x;
    while s.cx.opened_url().is_none() && x < paragraph.right() {
        s.cx.simulate_click(point(x, y), Modifiers::none());
        x += px(4.);
    }
    assert_eq!(s.cx.opened_url().as_deref(), Some("https://example.com"));
    s.draw();
    assert_eq!(tabs(&s), before);
}

/// Paints a document and reads back the colour of each glyph, and the
/// text laid out. The test platform's text system lays text out but paints
/// no glyph, so a stand-in gives every glyph a box to paint, with no
/// platform.
pub(super) mod painted {
    use std::{borrow::Cow, cell::RefCell, rc::Rc, sync::Arc};

    use fe_gpui::{EditorTheme, MarkdownRenderer};
    use gpui::{
        AppContext as _, AtlasKey, AtlasTextureId, AtlasTextureKind, AtlasTile, DevicePixels,
        HeadlessAppContext, Hsla, PlatformAtlas, PlatformHeadlessRenderer, Scene, Size, TileId, px,
        size,
    };
    use gpui::{Context, IntoElement, ParentElement as _, Render, Window, div};
    use kiln_platform::screenshots::RgbaImage;

    use gpui::{Bounds, NoopTextSystem, PlatformTextSystem};

    type Glyphs = Rc<RefCell<Vec<Hsla>>>;
    /// Each line of text laid out, in order.
    pub(crate) type Lines = Arc<std::sync::Mutex<Vec<String>>>;

    /// A renderer that draws nothing and keeps the glyph colours of each
    /// scene it's given. Its atlas hands out a tile per glyph.
    struct Recorder(Glyphs, Arc<Tiles>);

    #[derive(Default)]
    struct Tiles(std::sync::atomic::AtomicU32);

    impl PlatformAtlas for Tiles {
        fn get_or_insert_with<'a>(
            &self,
            _: AtlasKey,
            build: &mut dyn FnMut() -> gpui::Result<Option<(Size<DevicePixels>, Cow<'a, [u8]>)>>,
        ) -> gpui::Result<Option<AtlasTile>> {
            let Some((size, _)) = build()? else {
                return Ok(None);
            };
            let id = self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(Some(AtlasTile {
                texture_id: AtlasTextureId {
                    index: 0,
                    kind: AtlasTextureKind::Monochrome,
                },
                tile_id: TileId(id),
                padding: 0,
                bounds: gpui::Bounds {
                    origin: Default::default(),
                    size,
                },
            }))
        }

        fn remove(&self, _: &AtlasKey) {}
    }

    impl PlatformHeadlessRenderer for Recorder {
        fn render_scene_to_image(
            &mut self,
            scene: &Scene,
            size: Size<DevicePixels>,
        ) -> gpui::Result<RgbaImage> {
            self.render_scene(scene, size)?;
            Ok(RgbaImage::new(1, 1))
        }

        fn render_scene(&mut self, scene: &Scene, _: Size<DevicePixels>) -> gpui::Result<()> {
            *self.0.borrow_mut() = (scene.monochrome_sprites.iter())
                .map(|sprite| sprite.color)
                .chain(scene.subpixel_sprites.iter().map(|sprite| sprite.color))
                .collect();
            Ok(())
        }

        fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
            self.1.clone()
        }
    }

    /// The test platform's text system, which lays text out but paints no
    /// glyph, with a box rasterized for each glyph, and keeps each line it
    /// lays out in `lines`.
    pub(crate) fn text_system(lines: Lines) -> Arc<dyn PlatformTextSystem> {
        struct Boxes(NoopTextSystem, Lines);

        impl PlatformTextSystem for Boxes {
            fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> gpui::Result<()> {
                self.0.add_fonts(fonts)
            }
            fn all_font_names(&self) -> Vec<String> {
                self.0.all_font_names()
            }
            fn font_id(&self, descriptor: &gpui::Font) -> gpui::Result<gpui::FontId> {
                self.0.font_id(descriptor)
            }
            fn font_metrics(&self, font_id: gpui::FontId) -> gpui::FontMetrics {
                self.0.font_metrics(font_id)
            }
            fn typographic_bounds(
                &self,
                font_id: gpui::FontId,
                glyph_id: gpui::GlyphId,
            ) -> gpui::Result<Bounds<f32>> {
                self.0.typographic_bounds(font_id, glyph_id)
            }
            fn advance(
                &self,
                font_id: gpui::FontId,
                glyph_id: gpui::GlyphId,
            ) -> gpui::Result<Size<f32>> {
                self.0.advance(font_id, glyph_id)
            }
            fn glyph_for_char(&self, font_id: gpui::FontId, ch: char) -> Option<gpui::GlyphId> {
                self.0.glyph_for_char(font_id, ch)
            }
            fn glyph_raster_bounds(
                &self,
                _: &gpui::RenderGlyphParams,
            ) -> gpui::Result<Bounds<DevicePixels>> {
                Ok(Bounds {
                    origin: Default::default(),
                    size: size(DevicePixels(8), DevicePixels(8)),
                })
            }
            fn rasterize_glyph(
                &self,
                _: &gpui::RenderGlyphParams,
                bounds: Bounds<DevicePixels>,
            ) -> gpui::Result<(Size<DevicePixels>, Vec<u8>)> {
                Ok((bounds.size, vec![255; 64]))
            }
            fn layout_line(
                &self,
                text: &str,
                font_size: gpui::Pixels,
                runs: &[gpui::FontRun],
            ) -> gpui::LineLayout {
                self.1.lock().expect("the lines").push(text.to_owned());
                self.0.layout_line(text, font_size, runs)
            }
            fn recommended_rendering_mode(
                &self,
                font_id: gpui::FontId,
                font_size: gpui::Pixels,
            ) -> gpui::TextRenderingMode {
                self.0.recommended_rendering_mode(font_id, font_size)
            }
        }

        Arc::new(Boxes(NoopTextSystem::new(), lines))
    }

    /// The colour of each glyph `renderer` paints for `markdown`.
    pub(crate) fn glyphs(
        renderer: Rc<dyn MarkdownRenderer>,
        theme: EditorTheme,
        markdown: &str,
    ) -> Vec<Hsla> {
        paint(renderer, theme, markdown).0
    }

    /// The colour of each glyph `renderer` paints for `markdown`, and each
    /// line of text it lays out.
    pub(crate) fn paint(
        renderer: Rc<dyn MarkdownRenderer>,
        theme: EditorTheme,
        markdown: &str,
    ) -> (Vec<Hsla>, Vec<String>) {
        struct Host(Rc<dyn MarkdownRenderer>, EditorTheme, String);

        impl Render for Host {
            fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                div().child(self.0.render(&self.2, &self.1, px(400.), window, cx))
            }
        }

        let recorded = Glyphs::default();
        let recorder = recorded.clone();
        let lines = Lines::default();
        let platform = text_system(lines.clone());
        let mut cx = HeadlessAppContext::with_platform(platform, Arc::new(()), move || {
            let renderer = Recorder(recorder.clone(), Arc::default());
            Ok(Some(Box::new(renderer) as Box<dyn PlatformHeadlessRenderer>))
        });
        let markdown = markdown.to_owned();
        let window = cx
            .open_window(size(px(480.), px(160.)), |_, cx| {
                cx.new(|_| Host(renderer, theme, markdown))
            })
            .expect("open a window");
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            // Hands the frame's scene to the recorder.
            window.render_to_image().expect("record the scene");
        })
        .expect("draw");
        let lines = std::mem::take(&mut *lines.lock().expect("the lines"));
        (recorded.take(), lines)
    }
}
