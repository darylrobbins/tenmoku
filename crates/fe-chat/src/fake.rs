//! A scripted provider, for tests here and in the host product: it plays
//! replies from a list and records what it was asked. No network.

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use futures::Stream;

use crate::provider::{ChatEvent, ChatProvider, ChatRequest, ChatStream, ErrorKind, ProviderError};

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
