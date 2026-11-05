// SPDX-License-Identifier: Apache-2.0

use kiln_command::{ContextFrame, ContextStack, Invocation, Source};

use super::{keymap, keystroke, stack};
use crate::{KeyOutcome, Matcher};

fn dispatch(command: &str, context: &ContextStack) -> KeyOutcome {
    KeyOutcome::Dispatch(
        Invocation::of(command)
            .unwrap()
            .source(Source::Keyboard)
            .context(context.clone()),
    )
}

fn unmatched(keys: &[&str]) -> KeyOutcome {
    KeyOutcome::Unmatched(keys.iter().map(|key| keystroke(key)).collect())
}

const FOLD_ALL: &str = "\
[[bindings]]
keys = \"cmd-k cmd-0\"
command = \"editor.fold_all\"
context = \"editor\"
";

#[test]
fn chords_and_pending() {
    let editor = stack(&["workbench", "editor"]);
    let mut matcher = Matcher::new(keymap(FOLD_ALL, ""));

    // The chord dispatches on its last key, stamped with the stack.
    assert_eq!(
        matcher.push(keystroke("cmd-k"), &editor),
        KeyOutcome::Pending
    );
    assert_eq!(matcher.pending(), [keystroke("cmd-k")]);
    assert_eq!(
        matcher.push(keystroke("cmd-0"), &editor),
        dispatch("editor.fold_all", &editor)
    );
    assert!(matcher.pending().is_empty());

    // A key that doesn't continue it hands back every key swallowed.
    assert_eq!(
        matcher.push(keystroke("cmd-k"), &editor),
        KeyOutcome::Pending
    );
    assert_eq!(
        matcher.push(keystroke("x"), &editor),
        unmatched(&["cmd-k", "x"])
    );
    assert!(matcher.pending().is_empty());

    // The chord timer hands back the keys of a chord with no match yet.
    assert_eq!(
        matcher.push(keystroke("cmd-k"), &editor),
        KeyOutcome::Pending
    );
    assert_eq!(matcher.flush(), unmatched(&["cmd-k"]));

    // Out of context, the chord's first key isn't swallowed.
    let workbench = stack(&["workbench"]);
    assert_eq!(
        matcher.push(keystroke("cmd-k"), &workbench),
        unmatched(&["cmd-k"])
    );

    // A binding that's a prefix of another waits for the chord timer.
    let prefix = "[[bindings]]\nkeys = \"cmd-k\"\ncommand = \"workspace.open_recent\"\n";
    matcher.set_keymap(keymap(FOLD_ALL, prefix));
    assert_eq!(
        matcher.push(keystroke("cmd-k"), &editor),
        KeyOutcome::Pending
    );
    assert_eq!(matcher.flush(), dispatch("workspace.open_recent", &editor));
    assert_eq!(matcher.flush(), KeyOutcome::Unmatched(Vec::new()));

    // A key that doesn't continue the chord still hands every key back.
    assert_eq!(
        matcher.push(keystroke("cmd-k"), &editor),
        KeyOutcome::Pending
    );
    assert_eq!(
        matcher.push(keystroke("x"), &editor),
        unmatched(&["cmd-k", "x"])
    );
    // And the full chord still dispatches.
    assert_eq!(
        matcher.push(keystroke("cmd-k"), &editor),
        KeyOutcome::Pending
    );
    assert_eq!(
        matcher.push(keystroke("cmd-0"), &editor),
        dispatch("editor.fold_all", &editor)
    );

    // Swapping the keymap drops a chord in progress.
    assert_eq!(
        matcher.push(keystroke("cmd-k"), &editor),
        KeyOutcome::Pending
    );
    matcher.set_keymap(keymap(FOLD_ALL, ""));
    assert!(matcher.pending().is_empty());
    assert_eq!(matcher.flush(), KeyOutcome::Unmatched(Vec::new()));
}

