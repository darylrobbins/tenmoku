// SPDX-License-Identifier: LicenseRef-Proprietary

//! A tab's title colour: an item's `tab_title_color` replaces the theme's
//! tab text colour, active or not, with the dirty dot unchanged, and the
//! default keeps the theme's. Read from the tabs the pane builds, and from
//! the glyphs of the frame it paints.

mod support;

use gpui::{
    AppContext as _, Context, Entity, FocusHandle, Focusable, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, Styled as _, TestAppContext,
    VisualTestContext, Window, div, hsla,
};
use kiln_shell::{ItemHandle, ItemPane, PaneItem, PaneView as _};
use kiln_ui::ActiveTheme as _;
use support::note::{Note, note};

struct Host(Entity<ItemPane>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().child(self.0.clone())
    }
}

/// An item that keeps `PaneItem`'s default `tab_title_color`.
struct Plain(FocusHandle);

impl Render for Plain {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.0).size_full()
    }
}

impl Focusable for Plain {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.0.clone()
    }
}

impl PaneItem for Plain {
    const KIND: &'static str = "test.plain";

    fn tab_title(&self, _: &gpui::App) -> SharedString {
        "plain".into()
    }
}

/// Each tab's title colour and dirty flag, as the pane draws them.
fn drawn(pane: &Entity<ItemPane>, cx: &mut VisualTestContext) -> Vec<(Hsla, bool)> {
    cx.update(|_, cx| {
        let tabs = pane.read(cx).pane().tabs(cx);
        tabs.iter()
            .map(|tab| (tab.title_color(cx), tab.is_dirty()))
            .collect()
    })
}

/// How many dirty-dot quads were painted in the last frame inside each
/// tab's bounds, and in `colour`.
fn dots(pane: &Entity<ItemPane>, colour: Hsla, cx: &mut VisualTestContext) -> Vec<usize> {
    cx.update(|window, cx| {
        let scale = window.scale_factor();
        let quads = window.painted_quads();
        let n = pane.read(cx).pane().items().len();
        (0..n)
            .map(|ix| {
                let tab = pane.read(cx).pane().tab_bounds(ix).expect("drawn");
                let tab = tab.scale(scale);
                quads
                    .iter()
                    .filter(|quad| {
                        quad.background == colour.into() && tab.contains(&quad.bounds.center())
                    })
                    .count()
            })
            .collect()
    })
}

// Checks the tabs `PaneState` builds for drawing, and the painted dirty dots;
// the colour the glyphs are painted in is checked below.
#[gpui::test]
fn a_tinted_title_is_drawn_in_its_tint_active_or_not(cx: &mut TestAppContext) {
    let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(ItemPane::new)));
    let pane = host.read_with(cx, |host, _| host.0.clone());
    let tint = hsla(0.3, 0.6, 0.5, 1.);
    let [a, b] = ["a", "b"].map(|title| note(title, cx));
    let plain = cx.new(|cx| Plain(cx.focus_handle()));
    b.update(cx, |note: &mut Note, _| {
        note.tint = Some(tint);
        note.dirty = true;
    });
    let items: [Box<dyn ItemHandle>; 3] = [Box::new(a), Box::new(b), Box::new(plain)];
    for item in items {
        pane.update(cx, |pane, cx| pane.add_item(item, cx));
    }
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let (active_text, text, dirty) = cx.update(|_, cx| {
        let colors = &cx.theme().colors;
        (
            colors.tab.active_text,
            colors.text.primary,
            colors.state.dirty,
        )
    });

    // `plain`, with the default, is active; `b` is tinted, dirty, inactive.
    assert_eq!(
        drawn(&pane, cx),
        [(text, false), (tint, true), (active_text, false)]
    );

    // The dirty dot is painted, in the theme's colour and not the tint.
    assert_eq!(dots(&pane, dirty, cx), [0, 1, 0]);
    assert_eq!(dots(&pane, tint, cx), [0, 0, 0]);

    pane.update(cx, |pane, _| pane.pane_mut().set_active(1));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(
        drawn(&pane, cx),
        [(text, false), (tint, true), (text, false)],
        "the tint is the same active, and the dirty dot stays"
    );
    assert_eq!(dots(&pane, dirty, cx), [0, 1, 0]);
    assert_eq!(dots(&pane, tint, cx), [0, 0, 0]);
}

/// Paints the pane and reads back the colour of each glyph. On Linux the
/// glyphs come from the platform's real text system; gpui creates the real
/// Mac platform only on the main thread, so elsewhere a stand-in gives every
/// glyph a box to paint, with no platform.
mod real_glyphs {
    use std::{borrow::Cow, cell::RefCell, rc::Rc, sync::Arc};

    use gpui::{
        AppContext as _, AtlasKey, AtlasTextureId, AtlasTextureKind, AtlasTile, Bounds,
        DevicePixels, HeadlessAppContext, Hsla, PlatformAtlas, PlatformHeadlessRenderer, Scene,
        Size, TileId, px, size,
    };
    use kiln_platform::screenshots::RgbaImage;
    use kiln_shell::{ItemHandle, ItemPane, PaneView as _};

    #[cfg(not(target_os = "linux"))]
    use gpui::{NoopTextSystem, PlatformTextSystem};

    use super::{Host, Note, Plain};

    /// The colour of each glyph the last frame painted, with its bounds.
    type Glyphs = Rc<RefCell<Vec<(Hsla, Bounds<gpui::ScaledPixels>)>>>;

