//! The hover card in B3 (`docs/design/design-system.md`, Hover card): the
//! module path in faint small code, the signature in its syntax colours,
//! the docs, and a footer with where it's defined and `gd definition`, on
//! a floating surface: `surface.raised`, radius 6, a 1 px hairline and the
//! float shadow.
//!
//! The text parts are read from [`PaintedHoverCard`], which records each
//! with the runs it was drawn in; the surface, and the glyphs' colours,
//! from the scene a headless window paints.

use std::{cell::RefCell, path::PathBuf, rc::Rc};

use fe_buffer::{Buffer, BufferId, ResolvedSelection, Version};
use fe_config::{EditorPalette, themes};
use fe_editor::EditorModel;
use fe_gpui::{
    DefinitionFuture, EditorSettings, EditorTheme, EditorView, HoverCardPart, HoverCardText,
    HoverContent, HoverDefinition, HoverFuture, HoverProvider, PaintedHoverCard, init,
    set_vim_enabled,
};
use fe_syntax::Highlight;
use gpui::{
    Bounds, Entity, Focusable as _, FontWeight, Hsla, Pixels, TestAppContext, VisualTestContext, px,
};
use kiln_core::theme::ThemeData;

/// rust-analyzer's hover on `HashMap::new`: the module path's fence, the
/// signature's, a rule and the docs.
const HOVER: &str = "```rust\nstd::collections::hash::map::HashMap\n```\n\n\
                     ```rust\npub fn new() -> HashMap<K, V, RandomState>\n```\n\n---\n\n\
                     Creates an empty `HashMap`.\n";

const PATH: &str = "std::collections::hash::map::HashMap";
const SIGNATURE: &str = "pub fn new() -> HashMap<K, V, RandomState>";

const TEXT: &str = "fn main() {\n    let map = HashMap::new();\n}\n";

/// The bundled Yohen theme, the design's dark one, and its editor palette.
fn yohen() -> (ThemeData, EditorPalette) {
    let theme = ThemeData::from_file(&themes::find("yohen").expect("Yohen ships").file());
    let (palette, _) = EditorPalette::for_theme(&theme);
    (theme, palette)
}

/// Answers [`HOVER`] for every word, and `definition` for where it's
/// defined.
struct Provider {
    definition: RefCell<Option<HoverDefinition>>,
}

impl HoverProvider for Provider {
    fn hover(&self, _: BufferId, _: Version, _: usize) -> HoverFuture {
        Box::pin(std::future::ready(Some(HoverContent {
            text: HOVER.into(),
            range: None,
        })))
    }

    fn definition(&self, _: BufferId, _: Version, _: usize) -> DefinitionFuture {
        Box::pin(std::future::ready(self.definition.borrow().clone()))
    }
}

/// An editor on `src/main.rs` in Yohen, the caret in `new`, vim on or
/// off, asking `provider`.
fn editor(
    vim: bool,
    provider: Rc<Provider>,
    cx: &mut TestAppContext,
) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(|cx| {
        init(cx);
        set_vim_enabled(cx, vim);
        PaintedHoverCard::set(cx);
    });
    let (_, palette) = yohen();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let path = PathBuf::from("/work/src/main.rs");
        let mut model = EditorModel::new(Buffer::new(TEXT), Some(path));
        let caret = TEXT.find("new").unwrap() + 1;
        model.set_selections(vec![ResolvedSelection {
            range: caret..caret,
            reversed: false,
        }]);
        let theme = EditorTheme::from_palette(&palette, &EditorSettings::default(), cx);
        let mut view = EditorView::new(model, theme, cx);
        view.set_hover_provider(Some(provider));
        view
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    (view, cx)
}

