// SPDX-License-Identifier: Apache-2.0

//! Coloured icons as they're painted: a window drawn with [`Assets`] as its
//! asset source hands each frame's scene to a renderer that keeps its
//! sprites, each with the pixels its atlas tile was given.

use std::{
    borrow::Cow,
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
};

use gpui::{
    AnyElement, AnyWindowHandle, AppContext as _, AssetSource as _, AtlasKey, AtlasTextureId,
    AtlasTextureKind, AtlasTile, Context, DevicePixels, HeadlessAppContext, Hsla, IntoElement,
    NoopTextSystem, ParentElement as _, PlatformAtlas, PlatformHeadlessRenderer, Render, Scene,
    Size, Styled as _, TestAppContext, TileId, Window, div, px, size,
};

use crate::{
    ActiveTheme as _, Assets, Breadcrumbs, Crumb, CrumbKind, Icon, IconRef, IconSet, Tab, TabBar,
    Theme, Tokens, TreeRow, UiTheme, file_icon_ref, file_icons::rasterize_coloured, set_icon_set,
};

/// A two-colour square: red on the left half, blue on the right.
const RED_BLUE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect width="12" height="24" fill="#ff0000"/><rect x="12" width="12" height="24" fill="#0000ff"/></svg>"##;

/// A green square.
const GREEN: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect width="24" height="24" fill="#00ff00"/></svg>"##;

/// Registered with [`Assets`] by [`register`]; under no other test's
/// prefix, so their listings don't see it.
const COLOURED: &str = "kiln-ui-test/coloured/red-blue.svg";

fn register() {
    Assets::register_coloured(&[(COLOURED, RED_BLUE)]);
}

/// A masked icon [`Assets`] serves.
fn masked() -> IconRef {
    IconRef::Asset(Icon::FileCode.path().into())
}

/// Answers every file and folder with its icon.
struct Stub(IconRef);

impl IconSet for Stub {
    fn file(&self, _: &str) -> IconRef {
        self.0.clone()
    }

    fn folder(&self, _: bool) -> IconRef {
        self.0.clone()
    }
}

/// A surface that draws a file icon.
#[derive(Clone, Copy, Debug)]
enum Surface {
    Row,
    IgnoredRow,
    ErrorRow,
    Tab,
    Crumb,
}

impl Surface {
    /// Its icon's side, in logical pixels.
    fn icon_size(self) -> f32 {
        match self {
            Surface::Row | Surface::IgnoredRow | Surface::ErrorRow => 16.,
            Surface::Tab | Surface::Crumb => 14.,
        }
    }

    /// It, for `a.rs`, with the installed set's icon.
    fn element(self) -> AnyElement {
        let theme = UiTheme::default_dark();
        let icon = file_icon_ref("a.rs");
        let row = || TreeRow::new("row", "a.rs", &theme).icon(icon.clone());
        match self {
            Surface::Row => row().into_any_element(),
            Surface::IgnoredRow => row().ignored(true).into_any_element(),
            Surface::ErrorRow => row().error(true).into_any_element(),
            Surface::Tab => TabBar::new("tabs", &theme)
                .children([Tab::new("tab", "a.rs", &theme).icon(Some(icon.clone()))])
                .into_any_element(),
            Surface::Crumb => div()
                .h(px(24.))
                .flex()
                .child(Breadcrumbs::new(
                    vec![Crumb::new("a.rs", CrumbKind::File).icon(icon.clone())],
                    &theme,
                ))
                .into_any_element(),
        }
    }
}

/// A masked sprite: an `svg()`.
#[derive(Clone, Debug, PartialEq)]
struct Mask {
    color: Hsla,
    /// Its side in device pixels.
    side: f32,
}

/// A polychrome sprite: an `img()`.
#[derive(Clone, Debug)]
struct Image {
    opacity: f32,
    /// Its side in device pixels.
    side: f32,
    /// What its atlas tile was given.
    pixels: Size<DevicePixels>,
    bgra: Vec<u8>,
}

#[derive(Default, Debug)]
struct Painted {
    masks: Vec<Mask>,
    images: Vec<Image>,
}

