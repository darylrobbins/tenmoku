//! A set that answers coloured icons is drawn in its own colours on every
//! workbench surface that draws a file or folder icon itself: each draws an
//! image, never an `svg()`, and a reset to Lucide draws `svg()` again. Read
//! off the sprites a recording renderer is handed each frame, each file
//! under a name of its own with an icon of a colour of its own, so a sprite
//! tells which surface drew it.

use std::{
    borrow::Cow,
    cell::RefCell,
    collections::HashMap,
    fs,
    path::Path,
    process::Command,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
};

use fe_buffer::{Buffer, Origin, ProposedChange};
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::recent::{self, Entry};
use fe_editor::Changeset;
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use fe_ui::{Assets, IconRef, IconSet, LucideIcons};
use gpui::{
    AnyWindowHandle, AppContext as _, AtlasKey, AtlasTextureId, AtlasTextureKind, AtlasTile,
    Bounds, DevicePixels, Entity, Focusable as _, HeadlessAppContext, NoopTextSystem,
    PlatformAtlas, PlatformHeadlessRenderer, Scene, SharedString, Size, TileId, px, size,
};
use kiln_platform::screenshots::RgbaImage;

use crate::{
    Launch, Workbench, pane_toolbar::ToolbarRequest, workbench::recent_picker::set_recent_file,
};

/// Where the stub's coloured icons are registered.
const PREFIX: &str = "fe-workbench-test/coloured/";

/// An icon each surface is checked for: a file by its name, or a closed
/// folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Key {
    File(&'static str),
    Folder,
    /// Any other file, as a tab or crumb of no surface checked.
    Other,
}

/// Each icon's colour, red, green and blue, and the files, each but
/// `Dockerfile` under `d/` so the project tree, that folder closed, shows
/// only `Dockerfile` and `d`.
const KEYS: [(Key, [u8; 3]); 10] = [
    (Key::File("changed.rs"), [255, 0, 0]),
    (Key::File("graphed.md"), [0, 255, 0]),
    (Key::File("diffed.json"), [0, 0, 255]),
    (Key::File("ai.sh"), [255, 255, 0]),
    (Key::File("recent.csv"), [255, 0, 255]),
    (Key::File("picked.zip"), [0, 255, 255]),
    (Key::File("open.pem"), [255, 128, 0]),
    (Key::File("Dockerfile"), [0, 128, 255]),
    (Key::Folder, [128, 0, 255]),
    (Key::Other, [128, 128, 128]),
];

impl Key {
    fn of_file(name: &str) -> Key {
        (KEYS.iter())
            .find_map(|&(key, _)| matches!(key, Key::File(file) if file == name).then_some(key))
            .unwrap_or(Key::Other)
    }

    fn colour(self) -> [u8; 3] {
        KEYS.iter().find(|(key, _)| *key == self).expect("keyed").1
    }

    /// Its coloured icon's asset path.
    fn coloured(self) -> SharedString {
        let [r, g, b] = self.colour();
        format!("{PREFIX}{r:02x}{g:02x}{b:02x}.svg").into()
    }

    /// Lucide's icon for it.
    fn lucide(self) -> SharedString {
        match self {
            Key::File(name) => LucideIcons.file(name).path(),
            Key::Folder => LucideIcons.folder(false).path(),
            Key::Other => LucideIcons.file("").path(),
        }
    }
}

/// Registers a solid square of each key's colour.
fn register() {
    let icons: Vec<_> = (KEYS.iter())
        .map(|&(key, [r, g, b])| {
            let svg = format!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect width="24" height="24" fill="#{r:02x}{g:02x}{b:02x}"/></svg>"##
            );
            let path: &'static str = key.coloured().to_string().leak();
            (path, svg.leak().as_bytes())
        })
        .collect();
    Assets::register_coloured(&icons);
}

/// Answers each file and every folder with its key's coloured icon.
struct Coloured;

impl IconSet for Coloured {
    fn file(&self, name: &str) -> IconRef {
        IconRef::Coloured(Key::of_file(name).coloured())
    }

    fn folder(&self, _: bool) -> IconRef {
        IconRef::Coloured(Key::Folder.coloured())
    }
}

/// The set installed: [`Coloured`], or Lucide by a reset.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Set {
    Coloured,
    Lucide,
}

/// What a frame painted.
#[derive(Default, Debug)]
struct Painted {
    /// Each `svg()`'s asset path.
    masks: Vec<SharedString>,
    /// Each image's key, if its pixels are all one key's colour.
    images: Vec<Option<Key>>,
}

impl Painted {
    fn images_of(&self, key: Key) -> usize {
        self.images
            .iter()
            .filter(|&&image| image == Some(key))
            .count()
    }

    fn masks_of(&self, path: &str) -> usize {
        self.masks.iter().filter(|&mask| mask == path).count()
    }
}

