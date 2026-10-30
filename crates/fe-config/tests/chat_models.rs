//! The models the Settings tab lists for `chat.model`.

use std::collections::HashSet;

use fe_config::{Settings, chat_models::KNOWN};

#[test]
fn default_is_listed() {
    let default = Settings::default().chat.model;
    assert_eq!(default, "claude-opus-5-5");
    assert_eq!(KNOWN.first(), Some(&default.as_str()), "the default first");
    let unique: HashSet<&str> = KNOWN.iter().copied().collect();
    assert_eq!(unique.len(), KNOWN.len(), "no duplicates: {KNOWN:?}");
}
