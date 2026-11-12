//! The user-facing text that reaches status bars, menus and refactoring
//! errors comes from the catalogue (kiln-l10n), not from literals.

use lsp_types::CompletionItemKind;

use crate::event::{LspError, buffer_changed, not_open, too_large};
use crate::kind_label;

#[test]
fn every_completion_kind_has_a_label_that_is_not_its_key() {
    for number in 1..=25 {
        let kind: CompletionItemKind = serde_json::from_value(number.into()).unwrap();
        let label = kind_label(kind);
        assert!(
            !label.is_empty() && !label.starts_with("lsp-kind-"),
            "{kind:?}: {label}"
        );
    }
    assert_eq!(kind_label(CompletionItemKind::ENUM_MEMBER), "enum member");
    assert_eq!(
        kind_label(CompletionItemKind::TYPE_PARAMETER),
        "type parameter"
    );
}

#[test]
fn unavailable_reasons_and_server_failures_resolve_through_keys() {
    assert_eq!(buffer_changed().to_string(), "buffer changed");
    assert_eq!(too_large().to_string(), "too large for the language server");
    assert_eq!(not_open().to_string(), "not open on a language server");
    // The error shows its reason as it is, a kind's text or free text.
    assert_eq!(
        LspError::Unavailable(not_open()).to_string(),
        "not open on a language server"
    );
    assert_eq!(LspError::Unavailable("rename".into()).to_string(), "rename");
    assert_eq!(
        LspError::ServerDown.to_string(),
        "the language server isn't running"
    );
    assert_eq!(
        kiln_l10n::tr!(
            "lsp-server-initialize-timeout",
            name = "rust-analyzer",
            seconds = 30u64
        ),
        "rust-analyzer didn't answer initialize within 30 s"
    );
}

#[test]
fn workspace_edit_refusals_resolve_through_keys() {
    assert_eq!(
        kiln_l10n::tr!("lsp-edit-open-twice", path = "src/a.rs"),
        "src/a.rs is open twice"
    );
    assert_eq!(
        kiln_l10n::tr!("lsp-edit-after-deleted", path = "src/a.rs"),
        "it edits src/a.rs, which it deleted earlier"
    );
    assert_eq!(kiln_l10n::tr!("lsp-edit-overlap"), "its edits overlap");
}

#[test]
fn exit_messages_are_whole_for_every_kind() {
    use crate::launcher::ExitInfo;
    use crate::server::exit_text;

    let code = ExitInfo {
        code: Some(3),
        signal: None,
    };
    let signal = ExitInfo {
        code: None,
        signal: Some(9),
    };
    let plain = ExitInfo {
        code: None,
        signal: None,
    };
    assert_eq!(
        exit_text("lsp-server-crashed", "gopls", code),
        "gopls exited with code 3"
    );
    assert_eq!(
        exit_text("lsp-server-crashed", "gopls", signal),
        "gopls was killed by signal 9"
    );
    assert_eq!(
        exit_text("lsp-server-crashed", "gopls", plain),
        "gopls exited"
    );
    let before = "lsp-server-exited-before-initialize";
    assert_eq!(
        exit_text(before, "gopls", code),
        "gopls exited with code 3 before answering initialize"
    );
    assert_eq!(
        exit_text(before, "gopls", signal),
        "gopls was killed by signal 9 before answering initialize"
    );
    assert_eq!(
        exit_text(before, "gopls", plain),
        "gopls exited before answering initialize"
    );
}

#[test]
fn backlog_messages_are_whole_for_each_unit() {
    assert_eq!(
        kiln_l10n::tr!("lsp-backlog-over-mib", count = 64usize),
        "server output backlog over 64 MiB"
    );
    assert_eq!(
        kiln_l10n::tr!("lsp-backlog-over-bytes", count = 1024usize),
        "server output backlog over 1024 bytes"
    );
}
