//! Each language's tree memory against its source, the ratio its parse limit
//! is set from (`src/parse_limit.rs`). Rust, JSON and Markdown are left to
//! `cargo bench -p fe-syntax`, which measures them on their own corpora.
//!
//! The source is the language's golden sample, `tests/golden/<name>.<ext>`,
//! repeated whole to about 4 MB (see [`corpus`] for the few that need help to
//! repeat without parse errors). A tree's memory is malloc growth across one
//! parse, the median of three, each in a fresh [`Syntax`]. macOS and Linux
//! with glibc only.
//!
//! It also prints each language's tree memory per visible node
//! ([`Syntax::tree_nodes`]), Rust, JSON and Markdown's too, on the golden
//! samples: what [`fe_syntax::TREE_BYTES_PER_NODE`] is set from (PERF.md,
//! Tree memory by language).
//!
//! Run with `cargo bench -p fe-syntax --bench tree_ratio`.

use std::{fs, path::PathBuf};

use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name, language_names};

mod support;

const SOURCE_BYTES: usize = 4 * 1024 * 1024;
const MEASURED_ELSEWHERE: [&str; 3] = ["rust", "json", "markdown"];

/// The golden sample for `name`: `<name>.<ext>`, not its `.golden` output.
/// `None` for a language with none.
fn sample(name: &str) -> Option<String> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let path = fs::read_dir(&dir)
        .expect("tests/golden is readable")
        .map(|entry| entry.expect("tests/golden is readable").path())
        .find(|path| {
            path.extension().is_some_and(|ext| ext != "golden")
                && path.file_stem().is_some_and(|stem| stem == name)
        })?;
    Some(fs::read_to_string(path).expect("the sample is UTF-8"))
}

/// Whole copies of `sample` within [`SOURCE_BYTES`], as one file that parses
/// without errors.
fn corpus(name: &str, sample: &str) -> String {
    // Lines a file may hold only at its top, which later copies leave out:
    // Lua's shebang, Kotlin's package and imports, XML's prolog.
    let header_lines = match name {
        "lua" => 1,
        "kotlin" => 4,
        "xml" => 20,
        _ => 0,
    };
    let header_len: usize = sample
        .split_inclusive('\n')
        .take(header_lines)
        .map(str::len)
        .sum();
    let (header, body) = sample.split_at(header_len);
    let copies = match name {
        // tree-sitter-php stops recognising heredocs after about 200 in one
        // file, and the sample has three, so PHP gets about 0.1 MB. Other
        // languages' ratios at 50 copies are within 0.2 of their 4 MB ones.
        "php" => 50,
        _ => (SOURCE_BYTES - header.len()) / body.len(),
    };
    let body = body.repeat(copies);
    match name {
        // A document has one root element.
        "xml" => format!("{header}<copies>\n{body}</copies>\n"),
        _ => format!("{header}{body}"),
    }
}

fn main() {
    if support::allocated() == 0 {
        eprintln!("tree_ratio: not measured on this platform");
        return;
    }
    let mut per_node = Vec::new();
    for name in language_names() {
        let language = language_by_name(name).expect("every listed language loads");
        let Some(sample) = sample(name) else {
            eprintln!("{name}: no sample in tests/golden");
            continue;
        };
        let text = corpus(name, &sample);
        let buffer = Buffer::new(&text);
        let mut trees = Vec::new();
        let mut nodes = 0;
        for _ in 0..3 {
            let mut syntax = Syntax::new(language, &buffer.snapshot());
            let before = support::allocated();
            syntax.finish(syntax.parse_job().run());
            let tree = support::allocated().saturating_sub(before);
            match syntax.tree() {
                Some(parsed) if parsed.root_node().has_error() => {
                    eprintln!("{name}: the corpus has parse errors");
                }
                Some(_) => {
                    trees.push(tree);
                    nodes = syntax.tree_nodes();
                }
                None => eprintln!("{name}: no tree (the parse overran or panicked)"),
            }
        }
        trees.sort_unstable();
        let Some(&tree) = trees.get(trees.len() / 2) else {
            eprintln!("{name}: not measured");
            continue;
        };
        if !MEASURED_ELSEWHERE.contains(&name) {
            support::tree_memory(name, text.len(), tree);
        }
        let bytes = tree as f64 / nodes.max(1) as f64;
        eprintln!("{name}: {nodes} visible nodes, {bytes:.0} bytes per node");
        per_node.push(bytes);
    }
    per_node.sort_unstable_by(f64::total_cmp);
    if let Some(median) = per_node.get(per_node.len() / 2) {
        eprintln!(
            "bytes per node: median {median:.0} over {} languages (TREE_BYTES_PER_NODE is {})",
            per_node.len(),
            fe_syntax::TREE_BYTES_PER_NODE
        );
    }
}
