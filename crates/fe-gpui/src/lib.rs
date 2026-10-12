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
//! the caret. Vim mode, one app-wide switch
//! ([`set_vim_enabled`]), routes an editor's keys through `fe-vim`. An
//! editor is a kiln-shell pane item ([`pane_item`]).

pub mod commands;
pub mod completion_menu;
mod element;
mod file_viewer;
mod hover;
pub mod keymap;
pub mod menu;
pub mod pane_item;
mod timing;
mod view;
mod vim;

pub use completion_menu::{CompletionFuture, CompletionProvider, ResolveFuture};
pub use fe_config::{EditorSettings, SettingsEvent};
use fe_editor::diagnostics::Severity;
pub use fe_keymap;
use fe_syntax::Highlight;
pub use file_viewer::{FileViewer, FileViewerEvent};
use gpui::{App, Hsla, Pixels, SharedString, px, rgb, rgba};
pub use hover::{HoverContent, HoverFuture, HoverProvider};

pub use keymap::{default_keymap, keymap_report, set_user_keymap};
pub use pane_item::set_restore_style;
pub use timing::{FrameTiming, FrameTimings};
pub use view::EditorView;
pub use vim::{
    apply_vim_reload, set_vim_clipboard, set_vim_enabled, set_vim_jump_on_s, vim_enabled,
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
/// [`EditorSettings`]; the colours are hard-coded until themes land (M5).
#[derive(Clone, Debug, PartialEq)]
pub struct EditorTheme {
    pub font_family: SharedString,
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
    /// Text colour for each [`Highlight`], by [`Highlight::index`]; `None`
    /// uses `foreground`. Build it with [`fe_syntax::resolve_theme`].
    pub syntax: Vec<Option<Hsla>>,
}

impl EditorTheme {
    /// The dark theme with the default settings: the first installed font
    /// of a preferred list.
    pub fn default_dark(cx: &App) -> Self {
        Self::from_settings(&EditorSettings::default(), cx)
    }

    /// The dark theme with the font and metrics of `settings`.
    pub fn from_settings(settings: &EditorSettings, cx: &App) -> Self {
        let mut theme = Self {
            font_family: SharedString::default(),
            font_size: px(13.),
            line_height: 1.6,
            background: rgb(0x16181d).into(),
            foreground: rgb(0xc8ccd4).into(),
            gutter_foreground: rgb(0x4b5160).into(),
            gutter_active_foreground: rgb(0xa0a7b4).into(),
            active_line: rgba(0xffffff08).into(),
            selection: rgba(0x4f7cc855).into(),
            cursor: rgb(0x7aa2f7).into(),
            error: rgb(0xe06c75).into(),
            warning: rgb(0xe5c07b).into(),
            information: rgb(0x61afef).into(),
            hint: rgb(0x7f848e).into(),
            popover_background: rgb(0x21252b).into(),
            popover_border: rgb(0x3e4452).into(),
            syntax: fe_syntax::resolve_theme(&[
                ("attribute", rgb(0xd19a66).into()),
                ("comment", rgb(0x5c6370).into()),
                ("constant", rgb(0xd19a66).into()),
                ("constructor", rgb(0xe5c07b).into()),
                ("escape", rgb(0x56b6c2).into()),
                ("function", rgb(0x61afef).into()),
                ("function.macro", rgb(0x56b6c2).into()),
                ("keyword", rgb(0xc678dd).into()),
                ("label", rgb(0xe06c75).into()),
                ("markup.heading", rgb(0xe06c75).into()),
                ("markup.link", rgb(0x61afef).into()),
                ("markup.link.url", rgb(0x56b6c2).into()),
                ("markup.raw", rgb(0x98c379).into()),
                ("module", rgb(0xe5c07b).into()),
                ("number", rgb(0xd19a66).into()),
                ("operator", rgb(0x56b6c2).into()),
                ("property", rgb(0xe06c75).into()),
                ("punctuation", rgb(0x7f848e).into()),
                ("punctuation.special", rgb(0x56b6c2).into()),
                ("string", rgb(0x98c379).into()),
                ("string.special", rgb(0x56b6c2).into()),
                ("string.special.key", rgb(0xe06c75).into()),
                ("tag", rgb(0xe06c75).into()),
                ("type", rgb(0xe5c07b).into()),
                ("variable.builtin", rgb(0xe06c75).into()),
            ]),
        };
        theme.set_metrics(settings, cx);
        theme
    }

    /// Takes the font family, size and line height from `settings`.
    pub fn set_metrics(&mut self, settings: &EditorSettings, cx: &App) {
        self.font_family = font_family(settings, cx);
        self.font_size = px(settings.font_size as f32);
        self.line_height = settings.line_height as f32;
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
mod keymap_tests;
#[cfg(test)]
mod menu_tests;
#[cfg(test)]
mod settings_tests;
#[cfg(test)]
mod tests;
