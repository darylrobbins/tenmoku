use fe_buffer::{Origin, ResolvedSelection};
use fe_command::{ContextPredicate, ContextStack, Invocation};
use serde_json::{Value, json};

use super::{Measuring, commit, registers, run};
use crate::{
    FeedError, HeadlessVim, Pending, Step, VimKey, VimState, VisualKind, context, parse_keys,
    registry, step,
};

fn ai() -> Origin {
    Origin::Ai {
        agent: "agent".into(),
        session: "session".into(),
    }
}

/// Steps `keys` through the reducer from nothing pending in a visual mode.
/// Every key but the last must wait; returns what the last did.
fn run_visual(keys: &str) -> Step {
    let keys = parse_keys(keys).unwrap();
    let (last, init) = keys.split_last().expect("some keys");
    let mut pending = Pending::visual();
    for &key in init {
        match step(pending, key) {
            Step::Wait(next) => pending = next,
            other => panic!("{key:?} gave {other:?}, not Wait"),
        }
    }
    step(pending, *last)
}

/// What `keys` commit in a visual mode: the command and its args.
fn visual_commit(keys: &str) -> (String, Value) {
    match run_visual(keys) {
        Step::Commit(invocation) => (invocation.command.to_string(), invocation.args),
        other => panic!("{keys} gave {other:?}, not Commit"),
    }
}

fn operate(operator: Value, target: &str) -> (String, Value) {
    let args = json!({"operator": operator, "target": target});
    ("vim.operate".to_string(), args)
}

/// The model's selections as ranges and whether each is reversed.
fn selections(vim: &HeadlessVim) -> Vec<(std::ops::Range<usize>, bool)> {
    vim.model()
        .selections()
        .into_iter()
        .map(|selection| (selection.range, selection.reversed))
        .collect()
}

/// The text the model's primary selection covers.
fn selected(vim: &HeadlessVim) -> String {
    let range = vim.model().primary_selection().range;
    vim.model().text().slice_to_string(range)
}

