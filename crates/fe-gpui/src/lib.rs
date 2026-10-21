//! GPUI front end for the headless editor in `fe-editor`.
//!
//! [`EditorView`] owns an [`fe_editor::EditorModel`] and turns input into
//! commands, run through `fe-command`'s registries ([`commands`]); keys are
//! matched by `fe-keymap` ([`keymap`]), which also gives the menu bar
//! ([`menu`]) its shortcuts;
//! [`element::EditorElement`] lays out the visible rows with
//! [`fe_display::layout`], shaping them with GPUI and colouring them by
//! `fe-syntax`, then paints the result, with the buffer's diagnostics as
//! squiggles and gutter marks, and their messages in a popover on hover
//! (`hover`), with what the host's [`HoverProvider`] says about the word
//! there, or at the caret on `editor.show_hover`, and a completion menu
//! (`completion_menu`) of what the host's [`CompletionProvider`] offers at
//! the caret, and signature help (`signature_help`) from the host's
//! [`SignatureHelpProvider`] above the caret while a call's arguments are
//! typed. Vim mode, one app-wide switch
//! ([`set_vim_enabled`]), routes an editor's keys through `fe-vim`. An
//! editor is a kiln-shell pane item ([`pane_item`]).

pub mod commands;
pub mod completion_menu;
pub mod diff_view;
mod element;
mod file_viewer;
mod gutter_annotations;
mod hover;
pub mod inlay_hints;
pub mod keymap;
pub mod lens;
pub mod menu;
pub mod pane_item;
pub mod signature_help;
mod theme;
mod timing;
mod view;
mod vim;

pub use completion_menu::{CompletionFuture, CompletionProvider, ResolveFuture};
pub use diff_view::{DiffSide, DiffView};
pub use fe_config::{EditorSettings, SettingsEvent, theme::EditorPalette};
use fe_editor::diagnostics::Severity;
pub use fe_keymap;
use fe_syntax::Highlight;
pub use file_viewer::{FileViewer, FileViewerEvent};
use gpui::{App, Hsla, Pixels, SharedString, px};
pub use gutter_annotations::{GutterAnnotation, GutterAnnotations};
pub use hover::{HoverContent, HoverFuture, HoverProvider, MarkdownRenderer};
pub use inlay_hints::{InlayHint, InlayHintFuture, InlayHintHost, InlayHintKind};
pub use lens::{Lens, LensItem, LensProvider};

pub use fe_vim::search::set_time_limit as set_vim_time_limit;
pub use keymap::{default_keymap, keymap_report, set_user_keymap};
pub use pane_item::{SaveFixes, SaveHook, set_restore_style};
pub use signature_help::{
    Parameter, Signature, SignatureContext, SignatureHelp, SignatureHelpFuture,
    SignatureHelpProvider, SignatureTrigger,
};
pub use theme::{ActivePalette, editor_palette, set_editor_palette};
pub use timing::{FrameTiming, FrameTimings};
pub use view::EditorView;
pub use view::GhostTextEvent;
pub use view::ProposalEvent;
pub use view::{GlyphKind, GutterGlyph};
pub use vim::{
    apply_vim_reload, set_host_catalog, set_vim_clipboard, set_vim_enabled, set_vim_jump_on_s,
    set_vim_remaps, set_vim_replace_with_register, vim_enabled, vim_registers,
};

/// Key context active while an editor is focused: the `fe-command`
/// context name, so keymap and command predicates agree.
pub const KEY_CONTEXT: &str = fe_editor::commands::CONTEXT;

/// [`init_with_keymap`] with the built-in keymap alone.
pub fn init(cx: &mut App) {
    init_with_keymap(cx, default_keymap());
}

/// Registers the app's commands (`app.quit`) and routes every keystroke
/// through `keymap`, dispatching what it binds as a
/// [`commands::RunCommand`]. `keymap` is the built-in
/// [`fe_keymap::DEFAULT_MACOS`] with the user's layer, if any, over it, as
/// [`fe_keymap::KeymapStore`] loads it. Swap it with [`set_user_keymap`].
pub fn init_with_keymap(cx: &mut App, keymap: impl Into<std::sync::Arc<fe_keymap::Keymap>>) {
    commands::init(cx);
    keymap::init(keymap.into(), cx);
}

/// The keymap keys are matched against, the defaults with the user's layer
/// over them: for menus' shortcuts ([`menu`]) and tooltips. Empty before
/// [`init`].
pub fn current_keymap(cx: &App) -> std::sync::Arc<fe_keymap::Keymap> {
    keymap::current(cx)
}

