//! Scanning with a WebAssembly grammar: each worker thread parses with a
//! store of its own.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use fe_project::PathFilter;
use fe_structural::{Pattern, ScanOptions, scan};
use fe_syntax::{GrammarDeadlines, GrammarSpec, register_language, set_grammar_deadlines};

#[test]
fn scan_runs_a_wasm_grammar_on_worker_threads() {
    // A wasm parse under the gate's load needs room; this binary's only test
    // sets it.
    set_grammar_deadlines(GrammarDeadlines {
        load: Duration::from_secs(30),
        parse: Duration::from_secs(30),
    });
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("run by cargo");
    let spec = GrammarSpec {
        language: "scan-json".into(),
        wasm: Path::new(&manifest).join("../fe-syntax/tests/fixtures/tree-sitter-json.wasm"),
        extensions: vec!["scanjson".into()],
        ..GrammarSpec::default()
    };
    let json = register_language(&spec).unwrap();
    let pattern = Pattern::compile(json, r#"{"a": $V}"#).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut expected = Vec::new();
    for ix in 0..12 {
        let rel = format!("f{ix:02}.scanjson");
        let text = if ix % 2 == 0 {
            expected.push((PathBuf::from(&rel), ix.to_string()));
            format!("[{{\"a\": {ix}}}, {{\"b\": 1}}]\n")
        } else {
            "{\"b\": {\"a\": 1, \"c\": 2}}\n".to_owned()
        };
        fs::write(root.join(&rel), text).unwrap();
    }
    // Other files aren't parsed in the language.
    fs::write(root.join("x.json5"), "{\"a\": 1}\n").unwrap();

    let options = ScanOptions {
        threads: Some(4),
        wasm: true,
        // Room for a loaded machine.
        file_deadline: Duration::from_secs(30),
        ..ScanOptions::default()
    };
    let auto = ScanOptions {
        threads: None,
        ..options.clone()
    };
    assert!((1..=4).contains(&auto.thread_count()));
    let mut files = Vec::new();
    let summary = scan(
        root,
        &pattern,
        None,
        &options,
        &PathFilter::default(),
        Arc::new(AtomicBool::new(false)),
        |batch| files.extend(batch),
    );
    assert_eq!(summary.parse_failed, 0, "{summary:?}");
    assert_eq!(summary.timed_out, 0);
    assert_eq!(summary.files_of_language, 12);
    let mut found: Vec<_> = files
        .iter()
        .map(|file| {
            assert_eq!(file.matches.len(), 1, "{:?}", file.rel);
            (file.rel.clone(), file.matches[0].captures[0].1.clone())
        })
        .collect();
    found.sort();
    assert_eq!(found, expected);
}
