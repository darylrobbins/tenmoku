//! Request assembly, laid out for the prompt cache (the record's § 5): no
//! tools in v1, then one constant system block with an explicit breakpoint,
//! then the history, then the new turn, its context first and the question
//! last. Nothing per-workspace, per-request, timed or random goes before the
//! new turn, so each request's prefix is byte-identical to the last one's.
//!
//! JSON built here is built with its keys in sorted order, so it serialises
//! the same whether or not another crate turns on serde_json's
//! `preserve_order` (features unify across the workspace). Received blocks
//! are kept as parsed.

use serde_json::{Map, Value};

use crate::context::{ContextItem, ContextKind};
use crate::conversation::Conversation;
use crate::provider::{ApiMessage, CacheControl, ChatRequest, Effort, Role, SystemBlock};

/// The chat's instructions. Versioned in code and never built at runtime:
/// changing it empties every chat's cache.
pub const SYSTEM_PROMPT: &str = "\
You are the AI assistant in fastedit, a code editor. You help the user \
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
/// sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatSettings {
    pub model: String,
    pub effort: Effort,
    pub show_thinking: bool,
    pub max_tokens: u32,
}

impl Default for ChatSettings {
    fn default() -> ChatSettings {
        ChatSettings {
            model: "claude-opus-5-5".to_owned(),
            effort: Effort::Medium,
            show_thinking: false,
            max_tokens: 64_000,
        }
    }
}

/// The request for a new turn of `conversation`: the system prompt, its
/// history, then `new_user_content`. Call it before or after
/// [`Conversation::begin_turn`]: the streaming turn isn't in the history.
pub fn build_request(
    conversation: &Conversation,
    new_user_content: Vec<Value>,
    settings: &ChatSettings,
) -> ChatRequest {
    let mut messages = conversation.history();
    messages.push(ApiMessage {
        role: Role::User,
        content: new_user_content,
    });
    ChatRequest {
        model: conversation.model.clone(),
        system: vec![SystemBlock {
            text: SYSTEM_PROMPT.to_owned(),
            cache_control: Some(CacheControl::Ephemeral),
        }],
        tools: Vec::new(),
        messages,
        max_tokens: settings.max_tokens,
        effort: conversation.effort,
        show_thinking: conversation.show_thinking,
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
