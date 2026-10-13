use std::time::{Duration, Instant};

use kiln_core::parse_layer;
use serde_json::json;

fn schema() -> serde_json::Value {
    json!({"type": "object", "properties": {"editor": {"type": "object"}}})
}

#[test]
fn unknown_keys_are_reported_in_linear_time() {
    // 20,000 lines of about 1 KB each, a 20 MB source. The padding is a
    // trailing comment: a bare 200 KB fixture of `kNNNNNN=1` lines runs
    // under 5 s at base on this machine, so it wouldn't catch a per-key
    // rescan; this one takes tens of seconds there.
    let padding = "#".repeat(1_000);
    let source: String = (1..=20_000)
        .map(|n| format!("k{n:06}=1 {padding}\n"))
        .collect();
    let layer = parse_layer("settings.toml".as_ref(), source);
    let started = Instant::now();
    let diagnostics = layer.check_schema(&schema());
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(diagnostics.len(), 20_000);
    let last = diagnostics.last().unwrap();
    assert_eq!((last.line, last.column), (20_000, 1));
}

#[test]
fn line_and_column_match_the_old_scan() {
    // Line 1 ends in CRLF; on line 2 a multi-byte key precedes `zz` (which
    // starts at char 21); line 3 has no trailing newline.
    let source = "a=1\r\neditor = { \"\u{e9}\" = 1, zz = 2 }\r\n  z=2".to_owned();
    let layer = parse_layer("settings.toml".as_ref(), source);
    let schema =
        json!({"type": "object", "properties": {"editor": {"type": "object", "properties": {}}}});
    let found: Vec<(String, usize, usize)> = layer
        .check_schema(&schema)
        .into_iter()
        .map(|d| (d.key.unwrap(), d.line, d.column))
        .collect();
    assert_eq!(
        found,
        vec![
            ("a".into(), 1, 1),
            ("editor.zz".into(), 2, 21),
            ("editor.\u{e9}".into(), 2, 12),
            ("z".into(), 3, 3),
        ]
    );
}
