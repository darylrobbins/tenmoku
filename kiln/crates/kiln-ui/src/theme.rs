// SPDX-License-Identifier: Apache-2.0

//! The active theme: semantic colour tokens (kiln-core's [`ThemeData`])
//! resolved to GPUI's colours once per change, read by widgets while they
//! render with [`ActiveTheme::theme`] (`cx.theme()`). The flat [`UiTheme`]
//! the chrome's widgets once took stays, as a view of the active theme and
//! a way to set one ([`set_ui_theme`]).

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    sync::{Arc, LazyLock},
};

use gpui::{
    App, Global, Hsla, Pixels, Rgba, SharedString, Subscription, Window, WindowAppearance,
    WindowId, px, rgb, rgba,
};
pub use kiln_core::theme::{Appearance, Color, ThemeData, Tokens};
use kiln_core::theme::{Contrast, TOKEN_KEYS};
use kiln_platform::system_a11y::system_a11y;

/// Every list row's height: the project tree's, the outline's, the quick-open
/// picker's and the search results', so uniform lists lay them out alike.
pub const ROW_HEIGHT: Pixels = px(24.);

/// Colours for the workbench chrome: panels, rows and tabs, from before
/// theme tokens. [`ui_theme`] reads them: the last [`set_ui_theme`]'s, or
/// else the active [`Theme`]'s ([`Theme::ui_theme`]), which is
/// [`UiTheme::default_dark`] until a theme is set. kiln-ui's widgets read
/// `cx.theme()` instead, whose tokens [`set_ui_theme`] sets to these
/// colours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiTheme {
    /// Behind the editor and the empty pane, as the editor's own background.
    pub background: Hsla,
    pub panel_background: Hsla,
    pub border: Hsla,
    pub text: Hsla,
    /// Ignored entries and placeholders.
    pub muted_text: Hsla,
    /// Entries that couldn't be read.
    pub error_text: Hsla,
    pub row_hover: Hsla,
    pub row_selected: Hsla,
    /// The selected row while its list doesn't have focus.
    pub row_selected_inactive: Hsla,
    /// The outline of the selected row while its list has focus.
    pub row_focus_border: Hsla,
    /// A file's icon in a tree row.
    pub icon: Hsla,
    /// A folder's icon in a tree row.
    pub folder_icon: Hsla,
    /// A tree row's disclosure chevron.
    pub chevron: Hsla,
    /// The vertical lines marking a tree row's ancestor levels.
    pub indent_guide: Hsla,
    pub tab_active: Hsla,
    pub tab_active_text: Hsla,
    pub tab_inactive: Hsla,
    /// The dot on a tab with unsaved changes.
    pub dirty_dot: Hsla,
    /// Where a dragged tab would land: the insertion marker in a tab bar,
    /// and, at low alpha, the part of a pane a drop would fill.
    pub drop_target: Hsla,
    /// The bar beside the activity rail's highlighted button.
    pub accent: Hsla,
}

impl UiTheme {
    /// The colours the window had before the workbench: the editor's
    /// background (`EditorTheme::default_dark`) and the placeholder panel's.
    pub fn default_dark() -> Self {
        Self {
            background: rgb(0x16181d).into(),
            panel_background: rgb(0x121418).into(),
            border: rgb(0x23262d).into(),
            text: rgb(0x8b8f98).into(),
            muted_text: rgb(0x4b5160).into(),
            error_text: rgb(0xe06c75).into(),
            row_hover: rgba(0xffffff0a).into(),
            row_selected: rgba(0x4f7cc833).into(),
            row_selected_inactive: rgba(0xffffff0f).into(),
            row_focus_border: rgba(0x4f7cc8aa).into(),
            icon: rgb(0x8b8f98).into(),
            folder_icon: rgb(0x7f9fd8).into(),
            chevron: rgb(0x6c717c).into(),
            indent_guide: rgba(0xffffff14).into(),
            tab_active: rgb(0x16181d).into(),
            tab_active_text: rgb(0xc8ccd4).into(),
            tab_inactive: rgb(0x121418).into(),
            dirty_dot: rgb(0xd19a66).into(),
            drop_target: rgb(0x4f7cc8).into(),
            accent: rgb(0x4f7cc8).into(),
        }
    }
}

