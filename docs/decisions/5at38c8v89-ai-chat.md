# AI chat: a built-in Claude chat panel, its crates, provider, HTTP client, key storage, context, caching and privacy

Status: **proposed.** The user signs it off before any ai-chat build card
(T962-T973) is readied. Researched 2026-10-18 against fastedit `79955bef`.
Source card T961.

This record sets policy: what leaves the machine, where code and chats are
kept, and the repo's first network client.

**What was read.**

- **Repo, at the base:** `docs/ai-host.md`; `crates/fe-buffer/src/proposed.rs`
  (`ProposedChange::from_edits` line 113, `from_text` line 145) and
  `crates/fe-buffer/src/buffer.rs` (`Origin`, line 25, `Origin::Ai { agent:
  Arc<str>, session: Arc<str> }` line 27); `crates/fe-gpui/src/view/proposal.rs`
  (`EditorView::set_proposals`, line 48; accepting is the user's edit, lines 8
  and 91); `crates/fe-editor/src/scripted_ai.rs`;
  `crates/fe-gpui/examples/ai_demo.rs`; `kiln/crates/kiln-command/src/invocation.rs`
  (`Invocation` line 19, `Actor` line 86, `Source` line 101);
  `kiln/crates/kiln-core/src/layout.rs:48` (`layout_key`) and `paths.rs:187`
  (`AppPaths::state_dir`); `crates/fe-config/src/settings.rs:409` (`settings_schema` and
  its `sorted`); `scripts/check-no-gpui.sh`; `deny.toml`;
  `kiln/crates/kiln/examples/host/conversation.rs` and `threads.rs` (Kiln Chat).
- **API facts:** the claude-api skill's `curl/examples.md`,
  `shared/tool-use-concepts.md`, `shared/prompt-caching.md`,
  `shared/error-codes.md` and `shared/model-migration.md` § Migrating to Claude
  Opus 5.5, of 2026-10-17. They are kept as written; the beta header dates in
  particular are not "corrected".
- **Crates:** crates.io metadata (`cargo info`) for each, and `cargo deny check
  licenses` on a scratch branch (§ 6).

**Corrections to the draft this record was written from.** None of its
decisions changed. What moved:

- `fe-markdown` and `fe-markdown-view` (T846, T847) don't exist at the base, so
  Markdown replies (card 9) wait for them, and the claim that the preview
  "blocks remote images" can't be checked here.
- T839's plugin decision is at the base:
  `docs/decisions/vmtnmkay62-extensions-and-plugins.md`, itself still
  **proposed**. § 9 and § 7 are reconciled with it (capability strings,
  `net:<host>` enforcement, per-plugin secrets, `Actor::Host`); the code has no
  `Grants`, `Capability` or capability string yet, so `kiln-command`'s
  `capability.rs` is new.
- `scripts/check-no-gpui.sh` treats every workspace crate as core unless it is
  listed in `UI_CRATES`; so `fe-chat`, `fe-chat-anthropic` and `fe-net` are core
  by default, and `fe-chat-view` and `fe-workbench` are the ones that need
  listing (`fe-workbench` already is).
- hyper does depend on tokio (the crate, with only its `sync` feature: no
  runtime); the draft's "tokio isn't in our normal graph" is true of the graph
  today on macOS, not after hyper. Reqwest's rejection stands on the runtime.
- `Actor::Ai` holds `String`s and `Origin::Ai` holds `Arc<str>`s.
- The keychain service `dev.fastedit.fastedit.chat` builds on the app id that
  exists: `crates/fe-config/src/paths.rs:18` (`app_id()`) returns
  `AppId::new("dev.fastedit.fastedit", "fastedit", "fastedit")`. An earlier
  draft of this record said no app id was defined; that was wrong.

## 1. Decision

A built-in chat panel backed by Claude through the Anthropic Messages API, in
five crates and three workbench modules (the table is the unit of work for the
build cards):

