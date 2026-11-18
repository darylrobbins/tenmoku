//! [`PdfNotice`]: a small tab that says a file is a PDF. Detection only, no
//! rendering, the fallback for the PDF plugin: Tenmoku opens a PDF in this
//! view because it can't show one, and it offers the two things it can do
//! with the file, **Open in Default App** (`pdf.open_in_default_app`, which
//! the workbench runs through the OS's handler) and **Open as Hex**
//! (`hex.open_as_hex`, which replaces the tab in place).
//!
//! This view is the place the install prompt and the PDF plugin's own view
//! later plug in (`docs/decisions/*-plugin-viewers.md`); neither is built
//! here, and no PDF crate is a dependency of core. The notice shows the
//! file's name and size and the line "This is a PDF. Tenmoku can't show PDFs
//! yet.", centred in the editor background. It's never dirty and
//! read-only; a saved layout keeps its path, and a file that's gone isn't
//! restored. See `pdf_notice/pane_item.rs`.

use std::path::{Path, PathBuf};

use fe_command::{ContextFrame, ContextStack, Invocation, Source};
use fe_ui::Button;
use gpui::{
    App, Context, FocusHandle, Focusable, InteractiveElement as _, IntoElement, KeyContext,
    ParentElement as _, Render, Styled as _, Window, div, px,
};
use kiln_l10n::{text, tr, verbatim};

use crate::{EditorTheme, commands::RunCommand, image_viewer::size_text};

pub(crate) mod pane_item;

/// The key context of the notice, which its commands run in.
pub const KEY_CONTEXT: &str = verbatim!("PdfNotice");

/// Opens the file in the OS's handler for it. The workbench runs it.
pub const OPEN_IN_DEFAULT_APP: &str = "pdf.open_in_default_app";
/// Replaces the notice with a hex viewer of the file. The workbench runs it.
pub(crate) const OPEN_AS_HEX: &str = "hex.open_as_hex";

/// A tab saying that `path` is a PDF.
pub struct PdfNotice {
    path: PathBuf,
    theme: EditorTheme,
    focus: FocusHandle,
    /// The file's size when the notice was made, if it could be told.
    len: Option<u64>,
}

impl PdfNotice {
    pub fn new(path: PathBuf, theme: EditorTheme, cx: &mut Context<Self>) -> Self {
        let len = std::fs::metadata(&path).ok().map(|meta| meta.len());
        Self {
            path,
            theme,
            focus: cx.focus_handle(),
            len,
        }
    }

    /// The PDF's path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The PDF's size in bytes, if it could be told.
    pub fn file_len(&self) -> Option<u64> {
        self.len
    }
}

impl Focusable for PdfNotice {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// Runs `command` from the notice, where its context holds.
fn run(command: &str, focus: &FocusHandle, window: &mut Window, cx: &mut App) {
    window.focus(focus, cx);
    let invocation = Invocation::of(command)
        .expect("a valid command ID")
        .source(Source::Mouse)
        .context(ContextStack::new().with(ContextFrame::new(KEY_CONTEXT)));
    window.dispatch_action(Box::new(RunCommand(invocation)), cx);
}

impl Render for PdfNotice {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme.clone();
        let mut key_context = KeyContext::default();
        key_context.add(KEY_CONTEXT);
        let name = crate::pane_item::file_name(Some(&self.path));
        let size = self.len.map(size_text);
        let button = |id: &'static str, label, command: &'static str, primary: bool| {
            let focus = self.focus.clone();
            let button = Button::new(id, label)
                .debug_selector(|| id.into())
                .on_click(move |_, window, cx| run(command, &focus, window, cx));
            if primary { button.primary() } else { button }
        };
        div()
            .key_context(key_context)
            .id("pdf-notice")
            .track_focus(&self.focus)
            .size_full()
            .overflow_hidden()
            .bg(theme.background)
            .text_color(theme.foreground)
            .font_family(theme.font_family.clone())
            .text_size(theme.font_size)
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .child(
                div()
                    .debug_selector(|| verbatim!("pdf-notice-name").into())
                    .child(name),
            )
            .children(size.map(|size| div().text_color(theme.gutter_foreground).child(size)))
            .child(
                div()
                    .debug_selector(|| verbatim!("pdf-notice-says").into())
                    .child(tr!("gpui-pdf-says")),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .pt(px(8.))
                    .child(button(
                        verbatim!("pdf-notice-open-default"),
                        text!("gpui-pdf-open-default"),
                        OPEN_IN_DEFAULT_APP,
                        true,
                    ))
                    .child(button(
                        verbatim!("pdf-notice-open-hex"),
                        text!("gpui-pdf-open-hex"),
                        OPEN_AS_HEX,
                        false,
                    )),
            )
    }
}
