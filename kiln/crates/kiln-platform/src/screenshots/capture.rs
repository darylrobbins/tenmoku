// SPDX-License-Identifier: Apache-2.0

//! Off-screen windows, and drawing one until it settles before capturing
//! it.

use std::{cell::Cell, rc::Rc, sync::Arc};

use gpui::{
    AnyWindowHandle, App, AppContext, AssetSource, Entity, Pixels, Render, Size, Subscription,
    Window, WindowHandle,
};
use image::RgbaImage;

use super::Result;

/// How many frames a scene may take to settle.
pub const MAX_FRAMES: usize = 10;

/// What [`Settle`] needs of a test app context that draws for real.
pub trait Offscreen: AppContext {
    /// Updates the app.
    fn update_app<R>(&mut self, f: impl FnOnce(&mut App) -> R) -> R;

    /// Runs the foreground and background tasks until none are left.
    fn run_until_parked(&self);

    /// Opens a window of `size` logical pixels that nobody sees.
    fn open_offscreen_window<V: Render + 'static>(
        &mut self,
        size: Size<Pixels>,
        build_root: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
    ) -> gpui::Result<WindowHandle<V>>;
}

/// GPUI's real platform, its windows off screen (at -10000, -10000) and
/// drawn by Metal; AppKit needs it on the main thread.
#[cfg(target_os = "macos")]
impl Offscreen for gpui::VisualTestAppContext {
    fn update_app<R>(&mut self, f: impl FnOnce(&mut App) -> R) -> R {
        gpui::VisualTestAppContext::update(self, f)
    }

    fn run_until_parked(&self) {
        gpui::VisualTestAppContext::run_until_parked(self)
    }

    fn open_offscreen_window<V: Render + 'static>(
        &mut self,
        size: Size<Pixels>,
        build_root: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
    ) -> gpui::Result<WindowHandle<V>> {
        gpui::VisualTestAppContext::open_offscreen_window(self, size, build_root)
    }
}

/// GPUI's test platform with this platform's text system and headless
/// renderer, where it has one; its windows are 2× and never shown.
impl Offscreen for gpui::HeadlessAppContext {
    fn update_app<R>(&mut self, f: impl FnOnce(&mut App) -> R) -> R {
        gpui::HeadlessAppContext::update(self, f)
    }

    fn run_until_parked(&self) {
        gpui::HeadlessAppContext::run_until_parked(self)
    }

    fn open_offscreen_window<V: Render + 'static>(
        &mut self,
        size: Size<Pixels>,
        build_root: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
    ) -> gpui::Result<WindowHandle<V>> {
        gpui::HeadlessAppContext::open_window(self, size, build_root)
    }
}

/// This platform's app context for screenshots: on macOS the real platform
/// drawing with Metal, as the goldens are; elsewhere GPUI's headless
/// context, which draws with the platform's headless renderer (wgpu on
/// Linux, needing a Vulkan device) and its own text system, so its
/// captures can't match macOS's goldens.
#[cfg(target_os = "macos")]
pub type OffscreenApp = gpui::VisualTestAppContext;

/// This platform's app context for screenshots: on macOS the real platform
/// drawing with Metal, as the goldens are; elsewhere GPUI's headless
/// context, which draws with the platform's headless renderer (wgpu on
/// Linux, needing a Vulkan device) and its own text system, so its
/// captures can't match macOS's goldens.
#[cfg(not(target_os = "macos"))]
pub type OffscreenApp = gpui::HeadlessAppContext;

/// A new [`OffscreenApp`] whose views load their icons and other assets
/// from `assets`.
pub fn offscreen_app(assets: Arc<dyn AssetSource>) -> OffscreenApp {
    #[cfg(target_os = "macos")]
    {
        gpui::VisualTestAppContext::with_asset_source(
            gpui_platform::current_platform(false),
            assets,
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        let text_system = gpui_platform::current_platform(true).text_system();
        gpui::HeadlessAppContext::with_platform(
            text_system,
            assets,
            gpui_platform::current_headless_renderer,
        )
    }
}

/// Draws a window until it settles, then captures it. Views notify
/// themselves when background work lands (loading languages, parsing, a
/// listing) or a frame asks for another, so a frame after which no
/// observed view notified, and whose background work left them quiet,
/// shows the result.
///
/// Observe every view whose content can still change, then call
/// [`Settle::capture`].
pub struct Settle {
    notifications: Rc<Cell<usize>>,
    observers: Vec<Subscription>,
}

impl Settle {
    /// Observes nothing yet.
    pub fn new() -> Self {
        Self {
            notifications: Rc::default(),
            observers: Vec::new(),
        }
    }

    /// Counts `entity`'s notifications from now on.
    pub fn observe<T: 'static>(&mut self, entity: &Entity<T>, cx: &mut impl Offscreen) {
        let notifications = self.notifications.clone();
        let observer = cx.update_app(|cx| {
            cx.observe(entity, move |_, _| {
                notifications.set(notifications.get() + 1)
            })
        });
        self.observers.push(observer);
    }

    /// How many times the observed views have notified so far.
    pub fn notifications(&self) -> usize {
        self.notifications.get()
    }

    /// Draws `window` until a frame and the work it started leave every
    /// observed view quiet, at most [`MAX_FRAMES`] times, captures that
    /// frame, then closes the window. Returns the capture and how many
    /// notifications the observed views had sent when it was taken, and
    /// fails if the window is still changing after the last frame. (A
    /// test-mode app also redraws a window whenever it's notified, so the
    /// work can land over fewer frames than it has rounds.)
    pub fn capture(
        &self,
        cx: &mut impl Offscreen,
        window: AnyWindowHandle,
    ) -> Result<(RgbaImage, usize)> {
        let mut settled = false;
        for _ in 0..MAX_FRAMES {
            let before = self.notifications();
            cx.update_window(window, |_, window, cx| {
                window.draw(cx).clear(cx);
                window.simulate_next_frame(cx);
            })?;
            cx.run_until_parked();
            if self.notifications() == before {
                settled = true;
                break;
            }
        }
        let image = if settled {
            cx.update_window(window, |_, window, _| window.render_to_image())?
                .map(|image| (image, self.notifications()))
                .map_err(Into::into)
        } else {
            Err(format!("still changing after {MAX_FRAMES} frames").into())
        };
        cx.update_window(window, |_, window, _| window.remove_window())?;
        cx.run_until_parked();
        image
    }
}

impl Default for Settle {
    fn default() -> Self {
        Self::new()
    }
}