| Crate / module | GPUI? | What it holds |
|---|---|---|
| `crates/fe-chat` | no | the conversation model, the provider trait and its events, request assembly (the cache layout), context items and budgeting, code-block edit targets, chat persistence, a scripted fake provider |
| `crates/fe-chat-anthropic` | no | the SSE parser, the Messages API wire format, the provider over a `Transport` trait, retries |
| `crates/fe-net` | no | the repo's first network client: HTTPS over async-io with a host allowlist |
| `crates/fe-chat-view` | yes | the chat view (transcript, composer, attachment chips, states), knowing nothing of the workbench |
| `kiln-command` `capability.rs`, `kiln-core` `secrets.rs` | no | capability strings and grants (T839's format), and a secret store over the platform keychain |
| `fe-workbench` `chat_panel.rs`, `chat_context.rs`, `chat_apply.rs` | yes | the sidebar panel, context from the workspace, code blocks applied as proposals |

The host product takes `fe-chat`, `fe-chat-anthropic`, `fe-net` and
`fe-chat-view` and supplies its own context sources and panel, as fastedit's
workbench does. `fe-chat`, `fe-chat-anthropic` and `fe-net` know nothing of
fastedit and are headless: `scripts/check-no-gpui.sh` checks them without being
told (they are not in its `UI_CRATES`); `fe-chat-view` is added to `UI_CRATES`.

Nothing in the repo talks to a model today. `Cargo.lock` has
`gpui-pre-http-client`'s trait and no implementation, and no TLS crate.

## 2. Provider trait

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

- `ChatStream` is a `futures::Stream<Item = ChatEvent>`; dropping it closes the
  connection, which stops generation server-side. There is no separate cancel call.
- `ProviderError { kind, message, retry_after }`, `kind` one of `Network`,
  `RateLimited` (429), `Overloaded` (529), `Server` (500), `Auth` (401),
  `Billing` (402), `Permission` (403), `NotFound` (404), `TooLarge` (413),
  `InvalidRequest` (400), `Stream` (an SSE `error` event or a cut connection
  after the stream started). `retryable()` is true for the first four only.
- **Tools, for the agent loop that comes next.** `ToolSpec { name, description,
  input_schema, strict }` and `ToolChoice` with only `Auto` and `None`: forced
  `tool_choice` (`any`, `tool`) is a 400 on Claude Opus 5.5, so steering is by
  prompt plus `strict: true`, and the caller checks a call was made. A streamed
  request with client tools sets `eager_input_streaming: true` on each tool; a
  tool's input is parsed strictly at `content_block_stop` and validated against
  its schema before anything runs; a turn whose `stop_reason` is `max_tokens`
  or `refusal` runs none of its tools. Tool results go back as `tool_result`
  blocks. The full tool set is declared at a conversation's first request
  (adding a tool later breaks the prompt cache and the thinking-block binding;
  the agent loop uses `tool_addition` blocks, beta
  `mid-conversation-tool-changes-2026-07-01`). Running a tool is dispatching an
  `Invocation` with the AI actor (§ 9). v1 sends no tools.

## 3. The Anthropic provider

- `POST https://api.anthropic.com/v1/messages`, headers `x-api-key`,
  `anthropic-version: 2023-06-01`, `content-type: application/json`,
  `anthropic-beta: server-side-fallback-2026-07-01`, and a `user-agent` naming
  fastedit and its version.
- Body: `model` (default **`claude-opus-5-5`**, exactly, no date suffix),
  `max_tokens: 64000`, `stream: true`, `system`, `messages`,
  `output_config: {effort}` (default `medium`, the model's own default),
  `fallbacks: "default"`, and top-level `cache_control` (§ 5). Thinking is
  always on for Claude Opus 5.5 and can't be disabled (`{"type":"disabled"}`
  and `budget_tokens` are 400s): the request omits `thinking`, or, when the
  user shows thinking, sends `{"type": "adaptive", "display": "summarized"}`.
  No assistant prefill (a 400). No sampling parameters.
- **Server-side fallback, on by default** (`fallbacks: "default"` with exactly
  that beta header; the array form takes `server-side-fallback-2026-06-01`
  instead, and mixing them is a 400). A classifier decline re-runs on
  Anthropic's recommended fallback model inside the same call. The reply then
  names the model that served it (`message_start`'s `model`; a
  `fallback_message` entry in `usage.iterations`). `reasoning_extraction`
  declines are not retried. **The user is told this is on** (the privacy
  notice, § 10), and can turn it off (`chat.fallbacks`, § 12). It is a
  Claude API feature only, which is the only API this provider talks to.
- **SSE**: `message_start`, `content_block_start`, `content_block_delta`
  (`text_delta`, `thinking_delta`, `signature_delta`, `input_json_delta`),
  `content_block_stop`, `message_delta` (`stop_reason`, `stop_details`, usage),
  `message_stop`, `ping`, `error`. Unknown event and block types are kept, not
  errors. A block is rebuilt verbatim from its start and deltas (thinking text
  plus `signature`; `redacted_thinking`'s `data`; a `fallback` block's
  `from`/`to`).
- **Retries**: 429 (after `retry-after`, capped at 60 s), 500, 529 and network
  errors before the first event, twice, with 1 s then 2 s backoff plus jitter.
  Nothing is retried once events have arrived; nothing is retried on 400, 401,
  402, 403, 404 or 413.

## 4. Conversation model and history

- `Conversation { id, title, model, effort, show_thinking, turns }`; a `Turn`
  holds the user message as sent (its content blocks, rendered once at send,
  verbatim), the attachments' metadata (kind, path, rows, token estimate) for
  the UI, and the reply: its content blocks verbatim, the model that served
  it, `stop_reason`, `stop_details`, usage, and its state (streaming, done,
  stopped, refused, failed).
- **Append-only.** The API history is every done turn's user message and reply
  content, passed back unchanged: thinking blocks included (Claude Opus 5.5
  keeps preserved thinking; accounts created on or after 2026-08-31 have the
  prefix check enforced, so the system prompt, tools and earlier messages must
  stay byte-identical). Nothing earlier is ever edited, re-rendered or
  removed. Serialisation is deterministic whether or not serde_json's
  `preserve_order` is on (features unify across the workspace;
  `fe_config::settings_schema`'s `sorted` already guards against it, at
  `crates/fe-config/src/settings.rs:409`): values built in a fixed order,
  received blocks kept as parsed, the system prompt a constant, no timestamps
  or IDs in the prefix.
- After a mid-output fallback, a reply echoes back without the `thinking`,
  `redacted_thinking` and `tool_use` blocks that come before its last
  `fallback` block, as the fallback rules require.
- `model`, `effort` and `show_thinking` are fixed per conversation at its first
  send: a model switch empties the cache and drops thinking blocks, and an
  effort or thinking change empties the messages cache. Settings changes apply
  to new chats.
- A **refused** turn (`stop_reason: "refusal"`, before or mid-stream) and a
  **failed** turn stay in the transcript but leave the API history (the
  partial is discarded, as the docs require); the user edits and resends. A
  **stopped** turn keeps its user message and, as the reply, only its finished
  `text` blocks; with no text it leaves the history too. A `max_tokens` reply
  is kept, marked cut off.
- Conversations past 80 % of the model's context (1M tokens for Claude Opus
  5.5, by the last reply's `input_tokens + cache_creation_input_tokens +
  cache_read_input_tokens`) show a "start a new chat" notice. No compaction in v1.

## 5. Prompt caching

Render order is tools, system, messages. The layout:

1. `tools`: none in v1; the agent loop's set, declared at the first request,
   sorted by name.
2. `system`: one constant text block (the chat's instructions and the
   edit-block format of § 8), versioned in code, with an explicit
   `cache_control: {"type": "ephemeral"}` breakpoint. Nothing per-workspace or
   per-request in it.
3. `messages`: the history (§ 4), then the new user turn: its context blocks
   first (each a `text` block tagged with what it is: `<file path="…"
   rows="…">`, `<selection …>`, `<diagnostics>`, `<open-editors>`), the
   question last.
4. Top-level automatic `cache_control: {"type": "ephemeral"}` moves the tail
   breakpoint forward each turn. 5-minute TTL (decided, see "Decided by the
   user"). An explicit breakpoint and the automatic one compose; the explicit
   one is on `system`, never on the last block, so the two-TTL 400 can't occur.

Verified by `usage.cache_read_input_tokens`: a test asserts consecutive
requests share a byte-identical prefix, and the chat's debug log prints each
reply's usage. Claude Opus 5.5's minimum cacheable prefix is 512 tokens.

## 6. HTTP client and licences

The first network client in the repo. **hyper 1 (MIT) HTTP/1.1 client over
async-io**, the runtime the app already uses (`fe-lsp` has `async-io` and
`async-process`; workspace `Cargo.toml:88-89`), through `smol-hyper` (MIT OR
Apache-2.0); TLS by **rustls** (Apache-2.0 OR ISC OR MIT) with the **ring**
provider through `futures-rustls` (MIT/Apache-2.0); certificates checked by
**rustls-platform-verifier** (MIT OR Apache-2.0), which uses the OS trust store
(Security.framework on macOS), so corporate roots work. In `crates/fe-net`,
which:

- takes the hosts it may reach from the grants (§ 9): a request to any other
  host fails before a socket opens; `https` only, except `http://127.0.0.1`
  for tests;
- streams the response body as it arrives; dropping it closes the connection;
- is runtime-agnostic: the futures run on GPUI's background executor.

Rejected: **reqwest** (needs tokio's runtime, a second one beside GPUI's);
**ureq** (blocking: a stopped chat can't interrupt a read, so generation keeps
running and billing); **isahc** (libcurl, C, and OpenSSL on Linux);
**webpki-roots** (CDLA-Permissive-2.0, not on `deny.toml`'s list, and it
ignores corporate roots); **aws-lc-rs** (a C and CMake build; parts under the
OpenSSL licence). hyper brings in tokio with only `sync` (no runtime), which
is not the reason reqwest is out.

**Licences, as checked on 2026-10-17** against `deny.toml`'s allow list (MIT,
MIT-0, Apache-2.0 (+LLVM-exception), BSD-2/3-Clause, ISC, Zlib, 0BSD, BSL-1.0,
Unlicense, Unicode-3.0, bzip2-1.0.6, CC0-1.0):

