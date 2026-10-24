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
    time::SystemTime,
};

use fe_command::{Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Registry};
use gpui::{
    App, Bounds, ContentMask, Context, CursorStyle, FocusHandle, Focusable, Hsla,
    InteractiveElement as _, IntoElement, KeyContext, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement as _, PinchEvent, Pixels, Point, Render, RenderImage,
    ScrollWheelEvent, SharedString, Size, Styled as _, Subscription, Task, Window, canvas, div,
    fill, point, px, size,
};

use crate::{EditorTheme, commands::RunCommand};

pub(crate) mod pane_item;

/// A file over this many bytes isn't read.
pub(crate) const MAX_BYTES: u64 = 64 * 1024 * 1024;

/// An image of more pixels than this isn't decoded.
pub(crate) const MAX_PIXELS: u64 = 100_000_000;

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
    /// How many times the file was read.
    #[cfg(test)]
    pub(crate) loads: usize,
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
            #[cfg(test)]
            loads: 0,
        };
        viewer.load(cx);
        viewer
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
        let decoding = cx.background_spawn(async move { decode(&path) });
        self.loading = Some(cx.spawn(async move |this, cx| {
            let decoded = decoding.await;
            this.update(cx, |viewer, cx| {
                viewer.loading = None;
                viewer.content = match decoded {
                    Decoded::Ready(picture) => Content::Ready(picture),
                    Decoded::TooLarge => Content::Message("Too large to preview".into()),
                    Decoded::Missing => Content::Message("File not found".into()),
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
                        move |bounds, _, cx| {
                            viewer.update(cx, |viewer, _| {
                                viewer.viewport = bounds;
                                viewer.clamp_scroll();
                                viewer.geometry()
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
fn decode(path: &Path) -> Decoded {
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
    })
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
