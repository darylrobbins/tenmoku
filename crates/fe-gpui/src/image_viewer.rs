//! [`ImageViewer`]: a tab showing an image file (png, jpg, gif, webp, bmp,
//! ico), as VS Code's Image Preview does.
//!
//! The file is read and decoded on the background executor, by GPUI's own
//! decoders, as the Markdown preview draws images; a file over
//! [`MAX_BYTES`] or [`MAX_PIXELS`] isn't decoded. The picture is centred on
//! a checkerboard (drawn only if it has transparency) over the editor's
//! background. It starts at [`Zoom::Fit`], scaled down to the viewport and
//! never up; the zoom commands, Ctrl/Cmd and the wheel, and a pinch change
//! it around a point, and a picture larger than the viewport scrolls and
//! drags. A click toggles Fit and 100%. A viewer checks its file when it
//! gets focus or its window is activated, and reloads it if it changed.
//! It's a pane item: see `pane_item.rs`.

use std::{
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

use fe_command::{Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Registry};
use fe_command::{ContextFrame, ContextStack, Invocation, Source};
use gpui::{
    App, Bounds, ContentMask, Context, CursorStyle, DevicePixels, FocusHandle, Focusable, Hsla,
    InteractiveElement as _, IntoElement, KeyContext, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement as _, ParsedSvg, PinchEvent, Pixels, Point, Render, RenderImage,
    SMOOTH_SVG_SCALE_FACTOR, ScrollWheelEvent, SharedString, Size, StatefulInteractiveElement as _,
    Styled as _, Subscription, SvgRenderer, SvgSize, Task, Window, canvas, div, fill, point, px,
    size,
};

use crate::{EditorTheme, commands::RunCommand};

pub(crate) mod pane_item;

/// A file over this many bytes isn't read.
pub(crate) const MAX_BYTES: u64 = 64 * 1024 * 1024;

/// An image of more pixels than this isn't decoded.
pub(crate) const MAX_PIXELS: u64 = 100_000_000;

/// An SVG over this many bytes isn't drawn.
pub(crate) const MAX_SVG_BYTES: u64 = 8 * 1024 * 1024;

/// An SVG is re-rendered at the displayed size once the zoom has stayed put
/// this long.
const SETTLE: Duration = Duration::from_millis(150);

/// The most pixels a side of a re-rendered SVG has.
const MAX_RENDER_SIDE: f32 = 8192.;

/// Why an SVG isn't drawn when it links a file outside the workspace.
const BLOCKED: &str = "Blocked: links a file outside the workspace";

/// `image.open_as_text`: replaces an SVG viewer tab with an editor on
/// the file. The workbench runs it.
pub(crate) const OPEN_AS_TEXT: &str = "image.open_as_text";

/// The key context of an image viewer, which its commands run in.
pub const KEY_CONTEXT: &str = "ImageViewer";

/// The zoom steps `image.zoom_in` and `image.zoom_out` go through: 10%
/// to 2000%.
const STEPS: [f32; 13] = [0.1, 0.25, 0.5, 0.75, 1., 1.5, 2., 3., 4., 8., 12., 16., 20.];
const MIN_SCALE: f32 = 0.1;
const MAX_SCALE: f32 = 20.;

/// The checkerboard's squares, in points.
const SQUARE: f32 = 8.;
/// Draw squares in twos, fours, … when a viewport would need more.
const MAX_SQUARES: f32 = 60_000.;

/// How far a drag moves before it pans instead of clicks.
const CLICK_SLOP: f32 = 3.;

/// How a picture is scaled.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Zoom {
    /// Down to the viewport, never up past 100%.
    Fit,
    /// This many points to an image pixel: 1 is 100%.
    Scale(f32),
}

/// A file as the viewer last read it, to tell whether it changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
}

fn stamp_of(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some(Stamp {
        len: meta.len(),
        modified: meta.modified().ok(),
    })
}

/// A decoded picture.
#[derive(Clone)]
struct Picture {
    image: Arc<RenderImage>,
    width: u32,
    height: u32,
    /// Some pixel isn't opaque: the checkerboard shows through.
    transparent: bool,
    file_len: u64,
    /// What an SVG is drawn from, to draw it again at another size.
    svg: Option<Svg>,
}

/// A parsed SVG and its renderer.
#[derive(Clone)]
struct Svg {
    renderer: SvgRenderer,
    parsed: Arc<ParsedSvg>,
    /// The size in points the document says it is, before rounding up to
    /// a [`Picture`]'s whole pixels.
    intrinsic: (f32, f32),
}

/// Whether `path` names an SVG, by its extension (svg or svgz, in any
/// case).
pub(crate) fn is_svg(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("svg") || ext.eq_ignore_ascii_case("svgz"))
}

/// Whether `path` names an uncompressed SVG (`.svg`, in any case): text that
/// can be edited, unlike an `.svgz`.
pub(crate) fn is_svg_source(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
}

/// What the viewer shows.
#[derive(Clone)]
enum Content {
    Loading,
    Ready(Picture),
    /// Why there's no picture: "Too large to preview", "File not found",
    /// or "Can't display this image: …".
    Message(SharedString),
}

/// What [`ImageViewer::snapshot`] holds.
pub(crate) struct Snapshot {
    path: PathBuf,
    theme: EditorTheme,
    content: Content,
    zoom: Zoom,
    scroll: Point<Pixels>,
    stamp: Option<Stamp>,
    root: Option<PathBuf>,
    /// The file was being read: what the picture is of may be stale.
    reading: bool,
}

/// The zoom a saved state holds: `"fit"`, a scale, or Fit for anything
/// else.
pub(crate) fn zoom_of(state: &fe_command::Value) -> Zoom {
    match state.get("zoom").and_then(fe_command::Value::as_f64) {
        Some(scale) if scale.is_finite() && scale > 0. => {
            Zoom::Scale((scale as f32).clamp(MIN_SCALE, MAX_SCALE))
        }
        _ => Zoom::Fit,
    }
}

/// What decoding a file came to.
enum Decoded {
    Ready(Picture),
    TooLarge,
    Missing,
    /// An SVG linking a file outside the workspace.
    Blocked,
    Failed(String),
}

