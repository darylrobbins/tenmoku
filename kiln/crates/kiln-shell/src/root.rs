// SPDX-License-Identifier: Apache-2.0

//! [`Root`]: the view a window opens with, around the app's own. It draws
//! the window's chrome as the platform wants it, and sets up what every
//! window needs.

use gpui::{
    AnyView, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};
use kiln_platform::{Capabilities, focus};
use kiln_ui::{ActiveTheme as _, FontFamilies, follow_system_appearance};

use crate::{
    MenuBarView, TitleBar,
    menu_bar::Open,
    window_frame::{self, TopRow},
};

/// A window's root view, around the app's `content`:
///
/// ```ignore
/// cx.open_window(kiln::window_options(&spec, cx), |window, cx| {
///     let notes = cx.new(|cx| Notes::new(window, cx));
///     cx.new(|cx| kiln::Root::new(notes, window, cx).title("Notes"))
/// })
/// ```
///
/// It draws, above the content:
///
/// - on macOS, the unified title bar, the traffic lights inline;
/// - where the platform has no native menus (Linux), the installed menus
///   ([`kiln_platform::menus::install`]) in the window, opened by a click or
///   by [`Open`] (`menu_bar.open`, F10 by convention), which
///   [`crate::register`] records;
/// - where the platform leaves the frame to the app (client-side
///   decorations, as GNOME on Wayland), the frame: the title and the
///   window's buttons beside the menus, a drag to move, a double click to
///   maximize, a border, and edges that resize. Under server-side
///   decorations the window manager draws them.
///
/// Making it has the window keep keys working when the focused element
/// stops being drawn ([`focus::watch`]) and the theme follow the system's
/// appearance ([`follow_system_appearance`]), and, on macOS, restarts the
/// window's display link when its frame requests stop
/// ([`watch_frame_requests`](crate::watch_frame_requests)).
pub struct Root {
    content: AnyView,
    title: SharedString,
    context: Option<SharedString>,
    capabilities: Capabilities,
    menu_bar: Entity<MenuBarView>,
    _frame_watch: Option<gpui::Task<()>>,
}

impl Root {
    pub fn new(content: impl Into<AnyView>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        focus::watch(window, cx);
        follow_system_appearance(window, cx);
        Self {
            content: content.into(),
            title: SharedString::default(),
            context: None,
            capabilities: Capabilities::of(cx),
            menu_bar: cx.new(MenuBarView::new),
            _frame_watch: crate::watch_frame_requests(window, cx),
        }
    }

    /// The title the chrome shows, in the unified title bar or the frame
    /// drawn under client-side decorations. The window's own title, for
    /// the window manager and the task switcher, stays the app's to set
    /// with `window.set_window_title`.
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = title.into();
        self
    }

    /// Changes the title the chrome shows: see [`Root::title`].
    pub fn set_title(&mut self, title: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.title = title.into();
        cx.notify();
    }

    /// The app's root key context, which the frame's buttons run their
    /// commands in (`window.close` and the rest), as a menu item would.
    pub fn key_context(mut self, context: impl Into<SharedString>) -> Self {
        self.context = Some(context.into());
        self
    }

    /// Draws the chrome as on a platform with `capabilities`, rather than
    /// the app's ([`Capabilities::of`]): for tests, and for showing one
    /// platform's chrome on another.
    pub fn capabilities(mut self, capabilities: Capabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// The app's view.
    pub fn content(&self) -> &AnyView {
        &self.content
    }

    /// The in-window menu bar, if the platform has no native one.
    pub fn menu_bar(&self) -> Option<&Entity<MenuBarView>> {
        (!self.capabilities.native_menus).then_some(&self.menu_bar)
    }

    fn open_menu_bar(&mut self, _: &Open, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(bar) = self.menu_bar() {
            bar.update(cx, |bar, cx| bar.open(0, true, window, cx));
        }
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = &cx.theme().colors;
        let unified = self.capabilities.unified_title_bar;
        let tiling = window_frame::client_tiling(window, cx).filter(|_| !unified);
        let menu_bar = self.menu_bar().cloned();
        let top_row = (menu_bar.is_some() || tiling.is_some()).then(|| {
            let row = TopRow::new(self.title.clone(), tiling.is_some())
                .when_some(menu_bar, |row, bar| row.menu_bar(bar));
            match &self.context {
                Some(context) => row.key_context(context.clone()),
                None => row,
            }
        });
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.surface.background)
            .text_color(colors.text.primary)
            // Kiln's body text, as its prompts and fields have it, in the
            // UI family the app set ([`FontFamilies`]); the content can set
            // its own.
            .font_family(ui_family(cx))
            .text_size(px(13.))
            .on_action(cx.listener(Self::open_menu_bar))
            .when(unified, |root| {
                root.child(TitleBar::new(self.title.clone()))
            })
            .children(top_row)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .child(self.content.clone()),
            )
            // The frame's border and resize edges, over everything, unless
            // the window fills the screen.
            .when_some(tiling.filter(|tiling| !tiling.is_tiled()), |root, _| {
                root.border_1().border_color(colors.border.default)
            })
            .children(tiling.into_iter().flat_map(window_frame::resize_handles))
    }
}

/// The UI family of the app's [`FontFamilies`], or of the default when it
/// set none: the platform's UI font.
fn ui_family(cx: &gpui::App) -> SharedString {
    cx.try_global::<FontFamilies>().map_or_else(
        || FontFamilies::default().ui,
        |families| families.ui.clone(),
    )
}
