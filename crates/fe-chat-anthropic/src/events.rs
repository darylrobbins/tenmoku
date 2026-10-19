//! The Messages API's stream events as [`ChatEvent`]s (the record's § 3).
//! Each content block is rebuilt from its start and deltas into the block
//! the API would give back unstreamed, which the conversation then echoes.

use std::collections::BTreeMap;

use fe_chat::{BlockKind, ChatEvent, ErrorKind, ProviderError, StopReason, Usage};
use serde_json::Value;

use crate::sse::SseEvent;

/// Turns one reply's [`SseEvent`]s into [`ChatEvent`]s.
#[derive(Debug, Default)]
pub struct EventDecoder {
    /// Blocks started and not yet stopped, by index.
    blocks: BTreeMap<usize, Block>,
    usage: Usage,
    stop_reason: Option<StopReason>,
    stop_details: Option<Value>,
    /// Whether `message_stop` or an `error` ended the reply.
    ended: bool,
}

/// The stream events read; any other is ignored.
const KNOWN_EVENTS: [&str; 7] = [
    "message_start",
    "content_block_start",
    "content_block_delta",
    "content_block_stop",
    "message_delta",
    "message_stop",
    "error",
];

#[derive(Debug)]
struct Block {
    /// The block as `content_block_start` gave it.
    start: Value,
    text: String,
    thinking: String,
    signature: Option<String>,
    partial_json: String,
    /// Whether an `input_json_delta` arrived, so the input is the deltas'
    /// (even an empty one, which isn't JSON) rather than the start's.
    input_streamed: bool,
}

impl EventDecoder {
    pub fn new() -> EventDecoder {
        EventDecoder::default()
    }

    /// Whether the reply ended, with `message_stop` or an `error` event.
    pub fn ended(&self) -> bool {
        self.ended
    }

    /// The chat events `event` stands for: none, one, or (for
    /// `message_start` and `message_delta`) two.
    pub fn decode(&mut self, event: &SseEvent) -> Vec<ChatEvent> {
        if self.ended {
            return Vec::new();
        }
        // The event's name says what it is; an unnamed one (which the API
        // doesn't send) goes by its data's `type`. Pings and unknown events
        // are passed over before their data is read.
        let named = event.event != "message";
        if named && !KNOWN_EVENTS.contains(&event.event.as_str()) {
            if event.event != "ping" {
                log::debug!(
                    "fe-chat-anthropic: ignoring a `{}` stream event",
                    event.event
                );
            }
            return Vec::new();
        }
        let data: Value = match serde_json::from_str(&event.data) {
            Ok(data) => data,
            Err(error) if !named => {
                log::debug!(
                    "fe-chat-anthropic: ignoring an unnamed event that isn't JSON: {error}"
                );
                return Vec::new();
            }
            Err(error) => {
                self.ended = true;
                let message = format!(
                    "the stream sent an unreadable `{}` event: {error}",
                    event.event
                );
                return vec![ChatEvent::Failed(ProviderError::new(
                    ErrorKind::Stream,
                    message,
                ))];
            }
        };
        let kind = if named {
            event.event.as_str()
        } else {
            data["type"].as_str().unwrap_or_default()
        };
        match kind {
            "message_start" => self.message_start(&data["message"]),
            "content_block_start" => self.block_start(&data),
            "content_block_delta" => self.block_delta(&data),
            "content_block_stop" => self.block_stop(&data),
            "message_delta" => self.message_delta(&data),
            "message_stop" => {
                self.ended = true;
                vec![ChatEvent::Finished {
                    stop_reason: self
                        .stop_reason
                        .clone()
                        .unwrap_or_else(|| StopReason::Other(String::new())),
                    stop_details: self.stop_details.clone(),
                }]
            }
            "ping" => Vec::new(),
            "error" => {
                self.ended = true;
                let error = &data["error"];
                let message = match (error["type"].as_str(), error["message"].as_str()) {
                    (_, Some(message)) => message.to_owned(),
                    (Some(error_type), None) => error_type.to_owned(),
                    (None, None) => "the stream sent an error".to_owned(),
                };
                vec![ChatEvent::Failed(ProviderError::new(
                    ErrorKind::Stream,
                    message,
                ))]
            }
            other => {
                log::debug!("fe-chat-anthropic: ignoring a `{other}` stream event");
                Vec::new()
            }
        }
    }

    fn message_start(&mut self, message: &Value) -> Vec<ChatEvent> {
        merge_usage(&mut self.usage, &message["usage"]);
        vec![
            ChatEvent::Started {
                model: string(&message["model"]),
                message_id: string(&message["id"]),
            },
            ChatEvent::Usage(self.usage),
        ]
    }

    fn block_start(&mut self, data: &Value) -> Vec<ChatEvent> {
        let Some(index) = index(data) else {
            return Vec::new();
        };
        let start = data["content_block"].clone();
        let kind = block_kind(&start);
        self.blocks.insert(
            index,
            Block {
                start,
                text: String::new(),
                thinking: String::new(),
                signature: None,
                partial_json: String::new(),
                input_streamed: false,
            },
        );
        vec![ChatEvent::BlockStarted { index, kind }]
    }