/// Where the last frame put the picture, for tests.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Painted {
    /// The picture's bounds.
    pub(crate) image: Bounds<Pixels>,
    /// The checkerboard's, if one was painted.
    pub(crate) checkerboard: Option<Bounds<Pixels>>,
}

/// Where the picture is in the viewport.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Geometry {
    pub(crate) scale: f32,
    /// The picture's bounds in the window.
    pub(crate) bounds: Bounds<Pixels>,
}

/// A mouse press on the picture.
struct Press {
    at: Point<Pixels>,
    scroll: Point<Pixels>,
    panned: bool,
}

/// A viewer of an image file.
pub struct ImageViewer {
    path: PathBuf,
    pub(crate) theme: EditorTheme,
    pub(crate) focus: FocusHandle,
    content: Content,
    pub(crate) zoom: Zoom,
    /// How far the picture is scrolled, when it's larger than the viewport
    /// (both ≥ 0).
    pub(crate) scroll: Point<Pixels>,
    /// The viewport as the last frame painted it.
    pub(crate) viewport: Bounds<Pixels>,
    press: Option<Press>,
    /// The file as the content was read from it.
    stamp: Option<Stamp>,
    loading: Option<Task<()>>,
    watch: Option<Watch>,
    pub(crate) painted: Option<Painted>,
    /// The workspace root: an SVG may link files in it and in its own
    /// folder.
    root: Option<PathBuf>,
    /// An SVG waiting to be drawn again at a size the zoom needs.
    sharpen: Option<Sharpen>,
    /// Pictures replaced, for the next frame to free from the atlas.
    retired: Vec<Arc<RenderImage>>,
    /// How many times the file was read.
    #[cfg(test)]
    pub(crate) loads: usize,
}

/// An SVG's re-render, and the pixel size it is for.
struct Sharpen {
    target: (u32, u32),
    _task: Task<()>,
}

/// The checks of the file: on focus and when the window is activated.
struct Watch {
    _focus: Subscription,
    _activation: Subscription,
}

/// Zooms in to the next step, around the middle of the view.
pub(crate) const ZOOM_IN: &str = "image.zoom_in";
/// Zooms out to the previous step, around the middle of the view.
pub(crate) const ZOOM_OUT: &str = "image.zoom_out";
/// Scales the image down to the view, and no larger than its size.
pub(crate) const ZOOM_FIT: &str = "image.zoom_fit";
/// Shows the image at 100%: a pixel of it to a point of the view.
pub(crate) const ZOOM_ACTUAL: &str = "image.zoom_actual";

/// The [`CommandTarget`] of the viewer's commands, which its [`RunCommand`]
/// handler runs: registered so keymaps and the palette know them.
pub(crate) struct ImageCommands;

impl CommandTarget for ImageCommands {
    type Tx = ();
    type Target<'a> = ();
}

/// The four zoom commands, in the [`KEY_CONTEXT`] context. Their handlers
/// do nothing: the view runs them.
pub(crate) fn registry() -> Registry<ImageCommands> {
    let commands = [
        (ZOOM_IN, "Zoom In"),
        (ZOOM_OUT, "Zoom Out"),
        (ZOOM_FIT, "Fit to Window"),
        (ZOOM_ACTUAL, "Actual Size"),
    ];
    let mut registry = Registry::new();
    for (id, title) in commands {
        registry
            .register(id, title, |_: &mut (), _: &Actor| {
                Ok(Dispatched::new(Effect::Nothing))
            })
            .expect("image command IDs are valid and distinct")
            .context(ContextPredicate::identifier(KEY_CONTEXT));
    }
    registry
}

impl ImageViewer {
    /// A viewer of `path`, which starts decoding it.
    pub fn new(path: PathBuf, zoom: Zoom, theme: EditorTheme, cx: &mut Context<Self>) -> Self {
        Self::in_workspace(path, zoom, None, theme, cx)
    }

    /// [`ImageViewer::new`] for a file in the workspace at `root`, which an
    /// SVG may link files of.
    pub fn in_workspace(
        path: PathBuf,
        zoom: Zoom,
        root: Option<PathBuf>,
        theme: EditorTheme,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut viewer = Self {
            path,
            theme,
            focus: cx.focus_handle(),
            content: Content::Loading,
            zoom,
            scroll: Point::default(),
            viewport: Bounds::default(),
            press: None,
            stamp: None,
            loading: None,
            watch: None,
            painted: None,
            root,
            sharpen: None,
            retired: Vec::new(),
            #[cfg(test)]
            loads: 0,
        };
        viewer.load(cx);
        viewer
    }

    /// The workspace root an SVG may link files of.
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// The file shown.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// What a split needs of this viewer: the same file at the same zoom,
    /// over the picture already decoded.
    pub(crate) fn snapshot(&self) -> Snapshot {
        Snapshot {
            path: self.path.clone(),
            theme: self.theme.clone(),
            content: self.content.clone(),
            zoom: self.zoom,
            scroll: self.scroll,
            stamp: self.stamp,
            root: self.root.clone(),
            reading: self.loading.is_some(),
        }
    }

    /// A viewer of `snapshot`'s file in its own context, which reads the
    /// file again if the snapshot had no picture yet.
    pub(crate) fn from_snapshot(snapshot: Snapshot, cx: &mut Context<Self>) -> Self {
        let pending = snapshot.reading || matches!(snapshot.content, Content::Loading);
        let mut viewer = Self {
            path: snapshot.path,
            theme: snapshot.theme,
            focus: cx.focus_handle(),
            content: snapshot.content,
            zoom: snapshot.zoom,
            scroll: snapshot.scroll,
            viewport: Bounds::default(),
            press: None,
            stamp: snapshot.stamp,
            loading: None,
            watch: None,
            painted: None,
            root: snapshot.root,
            sharpen: None,
            retired: Vec::new(),
            #[cfg(test)]
            loads: 0,
        };
        if pending {
            viewer.load(cx);
        }
        viewer
    }

    /// Puts back the zoom a saved state holds, if it holds one.
    pub(crate) fn apply_zoom(&mut self, state: &fe_command::Value, cx: &mut Context<Self>) {
        self.zoom = zoom_of(state);
        self.scroll = Point::default();
        cx.notify();
    }

