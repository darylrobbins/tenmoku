//! Finding injections in a tree queried a split root at a time stops
//! visiting roots once the deadline passes or the matches are capped.

use std::time::Instant;

use fe_buffer::Buffer;

use super::{Layers, MAX_INJECTIONS, Painter, QUERIED_ROOTS};
use crate::*;

/// `statements` short tagged templates, each injecting HTML. Far more than
/// `FLAT_CHILDREN`, so the program is split into a root per statement.
fn templates(statements: usize) -> String {
    "html`<b>x</b>`;\n".repeat(statements)
}

/// The injections found in all of `text`, parsed as JavaScript, whether
/// finding them was complete, and how many roots were queried.
fn find(text: &str, deadline: Option<Instant>) -> (usize, bool, usize) {
    language_by_name("html");
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language_by_name("javascript").unwrap(), buffer.current());
    syntax.finish(syntax.parse_job().run_until(|_| || false));
    let mut layers = Layers::default();
    let mut painter = Painter {
        skipped: false,
        text: buffer.text(),
        range: 0..text.len(),
        deadline,
        host_parsed: true,
        layers: &mut layers,
        parser: None,
        complete: true,
    };
    QUERIED_ROOTS.set(0);
    let found = painter.find(syntax.language(), syntax.tree().unwrap(), None);
    (found.len(), painter.complete, QUERIED_ROOTS.get())
}

#[test]
fn every_split_root_is_queried_in_time() {
    // The program, then each statement.
    assert_eq!(find(&templates(2000), None), (2000, true, 1 + 2000));
}

#[test]
fn an_expired_deadline_queries_no_root() {
    assert_eq!(find(&templates(2000), Some(Instant::now())), (0, false, 0));
}

#[test]
fn capped_matches_query_no_later_root() {
    // Capped at the statement that finds the last match; past it the
    // injections stay plain, which is complete as it's the same every time.
    let found = find(&templates(MAX_INJECTIONS + 1000), None);
    assert_eq!(found, (MAX_INJECTIONS, true, 1 + MAX_INJECTIONS));
}