impl Default for UiTheme {
    fn default() -> Self {
        Self::default_dark()
    }
}

/// The chrome colours in use, as a GPUI global. [`set_ui_theme`] and
/// [`set_theme`] set it; components read it with [`ui_theme`] and follow
/// changes with `cx.observe_global::<ActiveUiTheme>`. kiln-ui never reads
/// settings or theme files itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActiveUiTheme(pub UiTheme);

impl Global for ActiveUiTheme {}

/// Makes `theme` the active chrome colours, notifying observers of
/// [`ActiveUiTheme`]. The active [`Theme`] becomes one with these colours
/// (over the base whose appearance the background suggests), so widgets
/// that read `cx.theme()` match. At [`Contrast::High`] both are those
/// colours through the high-contrast pass.
pub fn set_ui_theme(cx: &mut App, theme: UiTheme) {
    let plain = Theme::from_ui_theme(&theme);
    let contrast = current_contrast(cx);
    let active = plain.with_contrast(contrast);
    set_active(&active);
    let ui = match contrast {
        Contrast::Normal => theme,
        Contrast::High => active.ui_theme(),
    };
    cx.set_global(GlobalTheme {
        active,
        plain,
        choice: None,
        contrast,
    });
    cx.set_global(ActiveUiTheme(ui));
}

/// The active chrome colours: the last [`set_ui_theme`]'s, or the active
/// [`Theme`]'s, [`UiTheme::default_dark`] if none has been set.
pub fn ui_theme(cx: &App) -> UiTheme {
    cx.try_global::<ActiveUiTheme>()
        .map_or_else(|| cx.theme().ui_theme(), |active| active.0)
}

/// GPUI's colour for a theme colour.
pub fn hsla(color: Color) -> Hsla {
    rgba(color.to_rgba_u32()).into()
}

/// A theme colour for GPUI's, to the nearest 8 bits a channel.
fn color(hsla: Hsla) -> Color {
    let Rgba { r, g, b, a } = hsla.into();
    let byte = |channel: f32| u32::from((channel.clamp(0., 1.) * 255.).round() as u8);
    Color::rgba(byte(r) << 24 | byte(g) << 16 | byte(b) << 8 | byte(a))
}

/// A theme ready to draw with: its tokens as GPUI colours, resolved once
/// when it's made rather than on every frame. Widgets read the active one
/// with `cx.theme()`:
///
/// ```ignore
/// div().bg(cx.theme().colors.surface.panel)
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub name: SharedString,
    pub appearance: Appearance,
    pub colors: Tokens<Hsla>,
    data: Arc<ThemeData>,
}

static DARK: LazyLock<Theme> = LazyLock::new(|| Theme::new(ThemeData::dark().clone()));
static LIGHT: LazyLock<Theme> = LazyLock::new(|| Theme::new(ThemeData::light().clone()));

impl Theme {
    pub fn new(data: impl Into<Arc<ThemeData>>) -> Self {
        let data = data.into();
        Self {
            name: data.name.clone().into(),
            appearance: data.appearance,
            colors: data.tokens.map(hsla),
            data,
        }
    }

    /// The bundled dark base: the active theme until one is set.
    pub fn dark() -> Self {
        DARK.clone()
    }

    /// The bundled light base.
    pub fn light() -> Self {
        LIGHT.clone()
    }

    /// The theme as read, with its colours as written and the app's own
    /// tables ([`ThemeData::extension`]).
    pub fn data(&self) -> &ThemeData {
        &self.data
    }