    /// Whether the file is being read.
    pub fn is_loading(&self) -> bool {
        self.loading.is_some()
    }

    /// The picture's size in pixels, once decoded.
    pub fn pixel_size(&self) -> Option<(u32, u32)> {
        self.picture()
            .map(|picture| (picture.width, picture.height))
    }

    /// The file's size in bytes, once read.
    pub fn file_len(&self) -> Option<u64> {
        self.picture().map(|picture| picture.file_len)
    }

    /// What the viewer says instead of a picture, if it does.
    pub fn message(&self) -> Option<&SharedString> {
        match &self.content {
            Content::Message(message) => Some(message),
            _ => None,
        }
    }

    pub fn zoom(&self) -> Zoom {
        self.zoom
    }

    fn picture(&self) -> Option<&Picture> {
        match &self.content {
            Content::Ready(picture) => Some(picture),
            _ => None,
        }
    }

    /// The zoom as the status bar says it: `Fit` or `150%`.
    pub fn zoom_text(&self) -> SharedString {
        match self.zoom {
            Zoom::Fit => "Fit".into(),
            Zoom::Scale(scale) => format!("{}%", (scale * 100.).round()).into(),
        }
    }

    /// Reads and decodes the file on the background executor, keeping the
    /// zoom.
    fn load(&mut self, cx: &mut Context<Self>) {
        use gpui::AppContext as _;
        let path = self.path.clone();
        // Before the read: a change since is found by the next check.
        self.stamp = stamp_of(&path);
        #[cfg(test)]
        {
            self.loads += 1;
        }
        let renderer = cx.svg_renderer();
        let root = self.root.clone();
        let decoding = cx
            .background_spawn(async move { decode_in(&path, Some((&renderer, root.as_deref()))) });
        self.sharpen = None;
        self.loading = Some(cx.spawn(async move |this, cx| {
            let decoded = decoding.await;
            this.update(cx, |viewer, cx| {
                viewer.loading = None;
                viewer.content = match decoded {
                    Decoded::Ready(picture) => Content::Ready(picture),
                    Decoded::TooLarge => Content::Message("Too large to preview".into()),
                    Decoded::Missing => Content::Message("File not found".into()),
                    Decoded::Blocked => Content::Message(BLOCKED.into()),
                    Decoded::Failed(why) => {
                        Content::Message(format!("Can't display this image: {why}").into())
                    }
                };
                viewer.clamp_scroll();
                // Changed or deleted while it was read, with focus or
                // activation coming and going meanwhile: read it again.
                viewer.check_disk(cx);
                cx.notify();
            })
            .ok();
        }));
    }

    /// The size in pixels of the picture drawn, which an SVG's differs
    /// from [`ImageViewer::pixel_size`]: it's drawn larger to stay sharp.
    pub fn rendered_size(&self) -> Option<(u32, u32)> {
        let size = self.picture()?.image.size(0);
        Some((size.width.0 as u32, size.height.0 as u32))
    }

    /// Whether the file is an uncompressed SVG, which can be shown as text.
    pub fn is_svg_source(&self) -> bool {
        is_svg_source(&self.path)
    }

