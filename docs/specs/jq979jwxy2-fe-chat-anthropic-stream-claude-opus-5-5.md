---
task_uid: "01m57yv43sw0mcq7jq979jwxy2"
task: "T192"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-chat-anthropic test sse_parses_across_any_split (tests/sse.rs): every fixture parsed whole, fed one byte at a time, and split inside a CRLF and inside a multi-byte UTF-8 character gives the same events; multi-line data joins with a newline; comment lines and unknown fields are ignored"
    required: true
    manual: false
  - id: "C2"
    text: "fe-chat-anthropic test the_request_matches_the_record (tests/provider.rs): the body has model claude-opus-5-5, max_tokens 64000, stream true, output_config.effort medium, fallbacks default, top-level cache_control ephemeral, no thinking key unless show_thinking (then type adaptive, display summarized), no temperature, top_p, top_k, tool_choice or tools, and ends with the user's message; headers x-api-key, anthropic-version 2023-06-01, and anthropic-beta server-side-fallback-2026-07-01 only when fallbacks are on"
    required: true
    manual: false
  - id: "C3"
    text: "fe-chat-anthropic test fixtures_become_chat_events (tests/provider.rs): the text, summarized-thinking (summary and signature in the finished block), max_tokens, pre-output and mid-stream refusal (stop_details kept), fallback (Started names the fallback model, a Fallback block first), tool_use (input parsed at block stop) and mid-stream error fixtures give the expected ChatEvent sequences, and each finished block equals the block the API would echo"
    required: true
    manual: false
  - id: "C4"
    text: "fe-chat-anthropic test errors_are_typed_and_only_transient_ones_retry (tests/provider.rs): a 429 with retry-after 3 waits 3 s and retries; 500 and 529 retry after 1 s then 2 s plus under 250 ms jitter and fail after two retries; a transport error before any event retries; 400, 401, 402, 403, 404 and 413 fail at once with their ErrorKind and the API's message; a cut after the first event fails as Stream with no retry; dropping the stream drops the transport's body"
    required: true
    manual: false
  - id: "C5"
    text: "scripts/check-no-gpui.sh reports ok: fe-chat-anthropic; an ApiKey's Debug output doesn't contain the key; cargo deny check passes"
    required: true
    manual: false
---

# fe-chat-anthropic: stream Claude Opus 5.5 replies from the Messages API over SSE, with typed errors and retries, tested on recorded fixtures (Mac T964)

# fe-chat-anthropic: stream Claude's replies from the Messages API over SSE, tested on recorded fixtures

## Problem

The AI chat decision record (`docs/decisions/*-ai-chat.md`; signed off by the user) backs the
chat with the Anthropic Messages API, raw HTTP with SSE streaming, since there's no official Rust
SDK (record §3). `fe-chat` (the card this depends on) defines `ChatProvider` and its events
but nothing implements it against the API. This card does, with no network: the transport is a
trait, and tests feed recorded SSE bytes through a fake one. The HTTPS transport is the next
card.

## Change