| Crate | Version | Licence (crates.io) | On the list? |
|---|---|---|---|
| hyper | 1.12.0 | MIT | yes |
| http-body-util | 0.1.5 | MIT | yes |
| smol-hyper | 0.1.1 | MIT OR Apache-2.0 | yes |
| rustls | 0.23.46 | Apache-2.0 OR ISC OR MIT | yes |
| ring (via rustls) | 0.17.14 | Apache-2.0 AND ISC | yes |
| futures-rustls | 0.26.0 | MIT/Apache-2.0 | yes |
| rustls-platform-verifier | 0.6.2 | MIT OR Apache-2.0 | yes, **but** it depends on `webpki-root-certs` for `wasm32` (below) |
| keyring-core | 1.0.1 | MIT OR Apache-2.0 | yes |
| apple-native-keyring-store | 1.0.2 | MIT OR Apache-2.0 | yes |
| windows-native-keyring-store | 1.1.0 | MIT OR Apache-2.0 | yes |
| zbus-secret-service-keyring-store | 1.0.1 | MIT OR Apache-2.0 | yes |

**`cargo deny check licenses`**, run in a scratch worktree (detached at the
base, not committed) with a crate depending on all of the above (`rustls` with
`ring`, `std`, `tls12`, and `default-features = false`, since rustls' defaults
enable aws-lc-rs; `futures-rustls` 0.26 with `default-features = false` and
`ring`, `tls12`, for the same reason; `hyper` with `client`, `http1`), failed on exactly one
crate and passed on every other:

```
error[rejected]: failed to satisfy license requirements
  webpki-root-certs v1.0.9   license = "CDLA-Permissive-2.0"
  └── rustls-platform-verifier v0.6.2
```

`cargo deny check bans sources` and `check advisories` passed. `cargo tree`
shows `webpki-root-certs` in none of the macOS, Linux or Windows targets'
normal graphs: the verifier's manifest depends on it only for `wasm32`
(and as a dev-dependency), and cargo deny walks every target, so it isn't
linked into any build we ship. It is still in the graph
`cargo deny` checks, so adding the dependency as is would fail CI. **That is
not decided here; it's open question 1.** No `CDLA-Permissive-2.0` allowance
is added by this record. The rejected `webpki-roots` is the same licence, which
is why it was rejected; the same ruling covers both.

With those declarations the scratch run's macOS tree contained ring and no
aws-lc-rs, and no C build step in the
HTTP and keychain crates (ring builds its small C and assembly; that is the
one native step, and is the same as any rustls+ring use). Proxies
(`HTTPS_PROXY`) and a configurable base URL are a later card, not v1.

## 7. Key storage

- `kiln_core::secrets`: `trait SecretStore { get, set, delete }` by (service,
  account), with `KeychainStore` over **keyring-core** (MIT OR Apache-2.0) and
  its platform stores: `apple-native-keyring-store` (macOS Keychain),
  `windows-native-keyring-store`, and `zbus-secret-service-keyring-store` on
  Linux (pure Rust D-Bus; never the `dbus-secret-service` store's `vendored`
  feature, which builds libdbus, AFL/GPL), and `MemoryStore` for tests. It's
  kiln's because T839's decision 5 needs per-plugin host-held secrets too. Each store is
  a target-gated dependency.
