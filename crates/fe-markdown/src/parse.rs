//! pulldown-cmark's events, with their byte ranges, made into a
//! [`Document`] whose blocks know their source lines.

use std::{iter::Peekable, ops::Range};

use pulldown_cmark::{
    Alignment, BrokenLink, BrokenLinkCallback, CodeBlockKind, DefaultBrokenLinkCallback, Event,
    LinkType, OffsetIter, Options, RefDefs, Tag,
};

use crate::{
    Align, Block, BlockKind, Cell, Document, Inline, ListItem, Row, code, html_inline,
    inline::{self, Open, Tok, Token},
    slug::Slugs,
};

/// How deep quotes, list items and inline markup nest, together, before
/// deeper ones are left out and their content kept: a document of a few
/// thousand `>`s mustn't run a background thread out of stack.
const MAX_NESTING: usize = 64;

const OPTIONS: Options = Options::ENABLE_TABLES
    .union(Options::ENABLE_TASKLISTS)
    .union(Options::ENABLE_STRIKETHROUGH);

/// What an HTML block read as a paragraph is put after, so that its first
/// line, a tag, continues a paragraph rather than starting a block.
const LEAD: &str = "a\n";

pub(crate) fn parse(text: &str) -> Document {
    let events = pulldown_cmark::Parser::new_ext(text, OPTIONS).into_offset_iter();
    let mut parser = Parser {
        text,
        events: events.peekable(),
        line_starts: line_starts(text),
        slugs: Slugs::default(),
        depth: 0,
        task: None,
        paragraph: false,
        definitions: None,
    };
    let blocks = parser.blocks();
    Document { blocks }
}

/// What an HTML block read as a paragraph comes to.
enum Reread {
    Paragraph(Vec<Inline>),
    /// A row, 0 the lead's, starts a block of its own.
    Block(usize),
    /// It isn't a paragraph of mapped tags.
    Not,
}

/// Reads `source`, an HTML block after [`LEAD`], as a paragraph. A
/// backslash or an indent was put in at each of `inserted`, in order, to
/// keep a row from starting a block; `definitions` are the document's link references.
fn reread(
    source: &str,
    inserted: &[Range<usize>],
    definitions: &RefDefs<'_>,
    depth: usize,
) -> Reread {
    let callback = |link: BrokenLink<'_>| {
        let definition = definitions.get(&link.reference)?;
        let title = definition.title.as_deref().unwrap_or_default();
        Some((definition.dest.to_string().into(), title.to_owned().into()))
    };
    let events =
        pulldown_cmark::Parser::new_with_broken_link_callback(source, OPTIONS, Some(callback));
    let mut parser = Parser {
        text: source,
        events: events.into_offset_iter().peekable(),
        line_starts: line_starts(source),
        slugs: Slugs::default(),
        depth,
        task: None,
        paragraph: false,
        definitions: None,
    };
    match parser.events.next() {
        Some((Event::Start(Tag::Paragraph), _)) => {}
        // A setext heading takes the lead in: its underline starts it.
        Some((Event::Start(Tag::Heading { .. }), range)) if range.start == 0 => {
            return Reread::Block(parser.lines(range).end as usize - 1);
        }
        _ => return Reread::Not,
    }
    let mut tokens = parser.inline_tokens();
    let [
        lead,
        Tok {
            token: Token::SoftBreak,
            ..
        },
        ..,
    ] = &tokens[..]
    else {
        return Reread::Not;
    };
    if !matches!(lead.token, Token::Text(_)) || lead.range != (0..LEAD.len() - 1) {
        return Reread::Not;
    }
    let Some((Event::End(_), _)) = parser.events.next() else {
        return Reread::Not;
    };
    if let Some((event, range)) = parser.events.next() {
        let row = parser.row(range.start) as usize;
        return match event {
            Event::Start(Tag::HtmlBlock) => Reread::Not,
            // Its delimiter row, after its head, makes it a table.
            Event::Start(Tag::Table(_)) => Reread::Block(row + 1),
            _ => Reread::Block(row),
        };
    }
    tokens.drain(..2);
    // A code span shows what was put in as written.
    for tok in &mut tokens {
        if let Token::Code(code) = &mut tok.token
            && !inserted_in(inserted, &tok.range).is_empty()
        {
            *code = unescaped_code(source, tok.range.clone(), inserted).into();
        }
    }
    let tokens = html_inline::tags(inline::autolinks(source, tokens));
    if tokens.iter().any(|tok| matches!(tok.token, Token::Html(_))) {
        return Reread::Not;
    }
    Reread::Paragraph(inline::build(tokens, MAX_NESTING.saturating_sub(depth)))
}