/// An atlas that hands out a tile per key, as GPUI's do, until the key is
/// removed, and keeps what each tile was given.
#[derive(Default)]
struct Tiles {
    next: AtomicU32,
    given: Mutex<HashMap<u32, Given>>,
    /// The tiles not removed, by key.
    live: Mutex<HashMap<AtlasKey, AtlasTile>>,
}

impl Tiles {
    /// How many images' tiles aren't removed.
    fn live_images(&self) -> usize {
        let live = self.live.lock().expect("tiles");
        (live.keys())
            .filter(|key| matches!(key, AtlasKey::Image(_)))
            .count()
    }
}

/// A tile's size and pixels.
type Given = (Size<DevicePixels>, Vec<u8>);

impl PlatformAtlas for Tiles {
    fn get_or_insert_with<'a>(
        &self,
        key: AtlasKey,
        build: &mut dyn FnMut() -> gpui::Result<Option<(Size<DevicePixels>, Cow<'a, [u8]>)>>,
    ) -> gpui::Result<Option<AtlasTile>> {
        if let Some(tile) = self.live.lock().expect("tiles").get(&key) {
            return Ok(Some(*tile));
        }
        let Some((size, bytes)) = build()? else {
            return Ok(None);
        };
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        self.given
            .lock()
            .expect("tiles")
            .insert(id, (size, bytes.into_owned()));
        let tile = AtlasTile {
            texture_id: AtlasTextureId {
                index: 0,
                kind: AtlasTextureKind::Polychrome,
            },
            tile_id: TileId(id),
            padding: 0,
            bounds: gpui::Bounds {
                origin: Default::default(),
                size,
            },
        };
        self.live.lock().expect("tiles").insert(key, tile);
        Ok(Some(tile))
    }

    fn remove(&self, key: &AtlasKey) {
        self.live.lock().expect("tiles").remove(key);
    }
}

/// A renderer that draws nothing and keeps the sprites of each scene.
struct Recorder(Rc<RefCell<Painted>>, Arc<Tiles>);

impl PlatformHeadlessRenderer for Recorder {
    fn render_scene_to_image(
        &mut self,
        scene: &Scene,
        size: Size<DevicePixels>,
    ) -> gpui::Result<image::RgbaImage> {
        self.render_scene(scene, size)?;
        Ok(image::RgbaImage::new(1, 1))
    }

    fn render_scene(&mut self, scene: &Scene, _: Size<DevicePixels>) -> gpui::Result<()> {
        let given = self.1.given.lock().expect("tiles");
        let masks = (scene.monochrome_sprites.iter())
            .map(|sprite| Mask {
                color: sprite.color,
                side: sprite.bounds.size.width.0,
            })
            .collect();
        let images = (scene.polychrome_sprites.iter())
            .map(|sprite| {
                let (pixels, bgra) = given[&sprite.tile.tile_id.0].clone();
                Image {
                    opacity: sprite.opacity,
                    side: sprite.bounds.size.width.0,
                    pixels,
                    bgra,
                }
            })
            .collect();
        *self.0.borrow_mut() = Painted { masks, images };
        Ok(())
    }

    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.1.clone()
    }
}

/// Draws one surface.
struct Host(Surface);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.element())
    }
}

/// A window drawing a surface with [`Assets`] as the app's asset source.
struct Painter {
    cx: HeadlessAppContext,
    window: AnyWindowHandle,
    painted: Rc<RefCell<Painted>>,
    tiles: Arc<Tiles>,
}

impl Painter {
    /// With `icon` answered for every file.
    fn new(icon: IconRef, surface: Surface) -> Self {
        let painted = Rc::new(RefCell::new(Painted::default()));
        let recorder = painted.clone();
        let tiles = Arc::new(Tiles::default());
        let atlas = tiles.clone();
        let text_system = Arc::new(NoopTextSystem::new());
        let mut cx = HeadlessAppContext::with_platform(text_system, Arc::new(Assets), move || {
            let renderer = Recorder(recorder.clone(), atlas.clone());
            Ok(Some(Box::new(renderer) as Box<dyn PlatformHeadlessRenderer>))
        });
        cx.update(|cx| set_icon_set(Arc::new(Stub(icon)), cx));
        let window = cx
            .open_window(size(px(480.), px(120.)), |_, cx| cx.new(|_| Host(surface)))
            .expect("open a window")
            .into();
        Self {
            cx,
            window,
            painted,
            tiles,
        }
    }

