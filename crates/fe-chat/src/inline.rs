//! The headless core of inline edit: select code, give an instruction, and
//! the model's replacement comes back as a [`ProposedChange`] to review.
//!
//! - [`inline_request`] lays out the request from an [`InlineEditInput`].
//! - [`InlineEditSession`] reads the reply's one fenced block as it streams.
//! - [`run_inline_edit`] drives a provider and yields [`InlineEditUpdate`]s;
//!   dropping the stream cancels the request.
//! - [`to_proposal`] turns the replacement into the editor's proposal, so
//!   only the changed lines become hunks.
//!
//! The proposal is made when the stream finishes, with live text only for a
//! preview line: a live-applied edit would make rejecting an undo.

use std::ops::Range;

use fe_buffer::{BufferSnapshot, Origin, ProposedChange};
use futures::{Stream, StreamExt, stream};
use serde_json::{Map, Value};

use crate::provider::{
    ApiMessage, ChatEvent, ChatProvider, ChatRequest, ChatStream, ErrorKind, ProviderError, Role,
    SystemBlock,
};
use crate::request::ChatSettings;

/// How much text before and after the selection a request carries, each.
pub const CONTEXT_BYTES: usize = 4_000;

/// The agent name inline edits are proposed under.
pub const INLINE_EDIT_AGENT: &str = "fastedit.inline_edit";

/// What an inline edit is asked about.
#[derive(Clone, Debug)]
pub struct InlineEditInput {
    pub path: String,
    pub language: String,
    /// The file's text before the selection (or caret).
    pub text_before: String,
    /// The selected text; empty for a caret, which means insert there.
    pub selection: String,
    pub text_after: String,
    pub instruction: String,
    pub settings: ChatSettings,
}

const SYSTEM: &str = "\
You are the inline editor in Tenmoku, a code editor. The user selects code \
in a file and gives an instruction; you answer with the code that replaces \
the selection.

The user message holds the file's path and language, then a piece of the \
file around the selection, with the selection between <selection> and \
</selection>, then the instruction. When the selection is empty, a \
<caret/> marks the place instead, and you answer with the text to insert \
there.

