use std::time::Duration;

use fe_buffer::{Buffer, Origin};

use super::FAULT;
use crate::{Syntax, language_by_name};

const BUDGET: Duration = Duration::from_millis(1);

/// A parsed tree for `buffer` whose parse is recorded as having taken `took`,
/// then edited so it needs a reparse.
fn edited_after(buffer: &mut Buffer, took: Duration) -> Syntax {
    let mut syntax = Syntax::new(language_by_name("rust").unwrap(), buffer.current());
    syntax.finish(syntax.parse_job().run());
    buffer.edit([(0..0, "// ")], Origin::User);
    syntax.edit(buffer.current());
    // An incremental reparse, with the time it took set by hand.
    let mut parsed = syntax.parse_job().run();
    parsed.elapsed = took;
    assert!(parsed.incremental);
    syntax.finish(parsed);
    buffer.edit([(0..0, "// ")], Origin::User);
    syntax.edit(buffer.current());
    syntax
}

#[test]
fn skips_a_sync_reparse_after_a_slow_one() {
    let mut buffer = Buffer::new("fn f() {}\n");
    let mut syntax = edited_after(&mut buffer, BUDGET * 2);
    assert!(syntax.needs_parse());
    FAULT.set(Some("read"));
    assert!(!syntax.parse_within(BUDGET));
    assert_eq!(FAULT.get(), Some("read"), "the text wasn't read");
    assert!(!syntax.failed());
    FAULT.set(None);
}

#[test]
fn a_quick_reparse_turns_sync_parsing_back_on() {
    let mut buffer = Buffer::new("fn f() {}\n");
    let mut syntax = edited_after(&mut buffer, BUDGET * 2);
    assert!(!syntax.parse_within(BUDGET));
    // The background parse of the same text finishes quickly.
    let mut parsed = syntax.parse_job().run();
    parsed.elapsed = BUDGET / 2;
    syntax.finish(parsed);
    buffer.edit([(0..0, "// ")], Origin::User);
    syntax.edit(buffer.current());
    assert!(syntax.parse_until(BUDGET, || false));
    assert!(!syntax.failed());

    // A first full parse never turns it off, however long it took.
    let mut fresh = Syntax::new(language_by_name("rust").unwrap(), buffer.current());
    let mut parsed = fresh.parse_job().run();
    assert!(!parsed.incremental);
    parsed.elapsed = BUDGET * 100;
    fresh.finish(parsed);
    buffer.edit([(0..0, "// ")], Origin::User);
    fresh.edit(buffer.current());
    assert!(fresh.parse_until(BUDGET, || false));
    assert!(!fresh.failed());
}

#[test]
fn a_rejected_result_leaves_the_timing_alone() {
    let mut buffer = Buffer::new("fn f() {}\n");
    let mut syntax = edited_after(&mut buffer, BUDGET / 2);
    // A slow result for the version already parsed is rejected as stale.
    let mut stale = syntax.parse_job().run();
    syntax.finish(syntax.parse_job().run());
    stale.elapsed = BUDGET * 100;
    syntax.finish(stale);
    buffer.edit([(0..0, "// ")], Origin::User);
    syntax.edit(buffer.current());
    assert!(syntax.parse_until(BUDGET, || false));
}