    /// Draws a frame and returns what it painted.
    fn paint(&mut self) -> Painted {
        self.cx.run_until_parked();
        self.cx
            .update_window(self.window, |_, window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
                // Hands the frame's scene to the recorder.
                window.render_to_image().expect("record the scene");
            })
            .expect("draw");
        self.painted.take()
    }

    /// `logical` pixels in the window's device pixels.
    fn device(&mut self, logical: f32) -> f32 {
        self.cx
            .update_window(self.window, |_, window, _| logical * window.scale_factor())
            .expect("scale")
    }

    /// A colour of the active theme's.
    fn colour(&mut self, pick: impl Fn(&Tokens<Hsla>) -> Hsla) -> Hsla {
        self.cx.update(|cx| pick(&cx.theme().colors))
    }
}

/// Whether `bgra` has a red pixel and a blue one: drawn in its own colours,
/// not as a mask.
fn red_and_blue(bgra: &[u8]) -> bool {
    let (pixels, _) = bgra.as_chunks::<4>();
    let red = pixels
        .iter()
        .any(|&[b, _, r, a]| r > 200 && b < 50 && a > 200);
    let blue = pixels
        .iter()
        .any(|&[b, _, r, a]| b > 200 && r < 50 && a > 200);
    red && blue
}

/// Whether every opaque pixel of `bgra` is green, and there are some.
fn all_green(bgra: &[u8]) -> bool {
    let (pixels, _) = bgra.as_chunks::<4>();
    let opaque = || pixels.iter().filter(|&&[.., a]| a > 200);
    opaque().count() > 0 && opaque().all(|&[b, g, r, _]| g > 200 && r < 50 && b < 50)
}

/// The one image painted, sharp: rasterised at the side it's drawn.
fn one_image(painted: &Painted, side: f32, what: &str) -> Image {
    assert!(painted.masks.is_empty(), "{what}: no svg(): {painted:?}");
    let [image] = painted.images.as_slice() else {
        panic!("{what}: one image: {painted:?}");
    };
    assert_eq!(image.side, side, "{what}");
    let edge = DevicePixels(side as i32);
    assert_eq!(
        image.pixels,
        size(edge, edge),
        "{what}: rasterised at its side"
    );
    image.clone()
}

#[test]
fn coloured_ref_draws_as_image_on_row_tab_and_crumb() {
    register();
    for surface in [Surface::Row, Surface::Tab, Surface::Crumb] {
        let mut painter = Painter::new(IconRef::Coloured(COLOURED.into()), surface);
        let side = painter.device(surface.icon_size());
        let image = one_image(&painter.paint(), side, &format!("{surface:?}"));
        assert_eq!(image.opacity, 1., "{surface:?}");
        assert!(red_and_blue(&image.bgra), "{surface:?} in its own colours");
    }
}

/// Sharp at 1x and 2x: rasterised at exactly the device pixels drawn, in
/// its own colours.
#[gpui::test]
fn coloured_icon_rasterised_at_device_size(cx: &mut TestAppContext) {
    register();
    cx.update(|cx| {
        for side in [14, 16, 28, 32] {
            let image = rasterize_coloured(COLOURED, side, cx).expect("rasterised");
            let edge = DevicePixels(side as i32);
            assert_eq!(image.size(0), size(edge, edge), "{side}");
            assert!(red_and_blue(image.as_bytes(0).expect("frame")), "{side}");
        }
        assert!(rasterize_coloured("kiln-ui-test/coloured/missing.svg", 16, cx).is_err());
    });
}

#[test]
fn masked_asset_still_tinted() {
    type Tint = fn(&Tokens<Hsla>) -> Hsla;
    let cases: [(Surface, Tint); 5] = [
        (Surface::Row, |colors| colors.list.icon),
        // Dim, not disabled: the label alone is muted.
        (Surface::IgnoredRow, |colors| colors.list.icon),
        (Surface::ErrorRow, |colors| colors.status.error),
        (Surface::Tab, |colors| colors.list.icon),
        (Surface::Crumb, |colors| colors.list.icon),
    ];
    for (surface, tint) in cases {
        let mut painter = Painter::new(masked(), surface);
        let mask = Mask {
            color: painter.colour(tint),
            side: painter.device(surface.icon_size()),
        };
        let painted = painter.paint();
        assert!(painted.images.is_empty(), "{surface:?}: no image");
        assert_eq!(painted.masks, [mask], "{surface:?}");
    }
}

