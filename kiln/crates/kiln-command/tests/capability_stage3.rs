// SPDX-License-Identifier: Apache-2.0

use kiln_command::{Capability, Grants};

fn cap(capability: &str) -> Capability {
    capability.parse().unwrap()
}

#[test]
fn stage3_each_capability_parses_and_round_trips() {
    for text in [
        "jobs",
        "language",
        "panels",
        "tasks",
        "lsp:rust-analyzer",
        "lsp:acme-ls",
    ] {
        let capability = cap(text);
        assert_eq!(capability.to_string(), text);
        let json = serde_json::to_string(&capability).unwrap();
        assert_eq!(json, format!("\"{text}\""));
        assert_eq!(
            serde_json::from_str::<Capability>(&json).unwrap(),
            capability
        );
    }
    assert_eq!(cap("lsp:gopls"), Capability::Lsp("gopls".into()));
    assert!("jobs:x".parse::<Capability>().is_err());
    assert!("lsp".parse::<Capability>().is_err());

    let grants: Grants = [cap("lsp:gopls"), cap("jobs")].into_iter().collect();
    assert!(grants.allows(&cap("lsp:gopls")));
    assert!(grants.allows(&cap("jobs")));
    assert!(!grants.allows(&cap("lsp:pyright")));
    assert!(!grants.allows(&cap("tasks")));
}

#[test]
fn stage3_lsp_wildcard_is_refused() {
    for bad in ["lsp:*", "lsp:rust-*", "lsp:*-analyzer"] {
        assert!(bad.parse::<Capability>().is_err(), "{bad}");
    }
}

#[test]
fn stage3_lsp_name_refuses_separators() {
    for bad in [
        "lsp:", "lsp:a/b", "lsp:a\\b", "lsp:a:b", "lsp:a b", "lsp:a\tb",
    ] {
        assert!(bad.parse::<Capability>().is_err(), "{bad:?}");
    }
}
