use fe_command::Source;
use serde_json::{Value, json};

use super::{commit, run};
use crate::{MoveArgs, OperateArgs, Pending, PendingKind, Step, VimKey, step};

/// Checks `keys` commit `command` with `args`, from the keyboard, and that
/// the args round-trip through their type.
fn commits(keys: &str, command: &str, args: Value) {
    let invocation = commit(keys);
    assert_eq!(invocation.command.as_str(), command, "{keys}");
    assert_eq!(invocation.args, args, "{keys}");
    assert_eq!(invocation.source, Source::Keyboard, "{keys}");
    assert!(
        invocation.context.is_empty(),
        "{keys}: the caller stamps the context"
    );
    let again = match command {
        "vim.move" => {
            serde_json::to_value(serde_json::from_value::<MoveArgs>(args.clone()).unwrap())
        }
        _ => serde_json::to_value(serde_json::from_value::<OperateArgs>(args.clone()).unwrap()),
    };
    assert_eq!(again.unwrap(), args, "{keys} round-trips");
}

fn operate(keys: &str, args: Value) {
    commits(keys, "vim.operate", args);
}

fn moves(keys: &str, args: Value) {
    commits(keys, "vim.move", args);
}

#[test]
fn grammar_folds_counts_registers_and_operators() {
    operate(
        "2d3w",
        json!({"operator": "delete", "target": {"motion": "word_forward"}, "count": 6}),
    );
    operate(
        "d3w",
        json!({"operator": "delete", "target": {"motion": "word_forward"}, "count": 3}),
    );
    operate(
        "3dd",
        json!({"operator": "delete", "target": {"motion": "line"}, "count": 3}),
    );
    operate(
        "d3d",
        json!({"operator": "delete", "target": {"motion": "line"}, "count": 3}),
    );
    operate(
        "\"ayy",
        json!({"operator": "yank", "target": {"motion": "line"}, "register": "a"}),
    );
    operate(
        ">>",
        json!({"operator": "indent", "target": {"motion": "line"}}),
    );
    operate(
        "<<",
        json!({"operator": "outdent", "target": {"motion": "line"}}),
    );
    operate(
        "cc",
        json!({"operator": "change", "target": {"motion": "line"}}),
    );
    operate(
        "\"A2y3e",
        json!({"operator": "yank", "target": {"motion": "word_end"}, "count": 6, "register": "A"}),
    );
    moves("5j", json!({"motion": "down", "extend": false, "count": 5}));
    moves("G", json!({"motion": "document_end", "extend": false}));
    moves(
        "1G",
        json!({"motion": "document_end", "extend": false, "count": 1}),
    );
    moves("0", json!({"motion": "line_start", "extend": false}));
    moves(
        "10j",
        json!({"motion": "down", "extend": false, "count": 10}),
    );
    // A register means nothing to a move.
    moves(
        "\"a3k",
        json!({"motion": "up", "extend": false, "count": 3}),
    );
    for (key, motion) in [
        ("h", "left"),
        ("l", "right"),
        ("k", "up"),
        ("w", "word_forward"),
        ("b", "word_backward"),
        ("e", "word_end"),
        ("W", "big_word_forward"),
        ("B", "big_word_backward"),
        ("E", "big_word_end"),
        ("^", "first_non_blank"),
        ("$", "line_end"),
    ] {
        moves(key, json!({"motion": motion, "extend": false}));
    }
}

#[test]
fn prefixes_targets_cancel_and_forward() {
    for (keys, motion) in [
        ("dfx", "find_forward"),
        ("dtx", "till_forward"),
        ("dFx", "find_backward"),
        ("dTx", "till_backward"),
    ] {
        operate(
            keys,
            json!({"operator": "delete", "target": {"motion": {motion: "x"}}}),
        );
    }
    moves(
        "3f<Tab>",
        json!({"motion": {"find_forward": "\t"}, "extend": false, "count": 3}),
    );
    for (keys, operator, object) in [
        ("ci(", "change", "inner_paren"),
        ("cib", "change", "inner_paren"),
        ("da\"", "delete", "a_double_quote"),
        ("yiW", "yank", "inner_big_word"),
        ("da}", "delete", "a_brace"),
        ("yi'", "yank", "inner_single_quote"),
    ] {
        operate(
            keys,
            json!({"operator": operator, "target": {"text_object": object}}),
        );
    }
    moves("gg", json!({"motion": "document_start", "extend": false}));
    operate(
        "2dgg",
        json!({"operator": "delete", "target": {"motion": "document_start"}, "count": 2}),
    );

    for (keys, kind) in [
        ("3", PendingKind::Count),
        ("\"", PendingKind::Register),
        ("\"a", PendingKind::Register),
        ("3\"a", PendingKind::Register),
        ("\"a3", PendingKind::Count),
        ("d", PendingKind::Operator),
        ("d3", PendingKind::Count),
        ("g", PendingKind::G),
        ("f", PendingKind::Find),
        ("di", PendingKind::TextObject),
        ("3r", PendingKind::Replace),
    ] {
        match run(keys) {
            Step::Wait(pending) => assert_eq!(pending.kind(), Some(kind), "{keys}"),
            other => panic!("{keys} gave {other:?}"),
        }
    }
    assert_eq!(Pending::new().kind(), None);

    for keys in [
        "d<Esc>", "3<Esc>", "dq", "\"!", "g<Esc>", "dy", "d\"", "di<Esc>", "dix", "3q", "f<Esc>",
        "gx", "3Z", "r<Esc>", "r<CR>", "dr", "dJ", "di<C-r>",
    ] {
        assert_eq!(run(keys), Step::Cancel, "{keys}");
    }
    for keys in ["q", ":", "/", "K", "Z", "<C-x>", "<CR>"] {
        assert_eq!(run(keys), Step::Forward, "{keys}");
    }
    assert_eq!(run("<Esc>"), Step::Cancel);

    moves(
        "4294967296j",
        json!({"motion": "down", "extend": false, "count": u32::MAX}),
    );
    operate(
        "99999d99999w",
        json!({"operator": "delete", "target": {"motion": "word_forward"}, "count": u32::MAX}),
    );

    // By value, with no model: the type says so.
    let _: fn(Pending, VimKey) -> Step = step;
}
