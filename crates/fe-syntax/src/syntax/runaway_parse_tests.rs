use fe_buffer::{Buffer, Origin};

use crate::{Syntax, language_by_name};

#[test]
fn overrun_retries_from_scratch_then_gives_up() {
    // Big enough that tree-sitter asks the stop check at all.
    let mut buffer = Buffer::new(&"fn f() {}\n".repeat(20_000));
    let mut syntax = Syntax::new(language_by_name("rust").unwrap(), buffer.current());
    syntax.finish(syntax.parse_job().run());
    buffer.edit([(0..0, "fn g() {")], Origin::User);
    syntax.edit(buffer.current());

    // Only the incremental attempt overruns.
    let mut attempts = 0;
    let parsed = syntax.parse_job().run_until(|_| {
        attempts += 1;
        let overrun = attempts == 1;
        move || overrun
    });
    assert_eq!(attempts, 2);
    assert!(parsed.incremental);
    assert!(parsed.tree.is_some());
    syntax.finish(parsed);
    assert!(!syntax.failed());
    assert!(syntax.tree().is_some());

    // Both do.
    buffer.edit([(0..0, "fn g() {")], Origin::User);
    syntax.edit(buffer.current());
    let parsed = syntax.parse_job().run_until(|_| || true);
    assert!(parsed.incremental);
    assert!(parsed.tree.is_none());
    syntax.finish(parsed);
    assert!(syntax.failed());
    assert!(syntax.tree().is_none());
}