    /// Draws an SVG again at the size the zoom shows it at, on a device of
    /// `scale_factor` pixels to a point, once the zoom has settled: until
    /// then the picture scales. A size no larger than the one drawn is
    /// left alone.
    fn keep_sharp(&mut self, scale: f32, scale_factor: f32, cx: &mut Context<Self>) {
        let Some(picture) = self.picture() else {
            return;
        };
        let Some(svg) = picture.svg.clone() else {
            return;
        };
        let drawn = self.rendered_size().unwrap_or_default();
        let target = rendered_size_at(svg.intrinsic, scale * scale_factor);
        // Sharp enough when the picture has as many pixels as the zoom shows,
        // both ways.
        if target.0 <= drawn.0 && target.1 <= drawn.1 {
            self.sharpen = None;
            return;
        }
        if self.sharpen.as_ref().is_some_and(|s| s.target == target) {
            return;
        }
        use gpui::AppContext as _;
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SETTLE).await;
            let parsed = svg.parsed.clone();
            let renderer = svg.renderer.clone();
            let exact = size(DevicePixels(target.0 as i32), DevicePixels(target.1 as i32));
            let drawing = cx.background_spawn(async move {
                renderer.render_parsed(&parsed, SvgSize::ExactSize(exact))
            });
            let drawn = drawing.await;
            this.update(cx, |viewer, cx| {
                viewer.sharpen = None;
                let Ok(image) = drawn else {
                    return;
                };
                // Reloaded meanwhile: it's another document now.
                let Content::Ready(picture) = &mut viewer.content else {
                    return;
                };
                if !picture
                    .svg
                    .as_ref()
                    .is_some_and(|now| Arc::ptr_eq(&now.parsed, &svg.parsed))
                {
                    return;
                }
                viewer
                    .retired
                    .push(std::mem::replace(&mut picture.image, image));
                cx.notify();
            })
            .ok();
        });
        self.sharpen = Some(Sharpen {
            target,
            _task: task,
        });
    }

    /// Reloads the file if it changed on disk since it was read, keeping
    /// the zoom.
    pub(crate) fn check_disk(&mut self, cx: &mut Context<Self>) {
        if self.loading.is_some() || stamp_of(&self.path) == self.stamp {
            return;
        }
        self.load(cx);
        cx.notify();
    }

    /// Checks the file whenever the view is focused or its window
    /// activated.
    fn watch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.watch.is_some() {
            return;
        }
        let focus = cx.on_focus(&self.focus, window, |viewer, _, cx| viewer.check_disk(cx));
        let activation = cx.observe_window_activation(window, |viewer, window, cx| {
            if window.is_window_active() {
                viewer.check_disk(cx);
            }
        });
        self.watch = Some(Watch {
            _focus: focus,
            _activation: activation,
        });
    }

    /// The scale a [`Zoom`] comes to in a viewport of `viewport`.
    fn scale_in(&self, viewport: Size<Pixels>) -> Option<f32> {
        let picture = self.picture()?;
        Some(match self.zoom {
            Zoom::Scale(scale) => scale,
            Zoom::Fit => {
                let (w, h) = (picture.width as f32, picture.height as f32);
                let (vw, vh) = (f32::from(viewport.width), f32::from(viewport.height));
                if vw <= 0. || vh <= 0. {
                    1.
                } else {
                    (vw / w).min(vh / h).min(1.)
                }
            }
        })
    }

    /// Where the picture is, in the last painted viewport.
    pub(crate) fn geometry(&self) -> Option<Geometry> {
        let picture = self.picture()?;
        let viewport = self.viewport;
        let scale = self.scale_in(viewport.size)?;
        let content = size(
            px(picture.width as f32 * scale),
            px(picture.height as f32 * scale),
        );
        let place = |content: Pixels, view: Pixels, origin: Pixels, scroll: Pixels| {
            if content <= view {
                origin + (view - content) / 2.
            } else {
                origin - scroll.clamp(px(0.), content - view)
            }
        };
        let origin = point(
            place(
                content.width,
                viewport.size.width,
                viewport.origin.x,
                self.scroll.x,
            ),
            place(
                content.height,
                viewport.size.height,
                viewport.origin.y,
                self.scroll.y,
            ),
        );
        Some(Geometry {
            scale,
            bounds: Bounds::new(origin, content),
        })
    }

    /// Keeps the scroll inside the picture.
    pub(crate) fn clamp_scroll(&mut self) {
        let Some(geometry) = self.geometry() else {
            return;
        };
        let (content, view) = (geometry.bounds.size, self.viewport.size);
        self.scroll = point(
            self.scroll
                .x
                .clamp(px(0.), (content.width - view.width).max(px(0.))),
            self.scroll
                .y
                .clamp(px(0.), (content.height - view.height).max(px(0.))),
        );
    }

    fn viewport_centre(&self) -> Point<Pixels> {
        self.viewport.center()
    }

    /// Shows the picture at `scale` (clamped to 10%–2000%) with the point
    /// of it under `anchor`, a window position, staying there.
    pub(crate) fn zoom_to(&mut self, scale: f32, anchor: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(old) = self.geometry() else {
            return;
        };
        let (width, height) = {
            let picture = self.picture().expect("a geometry has a picture");
            (picture.width as f32, picture.height as f32)
        };
        let scale = scale.clamp(MIN_SCALE, MAX_SCALE);
        // The pixel under the anchor.
        let qx = f32::from(anchor.x - old.bounds.origin.x) / old.scale;
        let qy = f32::from(anchor.y - old.bounds.origin.y) / old.scale;
        let origin = point(anchor.x - px(qx * scale), anchor.y - px(qy * scale));
        let view = self.viewport;
        self.zoom = Zoom::Scale(scale);
        self.scroll = point(
            if px(width * scale) > view.size.width {
                view.origin.x - origin.x
            } else {
                px(0.)
            },
            if px(height * scale) > view.size.height {
                view.origin.y - origin.y
            } else {
                px(0.)
            },
        );
        self.clamp_scroll();
        cx.notify();
    }

    fn fit(&mut self, cx: &mut Context<Self>) {
        self.zoom = Zoom::Fit;
        self.scroll = Point::default();
        cx.notify();
    }

    /// The next zoom step up or down from the scale shown, around the
    /// viewport's middle.
    fn step(&mut self, up: bool, cx: &mut Context<Self>) {
        let Some(scale) = self.scale_in(self.viewport.size) else {
            return;
        };
        let next = if up {
            STEPS
                .iter()
                .copied()
                .find(|step| *step > scale * 1.001)
                .unwrap_or(MAX_SCALE)
        } else if scale <= MIN_SCALE * 1.001 {
            return;
        } else {
            STEPS
                .iter()
                .rev()
                .copied()
                .find(|step| *step < scale / 1.001)
                .unwrap_or(MIN_SCALE)
        };
        let centre = self.viewport_centre();
        self.zoom_to(next, centre, cx);
    }

    /// Multiplies the scale shown by `factor`, around `anchor`.
    fn zoom_by(&mut self, factor: f32, anchor: Point<Pixels>, cx: &mut Context<Self>) {
        if let Some(scale) = self.scale_in(self.viewport.size) {
            self.zoom_to(scale * factor, anchor, cx);
        }
    }

    /// Toggles Fit and 100% for a click at `anchor`.
    pub(crate) fn toggle(&mut self, anchor: Point<Pixels>, cx: &mut Context<Self>) {
        match self.zoom {
            Zoom::Fit => self.zoom_to(1., anchor, cx),
            Zoom::Scale(_) => self.fit(cx),
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        self.press = Some(Press {
            at: event.position,
            scroll: self.scroll,
            panned: false,
        });
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        let Some(press) = &mut self.press else {
            return;
        };
        let moved = event.position - press.at;
        if !press.panned && f32::from(moved.x).abs().max(f32::from(moved.y).abs()) <= CLICK_SLOP {
            return;
        }
        press.panned = true;
        self.scroll = press.scroll - moved;
        self.clamp_scroll();
        cx.notify();
    }

    fn on_mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(press) = self.press.take() else {
            return;
        };
        let on_picture = self
            .geometry()
            .is_some_and(|geometry| geometry.bounds.contains(&event.position));
        if !press.panned && on_picture {
            self.toggle(event.position, cx);
        }
    }

    fn on_press_cancelled(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.press = None;
    }

    /// Ctrl or Cmd and the wheel zoom around the pointer; the wheel alone
    /// scrolls.
    fn on_scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(px(20.));
        if event.modifiers.secondary() {
            let factor = (-f32::from(delta.y) * 0.01).exp();
            self.zoom_by(factor, event.position, cx);
        } else {
            self.scroll -= delta;
            self.clamp_scroll();
            cx.notify();
        }
    }

    /// Runs the viewer's commands; any other goes on to the app.
    fn on_run_command(&mut self, run: &RunCommand, _: &mut Window, cx: &mut Context<Self>) {
        match run.0.command.as_str() {
            ZOOM_IN => self.step(true, cx),
            ZOOM_OUT => self.step(false, cx),
            ZOOM_FIT => self.fit(cx),
            ZOOM_ACTUAL => {
                let centre = self.viewport_centre();
                self.zoom_to(1., centre, cx);
            }
            _ => cx.propagate(),
        }
    }

    fn on_pinch(&mut self, event: &PinchEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom_by((1. + event.delta).max(0.01), event.position, cx);
    }
}

