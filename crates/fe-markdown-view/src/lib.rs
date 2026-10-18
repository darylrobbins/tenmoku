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
//! Test builds give elements `debug_selector`s: `md-block-<ix>` for each
//! top-level block; `md-image-<n>` and `md-image-blocked-<n>` for the
//! document's `n`th image, with `md-image-<n>-loading` and
//! `md-image-<n>-failed` for the alt text shown in its place; and
//! `md-task-<n>` for the `n`th task checkbox.

mod blocks;
mod image;
mod inlines;
mod style;
mod table;
mod view;

pub use style::MarkdownStyle;
pub use view::{MarkdownView, MarkdownViewEvent};
