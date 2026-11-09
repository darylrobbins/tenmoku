//! Replies drawn as Markdown (the decision record's § 1): each turn's
//! reply and thinking parsed with `fe_markdown`, at most once a frame while
//! they stream, and drawn with `fe_markdown_view`'s document element, with
//! a header over each code block to copy or apply it (§ 8).

use std::{
    cell::RefCell,
    collections::HashMap,
    ops::Range,
    path::{Component, Path},
    rc::Rc,
    sync::Arc,
};

use fe_chat::{DIVIDER_MARKER, EditBlock, EditBody, REPLACE_MARKER, SEARCH_MARKER};
use fe_gpui::EditorTheme;
use fe_markdown::{Block, BlockKind, Document, LinkTarget, link_target};
use fe_markdown_view::{
    CodeBody, CodeHeader, ElementOptions, LinkHandler, MarkdownStyle, document_element,
};
use gpui::{
    AnyElement, App, Bounds, ClipboardItem, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, ScrollHandle, SharedString, Styled, StyledText, WeakEntity, Window,
    div, px,
};
use kiln_l10n::{text, tr};
use kiln_ui::{Button, UiTheme, h_flex};

use crate::{ChatView, ChatViewEvent};

/// What a code block's header shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeBlock {
    /// The language the fence names, as written: its info string's first
    /// word.
    pub language: Option<String>,
    /// The file the block is for: its info string's `path=`.
    pub path: Option<String>,
    /// The code, with `\n` line endings.
    pub code: String,
}

impl CodeBlock {
    /// The block a fence with info string `info` holds `code`.
    pub(crate) fn new(info: &str, code: &str) -> CodeBlock {
        // A fence may name only the path.
        let language = (info.split_whitespace().next()).filter(|word| !word.starts_with("path="));
        CodeBlock {
            language: language.map(str::to_owned),
            path: info_path(info).map(str::to_owned),
            code: code.to_owned(),
        }
    }
}

/// What came of applying a code block, as its header shows it: set by the
/// host ([`ChatView::set_block_state`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockState {
    /// Proposed in its file, `pending` of its `hunks` still to decide.
    Proposed { pending: usize, hunks: usize },
    /// Every hunk decided, `accepted` of `hunks` accepted.
    Decided { accepted: usize, hunks: usize },
    /// It changes nothing in its file.
    NoChange,
    /// It couldn't be applied, and why.
    Failed(String),
}

impl BlockState {
    /// The header's label for it.
    pub fn label(&self) -> String {
        match *self {
            BlockState::Proposed { pending, hunks } if pending >= hunks => {
                tr!("chat-view-block-proposed").to_string()
            }
            BlockState::Proposed { pending, .. } => {
                tr!("chat-view-block-proposed-pending", pending = pending).to_string()
            }
            BlockState::Decided { accepted, hunks } if accepted >= hunks => {
                tr!("chat-view-block-accepted").to_string()
            }
            BlockState::Decided { accepted: 0, .. } => tr!("chat-view-block-rejected").to_string(),
            BlockState::Decided { accepted, hunks } => tr!(
                "chat-view-block-accepted-some",
                accepted = accepted,
                hunks = hunks
            )
            .to_string(),
            BlockState::NoChange => tr!("chat-view-block-no-changes").to_string(),
            BlockState::Failed(_) => tr!("chat-view-block-not-applied").to_string(),
        }
    }
}

/// The `path=` an info string gives, a word or in double quotes.
fn info_path(info: &str) -> Option<&str> {
    let (at, _) = (info.match_indices("path=")).find(|(at, _)| {
        info[..*at]
            .chars()
            .next_back()
            .is_none_or(char::is_whitespace)
    })?;
    let value = &info[at + "path=".len()..];
    let path = match value.strip_prefix('"') {
        Some(quoted) => quoted.split('"').next(),
        None => value.split_whitespace().next(),
    };
    path.filter(|path| !path.is_empty())
}

