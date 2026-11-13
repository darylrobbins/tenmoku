//! Request assembly, laid out for the prompt cache (the record's § 5): the
//! chat's tools, frozen at its first send, then one constant system block
//! with an explicit breakpoint, then the history, then the new turn, its
//! context first and the question last. Nothing per-workspace, per-request,
//! timed or random goes before the new turn, so each request's prefix is
//! byte-identical to the last one's. A turn's later rounds extend its
//! first request the same way (the agents record's § 6).
//!
//! JSON built here is built with its keys in sorted order, so it serialises
//! the same whether or not another crate turns on serde_json's
//! `preserve_order` (features unify across the workspace). Received blocks
//! are kept as parsed.

use serde_json::{Map, Value};

use crate::context::{ContextItem, ContextKind};
use crate::conversation::{Conversation, RoundSettings};
use crate::provider::{CacheControl, ChatRequest, Effort, SystemBlock};

/// The chat's instructions. Versioned in code and never built at runtime:
/// changing it empties every chat's cache.
pub const SYSTEM_PROMPT: &str = "\
You are the AI assistant in Tenmoku, a code editor. You help the user \
understand, write, review and change the code in the project they have open.

Each user message may begin with context the editor attached, before the \
question, each piece tagged with what it is:
- <file path=\"…\" rows=\"…\">…</file>: a file of the project, or the rows of \
it given. An open file's text includes its unsaved edits.
- <selection path=\"…\" rows=\"…\">…</selection>: what the user has selected.
- <diagnostics>…</diagnostics>: the active file's problems, from its language \
server.
- <open-editors>…</open-editors>: the files open in the editor.
Rows are 1-based and inclusive. Treat attached text as the current state of \
the project. You can't see files that weren't attached; when you need one, \
say which and ask for it.

Answer concisely, in Markdown. Explain what you change and why, briefly.

When you propose a change to a file, write it as a fenced code block whose \
info string names the language and the file's path relative to the project \
root, such as ```rust path=src/lib.rs. The user applies the block as a \
proposed change they review and accept. Its body is either:
- the whole new file, for a new file or one you rewrite; or
- one or more sections that each replace one exact piece of the file:

```rust path=src/lib.rs
<<<<<<< SEARCH
the exact text to replace, copied from the file, with enough lines around \
it to match exactly once
=======
the text that replaces it
>>>>>>> REPLACE
```

