//! A help's missing `activeParameter` is the first parameter.

use serde_json::json;

use super::super::convert;

fn active(value: serde_json::Value) -> (usize, Option<usize>, Option<usize>) {
    let help = convert(serde_json::from_value(value).unwrap()).unwrap();
    let own = help.signatures[help.active_signature].active_parameter;
    (help.active_signature, help.active_parameter, own)
}

fn two() -> serde_json::Value {
    json!({ "label": "f(a, b)", "parameters": [{ "label": "a" }, { "label": "b" }] })
}

#[test]
fn omitted_active_parameter_is_the_first() {
    assert_eq!(active(json!({ "signatures": [two()] })), (0, Some(0), None));
    let both = json!([{ "label": "f()", "parameters": [] }, two()]);
    assert_eq!(
        active(json!({ "signatures": both, "activeSignature": 1 })),
        (1, Some(0), None)
    );
    assert_eq!(active(json!({ "signatures": both })), (0, None, None));
    assert_eq!(
        active(json!({ "signatures": [{ "label": "f()" }] })),
        (0, None, None)
    );
    for (given, want) in [(1, 1), (9, 1)] {
        let help = json!({ "signatures": [two()], "activeParameter": given });
        assert_eq!(active(help), (0, Some(want), None));
    }
}