/// `document`'s code blocks, in document order: those in quotes and list
/// items where they stand, as the document element numbers them.
pub(crate) fn code_blocks(document: &Document) -> Vec<CodeBlock> {
    fn walk(blocks: &[Block], out: &mut Vec<CodeBlock>) {
        for block in blocks {
            match &block.kind {
                BlockKind::Code { info, code, .. } => out.push(CodeBlock::new(info, code)),
                BlockKind::Quote(blocks) => walk(blocks, out),
                BlockKind::List { items, .. } => {
                    for item in items {
                        walk(&item.blocks, out);
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&document.blocks, &mut out);
    out
}

/// How many code blocks `document` has, in quotes and lists too.
fn code_block_count(document: &Document) -> usize {
    fn walk(blocks: &[Block]) -> usize {
        blocks
            .iter()
            .map(|block| match &block.kind {
                BlockKind::Code { .. } => 1,
                BlockKind::Quote(blocks) => walk(blocks),
                BlockKind::List { items, .. } => items.iter().map(|item| walk(&item.blocks)).sum(),
                _ => 0,
            })
            .sum()
    }
    walk(&document.blocks)
}

/// A row of a SEARCH/REPLACE block drawn as a diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DiffRow {
    pub(crate) kind: DiffKind,
    /// The line's text, by byte range of the block's code, its line ending
    /// left out; empty for a [`DiffKind::Gap`].
    pub(crate) range: Range<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiffKind {
    /// A line of a section's search text.
    Removed,
    /// A line of its replacement.
    Added,
    /// Between two sections.
    Gap,
}

/// A code block's `code` as a diff, when it reads as SEARCH/REPLACE
/// sections: each section's search lines removed and its replace lines
/// added, with a gap between each two sections, empty ones too. `None`
/// for any other code: a whole file, or markers that don't make whole
/// sections. The info string isn't read, so a path the block can't be
/// applied to still shows as a diff.
pub(crate) fn diff_rows(code: &str) -> Option<Vec<DiffRow>> {
    let Ok(EditBlock {
        body: EditBody::Sections(_),
        ..
    }) = fe_chat::parse_edit_block("", code)
    else {
        return None;
    };
    // The sections are whole, so each marker is where it belongs.
    let mut rows: Vec<DiffRow> = Vec::new();
    let (mut side, mut sections) = (None, 0);
    let mut at = 0;
    for line in code.split_inclusive('\n') {
        let start = at;
        at += line.len();
        let text = line.strip_suffix('\n').unwrap_or(line);
        let text = text.strip_suffix('\r').unwrap_or(text);
        match text {
            SEARCH_MARKER => {
                // Every section after the first, even an empty one.
                if sections > 0 {
                    let range = start..start;
                    rows.push(DiffRow {
                        kind: DiffKind::Gap,
                        range,
                    });
                }
                sections += 1;
                side = Some(DiffKind::Removed);
            }
            DIVIDER_MARKER => side = Some(DiffKind::Added),
            REPLACE_MARKER => side = None,
            _ => {
                if let Some(kind) = side {
                    let range = start..start + text.len();
                    rows.push(DiffRow { kind, range });
                }
            }
        }
    }
    Some(rows)
}

/// The rows' tints in a code block drawn as a diff: the editor's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DiffTints {
    pub(crate) removed: Hsla,
    pub(crate) added: Hsla,
}

impl DiffTints {
    pub(crate) fn new(editor: &EditorTheme) -> DiffTints {
        DiffTints {
            removed: editor.diff_removed_row,
            added: editor.diff_added_row,
        }
    }
}

/// Code block `block` of turn `turn`'s reply as a diff, if its code is
/// SEARCH/REPLACE sections ([`diff_rows`]): a row per line in its syntax
/// colours, after a `-` or `+`, tinted from edge to edge, the rows as wide
/// as the widest so they scroll sideways together. Its `debug_selector` is
/// `chat-diff-<turn>-<index>`, and its rows'
/// `chat-diff-<turn>-<index>-removed-<k>` and `-added-<k>`, `k` counted
/// for each kind.
fn diff_body(
    turn: usize,
    block: CodeBody<'_>,
    style: &MarkdownStyle,
    tints: DiffTints,
) -> Option<AnyElement> {
    let index = block.index;
    let (mut removed, mut added) = (0, 0);
    let rows = diff_rows(block.code)?.into_iter().map(|row| {
        let (sign, tint, name, k) = match row.kind {
            DiffKind::Removed => {
                removed += 1;
                ("-", tints.removed, "removed", removed - 1)
            }
            DiffKind::Added => {
                added += 1;
                ("+", tints.added, "added", added - 1)
            }
            DiffKind::Gap => {
                let height = style.code_font_size * style.code_line_height;
                return div().h(height).into_any_element();
            }
        };
        let text = block.code[row.range.clone()].to_owned();
        let runs = style.code_runs_in(block.code, block.highlights, row.range);
        div()
            .debug_selector(move || format!("chat-diff-{turn}-{index}-{name}-{k}"))
            .flex()
            .flex_row()
            .pl(px(8.))
            .pr(px(16.))
            .bg(tint)
            .child(
                div()
                    .flex_none()
                    .w(style.code_font_size)
                    .text_color(style.muted)
                    .child(sign),
            )
            .child(StyledText::new(text).with_runs(runs))
            .into_any_element()
    });
    let body = div()
        .debug_selector(move || format!("chat-diff-{turn}-{index}"))
        .flex_none()
        .min_w_full()
        .flex()
        .flex_col()
        .children(rows.collect::<Vec<_>>());
    Some(body.into_any_element())
}

/// How many of `document`'s code blocks are for a file: those whose info
/// string has a `path=`, in quotes and lists too.
pub(crate) fn path_blocks(document: &Document) -> usize {
    fn walk(blocks: &[Block]) -> usize {
        blocks
            .iter()
            .map(|block| match &block.kind {
                BlockKind::Code { info, code, .. } => {
                    usize::from(match fe_chat::parse_edit_block(info, code) {
                        Ok(edit) => edit.path.is_some(),
                        Err(fe_chat::EditBlockError::Path(_)) => true,
                        Err(_) => fe_chat::info_path(info).is_some(),
                    })
                }
                BlockKind::Quote(blocks) => walk(blocks),
                BlockKind::List { items, .. } => items.iter().map(|item| walk(&item.blocks)).sum(),
                _ => 0,
            })
            .sum()
    }
    walk(&document.blocks)
}

/// A turn's reply or thinking as last parsed, and whether the turn had
/// finished.
struct Parsed {
    text: String,
    finished: bool,
    document: Arc<Document>,
}

/// The documents the transcript draws, by turn, each parsed again only
/// when its text has changed, and once more when its turn finishes: as the
/// view parses in `render`, a frame's deltas are parsed together, once,
/// with the finish if it came in the same frame.
#[derive(Default)]
pub(crate) struct Documents {
    replies: HashMap<usize, Parsed>,
    thinking: HashMap<usize, Parsed>,
    parses: usize,
}

impl Documents {
    /// Turn `turn`'s reply, `text`, parsed, the turn `finished` or still
    /// streaming. An unclosed fence is code to the end, so a block half
    /// streamed is drawn as code.
    pub(crate) fn reply(&mut self, turn: usize, text: &str, finished: bool) -> Arc<Document> {
        parsed(&mut self.replies, turn, text, finished, &mut self.parses)
    }

    /// Turn `turn`'s thinking summary, `text`, parsed, as
    /// [`Documents::reply`].
    pub(crate) fn thinking(&mut self, turn: usize, text: &str, finished: bool) -> Arc<Document> {
        parsed(&mut self.thinking, turn, text, finished, &mut self.parses)
    }

    /// How many times a reply or thinking was parsed.
    pub(crate) fn parses(&self) -> usize {
        self.parses
    }
}

fn parsed(
    documents: &mut HashMap<usize, Parsed>,
    turn: usize,
    text: &str,
    finished: bool,
    parses: &mut usize,
) -> Arc<Document> {
    if let Some(parsed) = documents.get(&turn)
        && parsed.text == text
        && parsed.finished == finished
    {
        return parsed.document.clone();
    }
    *parses += 1;
    let document = Arc::new(fe_markdown::parse(text));
    let parsed = Parsed {
        text: text.to_owned(),
        finished,
        document: document.clone(),
    };
    documents.insert(turn, parsed);
    document
}

/// How replies are drawn: in the editor's colours and code font, as the
/// Markdown preview is, with code on the editor's background, since the
/// chat's own is the panel's, which the preview's code is drawn on.
pub(crate) fn reply_style(editor: &EditorTheme, ui: &UiTheme) -> MarkdownStyle {
    MarkdownStyle {
        code_background: editor.background,
        ..MarkdownStyle::new(editor, ui)
    }
}

/// How thinking is drawn: as a reply, muted.
pub(crate) fn thinking_style(reply: &MarkdownStyle) -> MarkdownStyle {
    MarkdownStyle {
        foreground: reply.muted,
        ..reply.clone()
    }
}

/// Where a reply's code block was last drawn in the transcript.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CodeSpan {
    pub(crate) turn: usize,
    pub(crate) block: usize,
    /// Its top and bottom, header included, below the top of the
    /// transcript's content, unscrolled: they don't change as the
    /// transcript scrolls.
    pub(crate) top: Pixels,
    pub(crate) bottom: Pixels,
    /// Its left, from the transcript's, and its width.
    pub(crate) left: Pixels,
    pub(crate) width: Pixels,
    /// How tall its header is drawn: a long path wraps.
    pub(crate) header: Pixels,
}

/// Where the transcript's reply code blocks are drawn, for the copy of a
/// header pinned over the transcript's top while its block is scrolled
/// under it. Each frame's prepaint records the blocks, and
/// [`CodeSpans::commit`], after the transcript, makes them the ones
/// [`CodeSpans::pinned`] reads.
#[derive(Clone)]
pub(crate) struct CodeSpans(Rc<RefCell<Spans>>);

struct Spans {
    /// The transcript's.
    scroll: ScrollHandle,
    /// The blocks this frame has drawn so far.
    drawing: Vec<CodeSpan>,
    /// The blocks the last frame drew.
    drawn: Vec<CodeSpan>,
    /// How tall the transcript was when they were drawn.
    viewport: Pixels,
    /// The heights of the headers this frame has drawn so far, by turn and
    /// block: each is drawn before its block is recorded.
    headers: HashMap<(usize, usize), Pixels>,
}

impl CodeSpans {
    pub(crate) fn new(scroll: &ScrollHandle) -> CodeSpans {
        CodeSpans(Rc::new(RefCell::new(Spans {
            scroll: scroll.clone(),
            drawing: Vec::new(),
            drawn: Vec::new(),
            viewport: Pixels::ZERO,
            headers: HashMap::new(),
        })))
    }

    /// Code block `block` of turn `turn`'s reply is drawn at `bounds`, in
    /// the window, at prepaint, when the transcript's bounds and offset are
    /// this frame's.
    fn record(&self, turn: usize, block: usize, bounds: Bounds<Pixels>) {
        let mut spans = self.0.borrow_mut();
        // A block drawn with no header has none to pin.
        let Some(header) = spans.headers.remove(&(turn, block)) else {
            return;
        };
        let (viewport, offset) = (spans.scroll.bounds(), spans.scroll.offset());
        let top = bounds.top() - viewport.top() - offset.y;
        spans.drawing.push(CodeSpan {
            turn,
            block,
            top,
            bottom: top + bounds.size.height,
            left: bounds.left() - viewport.left(),
            width: bounds.size.width,
            header,
        });
    }

    /// Code block `block` of turn `turn`'s reply's header is drawn
    /// `height` tall, at prepaint.
    fn header_drawn(&self, turn: usize, block: usize, height: Pixels) {
        self.0.borrow_mut().headers.insert((turn, block), height);
    }

    /// Makes the blocks this frame drew the ones [`CodeSpans::pinned`]
    /// reads, and returns the block whose header is pinned as the
    /// transcript is scrolled now.
    pub(crate) fn commit(&self) -> Option<CodeSpan> {
        let offset = {
            let mut spans = self.0.borrow_mut();
            spans.drawn = std::mem::take(&mut spans.drawing);
            spans.headers.clear();
            spans.viewport = spans.scroll.bounds().size.height;
            spans.scroll.offset().y
        };
        self.pinned(offset)
    }

    /// The block whose header is pinned with the transcript scrolled to
    /// `offset`: the one whose top is above the transcript's and of which
    /// at least its header's height is still in view, in a transcript at
    /// least that tall.
    pub(crate) fn pinned(&self, offset: Pixels) -> Option<CodeSpan> {
        let spans = self.0.borrow();
        (spans.drawn.iter().copied()).find(|span| {
            let in_view = (span.bottom + offset).min(spans.viewport);
            span.top + offset < Pixels::ZERO && in_view >= span.header
        })
    }
}

/// Where a code block's header is drawn.
#[derive(Clone)]
pub(crate) enum HeaderAt {
    /// Over its block, which reports its height to `spans`.
    Block(CodeSpans),
    /// Pinned over the transcript's top.
    Pinned,
}

/// Turn `turn`'s reply, `document`, with a header over each code block,
/// showing its state in `states`, by block,
/// each block's place reported to `spans`, and each SEARCH/REPLACE block
/// drawn as a diff in `tints` ([`diff_body`]). Apply is off while the
/// reply streams, its blocks unfinished, and its last code block, which
/// may still be coming, is drawn as code until it finishes.
#[allow(clippy::too_many_arguments)] // Each is one thing the reply needs.
pub(crate) fn reply_element(
    turn: usize,
    document: &Arc<Document>,
    style: &MarkdownStyle,
    streaming: bool,
    ui: UiTheme,
    tints: DiffTints,
    view: WeakEntity<ChatView>,
    spans: CodeSpans,
    states: HashMap<usize, BlockState>,
) -> AnyElement {
    let growing = streaming
        .then(|| code_block_count(document).checked_sub(1))
        .flatten();
    let body = {
        let style = style.clone();
        Rc::new(move |block: CodeBody<'_>, _: &mut Window, _: &mut App| {
            if Some(block.index) == growing {
                return None;
            }
            diff_body(turn, block, &style, tints)
        })
    };
    let header = {
        let (view, spans) = (view.clone(), spans.clone());
        Rc::new(move |block: CodeHeader<'_>, _: &mut Window, _: &mut App| {
            let code = CodeBlock::new(block.info, block.code);
            let at = HeaderAt::Block(spans.clone());
            let state = states.get(&block.index);
            code_header(
                turn,
                block.index,
                code,
                state,
                streaming,
                &ui,
                view.clone(),
                at,
            )
        })
    };
    let on_code_bounds = Rc::new(move |block, bounds, _: &mut Window, _: &mut App| {
        spans.record(turn, block, bounds)
    });
    let options = ElementOptions {
        on_link: Some(link_handler(view)),
        code_header: Some(header),
        on_code_bounds: Some(on_code_bounds),
        code_body: Some(body),
        ..ElementOptions::default()
    };
    document_element(document, style, options)
}

