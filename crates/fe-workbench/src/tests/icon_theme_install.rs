//! `workbench.icon_theme` chooses the installed icon set: a window opened on
//! the default settings draws a `.rs` file's tree row as Lucide's `svg()` in
//! the theme's `file_icon.rust`, a reload to `minimal` in `list.icon`, one to
//! `material` as a coloured image, and one back as the tinted line again.
//! Only the app's settings choose it, never a window's settings event
//! (T1362 F1). Read off the sprites a recording renderer is handed each
//! frame.

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

use fe_config::{SettingsStore, settings::IconTheme};
use fe_gpui::{EditorSettings, EditorTheme};
use fe_ui::{Assets, IconSet as _, LucideIcons, Theme, ThemeData};
use gpui::{
    AnyWindowHandle, AppContext as _, AtlasKey, AtlasTextureId, AtlasTextureKind, AtlasTile,
    Bounds, DevicePixels, HeadlessAppContext, Hsla, NoopTextSystem, PlatformAtlas,
    PlatformHeadlessRenderer, Scene, SharedString, Size, TileId, WindowHandle, px, size,
};
use kiln_platform::{
    config::{BatchSender, ConfigService, batch_channel},
    screenshots::RgbaImage,
};

use crate::{Launch, Workbench};

/// What a frame painted: each `svg()`'s asset path and colour, and how many
/// images.
#[derive(Clone, Debug, Default, PartialEq)]
struct Painted {
    svgs: Vec<(SharedString, Hsla)>,
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
            .filter_map(|sprite| Some((given.get(&sprite.tile.tile_id.0)?.clone()?, sprite.color)))
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

/// The colours of the frame's `svg()`s of `path`.
fn svgs_of(painted: &Painted, path: &str) -> Vec<Hsla> {
    (painted.svgs.iter())
        .filter(|(svg, _)| svg == path)
        .map(|&(_, color)| color)
        .collect()
}

/// An app whose settings service reads `user`, under Yohen, which tints
/// the file icons, with a renderer that records into the returned cell.
fn app(user: &Path) -> (HeadlessAppContext, Rc<RefCell<Painted>>, BatchSender, Theme) {
    let painted = Rc::new(RefCell::new(Painted::default()));
    let recorder = painted.clone();
    let text_system = Arc::new(NoopTextSystem::new());
    let mut cx = HeadlessAppContext::with_platform(text_system, Arc::new(Assets), move || {
        let renderer = Recorder(recorder.clone(), Arc::default());
        Ok(Some(Box::new(renderer) as Box<dyn PlatformHeadlessRenderer>))
    });
    let service = SettingsStore::load(Some(user.to_owned()), None).into_inner();
    let (sender, batches) = batch_channel();
    let yohen = fe_config::themes::find("yohen").expect("bundled").file();
    let theme = Theme::new(ThemeData::from_file(&yohen));
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
        fe_ui::set_theme(cx, theme.clone());
        (ConfigService::new().settings(service))
            .start_with(batches, cx)
            .detach();
    });
    (cx, painted, sender, theme)
}

/// A window on `root`.
fn open(cx: &mut HeadlessAppContext, root: &Path) -> WindowHandle<Workbench> {
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    cx.open_window(size(px(800.), px(600.)), |window, cx| {
        cx.new(|cx| {
            let theme = EditorTheme::default_dark(cx);
            Workbench::new(launch, theme, EditorSettings::default(), window, cx)
        })
    })
    .expect("open a window")
}

/// A project folder `name` in `dir` with a `main.rs`.
fn project(dir: &Path, name: &str) -> std::path::PathBuf {
    let root = dir.join(name);
    fs::create_dir(&root).unwrap();
    fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
    root
}

/// Writes `text` to the user's settings file and has the service reload it.
fn edit_user(user: &Path, text: &str, sender: &BatchSender, cx: &mut HeadlessAppContext) {
    fs::write(user, text).unwrap();
    sender.send(vec![user.to_owned()]);
    cx.run_until_parked();
}

