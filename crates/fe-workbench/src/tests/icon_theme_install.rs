//! `workbench.icon_theme` chooses the installed icon set: a window opened on
//! the default settings draws a `.rs` file's tree row as a coloured image, a
//! reload to `minimal` as Lucide's `svg()`, and one back as an image again.
//! Read off the sprites a recording renderer is handed each frame.

use std::{
    borrow::Cow,
    cell::RefCell,
    collections::HashMap,
    fs,
    path::Path,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
};

use fe_config::SettingsStore;
use fe_gpui::{EditorSettings, EditorTheme};
use fe_ui::{Assets, IconSet as _, LucideIcons};
use gpui::{
    AnyWindowHandle, AppContext as _, AtlasKey, AtlasTextureId, AtlasTextureKind, AtlasTile,
    Bounds, DevicePixels, HeadlessAppContext, NoopTextSystem, PlatformAtlas,
    PlatformHeadlessRenderer, Scene, SharedString, Size, TileId, px, size,
};
use kiln_platform::{
    config::{ConfigService, batch_channel},
    screenshots::RgbaImage,
};

use crate::{Launch, Workbench};

/// What a frame painted: each `svg()`'s asset path, and how many images.
#[derive(Clone, Debug, Default, PartialEq)]
struct Painted {
    svgs: Vec<SharedString>,
    images: usize,
}

/// An atlas that hands out a tile per key and keeps which were `svg()`s, by
/// path, and which images.
#[derive(Default)]
struct Tiles {
    next: AtomicU32,
    given: Mutex<HashMap<u32, Option<SharedString>>>,
}

impl PlatformAtlas for Tiles {
    fn get_or_insert_with<'a>(
        &self,
        key: AtlasKey,
        build: &mut dyn FnMut() -> gpui::Result<Option<(Size<DevicePixels>, Cow<'a, [u8]>)>>,
    ) -> gpui::Result<Option<AtlasTile>> {
        let Some((size, _)) = build()? else {
            return Ok(None);
        };
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let given = match key {
            AtlasKey::Svg(params) => Some(Some(params.path)),
            AtlasKey::Image(_) => Some(None),
            AtlasKey::Glyph(_) => None,
        };
        if let Some(given) = given {
            self.given.lock().expect("tiles").insert(id, given);
        }
        Ok(Some(AtlasTile {
            texture_id: AtlasTextureId {
                index: 0,
                kind: AtlasTextureKind::Polychrome,
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

/// A renderer that draws nothing and keeps what each scene painted.
struct Recorder(Rc<RefCell<Painted>>, Arc<Tiles>);

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
        let given = self.1.given.lock().expect("tiles");
        let svgs = (scene.monochrome_sprites.iter())
            .filter_map(|sprite| given.get(&sprite.tile.tile_id.0)?.clone())
            .collect();
        let images = (scene.polychrome_sprites.iter())
            .filter(|sprite| matches!(given.get(&sprite.tile.tile_id.0), Some(None)))
            .count();
        *self.0.borrow_mut() = Painted { svgs, images };
        Ok(())
    }

    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.1.clone()
    }
}

/// Lets background work land, drawing frames as it does, and returns what a
/// last frame painted.
fn paint(
    cx: &mut HeadlessAppContext,
    window: AnyWindowHandle,
    painted: &RefCell<Painted>,
) -> Painted {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))
            .expect("the window");
    }
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
        // Hands the frame's scene to the recorder.
        window.render_to_image().expect("record the scene");
    })
    .expect("the window");
    painted.take()
}

/// How many of the frame's `svg()`s are `path`.
fn svgs_of(painted: &Painted, path: &str) -> usize {
    painted.svgs.iter().filter(|svg| *svg == path).count()
}

#[test]
fn reload_switches_set() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("demo");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
    let user = dir.path().join("settings.toml");
    fs::write(&user, "").unwrap();

    let painted = Rc::new(RefCell::new(Painted::default()));
    let recorder = painted.clone();
    let text_system = Arc::new(NoopTextSystem::new());
    let mut cx = HeadlessAppContext::with_platform(text_system, Arc::new(Assets), move || {
        let renderer = Recorder(recorder.clone(), Arc::default());
        Ok(Some(Box::new(renderer) as Box<dyn PlatformHeadlessRenderer>))
    });
    let service = SettingsStore::load(Some(user.clone()), None).into_inner();
    // Nothing changes the service's file: the reloads below are the host's.
    let (_, batches) = batch_channel();
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
        (ConfigService::new().settings(service))
            .start_with(batches, cx)
            .detach();
    });
    let launch = Launch {
        root: Some(root.clone()),
        file: None,
    };
    let window = (cx.open_window(size(px(800.), px(600.)), |window, cx| {
        cx.new(|cx| {
            let theme = EditorTheme::default_dark(cx);
            Workbench::new(launch, theme, EditorSettings::default(), window, cx)
        })
    }))
    .expect("open a window");
    let workbench = window.root(&mut cx).expect("the workbench");

    let file_code = LucideIcons.file("main.rs").path();
    let mut store = SettingsStore::load(Some(user.clone()), None);
    let mut reload = |theme: &str, cx: &mut HeadlessAppContext| {
        write_theme(&user, theme);
        let event = store.reload(std::slice::from_ref(&user)).expect("changed");
        workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
    };
    let window = window.into();

    let by_default = paint(&mut cx, window, &painted);
    assert!(by_default.images > 0, "{by_default:?}");
    assert_eq!(svgs_of(&by_default, &file_code), 0, "{by_default:?}");

    reload("minimal", &mut cx);
    let minimal = paint(&mut cx, window, &painted);
    assert_eq!(minimal.images, 0, "{minimal:?}");
    assert_eq!(svgs_of(&minimal, &file_code), 1, "{minimal:?}");

    reload("tenmoku", &mut cx);
    assert_eq!(paint(&mut cx, window, &painted), by_default);
}

/// Writes `workbench.icon_theme = theme` to the user's settings file.
fn write_theme(user: &Path, theme: &str) {
    fs::write(user, format!("[workbench]\nicon_theme = \"{theme}\"\n")).unwrap();
}