New crate `crates/fe-chat-anthropic` ("Claude through the Anthropic Messages API, for fe-chat.
Must not depend on GPUI."), registered in the workspace `Cargo.toml`. Dependencies: `fe-chat`,
`serde`, `serde_json`, `futures`, `http` (already in the lock). Follow record §3 and §14
exactly; the record wins where this card differs.

- `src/transport.rs`: `trait Transport: Send + Sync { fn post(&self, url: &str, headers:
  Vec<(String, String)>, body: Vec<u8>) -> BoxFuture<'static, Result<Response, TransportError>> }`,
  `Response { status: u16, headers, body: BoxStream<'static, Result<Bytes, TransportError>> }`
  (`Bytes` as `Vec<u8>` if `bytes` isn't wanted). Dropping the body stream must close the
  connection; the transport's docs say so.
- `src/sse.rs`: an incremental SSE parser (`SseParser::push(&[u8]) -> Vec<SseEvent>`):
  `event:` and `data:` fields, multi-line `data` joined with `\n`, `:` comment lines and unknown
  fields ignored, LF, CR and CRLF line ends, a blank line dispatching. Bytes may split anywhere,
  inside a line, a CRLF or a UTF-8 sequence.
- `src/wire.rs`: the request body and headers (record §3): `model`, `max_tokens`,
  `stream: true`, `system` (with its `cache_control`), `messages`,
  `output_config: {"effort": …}`, `thinking: {"type": "adaptive", "display": "summarized"}`
  only when the conversation shows thinking (otherwise no `thinking` key), `fallbacks:
  "default"` when fallbacks are on, top-level `cache_control: {"type": "ephemeral"}`, `tools`
  only when non-empty (each with `strict` and `eager_input_streaming: true`), never
  `temperature`, `top_p`, `top_k`, `tool_choice` `any`/`tool`, or an assistant message last.
  Headers: `x-api-key`, `anthropic-version: 2023-06-01`, `content-type: application/json`,
  `user-agent: fastedit/<version>`, and `anthropic-beta: server-side-fallback-2026-07-01` only
  with `fallbacks: "default"`.
- `src/events.rs`: SSE events to `ChatEvent`s. `message_start` → `Started` (its `model`, `id`)
  and its usage; `content_block_start` → `BlockStarted` (`text`, `thinking`,
  `redacted_thinking`, `tool_use`, `fallback` with `from`/`to`, anything else `Other`);
  `text_delta`, `thinking_delta`, `input_json_delta` → their deltas; `signature_delta` kept
  for the block; `content_block_stop` → `BlockFinished` with the block rebuilt verbatim (a
  `tool_use` block's input parsed strictly from the joined `partial_json`, an invalid one kept
  as the raw string and flagged); `message_delta` → `Usage`, and `Finished` with
  `stop_reason`/`stop_details` at `message_stop`; `ping` ignored; `error` → `Failed(Stream)`.
  Unknown event types are ignored with a debug log.
- `src/error.rs`: a non-2xx response's status and JSON `error.type`/`message` to
  `ProviderError` (record §2's kinds), `retry-after` parsed into `retry_after`.
- `src/provider.rs`: `AnthropicProvider::new(transport, key: ApiKey, base_url)` implementing
  `ChatProvider`. `ApiKey` (`Debug` prints `ApiKey(<redacted>)`) lives here. Retries per record
  §3: 429 after `retry-after` (capped at 60 s), 500, 529 and transport errors before the first
  event, twice, 1 s then 2 s plus up to 250 ms jitter; nothing once events have arrived; never
  on 400, 401, 402, 403, 404 or 413. The waits use a `Sleep` hook (`async-io`'s `Timer` in the
  app; tests pass an instant one and record the asked durations).

Fixtures in `crates/fe-chat-anthropic/tests/fixtures/`, `.sse` files written to the documented
event shapes (the claude-api skill's `curl/examples.md` and the model-migration guide; there is
no key on the lanes to record real ones): a text reply; a summarized-thinking reply with
`signature_delta`; a `max_tokens` stop; a pre-output `refusal` with `stop_details`; a mid-stream
refusal after text; a pre-output fallback (`message_start` naming the fallback model, a
`fallback` block first, `usage.iterations` with a `fallback_message`); a mid-stream `error`
event (`overloaded_error`); a `tool_use` block with `input_json_delta`s. Each fixture's header
comment says what it shows and where its shape comes from.

Tests in `crates/fe-chat-anthropic/tests/sse.rs` and `crates/fe-chat-anthropic/tests/provider.rs`
(top-level fns), through a `FakeTransport` in the test that replays a fixture in chunks and
records requests. The live test against the real API comes with the HTTPS transport (next card).

## Implementation notes

- **Fallback blocks.** The model-migration guide gives a `fallback` block's `from` and `to`
  as objects (`{"from": {"model": …}, "to": {"model": …}}`). `fe_chat::BlockKind::of` reads
  them as strings, so it gives empty names; `BlockStarted` here reads `from.model` and
  `to.model` itself. `Conversation::apply` still re-derives the kind with `BlockKind::of` on
  `BlockFinished`, which blanks the names on the turn's block: a one-line follow-up for
  `fe-chat` (outside this card's scope).
- **Fallbacks are a provider setting.** `ChatRequest` has no fallbacks field, so
  `AnthropicProvider::with_fallbacks(bool)` (on by default) carries `chat.fallbacks`.
- **Waits.** `with_sleep` takes the hook; without one the provider waits on a thread of its
  own (works under any executor), so the crate doesn't depend on `async-io`. Jitter comes
  from the standard library's `RandomState`, so no `rand`.
- **Retry boundary.** "Before the first event" means before the first whole SSE event, a
  `ping` or an ignored one included: a connection that fails, breaks or closes before one
  is retried as `Network`, and after one fails as `Stream`. HTTP retries are by status
  (429, 500, 529 only): a 502 or 503 is a `Server` error, which `ErrorKind::retryable`
  calls retryable, but it fails at once. An over-long `retry-after` saturates, then caps at
  60 s.
- **Ignored events** (`ping`, unknown names) are passed over by name before their data is
  read, so empty or non-JSON data there is harmless.
- **Invalid tool input** is kept as the raw JSON string in the block's `input` (the API's
  input is always an object, so a string is the flag) and logged as a warning. Once any
  `input_json_delta` arrived, the input is the deltas' (an empty join is invalid), never the
  start block's `{}`.
- `usage.iterations` isn't surfaced: `fe_chat::Usage` has no field for it, and
  `Started`'s model already says which model served the reply.
- `log` is a dependency too, for the debug log of ignored events (already in the lock).
