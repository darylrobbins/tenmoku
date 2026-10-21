//! The highlight names semantic tokens land on.

use fe_syntax::{Highlight, resolve_theme};

#[test]
fn type_parameter_and_enum_member_are_names() {
    for name in ["type.parameter", "constant.enum_member"] {
        assert_eq!(Highlight::from_capture(name).unwrap().name(), name);
    }
}

#[test]
fn a_theme_styling_type_and_constant_styles_both() {
    let table = resolve_theme(&[("type", 1), ("constant", 2)]);
    let style = |name| table[Highlight::from_capture(name).unwrap().index()];
    assert_eq!(style("type.parameter"), Some(1));
    assert_eq!(style("constant.enum_member"), Some(2));
}
