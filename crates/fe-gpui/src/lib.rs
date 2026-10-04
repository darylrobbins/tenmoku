//! GPUI front end for the headless editor in `fe-editor`.
//!
//! [`EditorView`] owns an [`fe_editor::EditorModel`] and turns input into
//! model commands; [`element::EditorElement`] lays out the visible rows with
//! [`fe_display::layout`], shaping them with GPUI and colouring them by
//! `fe-syntax`, then paints the result.

mod element;
mod timing;
mod view;

use fe_syntax::Highlight;
use gpui::{App, Hsla, KeyBinding, Pixels, SharedString, actions, font, px, rgb, rgba};

pub use timing::{FrameTiming, FrameTimings};
pub use view::EditorView;

actions!(
    editor,
    [
        MoveLeft,
        MoveRight,
        MoveUp,
        MoveDown,
        MoveWordLeft,
        MoveWordRight,
        MoveLineStart,
        MoveLineEnd,
        MoveDocumentStart,
        MoveDocumentEnd,
        PageUp,
        PageDown,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        SelectWordLeft,
        SelectWordRight,
        SelectLineStart,
        SelectLineEnd,
        SelectDocumentStart,
        SelectDocumentEnd,
        SelectPageUp,
        SelectPageDown,
        SelectAll,
        Backspace,
        Delete,
        DeleteWordLeft,
        Newline,
        Tab,
        Fold,
        Unfold,
        FoldAll,
        UnfoldAll,
        ToggleSoftWrap,
        Copy,
        Cut,
        Paste,
        Undo,
        Redo,
        Save,
    ]
);

/// Key context active while an editor is focused.
pub const KEY_CONTEXT: &str = "Editor";

/// Registers the editor's default (macOS) key bindings.
pub fn init(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("left", MoveLeft, context),
        KeyBinding::new("right", MoveRight, context),
        KeyBinding::new("up", MoveUp, context),
        KeyBinding::new("down", MoveDown, context),
        KeyBinding::new("alt-left", MoveWordLeft, context),
        KeyBinding::new("alt-right", MoveWordRight, context),
        KeyBinding::new("cmd-left", MoveLineStart, context),
        KeyBinding::new("home", MoveLineStart, context),
        KeyBinding::new("cmd-right", MoveLineEnd, context),
        KeyBinding::new("end", MoveLineEnd, context),
        KeyBinding::new("cmd-up", MoveDocumentStart, context),
        KeyBinding::new("cmd-down", MoveDocumentEnd, context),
        KeyBinding::new("pageup", PageUp, context),
        KeyBinding::new("pagedown", PageDown, context),
        KeyBinding::new("shift-left", SelectLeft, context),
        KeyBinding::new("shift-right", SelectRight, context),
        KeyBinding::new("shift-up", SelectUp, context),
        KeyBinding::new("shift-down", SelectDown, context),
        KeyBinding::new("alt-shift-left", SelectWordLeft, context),
        KeyBinding::new("alt-shift-right", SelectWordRight, context),
        KeyBinding::new("cmd-shift-left", SelectLineStart, context),
        KeyBinding::new("shift-home", SelectLineStart, context),
        KeyBinding::new("cmd-shift-right", SelectLineEnd, context),
        KeyBinding::new("shift-end", SelectLineEnd, context),
        KeyBinding::new("cmd-shift-up", SelectDocumentStart, context),
        KeyBinding::new("cmd-shift-down", SelectDocumentEnd, context),
        KeyBinding::new("shift-pageup", SelectPageUp, context),
        KeyBinding::new("shift-pagedown", SelectPageDown, context),
        KeyBinding::new("cmd-a", SelectAll, context),
        KeyBinding::new("backspace", Backspace, context),
        KeyBinding::new("shift-backspace", Backspace, context),
        KeyBinding::new("delete", Delete, context),
        KeyBinding::new("alt-backspace", DeleteWordLeft, context),
        KeyBinding::new("enter", Newline, context),
        KeyBinding::new("tab", Tab, context),
        KeyBinding::new("cmd-alt-[", Fold, context),
        KeyBinding::new("cmd-alt-]", Unfold, context),
        KeyBinding::new("cmd-k cmd-0", FoldAll, context),
        KeyBinding::new("cmd-k cmd-j", UnfoldAll, context),
        KeyBinding::new("alt-z", ToggleSoftWrap, context),
        KeyBinding::new("cmd-c", Copy, context),
        KeyBinding::new("cmd-x", Cut, context),
        KeyBinding::new("cmd-v", Paste, context),
        KeyBinding::new("cmd-z", Undo, context),
        KeyBinding::new("cmd-shift-z", Redo, context),
        KeyBinding::new("cmd-s", Save, context),
    ]);
}

/// Colours and metrics for the editor. Hard-coded until themes land (M5).
#[derive(Clone, Debug)]
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
    /// Text colour for each [`Highlight`], by [`Highlight::index`]; `None`
    /// uses `foreground`. Build it with [`fe_syntax::resolve_theme`].
    pub syntax: Vec<Option<Hsla>>,
}

impl EditorTheme {
    /// A dark theme using the first installed font of a preferred list.
    pub fn default_dark(cx: &App) -> Self {
        // Resolving a family loads just that family, which the first frame
        // needs anyway; listing every installed font took ~70 ms at launch.
        // A missing family resolves to a fallback font under another name.
        let text_system = cx.text_system();
        let installed = |family: &str| {
            let id = text_system.resolve_font(&font(family));
            text_system
                .get_font_for_id(id)
                .is_some_and(|resolved| resolved.family == family)
        };
        let font_family = ["JetBrains Mono", "SF Mono", "Menlo"]
            .into_iter()
            .find(|family| installed(family))
            .unwrap_or("Menlo");
        Self {
            font_family: font_family.into(),
            font_size: px(13.),
            line_height: 1.6,
            background: rgb(0x16181d).into(),
            foreground: rgb(0xc8ccd4).into(),
            gutter_foreground: rgb(0x4b5160).into(),
            gutter_active_foreground: rgb(0xa0a7b4).into(),
            active_line: rgba(0xffffff08).into(),
            selection: rgba(0x4f7cc855).into(),
            cursor: rgb(0x7aa2f7).into(),
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
        }
    }

    pub fn syntax_color(&self, highlight: Highlight) -> Option<Hsla> {
        self.syntax.get(highlight.index()).copied().flatten()
    }
}

#[cfg(test)]
mod tests;
