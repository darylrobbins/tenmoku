//! The memory report's estimate of a tree's memory, its visible nodes times
//! [`TREE_BYTES_PER_NODE`], against what parsing it took from malloc,
//! measured as `benches/tree_ratio.rs` measures it: malloc growth across one
//! parse in a fresh [`Syntax`], the median of three. macOS and Linux with
//! glibc only; elsewhere there's nothing to compare with.

use std::{fs, path::PathBuf};

use fe_buffer::Buffer;
use fe_syntax::{Syntax, TREE_BYTES_PER_NODE, language_by_name};

#[allow(dead_code, reason = "the benches' report isn't printed here")]
#[path = "../benches/support/mod.rs"]
mod support;

/// About 64 KB of Rust: the golden sample, repeated.
fn sample() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/rust.rs");
    let sample = fs::read_to_string(path).expect("the Rust sample is readable");
    sample.repeat((64 * 1024 / sample.len()).max(1))
}

#[test]
fn node_count_estimate_is_within_2x_of_malloc_growth() {
    if support::allocated() == 0 {
        eprintln!("not measured on this platform");
        return;
    }
    let language = language_by_name("rust").expect("Rust is built in");
    let buffer = Buffer::new(&sample());
    let mut grown = Vec::new();
    let mut nodes = 0;
    for _ in 0..3 {
        let mut syntax = Syntax::new(language, &buffer.snapshot());
        let before = support::allocated();
        syntax.finish(syntax.parse_job().run());
        grown.push(support::allocated().saturating_sub(before));
        assert!(syntax.tree().is_some(), "the sample parses");
        nodes = syntax.tree_nodes();
    }
    grown.sort_unstable();
    let measured = grown[1];
    let estimate = nodes * TREE_BYTES_PER_NODE;
    eprintln!(
        "{nodes} visible nodes: estimate {estimate} B, malloc grew {measured} B ({:.0} B a node)",
        measured as f64 / nodes as f64
    );
    assert!(nodes > 0, "the tree has nodes");
    assert!(
        estimate <= measured * 2 && measured <= estimate * 2,
        "estimate {estimate} B is not within 2x of the {measured} B malloc grew"
    );
}
