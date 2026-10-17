//! pulldown-cmark's events, with their byte ranges, made into a
//! [`Document`] whose blocks know their source lines.

use std::{iter::Peekable, ops::Range};

use pulldown_cmark::{Alignment, CodeBlockKind, Event, LinkType, OffsetIter, Options, Tag};

use crate::{
    Align, Block, BlockKind, Cell, Document, Inline, ListItem, Row, code,
    inline::{self, Open, Tok, Token},
    slug::Slugs,
};

/// How deep quotes, list items and inline markup nest, together, before
/// deeper ones are left out and their content kept: a document of a few
/// thousand `>`s mustn't run a background thread out of stack.
const MAX_NESTING: usize = 64;

pub(crate) fn parse(text: &str) -> Document {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH;
    let events = pulldown_cmark::Parser::new_ext(text, options).into_offset_iter();
    let mut parser = Parser {
        text,
        events: events.peekable(),
        line_starts: line_starts(text),
        slugs: Slugs::default(),
        depth: 0,
        task: None,
        paragraph: false,
    };
    let blocks = parser.blocks();
    Document { blocks }
}

/// Where each source row starts.
fn line_starts(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(ix, _)| ix + 1))
        .collect()
}

/// Reads events into blocks. Text events aren't merged with
/// `TextMergeWithOffset`: each entity or escape is an event of its own, so
/// autolinks can tell its source from its text.
struct Parser<'a> {
    text: &'a str,
    events: Peekable<OffsetIter<'a>>,
    line_starts: Vec<usize>,
    slugs: Slugs,
    /// How many quotes and list items the next block is in.
    depth: usize,
    /// The `[x]` or `[ ]` marker read since the current list item started.
    task: Option<bool>,
    /// Whether the current list item has a paragraph of its own, which
    /// makes its list loose.
    paragraph: bool,
}

impl<'a> Parser<'a> {
    fn row(&self, offset: usize) -> u32 {
        (self.line_starts.partition_point(|&start| start <= offset) - 1) as u32
    }

    /// The 0-based rows a byte range spans, end exclusive, leaving out the
    /// blank lines and line ending it may end with.
    fn lines(&self, range: Range<usize>) -> Range<u32> {
        let trimmed = self.text[range.clone()].trim_end_matches([' ', '\t', '\r', '\n']);
        let start = self.row(range.start);
        let last = self.row(range.start + trimmed.len().max(1) - 1).max(start);
        start..last + 1
    }

    /// [`Parser::lines`], through the last row of content ending at `end`,
    /// such as a code block's blank last rows.
    fn lines_through(&self, range: Range<usize>, end: Option<usize>) -> Range<u32> {
        let lines = self.lines(range);
        match end {
            Some(end) if end > 0 => lines.start..lines.end.max(self.row(end - 1) + 1),
            _ => lines,
        }
    }

    /// A container's lines, through those of its last block.
    fn container_lines(&self, range: Range<usize>, blocks: &[Block]) -> Range<u32> {
        let lines = self.lines(range);
        let end = blocks.last().map_or(lines.end, |block| block.lines.end);
        lines.start..lines.end.max(end)
    }

    /// Blocks up to the end of their container, which is left unread.
    /// Quotes and lists nested deeper than [`MAX_NESTING`] are left out, and
    /// their blocks read as this container's.
    fn blocks(&mut self) -> Vec<Block> {
        let mut blocks = Vec::new();
        // How many of those are open here.
        let mut flattened = 0;
        while let Some((event, _)) = self.events.peek() {
            match event {
                Event::End(_) if flattened == 0 => break,
                Event::End(_) => flattened -= 1,
                Event::Start(Tag::BlockQuote(_) | Tag::List(_) | Tag::Item)
                    if self.depth >= MAX_NESTING =>
                {
                    flattened += 1
                }
                // A tight item's marker comes before its text.
                Event::TaskListMarker(checked) => {
                    self.task = self.task.or(Some(*checked));
                }
                _ => {
                    blocks.extend(self.block());
                    continue;
                }
            }
            self.events.next();
        }
        blocks
    }

