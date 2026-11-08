//! What the setup wizard's Appearance and Font pages show of a choice before
//! it's made: tiles drawn in a theme's own colours, a code sample in the theme
//! and font chosen so far, and fonts named in themselves. The sheet in
//! [`crate::setup_wizard`] lays them out.
//!
//! - A [`Swatch`] is read from the theme's file, not the theme in use, so a
//!   tile looks like what choosing it would bring. There's no file to read for
//!   a theme not bundled: that one is the active theme, or the bundled base of
//!   its appearance.
//! - The [`Sample`] follows the first pane: its palette as the pane's editors
//!   do, and its font from the pane, which [`crate::Workbench::reload_settings`]
//!   gives this window's settings, its project's layer included. It can't take
//!   focus: nothing reaches it through what's over it.
//! - Every colour here is one of those themes', so nothing bypasses them.

use std::collections::HashMap;

use fe_buffer::Buffer;
use fe_config::{
    EditorPalette,
    themes::{self, BundledTheme},
};
use fe_editor::EditorModel;
use fe_gpui::{ActivePalette, EditorSettings, EditorTheme, EditorView};
use fe_ui::{ActiveTheme as _, Theme, hsla};
use gpui::{
    App, AppContext as _, Context, Div, Entity, Hsla, InteractiveElement as _, ParentElement as _,
    Pixels, SharedString, Styled as _, Subscription, div, linear_color_stop, linear_gradient, px,
};
use kiln::{
    core::theme::{Appearance, ThemeData},
    platform::fonts,
};

use crate::pane::Pane;

/// A tile's outer width, which fits a title as long as "Catppuccin Macchiato".
pub(crate) const TILE_WIDTH: Pixels = px(128.);

/// How tall a tile's small window is.
pub(crate) const TILE_HEIGHT: Pixels = px(40.);

/// How wide the column of three tiles is, with what's between them.
pub(crate) const TILES_WIDTH: Pixels = px(400.);

/// How wide the small window's sidebar is.
const SIDEBAR_WIDTH: Pixels = px(24.);

/// The "code" bars' widths: keyword, string and function. Short enough to stay
/// on their own side of a split tile.
const BAR_WIDTHS: [Pixels; 3] = [px(30.), px(44.), px(22.)];

/// The sample: a row or two of each thing a theme colours, and narrow enough
/// for the column beside the tiles at the default size.
const SAMPLE: &str = r#"// Fire a glaze and report it.
const KILN: &str = "anagama";

struct Glaze {
    cone: u32,
}

fn fire(g: &Glaze) -> String {
    let heat = g.cone * 60;
    let done = heat > 1200;
    format!("{KILN}: {done}")
}
"#;

/// The colours a tile takes from its theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Swatch {
    pub(crate) panel: Hsla,
    pub(crate) background: Hsla,
    pub(crate) accent: Hsla,
    pub(crate) keyword: Hsla,
    pub(crate) string: Hsla,
    pub(crate) function: Hsla,
}

impl Swatch {
    fn of(data: &ThemeData) -> Self {
        let colors = Theme::new(data.clone()).colors;
        // Tables with problems are the bundled theme's own test's to report.
        let (palette, _) = EditorPalette::for_theme(data);
        let syntax =
            |capture: &str| (palette.syntax_color(capture)).map_or(colors.text.primary, hsla);
        Self {
            panel: colors.surface.panel,
            background: colors.surface.background,
            accent: colors.accent.base,
            keyword: syntax("keyword"),
            string: syntax("string"),
            function: syntax("function"),
        }
    }
}

/// Every bundled theme's [`Swatch`] by its title, parsed once when the sheet
/// opens. The wizard's model labels a theme's option with its title.
pub(crate) struct Swatches(HashMap<&'static str, Swatch>);

impl Swatches {
    pub(crate) fn load() -> Self {
        let swatch = |theme: &BundledTheme| Swatch::of(&ThemeData::from_file(&theme.file()));
        Self(
            (themes::bundled_themes().iter())
                .map(|theme| (theme.title, swatch(theme)))
                .collect(),
        )
    }

    /// The tile of the option labelled `label` in the list for `appearance`.
    pub(crate) fn of_option(&self, label: &str, appearance: Appearance, cx: &App) -> Swatch {
        self.0.get(label).copied().unwrap_or_else(|| {
            let active = cx.theme();
            match active.appearance == appearance {
                true => Swatch::of(active.data()),
                false => Swatch::of(ThemeData::base(appearance)),
            }
        })
    }
}

/// What a tile draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum TileArt {
    /// A small window in one theme's colours.
    Window(Swatch),
    /// Match System: split corner to corner, dark above and light below.
    Split { dark: Swatch, light: Swatch },
}

/// Three short "code" bars, one in each of a theme's syntax colours.
fn bars(swatch: &Swatch) -> Div {
    let colors = [swatch.keyword, swatch.string, swatch.function];
    let bars = (BAR_WIDTHS.iter().zip(colors))
        .map(|(width, color)| div().w(*width).h(px(3.)).rounded_sm().bg(color));
    div().flex().flex_col().gap(px(3.)).children(bars)
}

/// The tab underline of a theme's accent.
fn underline(swatch: &Swatch) -> Div {
    div().w(px(16.)).h(px(2.)).bg(swatch.accent)
}

/// The angle of the gradient whose middle runs through a `width` by `height`
/// box's corners. The shader scales a gradient's direction by the box's sides,
/// so it's the square of their ratio.
fn corner_angle(width: Pixels, height: Pixels) -> f32 {
    let ratio = f32::from(width) / f32::from(height);
    90. + (ratio * ratio).atan().to_degrees()
}