impl Focusable for ImageViewer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ImageViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.watch(window, cx);
        let theme = self.theme.clone();
        let (light, dark) = checker_colors(theme.background);
        let viewer = cx.entity();
        let picture = self.picture().map(|p| (p.image.clone(), p.transparent));
        let surface = match (&self.content, picture) {
            (_, Some((image, transparent))) => div().size_full().child(
                canvas(
                    {
                        let viewer = viewer.clone();
                        move |bounds, window, cx| {
                            let scale_factor = window.scale_factor();
                            viewer.update(cx, |viewer, cx| {
                                viewer.viewport = bounds;
                                viewer.clamp_scroll();
                                let geometry = viewer.geometry();
                                if let Some(geometry) = &geometry {
                                    viewer.keep_sharp(geometry.scale, scale_factor, cx);
                                }
                                for old in std::mem::take(&mut viewer.retired) {
                                    window.drop_image(old).ok();
                                }
                                geometry
                            })
                        }
                    },
                    move |bounds, geometry, window, cx| {
                        let Some(geometry) = geometry else {
                            return;
                        };
                        let painted =
                            window.with_content_mask(Some(ContentMask { bounds }), |window| {
                                let visible = bounds.intersect(&geometry.bounds);
                                let checkerboard =
                                    (transparent && !visible.is_empty()).then(|| {
                                        paint_checkerboard(
                                            window,
                                            geometry.bounds,
                                            visible,
                                            light,
                                            dark,
                                        );
                                        visible
                                    });
                                window
                                    .paint_image(
                                        bounds,
                                        geometry.bounds,
                                        Default::default(),
                                        image,
                                        0,
                                        false,
                                    )
                                    .ok();
                                Painted {
                                    image: geometry.bounds,
                                    checkerboard,
                                }
                            });
                        viewer.update(cx, |viewer, _| viewer.painted = Some(painted));
                    },
                )
                .size_full(),
            ),
            (Content::Message(message), None) => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme.gutter_foreground)
                .text_size(theme.font_size)
                .font_family(theme.font_family.clone())
                .child(message.clone()),
            (_, None) => div().size_full(),
        };
        let cursor = if self.press.as_ref().is_some_and(|press| press.panned) {
            CursorStyle::ClosedHand
        } else {
            CursorStyle::Arrow
        };
        let mut key_context = KeyContext::default();
        key_context.add(KEY_CONTEXT);
        if self.is_svg_source() {
            // `image.open_as_text` applies to SVGs only.
            key_context.add("svg");
        }
        let text_button = self.is_svg_source().then(|| {
            let focus = self.focus.clone();
            div()
                .id("image-viewer-open-as-text")
                .absolute()
                .top(px(8.))
                .right(px(8.))
                .px(px(8.))
                .py(px(2.))
                .rounded(px(4.))
                .cursor(CursorStyle::PointingHand)
                .bg(theme.background)
                .border_1()
                .border_color(theme.gutter_foreground)
                .text_color(theme.gutter_foreground)
                .text_size(theme.font_size)
                .font_family(theme.font_family.clone())
                .child("Text")
                // The viewer's own press would start a pan or a click.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, window, cx| {
                    window.focus(&focus, cx);
                    // Invoked in the viewer, which the command applies in.
                    let context =
                        ContextStack::new().with(ContextFrame::new(KEY_CONTEXT).flag("svg"));
                    let invocation = Invocation::of(OPEN_AS_TEXT)
                        .expect("a valid command ID")
                        .source(Source::Mouse)
                        .context(context);
                    window.dispatch_action(Box::new(RunCommand(invocation)), cx);
                })
        });
        div()
            .key_context(key_context)
            .id("image-viewer")
            .on_action(cx.listener(Self::on_run_command))
            .track_focus(&self.focus)
            .size_full()
            .overflow_hidden()
            .bg(theme.background)
            .cursor(cursor)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_press_cancelled))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .on_pinch(cx.listener(Self::on_pinch))
            .child(surface)
            .children(text_button)
    }
}

/// Paints the checkerboard over `visible`, a part of `image`: a field of
/// `light` with squares of `dark`, their grid starting at the picture's
/// corner so it stays put as the picture scrolls.
fn paint_checkerboard(
    window: &mut Window,
    image: Bounds<Pixels>,
    visible: Bounds<Pixels>,
    light: Hsla,
    dark: Hsla,
) {
    window.paint_quad(fill(visible, light));
    let area = f32::from(visible.size.width) * f32::from(visible.size.height);
    let mut square = SQUARE;
    while area / (square * square) > MAX_SQUARES {
        square *= 2.;
    }
    let (left, top) = (
        f32::from(visible.origin.x - image.origin.x),
        f32::from(visible.origin.y - image.origin.y),
    );
    let (right, bottom) = (
        left + f32::from(visible.size.width),
        top + f32::from(visible.size.height),
    );
    let first_row = (top / square).floor() as i32;
    let first_column = (left / square).floor() as i32;
    let mut row = first_row;
    while (row as f32) * square < bottom {
        let mut column = first_column;
        while (column as f32) * square < right {
            if (row + column).rem_euclid(2) == 1 {
                let x0 = (column as f32 * square).max(left);
                let y0 = (row as f32 * square).max(top);
                let x1 = ((column + 1) as f32 * square).min(right);
                let y1 = ((row + 1) as f32 * square).min(bottom);
                let origin = point(image.origin.x + px(x0), image.origin.y + px(y0));
                window.paint_quad(fill(
                    Bounds::new(origin, size(px(x1 - x0), px(y1 - y0))),
                    dark,
                ));
            }
            column += 1;
        }
        row += 1;
    }
}

