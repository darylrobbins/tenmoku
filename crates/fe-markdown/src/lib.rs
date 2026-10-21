//! Markdown documents parsed for the preview, with the source lines each
//! block spans so the preview can scroll in step with the editor.
//!
//! - [`parse`] reads CommonMark plus GitHub's tables, task lists,
//!   strikethrough and autolink literals into a [`Document`]. It's pure and
//!   the document is `Send`, so it can run on a background thread.
//! - Fenced code is highlighted with fe-syntax, by its fence's language.
//! - [`link_target`] and [`image_source`] say where a link or image points,
//!   as VS Code's preview opens and loads them.
//!
//! This crate must never depend on GPUI.

use std::ops::Range;

mod autolink;
mod code;
mod html_inline;
mod inline;
mod parse;
mod slug;
mod target;

pub use target::{BlockReason, ImageSource, LinkTarget, image_source, link_target};

/// A parsed Markdown document.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Document {
    pub blocks: Vec<Block>,
}

/// A block and the 0-based source rows it spans, end exclusive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    pub lines: Range<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Paragraph(Vec<Inline>),
    Heading {
        /// 1 to 6.
        level: u8,
        /// The heading's anchor, as GitHub and VS Code make it, numbered
        /// when an earlier heading has the same one.
        slug: String,
        content: Vec<Inline>,
    },
    Quote(Vec<Block>),
    List {
        /// The first number of an ordered list; `None` for a bullet list.
        start: Option<u64>,
        /// Whether no item is separated from the next by a blank line, so
        /// the items' paragraphs are shown without spacing.
        tight: bool,
        items: Vec<ListItem>,
    },
    Code {
        /// The fence's info string; empty for an indented block.
        info: String,
        /// The name of the fe-syntax language the info string names.
        language: Option<&'static str>,
        /// The code, with `\n` line endings.
        code: String,
        /// Byte ranges of `code` and their highlights.
        highlights: Vec<fe_syntax::HighlightSpan>,
    },
    Table {
        align: Vec<Align>,
        head: Vec<Cell>,
        rows: Vec<Row>,
    },
    Rule,
    /// Raw HTML, as its source text. The preview shows it escaped.
    Html(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListItem {
    /// `Some(checked)` for a `[x]` or `[ ]` task item.
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
    pub lines: Range<u32>,
}

/// A table column's alignment.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Align {
    None,
    Left,
    Center,
    Right,
}

/// A table cell's content.
pub type Cell = Vec<Inline>;

/// A table body row and the source rows it spans.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub cells: Vec<Cell>,
    pub lines: Range<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inline {
    Text(String),
    Code(String),
    Emphasis(Vec<Inline>),
    Strong(Vec<Inline>),
    Strikethrough(Vec<Inline>),
    /// `dest` is as written; see [`link_target`]. An e-mail autolink's has
    /// `mailto:` in front, and a `www.` literal's `http://`.
    Link {
        dest: String,
        title: String,
        content: Vec<Inline>,
    },
    /// `dest` is as written; see [`image_source`].
    Image {
        dest: String,
        title: String,
        alt: String,
    },
    SoftBreak,
    HardBreak,
    /// Inline raw HTML, as its source text.
    Html(String),
}

/// Parses `text` as CommonMark with GitHub's tables, task lists,
/// strikethrough and autolink literals. Footnotes, math, smart punctuation
/// and heading attributes are off, as in VS Code's preview.
pub fn parse(text: &str) -> Document {
    parse::parse(text)
}

impl Document {
    /// The first heading an `#anchor` link names: [`LinkTarget::Anchor`]'s
    /// slug, matched as written and then lowercased.
    pub fn heading(&self, slug: &str) -> Option<&Block> {
        let lower = slug.to_lowercase();
        self.headings()
            .find(|(heading, _)| *heading == slug)
            .or_else(|| self.headings().find(|(heading, _)| *heading == lower))
            .map(|(_, block)| block)
    }

    /// Every heading's slug and block, in document order, including those
    /// in quotes and lists.
    pub fn headings(&self) -> impl Iterator<Item = (&str, &Block)> {
        let mut stack = vec![self.blocks.iter()];
        std::iter::from_fn(move || {
            loop {
                let block = match stack.last_mut()?.next() {
                    Some(block) => block,
                    None => {
                        stack.pop();
                        continue;
                    }
                };
                match &block.kind {
                    BlockKind::Heading { slug, .. } => return Some((slug.as_str(), block)),
                    BlockKind::Quote(blocks) => stack.push(blocks.iter()),
                    BlockKind::List { items, .. } => {
                        // Items' blocks in order: push them last first.
                        for item in items.iter().rev() {
                            stack.push(item.blocks.iter());
                        }
                    }
                    _ => {}
                }
            }
        })
    }
}

impl Inline {
    /// The text these show, without markup or raw HTML: an image as its alt
    /// text, a break as a space.
    pub fn plain_text(inlines: &[Inline]) -> String {
        let mut text = String::new();
        push_plain_text(inlines, &mut text);
        text
    }
}

fn push_plain_text(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Text(text) | Inline::Code(text) => out.push_str(text),
            Inline::Emphasis(content)
            | Inline::Strong(content)
            | Inline::Strikethrough(content)
            | Inline::Link { content, .. } => push_plain_text(content, out),
            Inline::Image { alt, .. } => out.push_str(alt),
            Inline::SoftBreak | Inline::HardBreak => out.push(' '),
            // Tags aren't text, and what they enclose comes as text of its own.
            Inline::Html(_) => {}
        }
    }
}