/// Which key's colour every opaque pixel of `bgra` is, if any.
fn key_of(bgra: &[u8]) -> Option<Key> {
    let (pixels, _) = bgra.as_chunks::<4>();
    let mut opaque = pixels.iter().filter(|&&[.., a]| a > 200).peekable();
    let &&[b, g, r, _] = opaque.peek()?;
    let near = |x: u8, y: u8| x.abs_diff(y) < 24;
    let (key, colour) = *KEYS
        .iter()
        .find(|(_, [kr, kg, kb])| near(r, *kr) && near(g, *kg) && near(b, *kb))?;
    let [kr, kg, kb] = colour;
    (opaque.all(|&[b, g, r, _]| near(r, kr) && near(g, kg) && near(b, kb))).then_some(key)
}

/// What each atlas tile is: an `svg()`'s asset path, or an image's pixels.
enum Given {
    Svg(SharedString),
    Image(Vec<u8>),
    Glyph,
}

/// An atlas that hands out a tile per key and keeps what each was given.
#[derive(Default)]
struct Tiles {
    next: AtomicU32,
    given: Mutex<HashMap<u32, Given>>,
}

impl PlatformAtlas for Tiles {
    fn get_or_insert_with<'a>(
        &self,
        key: AtlasKey,
        build: &mut dyn FnMut() -> gpui::Result<Option<(Size<DevicePixels>, Cow<'a, [u8]>)>>,
    ) -> gpui::Result<Option<AtlasTile>> {
        let Some((size, bytes)) = build()? else {
            return Ok(None);
        };
        let given = match key {
            AtlasKey::Svg(params) => Given::Svg(params.path),
            AtlasKey::Image(_) => Given::Image(bytes.into_owned()),
            AtlasKey::Glyph(_) => Given::Glyph,
        };
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        self.given.lock().expect("tiles").insert(id, given);
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
        let masks = (scene.monochrome_sprites.iter())
            .filter_map(|sprite| match given.get(&sprite.tile.tile_id.0) {
                Some(Given::Svg(path)) => Some(path.clone()),
                _ => None,
            })
            .collect();
        let images = (scene.polychrome_sprites.iter())
            .filter_map(|sprite| match given.get(&sprite.tile.tile_id.0) {
                Some(Given::Image(bgra)) => Some(key_of(bgra)),
                _ => None,
            })
            .collect();
        *self.0.borrow_mut() = Painted { masks, images };
        Ok(())
    }

    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.1.clone()
    }
}

/// Runs `git` in `dir`, with the developer's own config shut out.
fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write(root: &Path, file: &str) {
    let path = root.join("d").join(file);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, format!("{file}\n")).unwrap();
}

fn commit(root: &Path, summary: &str) {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", summary]);
}

/// A repository of three commits, newest first: `graphed.md` added; the
/// rest added, `Dockerfile` at the root; `diffed.json` added. `changed.rs`
/// is modified since.
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    write(root, "diffed.json");
    commit(root, "diffed");
    for file in [
        "changed.rs",
        "ai.sh",
        "recent.csv",
        "picked.zip",
        "open.pem",
        "sub/inner.txt",
    ] {
        write(root, file);
    }
    fs::write(root.join("Dockerfile"), "FROM scratch\n").unwrap();
    commit(root, "rest");
    write(root, "graphed.md");
    commit(root, "graphed");
    fs::write(root.join("d/changed.rs"), "changed\n").unwrap();
    dir
}

/// A workbench in a window whose frames a [`Recorder`] keeps.
struct Bench {
    // Dropped before the app shuts down, as fields drop in order.
    workbench: Entity<Workbench>,
    cx: HeadlessAppContext,
    window: AnyWindowHandle,
    painted: Rc<RefCell<Painted>>,
}

impl Bench {
    fn new(root: &Path, recent_file: &Path) -> Self {
        let painted = Rc::new(RefCell::new(Painted::default()));
        let recorder = painted.clone();
        let text_system = Arc::new(NoopTextSystem::new());
        let mut cx = HeadlessAppContext::with_platform(text_system, Arc::new(Assets), move || {
            let renderer = Recorder(recorder.clone(), Arc::default());
            Ok(Some(Box::new(renderer) as Box<dyn PlatformHeadlessRenderer>))
        });
        cx.update(|cx| {
            fe_gpui::init(cx);
            crate::init(cx);
            set_recent_file(cx, recent_file.to_owned());
        });
        let launch = Launch {
            root: Some(root.to_owned()),
            file: None,
        };
        let window = cx
            .open_window(size(px(1200.), px(800.)), |window, cx| {
                cx.new(|cx| {
                    let theme = EditorTheme::default_dark(cx);
                    let mut workbench =
                        Workbench::new(launch, theme, EditorSettings::default(), window, cx);
                    workbench.set_preview_tabs(false);
                    workbench
                })
            })
            .expect("open a window");
        let workbench = window.root(&mut cx).expect("the workbench");
        let mut bench = Self {
            cx,
            window: window.into(),
            workbench,
            painted,
        };
        bench.update(|workbench, window, cx| {
            window.activate_window();
            window.focus(&workbench.focus_handle(cx), cx);
        });
        bench.draw();
        bench
    }

