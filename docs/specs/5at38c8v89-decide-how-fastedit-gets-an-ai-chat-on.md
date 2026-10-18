---
task_uid: "01m55vmncw7fgp2c5at38c8v89"
task: "T961"
rev: 3
type: "docs"
complexity: "s"
criteria:
  - id: "C1"
    text: "docs/decisions/\u003cthis task's key\u003e-ai-chat.md exists, status proposed, with every section of the spec's draft: crate layout, provider trait and tool seam, the Anthropic provider (claude-opus-5-5, adaptive thinking, effort, fallbacks default with server-side-fallback-2026-07-01, SSE, retries), conversation model and append-only history, prompt caching layout, HTTP client and licences, key storage, code blocks as proposals, the AI actor and capabilities and how they relate to T839, privacy, persistence, settings, context and its budget, the errors and refusals table, testing, build cards, out of scope"
    required: true
    manual: false
  - id: "C2"
    text: "Every file, symbol and line number the record cites exists at the base, or is corrected; each HTTP and keychain crate's licence is stated as checked against deny.toml (with the cargo deny result if run); a crate needing a licence deny.toml lacks is an open question, not chosen; the API facts match the claude-api skill's reference files as the spec quotes them"
    required: true
    manual: false
  - id: "C3"
    text: "The record ends with its open questions for the user, and the finish notes list them and ask the USER to sign off the record before any ai-chat build card is readied"
    required: true
    manual: false
---

# Decide how fastedit gets an AI chat on Claude (crates, provider, HTTP client, key storage, context, caching, privacy), in a decision record the user signs off

# Decide how fastedit gets an AI chat, in a decision record the user signs off

## Problem

fastedit is the code surface of an AI-first dev tool (`docs/ai-host.md`), and it has every AI
seam a host needs: ghost text, streamed edits, `ProposedChange` diff blocks with Accept and
Reject (`crates/fe-buffer/src/proposed.rs`, `EditorView::set_proposals` in
`crates/fe-gpui/src/view/proposal.rs`), AI-origin undo (`Origin::Ai { agent, session }`,
`crates/fe-buffer/src/buffer.rs`). Only a scripted fake drives them
(`crates/fe-editor/src/scripted_ai.rs`, `crates/fe-gpui/examples/ai_demo.rs`). Nothing in
the repo talks to a model: there's no network client at all (Cargo.lock has
`gpui-pre-http-client`'s trait but no implementation, no TLS crate; the Markdown preview
blocks remote images for that reason, T847). kiln's example host, Kiln Chat
(`kiln/crates/kiln/examples/host/conversation.rs`, `threads.rs`), is a chat UI with no backend.

The USER decided on 2026-10-17:

1. **Home**: a reusable headless crate (conversation, provider trait, tools, context) plus a
   kiln chat panel in fastedit's workbench, so the future host product reuses both.
