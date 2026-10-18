//! Code actions and signature help have timeouts of their own, not hover's.

use std::time::Duration;

use fe_config::settings::LspTimeouts;

#[test]
fn code_actions_and_signature_help_have_their_own_timeouts() {
    let ms = |method: &str, timeouts: &LspTimeouts| {
        crate::requests::timeout(method, timeouts).map(|t| t.as_millis())
    };
    assert_eq!(
        crate::requests::timeout("textDocument/codeAction", &LspTimeouts::default()),
        Some(Duration::from_secs(10))
    );
    let timeouts = LspTimeouts {
        hover: 500,
        ..Default::default()
    };
    assert_eq!(ms("textDocument/signatureHelp", &timeouts), Some(3000));
    assert_eq!(ms("textDocument/hover", &timeouts), Some(500));
}