    fn update<R>(
        &mut self,
        f: impl FnOnce(&Entity<Workbench>, &mut gpui::Window, &mut gpui::App) -> R,
    ) -> R {
        let workbench = self.workbench.clone();
        (self.cx)
            .update_window(self.window, |_, window, cx| f(&workbench, window, cx))
            .expect("the window")
    }

    /// Lets background work land, drawing frames as it does.
    fn draw(&mut self) {
        for _ in 0..3 {
            self.cx.run_until_parked();
            self.update(|_, window, cx| window.draw(cx).clear(cx));
        }
        self.cx.run_until_parked();
    }

    /// Draws a frame and returns what it painted.
    fn paint(&mut self) -> Painted {
        self.draw();
        self.update(|_, window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
            // Hands the frame's scene to the recorder.
            window.render_to_image().expect("record the scene");
        });
        self.painted.take()
    }

    fn install(&mut self, set: Set) {
        self.cx.update(|cx| match set {
            Set::Coloured => fe_ui::set_icon_set(Arc::new(Coloured), cx),
            Set::Lucide => fe_ui::reset_icon_set(cx),
        });
    }

    fn run(&mut self, id: &str) {
        let context = ContextStack::new().with(ContextFrame::new("workbench"));
        let run = RunCommand(Invocation::of(id).unwrap().context(context));
        self.update(|_, window, cx| window.dispatch_action(Box::new(run), cx));
        self.draw();
    }
}

/// Shows a surface on the workbench rooted at the path.
type Show = Box<dyn Fn(&mut Bench, &Path)>;

/// Hides a surface.
type Hide = Box<dyn Fn(&mut Bench)>;

/// A surface: what shows it, the icons it draws, each with how many, and
/// what hides it, if it's to go before the next is shown.
struct Surface {
    name: &'static str,
    show: Show,
    icons: &'static [(Key, usize)],
    hide: Option<Hide>,
}

fn surfaces() -> Vec<Surface> {
    vec![
        Surface {
            name: "Changes panel",
            show: Box::new(|bench, _| bench.run("git.show_changes")),
            icons: &[(Key::File("changed.rs"), 1)],
            hide: None,
        },
        Surface {
            name: "Graph file rows",
            show: Box::new(|bench, _| {
                bench.run("git.log");
                bench.update(|workbench, window, cx| {
                    let panel = (workbench.read(cx).dock().read(cx).git_log().cloned())
                        .expect("the Graph was made");
                    // The newest commit, `graphed`.
                    panel.update(cx, |panel, cx| panel.open_row(0, window, cx));
                });
                bench.draw();
            }),
            icons: &[(Key::File("graphed.md"), 1)],
            hide: None,
        },
        Surface {
            name: "commit diff headers",
            show: Box::new(|bench, _| {
                bench.update(|workbench, window, cx| {
                    let panel = (workbench.read(cx).dock().read(cx).git_log().cloned())
                        .expect("the Graph was made");
                    // The oldest commit, `diffed`.
                    panel.update(cx, |panel, cx| panel.open_commit_diff(2, window, cx));
                });
                bench.draw();
            }),
            icons: &[(Key::File("diffed.json"), 1)],
            hide: None,
        },
        Surface {
            name: "AI Changes panel",
            show: Box::new(|bench, root| {
                let ai = Origin::Ai {
                    agent: "claude".into(),
                    session: "s".into(),
                };
                let buffer = Buffer::new("ai.sh\n");
                let snapshot = buffer.snapshot();
                let proposal =
                    ProposedChange::from_text(&snapshot, 0..snapshot.len(), "ai\n", ai.clone());
                let mut changeset = Changeset::new("turn", ai);
                changeset.add_file(
                    root.join("d/ai.sh"),
                    buffer.id(),
                    Rc::new(RefCell::new(proposal)),
                );
                bench.update(|workbench, window, cx| {
                    workbench.update(cx, |workbench, cx| {
                        workbench.set_changeset(changeset, window, cx)
                    })
                });
                bench.draw();
            }),
            icons: &[(Key::File("ai.sh"), 1)],
            hide: None,
        },
        Surface {
            name: "pane crumb",
            show: Box::new(|bench, root| {
                let path = root.join("d/open.pem");
                bench.update(|workbench, window, cx| {
                    let pane = workbench.read(cx).pane().clone();
                    pane.update(cx, |pane, cx| pane.open(path, window, cx));
                });
                bench.draw();
            }),
            // Its tab's, and its crumb's.
            icons: &[(Key::File("open.pem"), 2)],
            hide: None,
        },
        Surface {
            name: "toolbar picker",
            show: Box::new(|bench, root| {
                let dir = root.join("d");
                bench.update(|workbench, _, cx| {
                    let pane = workbench.read(cx).pane().clone();
                    pane.update(cx, |_, cx| {
                        cx.emit(ToolbarRequest::Entries { dir, select: None })
                    });
                });
                bench.draw();
            }),
            icons: &[(Key::File("picked.zip"), 1), (Key::Folder, 1)],
            // Closed, as it lists `recent.csv` too.
            hide: Some(Box::new(|bench| {
                bench.update(|workbench, window, cx| {
                    let picker = workbench.read(cx).toolbar_picker().cloned();
                    let picker = picker.expect("the toolbar picker shows");
                    picker.update(cx, |picker, cx| picker.dismiss(window, cx));
                });
                bench.draw();
            })),
        },
        Surface {
            name: "recent picker",
            show: Box::new(|bench, _| bench.run("workbench.open_recent")),
            // `recent.csv`, and the folder `d/sub`.
            icons: &[(Key::File("recent.csv"), 1), (Key::Folder, 1)],
            hide: Some(Box::new(|bench| {
                bench.update(|workbench, window, cx| {
                    let picker = workbench.read(cx).recent_picker().clone();
                    picker.update(cx, |picker, cx| picker.dismiss(window, cx));
                });
                bench.draw();
            })),
        },
        Surface {
            // Hidden since the Changes panel took the dock.
            name: "project tree",
            show: Box::new(|bench, _| bench.run("dock.show_project")),
            icons: &[(Key::File("Dockerfile"), 1), (Key::Folder, 1)],
            hide: None,
        },
    ]
}