/// An ignored row reads as dim, not disabled: its coloured icon keeps its
/// full strength, as its label alone is muted.
#[test]
fn ignored_row_keeps_coloured_icon_at_full_strength() {
    register();
    let mut painter = Painter::new(IconRef::Coloured(COLOURED.into()), Surface::IgnoredRow);
    let side = painter.device(16.);
    let image = one_image(&painter.paint(), side, "ignored row");
    assert_eq!(image.opacity, 1.);
    assert!(red_and_blue(&image.bgra), "not tinted");

    let theme = UiTheme::default_dark();
    let tokens = &Theme::from_ui_theme(&theme).colors;
    let row = TreeRow::new("row", "a.rs", &theme)
        .icon(IconRef::Coloured(COLOURED.into()))
        .ignored(true);
    assert_eq!(row.colors(tokens).icon_opacity, 1.);
}

#[test]
fn registering_again_draws_the_new_file() {
    const PATH: &str = "kiln-ui-test/coloured/replaced.svg";
    Assets::register_coloured(&[(PATH, RED_BLUE)]);
    let mut painter = Painter::new(IconRef::Coloured(PATH.into()), Surface::Row);
    let side = painter.device(16.);
    let before = one_image(&painter.paint(), side, "before");
    assert!(red_and_blue(&before.bgra));
    assert!(red_and_blue(
        &one_image(&painter.paint(), side, "again").bgra
    ));

    Assets::register_coloured(&[(PATH, GREEN)]);
    let after = one_image(&painter.paint(), side, "after");
    assert!(all_green(&after.bgra), "the new file is drawn");
}

#[test]
fn registering_a_missing_icon_draws_it() {
    const PATH: &str = "kiln-ui-test/coloured/late.svg";
    let mut painter = Painter::new(IconRef::Coloured(PATH.into()), Surface::Row);
    let side = painter.device(16.);
    let painted = painter.paint();
    assert!(
        painted.images.is_empty() && painted.masks.is_empty(),
        "{painted:?}"
    );

    Assets::register_coloured(&[(PATH, RED_BLUE)]);
    let image = one_image(&painter.paint(), side, "registered");
    assert!(red_and_blue(&image.bgra));
}

#[test]
fn assets_serve_registered_and_lucide() {
    register();
    let served = Assets.load(COLOURED).expect("load").expect("registered");
    assert_eq!(*served, *RED_BLUE);
    assert!(
        Assets
            .list("kiln-ui-test/coloured/")
            .expect("list")
            .iter()
            .any(|path| path == COLOURED)
    );
    assert!(matches!(
        Assets.load("kiln-ui-test/coloured/missing.svg"),
        Ok(None)
    ));
    for icon in Icon::ALL {
        let bytes = Assets.load(icon.path()).expect("load").expect("bundled");
        assert_eq!(*bytes, *icon.svg(), "{icon:?}");
    }
}

/// A registration that changes the file drops the old image's tile, so
/// changing it back and forth doesn't fill the atlas.
#[test]
fn registering_again_drops_the_old_tile() {
    const PATH: &str = "kiln-ui-test/coloured/retired.svg";
    Assets::register_coloured(&[(PATH, RED_BLUE)]);
    let mut painter = Painter::new(IconRef::Coloured(PATH.into()), Surface::Row);
    let side = painter.device(16.);
    one_image(&painter.paint(), side, "first");
    assert_eq!(painter.tiles.live_images(), 1);
    for (round, svg) in [GREEN, RED_BLUE, GREEN, RED_BLUE].into_iter().enumerate() {
        Assets::register_coloured(&[(PATH, svg)]);
        let image = one_image(&painter.paint(), side, &format!("round {round}"));
        let colours_right = if svg == GREEN {
            all_green(&image.bgra)
        } else {
            red_and_blue(&image.bgra)
        };
        assert!(colours_right, "round {round}: the new file is drawn");
        assert_eq!(painter.tiles.live_images(), 1, "round {round}");
    }
}