/// Shows the hover at the caret and draws it again: the card's texts in
/// the last frame, by part.
fn hover(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<HoverCardText> {
    cx.simulate_keystrokes("cmd-k cmd-i");
    cx.run_until_parked();
    last_frame(view, cx)
}

/// The card's texts as a fresh frame builds them.
fn last_frame(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<HoverCardText> {
    cx.update(|_, cx| PaintedHoverCard::take(cx));
    view.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    let texts = cx.update(|_, cx| PaintedHoverCard::take(cx));
    // Each frame records the same: keep one.
    let len = (1..=texts.len())
        .find(|&len| {
            texts.len().is_multiple_of(len)
                && (texts.chunks(len)).all(|frame| frame == &texts[..len])
        })
        .unwrap_or(texts.len());
    texts[..len].to_vec()
}

fn parts(texts: &[HoverCardText], part: HoverCardPart) -> Vec<&HoverCardText> {
    texts.iter().filter(|text| text.part == part).collect()
}

fn one(texts: &[HoverCardText], part: HoverCardPart) -> &HoverCardText {
    match parts(texts, part).as_slice() {
        [one] => one,
        found => panic!("{} {part:?} in {texts:#?}", found.len()),
    }
}

/// `text`'s colour at each byte.
fn colours(text: &HoverCardText) -> Vec<Hsla> {
    let covered: usize = text.runs.iter().map(|run| run.len).sum();
    assert_eq!(covered, text.text.len(), "the runs cover {:?}", text.text);
    (text.runs.iter())
        .flat_map(|run| std::iter::repeat_n(run.color, run.len))
        .collect()
}

fn theme(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> EditorTheme {
    view.read_with(cx, |view, _| view.theme().clone())
}

#[gpui::test]
fn path_signature_docs_and_footer(cx: &mut TestAppContext) {
    let provider = Rc::new(Provider {
        definition: RefCell::new(Some(HoverDefinition {
            path: Some(PathBuf::from(
                "/rust/library/std/src/collections/hash/map.rs",
            )),
            row: 264,
        })),
    });
    let (view, cx) = editor(true, provider.clone(), cx);
    let theme = theme(&view, cx);
    let (tokens, _) = yohen();
    let texts = hover(&view, cx);

    // The module path: faint code small, the theme's faint text.
    let path = one(&texts, HoverCardPart::ModulePath);
    assert_eq!(path.text, PATH);
    assert_eq!(path.font_size, px(12.), "code small, at the default size");
    assert_eq!(path.font_size, theme.font_size - px(1.));
    assert_eq!(path.runs.len(), 1);
    assert_eq!(path.runs[0].font.family, theme.font_family);
    let faint = path.runs[0].color;
    assert_eq!(faint, kiln_ui_hsla(tokens.tokens.text.faint.to_rgba_u32()));
    assert_ne!(faint, theme.foreground);

    // The signature in the editor's font and size, keywords in the
    // theme's keyword colour, the rest not all of it.
    let signature = one(&texts, HoverCardPart::Signature);
    assert_eq!(signature.text, SIGNATURE);
    assert_eq!(signature.font_size, theme.font_size);
    assert!(
        (signature.runs.iter()).all(|run| run.font.family == theme.font_family),
        "{signature:?}"
    );
    let keyword = theme
        .syntax_color(Highlight::from_capture("keyword").unwrap())
        .expect("a keyword colour");
    assert_ne!(keyword, theme.foreground);
    let colours = colours(signature);
    for word in ["pub", "fn"] {
        let at = SIGNATURE.find(word).unwrap();
        assert!(
            colours[at..at + word.len()].iter().all(|c| *c == keyword),
            "{word:?} in {:?}",
            signature.runs
        );
    }
    let name = SIGNATURE.find("new").unwrap();
    assert_ne!(colours[name], keyword, "the name isn't a keyword");

    // The docs as text, without a Markdown renderer: the rule, the prose.
    let docs: Vec<&str> = (parts(&texts, HoverCardPart::Docs).into_iter())
        .map(|docs| docs.text.as_str())
        .collect();
    assert_eq!(docs, ["---\n\nCreates an empty `HashMap`."]);

    // The footer: where it's defined, and gd, in UI small, faint.
    let footer: Vec<&str> = (parts(&texts, HoverCardPart::Footer).into_iter())
        .map(|footer| footer.text.as_str())
        .collect();
    assert_eq!(footer, ["map.rs:265", "gd definition"]);
    for footer in parts(&texts, HoverCardPart::Footer) {
        assert_eq!(footer.font_size, px(11.), "UI small, at the default size");
        assert_eq!(footer.runs.len(), 1);
        let run = &footer.runs[0];
        assert_eq!((run.color, run.font.weight), (faint, FontWeight::MEDIUM));
        assert_ne!(run.font.family, theme.font_family, "the UI font, not code");
    }

    // Top to bottom, inside the card: the path, the signature, the docs
    // and the footer.
    let bounds = |selector: &'static str, cx: &mut VisualTestContext| -> Bounds<Pixels> {
        cx.debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is drawn"))
    };
    let card = bounds("hover-popover", cx);
    let order = [
        "hover-card-path",
        "hover-card-signature",
        "hover-card-docs",
        "hover-card-footer",
    ]
    .map(|selector| bounds(selector, cx));
    for pair in order.windows(2) {
        assert!(pair[0].bottom() <= pair[1].top(), "{pair:?}");
    }
    for part in order {
        assert!(card.contains(&part.origin), "{part:?} in {card:?}");
    }
    assert!(card.bottom() >= order[3].bottom());

    // Defined where the hover is: no location, the key still.
    let row = TEXT[..TEXT.find("new").unwrap()].matches('\n').count();
    *provider.definition.borrow_mut() = Some(HoverDefinition {
        path: None,
        row: row as u32,
    });
    cx.simulate_keystrokes("escape");
    let texts = hover(&view, cx);
    let footer: Vec<&str> = (parts(&texts, HoverCardPart::Footer).into_iter())
        .map(|footer| footer.text.as_str())
        .collect();
    assert_eq!(footer, ["gd definition"]);

    // Elsewhere in the same file: its name and row.
    *provider.definition.borrow_mut() = Some(HoverDefinition { path: None, row: 0 });
    cx.simulate_keystrokes("escape");
    let texts = hover(&view, cx);
    let footer: Vec<&str> = (parts(&texts, HoverCardPart::Footer).into_iter())
        .map(|footer| footer.text.as_str())
        .collect();
    assert_eq!(footer, ["main.rs:1", "gd definition"]);
}

/// Without vim, the footer has no `gd`; with nothing to say, there's none.
#[gpui::test]
fn the_footer_without_vim(cx: &mut TestAppContext) {
    let provider = Rc::new(Provider {
        definition: RefCell::new(Some(HoverDefinition {
            path: Some(PathBuf::from("/rust/map.rs")),
            row: 9,
        })),
    });
    let (view, cx) = editor(false, provider.clone(), cx);
    let texts = hover(&view, cx);
    let footer: Vec<&str> = (parts(&texts, HoverCardPart::Footer).into_iter())
        .map(|footer| footer.text.as_str())
        .collect();
    assert_eq!(footer, ["map.rs:10"]);

    *provider.definition.borrow_mut() = None;
    cx.simulate_keystrokes("escape");
    let texts = hover(&view, cx);
    assert!(
        parts(&texts, HoverCardPart::Footer).is_empty(),
        "{texts:#?}"
    );
    assert!(cx.debug_bounds("hover-card-footer").is_none());
    assert_eq!(one(&texts, HoverCardPart::Signature).text, SIGNATURE);
}

/// One fence with the path, a blank line and the signature, as the audit's
/// scene has it, splits the same way; a fence that's code throughout is
/// all signature.
#[gpui::test]
fn one_fence_headers(cx: &mut TestAppContext) {
    let (view, cx) = editor(false, Rc::new(Provider::default()), cx);
    let show = |text: &str, cx: &mut VisualTestContext| {
        view.update(cx, |view, cx| {
            let content = HoverContent {
                text: text.into(),
                range: None,
            };
            view.show_popover(content, cx)
        });
        cx.run_until_parked();
        last_frame(&view, cx)
    };
    let texts = show(&format!("```rust\n{PATH}\n\n{SIGNATURE}\n```\n\nDocs."), cx);
    assert_eq!(one(&texts, HoverCardPart::ModulePath).text, PATH);
    assert_eq!(one(&texts, HoverCardPart::Signature).text, SIGNATURE);
    assert_eq!(one(&texts, HoverCardPart::Docs).text, "Docs.");
    // The host's text: no footer.
    assert!(parts(&texts, HoverCardPart::Footer).is_empty());

    let texts = show("```rust\nlet x = 1;\n\nlet y = 2;\n```", cx);
    assert!(parts(&texts, HoverCardPart::ModulePath).is_empty());
    assert_eq!(
        one(&texts, HoverCardPart::Signature).text,
        "let x = 1;\n\nlet y = 2;"
    );

    // No fence first: all of it docs.
    let texts = show("Some *docs*.\n\n```rust\nfn f()\n```", cx);
    assert!(parts(&texts, HoverCardPart::Signature).is_empty());
    let docs: Vec<&str> = (parts(&texts, HoverCardPart::Docs).into_iter())
        .map(|docs| docs.text.as_str())
        .collect();
    assert_eq!(docs, ["Some *docs*.", "fn f()"]);
}

impl Default for Provider {
    fn default() -> Self {
        Self {
            definition: RefCell::new(None),
        }
    }
}

/// A theme file colour as GPUI's, as the editor palette turns it.
fn kiln_ui_hsla(rgba: u32) -> Hsla {
    gpui::rgba(rgba).into()
}

/// The card as a headless window paints it: one quad in the raised
/// surface's colour, a 1 px hairline round it in the popover border's,
/// both radius 6, the float shadow under exactly it, and glyphs in it in the
/// faint text's, the foreground's and the keyword colour.
#[test]
fn the_card_is_a_floating_surface() {
    let (tokens, palette) = yohen();
    let (scene, theme) = painted::card(&palette, HOVER);
    let raised = kiln_ui_hsla(tokens.tokens.surface.raised.to_rgba_u32());
    assert_eq!(theme.popover_background, raised, "Yohen's overlay");
    let scale = scene.scale;

    let cards: Vec<_> = (scene.quads.iter())
        .filter(|quad| quad.background == raised.into())
        .collect();
    let [card] = cards.as_slice() else {
        panic!("one raised quad: {cards:#?}");
    };
    // Its border, painted over its content round the same box, an edge at
    // a time.
    let borders: Vec<_> = (scene.quads.iter())
        .filter(|quad| quad.bounds == card.bounds && quad.border_color == theme.popover_border)
        .collect();
    assert!(
        !borders.is_empty(),
        "a hairline round the card: {:#?}",
        scene.quads
    );
    for (ix, quad) in borders.iter().chain([card]).enumerate() {
        let widths = &quad.border_widths;
        let widths = [widths.top, widths.right, widths.bottom, widths.left];
        if ix < borders.len() {
            assert_eq!(widths, [px(1.).scale(scale); 4], "a 1 px hairline");
        }
        let radii = &quad.corner_radii;
        let radii = [
            radii.top_left,
            radii.top_right,
            radii.bottom_right,
            radii.bottom_left,
        ];
        assert_eq!(radii, [px(6.).scale(scale); 4]);
    }

    let shadows: Vec<_> = (scene.shadows.iter())
        .filter(|shadow| shadow.element_bounds == card.bounds)
        .collect();
    let [shadow] = shadows.as_slice() else {
        panic!("one shadow under the card: {:#?}", scene.shadows);
    };
    assert_eq!(shadow.blur_radius, px(32.).scale(scale));
    assert_eq!(
        shadow.color,
        gpui::hsla(0., 0., 0., 0.55),
        "55% black, dark"
    );
    assert_eq!(
        shadow.bounds.origin.y - card.bounds.origin.y,
        px(12.).scale(scale)
    );
    assert_eq!(shadow.bounds.origin.x, card.bounds.origin.x);

    // The glyphs painted on the card.
    let keyword = theme
        .syntax_color(Highlight::from_capture("keyword").unwrap())
        .expect("a keyword colour");
    let faint = kiln_ui_hsla(tokens.tokens.text.faint.to_rgba_u32());
    let on_card: Vec<Hsla> = (scene.glyphs.iter())
        .filter(|(at, _)| card.bounds.contains(at))
        .map(|(_, colour)| *colour)
        .collect();
    let count = |colour: Hsla| on_card.iter().filter(|c| **c == colour).count();
    // The path's 36 characters, faint; `pub` and `fn`, keyword-coloured.
    assert!(count(faint) >= PATH.len(), "{on_card:?}");
    assert!(count(keyword) >= 5, "{on_card:?}");
    assert!(count(theme.foreground) > 0, "{on_card:?}");
}

/// A headless window's scene, read back through a renderer that keeps it.
/// The test platform's text system lays text out but paints no glyph, so
/// a stand-in gives every glyph a box to paint.
mod painted {
    use std::{borrow::Cow, cell::RefCell, path::PathBuf, rc::Rc, sync::Arc};

    use fe_buffer::{Buffer, ResolvedSelection};
    use fe_config::EditorPalette;
    use fe_editor::EditorModel;
    use fe_gpui::{EditorSettings, EditorTheme, EditorView, HoverContent, init};
    use gpui::{
        AppContext as _, AtlasKey, AtlasTextureId, AtlasTextureKind, AtlasTile, Bounds,
        DevicePixels, HeadlessAppContext, Hsla, NoopTextSystem, PlatformAtlas,
        PlatformHeadlessRenderer, PlatformTextSystem, Point, Quad, ScaledPixels, Scene, Shadow,
        Size, TileId, px, size,
    };
    use kiln_platform::screenshots::RgbaImage;

    /// What a frame painted.
    #[derive(Default)]
    pub(super) struct Painted {
        pub(super) quads: Vec<Quad>,
        pub(super) shadows: Vec<Shadow>,
        /// Each glyph's origin and colour.
        pub(super) glyphs: Vec<(Point<ScaledPixels>, Hsla)>,
        pub(super) scale: f32,
    }

    type Kept = Rc<RefCell<Painted>>;

    struct Recorder(Kept, Arc<Tiles>);

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
            let mut kept = self.0.borrow_mut();
            kept.quads = scene.quads.clone();
            kept.shadows = scene.shadows.clone();
            kept.glyphs = (scene.monochrome_sprites.iter())
                .map(|sprite| (sprite.bounds.origin, sprite.color))
                .chain(
                    (scene.subpixel_sprites.iter())
                        .map(|sprite| (sprite.bounds.origin, sprite.color)),
                )
                .collect();
            Ok(())
        }

        fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
            self.1.clone()
        }
    }

    /// The test platform's text system with a box rasterized for each
    /// glyph.
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

    /// An editor in `palette` showing `hover` at its caret, painted: the
    /// frame's quads, shadows and glyphs, and the editor's theme. Its text
    /// has no language, so only the card's code is highlighted.
    pub(super) fn card(palette: &EditorPalette, hover: &str) -> (Painted, EditorTheme) {
        let kept = Kept::default();
        let recorder = kept.clone();
        let mut cx = HeadlessAppContext::with_platform(text_system(), Arc::new(()), move || {
            let renderer = Recorder(recorder.clone(), Arc::default());
            Ok(Some(Box::new(renderer) as Box<dyn PlatformHeadlessRenderer>))
        });
        cx.update(init);
        let palette = palette.clone();
        let window = cx
            .open_window(size(px(900.), px(500.)), move |_, cx| {
                cx.new(|cx| {
                    let path = PathBuf::from("/work/notes.txt");
                    let mut model = EditorModel::new(Buffer::new("one\ntwo\n"), Some(path));
                    model.set_selections(vec![ResolvedSelection {
                        range: 1..1,
                        reversed: false,
                    }]);
                    let theme = EditorTheme::from_palette(&palette, &EditorSettings::default(), cx);
                    EditorView::new(model, theme, cx)
                })
            })
            .expect("open a window");
        let draw = |cx: &mut HeadlessAppContext| {
            cx.run_until_parked();
            cx.update_window(window.into(), |_, window, cx| {
                window.draw(cx).clear(cx);
                window.render_to_image().expect("record the scene");
            })
            .expect("draw");
        };
        draw(&mut cx);
        let content = HoverContent {
            text: hover.into(),
            range: None,
        };
        let view = cx.update(|cx| window.entity(cx)).expect("the editor");
        cx.update(|cx| view.update(cx, |view, cx| view.show_popover(content, cx)));
        draw(&mut cx);
        let scale = cx
            .update_window(window.into(), |_, window, _| window.scale_factor())
            .expect("the scale");
        let theme = cx.update(|cx| view.read(cx).theme().clone());
        let mut painted = kept.take();
        painted.scale = scale;
        (painted, theme)
    }
}