/// How a row is kept from starting a block.
#[derive(Clone, Copy)]
enum Hold {
    /// A backslash at this byte of the row, which escapes what is there.
    Escape(usize),
    /// An indent, for a row whose punctuation is inline markup too: a
    /// backslash would end a run of backticks.
    Indent,
}

/// What an indent put in front of a row is.
const INDENT: &str = "    ";

/// How many extra reads a row starting a block no guess caught may take.
const MAX_REREADS: usize = 8;

/// How `line` is kept from starting a block, if it looks as if it would:
/// before its first punctuation mark, or before the `.` or `)` after a
/// list item's number. Rows indented four columns can't start one.
fn hold(line: &str) -> Option<Hold> {
    let rest = line.trim_start_matches([' ', '\t']);
    if line.len() - rest.len() >= INDENT.len() || line.starts_with('\t') {
        return None;
    }
    let at = line.len() - rest.len();
    let body = rest.trim_end_matches([' ', '\t', '\r', '\n']);
    if rest.starts_with("```") || rest.starts_with("~~~") {
        return Some(Hold::Indent);
    }
    let after = |n: usize| {
        matches!(
            rest.as_bytes().get(n),
            None | Some(b' ' | b'\t' | b'\r' | b'\n')
        )
    };
    let head = rest.bytes().next()?;
    let starts = match head {
        b'#' => {
            let hashes = rest.bytes().take_while(|&b| b == b'#').count();
            hashes <= 6 && after(hashes)
        }
        b'>' => true,
        b'-' | b'+' | b'*' | b'=' | b'|' | b':' | b'_' => {
            // A bullet, a rule, a setext underline or a table's delimiter row.
            after(1)
                || body
                    .bytes()
                    .all(|b| matches!(b, b'-' | b'=' | b'|' | b':' | b' ' | b'\t'))
                    && body.contains(['-', '='])
                || {
                    let marks = body.bytes().filter(|b| !matches!(b, b' ' | b'\t'));
                    marks.clone().count() >= 3 && marks.clone().all(|b| b == head)
                }
        }
        b'0'..=b'9' => {
            let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
            matches!(rest.as_bytes().get(digits), Some(b'.' | b')')) && after(digits + 1)
        }
        _ => false,
    };
    starts.then(|| {
        Hold::Escape(
            at + usize::from(head.is_ascii_digit())
                * rest.bytes().take_while(u8::is_ascii_digit).count(),
        )
    })
}

/// The row of `line` a fallback hold may go on: like [`hold`], but whatever
/// the row starts with.
fn fallback_hold(line: &str) -> Option<Hold> {
    let rest = line.trim_start_matches([' ', '\t']);
    let at = line.len() - rest.len();
    if rest.starts_with(['`', '~']) {
        return Some(Hold::Indent);
    }
    if rest.starts_with(|c: char| c.is_ascii_punctuation()) {
        return Some(Hold::Escape(at));
    }
    let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    match rest[digits..].chars().next() {
        Some('.' | ')') if digits > 0 => Some(Hold::Escape(at + digits)),
        _ => None,
    }
}

/// The ones of `inserted`, ordered, that overlap `span`.
fn inserted_in<'a>(inserted: &'a [Range<usize>], span: &Range<usize>) -> &'a [Range<usize>] {
    let from = inserted.partition_point(|put| put.end <= span.start);
    let to = inserted.partition_point(|put| put.start < span.end);
    &inserted[from..to.max(from)]
}