/// The checkerboard's two greys, a step and two steps from `background`'s
/// lightness toward the contrasting end.
pub(crate) fn checker_colors(background: Hsla) -> (Hsla, Hsla) {
    let step = if background.l < 0.5 { 0.08 } else { -0.08 };
    let grey = |l: f32| Hsla {
        h: 0.,
        s: 0.,
        l: l.clamp(0., 1.),
        a: 1.,
    };
    (grey(background.l + step), grey(background.l + step * 2.))
}

/// All of `reader` up to `max` bytes, or `None` if it holds more.
fn read_limited(reader: impl Read, max: u64) -> std::io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    reader.take(max + 1).read_to_end(&mut bytes)?;
    Ok((bytes.len() as u64 <= max).then_some(bytes))
}

/// Reads and decodes `path`, off the main thread. Only the first frame of
/// an animation is decoded and kept.
#[cfg(test)]
fn decode(path: &Path) -> Decoded {
    decode_in(path, None)
}

/// [`decode`], drawing an SVG with `svg`'s renderer and letting it link
/// files in its workspace root.
fn decode_in(path: &Path, svg: Option<(&SvgRenderer, Option<&Path>)>) -> Decoded {
    use image::{DynamicImage, ImageDecoder as _, ImageFormat as Raster};
    let not_found = |error: &std::io::Error| error.kind() == std::io::ErrorKind::NotFound;
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if not_found(&error) => return Decoded::Missing,
        Err(error) => return Decoded::Failed(error.to_string()),
    };
    match file.metadata() {
        Ok(meta) if !meta.is_file() => return Decoded::Failed("not a regular file".into()),
        Ok(meta) if meta.len() > MAX_BYTES => return Decoded::TooLarge,
        Ok(_) => {}
        Err(error) => return Decoded::Failed(error.to_string()),
    }
    // The file may have grown since the check.
    let bytes = match read_limited(file, MAX_BYTES) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Decoded::TooLarge,
        Err(error) => return Decoded::Failed(error.to_string()),
    };
    let file_len = bytes.len() as u64;
    if is_svg(path) {
        return match svg {
            Some((renderer, root)) => decode_svg(path, bytes, renderer, root),
            None => Decoded::Failed("not a supported image format".into()),
        };
    }
    let reader = match image::ImageReader::new(Cursor::new(&bytes)).with_guessed_format() {
        Ok(reader) => reader,
        Err(error) => return Decoded::Failed(error.to_string()),
    };
    let format = match reader.format() {
        Some(
            format @ (Raster::Png
            | Raster::Jpeg
            | Raster::WebP
            | Raster::Gif
            | Raster::Bmp
            | Raster::Ico),
        ) => format,
        _ => return Decoded::Failed("not a supported image format".into()),
    };
    match reader.into_dimensions() {
        Ok((width, height)) if u64::from(width) * u64::from(height) > MAX_PIXELS => {
            return Decoded::TooLarge;
        }
        Ok(_) => {}
        Err(error) => return Decoded::Failed(error.to_string()),
    }
    let decoded = image::ImageReader::with_format(Cursor::new(&bytes), format)
        .into_decoder()
        .and_then(|mut decoder| {
            let orientation = decoder.orientation()?;
            let mut image = DynamicImage::from_decoder(decoder)?;
            image.apply_orientation(orientation);
            Ok(image.into_rgba8())
        });
    let mut pixels = match decoded {
        Ok(pixels) => pixels,
        Err(error) => return Decoded::Failed(error.to_string()),
    };
    let (width, height) = pixels.dimensions();
    let transparent = pixels.pixels().any(|pixel| pixel.0[3] != 255);
    // GPUI paints BGRA.
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    let image = Arc::new(RenderImage::new(vec![image::Frame::new(pixels)]));
    Decoded::Ready(Picture {
        width,
        height,
        image,
        transparent,
        file_len,
        svg: None,
    })
}

/// Draws the SVG `bytes` of `path` at twice its size, as the Markdown
/// preview does, unless it's too large or links a file outside `root` and
/// its own folder.
fn decode_svg(path: &Path, bytes: Vec<u8>, renderer: &SvgRenderer, root: Option<&Path>) -> Decoded {
    let file_len = bytes.len() as u64;
    if file_len > MAX_SVG_BYTES {
        return Decoded::TooLarge;
    }
    // As `usvg::Tree::from_data` tells compressed SVG.
    let data = if bytes.starts_with(&[0x1f, 0x8b]) {
        match gunzip(&bytes, MAX_SVG_BYTES as usize) {
            Ok(data) => data,
            Err(Gunzip::TooLarge) => return Decoded::TooLarge,
            Err(Gunzip::Malformed) => return Decoded::Failed("malformed gzip data".into()),
        }
    } else {
        bytes
    };
    let folders: Vec<PathBuf> = [root, path.parent()]
        .into_iter()
        .flatten()
        .filter_map(|folder| dunce::canonicalize(folder).ok())
        .collect();
    match check_svg_links(&data, &folders) {
        Ok(true) => {}
        Ok(false) => return Decoded::Blocked,
        Err(why) => return Decoded::Failed(why.into()),
    }
    let data = with_default_size(data);
    let parsed = match renderer.parse_svg(&data) {
        Ok(parsed) => parsed,
        Err(error) => return Decoded::Failed(error.to_string()),
    };
    // usvg sizes a document by its width and height, else its viewBox
    // (and `with_default_size` gave one without either 300 × 150).
    let tree = match usvg::Tree::from_data(&data, &usvg::Options::default()) {
        Ok(tree) => tree,
        Err(error) => return Decoded::Failed(error.to_string()),
    };
    let intrinsic = (tree.size().width(), tree.size().height());
    let (width, height) = (intrinsic.0.ceil().max(1.), intrinsic.1.ceil().max(1.));
    // The first render is at twice the size (see `SMOOTH_SVG_SCALE_FACTOR`),
    // which GPUI keeps within MAX_RENDER_SIDE a side.
    let (drawn_width, drawn_height) = rendered_size_at(intrinsic, SMOOTH_SVG_SCALE_FACTOR);
    if f64::from(width) * f64::from(height) > MAX_PIXELS as f64
        || f64::from(drawn_width) * f64::from(drawn_height) > MAX_PIXELS as f64
    {
        return Decoded::TooLarge;
    }
    // At exactly the size checked, never a side of no pixels.
    let exact = size(
        DevicePixels(drawn_width as i32),
        DevicePixels(drawn_height as i32),
    );
    let image = match renderer.render_parsed(&parsed, SvgSize::ExactSize(exact)) {
        Ok(image) => image,
        Err(error) => return Decoded::Failed(error.to_string()),
    };
    let transparent = image.as_bytes(0).is_some_and(|pixels| {
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] != 255)
    });
    Decoded::Ready(Picture {
        image,
        width: width as u32,
        height: height as u32,
        transparent,
        file_len,
        svg: Some(Svg {
            renderer: renderer.clone(),
            parsed: Arc::new(parsed),
            intrinsic,
        }),
    })
}