/// A thinking summary, `document`.
pub(crate) fn thinking_element(
    document: &Arc<Document>,
    style: &MarkdownStyle,
    view: WeakEntity<ChatView>,
) -> AnyElement {
    let options = ElementOptions {
        on_link: Some(link_handler(view)),
        ..ElementOptions::default()
    };
    document_element(document, style, options)
}

/// How much faster a code header's state label shrinks than its path.
const LABEL_SHRINK: f32 = 1000.;

/// Code block `index` of turn `turn`'s reply's header, `block`: its
/// language and path, what came of applying it, `state`, Copy, and Apply,
/// which asks the host to apply it, again too. Over its block, its
/// `debug_selector` is `chat-code-<turn>-<index>`, its state's
/// `chat-block-state-<turn>-<index>`, and its buttons'
/// `chat-copy-<turn>-<index>` and `chat-apply-<turn>-<index>`, and a block
/// that couldn't be applied says why in a row under it,
/// `chat-block-error-<turn>-<index>`; pinned, they are `chat-code-pinned`,
/// `chat-block-state-pinned`, `chat-copy-pinned` and `chat-apply-pinned`,
/// and there's no row.
#[allow(clippy::too_many_arguments)] // Each is one thing the header shows.
pub(crate) fn code_header(
    turn: usize,
    index: usize,
    block: CodeBlock,
    state: Option<&BlockState>,
    streaming: bool,
    ui: &UiTheme,
    view: WeakEntity<ChatView>,
    at: HeaderAt,
) -> AnyElement {
    let CodeBlock {
        language,
        path,
        code,
    } = block;
    let copy = move |_: &_, _: &mut Window, cx: &mut App| {
        cx.write_to_clipboard(ClipboardItem::new_string(code.clone()));
    };
    let apply = move |_: &_, _: &mut Window, cx: &mut App| {
        view.update(cx, |_, cx| {
            cx.emit(ChatViewEvent::ApplyBlock { turn, block: index })
        })
        .ok();
    };
    let suffix = match at {
        HeaderAt::Block(_) => format!("{turn}-{index}"),
        HeaderAt::Pinned => "pinned".to_owned(),
    };
    let id = |name: &str| SharedString::from(format!("{name}-{suffix}"));
    let (code_id, copy_id, apply_id) = (id("chat-code"), id("chat-copy"), id("chat-apply"));
    let failed = matches!(state, Some(BlockState::Failed(_)));
    let label = state.map(|state| {
        let state_id = id("chat-block-state");
        let mut label = div();
        label.style().flex_shrink = Some(LABEL_SHRINK);
        label
            .debug_selector(move || state_id.to_string())
            // In a narrow panel it gives way first, before the path and
            // the buttons.
            .min_w_0()
            .truncate()
            .text_color(if failed { ui.error_text } else { ui.muted_text })
            .child(state.label())
    });
    let header = h_flex()
        .debug_selector(move || code_id.to_string())
        .gap_2()
        .px_2()
        .py_1()
        .items_center()
        .border_b_1()
        .border_color(ui.border)
        .text_color(ui.muted_text)
        .children(language.map(|language| div().child(language)))
        .children(path.map(|path| div().min_w_0().text_color(ui.text).child(path)))
        .child(div().flex_1())
        .children(label)
        .child(
            Button::new(copy_id.clone(), text!("chat-view-block-copy"))
                .small()
                .debug_selector(move || copy_id.to_string())
                .on_click(copy),
        )
        .child(
            Button::new(apply_id.clone(), text!("chat-view-block-apply"))
                .small()
                .disabled(streaming)
                .debug_selector(move || apply_id.to_string())
                .on_click(apply),
        );
    match at {
        // In a column of its own, which tells `spans` how tall it is, the
        // reason a block wasn't applied under it.
        HeaderAt::Block(spans) => {
            let why = match state {
                Some(BlockState::Failed(why)) => Some(why.clone()),
                _ => None,
            };
            let error = why.map(|why| {
                div()
                    .debug_selector(move || format!("chat-block-error-{turn}-{index}"))
                    .px_2()
                    .py_1()
                    .border_b_1()
                    .border_color(ui.border)
                    .text_color(ui.error_text)
                    .child(why)
            });
            div()
                .flex()
                .flex_col()
                .on_children_prepainted(move |bounds, _, _| {
                    if let Some(header) = bounds.first() {
                        spans.header_drawn(turn, index, header.size.height);
                    }
                })
                .child(header)
                .children(error)
                .into_any_element()
        }
        HeaderAt::Pinned => header.into_any_element(),
    }
}

