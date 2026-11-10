//! The compact Markdown element in a test window: block layout, code
//! colours, link clicks and images.

use std::{cell::RefCell, rc::Rc};

use fe_gpui::{EditorPalette, EditorSettings, EditorTheme};
use fe_markdown::BlockKind;
use fe_markdown_view::{LinkHandler, MarkdownStyle, markdown_element};
use fe_syntax::Highlight;
use gpui::{
    Bounds, Context, IntoElement, Modifiers, ParentElement as _, Pixels, Render, Styled as _,
    TestAppContext, VisualTestContext, Window, anchored, div, point, px,
};
use kiln_ui::UiTheme;

type Clicks = Rc<RefCell<Vec<String>>>;

struct Host {
    document: fe_markdown::Document,
    style: MarkdownStyle,
    clicks: Clicks,
    /// In an anchored box as wide as its content, as a hover popover
    /// places it, rather than a block as wide as the window.
    floating: bool,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let clicks = self.clicks.clone();
        let on_link: LinkHandler =
            Rc::new(move |dest, _, _| clicks.borrow_mut().push(dest.to_owned()));
        let element = markdown_element(&self.document, &self.style, px(400.), Some(on_link));
        if self.floating {
            div().child(
                anchored()
                    .position(point(px(10.), px(10.)))
                    .child(div().max_w(px(420.)).px(px(8.)).py(px(4.)).child(element)),
            )
        } else {
            div().child(element)
        }
    }
}

fn open<'a>(
    cx: &'a mut TestAppContext,
    markdown: &str,
) -> (
    &'a mut VisualTestContext,
    Clicks,
    EditorTheme,
    MarkdownStyle,
) {
    open_in(cx, markdown, false)
}

fn open_in<'a>(
    cx: &'a mut TestAppContext,
    markdown: &str,
    floating: bool,
) -> (
    &'a mut VisualTestContext,
    Clicks,
    EditorTheme,
    MarkdownStyle,
) {
    let (editor, style) = cx.update(|cx| {
        let editor = EditorTheme::from_palette(
            EditorPalette::fastedit_dark(),
            &EditorSettings::default(),
            cx,
        );
        let style = MarkdownStyle::new(&editor, &UiTheme::default_dark()).compact();
        (editor, style)
    });
    let clicks = Clicks::default();
    let host = Host {
        document: fe_markdown::parse(markdown),
        style: style.clone(),
        clicks: clicks.clone(),
        floating,
    };
    let (_, cx) = cx.add_window_view(|_, _| host);
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    (cx, clicks, editor, style)
}

fn bounds(cx: &mut VisualTestContext, selector: String) -> Option<Bounds<Pixels>> {
    cx.debug_bounds(selector.leak())
}

#[gpui::test]
fn compact_markdown_lays_out_blocks(cx: &mut TestAppContext) {
    let markdown = "# Title\n\n\
                    A paragraph with `inline code` and a [link](https://example.com/x).\n\n\
                    - one\n- two\n\n\
                    ```rust\npub struct Point;\n```\n\n\
                    ![the alt](pic.png)\n";
    let (cx, clicks, editor, style) = open(cx, markdown);

    let mut previous_bottom = None;
    let mut blocks = Vec::new();
    for ix in 0..5 {
        let b = bounds(cx, format!("md-block-{ix}")).unwrap_or_else(|| panic!("block {ix}"));
        assert!(b.size.width <= px(400.), "block {ix} is {:?} wide", b.size);
        if let Some(bottom) = previous_bottom {
            assert!(b.top() >= bottom, "block {ix} is below the one before");
        }
        previous_bottom = Some(b.bottom());
        blocks.push(b);
    }

    // The fence's keywords are painted in the theme's keyword colour.
    let keyword = Highlight::from_capture("keyword").expect("a highlight");
    let keyword_color = editor.syntax_color(keyword).expect("a keyword colour");
    assert_ne!(keyword_color, style.foreground);
    let document = fe_markdown::parse(markdown);
    assert!(matches!(document.blocks[3].kind, BlockKind::Code { .. }));
    let fence = fe_markdown::parse("```rust\npub struct Point;\n```\n");
    let glyphs = painted::glyphs(fence, style.clone());
    let keywords = glyphs.iter().filter(|c| **c == keyword_color).count();
    // `pub` and `struct`, a glyph each.
    assert!(keywords >= 9, "{keywords} keyword glyphs of {glyphs:?}");
    assert!(keywords < glyphs.len(), "the rest isn't all keyword colour");

    // A click on the link calls on_link; one beside it doesn't.
    let paragraph = blocks[1];
    let y = paragraph.origin.y + px(8.);
    cx.simulate_click(point(paragraph.origin.x + px(4.), y), Modifiers::none());
    assert_eq!(*clicks.borrow(), Vec::<String>::new());
    // The link is the paragraph's last words; the line is shorter than the box.
    let mut found = false;
    let mut x = paragraph.origin.x;
    while x < paragraph.right() && !found {
        cx.simulate_click(point(x, y), Modifiers::none());
        found = !clicks.borrow().is_empty();
        x += px(4.);
    }
    assert_eq!(*clicks.borrow(), ["https://example.com/x"]);

    // The image is its alt text, not an img.
    assert_eq!(bounds(cx, "md-image-0".into()), None);
    assert!(bounds(cx, "md-image-blocked-0-alt".into()).is_some());
}

