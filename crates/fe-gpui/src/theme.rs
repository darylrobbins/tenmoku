//! The editor's colours from an [`EditorPalette`]: building and recolouring
//! an [`EditorTheme`], and the palette in use, which the host sets from its
//! theme ([`set_editor_palette`]). fe-gpui doesn't read kiln's theme
//! itself: the host turns it into a palette.

use std::sync::Arc;

use fe_config::theme::{Color, EditorPalette};
use gpui::{App, Context, Global, Hsla, Pixels, SharedString, px, rgba};

use crate::{EditorSettings, EditorTheme, EditorView};

impl EditorTheme {
    /// `palette`'s colours with the font and metrics of `settings`.
    pub fn from_palette(palette: &EditorPalette, settings: &EditorSettings, cx: &App) -> Self {
        let mut theme = Self::colored(palette, SharedString::default(), px(13.), 1.6);
        theme.set_metrics(settings, cx);
        theme
    }

    /// Takes every colour from `palette`, leaving the metrics alone.
    pub fn set_colors(&mut self, palette: &EditorPalette) {
        let font_family = std::mem::take(&mut self.font_family);
        let font_features = self.font_features.clone();
        let (indent_guides, bracket_pair_colors) = (self.indent_guides, self.bracket_pair_colors);
        *self = Self::colored(palette, font_family, self.font_size, self.line_height);
        self.font_features = font_features;
        self.indent_guides = indent_guides;
        self.bracket_pair_colors = bracket_pair_colors;
    }

    /// `palette`'s colours with the given metrics.
    fn colored(
        palette: &EditorPalette,
        font_family: SharedString,
        font_size: Pixels,
        line_height: f32,
    ) -> Self {
        let editor = &palette.editor;
        let syntax: Vec<(&str, Hsla)> = (palette.syntax.iter())
            .map(|(name, color)| (name.as_str(), hsla(*color)))
            .collect();
        Self {
            font_family,
            font_features: gpui::FontFeatures::default(),
            font_size,
            line_height,
            background: hsla(editor.background),
            foreground: hsla(editor.foreground),
            gutter_foreground: hsla(editor.gutter_foreground),
            gutter_active_foreground: hsla(editor.gutter_active_foreground),
            active_line: hsla(editor.active_line),
            selection: hsla(editor.selection),
            cursor: hsla(editor.cursor),
            error: hsla(editor.error),
            warning: hsla(editor.warning),
            information: hsla(editor.information),
            hint: hsla(editor.hint),
            popover_background: hsla(editor.popover_background),
            popover_border: hsla(editor.popover_border),
            jump_label: hsla(editor.jump_label),
            jump_label_preview: hsla(editor.jump_label_preview),
            jump_label_foreground: hsla(editor.jump_label_foreground),
            change_added: hsla(editor.change_added),
            change_modified: hsla(editor.change_modified),
            change_deleted: hsla(editor.change_deleted),
            indent_guide: hsla(editor.indent_guide),
            indent_guide_active: hsla(editor.indent_guide_active),
            vim_mode_normal: hsla(editor.vim_mode_normal),
            vim_mode_insert: hsla(editor.vim_mode_insert),
            vim_mode_visual: hsla(editor.vim_mode_visual),
            vim_mode_replace: hsla(editor.vim_mode_replace),
            vim_mode_command: hsla(editor.vim_mode_command),
            vim_mode_foreground: hsla(editor.vim_mode_foreground),
            indent_guides: true,
            brackets: [editor.bracket_1, editor.bracket_2, editor.bracket_3].map(hsla),
            bracket_pair_colors: false,
            syntax: fe_syntax::resolve_theme(&syntax),
        }
    }
}

fn hsla(color: Color) -> Hsla {
    rgba(color.to_rgba_u32()).into()
}

impl EditorView {
    /// Recolours the editor with `palette`, keeping its font and metrics.
    pub fn set_colors(&mut self, palette: &EditorPalette, cx: &mut Context<Self>) {
        self.theme.set_colors(palette);
        cx.notify();
    }

    /// The editor's colours and metrics.
    pub fn theme(&self) -> &EditorTheme {
        &self.theme
    }
}

/// The editor colours in use, as a GPUI global the host sets
/// ([`set_editor_palette`]) from its theme.
pub struct ActivePalette(pub Arc<EditorPalette>);

impl Global for ActivePalette {}

/// Makes `palette` the editor colours in use, notifying observers of
/// [`ActivePalette`].
pub fn set_editor_palette(cx: &mut App, palette: Arc<EditorPalette>) {
    cx.set_global(ActivePalette(palette));
}

/// The editor colours in use: the last [`set_editor_palette`], or fastedit
/// Dark.
pub fn editor_palette(cx: &App) -> Arc<EditorPalette> {
    match cx.try_global::<ActivePalette>() {
        Some(ActivePalette(palette)) => palette.clone(),
        None => Arc::new(EditorPalette::fastedit_dark().clone()),
    }
}
