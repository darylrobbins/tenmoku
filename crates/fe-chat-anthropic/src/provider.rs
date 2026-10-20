//! [`AnthropicProvider`]: a [`ChatProvider`] over a [`Transport`], with the
//! record's retries (§ 3): 429 after `retry-after` (at most 60 s), and 500,
//! 529 and connection failures, each before the reply's first event, twice,
//! 1 s then 2 s apart plus up to 250 ms of jitter. Nothing is retried once
//! an event has arrived, and 400, 401, 402, 403, 404 and 413 never are.

use std::collections::VecDeque;
use std::collections::hash_map::RandomState;
use std::fmt;
use std::hash::BuildHasher;
use std::sync::Arc;
use std::time::Duration;

use fe_chat::{ChatEvent, ChatProvider, ChatRequest, ChatStream, ErrorKind, ProviderError};
use futures::StreamExt;
use futures::future::BoxFuture;
use kiln_command::Grants;

use crate::error::error_from_response;
use crate::events::EventDecoder;
use crate::sse::SseParser;
use crate::transport::{Body, Transport};
use crate::wire::{request_body, request_headers};

/// Where the Messages API is: `/v1/messages` is appended.
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";

/// How many times a request is sent again after its first try.
const MAX_RETRIES: usize = 2;
/// The HTTP statuses that are retried: rate limited, server error, overloaded.
const RETRIED_STATUSES: [u16; 3] = [429, 500, 529];
/// The waits before each retry, before jitter.
const BACKOFF: [Duration; MAX_RETRIES] = [Duration::from_secs(1), Duration::from_secs(2)];
/// Jitter added to a backoff is under this.
const MAX_JITTER_MS: u64 = 250;
/// The longest `retry-after` that is waited for.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);
/// The most of an error response's body that is read.
const MAX_ERROR_BODY: usize = 64 * 1024;

/// Waits for a duration: the hook the retries wait through. The app passes
/// one over `async-io`'s `Timer`; tests pass one that returns at once and
/// records what it was asked.
pub type Sleep = Arc<dyn Fn(Duration) -> BoxFuture<'static, ()> + Send + Sync>;

/// An Anthropic API key. Its `Debug` output never shows it, and it has no
/// `Display`.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    pub fn new(key: impl Into<String>) -> ApiKey {
        ApiKey(key.into())
    }

    /// The key itself, for the request's `x-api-key` header.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(<redacted>)")
    }
}

/// Claude through the Messages API, streamed.
pub struct AnthropicProvider {
    transport: Arc<dyn Transport>,
    key: ApiKey,
    url: String,
    fallbacks: bool,
    sleep: Sleep,
}

impl AnthropicProvider {
    /// A provider that sends to `base_url` (normally [`DEFAULT_BASE_URL`])
    /// through `transport`, with server-side fallbacks on and waits on a
    /// timer thread (see [`AnthropicProvider::with_sleep`]).
    pub fn new(
        transport: Arc<dyn Transport>,
        key: ApiKey,
        base_url: impl Into<String>,
    ) -> AnthropicProvider {
        let base_url = base_url.into();
        AnthropicProvider {
            transport,
            key,
            url: format!("{}/v1/messages", base_url.trim_end_matches('/')),
            fallbacks: true,
            sleep: Arc::new(thread_sleep),
        }
    }

    /// A provider that sends to [`DEFAULT_BASE_URL`] over HTTPS, through an
    /// [`fe_net::Client`] reaching what `grants` allow (the chat holds
    /// `net:api.anthropic.com`).
    pub fn with_client(grants: &Grants, key: ApiKey) -> AnthropicProvider {
        AnthropicProvider::new(Arc::new(fe_net::Client::new(grants)), key, DEFAULT_BASE_URL)
    }

    /// Whether a declined request re-runs on Anthropic's recommended
    /// fallback model (`fallbacks: "default"`; the `chat.fallbacks` setting).
    pub fn with_fallbacks(mut self, fallbacks: bool) -> AnthropicProvider {
        self.fallbacks = fallbacks;
        self
    }

    /// Waits between retries with `sleep`.
    pub fn with_sleep(
        mut self,
        sleep: impl Fn(Duration) -> BoxFuture<'static, ()> + Send + Sync + 'static,
    ) -> AnthropicProvider {
        self.sleep = Arc::new(sleep);
        self
    }
}

impl fmt::Debug for AnthropicProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnthropicProvider")
            .field("key", &self.key)
            .field("url", &self.url)
            .field("fallbacks", &self.fallbacks)
            .finish_non_exhaustive()
    }
}

impl ChatProvider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
    }

    fn stream(&self, request: ChatRequest) -> ChatStream {
        let mut run = Run {
            transport: Arc::clone(&self.transport),
            url: self.url.clone(),
            headers: request_headers(self.key.expose(), self.fallbacks),
            body: Vec::new(),
            sleep: Arc::clone(&self.sleep),
            retries: 0,
            phase: Phase::Connect,
            pending: VecDeque::new(),
        };
        match request_body(&request, self.fallbacks) {
            Ok(body) => run.body = body,
            Err(error) => {
                run.phase = Phase::Done;
                run.pending.push_back(ChatEvent::Failed(error));
            }
        }
        let stream = futures::stream::unfold(run, |mut run| async move {
            let event = run.next().await?;
            Some((event, run))
        });
        Box::pin(stream.fuse())
    }
}