Answer with only the replacement, in one fenced code block, such as \
```rust, and nothing outside the fence: no explanation before or after. \
The block's body is the complete new text for the selection, keeping its \
indentation, and nothing of the text around it. Do not include the \
<selection> markers.
";

/// The request for `input`: see the module's notes.
pub fn inline_request(input: &InlineEditInput) -> ChatRequest {
    let before = tail_at_line_start(&input.text_before, CONTEXT_BYTES);
    let after = head_at_line_end(&input.text_after, CONTEXT_BYTES);
    let mut context = format!(
        "<file path=\"{}\" language=\"{}\">\n",
        escape_attribute(&input.path),
        escape_attribute(&input.language)
    );
    context.push_str(before);
    if input.selection.is_empty() {
        context.push_str("<caret/>");
    } else {
        context.push_str("<selection>");
        context.push_str(&input.selection);
        context.push_str("</selection>");
    }
    context.push_str(after);
    if !context.ends_with('\n') {
        context.push('\n');
    }
    context.push_str("</file>");

    let system = format!(
        "{SYSTEM}\nFile: {} ({})\n",
        input.path,
        if input.language.is_empty() {
            "unknown language"
        } else {
            &input.language
        }
    );
    ChatRequest {
        model: input.settings.model.clone(),
        system: vec![SystemBlock {
            text: system,
            cache_control: None,
        }],
        tools: Vec::new(),
        messages: vec![ApiMessage {
            role: Role::User,
            content: vec![text_block(context), text_block(input.instruction.clone())],
        }],
        max_tokens: input.settings.max_tokens,
        effort: input.settings.effort,
        show_thinking: false,
    }
}

/// The last at most `max` bytes of `text`, starting at a line start.
fn tail_at_line_start(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut start = text.len() - max;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    if text.as_bytes()[start - 1] == b'\n' {
        return &text[start..];
    }
    match text[start..].find('\n') {
        Some(newline) => &text[start + newline + 1..],
        None => "",
    }
}

/// The first at most `max` bytes of `text`, ending after a line's `\n`.
fn head_at_line_end(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    if text.as_bytes()[end - 1] == b'\n' {
        return &text[..end];
    }
    match text[..end].rfind('\n') {
        Some(newline) => &text[..=newline],
        None => "",
    }
}

fn text_block(text: String) -> Value {
    let mut block = Map::new();
    block.insert("text".to_owned(), Value::String(text));
    block.insert("type".to_owned(), Value::String("text".to_owned()));
    Value::Object(block)
}

fn escape_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
}

/// Where an [`InlineEditSession`] is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineEditState {
    Streaming,
    /// The reply ended with one closed fence: [`InlineEditSession::finish`]
    /// has its body.
    Done,
    Failed(ProviderError),
    /// The reply ended without a closed fenced block. Never guessed at.
    NoCode,
}

/// Reads the reply of an inline edit as its events arrive.
#[derive(Debug)]
pub struct InlineEditSession {
    reply: String,
    state: InlineEditState,
}

impl Default for InlineEditSession {
    fn default() -> Self {
        Self::new()
    }
}

impl InlineEditSession {
    pub fn new() -> InlineEditSession {
        InlineEditSession {
            reply: String::new(),
            state: InlineEditState::Streaming,
        }
    }

    /// Takes the next event. Once the session is no longer streaming,
    /// events are ignored.
    pub fn apply(&mut self, event: &ChatEvent) {
        if self.state != InlineEditState::Streaming {
            return;
        }
        match event {
            ChatEvent::TextDelta { text, .. } => self.reply.push_str(text),
            ChatEvent::Failed(error) => self.state = InlineEditState::Failed(error.clone()),
            ChatEvent::Finished { .. } => self.end(),
            _ => {}
        }
    }

    /// The stream ended: a reply that never finished failed.
    pub fn end_of_stream(&mut self) {
        if self.state == InlineEditState::Streaming {
            let error = ProviderError::new(ErrorKind::Stream, "the reply ended early");
            self.state = InlineEditState::Failed(error);
        }
    }

    fn end(&mut self) {
        self.state = match read_fence(&self.reply, true) {
            Fence::Closed(_) => InlineEditState::Done,
            _ => InlineEditState::NoCode,
        };
    }

    pub fn state(&self) -> &InlineEditState {
        &self.state
    }

    /// The fence's body read so far: what the replacement is up to now.
    /// Empty before a fence opens.
    pub fn text_so_far(&self) -> String {
        match read_fence(&self.reply, self.state != InlineEditState::Streaming) {
            Fence::None => String::new(),
            Fence::Open(body) | Fence::Closed(body) => body,
        }
    }

    /// The replacement, once the reply is [`Done`](InlineEditState::Done).
    pub fn finish(&self) -> Option<String> {
        match (&self.state, read_fence(&self.reply, true)) {
            (InlineEditState::Done, Fence::Closed(body)) => Some(body),
            _ => None,
        }
    }
}

enum Fence {
    /// No complete opening line yet, or none at all.
    None,
    /// Opened, not closed: the body so far.
    Open(String),
    Closed(String),
}

/// The first fenced block of `reply`. A fence is a line of at least three
/// backticks (up to three spaces in), its rest the info string; it closes
/// at a line of only backticks, at least as many, with up to three spaces
/// before and any spaces or tabs after. While streaming
/// (`ended` false) a last line still arriving isn't taken as body when it
/// could be the start of the closing fence.
fn read_fence(reply: &str, ended: bool) -> Fence {
    let mut lines = reply.split_inclusive('\n');
    let mut opening = 0;
    for line in lines.by_ref() {
        if let Some(ticks) = opening_fence(line) {
            if !line.ends_with('\n') && !ended {
                return Fence::None;
            }
            opening = ticks;
            break;
        }
    }
    if opening == 0 {
        return Fence::None;
    }
    let mut body = String::new();
    for line in lines {
        let complete = line.ends_with('\n') || ended;
        let text = line.trim_end_matches(['\n', '\r']);
        let closing = closing_ticks(text);
        if complete && closing.is_some_and(|ticks| ticks >= opening) {
            return Fence::Closed(body);
        }
        if !complete && closing.is_some() {
            // Might be the closing fence, split across deltas.
            break;
        }
        body.push_str(line);
    }
    Fence::Open(body)
}

/// The backticks of `line` if it could be a closing fence: up to three
/// spaces, backticks, then only spaces or tabs. Zero backticks is a line
/// that might yet become one.
fn closing_ticks(line: &str) -> Option<usize> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let ticks = rest.chars().take_while(|&c| c == '`').count();
    rest[ticks..]
        .chars()
        .all(|c| c == ' ' || c == '\t')
        .then_some(ticks)
}

