---
task_uid: "01m57yv3ynp8qrq67dy41s0ahs"
task: "T190"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-config test chat_defaults_match_the_record (tests/chat_settings.rs): Settings::default().chat is model claude-opus-5-5, effort medium, show_thinking false, max_tokens 64000, context_budget 32000, fallbacks true, and a user file setting each key reads back its value"
    required: true
    manual: false
  - id: "C2"
    text: "fe-config test chat_values_out_of_range_are_diagnostics (tests/chat_settings.rs): effort none, max_tokens 100, a negative context_budget and an empty model each give a diagnostic at the key and keep the default"
    required: true
    manual: false
  - id: "C3"
    text: "fe-config test a_project_cannot_set_chat (tests/chat_settings.rs): a project settings.toml with [chat] model and fallbacks leaves the user's values in effect and gives one warning at the [chat] table naming chat"
    required: true
    manual: false
  - id: "C4"
    text: "settings.schema.json is regenerated (fe-config's schema golden test passes) and tests/template.rs's template_is_all_defaults passes with a commented [chat] section in settings-template.toml"
    required: true
    manual: false
---

# Add the [chat] settings (model, effort, show_thinking, max_tokens, context_budget, fallbacks), which a project file can't set (Mac T966)

# Add the [chat] settings: model, effort, shown thinking, token limits and fallbacks, which a project file can't set

## Problem

The AI chat decision record (`docs/decisions/*-ai-chat.md`; signed off by the user) gives the
chat a `[chat]` table in `settings.toml` (record §12): the model (default `claude-opus-5-5`,
user-selectable), the effort that controls how much Claude Opus 5.5 thinks (thinking can't be
turned off on it), whether the reasoning summary is shown, the reply and context token limits,
and server-side fallback. fastedit's `Settings` (`crates/fe-config/src/settings.rs`) has no such
table. A project's `settings.toml` mustn't set it: a cloned repo shouldn't pick the model,
raise the spend or turn fallbacks off.

## Change

In `crates/fe-config/src/settings.rs`:

- `pub chat: ChatSettings` on `Settings`, `#[serde(default)]`, documented "The AI chat."
- `ChatSettings` (`#[serde(default)]`, `JsonSchema`), each field documented for the schema:
  - `model: String`, default `"claude-opus-5-5"`, `#[schemars(length(min = 1))]`: "The Claude
    model new chats use. A chat keeps the model it started with."
  - `effort: ChatEffort` (`low`, `medium`, `high`, `xhigh`, `max`; snake_case), default
    `medium`: "How much Claude thinks before it answers: lower is faster and cheaper. Applies
    to new chats."
  - `show_thinking: bool`, default `false`: "Show a summary of Claude's reasoning above each
    reply. Applies to new chats."
  - `max_tokens: u32`, default `64000`, range 1024–128000: "The most a reply may use, its
    thinking included."
  - `context_budget: u32`, default `32000`, range 0–500000: "The most tokens of attached
    context (files, selection, diagnostics) one message sends."
  - `fallbacks: bool`, default `true`: "When Claude declines a request, let Anthropic's API
    answer with its recommended fallback model."
- A `ProjectFilter::new("chat", drop_chat)` beside `drop_theme`: a project file's `[chat]`
  table is removed with one warning diagnostic at the table, "A project's settings can't set
  `chat`; set it in your own settings.toml", as `drop_terminal_shell` reports its keys.
- Regenerate `crates/fe-config/settings.schema.json` (`UPDATE_GOLDEN=1` on fe-config's schema
  test, `src/tests/settings.rs`, then review the diff), and add a
  commented `[chat]` section with every default to `crates/fe-config/settings-template.toml`
  (`tests/template.rs`'s `template_is_all_defaults` must still pass).

No consumer in this card; the chat panel reads it.

Tests in `crates/fe-config/tests/chat_settings.rs` (top-level fns).
