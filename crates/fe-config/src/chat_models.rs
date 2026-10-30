//! The Claude models the app knows, which the Settings tab lists for
//! `chat.model`. A curated list, not fetched: a model newer than the app is
//! still typed in, as Other… in the dropdown does.

/// The model ids, the default first.
pub const KNOWN: &[&str] = &[
    "claude-opus-5-5",
    "claude-sonnet-5-5",
    "claude-haiku-5-5",
    "claude-fable-5-1",
];