/// The backticks opening a fence at `line`, if it opens one.
fn opening_fence(line: &str) -> Option<usize> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let ticks = line[indent..].chars().take_while(|&c| c == '`').count();
    let info = line[indent + ticks..].trim_end();
    (ticks >= 3 && !info.contains('`')).then_some(ticks)
}

/// What [`run_inline_edit`] reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineEditUpdate {
    /// The replacement so far, whenever it grew.
    TextSoFar(String),
    /// The reply ended: the replacement.
    Done(String),
    Failed(ProviderError),
    /// The reply had no closed fenced block.
    NoCode,
}

/// Sends `input`'s request to `provider` and streams updates, ending with
/// one of [`Done`](InlineEditUpdate::Done),
/// [`Failed`](InlineEditUpdate::Failed) or
/// [`NoCode`](InlineEditUpdate::NoCode). Dropping the stream drops the
/// provider's, which cancels the request.
pub fn run_inline_edit(
    provider: &dyn ChatProvider,
    input: &InlineEditInput,
) -> impl Stream<Item = InlineEditUpdate> + use<> {
    struct Run {
        events: ChatStream,
        session: InlineEditSession,
        shown: usize,
        over: bool,
    }
    let run = Run {
        events: provider.stream(inline_request(input)),
        session: InlineEditSession::new(),
        shown: 0,
        over: false,
    };
    stream::unfold(run, |mut run| async move {
        loop {
            if run.over {
                return None;
            }
            match run.events.next().await {
                Some(event) => run.session.apply(&event),
                None => run.session.end_of_stream(),
            }
            let update = match run.session.state() {
                InlineEditState::Streaming => {
                    let text = run.session.text_so_far();
                    if text.len() == run.shown {
                        continue;
                    }
                    run.shown = text.len();
                    InlineEditUpdate::TextSoFar(text)
                }
                InlineEditState::Done => {
                    run.over = true;
                    InlineEditUpdate::Done(run.session.finish().unwrap_or_default())
                }
                InlineEditState::Failed(error) => {
                    run.over = true;
                    InlineEditUpdate::Failed(error.clone())
                }
                InlineEditState::NoCode => {
                    run.over = true;
                    InlineEditUpdate::NoCode
                }
            };
            return Some((update, run));
        }
    })
}

/// The proposal that replaces `range` of `snapshot` with `replacement`,
/// from the AI session `session`. Only changed lines become hunks; an
/// unchanged replacement has none.
pub fn to_proposal(
    snapshot: &BufferSnapshot,
    range: Range<usize>,
    replacement: &str,
    session: &str,
) -> ProposedChange {
    let origin = Origin::Ai {
        agent: INLINE_EDIT_AGENT.into(),
        session: session.into(),
    };
    ProposedChange::from_text(snapshot, range, replacement, origin)
}