/// Colours and metrics for the editor. The metrics come from
/// [`EditorSettings`], the colours from an [`EditorPalette`]
/// ([`EditorTheme::from_palette`], [`EditorView::set_colors`]).
#[derive(Clone, Debug, PartialEq)]
pub struct EditorTheme {
    pub font_family: SharedString,
    /// Whether the font's ligatures are shaped (`calt` on) or not.
    pub font_features: gpui::FontFeatures,
    pub font_size: Pixels,
    /// Line height as a multiple of the font size.
    pub line_height: f32,
    pub background: Hsla,
    pub foreground: Hsla,
    pub gutter_foreground: Hsla,
    pub gutter_active_foreground: Hsla,
    pub active_line: Hsla,
    pub selection: Hsla,
    pub cursor: Hsla,
    /// Each diagnostic severity's squiggle and gutter mark.
    pub error: Hsla,
    pub warning: Hsla,
    pub information: Hsla,
    pub hint: Hsla,
    /// The hover popover's.
    pub popover_background: Hsla,
    pub popover_border: Hsla,
    /// A vim jump's live label's box.
    pub jump_label: Hsla,
    /// A vim jump's preview and concealed labels' box.
    pub jump_label_preview: Hsla,
    /// The glyph on a jump label's box.
    pub jump_label_foreground: Hsla,
    /// The git change bar of added rows.
    pub change_added: Hsla,
    /// The git change bar of modified rows.
    pub change_modified: Hsla,
    /// The git change mark where rows were deleted.
    pub change_deleted: Hsla,
    /// A vertical line at an indent level.
    pub indent_guide: Hsla,
    /// The indent guide of the block the caret is in.
    pub indent_guide_active: Hsla,
    /// The vim mode's status bar segment in normal mode, operator pending
    /// and while a jump is pending.
    pub vim_mode_normal: Hsla,
    /// The vim mode's segment in insert mode.
    pub vim_mode_insert: Hsla,
    /// The vim mode's segment in the three visual modes.
    pub vim_mode_visual: Hsla,
    /// The vim mode's segment in replace mode.
    pub vim_mode_replace: Hsla,
    /// The segment on vim's command line.
    pub vim_mode_command: Hsla,
    /// The label on every vim mode's segment.
    pub vim_mode_foreground: Hsla,
    /// Whether indent guides are drawn, from `editor.indent_guides`.
    pub indent_guides: bool,
    /// The brackets of pairs by nesting depth, outermost first, round again
    /// past the third.
    pub brackets: [Hsla; 3],
    /// Whether bracket pairs take `brackets`, from
    /// `editor.bracket_pair_colors`.
    pub bracket_pair_colors: bool,
    /// Behind the minimap.
    pub minimap_background: Hsla,
    /// The minimap's slider over the visible rows: at rest, under the
    /// pointer and while it's dragged.
    pub minimap_slider: Hsla,
    pub minimap_slider_hover: Hsla,
    pub minimap_slider_active: Hsla,
    /// The minimap's marks for the selection, find matches, and error and
    /// warning diagnostics.
    pub minimap_selection: Hsla,
    pub minimap_find_match: Hsla,
    pub minimap_error: Hsla,
    pub minimap_warning: Hsla,
    /// The minimap's git change marks.
    pub minimap_gutter_added: Hsla,
    pub minimap_gutter_modified: Hsla,
    pub minimap_gutter_deleted: Hsla,
    /// Text colour for each [`Highlight`], by [`Highlight::index`]; `None`
    /// uses `foreground`. Build it with [`fe_syntax::resolve_theme`].
    pub syntax: Vec<Option<Hsla>>,
}

impl EditorTheme {
    /// fastedit Dark with the default settings: the first installed font
    /// of a preferred list.
    pub fn default_dark(cx: &App) -> Self {
        Self::from_settings(&EditorSettings::default(), cx)
    }

    /// fastedit Dark with the font and metrics of `settings`.
    pub fn from_settings(settings: &EditorSettings, cx: &App) -> Self {
        Self::from_palette(EditorPalette::fastedit_dark(), settings, cx)
    }

    /// Takes the font family, size and line height from `settings`, and
    /// whether indent guides are drawn and bracket pairs coloured.
    pub fn set_metrics(&mut self, settings: &EditorSettings, cx: &App) {
        self.font_family = font_family(settings, cx);
        self.font_features = if settings.font_ligatures {
            gpui::FontFeatures::default()
        } else {
            gpui::FontFeatures::disable_ligatures()
        };
        self.font_size = px(settings.font_size as f32);
        self.line_height = settings.line_height as f32;
        self.indent_guides = settings.indent_guides;
        self.bracket_pair_colors = settings.bracket_pair_colors;
    }

    /// The editor's font: `font_family` with `font_features`.
    pub fn font(&self) -> gpui::Font {
        let mut font = gpui::font(self.font_family.clone());
        font.features = self.font_features.clone();
        font
    }

    /// The colour of a bracket pair `depth` pairs deep.
    pub fn bracket_color(&self, depth: usize) -> Hsla {
        self.brackets[depth % self.brackets.len()]
    }

    pub fn syntax_color(&self, highlight: Highlight) -> Option<Hsla> {
        self.syntax.get(highlight.index()).copied().flatten()
    }

    /// The colour diagnostics of `severity` are marked in.
    pub fn diagnostic_color(&self, severity: Severity) -> Hsla {
        match severity {
            Severity::Error => self.error,
            Severity::Warning => self.warning,
            Severity::Information => self.information,
            Severity::Hint => self.hint,
        }
    }
}

/// The font family `settings` names, or else the first installed of a
/// preferred list.
fn font_family(settings: &EditorSettings, cx: &App) -> SharedString {
    if let Some(family) = &settings.font_family {
        return family.clone().into();
    }
    kiln_platform::fonts::mono_family(cx)
}

#[cfg(test)]
mod command_tests;
#[cfg(test)]
mod fold_gutter_tests;
#[cfg(test)]
mod font_ligatures_tests;
#[cfg(test)]
mod keymap_tests;
#[cfg(test)]
mod menu_tests;
#[cfg(test)]
mod settings_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod theme_tests;
#[cfg(test)]
#[path = "vim_key_fallback_tests.rs"]
mod vim_key_fallback_tests;
