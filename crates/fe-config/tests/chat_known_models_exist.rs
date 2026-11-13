//! The model list names only models that exist.

use fe_config::chat_models::KNOWN;

const REAL: &[&str] = &[
    "claude-opus-5-5",
    "claude-sonnet-5-5",
    "claude-haiku-4-5",
    "claude-fable-5-1",
];

#[test]
fn known_models_are_real_ids() {
    assert_eq!(KNOWN.first(), Some(&"claude-opus-5-5"));
    for id in KNOWN {
        assert!(REAL.contains(id), "{id} is not a real model id");
    }
}