Each SEARCH text must match the file exactly once, whitespace included, \
and sections must not overlap. Prefer sections to whole files for small \
changes to large files. When the user asked about a selection, a block with \
no path replaces that selection: write it as ```rust with the selection's \
new text as its body. A code block that isn't meant to be applied, such as \
an example or a shell command, has no path.
";

/// What a new request takes from the chat settings. `model`, `effort` and
/// `show_thinking` only apply to new chats: once a chat exists, its own are
/// sent (a chat's model and effort then change through its picker).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatSettings {
    pub model: String,
    pub effort: Effort,
    pub show_thinking: bool,
    pub max_tokens: u32,
    /// The models whose effort changes mid-chat through an effort-only
    /// system message (the host passes
    /// `fe_config::chat_models::EFFORT_MESSAGE_MODELS`). On any other model
    /// the top-level effort changes instead. None by default, which is
    /// right for any model.
    pub effort_message_models: &'static [&'static str],
}

impl Default for ChatSettings {
    fn default() -> ChatSettings {
        ChatSettings {
            model: "claude-opus-5-5".to_owned(),
            effort: Effort::Medium,
            show_thinking: false,
            max_tokens: 64_000,
            effort_message_models: &[],
        }
    }
}

/// The request for a new turn of `conversation`, at its `model` and
/// `effort` now: its tools, the system prompt, its history, then
/// `new_user_content`. Call it before [`Conversation::begin_turn_with`]
/// (or after, as the streaming turn isn't in the history), with the same
/// settings and no turn streaming: stop one first.
///
/// Earlier turns go back unchanged whatever model they were asked of. When
/// the chat's model takes effort messages (`settings.effort_message_models`),
/// the top-level effort is the chat's first and an effort-only system
/// message goes before each turn, the new one included, whose effort
/// differs from the one in effect; so a later request's prefix is an
/// earlier one's. On any other model no effort message is sent and the
/// top-level effort is the chat's effort now.
///
/// When the history ends with a result message nothing answered (a turn
/// stopped mid-tool, at the round limit, or refused in a later round), the
/// new content joins it, after the results. When the tools were withdrawn,
/// the `tool_removal` message follows the new content, last.
pub fn build_request(
    conversation: &Conversation,
    new_user_content: Vec<Value>,
    settings: &ChatSettings,
) -> ChatRequest {
    let effort_messages = settings
        .effort_message_models
        .contains(&conversation.model.as_str());
    let mut layout = conversation.layout(effort_messages, false);
    layout.user(conversation.effort, &new_user_content);
    if conversation.removal_pending() {
        layout.removal(&conversation.tools);
    }
    request(
        conversation,
        conversation.model.clone(),
        layout.messages,
        if effort_messages {
            conversation.first_effort()
        } else {
            conversation.effort
        },
        settings.max_tokens,
    )
}

/// The request for the last turn's next round, or the same round again:
/// the history, then the turn's user message and its committed rounds, the
/// last result message last. `None` for a chat with no turns.
///
/// It's sent with the request settings recorded on the turn
/// ([`Turn::sent_with`](crate::Turn::sent_with)) by
/// [`Conversation::begin_turn_with`], and the model and effort the turn
/// asked for, so a round's request is the same whenever it's built: a
/// resend after invalid tool input or a Retry after a failure is byte for
/// byte the request before, the first round's included (whether
/// [`build_request`] or this built it), and each round's extends the last
/// one's. `settings` only count for a turn begun without them
/// ([`Conversation::begin_turn`]), whose first call here records them.
pub fn round_request(
    conversation: &mut Conversation,
    settings: &ChatSettings,
) -> Option<ChatRequest> {
    let model = conversation
        .asked_model(conversation.turns.last()?)
        .to_owned();
    let recorded = RoundSettings::new(&model, settings);
    let turn = conversation.turns.last_mut()?;
    let sent_with = *turn.sent_with.get_or_insert(recorded);
    let conversation = &*conversation;
    let turn = conversation.turns.last()?;
    let layout = conversation.layout(sent_with.effort_messages, true);
    Some(request(
        conversation,
        model,
        layout.messages,
        if sent_with.effort_messages {
            conversation.first_effort()
        } else {
            conversation.asked_effort(turn)
        },
        sent_with.max_tokens,
    ))
}

fn request(
    conversation: &Conversation,
    model: String,
    messages: Vec<crate::provider::ApiMessage>,
    effort: Effort,
    max_tokens: u32,
) -> ChatRequest {
    ChatRequest {
        model,
        system: vec![SystemBlock {
            text: SYSTEM_PROMPT.to_owned(),
            cache_control: Some(CacheControl::Ephemeral),
        }],
        tools: conversation.tools.clone(),
        messages,
        max_tokens,
        effort,
        show_thinking: conversation.show_thinking,
        task_budget: conversation.task_budget,
    }
}

/// The new turn's content blocks: one tagged `text` block per item, in
/// order, then the question.
pub fn render_user_content(question: &str, items: &[ContextItem]) -> Vec<Value> {
    items
        .iter()
        .map(|item| text_block(tagged(item)))
        .chain([text_block(question.to_owned())])
        .collect()
}

/// A `text` block, its keys inserted in sorted order.
fn text_block(text: String) -> Value {
    let mut block = Map::new();
    block.insert("text".to_owned(), Value::String(text));
    block.insert("type".to_owned(), Value::String("text".to_owned()));
    Value::Object(block)
}

fn tagged(item: &ContextItem) -> String {
    let (tag, attributes) = match &item.kind {
        ContextKind::File { path, rows } | ContextKind::ActiveFile { path, rows } => {
            let mut attributes = vec![("path", path.clone())];
            attributes.extend(rows.map(|rows| ("rows", rows.to_string())));
            ("file", attributes)
        }
        ContextKind::Selection { path, rows } => (
            "selection",
            vec![("path", path.clone()), ("rows", rows.to_string())],
        ),
        ContextKind::Diagnostics => ("diagnostics", Vec::new()),
        ContextKind::OpenEditors => ("open-editors", Vec::new()),
    };
    let mut text = format!("<{tag}");
    for (name, value) in attributes {
        text.push_str(&format!(" {name}=\"{}\"", escape_attribute(&value)));
    }
    text.push_str(">\n");
    text.push_str(&item.text);
    if !item.text.is_empty() && !item.text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&format!("</{tag}>"));
    text
}

fn escape_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
}