impl TileArt {
    /// The art, filling what it's put in: `width` by [`TILE_HEIGHT`].
    pub(crate) fn render(&self, width: Pixels) -> Div {
        // Rounded itself, as a clip is square and its frame's corners aren't.
        let base = div()
            .relative()
            .h(TILE_HEIGHT)
            .w(width)
            .overflow_hidden()
            .rounded_sm();
        match self {
            TileArt::Window(swatch) => base
                .flex()
                .bg(swatch.background)
                .child(
                    div()
                        .w(SIDEBAR_WIDTH)
                        .h_full()
                        .rounded_l_sm()
                        .bg(swatch.panel),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(5.))
                        .pl(px(6.))
                        .child(underline(swatch))
                        .child(bars(swatch)),
                ),
            TileArt::Split { dark, light } => base
                .bg(linear_gradient(
                    corner_angle(width, TILE_HEIGHT),
                    // Stops apart by a hair: the same stop twice is a division
                    // by nothing in the shader.
                    linear_color_stop(dark.background, 0.5),
                    linear_color_stop(light.background, 0.502),
                ))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left(px(6.))
                        .flex()
                        .flex_col()
                        .gap(px(5.))
                        .child(underline(dark))
                        .child(bars(dark)),
                )
                .child(
                    div()
                        .absolute()
                        .bottom(px(5.))
                        .right(px(6.))
                        .flex()
                        .flex_col()
                        .items_end()
                        .gap(px(5.))
                        .child(bars(light).items_end())
                        .child(underline(light)),
                ),
        }
    }
}

/// The families the Font page lists, for the wizard's model: those of
/// [`fonts::MONO_FAMILIES`] that `installed` says are, in their order. It asks
/// by family, as listing every installed font is slow and would offer them all.
pub(crate) fn font_families(installed: impl Fn(&'static str) -> bool) -> Vec<&'static str> {
    (fonts::MONO_FAMILIES.into_iter())
        .filter(|family| installed(family))
        .collect()
}

/// [`font_families`], as the text system has them.
pub(crate) fn installed_font_families(cx: &App) -> Vec<&'static str> {
    font_families(|family| fonts::first_installed(&[family], cx).is_some())
}

/// The family a font's option is drawn in: "Default (Spline Sans Mono)" is
/// Spline Sans Mono's.
pub(crate) fn label_family(label: &str) -> SharedString {
    let default = label.strip_prefix("Default (");
    let family = default.and_then(|label| label.strip_suffix(')'));
    SharedString::from(family.unwrap_or(label).to_owned())
}

/// What `theme` was given by [`EditorTheme::set_metrics`], for
/// [`EditorView::apply_settings`], which only takes it this way.
fn metrics_of(theme: &EditorTheme) -> EditorSettings {
    EditorSettings {
        font_family: Some(theme.font_family.to_string()),
        font_ligatures: theme.font_features == gpui::FontFeatures::default(),
        font_size: f64::from(f32::from(theme.font_size)),
        line_height: f64::from(theme.line_height),
        indent_guides: theme.indent_guides,
        bracket_pair_colors: theme.bracket_pair_colors,
        sticky_scroll: theme.sticky_scroll.clone(),
        render_whitespace: theme.render_whitespace,
        rulers: theme.rulers.clone(),
        inline_diagnostics: theme.inline_diagnostics,
        ..EditorSettings::default()
    }
}

/// The code sample and what keeps it up with the window's theme and font.
pub(crate) struct Sample {
    editor: Entity<EditorView>,
    _follow: [Subscription; 2],
}

impl Sample {
    pub(crate) fn new<T: 'static>(pane: &Entity<Pane>, cx: &mut Context<T>) -> Self {
        let mut model = EditorModel::new(Buffer::new(SAMPLE), None);
        // No caret shows without focus, and without this no band for it either.
        model.set_cursor_line(false);
        let theme = pane.read(cx).theme().clone();
        let editor = cx.new(|cx| {
            let mut view = EditorView::new(model, theme, cx);
            view.set_read_only(true, cx);
            view.set_gutter(false, cx);
            view.set_language(Some("rust"), cx);
            view
        });
        let palette = cx.observe_global::<ActivePalette>({
            let editor = editor.clone();
            move |_, cx| {
                let palette = fe_gpui::editor_palette(cx);
                editor.update(cx, |editor, cx| editor.set_colors(&palette, cx));
            }
        });
        let metrics = cx.observe(pane, {
            let editor = editor.clone();
            move |_, pane, cx| {
                let metrics = metrics_of(pane.read(cx).theme());
                if metrics != metrics_of(editor.read(cx).theme()) {
                    editor.update(cx, |editor, cx| editor.apply_settings(&metrics, cx));
                }
            }
        });
        Self {
            editor,
            _follow: [palette, metrics],
        }
    }

    #[cfg(test)]
    pub(crate) fn editor(&self) -> &Entity<EditorView> {
        &self.editor
    }

    /// The sample, under a layer that keeps every mouse event from it: it
    /// neither takes focus nor scrolls nor selects.
    pub(crate) fn render(&self, cx: &App) -> Div {
        div()
            .relative()
            .size_full()
            .overflow_hidden()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().colors.border.default)
            .child(self.editor.clone())
            .child(div().absolute().top_0().left_0().size_full().occlude())
    }
}
