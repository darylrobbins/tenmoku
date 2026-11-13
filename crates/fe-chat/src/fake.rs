//! A scripted provider, for tests here and in the host product: it plays
//! replies from a list and records what it was asked. No network.
//! [`ReplyScript`] writes a reply's events, tool calls included, so a test
//! scripts a multi-round turn as one reply per round.

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use futures::Stream;
use serde_json::{Map, Value};

use crate::provider::{
    BlockKind, ChatEvent, ChatProvider, ChatRequest, ChatStream, ErrorKind, ProviderError,
    StopReason,
};

/// Plays one scripted reply per [`ChatProvider::stream`] call, in order,
/// yielding to the executor between events. Each reply is a list of
/// events, which may end in a [`ChatEvent::Failed`]. Once the replies run
/// out, a stream fails at once.
pub struct ScriptedProvider {
    replies: Mutex<VecDeque<Vec<ChatEvent>>>,
    requests: Mutex<Vec<ChatRequest>>,
    dropped: Arc<AtomicUsize>,
}

impl ScriptedProvider {
    pub fn new(replies: impl IntoIterator<Item = Vec<ChatEvent>>) -> ScriptedProvider {
        ScriptedProvider {
            replies: Mutex::new(replies.into_iter().collect()),
            requests: Mutex::new(Vec::new()),
            dropped: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Queues one more reply, after those left.
    pub fn push(&self, reply: Vec<ChatEvent>) {
        self.replies.lock().expect("replies lock").push_back(reply);
    }

    /// Every request it was given, in order.
    pub fn requests(&self) -> Vec<ChatRequest> {
        self.requests.lock().expect("requests lock").clone()
    }

    /// How many of its streams were dropped before their reply ended.
    pub fn dropped(&self) -> usize {
        self.dropped.load(Ordering::SeqCst)
    }
}

impl ChatProvider for ScriptedProvider {
    fn name(&self) -> &str {
        "scripted"
    }

    fn stream(&self, request: ChatRequest) -> ChatStream {
        self.requests.lock().expect("requests lock").push(request);
        let events = self
            .replies
            .lock()
            .expect("replies lock")
            .pop_front()
            .unwrap_or_else(|| {
                let error = ProviderError::new(
                    ErrorKind::InvalidRequest,
                    "the scripted provider has no reply left",
                );
                vec![ChatEvent::Failed(error)]
            });
        Box::pin(ScriptedStream {
            events: events.into(),
            yielded: false,
            dropped: Arc::clone(&self.dropped),
        })
    }
}

struct ScriptedStream {
    events: VecDeque<ChatEvent>,
    /// Whether it yielded since the last event, so the next poll gives one.
    yielded: bool,
    dropped: Arc<AtomicUsize>,
}

impl Stream for ScriptedStream {
    type Item = ChatEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<ChatEvent>> {
        if self.events.is_empty() {
            return Poll::Ready(None);
        }
        if !self.yielded {
            self.yielded = true;
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        self.yielded = false;
        Poll::Ready(self.events.pop_front())
    }
}

impl Drop for ScriptedStream {
    fn drop(&mut self) {
        if !self.events.is_empty() {
            self.dropped.fetch_add(1, Ordering::SeqCst);
        }
    }
}

/// One scripted reply's events, as the Anthropic provider streams them:
/// `Started`, each block started, streamed and finished, then how it ends.
/// Blocks are indexed in the order they're added.
#[derive(Clone, Debug)]
pub struct ReplyScript {
    events: Vec<ChatEvent>,
    next: usize,
}

impl ReplyScript {
    /// A reply from `model`, with message id `message_id`.
    pub fn new(model: &str, message_id: &str) -> ReplyScript {
        ReplyScript {
            events: vec![ChatEvent::Started {
                model: model.to_owned(),
                message_id: message_id.to_owned(),
            }],
            next: 0,
        }
    }

    fn index(&mut self) -> usize {
        self.next += 1;
        self.next - 1
    }

    /// A finished thinking block.
    pub fn thinking(mut self, thinking: &str, signature: &str) -> ReplyScript {
        let index = self.index();
        self.events.extend([
            ChatEvent::BlockStarted {
                index,
                kind: BlockKind::Thinking,
            },
            ChatEvent::ThinkingDelta {
                index,
                text: thinking.to_owned(),
            },
            ChatEvent::BlockFinished {
                index,
                block: object([
                    ("signature", signature.into()),
                    ("thinking", thinking.into()),
                    ("type", "thinking".into()),
                ]),
            },
        ]);
        self
    }

    /// A finished text block.
    pub fn text(mut self, text: &str) -> ReplyScript {
        let index = self.index();
        self.events.extend([
            ChatEvent::BlockStarted {
                index,
                kind: BlockKind::Text,
            },
            ChatEvent::TextDelta {
                index,
                text: text.to_owned(),
            },
            ChatEvent::BlockFinished {
                index,
                block: object([("text", text.into()), ("type", "text".into())]),
            },
        ]);
        self
    }

    /// A text block that started and streamed `text` but didn't finish.
    pub fn partial_text(mut self, text: &str) -> ReplyScript {
        let index = self.index();
        self.events.extend([
            ChatEvent::BlockStarted {
                index,
                kind: BlockKind::Text,
            },
            ChatEvent::TextDelta {
                index,
                text: text.to_owned(),
            },
        ]);
        self
    }

    /// A finished `tool_use` block calling `name` with `input`, its input
    /// streamed as one delta.
    pub fn tool_use(self, id: &str, name: &str, input: Value) -> ReplyScript {
        let partial_json = input.to_string();
        self.tool_block(id, name, &partial_json, Some(input))
    }

    /// A finished `tool_use` block whose streamed input `raw` isn't JSON:
    /// kept as the raw string, as the Anthropic provider keeps it.
    pub fn invalid_tool_use(self, id: &str, name: &str, raw: &str) -> ReplyScript {
        self.tool_block(id, name, raw, Some(Value::String(raw.to_owned())))
    }

    /// A `tool_use` block whose input is still streaming: started, with
    /// `partial_json` so far, not finished.
    pub fn tool_input(self, id: &str, name: &str, partial_json: &str) -> ReplyScript {
        self.tool_block(id, name, partial_json, None)
    }

    fn tool_block(
        mut self,
        id: &str,
        name: &str,
        partial_json: &str,
        input: Option<Value>,
    ) -> ReplyScript {
        let index = self.index();
        self.events.extend([
            ChatEvent::BlockStarted {
                index,
                kind: BlockKind::ToolUse {
                    id: id.to_owned(),
                    name: name.to_owned(),
                },
            },
            ChatEvent::ToolInputDelta {
                index,
                partial_json: partial_json.to_owned(),
            },
        ]);
        if let Some(input) = input {
            self.events.push(ChatEvent::BlockFinished {
                index,
                block: object([
                    ("id", id.into()),
                    ("input", input),
                    ("name", name.into()),
                    ("type", "tool_use".into()),
                ]),
            });
        }
        self
    }

    /// The events so far, ending as the reply ends with `stop_reason`.
    pub fn stop(mut self, stop_reason: StopReason) -> Vec<ChatEvent> {
        self.events.push(ChatEvent::Finished {
            stop_reason,
            stop_details: None,
        });
        self.events
    }

    /// The events so far, then a failure.
    pub fn fail(mut self, error: ProviderError) -> Vec<ChatEvent> {
        self.events.push(ChatEvent::Failed(error));
        self.events
    }

    /// The events so far, the reply still streaming: what arrived before a
    /// Stop or a withdrawal.
    pub fn streaming(self) -> Vec<ChatEvent> {
        self.events
    }
}

/// A JSON object of `fields`, given in sorted order.
fn object<const N: usize>(fields: [(&str, Value); N]) -> Value {
    let mut object = Map::new();
    for (name, value) in fields {
        object.insert(name.to_owned(), value);
    }
    Value::Object(object)
}
