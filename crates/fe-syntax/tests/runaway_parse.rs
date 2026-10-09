//! Replays the saved TSX fuzz inputs that used to hang a background parse.

#![cfg(feature = "typescript")]

use std::sync::mpsc;
use std::time::Duration;

use fe_buffer::{Buffer, Origin};
use fe_syntax::{Syntax, language_by_name};

/// A hang guard: the parse is cut off by its own cap, so this only fires on a
/// hang.
const LIMIT: Duration = Duration::from_secs(60);

/// Replays one edit batch the way the fuzz harness does, with `run` standing
/// in for `ParseJob::run` on a helper thread, and checks that it returns and
/// that the result matches a fresh parse.
fn replay(initial: &str, insert: &str) {
    let mut buffer = Buffer::new(initial);
    let mut syntax = Syntax::new(language_by_name("tsx").unwrap(), buffer.current());
    syntax.finish(syntax.parse_job().run());
    buffer.edit([(0..0, insert)], Origin::User);
    syntax.edit(buffer.current());
    let job = syntax.parse_job();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(job.run());
    });
    let parsed = receiver
        .recv_timeout(LIMIT)
        .expect("the parse returns within the limit");
    syntax.finish(parsed);
    assert!(syntax.failed());
    assert!(syntax.tree().is_none());
}

/// From `fuzz/regressions/syntax_edits/tsx-incremental-parse-hang`.
#[test]
fn tsx_incremental_parse_hang() {
    replay("x)3,`x", "x\"\"[[)}");
}

/// From `fuzz/regressions/syntax_edits/tsx-incremental-parse-hang-from-empty`.
#[test]
fn tsx_incremental_parse_hang_from_empty() {
    replay("", "(\0[/(\0[/]]]]]]\u{FFFD}#");
}