/// The pixels an SVG of `intrinsic` points is drawn in at `scale` pixels to
/// a point, as GPUI draws it: no more than [`MAX_RENDER_SIDE`] a side.
fn rendered_size_at(intrinsic: (f32, f32), scale: f32) -> (u32, u32) {
    let (width, height) = (
        (intrinsic.0 * scale).ceil().max(1.),
        (intrinsic.1 * scale).ceil().max(1.),
    );
    let fit = (MAX_RENDER_SIDE / width)
        .min(MAX_RENDER_SIDE / height)
        .min(1.);
    (
        (width * fit).floor().max(1.) as u32,
        (height * fit).floor().max(1.) as u32,
    )
}

/// `data` with a `width` of 300 and a `height` of 150 on its root element
/// where it has no such attribute and no viewBox, as a browser sizes a
/// standalone SVG: usvg would take the size of what's drawn.
fn with_default_size(data: Vec<u8>) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(&data) else {
        return data;
    };
    let options = usvg::roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    let Ok(document) = usvg::roxmltree::Document::parse_with_options(text, options) else {
        return data;
    };
    let root = document.root_element();
    if root.attribute("viewBox").is_some() {
        return data;
    }
    let mut insert = String::new();
    if root.attribute("width").is_none() {
        insert.push_str(" width=\"300\"");
    }
    if root.attribute("height").is_none() {
        insert.push_str(" height=\"150\"");
    }
    if insert.is_empty() {
        return data;
    }
    // After the root's tag name.
    let start = root.range().start;
    let name_end = text[start..]
        .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
        .map_or(text.len(), |at| start + at);
    format!("{}{insert}{}", &text[..name_end], &text[name_end..]).into_bytes()
}

/// Why [`gunzip`] gave up.
#[derive(Debug, PartialEq)]
enum Gunzip {
    /// It unpacks to more than the limit.
    TooLarge,
    Malformed,
}

/// The gzip `data` unpacked, no larger than `limit` bytes: the output is cut
/// off as it's made, so a bomb costs `limit` bytes of memory at most, whatever
/// its trailer says. (The checksum isn't checked.)
fn gunzip(data: &[u8], limit: usize) -> Result<Vec<u8>, Gunzip> {
    let mut at = 10;
    let flags = *data.get(3).ok_or(Gunzip::Malformed)?;
    if data.len() < 18 || data[2] != 8 {
        return Err(Gunzip::Malformed);
    }
    if flags & 4 != 0 {
        let len = u16::from_le_bytes([data[at], *data.get(at + 1).ok_or(Gunzip::Malformed)?]);
        at += 2 + usize::from(len);
    }
    for flag in [8, 16] {
        if flags & flag != 0 {
            let end = data
                .get(at..)
                .ok_or(Gunzip::Malformed)?
                .iter()
                .position(|b| *b == 0);
            at += end.ok_or(Gunzip::Malformed)? + 1;
        }
    }
    if flags & 2 != 0 {
        at += 2;
    }
    let mut inflater = Inflater {
        data: data.get(at..).ok_or(Gunzip::Malformed)?,
        pos: 0,
        bits: 0,
        count: 0,
        out: Vec::new(),
        limit,
    };
    inflater.run()?;
    Ok(inflater.out)
}

/// A canonical Huffman code: how many codes there are of each length, and
/// the symbols in code order.
struct Huffman {
    count: [u16; 16],
    symbol: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Result<Self, Gunzip> {
        let mut count = [0u16; 16];
        for length in lengths {
            count[usize::from(*length)] += 1;
        }
        let mut offsets = [0u16; 16];
        for length in 1..15 {
            offsets[length + 1] = offsets[length] + count[length];
        }
        let mut symbol = vec![0u16; lengths.len()];
        for (value, length) in lengths.iter().enumerate() {
            if *length != 0 {
                symbol[usize::from(offsets[usize::from(*length)])] = value as u16;
                offsets[usize::from(*length)] += 1;
            }
        }
        Ok(Self { count, symbol })
    }
}

/// A DEFLATE decoder with an output limit.
struct Inflater<'a> {
    data: &'a [u8],
    pos: usize,
    bits: u32,
    count: u32,
    out: Vec<u8>,
    limit: usize,
}