/// Where a link in a reply goes.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Link {
    /// An `http:` or `https:` URL, for the browser.
    Web(String),
    /// A path relative to the project, and the 1-based row a `#L12`
    /// fragment names.
    Path { path: String, row: Option<u32> },
}

/// Where the link `dest` goes, as written: `None` for an anchor, an
/// absolute path, a path out of the project, or any scheme but `http:` and
/// `https:`, `file:` among them.
fn link(dest: &str) -> Option<Link> {
    let dest = dest.trim();
    if dest.starts_with(['/', '\\']) {
        return None;
    }
    let scheme = dest.split_once(':').map(|(scheme, _)| scheme);
    let web = scheme.is_some_and(|scheme| {
        scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
    });
    // As fe-markdown reads a scheme: a letter, then letters, digits, `+`,
    // `-` or `.`.
    let schemed = scheme.is_some_and(|scheme| {
        let mut chars = scheme.chars();
        chars.next().is_some_and(|c| c.is_ascii_alphabetic())
            && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    });
    if schemed && !web {
        return None;
    }
    match link_target(dest, Path::new(""), None) {
        LinkTarget::Web(url) if web => Some(Link::Web(url)),
        LinkTarget::File { path, line }
            if !path.as_os_str().is_empty()
                && path
                    .components()
                    .all(|part| matches!(part, Component::Normal(_))) =>
        {
            // Joined with `/` as the reply wrote it, on Windows too.
            let path = path
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            Some(Link::Path {
                path,
                row: line.map(|line| line + 1),
            })
        }
        _ => None,
    }
}