/// Why a try failed, and whether the request may be sent again. Only
/// [`RETRIED_STATUSES`] and connection failures before the reply began are:
/// a 502 is a `Server` error, which [`ErrorKind::retryable`] calls
/// retryable, but the record retries 500 and 529 only.
struct Failure {
    error: ProviderError,
    retry: bool,
}

/// One request, from its first try to its reply's end.
struct Run {
    transport: Arc<dyn Transport>,
    url: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    sleep: Sleep,
    /// How many times it was sent again.
    retries: usize,
    phase: Phase,
    /// Events decoded and not yet given out.
    pending: VecDeque<ChatEvent>,
}

enum Phase {
    /// To be sent.
    Connect,
    /// Its reply is streaming.
    Reading {
        body: Body,
        parser: SseParser,
        decoder: Box<EventDecoder>,
        /// Whether an event arrived, after which nothing is retried.
        started: bool,
    },
    /// Over: the connection, if any, is dropped.
    Done,
}

impl Run {
    async fn next(&mut self) -> Option<ChatEvent> {
        loop {
            if let Some(event) = self.pending.pop_front() {
                return Some(event);
            }
            let failure = match &mut self.phase {
                Phase::Done => return None,
                Phase::Connect => match self.connect().await {
                    Ok(body) => {
                        self.phase = Phase::Reading {
                            body,
                            parser: SseParser::new(),
                            decoder: Box::default(),
                            started: false,
                        };
                        continue;
                    }
                    Err(failure) => failure,
                },
                Phase::Reading {
                    body,
                    parser,
                    decoder,
                    started,
                } => {
                    let cut = match body.next().await {
                        Some(Ok(bytes)) => {
                            for event in parser.push(&bytes) {
                                // Even an ignored event, such as a ping, means
                                // the reply began: nothing is retried after it.
                                *started = true;
                                self.pending.extend(decoder.decode(&event));
                                if decoder.ended() {
                                    break;
                                }
                            }
                            if decoder.ended() {
                                // Closes the connection.
                                self.phase = Phase::Done;
                            }
                            continue;
                        }
                        Some(Err(error)) => format!("the connection broke: {error}"),
                        None if *started => {
                            "the connection closed before the reply ended".to_owned()
                        }
                        None => "the connection closed before the reply started".to_owned(),
                    };
                    if *started {
                        self.phase = Phase::Done;
                        return Some(ChatEvent::Failed(ProviderError::new(
                            ErrorKind::Stream,
                            cut,
                        )));
                    }
                    Failure {
                        error: ProviderError::new(ErrorKind::Network, cut),
                        retry: true,
                    }
                }
            };
            if !failure.retry || self.retries == MAX_RETRIES {
                self.phase = Phase::Done;
                return Some(ChatEvent::Failed(failure.error));
            }
            let delay = self.delay(&failure.error);
            // Drops a broken connection before waiting.
            self.phase = Phase::Connect;
            self.retries += 1;
            log::debug!(
                "fe-chat-anthropic: retrying in {delay:?} after {}",
                failure.error
            );
            (self.sleep)(delay).await;
        }
    }

    /// Sends the request, giving the body of a 2xx response and an error
    /// for anything else.
    fn connect(&self) -> BoxFuture<'static, Result<Body, Failure>> {
        let sent = self
            .transport
            .post(&self.url, self.headers.clone(), self.body.clone());
        Box::pin(async move {
            let response = sent.await.map_err(|error| Failure {
                error: ProviderError::new(ErrorKind::Network, error.message),
                retry: true,
            })?;
            if (200..300).contains(&response.status) {
                return Ok(response.body);
            }
            let mut body = response.body;
            let mut bytes = Vec::new();
            while let Some(Ok(chunk)) = body.next().await {
                bytes.extend_from_slice(&chunk);
                if bytes.len() >= MAX_ERROR_BODY {
                    break;
                }
            }
            Err(Failure {
                error: error_from_response(response.status, &response.headers, &bytes),
                retry: RETRIED_STATUSES.contains(&response.status),
            })
        })
    }

    /// How long to wait before the next retry after `error`.
    fn delay(&self, error: &ProviderError) -> Duration {
        match error.retry_after {
            Some(after) if error.kind == ErrorKind::RateLimited => after.min(MAX_RETRY_AFTER),
            _ => BACKOFF[self.retries] + jitter(),
        }
    }
}

/// Under [`MAX_JITTER_MS`], from the standard library's per-process random
/// hash keys.
fn jitter() -> Duration {
    Duration::from_millis(RandomState::new().hash_one(()) % MAX_JITTER_MS)
}

/// Waits on a thread of its own, so it works under any executor.
fn thread_sleep(duration: Duration) -> BoxFuture<'static, ()> {
    let (done, waited) = futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        std::thread::sleep(duration);
        let _ = done.send(());
    });
    Box::pin(async move {
        let _ = waited.await;
    })
}