/// The compact form is tighter than the preview: code padded 8, blocks 8
/// apart, and a rule drawn 1 px tall across the column in the floating
/// surface's hairline, which shows on the popover, even in a box only as
/// wide as its content. Code keeps the editor's line height.
#[gpui::test]
fn compact_spacing_and_a_visible_rule(cx: &mut TestAppContext) {
    let markdown = "```rust\nfn f()\n```\n\n---\n\nThe docs, long enough to set the width.\n";
    let (cx, _, editor, style) = open_in(cx, markdown, true);

    // The preview's are as they were; the compact form's are 8.
    let preview = MarkdownStyle::new(&editor, &UiTheme::default_dark());
    assert_eq!(preview.block_gap, MarkdownStyle::BLOCK_GAP);
    assert_eq!(preview.code_padding, px(16.));
    assert_eq!(preview.rule, preview.border);
    assert_eq!((style.block_gap, style.code_padding), (px(8.), px(8.)));

    let code = bounds(cx, "md-code-0".into()).expect("the code block");
    let text = bounds(cx, "md-code-0-text".into()).expect("its text");
    assert_eq!(text.left() - code.left(), px(8.), "{text:?} in {code:?}");
    assert_eq!(text.top() - code.top(), px(8.), "{text:?} in {code:?}");
    assert_eq!(
        code.bottom() - text.bottom(),
        px(8.),
        "{text:?} in {code:?}"
    );
    // One line, at the editor's line height, not the prose's 1.4.
    let line = style.code_font_size * style.code_line_height;
    assert!(
        (text.size.height - line).abs() < px(0.5),
        "{:?} for a {line:?} line",
        text.size.height
    );

    let blocks: Vec<_> = (0..3)
        .map(|ix| bounds(cx, format!("md-block-{ix}")).expect("a block"))
        .collect();
    for pair in blocks.windows(2) {
        assert_eq!(pair[1].top() - pair[0].bottom(), px(8.), "{pair:?}");
    }

    let rule = bounds(cx, "md-rule".into()).expect("the rule is drawn");
    assert_eq!(rule.size.height, px(1.));
    assert_eq!(
        rule.size.width, blocks[2].size.width,
        "{rule:?} under {blocks:?}"
    );
    assert!(rule.size.width > px(100.), "{rule:?}");
    assert_eq!(style.rule, editor.popover_border);
    assert_ne!(
        style.rule, editor.popover_background,
        "it shows on the popover"
    );
}

/// Paints a document and reads back the colour of each glyph. The test
/// platform's text system lays text out but paints no glyph, so a stand-in
/// gives every glyph a box to paint, with no platform.
mod painted {
    use std::{borrow::Cow, cell::RefCell, rc::Rc, sync::Arc};

    use gpui::{
        AppContext as _, AtlasKey, AtlasTextureId, AtlasTextureKind, AtlasTile, DevicePixels,
        HeadlessAppContext, Hsla, PlatformAtlas, PlatformHeadlessRenderer, Scene, Size, TileId, px,
        size,
    };
    use kiln_platform::screenshots::RgbaImage;

    use gpui::{Bounds, NoopTextSystem, PlatformTextSystem};

    use super::{Clicks, Host, MarkdownStyle};

    type Glyphs = Rc<RefCell<Vec<Hsla>>>;

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
    /// glyph, with a box rasterized for each glyph.
    fn text_system() -> Arc<dyn PlatformTextSystem> {
        struct Boxes(NoopTextSystem);

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

        Arc::new(Boxes(NoopTextSystem::new()))
    }

    /// The colour of each glyph painted by `markdown_element` drawing
    /// `document` in `style`.
    pub(super) fn glyphs(document: fe_markdown::Document, style: MarkdownStyle) -> Vec<Hsla> {
        let recorded = Glyphs::default();
        let recorder = recorded.clone();
        let mut cx = HeadlessAppContext::with_platform(text_system(), Arc::new(()), move || {
            let renderer = Recorder(recorder.clone(), Arc::default());
            Ok(Some(Box::new(renderer) as Box<dyn PlatformHeadlessRenderer>))
        });
        let window = cx
            .open_window(size(px(480.), px(160.)), |_, cx| {
                cx.new(|_| Host {
                    document,
                    style,
                    clicks: Clicks::default(),
                    floating: false,
                })
            })
            .expect("open a window");
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            // Hands the frame's scene to the recorder.
            window.render_to_image().expect("record the scene");
        })
        .expect("draw");
        recorded.take()
    }
}