    /// Reads the next block. `None` for one this doesn't show, such as a
    /// tag of an extension that is off.
    fn block(&mut self) -> Option<Block> {
        let (event, range) = self.events.peek()?.clone();
        if is_inline(&event) {
            // A tight list item's text, which has no paragraph of its own.
            let (content, span) = self.inlines();
            return Some(Block {
                kind: BlockKind::Paragraph(content),
                lines: self.lines(span?),
            });
        }
        self.events.next();
        let mut lines = self.lines(range.clone());
        let kind = match event {
            Event::Start(Tag::Paragraph) => {
                self.paragraph = true;
                BlockKind::Paragraph(self.inlines_to_end())
            }
            Event::Start(Tag::Heading { level, .. }) => {
                let content = self.inlines_to_end();
                let slug = self.slugs.next(&Inline::plain_text(&content));
                BlockKind::Heading {
                    level: level as u8,
                    slug,
                    content,
                }
            }
            Event::Start(Tag::BlockQuote(_)) => {
                let paragraph = self.paragraph;
                self.depth += 1;
                let blocks = self.blocks();
                self.depth -= 1;
                self.paragraph = paragraph;
                self.events.next();
                lines = self.container_lines(range, &blocks);
                BlockKind::Quote(blocks)
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let (code, end) = self.text_to_end();
                lines = self.lines_through(range, end);
                let code = code.replace("\r\n", "\n");
                match kind {
                    CodeBlockKind::Fenced(info) => {
                        let (language, highlights) = code::highlight(&info, &code);
                        BlockKind::Code {
                            info: info.into_string(),
                            language,
                            code,
                            highlights,
                        }
                    }
                    CodeBlockKind::Indented => BlockKind::Code {
                        info: String::new(),
                        language: None,
                        code,
                        highlights: Vec::new(),
                    },
                }
            }
            Event::Start(Tag::HtmlBlock) => {
                let (html, end) = self.text_to_end();
                lines = self.lines_through(range, end);
                BlockKind::Html(html)
            }
            Event::Start(Tag::List(start)) => {
                let (kind, items_end) = self.list(start);
                lines = lines.start..lines.end.max(items_end);
                kind
            }
            Event::Start(Tag::Table(align)) => self.table(align),
            Event::Rule => BlockKind::Rule,
            Event::Html(html) => BlockKind::Html(html.into_string()),
            Event::Start(_) => {
                self.skip_to_end();
                return None;
            }
            _ => return None,
        };
        Some(Block { kind, lines })
    }

    /// A list, through its end, and the row after its last item.
    fn list(&mut self, start: Option<u64>) -> (BlockKind, u32) {
        let mut items: Vec<ListItem> = Vec::new();
        let mut tight = true;
        while let Some((event, range)) = self.events.next() {
            if !matches!(event, Event::Start(Tag::Item)) {
                // The list's end.
                break;
            }
            // An enclosing item's marker and paragraph are its own.
            let task = self.task.take();
            let paragraph = std::mem::take(&mut self.paragraph);
            self.depth += 1;
            let blocks = self.blocks();
            self.depth -= 1;
            self.events.next();
            tight &= !self.paragraph;
            items.push(ListItem {
                task: self.task.take(),
                lines: self.container_lines(range, &blocks),
                blocks,
            });
            self.task = task;
            self.paragraph = paragraph;
        }
        let end = items.last().map_or(0, |item| item.lines.end);
        (
            BlockKind::List {
                start,
                tight,
                items,
            },
            end,
        )
    }

    fn table(&mut self, align: Vec<Alignment>) -> BlockKind {
        let align = align
            .into_iter()
            .map(|align| match align {
                Alignment::None => Align::None,
                Alignment::Left => Align::Left,
                Alignment::Center => Align::Center,
                Alignment::Right => Align::Right,
            })
            .collect();
        let mut head = Vec::new();
        let mut rows = Vec::new();
        while let Some((event, range)) = self.events.next() {
            match event {
                Event::Start(Tag::TableHead) => head = self.cells(),
                Event::Start(Tag::TableRow) => {
                    let cells = self.cells();
                    rows.push(Row {
                        cells,
                        lines: self.lines(range),
                    });
                }
                Event::Start(_) => self.skip_to_end(),
                // The table's end.
                _ => break,
            }
        }
        BlockKind::Table { align, head, rows }
    }

    /// A row's cells, through its end.
    fn cells(&mut self) -> Vec<Cell> {
        let mut cells = Vec::new();
        while let Some((event, _)) = self.events.next() {
            match event {
                Event::Start(Tag::TableCell) => cells.push(self.inlines_to_end()),
                Event::Start(_) => self.skip_to_end(),
                _ => break,
            }
        }
        cells
    }

    /// Inlines through the end of the tag they're in.
    fn inlines_to_end(&mut self) -> Vec<Inline> {
        let (content, _) = self.inlines();
        self.skip_to_end();
        content
    }