#[test]
fn reducer_and_commands() {
    // From normal mode.
    for (keys, kind) in [("v", "char"), ("V", "line")] {
        let invocation = commit(keys);
        assert_eq!(invocation.command.as_str(), "vim.visual");
        assert_eq!(invocation.args, json!({"kind": kind}));
    }
    // A count before them is out of scope.
    assert_eq!(run("3v"), Step::Cancel);

    // In a visual mode, operators act at once on the selection.
    for (key, operator) in [
        ("d", json!("delete")),
        ("x", json!("delete")),
        ("y", json!("yank")),
        ("c", json!("change")),
        ("s", json!("change")),
        (">", json!("indent")),
        ("<lt>", json!("outdent")),
        ("~", json!("toggle_case")),
        ("u", json!("lowercase")),
        ("U", json!("uppercase")),
        ("J", json!("join")),
        ("rx", json!({"replace": "x"})),
    ] {
        assert_eq!(visual_commit(key), operate(operator, "selection"), "{key}");
    }
    for (key, operator) in [
        ("X", "delete"),
        ("D", "delete"),
        ("Y", "yank"),
        ("C", "change"),
        ("R", "change"),
    ] {
        assert_eq!(
            visual_commit(key),
            operate(json!(operator), "selected_lines"),
            "{key}"
        );
    }
    // `S` waits for surround's pair (`feature/surround.rs`).
    assert_eq!(
        visual_commit("S)"),
        (
            "vim.surround_add".into(),
            json!({"target": "selection", "pair": ")"})
        )
    );
    assert_eq!(
        visual_commit("3>"),
        (
            "vim.operate".into(),
            json!({"operator": "indent", "target": "selection", "count": 3})
        )
    );
    assert_eq!(
        visual_commit("\"ad"),
        (
            "vim.operate".into(),
            json!({"operator": "delete", "target": "selection", "register": "a"})
        )
    );
    assert_eq!(visual_commit("o"), ("vim.visual_swap".into(), Value::Null));
    assert_eq!(visual_commit("gv"), ("vim.reselect".into(), Value::Null));
    assert_eq!(commit("gv").command.as_str(), "vim.reselect");
    assert_eq!(
        visual_commit("<Esc>"),
        ("vim.visual_exit".into(), Value::Null)
    );
    assert_eq!(
        visual_commit("3<Esc>"),
        ("vim.visual_exit".into(), Value::Null)
    );
    assert_eq!(
        visual_commit("iw"),
        ("vim.select".into(), json!({"text_object": "inner_word"}))
    );
    assert_eq!(
        visual_commit("2a("),
        (
            "vim.select".into(),
            json!({"text_object": "a_paren", "count": 2})
        )
    );
    assert_eq!(
        visual_commit("w"),
        (
            "vim.move".into(),
            json!({"motion": "word_forward", "extend": true})
        )
    );
    assert_eq!(
        visual_commit("V"),
        ("vim.visual".into(), json!({"kind": "line"}))
    );
    // A prefix's `<Esc>` cancels only it; what isn't vim's here is forwarded.
    assert_eq!(run_visual("f<Esc>"), Step::Cancel);
    for keys in [".", "I", "q", "<C-r>"] {
        assert_eq!(run_visual(keys), Step::Forward, "{keys}");
    }
    assert_eq!(Pending::visual().kind(), None);

    // The mode on the context, and the flag every visual kind sets.
    let stamped = |state: &VimState| {
        let stack = context(state);
        let frame = stack.frames().last().unwrap().clone();
        (
            frame.values.get("vim.mode").cloned().unwrap(),
            frame.flags.contains("vim.visual"),
        )
    };
    let mut vim = HeadlessVim::new("abc\ndef", (0, 0));
    assert_eq!(stamped(vim.state()), ("normal".into(), false));
    let normal = vim.state().clone();
    vim.feed("v").unwrap();
    assert_eq!(vim.mode(), "visual");
    assert_eq!(stamped(vim.state()), ("visual".into(), true));
    let visual = vim.state().clone();
    vim.feed("V").unwrap();
    assert_eq!(vim.mode(), "visual_line");
    assert_eq!(stamped(vim.state()), ("visual_line".into(), true));
    let visual_line = vim.state().clone();
    vim.feed("<Esc>").unwrap();
    assert_eq!(stamped(vim.state()), ("normal".into(), false));
    let mut operator_pending = VimState::new();
    operator_pending.feed(VimKey::Char('d'));
    let mut insert = HeadlessVim::new("abc", (0, 0));
    insert.feed("i").unwrap();

    // Each command's context holds only in its modes.
    let registry = registry();
    let holds = |id: &str, state: &VimState| {
        let meta = registry.get(&id.parse().unwrap()).expect(id);
        let predicate: &ContextPredicate = meta.context.as_ref().expect(id);
        let stack: ContextStack = context(state);
        predicate.eval(&stack)
    };
    let states = [
        &normal,
        &operator_pending,
        insert.state(),
        &visual,
        &visual_line,
    ];
    for (id, modes) in [
        ("vim.visual", [true, false, false, true, true]),
        ("vim.reselect", [true, false, false, true, true]),
        ("vim.visual_exit", [false, false, false, true, true]),
        ("vim.visual_swap", [false, false, false, true, true]),
        ("vim.select", [false, false, false, true, true]),
        ("vim.move", [true, false, false, true, true]),
        ("vim.operate", [true, false, false, true, true]),
        ("vim.put", [true, false, false, false, false]),
    ] {
        let held: Vec<bool> = states.iter().map(|state| holds(id, state)).collect();
        assert_eq!(held, modes, "{id}");
    }

    // By value, with no model: the type says so.
    let _: fn(Pending, VimKey) -> Step = step;
}

