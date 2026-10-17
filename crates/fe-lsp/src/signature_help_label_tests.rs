//! String parameter labels are placed on the first whole-word match.

use lsp_types::{ParameterInformation, ParameterLabel, SignatureInformation};

use super::super::signature;

fn ranges(label: &str, parameters: &[&str]) -> Vec<std::ops::Range<usize>> {
    signature(SignatureInformation {
        label: label.to_string(),
        documentation: None,
        parameters: Some(
            parameters
                .iter()
                .map(|p| ParameterInformation {
                    label: ParameterLabel::Simple(p.to_string()),
                    documentation: None,
                })
                .collect(),
        ),
        active_parameter: None,
    })
    .parameters
}

#[test]
#[allow(clippy::single_range_in_vec_init)]
fn string_labels_are_found_as_words() {
    assert_eq!(ranges("max(a, b)", &["a", "b"]), [4..5, 7..8]);
    assert_eq!(ranges("fn foo(o: Opts)", &["o"]), [7..8]);
    assert_eq!(
        ranges("fn pick(i: u8, item: T)", &["i", "item"]),
        [8..9, 15..19]
    );
    assert_eq!(
        ranges("f(x: Vec<x>, y)", &["x: Vec<x>", "y"]),
        [2..11, 13..14]
    );
    assert_eq!(ranges("g(a, ...)", &["a", "..."]), [2..3, 5..8]);
    assert_eq!(ranges("g(a, aa)", &["a", "a", "zz"]), [2..3, 5..6, 8..8]);
    assert_eq!(ranges("éa(é, b)", &["é", "b"]), [4..6, 8..9]);
}