    /// Inlines up to the next end or block, which is left unread, and the
    /// bytes they span.
    fn inlines(&mut self) -> (Vec<Inline>, Option<Range<usize>>) {
        let tokens = self.inline_tokens();
        let span = tokens
            .first()
            .zip(tokens.last())
            .map(|(first, last)| first.range.start..last.range.end);
        let tokens = inline::autolinks(self.text, tokens);
        let depth = MAX_NESTING.saturating_sub(self.depth);
        (inline::build(tokens, depth), span)
    }

    /// The inline events up to the next end or block, as tokens.
    fn inline_tokens(&mut self) -> Vec<Tok<'a>> {
        let mut tokens: Vec<Tok<'a>> = Vec::new();
        // The open tags' tokens, innermost last.
        let mut open: Vec<usize> = Vec::new();
        while let Some((event, _)) = self.events.peek() {
            let ends = matches!(event, Event::End(_));
            if (ends && open.is_empty()) || !(ends || is_inline(event)) {
                break;
            }
            let (event, range) = self.events.next().expect("peeked");
            // The innermost tag's first content ends its opening delimiter.
            if let Some(&tag) = open.last()
                && tokens.len() == tag + 1
                && !ends
            {
                tokens[tag].range.end = range.start;
            }
            let token = match event {
                Event::Start(tag) => {
                    open.push(tokens.len());
                    tokens.push(Tok {
                        token: Token::Open(open_tag(tag)),
                        range: range.start..range.start,
                    });
                    continue;
                }
                Event::End(_) => {
                    let tag = open.pop().expect("an open tag");
                    // What follows its content; all of it if it has none.
                    let start = match tokens.len() == tag + 1 {
                        true => tokens[tag].range.start,
                        false => tokens.last().expect("content").range.end,
                    };
                    tokens.push(Tok {
                        token: Token::Close,
                        range: start..range.end,
                    });
                    continue;
                }
                Event::Text(text) => Token::Text(text),
                Event::Code(code) => Token::Code(code),
                Event::InlineHtml(html) | Event::Html(html) => Token::Html(html),
                Event::SoftBreak => Token::SoftBreak,
                Event::HardBreak => Token::HardBreak,
                // A loose item's marker starts its first paragraph.
                Event::TaskListMarker(checked) => {
                    self.task = self.task.or(Some(checked));
                    continue;
                }
                _ => continue,
            };
            tokens.push(Tok { token, range });
        }
        tokens
    }

    /// The text and HTML through the end of the tag they're in, and where
    /// the last of it ends.
    fn text_to_end(&mut self) -> (String, Option<usize>) {
        let mut text = String::new();
        let mut end = None;
        for (event, range) in self.events.by_ref() {
            match event {
                Event::Text(chunk) | Event::Html(chunk) => {
                    text.push_str(&chunk);
                    end = Some(range.end);
                }
                Event::End(_) => break,
                _ => {}
            }
        }
        (text, end)
    }

    /// Reads through the end of the tag whose start was just read.
    fn skip_to_end(&mut self) {
        let mut depth = 1;
        for (event, _) in self.events.by_ref() {
            match event {
                Event::Start(_) => depth += 1,
                Event::End(_) => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                break;
            }
        }
    }
}

fn open_tag(tag: Tag<'_>) -> Open<'_> {
    match tag {
        Tag::Emphasis => Open::Emphasis,
        Tag::Strong => Open::Strong,
        Tag::Strikethrough => Open::Strikethrough,
        Tag::Link {
            link_type,
            dest_url,
            title,
            ..
        } => Open::Link {
            dest: match link_type {
                LinkType::Email => format!("mailto:{dest_url}"),
                _ => dest_url.into_string(),
            },
            title,
        },
        Tag::Image {
            dest_url, title, ..
        } => Open::Image {
            dest: dest_url,
            title,
        },
        _ => Open::Other,
    }
}

/// Whether an event belongs to a paragraph's content rather than starting
/// or ending a block.
fn is_inline(event: &Event<'_>) -> bool {
    match event {
        Event::Start(tag) => matches!(
            tag,
            Tag::Emphasis
                | Tag::Strong
                | Tag::Strikethrough
                | Tag::Superscript
                | Tag::Subscript
                | Tag::Link { .. }
                | Tag::Image { .. }
        ),
        Event::End(_) | Event::Rule | Event::Html(_) | Event::DisplayMath(_) => false,
        // Also a loose task item's first paragraph starts with its marker.
        _ => true,
    }
}