    /// The theme at `contrast`: itself at [`Contrast::Normal`], and at
    /// [`Contrast::High`] with its tokens through the high-contrast pass
    /// ([`ThemeData::high_contrast`]). A colour the pass leaves alone keeps
    /// its GPUI colour exactly, as one [`Theme::from_ui_theme`] gave.
    pub fn with_contrast(&self, contrast: Contrast) -> Theme {
        if contrast == Contrast::Normal || self.data.contrast == Contrast::High {
            return self.clone();
        }
        let data = self.data.high_contrast();
        let mut colors = self.colors;
        for key in TOKEN_KEYS {
            let (before, after) = (self.data.tokens.get(key), data.tokens.get(key));
            if let (Some(before), Some(&after)) = (before, after)
                && *before != after
            {
                *colors.get_mut(key).expect("TOKEN_KEYS are tokens") = hsla(after);
            }
        }
        Theme {
            colors,
            data: Arc::new(data),
            ..self.clone()
        }
    }

    /// The flat chrome colours that the first widgets take.
    pub fn ui_theme(&self) -> UiTheme {
        let c = &self.colors;
        UiTheme {
            background: c.surface.background,
            panel_background: c.surface.panel,
            border: c.border.default,
            text: c.text.primary,
            muted_text: c.text.muted,
            error_text: c.status.error,
            row_hover: c.list.row_hover,
            row_selected: c.list.row_selected,
            row_selected_inactive: c.list.row_selected_inactive,
            row_focus_border: c.list.row_focus_border,
            icon: c.list.icon,
            folder_icon: c.list.folder_icon,
            chevron: c.list.chevron,
            indent_guide: c.list.indent_guide,
            tab_active: c.tab.active,
            tab_active_text: c.tab.active_text,
            tab_inactive: c.tab.inactive,
            dirty_dot: c.state.dirty,
            drop_target: c.state.drop_target,
            accent: c.accent.base,
        }
    }

    /// A theme with the flat chrome colours of `ui`, laid over the dark or
    /// light base as its background is dark or light, the tokens it
    /// doesn't name derived as a theme file's would be.
    ///
    /// The widgets that drew from a `UiTheme` now read these tokens, so
    /// each draws exactly the colour it did: every field's token is the
    /// field's colour as given (not rounded to a theme file's 8 bits a
    /// channel), and so are the colours they computed from it, the
    /// scrollbar's and the prompt's scrim.
    pub fn from_ui_theme(ui: &UiTheme) -> Self {
        let appearance = if ui.background.l > 0.5 {
            Appearance::Light
        } else {
            Appearance::Dark
        };
        let base = ThemeData::base(appearance);
        let set = Self::ui_tokens(ui);
        let tokens = kiln_core::theme::resolve(
            set.iter()
                .map(|&(key, hsla)| (key, color(hsla)))
                .chain([("surface.overlay", UI_OVERLAY)]),
            Some(&base.tokens),
        );
        let mut theme = Self::new(ThemeData {
            tokens,
            ..base.clone()
        });
        let colors = &mut theme.colors;
        for (key, hsla) in set {
            *colors.get_mut(key).expect("UiTheme fields are tokens") = hsla;
        }
        // Not 8-bit colours in general: half the muted text's alpha.
        colors.scrollbar.thumb = ui.muted_text.opacity(0.5);
        colors.scrollbar.thumb_hover = ui.text.opacity(0.6);
        theme
    }

