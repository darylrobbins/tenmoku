//! The GPUI view of an `fe-markdown` document: blocks, inlines, highlighted
//! code, tables, task lists and images, in the editor's theme.
//!
//! - [`MarkdownStyle`] takes its colours and code font from the editor's
//!   [`fe_gpui::EditorTheme`] and the chrome's [`kiln_ui::UiTheme`], so a
//!   light editor theme gives a light preview.
//! - [`MarkdownView`] draws a [`fe_markdown::Document`] in a scrolling
//!   column, as VS Code's preview does. Local images inside the document's
//!   folder or the workspace are drawn, unless an SVG among them draws a
//!   file outside; remote ones are blocked and never fetched. A link
//!   click is a [`MarkdownViewEvent`]; acting on it is the host's job.
//!
//! - [`markdown_element`] lays a document out statelessly in a narrow,
//!   unpadded column for hover popovers and completion docs: no scroll, no
//!   focus, images shown as their alt text, link clicks to a callback.
//! - [`document_element`] lays a document's blocks out as the view does,
//!   with no scroll or width cap, for a host that places documents in a
//!   scrolling view of its own: a chat's replies. Images are loaded or
//!   shown as their alt text, link clicks go to a callback, and a host can
//!   draw a header over each code block and be told where each code block
//!   is drawn ([`ElementOptions::on_code_bounds`]), to pin a copy of its
//!   header while the block is scrolled under the top of its view. A host
//!   can also draw a code block's body in place of its code text
//!   ([`ElementOptions::code_body`]), such as an edit's sections as a
//!   diff, keeping syntax colours with [`MarkdownStyle::code_runs_in`].
//!   [`ElementOptions::default`] shows images as their alt text and has no
//!   callbacks.
//!
//! Test builds give elements `debug_selector`s: `md-block-<ix>` for each
//! top-level block; `md-image-<n>` and `md-image-blocked-<n>` for the
//! document's `n`th image, with `md-image-<n>-loading` and
//! `md-image-<n>-failed` for the alt text shown in its place, and
//! `md-image-alt-<n>` for [`Images::AltText`]'s; `md-task-<n>` for the
//! `n`th task checkbox; and `md-code-<n>` for the `n`th code block, with
//! `md-code-<n>-text` for its text, as wide as its longest line.

mod blocks;
mod compact;
mod image;
mod inlines;
mod style;
mod table;
mod view;

use gpui::{App, Global, TextRun};

pub use blocks::{
    CodeBody, CodeBodyFn, CodeBoundsFn, CodeHeader, CodeHeaderFn, ElementOptions, Images,
    document_element,
};
pub use compact::markdown_element;
pub use style::MarkdownStyle;
pub use view::LinkHandler;
pub use view::{MarkdownView, MarkdownViewEvent};

/// A test seam: the text and [`TextRun`]s of each `StyledText` the renderer
/// builds, for asserting the colours that are painted, which a headless
/// GPUI test cannot read back.
///
/// Nothing is recorded, and the renderer does nothing extra, unless the
/// Global is installed with [`PaintedRuns::set`]. A frame records every
/// styled text it builds, so [`PaintedRuns::take`] after the draw you mean.
#[derive(Default)]
pub struct PaintedRuns(Vec<(String, Vec<TextRun>)>);

impl Global for PaintedRuns {}

impl PaintedRuns {
    /// Starts recording, dropping anything recorded before.
    pub fn set(cx: &mut App) {
        cx.set_global(Self::default());
    }

    /// What has been recorded since the last take, in build order; empty
    /// if not recording.
    pub fn take(cx: &mut App) -> Vec<(String, Vec<TextRun>)> {
        if cx.has_global::<Self>() {
            std::mem::take(&mut cx.global_mut::<Self>().0)
        } else {
            Vec::new()
        }
    }

    /// Stops recording.
    pub fn clear(cx: &mut App) {
        if cx.has_global::<Self>() {
            cx.remove_global::<Self>();
        }
    }

    /// Records `text` with `runs` if recording.
    pub(crate) fn record(cx: &mut App, text: &str, runs: &[TextRun]) {
        if cx.has_global::<Self>() {
            cx.global_mut::<Self>()
                .0
                .push((text.to_owned(), runs.to_vec()));
        }
    }
}