- The chat's key: service `dev.fastedit.fastedit.chat`, account
  `anthropic-api-key` (the app id of `crates/fe-config/src/paths.rs:18` plus
  `.chat`: the chat's own namespace, as a plugin's secrets would be, keyed by
  its id as T839 decision 5 has it). Resolution: `ANTHROPIC_API_KEY`
  from the environment if set (development; the panel says the key comes from
  the environment), else the keychain. Never in settings, logs or chat files;
  an `ApiKey` type whose `Debug` is redacted. Commands set and clear it (§ 16, card 11).
- On macOS a rebuilt, unsigned dev binary is a different keychain client, so
  the Keychain asks again after each rebuild: developers use the environment variable.

## 8. Code blocks become proposals

The system prompt asks for edits as fenced blocks whose info string names the
file: ` ```rust path=src/lib.rs `. The body is either a whole-file replacement
or one or more `<<<<<<< SEARCH` / `=======` / `>>>>>>> REPLACE` sections. A
block with no `path` applies to the selection the turn was asked about, if it
had one.

"Apply" on a block opens the file in an editor (an open buffer's text, unsaved
edits included, is what's matched) and shows a `ProposedChange` with
`Origin::Ai { agent: "fastedit.chat", session: <chat id> }`: `from_text` over
the whole file for a replacement, `from_edits` at each SEARCH text's one exact
match for sections, the selection's anchored range for a selection reply. It
is shown by `EditorView::set_proposals`. A SEARCH text that matches nowhere or
more than once applies nothing and says which. The whole resolved edit set is
validated before `from_edits` is called, since it panics on overlapping ranges
and on two insertions at one offset: sections that overlap, or resolve to the
same range, reject the block and apply nothing (the build card tests both). Accepting a hunk is the user's
edit (one `Origin::User` transaction, cmd-z takes it back:
`crates/fe-gpui/src/view/proposal.rs:8`). The apply is dispatched as the
`chat.propose_edit` command in an `Invocation` with the AI actor (§ 9), so it's
recorded and replayable.

## 9. The AI actor and capabilities

- Everything the AI does goes through `kiln_command::Invocation` with
  `Actor::Ai { agent: "fastedit.chat", session: <chat id> }` and
  `Source::Script`: in v1, `chat.propose_edit`; later, every tool call. What
  the user does in the panel (send, stop, attach, apply) is the user's.
- **Relation to T839** (`docs/decisions/vmtnmkay62-extensions-and-plugins.md`,
  "Sandbox and capabilities", which says "the product's own AI features run as
  plugins under the same rules, with capabilities granted by the app"). The
  built-in chat is a first-party plugin compiled in: it declares the
  capabilities a plugin's manifest would, in T839's strings, `buffer.read` and
  `net:api.anthropic.com`, and the app grants them at build time without a
  prompt. They're checked where a plugin's would be: `kiln_command::Grants`
  (new: `Capability` parsed from and printed as T839's strings, and
  `Grants::allows`) is checked by every context read (`buffer.read`) and is
  what `fe-net`'s allowlist is built from (`net:<host>`). Where this record
  and T839 differ, on purpose:
  - **Actor.** T839 has plugins invoke commands as `Actor::Host("<extension
    id>")`, `Source::Script`. The chat uses `Actor::Ai` (above), because the
    AI actor is what maps to `Origin::Ai` and AI-scoped undo. When the chat
    becomes a WASM plugin, whether it keeps `Actor::Ai` is decided there.
  - **Network.** T839 checks the destination after DNS and on each redirect.
    `fe-net` does the same against the grant, and follows no redirects at all in v1.
  - **Combination warning.** T839 decision 4 warns that `buffer.read` with a
    `net` grant "can read your code and send it to <hosts>". For the built-in
    chat the first-open notice (§ 10) is that warning.
  - **Environment.** T839 gives plugins no app environment. The chat reads
    `ANTHROPIC_API_KEY` (§ 7) as a first-party exception, for development only.
  v1 has no `buffer.write`: an edit is a proposal the user accepts. T839 is
  still proposed, so the strings may change with its sign-off; the grants card
  follows it.

## 10. Privacy

- What leaves the machine: the messages the user sends, each turn's
  attachments, the earlier turns, the system prompt; to `api.anthropic.com`
  only, under the user's key. Nothing is sent until the user presses Send; no telemetry.
- Before Send, each attachment is a chip naming what it is and its token
  estimate, removable.
- The first time the panel opens it shows a notice saying the above, that
  server-side fallback may answer with another Claude model, and where chats
  are kept (§ 11); dismissed once, remembered in the state dir.
- **Untrusted folders**: only what the user explicitly attaches is sent (files
  and selections added by hand); nothing is attached automatically (open
  editors, diagnostics). Trust is the workbench's existing folder trust
  (`crates/fe-workbench/src/trust.rs`).
- @file refuses, and its picker leaves out, the built-in deny list (`.git/`,
  `.env*`, `*.pem`, `*.key`, `id_*`, credentials files and the like), anything
  matching `chat.exclude` (§ 12), and files over 1 MiB or binary.

## 11. Persistence

Each chat is `<state_dir>/chats/<layout_key(root)>/<chat id>.json`
(`kiln_core::layout::layout_key`, as saved layouts use; `state_dir` is
`Paths::state_dir`); a window with no folder uses `<state_dir>/chats/no-folder/`.
Files 0600, directories 0700, written atomically after each finished turn. The
panel reopens the last chat of the folder and lists earlier ones, newest
first, titled by their first user line. Kept until the user deletes them, one
chat or all of a folder's (`chat.delete`). A file that doesn't parse is
skipped with a warning, never deleted.

## 12. Settings

`[chat]` in `settings.toml`: `model = "claude-opus-5-5"`, `effort = "medium"`
(`low`, `medium`, `high`, `xhigh`, `max`), `show_thinking = false`,
`max_tokens = 64000`, `context_budget = 32000` (tokens of attachments per
turn), `fallbacks = true`, `exclude = []` (globs the @file picker and attach
refuse, from the user's and the workspace's settings, merged). A project's
`settings.toml` can't set the rest of `[chat]`: a cloned repo mustn't pick the
model or turn fallbacks off. That is a `ProjectFilter` on `chat` that keeps
only `exclude` (`kiln_core::ProjectFilter`, as `lsp` and `theme` are narrowed
in `crates/fe-config/src/settings.rs`).

## 13. Context and its budget

Sources, each a `ContextItem { kind, label, text, estimate }` gathered by the
workbench through the `buffer.read` grant:

- **@file**: a file under the root, chosen in a picker of the project's files
  (quick open's list, `crates/fe-workbench/src/quick_open.rs`) that `@` typed
  at a word start in the composer, or the Attach button, opens; an open
  buffer's text (unsaved edits included), else the file on disk.
- **Selection**: the active editor's primary selection, with its path and rows;
  also what `chat.ask_about_selection` attaches.
- **Diagnostics**: the active file's problems (severity, row, message, source).
- **Open editors**: the paths of the open tabs, and the active file's text.

Estimate: `ceil(bytes / 3)` tokens (conservative; the real count comes back in
`usage`). Budget: `chat.context_budget` per turn, filled in the order
selection, @files (in the order typed), diagnostics, the active file, the
open-editor list; an item that doesn't fit is truncated at a row boundary with
a `[truncated: N more rows]` line, or dropped when less than 10 rows fit, and
its chip says so.

## 14. Errors, refusals and rate limits

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

## 15. Testing

No network in CI. `fe-chat`'s `ScriptedProvider` plays events and records the
requests; `fe-chat-anthropic` reads recorded SSE fixtures (written to the
documented event shapes, fed through a fake transport in chunks split at every
byte boundary that matters); `fe-net` is tested against a loopback HTTP
server. One `#[ignore]`d test per crate talks to the real API when
`ANTHROPIC_API_KEY` is set, for a person to run, and can re-record fixtures.
UI tests use GPUI's test platform; the reply rendering has macOS screenshot goldens.

## 16. Build cards

In order (each depends on this record):

1. fe-chat: conversation, provider trait, request assembly, scripted provider.
2. kiln: capability grants and the keychain secret store.
3. fe-chat-anthropic: SSE parser, wire format, provider with retries, fixtures.
4. fe-net: the HTTPS client, its allowlist, and the provider's transport.
5. `[chat]` settings.
6. fe-chat-view: the chat view with plain-text replies.
7. The workbench's chat panel, wired to the provider, settings and key.
8. Context gathering and its budget.
9. Markdown replies (after T846 and T847, which are not at the base), with goldens.
10. Code blocks applied as proposals.
11. Commands, keys and menus, and the key prompt.
12. Chats kept per folder.

## 17. Out of scope (v1)

The agent tool loop; inline chat in the editor; ghost-text completions from
the model; other providers; images and PDFs as attachments; compaction; HTTP
proxies and a configurable base URL (a later card, below); per-message effort
(beta `mid-conversation-output-config-2026-07-01`); usage and cost reporting
beyond the context meter.

## Decided by the user (2026-10-17)

1. **Default effort: medium** (claude-opus-5-5's default), a `chat.effort` setting.
2. **Untrusted folders: only what the user explicitly attaches** is sent; no
   automatic context (open editors, diagnostics) there.
3. **Retention: chats are kept until the user deletes them** (per chat, or
   all), per folder.
4. **Server-side fallback: on by default** (`fallbacks: "default"`, beta
   `server-side-fallback-2026-07-01`), with a setting to turn it off; a reply
   notes which model answered when a fallback ran.
5. **HTTPS proxy and a configurable base URL: a later card**, not v1 (v1 talks
   to api.anthropic.com only). Follow-up work: `HTTPS_PROXY` in `fe-net`, and a
   `chat.base_url` that is also a `net:` grant.
6. **@file deny list: a built-in list** (`.env*`, `*.pem`, `id_*`, credentials
   files and the like) **plus a `chat.exclude` glob setting** (user and workspace).
7. **Prompt cache TTL: 5 minutes** (the default).

## Open questions for the user

Sign this record off, or say what to change, before any build card (T962-T973)
is readied.

1. **A licence `deny.toml` lacks: `CDLA-Permissive-2.0`.**
   `rustls-platform-verifier` 0.6.2 depends on `webpki-root-certs` 1.0.9,
   which has that licence, and `cargo deny check licenses` fails on it. It is
   not linked into macOS, Linux or Windows builds. Options: (a) allow
   `CDLA-Permissive-2.0` for that one crate with an `exceptions` entry, as
   `dwrote` has for MPL-2.0; (b) limit `deny.toml`'s `[graph]` to the targets
   we ship (macOS, Linux, Windows), which drops the wasm32-only dependency
   but changes what every other check covers (not run; it would need its own
   `cargo deny` pass); (c) drop the platform verifier for a different
   certificate source. Nothing is chosen. Card 4 can't land until this is answered.
2. **T839 isn't signed off.** § 9 follows its capability strings, its `net:`
   enforcement and its secrets rule, and departs from it in four marked places
   (`Actor::Ai` rather than `Actor::Host`, no redirects, the first-open notice
   as the combination warning, the environment-key exception). Is that
   reconciliation right, and should the grants card (card 2) wait for T839's
   sign-off or follow it as written?
3. **The keychain service** is `dev.fastedit.fastedit.chat`, from the existing
   app id. It can't change later without losing stored keys; is the `.chat`
   suffix what you want, or should the key live under the bare app id?
4. **Markdown replies depend on T846 and T847.** They aren't at the base. OK to
   ship cards 6-8 with plain-text replies and hold card 9 for them?
5. **Server-side fallback sends a declined request to another model.** Decided
   on by default (answer 4 above); confirm the first-open notice's wording is
   enough disclosure, since this is what leaves the machine beyond what the
   user chose.
6. **Chat files hold code in plain text** under the state dir (0600). Decided
   as kept until deleted; is that acceptable for folders the user marks
   untrusted, or should those chats be kept in memory only?