    /// Each `UiTheme` field's token, with the field's colour: the `[ui]`
    /// alias table's pairs.
    fn ui_tokens(ui: &UiTheme) -> [(&'static str, Hsla); 20] {
        [
            ("surface.background", ui.background),
            ("surface.panel", ui.panel_background),
            ("border.default", ui.border),
            ("text.primary", ui.text),
            ("text.muted", ui.muted_text),
            ("status.error", ui.error_text),
            ("list.row_hover", ui.row_hover),
            ("list.row_selected", ui.row_selected),
            ("list.row_selected_inactive", ui.row_selected_inactive),
            ("list.row_focus_border", ui.row_focus_border),
            ("list.icon", ui.icon),
            ("list.folder_icon", ui.folder_icon),
            ("list.chevron", ui.chevron),
            ("list.indent_guide", ui.indent_guide),
            ("tab.active", ui.tab_active),
            ("tab.active_text", ui.tab_active_text),
            ("tab.inactive", ui.tab_inactive),
            ("state.dirty", ui.dirty_dot),
            ("state.drop_target", ui.drop_target),
            ("accent.base", ui.accent),
        ]
    }
}

/// The scrim behind a prompt in the flat chrome, whatever its background:
/// the dark base's `surface.overlay`.
const UI_OVERLAY: Color = Color::rgba(0x00000080);

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

/// Which theme of a dark and light pair is in use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ThemeMode {
    Dark,
    Light,
    /// The one matching the system's appearance, followed as it changes
    /// while a window is observing it ([`observe_system_appearance`]).
    #[default]
    System,
}

/// The active theme, as a GPUI global: follow changes with
/// `cx.observe_global::<GlobalTheme>`.
#[derive(Clone, Debug)]
pub struct GlobalTheme {
    /// `plain` at `contrast`: what's drawn.
    active: Theme,
    /// The theme set, or picked from `choice`, before the contrast.
    plain: Theme,
    /// The mode and the dark and light pair, if [`set_themes`] set it.
    choice: Option<(ThemeMode, Theme, Theme)>,
    /// The contrast every theme set is drawn at.
    contrast: Contrast,
}

impl Global for GlobalTheme {}

impl GlobalTheme {
    pub fn theme(&self) -> &Theme {
        &self.active
    }

    /// The mode [`set_themes`] set, or `None` if the theme was set alone.
    pub fn mode(&self) -> Option<ThemeMode> {
        self.choice.as_ref().map(|(mode, _, _)| *mode)
    }

    /// The dark and light pair [`set_themes`] set, or `None` if the theme
    /// was set alone.
    pub fn themes(&self) -> Option<(&Theme, &Theme)> {
        self.choice.as_ref().map(|(_, dark, light)| (dark, light))
    }

    /// The contrast the theme is drawn at ([`GlobalTheme::set_contrast`]).
    pub fn contrast(&self) -> Contrast {
        self.contrast
    }

    /// Draws the active theme, and every theme set after it, at
    /// `contrast`, as an app does for the system's Increase Contrast
    /// option or its own setting, redrawing every window if that changes
    /// what's drawn. In [`ThemeMode::System`] it also picks the theme for
    /// the system's appearance again, as that's read differently while
    /// Increase Contrast is on ([`system_appearance`]): call it whenever
    /// the system's options change, even with the contrast the same.
    pub fn set_contrast(cx: &mut App, contrast: Contrast) {
        let (plain, choice) = match cx.try_global::<GlobalTheme>() {
            Some(global) => (global.plain.clone(), global.choice.clone()),
            None => (Theme::dark(), None),
        };
        let plain = match &choice {
            Some((ThemeMode::System, dark, light)) => {
                let appearance = system_appearance(cx, App::window_appearance);
                pick(ThemeMode::System, appearance, dark, light).clone()
            }
            _ => plain,
        };
        let unchanged = cx
            .try_global::<GlobalTheme>()
            .map_or(contrast == Contrast::Normal, |global| {
                global.contrast == contrast && global.plain == plain
            });
        if !unchanged {
            activate(cx, plain, choice, contrast);
        }
    }
}

/// The contrast themes are drawn at: [`Contrast::Normal`] until
/// [`GlobalTheme::set_contrast`] says otherwise.
fn current_contrast(cx: &App) -> Contrast {
    cx.try_global::<GlobalTheme>()
        .map_or(Contrast::Normal, |global| global.contrast)
}

/// Reads the system's appearance as plain dark or light for
/// [`system_appearance`]: kiln-platform's reader, unless a test sets
/// another.
#[derive(Clone, Copy)]
struct PlatformAppearance(fn() -> Option<Appearance>);

