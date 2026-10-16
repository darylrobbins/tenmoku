//! deny.toml must ban the plan's "Do not use" crates, each with a reason, and
//! must not allow MPL-2.0 wholesale.

use std::fs;

fn deny_toml() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../deny.toml");
    fs::read_to_string(path).expect("read deny.toml")
}

/// Lines of the `[name]` table, up to the next table header.
fn table_lines<'a>(text: &'a str, name: &str) -> Vec<&'a str> {
    let header = format!("[{name}]");
    text.lines()
        .skip_while(|l| l.trim() != header)
        .skip(1)
        .take_while(|l| !l.trim_start().starts_with('['))
        .collect()
}

#[test]
fn plan_bans_are_listed() {
    let text = deny_toml();
    let bans = table_lines(&text, "bans");
    for name in [
        "tree-sitter-language-pack",
        "stack-graphs",
        "nucleo",
        "nucleo-matcher",
        "tree-house",
        "tree-house-bindings",
    ] {
        let needle = format!("crate = \"{name}\"");
        let line = bans
            .iter()
            .find(|l| l.contains(&needle))
            .unwrap_or_else(|| panic!("{name} is not in [bans] deny"));
        let reason = line
            .split("reason = \"")
            .nth(1)
            .and_then(|r| r.split('"').next())
            .unwrap_or("");
        assert!(!reason.trim().is_empty(), "{name} has no reason");
    }

    let licenses = table_lines(&text, "licenses");
    let allow: Vec<&str> = licenses
        .iter()
        .skip_while(|l| !l.trim_start().starts_with("allow = ["))
        .take_while(|l| l.trim() != "]")
        .copied()
        .collect();
    assert!(!allow.is_empty(), "[licenses] allow list not found");
    assert!(
        !allow.iter().any(|l| l.contains("MPL-2.0")),
        "[licenses] allow must not list MPL-2.0"
    );
}