/// Shows each surface in turn and, with each of `sets` installed while it's
/// on screen, checks what it added to the frame with `check`: the surface,
/// the set, and what was painted before it was shown and after.
fn every_surface(sets: &[Set], check: impl Fn(&Surface, Set, &Painted, &Painted)) {
    register();
    let dir = fixture();
    let root = dunce::canonicalize(dir.path()).unwrap();
    let state = tempfile::tempdir().unwrap();
    let recent_file = state.path().join("recent.toml");
    recent::record(&recent_file, Entry::Folder(&root.join("d/sub"))).unwrap();
    recent::record(&recent_file, Entry::File(&root.join("d/recent.csv"))).unwrap();
    let mut bench = Bench::new(&root, &recent_file);
    for surface in surfaces() {
        let before: Vec<_> = (sets.iter())
            .map(|&set| {
                bench.install(set);
                bench.paint()
            })
            .collect();
        (surface.show)(&mut bench, &root);
        for (&set, before) in sets.iter().zip(&before) {
            bench.install(set);
            check(&surface, set, before, &bench.paint());
        }
        if let Some(hide) = &surface.hide {
            hide(&mut bench);
        }
    }
    bench.cx.update(fe_ui::reset_icon_set);
}

/// No `svg()` draws a coloured icon's path.
#[track_caller]
fn no_coloured_masks(painted: &Painted, what: &str) {
    let masked: Vec<_> = (painted.masks.iter())
        .filter(|path| path.starts_with(PREFIX))
        .collect();
    assert!(masked.is_empty(), "{what}: svg() of coloured {masked:?}");
}

#[test]
fn every_surface_draws_coloured_ref_as_image() {
    every_surface(&[Set::Coloured], |surface, _, before, after| {
        no_coloured_masks(after, surface.name);
        for &(key, count) in surface.icons {
            let added = after.images_of(key).saturating_sub(before.images_of(key));
            assert!(
                added >= count,
                "{}: {count} image(s) of {key:?}, {added} drawn: {after:?}",
                surface.name
            );
            let lucide = key.lucide();
            assert_eq!(
                after.masks_of(&lucide),
                before.masks_of(&lucide),
                "{}: no svg() of {key:?}",
                surface.name
            );
        }
    });
}

#[test]
fn lucide_reset_redraws_svg() {
    every_surface(
        &[Set::Coloured, Set::Lucide],
        |surface, set, before, after| {
            if set == Set::Coloured {
                return;
            }
            let ours = (after.images.iter())
                .filter(|image| image.is_some())
                .count();
            assert_eq!(ours, 0, "{}: no coloured image after a reset", surface.name);
            no_coloured_masks(after, surface.name);
            for &(key, count) in surface.icons {
                let lucide = key.lucide();
                let added = after
                    .masks_of(&lucide)
                    .saturating_sub(before.masks_of(&lucide));
                assert!(
                    added >= count,
                    "{}: {count} svg() of {lucide}, {added} drawn: {after:?}",
                    surface.name
                );
            }
        },
    );
}