#[test]
fn reload_switches_set() {
    let dir = tempfile::tempdir().unwrap();
    let root = project(dir.path(), "demo");
    let user = dir.path().join("settings.toml");
    fs::write(&user, "").unwrap();
    let (mut cx, painted, sender, theme) = app(&user);
    let window = open(&mut cx, &root).into();

    let file_code = LucideIcons.file("main.rs").path();
    let rust = theme.colors.file_icon.rust;
    let plain = theme.colors.list.icon;
    assert_ne!(rust, plain, "Yohen tints Rust");

    let by_default = paint(&mut cx, window, &painted);
    assert_eq!(by_default.images, 0, "{by_default:?}");
    assert_eq!(svgs_of(&by_default, &file_code), [rust], "{by_default:?}");

    edit_user(&user, &setting("minimal"), &sender, &mut cx);
    let minimal = paint(&mut cx, window, &painted);
    assert_eq!(minimal.images, 0, "{minimal:?}");
    assert_eq!(svgs_of(&minimal, &file_code), [plain], "{minimal:?}");

    edit_user(&user, &setting("material"), &sender, &mut cx);
    let material = paint(&mut cx, window, &painted);
    assert!(material.images > 0, "{material:?}");
    assert_eq!(svgs_of(&material, &file_code), [], "{material:?}");

    edit_user(&user, &setting("tenmoku"), &sender, &mut cx);
    assert_eq!(paint(&mut cx, window, &painted), by_default);
}

/// Two windows on two projects: the other project's settings, loaded afresh
/// and falling back to the default while the user's file doesn't parse,
/// leave the user's last good choice installed, and a fixed file installs
/// its choice in both.
#[test]
fn window_settings_leave_the_users_choice() {
    let dir = tempfile::tempdir().unwrap();
    let (first, second) = (project(dir.path(), "one"), project(dir.path(), "two"));
    let user = dir.path().join("settings.toml");
    fs::write(&user, setting("minimal")).unwrap();
    let (mut cx, painted, sender, theme) = app(&user);
    let one = open(&mut cx, &first);
    let two = open(&mut cx, &second);

    let file_code = LucideIcons.file("main.rs").path();
    let (rust, plain) = (theme.colors.file_icon.rust, theme.colors.list.icon);
    let drawn = |window: WindowHandle<Workbench>, cx: &mut HeadlessAppContext| {
        svgs_of(&paint(cx, window.into(), &painted), &file_code)
    };
    assert_eq!(drawn(one, &mut cx), [plain]);
    assert_eq!(drawn(two, &mut cx), [plain]);

    // The user's file stops parsing: the service keeps minimal.
    let broken = "[workbench\nicon_theme = \"minimal\"\n";
    edit_user(&user, broken, &sender, &mut cx);
    // The second window's own load of it falls back to the default, and
    // the host hands that window the change, as fastedit's `settle` does.
    let fresh = SettingsStore::load(Some(user.clone()), None)
        .settings()
        .clone();
    assert_eq!(fresh.workbench.icon_theme, IconTheme::Tenmoku);
    let mut given = (*fresh).clone();
    given.workbench.icon_theme = IconTheme::Minimal;
    let stale = fe_gpui::SettingsEvent {
        settings: fresh,
        previous: Arc::new(given),
        diagnostics: Vec::new(),
    };
    (two.update(&mut cx, |workbench, _, cx| {
        workbench.reload_settings(&stale, cx)
    }))
    .unwrap();
    assert_eq!(drawn(one, &mut cx), [plain], "the user's choice stays");
    assert_eq!(drawn(two, &mut cx), [plain], "the user's choice stays");

    // Fixed, the user's file's choice is every window's.
    edit_user(&user, &setting("tenmoku"), &sender, &mut cx);
    assert_eq!(drawn(one, &mut cx), [rust]);
    assert_eq!(drawn(two, &mut cx), [rust]);
}

/// `workbench.icon_theme = theme` as a settings file.
fn setting(theme: &str) -> String {
    format!("[workbench]\nicon_theme = \"{theme}\"\n")
}