2. **Backend**: the Anthropic Messages API behind a provider trait. There is no official Rust
   SDK, so it's raw HTTP: `POST https://api.anthropic.com/v1/messages` with SSE streaming.
   API key in the macOS Keychain (the platform's equivalent elsewhere), never in settings
   files; `ANTHROPIC_API_KEY` honoured for development.
3. **v1 scope**: streamed Markdown chat; context from @file, the selection, diagnostics and
   open editors; code blocks applied as `ProposedChange` diff blocks (accept/reject,
   `Origin::Ai`). No autonomous agent tool loop in v1 (that's next; the provider/tool seam
   must fit it). All AI actions go through `fe-command` (`kiln-command`) `Invocation`s with an
   AI actor. The plugin decision draft (T839) says "The product's own AI features run as
   plugins under the same rules, with capabilities granted by the app"; this record says how
   the built-in chat relates to that.
4. **Rendering** reuses fe-markdown (T846) and fe-markdown-view (T847).

## Change

Write `docs/decisions/<this task's key>-ai-chat.md` (new; the key `rgt task show` prints for
this task) from the draft below. Check every file, symbol and line it cites at the base and
fix what has moved; tighten the prose. Don't change its decisions without saying so in the
finish notes. Commit it. Status line: **proposed**, until the user signs it off.

The API facts in the draft come from the claude-api skill's reference files of 2026-10-17
(`curl/examples.md`, `shared/tool-use-concepts.md`, `shared/prompt-caching.md`,
`shared/error-codes.md`, and `shared/model-migration.md` § Migrating to Claude Opus 5.5).
Keep them as written; don't "correct" a beta header to a newer-looking date.

For the HTTP and keychain crates: check each one's licence on crates.io against `deny.toml`'s
allow list and say what you found in the record; run `cargo deny check licenses` on a scratch
branch with the dependencies added if you can, and record the result (don't commit that
branch). If a crate needs a licence `deny.toml` doesn't allow, the record names it as an open
question for the user instead of choosing it.

**The USER signs off the final record.** It sets policy: what leaves the machine, where code
and chats are kept, the first network client. The finish notes list the record's open
questions and ask the user to sign the record off before any build card is readied.

---

## Draft record

### 1. Decision

A built-in chat panel backed by Claude through the Anthropic Messages API, in five crates
and three workbench modules:

| Crate / module | GPUI? | What it holds |
|---|---|---|
| `crates/fe-chat` | no | the conversation model, the provider trait and its events, request assembly (the cache layout), context items and budgeting, code-block edit targets, chat persistence, a scripted fake provider |
| `crates/fe-chat-anthropic` | no | the SSE parser, the Messages API wire format, the provider over a `Transport` trait, retries |
| `crates/fe-net` | no | the repo's first network client: HTTPS over async-io with a host allowlist |
| `crates/fe-chat-view` | yes | the chat view (transcript, composer, attachment chips, states), knowing nothing of the workbench |
| `kiln-command` `capability.rs`, `kiln-core` `secrets.rs` | no | capability strings and grants (T839's format), and a secret store over the platform keychain |
| `fe-workbench` `chat_panel.rs`, `chat_context.rs`, `chat_apply.rs` | yes | the sidebar panel, context from the workspace, code blocks applied as proposals |

The host product takes `fe-chat`, `fe-chat-anthropic`, `fe-net` and `fe-chat-view` and supplies
its own context sources and panel, as fastedit's workbench does. `fe-chat` and
`fe-chat-anthropic` know nothing of fastedit; both are headless and in
`scripts/check-no-gpui.sh`'s core set.

### 2. Provider trait

```rust
pub trait ChatProvider: Send + Sync {
    fn name(&self) -> &str;                       // "anthropic"
    fn stream(&self, request: ChatRequest) -> ChatStream; // dropping the stream cancels
}
pub struct ChatRequest { model, system: Vec<SystemBlock>, tools: Vec<ToolSpec>,
    messages: Vec<ApiMessage>, max_tokens: u32, effort: Effort, show_thinking: bool }
pub enum ChatEvent {
    Started { model: String, message_id: String },
    BlockStarted { index: usize, kind: BlockKind },   // Text | Thinking | RedactedThinking
                                                      // | ToolUse { id, name } | Fallback { from, to } | Other(type)
    TextDelta { index, text }, ThinkingDelta { index, text }, ToolInputDelta { index, partial_json },
    BlockFinished { index, block: serde_json::Value }, // the whole block, verbatim, as the API would echo it
    Usage(Usage),                                      // input, output, cache_creation, cache_read
    Finished { stop_reason: StopReason, stop_details: Option<Value> },
    Failed(ProviderError),
}
```

- `ChatStream` is a `futures::Stream<Item = ChatEvent>`; dropping it closes the connection,
  which stops generation server-side. There is no separate cancel call.
- `ProviderError { kind, message, retry_after }`, `kind` one of `Network`, `RateLimited`
  (429), `Overloaded` (529), `Server` (500), `Auth` (401), `Billing` (402), `Permission` (403),
  `NotFound` (404), `TooLarge` (413), `InvalidRequest` (400), `Stream` (an SSE `error` event or
  a cut connection after the stream started). `retryable()` is true for the first four only.
- **Tools, for the agent loop that comes next.** `ToolSpec { name, description, input_schema,
  strict }` and `ToolChoice` with only `Auto` and `None`: forced `tool_choice` (`any`, `tool`)
  is a 400 on Claude Opus 5.5, so steering is by prompt plus `strict: true`, and the caller
  checks a call was made. A streamed request with client tools sets
  `eager_input_streaming: true` on each tool; a tool's input is parsed strictly at
  `content_block_stop` and validated against its schema before anything runs; a turn whose
  `stop_reason` is `max_tokens` or `refusal` runs none of its tools. Tool results go back as
  `tool_result` blocks. The full tool set is declared at a conversation's first request
  (adding a tool later breaks the prompt cache and the thinking-block binding; the agent loop
  uses `tool_addition` blocks, beta `mid-conversation-tool-changes-2026-07-01`). Running a
  tool is dispatching an `Invocation` with the AI actor (§9). v1 sends no tools.

### 3. The Anthropic provider

- `POST https://api.anthropic.com/v1/messages`, headers `x-api-key`,
  `anthropic-version: 2023-06-01`, `content-type: application/json`,
  `anthropic-beta: server-side-fallback-2026-07-01`, and a `user-agent` naming fastedit and its
  version.
- Body: `model` (default **`claude-opus-5-5`**, exactly, no date suffix), `max_tokens: 64000`,
  `stream: true`, `system`, `messages`, `output_config: {effort}` (default `medium`, the
  model's own default), `fallbacks: "default"`, and top-level `cache_control` (§5). Thinking
  is always on for Claude Opus 5.5 and can't be disabled (`{"type":"disabled"}` and
  `budget_tokens` are 400s): the request omits `thinking`, or, when the user shows thinking,
  sends `{"type": "adaptive", "display": "summarized"}`. No assistant prefill (a 400). No
  sampling parameters.
- **Server-side fallback, on by default** (`fallbacks: "default"` with exactly that beta
  header; the array form takes a different header, and mixing them is a 400). A classifier
  decline re-runs on Anthropic's recommended fallback model inside the same call. The reply
  then names the model that served it (`message_start`'s `model`; a `fallback_message` entry in
  `usage.iterations`). `reasoning_extraction` declines are not retried. **The user is told
  this is on** (the privacy notice, §10), and can turn it off (`chat.fallbacks`, §12).
- **SSE**: `message_start`, `content_block_start`, `content_block_delta` (`text_delta`,
  `thinking_delta`, `signature_delta`, `input_json_delta`), `content_block_stop`,
  `message_delta` (`stop_reason`, `stop_details`, usage), `message_stop`, `ping`, `error`.
  Unknown event and block types are kept, not errors. A block is rebuilt verbatim from its
  start and deltas (thinking text plus `signature`; `redacted_thinking`'s `data`; a
  `fallback` block's `from`/`to`).
- **Retries**: 429 (after `retry-after`, capped at 60 s), 500, 529 and network errors before
  the first event, twice, with 1 s then 2 s backoff plus jitter. Nothing is retried once
  events have arrived; nothing is retried on 400, 401, 402, 403, 404 or 413.

### 4. Conversation model and history

- `Conversation { id, title, model, effort, show_thinking, turns }`; a `Turn` holds the user
  message as sent (its content blocks, rendered once at send, verbatim), the attachments'
  metadata (kind, path, rows, token estimate) for the UI, and the reply: its content blocks
  verbatim, the model that served it, `stop_reason`, `stop_details`, usage, and its state
  (streaming, done, stopped, refused, failed).
- **Append-only.** The API history is every done turn's user message and reply content,
  passed back unchanged: thinking blocks included (Claude Opus 5.5 keeps preserved thinking;
  accounts created on or after 2026-08-31 have the prefix check enforced, so the system
  prompt, tools and earlier messages must stay byte-identical). Nothing earlier is ever
  edited, re-rendered or removed. Serialisation is deterministic whether or not serde_json's `preserve_order` is on (features
  unify across the workspace; `fe_config::settings_schema` already guards against it): values
  built in a fixed order, received blocks kept as parsed, the system prompt a constant, no
  timestamps or IDs in the prefix.
- After a mid-output fallback, a reply echoes back without the `thinking`,
  `redacted_thinking` and `tool_use` blocks that come before its last `fallback` block, as the
  fallback rules require.
- `model`, `effort` and `show_thinking` are fixed per conversation at its first send: a model
  switch empties the cache and drops thinking blocks, and an effort or thinking change empties
  the messages cache. Settings changes apply to new chats.
- A **refused** turn (`stop_reason: "refusal"`, before or mid-stream) and a **failed** turn
  stay in the transcript but leave the API history (the partial is discarded, as the docs
  require); the user edits and resends. A **stopped** turn keeps its user message and, as the
  reply, only its finished `text` blocks; with no text it leaves the history too. A
  `max_tokens` reply is kept, marked cut off.
- Conversations past 80 % of the model's context (1M tokens for Claude Opus 5.5, by the last
  reply's `input_tokens + cache_creation_input_tokens + cache_read_input_tokens`) show a
  "start a new chat" notice. No compaction in v1.

### 5. Prompt caching

Render order is tools, system, messages. The layout:

1. `tools`: none in v1; the agent loop's set, declared at the first request, sorted by name.
2. `system`: one constant text block (the chat's instructions and the edit-block format of
   §8), versioned in code, with an explicit `cache_control: {"type": "ephemeral"}` breakpoint.
   Nothing per-workspace or per-request in it.
3. `messages`: the history (§4), then the new user turn: its context blocks first (each a
   `text` block tagged with what it is: `<file path="…" rows="…">`, `<selection …>`,
   `<diagnostics>`, `<open-editors>`), the question last.
4. Top-level automatic `cache_control: {"type": "ephemeral"}` moves the tail breakpoint
   forward each turn. 5-minute TTL.

Verified by `usage.cache_read_input_tokens`: a test asserts consecutive requests share a
byte-identical prefix, and the chat's debug log prints each reply's usage. Claude Opus 5.5's
minimum cacheable prefix is 512 tokens.

### 6. HTTP client and licences

The first network client in the repo. **hyper 1 (MIT) HTTP/1.1 client over async-io**, the
runtime the app already uses (fe-lsp's `async-process`), through `smol-hyper` (MIT OR
Apache-2.0); TLS by **rustls** (Apache-2.0 OR ISC OR MIT) with the **ring** provider
(Apache-2.0 AND ISC) through `futures-rustls` (MIT OR Apache-2.0); certificates checked by
**rustls-platform-verifier** (MIT OR Apache-2.0), which uses the OS trust store (Security.framework
on macOS), so corporate roots work. In `crates/fe-net`, which:

- takes the hosts it may reach from the grants (§9): a request to any other host fails before a
  socket opens; `https` only, except `http://127.0.0.1` for tests;
- streams the response body as it arrives; dropping it closes the connection;
- is runtime-agnostic: the futures run on GPUI's background executor.

Rejected: **reqwest** (needs tokio, a second runtime beside GPUI's; tokio isn't in our normal
graph today); **ureq** (blocking: a stopped chat can't interrupt a read, so generation keeps
running and billing); **isahc** (libcurl, C, and OpenSSL on Linux); **webpki-roots**
(CDLA-Permissive-2.0, not on `deny.toml`'s list, and it ignores corporate roots);
**aws-lc-rs** (a C and CMake build; parts under the OpenSSL licence). Proxies (`HTTPS_PROXY`)
aren't in v1 (open question).

The record states each crate's licence as checked, and the `cargo deny` result.

### 7. Key storage

- `kiln_core::secrets`: `trait SecretStore { get, set, delete }` by (service, account), with
  `KeychainStore` over **keyring-core** (MIT OR Apache-2.0) and its platform stores:
  `apple-native-keyring-store` (macOS Keychain), `windows-native-keyring-store`, and
  `zbus-secret-service-keyring-store` on Linux (pure Rust D-Bus; never the
  `dbus-secret-service` store's `vendored` feature, which builds libdbus, AFL/GPL), and
  `MemoryStore` for tests. It's kiln's because T839's plugin host needs per-plugin secrets too.
- The chat's key: service `dev.fastedit.fastedit.chat`, account `anthropic-api-key` (the
  chat's own namespace, as a plugin's secrets would be). Resolution: `ANTHROPIC_API_KEY` from
  the environment if set (development; the panel says the key comes from the environment), else
  the keychain. Never in settings, logs or chat files; an `ApiKey` type whose `Debug` is
  redacted. Commands set and clear it (§13).
- On macOS a rebuilt, unsigned dev binary is a different keychain client, so the Keychain asks
  again after each rebuild: developers use the environment variable.

### 8. Code blocks become proposals

The system prompt asks for edits as fenced blocks whose info string names the file:
` ```rust path=src/lib.rs `. The body is either a whole-file replacement or one or more
`<<<<<<< SEARCH` / `=======` / `>>>>>>> REPLACE` sections. A block with no `path` applies to
the selection the turn was asked about, if it had one.

"Apply" on a block opens the file in an editor (an open buffer's text, unsaved edits
included, is what's matched) and shows a `ProposedChange` with `Origin::Ai { agent:
"fastedit.chat", session: <chat id> }`: `from_text` over the whole file for a replacement,
`from_edits` at each SEARCH text's one exact match for sections, the selection's anchored range
for a selection reply. A SEARCH text that matches nowhere or more than once applies nothing and
says which. Accepting a hunk is the user's edit (one `Origin::User` transaction, cmd-z takes it
back), as `docs/ai-host.md`'s undo rule says. The apply is dispatched as the `chat.propose_edit`
command in an `Invocation` with the AI actor (§9), so it's recorded and replayable.

### 9. The AI actor and capabilities

- Everything the AI does goes through `kiln_command::Invocation` with
  `Actor::Ai { agent: "fastedit.chat", session: <chat id> }` and `Source::Script`: in v1,
  `chat.propose_edit`; later, every tool call. What the user does in the panel (send, stop,
  attach, apply) is the user's.
- **Relation to T839.** The built-in chat is a first-party plugin compiled in: it declares the
  capabilities a plugin's manifest would, in T839's strings, `buffer.read` and
  `net:api.anthropic.com`, and the app grants them at build time without a prompt. They're
  checked where a plugin's would be: `kiln_command::Grants` (new: `Capability` parsed from and
  printed as T839's strings, and `Grants::allows`) is checked by every context read
  (`buffer.read`) and is what `fe-net`'s allowlist is built from (`net:<host>`). v1 has no
  `buffer.write`: an edit is a proposal the user accepts. When T839's stage 2 lands, these are
  the checks its host makes, and whether the chat then runs as a WASM component is decided
  there.

### 10. Privacy

- What leaves the machine: the messages the user sends, each turn's attachments, the earlier
  turns, the system prompt; to `api.anthropic.com` only, under the user's key. Nothing is sent
  until the user presses Send; no telemetry.
- Before Send, each attachment is a chip naming what it is and its token estimate, removable.
- The first time the panel opens it shows a notice saying the above, that server-side
  fallback may answer with another Claude model, and where chats are kept (§11); dismissed
  once, remembered in the state dir.
- **Untrusted folders**: nothing is attached automatically (open editors, diagnostics);
  @file and the selection still attach, because the user chose them.
- @file refuses, and its picker leaves out, `.git/`, `.env*`, `*.pem`, `*.key`, `id_*`
  and files over 1 MiB or binary.

### 11. Persistence

Each chat is `<state_dir>/chats/<layout_key(root)>/<chat id>.json` (`kiln_core::layout::layout_key`, as
saved layouts use); a window with no folder uses `<state_dir>/chats/no-folder/`. Files 0600,
directories 0700, written atomically after each finished turn. The panel reopens the last
chat of the folder and lists earlier ones, newest first, titled by their first user line. Kept
until the user deletes them (`chat.delete`). A file that doesn't parse is skipped with a
warning, never deleted.

### 12. Settings

`[chat]` in `settings.toml`: `model = "claude-opus-5-5"`, `effort = "medium"` (`low`,
`medium`, `high`, `xhigh`, `max`), `show_thinking = false`, `max_tokens = 64000`,
`context_budget = 32000` (tokens of attachments per turn), `fallbacks = true`. A project's
`settings.toml` can't set `[chat]`: a cloned repo mustn't pick the model or turn fallbacks off.

### 13. Context and its budget

Sources, each a `ContextItem { kind, label, text, estimate }` gathered by the workbench
through the `buffer.read` grant:

- **@file**: a file under the root, chosen in a picker of the project's files (quick open's
  list) that `@` typed at a word start in the composer, or the Attach button, opens; an open
  buffer's text (unsaved edits included), else the file on disk.
- **Selection**: the active editor's primary selection, with its path and rows; also what
  `chat.ask_about_selection` attaches.
- **Diagnostics**: the active file's problems (severity, row, message, source).
- **Open editors**: the paths of the open tabs, and the active file's text.

Estimate: `ceil(bytes / 3)` tokens (conservative; the real count comes back in `usage`).
Budget: `chat.context_budget` per turn, filled in the order selection, @files (in the order
typed), diagnostics, the active file, the open-editor list; an item that doesn't fit is
truncated at a row boundary with a `[truncated: N more rows]` line, or dropped when less than
10 rows fit, and its chip says so.

### 14. Errors, refusals and rate limits

| What | The panel shows | Action |
|---|---|---|
| no key | "Add your Anthropic API key" | Set API Key |
| 401 | "Your API key was rejected" | Set API Key |
| 402 / 403 | the API's message (billing, permission) | — |
| 404 | "Model `<model>` isn't available to this key" | open settings |
| 413 | "Too much context" | remove attachments |
| 400 | the API's message | edit and resend |
| 429 / 500 / 529 / network, before the stream | "Retrying in N s…", then the error | Retry |
| mid-stream `error` or cut connection | the partial, marked interrupted | Retry |
| `refusal` | "Claude declined this request" (and `stop_details.category` if any) | edit and resend |
| fallback served | a note "Answered by `<model>`" on the reply | — |
| `max_tokens` | "Reply cut off at the token limit" | — |

### 15. Testing

No network in CI. `fe-chat`'s `ScriptedProvider` plays events and records the requests;
`fe-chat-anthropic` reads recorded SSE fixtures (written to the documented event shapes, fed
through a fake transport in chunks split at every byte boundary that matters); `fe-net` is
tested against a loopback HTTP server. One `#[ignore]`d test per crate talks to the real API
when `ANTHROPIC_API_KEY` is set, for a person to run, and can re-record fixtures. UI tests use
GPUI's test platform; the reply rendering has macOS screenshot goldens.

### 16. Build cards

In order (each depends on this record):

1. fe-chat: conversation, provider trait, request assembly, scripted provider.
2. kiln: capability grants and the keychain secret store.
3. fe-chat-anthropic: SSE parser, wire format, provider with retries, fixtures.
4. fe-net: the HTTPS client, its allowlist, and the provider's transport.
5. `[chat]` settings.
6. fe-chat-view: the chat view with plain-text replies.
7. The workbench's chat panel, wired to the provider, settings and key.
8. Context gathering and its budget.
9. Markdown replies (after T847), with goldens.
10. Code blocks applied as proposals.
11. Commands, keys and menus, and the key prompt.
12. Chats kept per folder.

### 17. Out of scope (v1)

The agent tool loop; inline chat in the editor; ghost-text completions from the model; other
providers; images and PDFs as attachments; compaction; HTTP proxies; per-message effort
(beta `mid-conversation-output-config-2026-07-01`); usage and cost reporting beyond the
context meter.

### 18. Open questions for the user

1. Default effort for chat: `medium` (the model's default) or `low` (faster, cheaper; the
   skill says `low` comes close on many coding tasks)?
2. Untrusted folders: is "no automatic context, explicit attachments allowed" right, or should
   the chat refuse to attach anything there?
3. Chat retention: keep until deleted, or prune after N days?
4. Server-side fallback on by default (the skill's advice) with a setting to turn it off: OK?
5. HTTP proxies (`HTTPS_PROXY`) and a configurable base URL (enterprise gateways): v1 or later?
6. The @file deny list: built in, or a `chat.exclude` setting too?
7. 5-minute cache TTL, or 1-hour for chats a user comes back to after a pause (writes cost 2x)?

## Decided by the USER (2026-10-17): answers to the open questions

The record states each of these as decided (not as an open question), with its design:

1. **Default effort: medium** (claude-opus-5-5's default), a `chat.effort` setting.
2. **Untrusted folders: only what the user explicitly attaches** is sent (files and selections added by hand); no automatic context (open editors, diagnostics) there.
3. **Retention: chats are kept until the user deletes them** (per chat, or all), per folder.
4. **Server-side fallback: on by default** (`fallbacks: "default"`, beta `server-side-fallback-2026-07-01`), with a setting to turn it off; a reply notes which model answered when a fallback ran.
5. **HTTPS proxy and a configurable base URL: a later card**, not v1 (v1 talks to api.anthropic.com only); the record names it as follow-up work.
6. **@file deny list: a built-in list** (`.env*`, `*.pem`, `id_*`, credentials files and the like) **plus a `chat.exclude` glob setting** (user and workspace).
7. **Prompt cache TTL: 5 minutes** (the default).

The finish notes still ask the USER to sign off the final record before any build card (T962-T973) is readied.
