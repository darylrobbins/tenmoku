// SPDX-License-Identifier: Apache-2.0

//! A window's options, right for the platform: on macOS the unified title
//! bar with the traffic lights inline and the app owning the title row's
//! drag; elsewhere a plain title for the window manager to draw.

use gpui::{
    App, Pixels, Point, SharedString, Size, TitlebarOptions, WindowBounds, WindowOptions, point,
    px, size,
};

use crate::Capabilities;

/// The unified title bar's height.
pub const TITLE_BAR_HEIGHT: Pixels = px(32.);

/// Where the traffic lights sit, from the window's top-left corner: 9 px
/// down centres the 14 px buttons in the 32 px bar.
pub const TRAFFIC_LIGHTS: Point<Pixels> = point(px(12.), px(9.));

/// The width the traffic lights take, plus a gap: a centred title keeps
/// out of it on both sides.
pub const TRAFFIC_LIGHT_INSET: Pixels = px(78.);

/// The title bar options for a window whose content draws under its title
/// bar, on macOS. Its host also sets `WindowOptions::app_owns_titlebar_drag`
/// and draws the title row: [`window_options`] does the first.
pub fn titlebar_options(title: impl Into<SharedString>) -> TitlebarOptions {
    TitlebarOptions {
        title: Some(title.into()),
        appears_transparent: true,
        traffic_light_position: Some(TRAFFIC_LIGHTS),
    }
}

/// What a window should be.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowSpec {
    pub title: SharedString,
    pub size: Size<Pixels>,
    /// The platform's ID for the app, which Wayland and X11 match to its
    /// `.desktop` file: its reverse-DNS ID.
    pub app_id: Option<String>,
}

impl WindowSpec {
    /// An app's main window: 1200 by 800, centred.
    pub fn main(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            size: size(px(1200.), px(800.)),
            app_id: None,
        }
    }

    pub fn size(mut self, size: Size<Pixels>) -> Self {
        self.size = size;
        self
    }

    pub fn app_id(mut self, app_id: impl Into<String>) -> Self {
        self.app_id = Some(app_id.into());
        self
    }
}

/// GPUI's options for `spec` on a platform with `capabilities`, centred on
/// the main display. Edit the result for anything else.
pub fn window_options(spec: &WindowSpec, capabilities: &Capabilities, cx: &App) -> WindowOptions {
    let unified = capabilities.unified_title_bar;
    WindowOptions {
        window_bounds: Some(WindowBounds::centered(spec.size, cx)),
        titlebar: Some(if unified {
            titlebar_options(spec.title.clone())
        } else {
            TitlebarOptions {
                title: Some(spec.title.clone()),
                ..Default::default()
            }
        }),
        app_owns_titlebar_drag: unified,
        app_id: spec.app_id.clone(),
        ..Default::default()
    }
}