/// A clicked link: a URL opens in the browser, and a path is
/// [`ChatViewEvent::OpenPath`] for the host.
fn link_handler(view: WeakEntity<ChatView>) -> LinkHandler {
    Rc::new(move |dest, _, cx| match link(dest) {
        Some(Link::Web(url)) => cx.open_url(&url),
        Some(Link::Path { path, row }) => {
            view.update(cx, |_, cx| cx.emit(ChatViewEvent::OpenPath { path, row }))
                .ok();
        }
        None => {}
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_strings_give_language_and_path() {
        let block = |info| {
            let block = CodeBlock::new(info, "");
            (block.language, block.path)
        };
        let some = |text: &str| Some(text.to_owned());
        assert_eq!(
            block("rust path=src/lib.rs"),
            (some("rust"), some("src/lib.rs"))
        );
        assert_eq!(
            block("rust  path=\"src/a b.rs\" title=x"),
            (some("rust"), some("src/a b.rs"))
        );
        assert_eq!(block("rust xpath=a"), (some("rust"), None));
        assert_eq!(
            block("ts path=\"web/app.ts\""),
            (some("ts"), some("web/app.ts"))
        );
        assert_eq!(block("path=Cargo.toml"), (None, some("Cargo.toml")));
        assert_eq!(block("py"), (some("py"), None));
        assert_eq!(block("rust path="), (some("rust"), None));
        assert_eq!(block(""), (None, None));
    }

    #[test]
    fn diff_rows_split_sections_into_removed_and_added() {
        // Two lines replaced by one, a blank line, a line deleted, then a
        // line added at the top of an empty search; the last marker has no
        // `\n`.
        let code = "<<<<<<< SEARCH\na = 1\nb = 2\n=======\na = 3\n>>>>>>> REPLACE\n\n\
                    <<<<<<< SEARCH\nc = 4\n=======\n>>>>>>> REPLACE\n\
                    <<<<<<< SEARCH\n=======\n\n>>>>>>> REPLACE";
        let rows = diff_rows(code).expect("sections");
        let texts: Vec<_> = (rows.iter())
            .map(|row| (row.kind, &code[row.range.clone()]))
            .collect();
        use DiffKind::{Added, Gap, Removed};
        assert_eq!(
            texts,
            [
                (Removed, "a = 1"),
                (Removed, "b = 2"),
                (Added, "a = 3"),
                (Gap, ""),
                (Removed, "c = 4"),
                (Gap, ""),
                (Added, ""),
            ]
        );
        // By byte range of the code as written, markers and all.
        assert_eq!(rows[0].range, 15..20);
        assert_eq!(rows[2].range, 35..40);
        let second = code.find("c = 4").unwrap();
        assert_eq!(rows[4].range, second..second + "c = 4".len());
        // Each gap stands where the next section opens.
        let third = code.rfind(SEARCH_MARKER).unwrap();
        assert_eq!(rows[5].range, third..third);

        // A `\r\n` line keeps its text, and its markers are markers.
        let crlf = "<<<<<<< SEARCH\r\nx\r\n=======\r\ny\r\n>>>>>>> REPLACE\r\n";
        let rows = diff_rows(crlf).expect("sections");
        let texts: Vec<_> = (rows.iter())
            .map(|row| (row.kind, &crlf[row.range.clone()]))
            .collect();
        assert_eq!(texts, [(Removed, "x"), (Added, "y")]);

        // Sections with both sides empty, first and in a row, each have
        // their gap.
        let empty = "<<<<<<< SEARCH\n=======\n>>>>>>> REPLACE\n";
        let full = "<<<<<<< SEARCH\nx\n=======\ny\n>>>>>>> REPLACE\n";
        let kinds = |code: &str| {
            let rows = diff_rows(code).expect("sections");
            rows.iter().map(|row| row.kind).collect::<Vec<_>>()
        };
        assert_eq!(kinds(empty), []);
        assert_eq!(kinds(&[empty, empty].concat()), [Gap]);
        assert_eq!(kinds(&[empty, full].concat()), [Gap, Removed, Added]);
        assert_eq!(
            kinds(&[full, empty, empty, full].concat()),
            [Removed, Added, Gap, Gap, Gap, Removed, Added]
        );
    }

    #[test]
    fn diff_rows_are_none_for_code_that_is_not_sections() {
        for code in [
            // A whole file.
            "[package]\nname = \"demo\"\n",
            "",
            // A section never closed.
            "<<<<<<< SEARCH\na\n=======\nb\n",
            "<<<<<<< SEARCH\na\n",
            // Text between sections.
            "<<<<<<< SEARCH\na\n=======\nb\n>>>>>>> REPLACE\nstray\n\
             <<<<<<< SEARCH\nc\n=======\nd\n>>>>>>> REPLACE\n",
            // Text before the first.
            "intro\n<<<<<<< SEARCH\na\n=======\nb\n>>>>>>> REPLACE\n",
            // A marker out of place.
            "=======\n",
        ] {
            assert_eq!(diff_rows(code), None, "{code:?}");
        }
    }

    #[test]
    fn links_go_to_the_browser_or_a_project_path() {
        let path = |path: &str, row| {
            Some(Link::Path {
                path: path.to_owned(),
                row,
            })
        };
        assert_eq!(
            link("https://example.com/a?b#c"),
            Some(Link::Web("https://example.com/a?b#c".to_owned()))
        );
        assert_eq!(
            link("HTTP://example.com"),
            Some(Link::Web("HTTP://example.com".to_owned()))
        );
        assert_eq!(link("src/lib.rs#L12"), path("src/lib.rs", Some(12)));
        assert_eq!(link("src/lib.rs#L12C3"), path("src/lib.rs", Some(12)));
        assert_eq!(link("./src/a%20b.rs"), path("src/a b.rs", None));
        assert_eq!(link("src/../Cargo.toml"), path("Cargo.toml", None));
        // A colon past the first path component isn't a scheme. (Windows
        // names can't hold one.)
        #[cfg(unix)]
        assert_eq!(link("src/a:b.rs"), path("src/a:b.rs", None));
        for nowhere in [
            "mailto:a@example.com",
            "#heading",
            "#L3",
            "?query",
            "/etc/passwd",
            "\\\\server\\share",
            "../outside.rs",
            "file:///etc/passwd",
            "file:src/lib.rs",
            "FILE:src/lib.rs#L3",
            "vscode:src/lib.rs",
            "c:src/lib.rs",
            "javascript:alert(1)",
            "",
        ] {
            assert_eq!(link(nowhere), None, "{nowhere}");
        }
    }
}