/// The text of the code span at `span` in `source`, without what was put
/// in at `inserted`, read as pulldown-cmark reads a code span's: each line
/// ending a space, and a space stripped from each end when both have one.
fn unescaped_code(source: &str, span: Range<usize>, inserted: &[Range<usize>]) -> String {
    let mut text = String::new();
    let mut from = span.start;
    for put in inserted_in(inserted, &span) {
        text.push_str(&source[from..put.start.max(from)]);
        from = put.end.max(from);
    }
    text.push_str(&source[from..span.end]);
    let ticks = text.len() - text.trim_start_matches('`').len();
    let text = text[ticks..text.len() - ticks].replace(['\r', '\n'], " ");
    match text
        .strip_prefix(' ')
        .and_then(|text| text.strip_suffix(' '))
    {
        Some(stripped) if !text.bytes().all(|byte| byte == b' ') => stripped.to_owned(),
        _ => text,
    }
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
struct Parser<'a, F: BrokenLinkCallback<'a> = DefaultBrokenLinkCallback> {
    text: &'a str,
    events: Peekable<OffsetIter<'a, F>>,
    line_starts: Vec<usize>,
    slugs: Slugs,
    /// How many quotes and list items the next block is in.
    depth: usize,
    /// The `[x]` or `[ ]` marker read since the current list item started.
    task: Option<bool>,
    /// Whether the current list item has a paragraph of its own, which
    /// makes its list loose.
    paragraph: bool,
    /// A parser of `text` for its link reference definitions, made once an
    /// HTML block is read as a paragraph.
    definitions: Option<pulldown_cmark::Parser<'a>>,
}

impl<'a, F: BrokenLinkCallback<'a>> Parser<'a, F> {
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
                match self.html_paragraph(&html) {
                    Some(content) => BlockKind::Paragraph(content),
                    None => BlockKind::Html(html),
                }
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

    /// An HTML block's content, if the block is a paragraph's inlines and
    /// phrasing tags [`html_inline`] maps, all with their pairs: rustdoc's
    /// `<code>` alone on a line starts an HTML block in CommonMark.
    fn html_paragraph(&mut self, html: &str) -> Option<Vec<Inline>> {
        if !html_inline::starts_phrasing(html.lines().next()?) {
            return None;
        }
        let text = self.text;
        let definitions = self
            .definitions
            .get_or_insert_with(|| pulldown_cmark::Parser::new_ext(text, OPTIONS))
            .reference_definitions();
        let lines: Vec<&str> = html.split_inclusive('\n').collect();
        // How each row is kept from starting a block, as a paragraph's: the
        // rows that look as if they would are held at once, so one read
        // usually does; a few more catch the rest, one row each.
        let mut holds: Vec<Option<Hold>> = lines.iter().map(|line| hold(line)).collect();
        for _ in 0..=MAX_REREADS {
            let mut source = LEAD.to_owned();
            let mut inserted = Vec::new();
            for (line, held) in lines.iter().zip(&holds) {
                match held {
                    Some(Hold::Escape(at)) => {
                        let (before, after) = line.split_at(*at);
                        source.push_str(before);
                        inserted.push(source.len()..source.len() + 1);
                        source.push('\\');
                        source.push_str(after);
                    }
                    Some(Hold::Indent) => {
                        inserted.push(source.len()..source.len() + INDENT.len());
                        source.push_str(INDENT);
                        source.push_str(line);
                    }
                    None => source.push_str(line),
                }
            }
            match reread(&source, &inserted, definitions, self.depth) {
                Reread::Paragraph(content) => return Some(content),
                Reread::Block(0) | Reread::Not => return None,
                Reread::Block(row) => {
                    let held = holds.get_mut(row - 1)?;
                    if held.is_some() {
                        return None;
                    }
                    *held = Some(fallback_hold(lines[row - 1])?);
                }
            }
        }
        None
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
        let tokens = html_inline::tags(inline::autolinks(self.text, tokens));
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
