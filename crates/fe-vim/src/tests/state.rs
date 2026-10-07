use fe_command::{ContextFrame, ContextPredicate};

use crate::{Step, VimKey, VimState, context, parse_keys};

fn feed(state: &mut VimState, keys: &str) -> Step {
    let mut last = Step::Cancel;
    for key in parse_keys(keys).unwrap() {
        last = state.feed(key);
    }
    last
}

#[test]
fn mode_on_the_editor_frame() {
    let mut state = VimState::new();
    let frame = state.stamp(ContextFrame::new("editor"));
    assert_eq!(frame.name, "editor");
    assert_eq!(frame.get("vim.mode"), Some("normal"));
    assert!(!frame.has("vim.pending"));

    assert!(matches!(feed(&mut state, "d"), Step::Wait(_)));
    let frame = state.stamp(ContextFrame::new("editor"));
    assert_eq!(frame.get("vim.mode"), Some("operator_pending"));
    assert_eq!(frame.get("vim.pending_kind"), Some("operator"));
    assert_eq!(feed(&mut state, "<Esc>"), Step::Cancel);

    for (keys, kind) in [("3", "count"), ("\"a", "register")] {
        let mut state = VimState::new();
        assert!(matches!(feed(&mut state, keys), Step::Wait(_)), "{keys}");
        let pending = state.stamp(ContextFrame::new("editor"));
        assert_eq!(pending.get("vim.mode"), Some("normal"), "{keys}");
        assert!(pending.has("vim.pending"), "{keys}");
        assert_eq!(pending.get("vim.pending_kind"), Some(kind), "{keys}");

        // Committed or cancelled, nothing is pending, even on a frame that
        // was stamped while something was.
        let mut committed = state.clone();
        assert!(matches!(committed.feed(VimKey::Char('j')), Step::Commit(_)));
        let mut cancelled = state.clone();
        assert_eq!(cancelled.feed(VimKey::Esc), Step::Cancel);
        for state in [committed, cancelled] {
            for frame in [
                state.stamp(ContextFrame::new("editor")),
                state.stamp(pending.clone()),
            ] {
                assert_eq!(frame.get("vim.mode"), Some("normal"), "{keys}");
                assert!(!frame.has("vim.pending"), "{keys}");
                assert_eq!(frame.get("vim.pending_kind"), None, "{keys}");
            }
        }
    }

    let normal = ContextPredicate::parse("editor && vim.mode == normal").unwrap();
    let mut state = VimState::new();
    assert!(normal.eval(&context(&state)));
    assert_eq!(context(&state).frames().len(), 1);
    feed(&mut state, "d");
    assert!(!normal.eval(&context(&state)));
    feed(&mut state, "d");
    assert!(normal.eval(&context(&state)));

    let plain = fe_editor::commands::context();
    assert_eq!(plain.get("vim.mode"), None);
    assert_eq!(ContextFrame::new("editor").get("vim.mode"), None);
}