#[test]
fn selection_on_the_model() {
    let text = "abc\nde\u{301}f";
    let mut vim = HeadlessVim::new(text, (0, 0));
    vim.feed("v").unwrap();
    assert_eq!(selections(&vim), [(0..1, false)]);
    vim.feed("l").unwrap();
    assert_eq!(selections(&vim), [(0..2, false)]);

    let mut vim = HeadlessVim::new(text, (0, 2));
    vim.feed("vh").unwrap();
    assert_eq!(selections(&vim), [(1..3, true)]);
    assert_eq!(vim.cursor(), (0, 1));
    assert_eq!(vim.visual_anchor(), Some((0, 2)));
    assert_eq!(vim.state().visual_cursor(vim.model()), Some(1));

    // The whole grapheme under the cursor.
    let mut vim = HeadlessVim::new(text, (1, 1));
    vim.feed("v").unwrap();
    assert_eq!(selections(&vim), [(5..8, false)]);
    assert_eq!(selected(&vim), "e\u{301}");

    // Whole rows, without the last line break.
    let mut vim = HeadlessVim::new(text, (0, 1));
    vim.feed("V").unwrap();
    assert_eq!(selections(&vim), [(0..3, false)]);
    vim.feed("j").unwrap();
    assert_eq!(selections(&vim), [(0..text.len(), false)]);
    assert_eq!(vim.cursor(), (1, 1));

    // `$` takes the line break.
    let mut vim = HeadlessVim::new(text, (0, 1));
    vim.feed("v$").unwrap();
    assert_eq!(vim.cursor(), (0, 3));
    assert_eq!(selected(&vim), "bc\n");

    // One cursor: visual mode keeps the primary caret only.
    let mut vim = HeadlessVim::new("abc def ghi", (0, 0));
    vim.model_mut()
        .set_selections([caret(0), caret(4), caret(8)]);
    vim.feed("vl").unwrap();
    assert_eq!(selections(&vim), [(8..10, false)]);

    // `vim.move` keeps the anchor whatever `extend` says.
    for extend in [false, true] {
        let mut vim = HeadlessVim::new("abc def", (0, 1));
        vim.feed("v").unwrap();
        let invocation = Invocation::new("vim.move".parse().unwrap())
            .args(json!({"motion": "word_forward", "extend": extend}))
            .context(context(vim.state()));
        vim.dispatch(&invocation).unwrap();
        assert_eq!(selections(&vim), [(1..5, false)], "extend {extend}");
        assert_eq!(vim.visual_anchor(), Some((0, 1)));
    }
}

#[test]
fn objects_over_wide_characters() {
    // A word object grown onto a row of multibyte characters ends inside
    // one: its last grapheme is found without slicing there.
    let mut vim = HeadlessVim::new("ab\n日本語 x", (0, 0));
    vim.feed("vwiw").unwrap();
    assert_eq!(vim.cursor(), (1, 6));
    assert_eq!(selected(&vim), "ab\n日本語");
}

fn caret(offset: usize) -> ResolvedSelection {
    ResolvedSelection {
        range: offset..offset,
        reversed: false,
    }
}