#[test]
fn context_changes_mid_chord() {
    let bindings = "\
[[bindings]]
keys = \"cmd-k cmd-0 cmd-1\"
command = \"app.three\"

[[bindings]]
keys = \"cmd-k\"
command = \"app.recent\"

[[bindings]]
keys = \"cmd-k\"
command = \"editor.kill\"
context = \"editor && !read_only\"

[[bindings]]
keys = \"cmd-k cmd-0\"
command = \"editor.fold\"
context = \"vim.mode == normal\"
";
    let mut matcher = Matcher::new(keymap(bindings, ""));
    let editor = stack(&["editor"]);
    let frame = || ContextFrame::new("editor");
    let read_only = ContextStack::new().with(frame().flag("read_only"));
    let normal = ContextStack::new().with(frame().set("vim.mode", "normal"));
    let mut chord = |first: &ContextStack, second: &ContextStack| {
        assert_eq!(matcher.push(keystroke("cmd-k"), first), KeyOutcome::Pending);
        assert_eq!(
            matcher.push(keystroke("cmd-0"), second),
            KeyOutcome::Pending
        );
        matcher.flush()
    };

    // The editor's binding stops holding: the context-free one runs, in
    // the latest stack.
    assert_eq!(
        chord(&editor, &read_only),
        dispatch("app.recent", &read_only)
    );
    // A binding that starts to hold takes over: a deeper one for the same
    // keys, or one for more of the keys pressed.
    let workbench = stack(&["workbench"]);
    assert_eq!(chord(&workbench, &editor), dispatch("editor.kill", &editor));
    assert_eq!(chord(&editor, &normal), dispatch("editor.fold", &normal));
}

#[test]
fn context_precedence() {
    // The deepest context wins, whatever the order.
    let defaults = "\
[[bindings]]
keys = \"cmd-p\"
command = \"editor.palette\"
context = \"editor\"

[[bindings]]
keys = \"cmd-p\"
command = \"workbench.palette\"
context = \"workbench\"

[[bindings]]
keys = \"cmd-p\"
command = \"app.palette\"

[[bindings]]
keys = \"cmd-o\"
command = \"workbench.open\"
context = \"workbench\"

[[bindings]]
keys = \"cmd-o\"
command = \"app.open\"

[[bindings]]
keys = \"cmd-j\"
command = \"editor.default_join\"
context = \"editor\"

[[bindings]]
keys = \"cmd-l\"
command = \"editor.first\"
context = \"editor\"

[[bindings]]
keys = \"cmd-l\"
command = \"editor.second\"
context = \"workbench && editor\"

[[bindings]]
keys = \"cmd-e\"
command = \"editor.edit\"
context = \"editor && !read_only\"

[[bindings]]
keys = \"j\"
command = \"vim.down\"
context = \"vim.mode == normal\"
";
    let user = "\
[[bindings]]
keys = \"cmd-j\"
command = \"editor.user_join\"
context = \"!read_only && editor\"
";
    let mut matcher = Matcher::new(keymap(defaults, user));
    let mut press = |key: &str, context: &ContextStack| {
        let outcome = matcher.push(keystroke(key), context);
        match outcome {
            KeyOutcome::Dispatch(invocation) => {
                assert_eq!(&invocation.context, context);
                Some(invocation.command.to_string())
            }
            KeyOutcome::Unmatched(_) => None,
            outcome => panic!("{outcome:?}"),
        }
    };
    let both = stack(&["workbench", "editor"]);
    assert_eq!(press("cmd-p", &both).as_deref(), Some("editor.palette"));
    assert_eq!(press("cmd-o", &both).as_deref(), Some("workbench.open"));
    let workbench = stack(&["workbench"]);
    assert_eq!(
        press("cmd-p", &workbench).as_deref(),
        Some("workbench.palette")
    );
    assert_eq!(press("cmd-p", &stack(&[])).as_deref(), Some("app.palette"));

    // At equal depth, the user layer beats the defaults, and a later entry
    // an earlier one.
    assert_eq!(press("cmd-j", &both).as_deref(), Some("editor.user_join"));
    assert_eq!(press("cmd-l", &both).as_deref(), Some("editor.second"));

    // `editor && !read_only` never matches in a read-only editor.
    let read_only: ContextStack = [
        ContextFrame::new("workbench"),
        ContextFrame::new("editor").flag("read_only"),
    ]
    .into_iter()
    .collect();
    assert_eq!(press("cmd-e", &both).as_deref(), Some("editor.edit"));
    assert_eq!(press("cmd-e", &read_only), None);
    assert_eq!(
        press("cmd-j", &read_only).as_deref(),
        Some("editor.default_join")
    );

    // `vim.mode == normal` matches only with that value.
    let mode =
        |mode: &str| stack(&["workbench"]).with(ContextFrame::new("editor").set("vim.mode", mode));
    assert_eq!(press("j", &mode("normal")).as_deref(), Some("vim.down"));
    assert_eq!(press("j", &mode("insert")), None);
    assert_eq!(press("j", &both), None);
}