    fn block_delta(&mut self, data: &Value) -> Vec<ChatEvent> {
        let Some(index) = index(data) else {
            return Vec::new();
        };
        let Some(block) = self.blocks.get_mut(&index) else {
            log::debug!("fe-chat-anthropic: ignoring a delta to block {index}, which didn't start");
            return Vec::new();
        };
        let delta = &data["delta"];
        match delta["type"].as_str().unwrap_or_default() {
            "text_delta" => {
                let text = string(&delta["text"]);
                block.text.push_str(&text);
                vec![ChatEvent::TextDelta { index, text }]
            }
            "thinking_delta" => {
                let text = string(&delta["thinking"]);
                block.thinking.push_str(&text);
                vec![ChatEvent::ThinkingDelta { index, text }]
            }
            "signature_delta" => {
                block
                    .signature
                    .get_or_insert_with(String::new)
                    .push_str(&string(&delta["signature"]));
                Vec::new()
            }
            "input_json_delta" => {
                let partial_json = string(&delta["partial_json"]);
                block.partial_json.push_str(&partial_json);
                block.input_streamed = true;
                vec![ChatEvent::ToolInputDelta {
                    index,
                    partial_json,
                }]
            }
            other => {
                log::debug!("fe-chat-anthropic: ignoring a `{other}` delta to block {index}");
                Vec::new()
            }
        }
    }

    fn block_stop(&mut self, data: &Value) -> Vec<ChatEvent> {
        let Some(index) = index(data) else {
            return Vec::new();
        };
        let Some(block) = self.blocks.remove(&index) else {
            log::debug!(
                "fe-chat-anthropic: ignoring the stop of block {index}, which didn't start"
            );
            return Vec::new();
        };
        vec![ChatEvent::BlockFinished {
            index,
            block: block.finish(),
        }]
    }

    fn message_delta(&mut self, data: &Value) -> Vec<ChatEvent> {
        let delta = &data["delta"];
        if let Some(reason) = delta["stop_reason"].as_str() {
            self.stop_reason = Some(StopReason::from(reason));
        }
        if !delta["stop_details"].is_null() {
            self.stop_details = Some(delta["stop_details"].clone());
        }
        merge_usage(&mut self.usage, &data["usage"]);
        vec![ChatEvent::Usage(self.usage)]
    }
}

impl Block {
    /// The block as the API would echo it: its start, with the deltas'
    /// text, thinking and signature in place. A `tool_use` block's input is
    /// its joined `partial_json`, parsed strictly; when that isn't JSON, the
    /// input is kept as the raw string, which flags it (the API's input is
    /// always an object), and the caller must not run the tool.
    fn finish(self) -> Value {
        let mut block = self.start;
        let Some(fields) = block.as_object_mut() else {
            return block;
        };
        match fields
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "text" => {
                let mut text = fields.get("text").map(string).unwrap_or_default();
                text.push_str(&self.text);
                fields.insert("text".to_owned(), text.into());
            }
            "thinking" => {
                let mut thinking = fields.get("thinking").map(string).unwrap_or_default();
                thinking.push_str(&self.thinking);
                fields.insert("thinking".to_owned(), thinking.into());
                if let Some(signature) = self.signature {
                    fields.insert("signature".to_owned(), signature.into());
                }
            }
            "tool_use" if self.input_streamed => {
                let input = match serde_json::from_str::<Value>(&self.partial_json) {
                    Ok(input) => input,
                    Err(error) => {
                        log::warn!(
                            "fe-chat-anthropic: a tool_use block's input isn't JSON: {error}"
                        );
                        Value::String(self.partial_json)
                    }
                };
                fields.insert("input".to_owned(), input);
            }
            _ => {}
        }
        block
    }
}

/// The kind of a block the API started. A `fallback` block's `from` and `to`
/// are objects naming a `model` (the model-migration guide), which
/// [`BlockKind::of`] would read as empty strings, so they're read here.
fn block_kind(block: &Value) -> BlockKind {
    match BlockKind::of(block) {
        BlockKind::Fallback { .. } => {
            let model = |end: &Value| match end {
                Value::Object(end) => end.get("model").map(string).unwrap_or_default(),
                end => string(end),
            };
            BlockKind::Fallback {
                from: model(&block["from"]),
                to: model(&block["to"]),
            }
        }
        kind => kind,
    }
}

/// Copies the counts `usage` has into `into`. The stream's later usage
/// carries the final counts; a field it leaves out (or sends as null) keeps
/// the earlier one.
fn merge_usage(into: &mut Usage, usage: &Value) {
    let count = |name: &str, into: &mut u64| {
        if let Some(value) = usage[name].as_u64() {
            *into = value;
        }
    };
    count("input_tokens", &mut into.input_tokens);
    count("output_tokens", &mut into.output_tokens);
    count(
        "cache_creation_input_tokens",
        &mut into.cache_creation_input_tokens,
    );
    count("cache_read_input_tokens", &mut into.cache_read_input_tokens);
}

fn index(data: &Value) -> Option<usize> {
    let index = data["index"]
        .as_u64()
        .and_then(|index| usize::try_from(index).ok());
    if index.is_none() {
        log::debug!("fe-chat-anthropic: ignoring a block event without an index");
    }
    index
}

fn string(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}