#[test]
fn operators_marks_and_reselect() {
    // A delete over two rows is one transaction, undone by one `u`.
    let text = "abc def\nghi jkl\nmno";
    let mut vim = HeadlessVim::new(text, (0, 5));
    vim.feed("vj").unwrap();
    let d = commit_in(&vim, "d");
    let dispatched = vim.dispatch(&d).unwrap();
    assert_eq!(dispatched.transactions.len(), 1);
    assert_eq!(vim.text(), "abc dl\nmno");
    assert_eq!(vim.mode(), "normal");
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), text);

    // Linewise, into `"` and `1`, shifting `1` to `2`.
    let mut vim = HeadlessVim::new("x\nabc\ndef\nghi", (0, 0));
    vim.feed("ddVjd").unwrap();
    assert_eq!(vim.text(), "ghi");
    let registers = registers(&vim);
    assert!(registers.contains(&('"', "abc\ndef\n".into(), true)));
    assert!(registers.contains(&('1', "abc\ndef\n".into(), true)));
    assert!(registers.contains(&('2', "x\n".into(), true)));

    // A yank writes `"` and `0`, and leaves the caret at the start.
    let mut vim = HeadlessVim::new("abc def", (0, 5));
    vim.feed("vhhy").unwrap();
    assert_eq!(vim.mode(), "normal");
    assert_eq!(vim.cursor(), (0, 3));
    assert_eq!(
        super::registers(&vim)[..2],
        [('"', " de".into(), false), ('0', " de".into(), false)]
    );

    // `c` deletes and enters insert mode in one transaction.
    let mut vim = HeadlessVim::new("abc def", (0, 4));
    vim.feed("vlc").unwrap();
    assert_eq!(vim.mode(), "insert");
    vim.feed("XY<Esc>").unwrap();
    assert_eq!(vim.text(), "abc XYf");
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "abc def");

    // `'<` and `'>` after leaving by `<Esc>`, an operator or `v`.
    let marks = |vim: &HeadlessVim| (vim.mark('<'), vim.mark('>'));
    for keys in ["vjh<Esc>", "vjhy", "vjhv"] {
        let mut vim = HeadlessVim::new(text, (0, 5));
        assert_eq!(marks(&vim), (None, None));
        vim.feed(keys).unwrap();
        assert_eq!(vim.mode(), "normal", "{keys}");
        assert_eq!(marks(&vim), (Some((0, 5)), Some((1, 4))), "{keys}");
        let mark = vim.state().mark('<').unwrap();
        assert_eq!(mark.position(vim.model().snapshot()), (0, 5));
    }
    // Linewise: column 0 and the last row's last byte.
    let mut vim = HeadlessVim::new(text, (1, 2));
    vim.feed("Vk<Esc>").unwrap();
    assert_eq!(marks(&vim), (Some((0, 0)), Some((1, 6))));

    // `gv` after a row put in above reselects the same text.
    let mut vim = HeadlessVim::new(text, (0, 4));
    vim.feed("vjh<Esc>").unwrap();
    let before = {
        let mut again = HeadlessVim::new(text, (0, 4));
        again.feed("vjh").unwrap();
        selected(&again)
    };
    vim.feed("ggOnew<Esc>gv").unwrap();
    assert_eq!(vim.mode(), "visual");
    assert_eq!(selected(&vim), before);
    assert_eq!(marks(&vim), (Some((1, 4)), Some((2, 3))));

    // Nothing to reselect: nothing happens.
    let mut vim = HeadlessVim::new(text, (0, 4));
    vim.feed("gv").unwrap();
    assert_eq!((vim.mode(), vim.cursor()), ("normal", (0, 4)));
    assert_eq!(selections(&vim), [(4..4, false)]);

    // `o` swaps the cursor and the anchor.
    let mut vim = HeadlessVim::new(text, (0, 1));
    vim.feed("vjl").unwrap();
    let range = vim.model().primary_selection().range;
    vim.feed("o").unwrap();
    assert_eq!((vim.cursor(), vim.visual_anchor()), ((0, 1), Some((1, 2))));
    assert_eq!(selections(&vim), [(range, true)]);

    // For hosts: a visual mode over a range.
    let mut vim = HeadlessVim::new("abcdef\nghi", (1, 0));
    let mut state = vim.state().clone();
    state.select_visual(vim.model_mut(), VisualKind::Char, 0, 3);
    assert_eq!(state.mode_name(), "visual");
    assert_eq!(state.visual_cursor(vim.model()), Some(3));
    assert_eq!(state.visual_anchor(vim.model()), Some(0));
    assert_eq!(selections(&vim), [(0..4, false)]);
    state.select_visual(vim.model_mut(), VisualKind::Line, 2, 8);
    assert_eq!(state.mode_name(), "visual_line");
    assert_eq!(selections(&vim), [(0..10, false)]);
}

/// The invocation `keys` commit in `vim`'s mode, stamped with its context.
fn commit_in(vim: &HeadlessVim, keys: &str) -> Invocation {
    let mut state = vim.state().clone();
    let mut last = None;
    for key in parse_keys(keys).unwrap() {
        last = Some(state.feed(key));
    }
    match last {
        Some(Step::Commit(invocation)) => invocation.context(context(vim.state())),
        other => panic!("{keys} gave {other:?}"),
    }
}

#[test]
fn named_arrows_continue_pending_input() {
    // In the reducer: none cancels.
    for (keys, motion) in [
        ("3<Down>", "down"),
        ("d<Down>", "down"),
        ("2d<Right>", "right"),
        ("\"a<Up>", "up"),
    ] {
        let invocation = commit(keys);
        assert_eq!(
            invocation.args["target"]["motion"]
                .as_str()
                .or(invocation.args["motion"].as_str()),
            Some(motion),
            "{keys}"
        );
    }
    assert_eq!(run("<Down>"), Step::Forward);
    assert!(matches!(run_visual("<Left>"), Step::Commit(_)));

    let rows = "r0\nr1\nr2\nr3\nr4";
    let mut vim = HeadlessVim::new(rows, (0, 1));
    vim.feed("3<Down>").unwrap();
    assert_eq!(vim.cursor(), (3, 1));

    let mut arrow = HeadlessVim::new(rows, (1, 0));
    let mut letter = HeadlessVim::new(rows, (1, 0));
    arrow.feed("d<Down>").unwrap();
    letter.feed("dj").unwrap();
    assert_eq!(arrow.text(), "r0\nr3\nr4");
    assert_eq!(arrow.text(), letter.text());

    let mut vim = HeadlessVim::new("ae\u{301}bc", (0, 0));
    vim.feed("2d<Right>").unwrap();
    assert_eq!(vim.text(), "bc");

    let mut vim = HeadlessVim::new("ae\u{301}bc", (0, 0));
    vim.feed("v<Right>").unwrap();
    assert_eq!(selected(&vim), "ae\u{301}");
    assert_eq!(vim.mode(), "visual");
}

