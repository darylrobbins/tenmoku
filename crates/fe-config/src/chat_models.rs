//! The Claude models the app knows, which the Settings tab lists for
//! `chat.model` and the chat's model picker offers. A curated list, not
//! fetched: a model newer than the app is still typed in, as Other… in the
//! dropdown does.

/// The model ids, the default first.
pub const KNOWN: &[&str] = &[
    "claude-opus-5-5",
    "claude-sonnet-5-5",
    "claude-haiku-4-5",
    "claude-fable-5-1",
];

/// Whether requests to `id` carry an effort. Only `claude-haiku-4-5` does
/// not: its API rejects `output_config.effort`. The wire code in
/// `fe-chat-anthropic` keeps its own copy of this check.
pub fn takes_effort(id: &str) -> bool {
    id != "claude-haiku-4-5"
}

/// The models whose effort changes mid-chat through an effort-only system
/// message (beta `mid-conversation-output-config-2026-07-01`), which keeps
/// the messages cache. On any other model the request's top-level effort
/// changes instead.
pub const EFFORT_MESSAGE_MODELS: &[&str] =
    &["claude-opus-5-5", "claude-sonnet-5-5", "claude-fable-5-1"];

/// Whether `id` takes the effort-only system message
/// ([`EFFORT_MESSAGE_MODELS`]).
pub fn takes_effort_message(id: &str) -> bool {
    EFFORT_MESSAGE_MODELS.contains(&id)
}

/// The model's product name, such as "Opus 5.5": not translated. A model
/// the app doesn't know shows as its id.
pub fn label(id: &str) -> &str {
    match id {
        "claude-opus-5-5" => kiln_l10n::verbatim!("Opus 5.5"),
        "claude-sonnet-5-5" => kiln_l10n::verbatim!("Sonnet 5.5"),
        "claude-haiku-4-5" => kiln_l10n::verbatim!("Haiku 4.5"),
        "claude-fable-5-1" => kiln_l10n::verbatim!("Fable 5.1"),
        other => other,
    }
}