impl Global for PlatformAppearance {}

/// Has [`system_appearance`] read the platform's appearance with `read`.
#[cfg(test)]
pub(crate) fn set_platform_appearance(cx: &mut App, read: fn() -> Option<Appearance>) {
    cx.set_global(PlatformAppearance(read));
}

/// The system's appearance, as [`ThemeMode::System`] follows it: GPUI's
/// reading (`gpui`), except while the system's Increase Contrast option
/// is on. Then GPUI's mapping sees a high-contrast appearance name it
/// doesn't know, logs "unknown appearance" and answers light whatever the
/// system is, so the platform's own reading
/// ([`kiln_platform::system_a11y::appearance`]) is taken instead, and
/// GPUI's isn't asked at all while that answers.
pub(crate) fn system_appearance(
    cx: &App,
    gpui: impl FnOnce(&App) -> WindowAppearance,
) -> WindowAppearance {
    if system_a11y(cx).increase_contrast {
        let read = cx.try_global::<PlatformAppearance>().map_or(
            kiln_platform::system_a11y::appearance as fn() -> Option<Appearance>,
            |platform| platform.0,
        );
        match read() {
            Some(Appearance::Dark) => return WindowAppearance::Dark,
            Some(Appearance::Light) => return WindowAppearance::Light,
            None => {}
        }
    }
    gpui(cx)
}

/// Reads the active theme: `cx.theme()`, from any `App` or `Context`.
pub trait ActiveTheme {
    fn theme(&self) -> &Theme;
}

impl ActiveTheme for App {
    /// The last theme set, or the bundled dark base.
    fn theme(&self) -> &Theme {
        self.try_global::<GlobalTheme>()
            .map_or_else(|| &*DARK, |global| &global.active)
    }
}

/// Makes `theme` the active theme whatever the system's appearance,
/// notifying observers of [`GlobalTheme`] and [`ActiveUiTheme`] and
/// redrawing every window.
pub fn set_theme(cx: &mut App, theme: Theme) {
    let contrast = current_contrast(cx);
    activate(cx, theme, None, contrast);
}

/// Sets a dark and a light theme and which to use: `mode` picks one, or
/// for [`ThemeMode::System`] the one matching the system's appearance now
/// ([`system_appearance`]).
pub fn set_themes(cx: &mut App, mode: ThemeMode, dark: Theme, light: Theme) {
    let active = match mode {
        ThemeMode::Dark => dark.clone(),
        ThemeMode::Light => light.clone(),
        ThemeMode::System => {
            let appearance = system_appearance(cx, App::window_appearance);
            pick(mode, appearance, &dark, &light).clone()
        }
    };
    let contrast = current_contrast(cx);
    activate(cx, active, Some((mode, dark, light)), contrast);
}

/// Switches to `mode` over the dark and light pair [`set_themes`] set, or
/// kiln's bundled pair if none was set.
pub fn set_theme_mode(cx: &mut App, mode: ThemeMode) {
    let (dark, light) = cx
        .try_global::<GlobalTheme>()
        .and_then(|global| global.choice.as_ref())
        .map(|(_, dark, light)| (dark.clone(), light.clone()))
        .unwrap_or_else(|| (Theme::dark(), Theme::light()));
    set_themes(cx, mode, dark, light);
}

/// Keeps the active theme matching `window`'s appearance while the mode is
/// [`ThemeMode::System`], until the subscription is dropped. One window is
/// enough: the appearance is the system's. While Increase Contrast is on,
/// the appearance is the platform's own reading ([`system_appearance`]).
pub fn observe_system_appearance(window: &Window) -> Subscription {
    window.observe_window_appearance(|window, cx| {
        let appearance = system_appearance(cx, |_| window.appearance());
        system_appearance_changed(cx, appearance)
    })
}