#[test]
fn oversized_shift_counts_are_refused() {
    // `>` puts in count 'shiftwidth's on each row: past what a put may
    // insert, it's refused before anything is allocated or changed.
    let mut vim = HeadlessVim::new("a", (0, 0));
    let error = vim.feed("v4294967295>").unwrap_err();
    assert!(matches!(error, FeedError::Dispatch(_)), "{error:?}");
    assert_eq!(vim.text(), "a");
    assert_eq!(vim.mode(), "visual");

    // `.` with a count reaches the same check, and stays in normal mode.
    let mut vim = HeadlessVim::new("a\nb", (0, 0));
    vim.feed("v>j").unwrap();
    assert!(vim.feed("4294967295.").is_err());
    assert_eq!((vim.text(), vim.mode()), ("    a\nb".to_string(), "normal"));
    assert_eq!(vim.cursor(), (1, 0));
    // A count that fits shifts.
    vim.feed("v100>").unwrap();
    assert_eq!(vim.text(), format!("    a\n{}b", " ".repeat(400)));
}

#[test]
fn external_edits_keep_the_selection_in_step() {
    // After another origin's edit, the painted selection is made what the
    // visual one takes (as the host does once the model catches up), so a
    // yank takes what was painted.
    let check = |text: &str,
                 cursor: (u32, usize),
                 keys: &str,
                 edit: (std::ops::Range<usize>, &str),
                 painted: &str| {
        let mut vim = HeadlessVim::new(text, cursor);
        vim.feed(keys).unwrap();
        vim.model_mut().buffer_mut().edit([edit], ai());
        vim.sync_visual();
        assert_eq!(selected(&vim), painted, "{text:?} {keys}");
        vim.feed("y").unwrap();
        let yanked = vim.registers().get('"').unwrap().text.clone();
        assert_eq!(yanked.trim_end_matches('\n'), painted, "{text:?} {keys}");
    };
    // The selected text deleted: what follows it is selected.
    check("abc", (0, 0), "vl", (0..2, ""), "c");
    // Text replaced inside the selection.
    check("abcdef", (0, 0), "vlll", (1..3, "XYZW"), "aXYZWd");
    // A combining mark put after the cursor's character joins its grapheme.
    check("abc", (0, 0), "vl", (2..2, "\u{301}"), "ab\u{301}");
    // Linewise: a row put in above doesn't join the selection.
    check("ab\ncd\nef", (0, 0), "Vj", (0..0, "X\n"), "ab\ncd");
    // Linewise: the selection's rows deleted.
    check("ab\ncd\nef", (0, 0), "Vj", (0..6, ""), "ef");
}

#[test]
fn join_marks_survive_edits_made_while_measuring() {
    // Measuring text as the model catches up after `J` may edit the buffer
    // through another model, here emptying the joined row: the marks are
    // placed from the text before the join, so nothing underflows.
    for keys in ["vjJ", "vjjJggj."] {
        let mut vim = HeadlessVim::new("aa\n  bb\ncc\ndd\nee", (0, 1));
        let mut split = vim.model().split();
        let mut edited = false;
        let _measuring = Measuring::new(move |after_edit| {
            if after_edit && !edited {
                edited = true;
                let row = split.text().row_len(0) as usize;
                split.buffer_mut().edit([(0..row, "")], ai());
            }
        });
        vim.feed(keys).unwrap();
        assert_eq!(vim.mark('<').map(|(row, _)| row), Some(0), "{keys}");
        assert_eq!(vim.mark('>').map(|(row, _)| row), Some(0), "{keys}");
    }
}
