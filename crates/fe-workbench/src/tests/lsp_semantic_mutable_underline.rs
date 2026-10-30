//! rust-analyzer's `selfKeyword` and `selfTypeKeyword` tokens map to no
//! highlight name, so a mutable `self` is underlined in the colour the
//! grammar paints its text.

use super::*;

const LINE: &str = "fn f(&mut self) -> Self {}";

fn script() -> (Script, u32, u32, u32) {
    let mut script = super::script();
    let init = match &mut script.initialize {
        InitializeReply::Result(init) => init,
        _ => unreachable!(),
    };
    let Some(SemanticTokensServerCapabilities::SemanticTokensOptions(options)) =
        &mut init.capabilities.semantic_tokens_provider
    else {
        unreachable!()
    };
    let legend = &mut options.legend;
    let (self_ty, self_type_ty) = (
        legend.token_types.len() as u32,
        legend.token_types.len() as u32 + 1,
    );
    legend.token_types.extend([
        SemanticTokenType::new("selfKeyword"),
        SemanticTokenType::new("selfTypeKeyword"),
    ]);
    let mutable = legend.token_modifiers.len() as u32;
    legend
        .token_modifiers
        .push(SemanticTokenModifier::new("mutable"));
    (script, self_ty, self_type_ty, 1 << mutable)
}

#[gpui::test]
fn a_mutable_self_is_underlined_in_its_texts_colour(cx: &mut TestAppContext) {
    let (script, self_ty, self_type_ty, mutable) = script();
    let mut s = Lsp::with_fake_clock(cx, script, Script::default());
    std::fs::write(s.path("src/main.rs"), format!("{LINE}\n")).unwrap();
    let at = |needle: &str| LINE.find(needle).unwrap() as u32;
    let (self_at, type_at) = (at("self"), at("Self"));
    s.fake.set_reply(
        FULL,
        Scripted::now(Reply::Result(json!({
            "resultId": "1",
            "data": [0, self_at, 4, self_ty, mutable, 0, type_at - self_at, 4, self_type_ty, 0],
        }))),
    );
    s.set_trust(true);
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake
        .wait_for_document(&main, "didOpen", |r| r.is_some_and(|r| r.open));
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("main.rs is shown");
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    s.draw();
    s.until("the first ask", |s| {
        s.clock.advance(Duration::from_millis(20));
        s.cx.executor().advance_clock(REST);
        !asked(s).is_empty()
    });
    let underlined = |s: &Lsp| -> Vec<_> {
        editor.read_with(&s.cx, |view, _| {
            let model = view.model();
            (model
                .decorations(0..model.snapshot().text().len())
                .into_iter())
            .filter_map(|item| match item.kind {
                DecorationKind::SyntaxUnderline(name) => Some((item.range, name.to_string())),
                _ => None,
            })
            .collect()
        })
    };
    s.until("the tokens", |s| !underlined(s).is_empty());
    // Neither is recoloured, so the grammar's colours stay.
    let (a, b) = (self_at as usize, type_at as usize);
    assert_eq!(painted(&s, &editor), vec![]);
    // The mutable `self` has an underline with no name, drawn in the colour
    // its text is painted in (fe-gpui's render tests check the colour).
    let underlined = underlined(&s);
    assert_eq!(underlined, vec![(a..a + 4, String::new())]);
    s.draw();
    let text = editor.read_with(&s.cx, |view, _| view.drawn_color(a).unwrap());
    assert_ne!(
        text,
        editor.read_with(&s.cx, |view, _| view.theme().foreground)
    );
    let _ = b;
}

/// The scene's underline colours, with the text colour under `self`.
mod scene {
    use std::{borrow::Cow, cell::RefCell, rc::Rc, sync::Arc};

    use fe_buffer::Buffer;
    use fe_editor::{
        EditorModel,
        decorations::{Decoration, DecorationKind, DecorationProvider, Tint, overlaps},
    };
    use fe_gpui::{EditorTheme, EditorView};
    use gpui::{
        AppContext as _, AtlasKey, AtlasTextureId, AtlasTextureKind, AtlasTile, Bounds,
        DevicePixels, HeadlessAppContext, Hsla, NoopTextSystem, PlatformAtlas,
        PlatformHeadlessRenderer, PlatformTextSystem, Scene, Size, TileId, px, size,
    };
    use kiln_platform::screenshots::RgbaImage;

    const TEXT: &str = "fn f(&mut self) {}\n";

    struct Fixed(Vec<Decoration>);

    impl DecorationProvider for Fixed {
        fn decorations(
            &self,
            _: &fe_buffer::BufferSnapshot,
            range: std::ops::Range<usize>,
            out: &mut Vec<Decoration>,
        ) {
            out.extend(
                self.0
                    .iter()
                    .filter(|d| overlaps(&d.range, &range))
                    .cloned(),
            );
        }
    }

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
                bounds: Bounds {
                    origin: Default::default(),
                    size,
                },
            }))
        }

        fn remove(&self, _: &AtlasKey) {}
    }

    /// Keeps the colour of each underline of the scenes it's given.
    struct Recorder(Rc<RefCell<Vec<Hsla>>>, Arc<Tiles>);

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
            *self.0.borrow_mut() = scene.underlines.iter().map(|u| u.color).collect();
            Ok(())
        }

        fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
            self.1.clone()
        }
    }

    /// Lays text out and paints no glyph, with a box rasterized for each.
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

    #[test]
    fn a_mutable_selfs_underline_is_painted_in_its_texts_colour() {
        let underlines = Rc::new(RefCell::new(Vec::new()));
        let recorder = underlines.clone();
        let platform: Arc<dyn PlatformTextSystem> = Arc::new(Boxes(NoopTextSystem::new()));
        let mut cx = HeadlessAppContext::with_platform(platform, Arc::new(()), move || {
            let renderer = Recorder(recorder.clone(), Arc::default());
            Ok(Some(Box::new(renderer) as Box<dyn PlatformHeadlessRenderer>))
        });
        cx.update(fe_gpui::init);
        let start = TEXT.find("self").unwrap();
        // No highlight name: rust-analyzer's `self` maps to none.
        let provider = Rc::new(RefCell::new(Fixed(vec![Decoration::new(
            start..start + 4,
            DecorationKind::SyntaxUnderline("".into()),
            Tint::Muted,
        )])));
        let theme = cx.update(|cx| EditorTheme::default_dark(cx));
        let foreground = theme.foreground;
        let window = cx
            .open_window(size(px(480.), px(160.)), |_, cx| {
                cx.new(|cx| {
                    let mut model = EditorModel::new(Buffer::new(TEXT), None);
                    model.add_decoration_provider(provider);
                    EditorView::new(model, theme, cx)
                })
            })
            .expect("open a window");
        cx.update_window(window.into(), |view, _, cx| {
            let view = view.downcast::<EditorView>().expect("an editor");
            view.update(cx, |view, cx| view.set_language(Some("rust"), cx));
        })
        .expect("language");
        cx.run_until_parked();
        let mut text = None;
        for _ in 0..2 {
            cx.update_window(window.into(), |view, window, cx| {
                window.draw(cx).clear(cx);
                // Hands the frame's scene to the recorder.
                window.render_to_image().expect("record the scene");
                let view = view.downcast::<EditorView>().expect("an editor");
                text = view.read(cx).drawn_color(start);
            })
            .expect("draw");
        }
        let text = text.expect("self is drawn");
        assert_ne!(text, foreground);
        let underlines = underlines.borrow();
        assert_eq!(*underlines, vec![text]);
    }
}