/// Keeps the theme following the system's dark or light appearance, as
/// `window` sees it, while the mode is [`ThemeMode::System`], holding the
/// subscription ([`observe_system_appearance`]) until the window closes.
/// Calling it again for the same window does nothing more. GPUI tells only
/// windows of appearance changes, and has no hook for a window opening, so
/// each window's root view calls it as it's made.
pub fn follow_system_appearance(window: &Window, cx: &mut App) {
    let id = window.window_handle().window_id();
    if !cx.has_global::<Following>() {
        let closed = cx.on_window_closed(|cx, closed| {
            cx.global_mut::<Following>().windows.remove(&closed);
        });
        cx.set_global(Following {
            windows: HashMap::new(),
            _closed: closed,
        });
    }
    if cx.global::<Following>().windows.contains_key(&id) {
        return;
    }
    let subscription = observe_system_appearance(window);
    cx.global_mut::<Following>()
        .windows
        .insert(id, subscription);
}

/// The windows whose appearance the theme follows.
struct Following {
    windows: HashMap<WindowId, Subscription>,
    _closed: Subscription,
}

impl Global for Following {}

/// Re-picks the theme for `appearance` if the mode is [`ThemeMode::System`]
/// and that changes it.
pub fn system_appearance_changed(cx: &mut App, appearance: WindowAppearance) {
    let Some(global) = cx.try_global::<GlobalTheme>() else {
        return;
    };
    let Some((ThemeMode::System, dark, light)) = &global.choice else {
        return;
    };
    let theme = pick(ThemeMode::System, appearance, dark, light);
    if *theme != global.plain {
        let (theme, choice) = (theme.clone(), global.choice.clone());
        activate(cx, theme, choice, global.contrast);
    }
}

fn pick<'a>(
    mode: ThemeMode,
    appearance: WindowAppearance,
    dark: &'a Theme,
    light: &'a Theme,
) -> &'a Theme {
    let dark_system = matches!(
        appearance,
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    );
    match mode {
        ThemeMode::Dark => dark,
        ThemeMode::Light => light,
        ThemeMode::System if dark_system => dark,
        ThemeMode::System => light,
    }
}

thread_local! {
    /// The active theme's appearance, for what answers without an `App`,
    /// as an icon set does. Per thread, as the icon set is
    /// (`file_icons::ICON_SET`).
    static APPEARANCE: Cell<Appearance> = const { Cell::new(Appearance::Dark) };
}

/// The active theme's appearance: the bundled dark base's until a theme is
/// set. A theme change redraws every window, so what's drawn by it follows.
pub(crate) fn active_appearance() -> Appearance {
    APPEARANCE.get()
}

thread_local! {
    /// The active theme's colours, for what's drawn without an `App`, as a
    /// tinted icon is ([`IconRef::Tinted`](crate::IconRef::Tinted)). `None`
    /// until a theme is set, for the bundled dark base's.
    static COLORS: RefCell<Option<Tokens<Hsla>>> = const { RefCell::new(None) };
}

/// The active theme's colour for the token `key`, such as
/// `"file_icon.rust"`: the bundled dark base's until a theme is set.
pub(crate) fn active_color(key: &str) -> Option<Hsla> {
    COLORS.with_borrow(|colors| (colors.as_ref()).unwrap_or(&DARK.colors).get(key).copied())
}

/// Makes `active` what [`active_appearance`] and [`active_color`] answer.
fn set_active(active: &Theme) {
    APPEARANCE.set(active.appearance);
    COLORS.set(Some(active.colors));
}

/// Makes `plain` at `contrast` the active theme.
fn activate(
    cx: &mut App,
    plain: Theme,
    choice: Option<(ThemeMode, Theme, Theme)>,
    contrast: Contrast,
) {
    let active = plain.with_contrast(contrast);
    set_active(&active);
    cx.set_global(ActiveUiTheme(active.ui_theme()));
    cx.set_global(GlobalTheme {
        active,
        plain,
        choice,
        contrast,
    });
    cx.refresh_windows();
}