impl Inflater<'_> {
    fn take(&mut self, need: u32) -> Result<u32, Gunzip> {
        while self.count < need {
            let byte = *self.data.get(self.pos).ok_or(Gunzip::Malformed)?;
            self.pos += 1;
            self.bits |= u32::from(byte) << self.count;
            self.count += 8;
        }
        let value = self.bits & ((1u32 << need) - 1);
        self.bits >>= need;
        self.count -= need;
        Ok(value)
    }

    fn symbol(&mut self, huffman: &Huffman) -> Result<usize, Gunzip> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for length in 1..16 {
            code |= self.take(1)? as i32;
            let count = i32::from(huffman.count[length]);
            if code - count < first {
                return huffman
                    .symbol
                    .get((index + (code - first)) as usize)
                    .map(|symbol| usize::from(*symbol))
                    .ok_or(Gunzip::Malformed);
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err(Gunzip::Malformed)
    }

    fn push(&mut self, byte: u8) -> Result<(), Gunzip> {
        if self.out.len() >= self.limit {
            return Err(Gunzip::TooLarge);
        }
        self.out.push(byte);
        Ok(())
    }

    fn run(&mut self) -> Result<(), Gunzip> {
        loop {
            let last = self.take(1)? == 1;
            match self.take(2)? {
                0 => self.stored()?,
                1 => {
                    let mut lengths = [8u8; 288];
                    lengths[144..256].fill(9);
                    lengths[256..280].fill(7);
                    let literals = Huffman::new(&lengths)?;
                    let distances = Huffman::new(&[5u8; 30])?;
                    self.block(&literals, &distances)?;
                }
                2 => {
                    let (literals, distances) = self.dynamic_codes()?;
                    self.block(&literals, &distances)?;
                }
                _ => return Err(Gunzip::Malformed),
            }
            if last {
                return Ok(());
            }
        }
    }

    fn stored(&mut self) -> Result<(), Gunzip> {
        self.bits = 0;
        self.count = 0;
        let header = self
            .data
            .get(self.pos..self.pos + 4)
            .ok_or(Gunzip::Malformed)?;
        let len = usize::from(u16::from_le_bytes([header[0], header[1]]));
        if header[0] != !header[2] || header[1] != !header[3] {
            return Err(Gunzip::Malformed);
        }
        self.pos += 4;
        let chunk = self
            .data
            .get(self.pos..self.pos + len)
            .ok_or(Gunzip::Malformed)?;
        if self.out.len() + len > self.limit {
            return Err(Gunzip::TooLarge);
        }
        self.out.extend_from_slice(chunk);
        self.pos += len;
        Ok(())
    }

    fn dynamic_codes(&mut self) -> Result<(Huffman, Huffman), Gunzip> {
        const ORDER: [usize; 19] = [
            16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
        ];
        let literals = self.take(5)? as usize + 257;
        let distances = self.take(5)? as usize + 1;
        let codes = self.take(4)? as usize + 4;
        if literals > 286 || distances > 30 {
            return Err(Gunzip::Malformed);
        }
        let mut lengths = [0u8; 320];
        for order in &ORDER[..codes] {
            lengths[*order] = self.take(3)? as u8;
        }
        let code_lengths = Huffman::new(&lengths[..19])?;
        let mut lengths = [0u8; 320];
        let mut at = 0;
        while at < literals + distances {
            let symbol = self.symbol(&code_lengths)?;
            if symbol < 16 {
                lengths[at] = symbol as u8;
                at += 1;
                continue;
            }
            let (previous, repeat) = match symbol {
                16 => {
                    if at == 0 {
                        return Err(Gunzip::Malformed);
                    }
                    (lengths[at - 1], 3 + self.take(2)? as usize)
                }
                17 => (0, 3 + self.take(3)? as usize),
                _ => (0, 11 + self.take(7)? as usize),
            };
            if at + repeat > literals + distances {
                return Err(Gunzip::Malformed);
            }
            lengths[at..at + repeat].fill(previous);
            at += repeat;
        }
        Ok((
            Huffman::new(&lengths[..literals])?,
            Huffman::new(&lengths[literals..literals + distances])?,
        ))
    }

    fn block(&mut self, literals: &Huffman, distances: &Huffman) -> Result<(), Gunzip> {
        const LENGTH_BASE: [u16; 29] = [
            3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99,
            115, 131, 163, 195, 227, 258,
        ];
        const LENGTH_EXTRA: [u8; 29] = [
            0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
        ];
        const DISTANCE_BASE: [u16; 30] = [
            1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025,
            1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
        ];
        const DISTANCE_EXTRA: [u8; 30] = [
            0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12,
            12, 13, 13,
        ];
        loop {
            let symbol = self.symbol(literals)?;
            match symbol {
                0..=255 => self.push(symbol as u8)?,
                256 => return Ok(()),
                257..=285 => {
                    let ix = symbol - 257;
                    let length = usize::from(LENGTH_BASE[ix])
                        + self.take(u32::from(LENGTH_EXTRA[ix]))? as usize;
                    let ix = self.symbol(distances)?;
                    let base = *DISTANCE_BASE.get(ix).ok_or(Gunzip::Malformed)?;
                    let distance =
                        usize::from(base) + self.take(u32::from(DISTANCE_EXTRA[ix]))? as usize;
                    if distance > self.out.len() {
                        return Err(Gunzip::Malformed);
                    }
                    if self.out.len() + length > self.limit {
                        return Err(Gunzip::TooLarge);
                    }
                    for _ in 0..length {
                        let byte = self.out[self.out.len() - distance];
                        self.out.push(byte);
                    }
                }
                _ => return Err(Gunzip::Malformed),
            }
        }
    }
}

/// Checks that the SVG `data` links to no file outside `folders`: `Ok(false)`
/// if it does.
///
/// usvg reads the file an `<image>` or `<feImage>` `href` names, unless
/// it's a data URL: an absolute path as it is, a relative one against the
/// process's working directory. Whether it reads an element's `href` can
/// turn on how the SVG draws, so this checks every `href` in the document,
/// read with the XML parser and options usvg reads it with, as a path. An
/// existing file outside the folders blocks the SVG, and data this can't
/// read isn't drawn. (The Markdown preview's rule.)
fn check_svg_links(data: &[u8], folders: &[PathBuf]) -> Result<bool, &'static str> {
    let text = std::str::from_utf8(data).map_err(|_| "the SVG isn't UTF-8")?;
    let options = usvg::roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    let document = usvg::roxmltree::Document::parse_with_options(text, options)
        .map_err(|_| "can't parse the SVG")?;
    let hrefs = (document.descendants())
        .flat_map(|node| node.attributes())
        .filter(|attr| attr.name() == "href");
    for href in hrefs {
        // A file that isn't there isn't read.
        if let Ok(path) = dunce::canonicalize(Path::new(href.value()))
            && !folders.iter().any(|folder| path.starts_with(folder))
        {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `bytes` for a status bar or banner: `1.5 GiB`, `512 MiB`, `12 KiB`.
pub fn size_text(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;
    match bytes {
        GIB.. => format!("{:.1} GiB", bytes as f64 / GIB as f64),
        MIB.. => format!("{} MiB", bytes / MIB),
        KIB.. => format!("{} KiB", bytes / KIB),
        _ => format!("{bytes} bytes"),
    }
}

#[cfg(test)]
#[path = "image_viewer_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "svg_viewer_tests.rs"]
mod svg_tests;
