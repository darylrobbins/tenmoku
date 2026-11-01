//! docs/design-language.md keeps up with the theme: a new token group without
//! a section fails here.

use std::{fs, path::Path};

use kiln_core::theme::TOKEN_GROUPS;

const SECTIONS: [&str; 6] = [
    "Colour tokens",
    "Spacing",
    "Type",
    "Icons",
    "Motion",
    "Gaps",
];

#[test]
fn every_token_group_has_a_section() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/design-language.md");
    let doc = fs::read_to_string(&path).unwrap();
    let has = |heading: String| doc.lines().any(|line| line == heading);
    for section in SECTIONS {
        assert!(has(format!("## {section}")), "no `## {section}` section");
    }
    for group in TOKEN_GROUPS {
        assert!(has(format!("### {group}")), "no `### {group}` heading");
    }
}