    /// A renderer that draws nothing and keeps the glyphs of each scene it's
    /// given, so a test can read the colours the text was painted in on any
    /// platform, with no GPU. Its atlas hands out a tile per glyph.
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
                .map(|sprite| (sprite.color, sprite.bounds))
                .chain(
                    scene
                        .subpixel_sprites
                        .iter()
                        .map(|sprite| (sprite.color, sprite.bounds)),
                )
                .collect();
            Ok(())
        }

        fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
            self.1.clone()
        }
    }

    #[cfg(target_os = "linux")]
    fn text_system() -> Arc<dyn gpui::PlatformTextSystem> {
        gpui_platform::current_platform(true).text_system()
    }

    /// The test platform's text system, which lays text out but paints no
    /// glyph, with a box rasterized for each glyph.
    #[cfg(not(target_os = "linux"))]
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
                    size: gpui::size(DevicePixels(8), DevicePixels(8)),
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

    /// Draws a pane of a tinted note and a `Plain` item, `active` the active tab,
    /// and returns each tab's glyph colours, in its bounds, as painted.
    pub(super) fn painted(active: usize, tint: Hsla) -> [Vec<Hsla>; 2] {
        let glyphs = Glyphs::default();
        let recorder = glyphs.clone();
        let text_system = text_system();
        let mut cx = HeadlessAppContext::with_platform(text_system, Arc::new(()), move || {
            let renderer = Recorder(recorder.clone(), Arc::default());
            Ok(Some(Box::new(renderer) as Box<dyn PlatformHeadlessRenderer>))
        });
        let window = cx
            .open_window(size(px(480.), px(160.)), |_, cx| {
                let pane = cx.new(ItemPane::new);
                cx.new(|_| Host(pane))
            })
            .expect("open a window");
        let pane = window
            .read_with(&cx, |host, _| host.0.clone())
            .expect("host");
        let tinted = cx.update(|cx| {
            cx.new(|cx| Note {
                document: Default::default(),
                title: "tinted".into(),
                dirty: false,
                tint: Some(tint),
                focus: cx.focus_handle(),
            })
        });
        // `Plain` keeps the trait's default `tab_title_color`.
        let plain = cx.update(|cx| cx.new(|cx| Plain(cx.focus_handle())));
        let items: [Box<dyn ItemHandle>; 2] = [Box::new(tinted), Box::new(plain)];
        for item in items {
            cx.update(|cx| pane.update(cx, |pane, cx| pane.add_item(item, cx)));
        }
        cx.update(|cx| pane.update(cx, |pane, _| pane.pane_mut().set_active(active)));
        cx.run_until_parked();
        let bounds = cx
            .update_window(window.into(), |_, window, cx| {
                window.draw(cx).clear(cx);
                // Hands the frame's scene to the recorder.
                window.render_to_image().expect("record the scene");
                let scale = window.scale_factor();
                [0, 1].map(|ix| {
                    pane.read(cx)
                        .pane()
                        .tab_bounds(ix)
                        .expect("drawn")
                        .scale(scale)
                })
            })
            .expect("draw");
        let glyphs = glyphs.borrow();
        bounds.map(|tab| {
            (glyphs.iter())
                .filter(|(_, glyph)| tab.contains(&glyph.center()))
                .map(|(colour, _)| *colour)
                .collect()
        })
    }
}

// Reads the colour each title is drawn in, active or not, tinted or default;
// on Linux also from the glyphs the frame paints, so the colour is checked
// past the tab that carries it. The × is muted.
#[gpui::test]
fn a_title_is_painted_in_its_colour_active_or_not(cx: &mut TestAppContext) {
    let tint = hsla(0.3, 0.6, 0.5, 1.);
    let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(ItemPane::new)));
    let pane = host.read_with(cx, |host, _| host.0.clone());
    let tinted = note("tinted", cx);
    tinted.update(cx, |note: &mut Note, _| note.tint = Some(tint));
    let plain_item = cx.new(|cx| Plain(cx.focus_handle()));
    let items: [Box<dyn ItemHandle>; 2] = [Box::new(tinted), Box::new(plain_item)];
    for item in items {
        pane.update(cx, |pane, cx| pane.add_item(item, cx));
    }
    let (active_text, text) = cx.update(|_, cx| {
        let colors = &cx.theme().colors;
        (colors.tab.active_text, colors.text.primary)
    });
    assert_ne!(active_text, text);
    // The tinted tab (0) is active, then the plain one (1).
    for (active, plain) in [(0, text), (1, active_text)] {
        pane.update(cx, |pane, _| pane.pane_mut().set_active(active));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let colours: Vec<Hsla> = drawn(&pane, cx).into_iter().map(|(c, _)| c).collect();
        assert_eq!(colours, [tint, plain]);
    }

    let muted = cx.update(|_, cx| cx.theme().colors.text.muted);
    for (active, plain) in [(0, text), (1, active_text)] {
        let [tinted_tab, plain_tab] = real_glyphs::painted(active, tint);
        let title = |glyphs: Vec<Hsla>| {
            glyphs
                .into_iter()
                .filter(|c| *c != muted)
                .collect::<Vec<_>>()
        };
        let (tinted_tab, plain_tab) = (title(tinted_tab), title(plain_tab));
        assert!(
            !tinted_tab.is_empty() && tinted_tab.iter().all(|c| *c == tint),
            "{tinted_tab:?}"
        );
        assert!(
            !plain_tab.is_empty() && plain_tab.iter().all(|c| *c == plain),
            "{plain_tab:?}"
        );
    }
}
