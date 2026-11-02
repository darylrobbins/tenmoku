use fe_command::{Actor, ContextPredicate, DispatchError, Effect};

use super::commit;
use crate::{FeedError, HeadlessVim, VimKey, VimState, context, parse_keys, registry};

#[test]
fn move_dispatches_through_the_registry() {
    let registry = registry();
    let id = "vim.move".parse().unwrap();
    let meta = registry.get(&id).expect("vim.move is registered");
    assert_eq!(
        meta.context,
        Some(ContextPredicate::parse("editor && (vim.mode == normal || vim.visual)").unwrap())
    );
    assert!(!meta.mutates_buffer);
    assert!(meta.args_schema.is_some());

    let text = (0..10)
        .map(|row| format!("row {row}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut vim = HeadlessVim::new(&text, (1, 2));
    vim.feed("5j").unwrap();
    assert_eq!(vim.cursor(), (6, 2));
    assert!(!vim.model().is_dirty());

    // The same invocation, dispatched as the host would.
    let invocation = commit("5j").context(context(&VimState::new()));
    let mut vim = HeadlessVim::new(&text, (1, 2));
    let dispatched = vim.dispatch(&invocation).unwrap();
    assert_eq!(dispatched.effect, Effect::Changed);
    assert!(dispatched.transactions.is_empty());
    assert_eq!(vim.cursor(), (6, 2));

    // Not while an operator is pending.
    let mut operator = VimState::new();
    operator.feed(VimKey::Char('d'));
    let pending = commit("5j").context(context(&operator));
    assert!(matches!(
        vim.dispatch(&pending),
        Err(DispatchError::NotInContext { .. })
    ));
    assert_eq!(vim.cursor(), (6, 2));

    assert_eq!(vim.feed("K"), Err(FeedError::Forwarded(VimKey::Char('K'))));
    assert!(matches!(vim.feed("<nope>"), Err(FeedError::Keys(_))));
    assert_eq!(vim.mode(), "normal");
    assert!(parse_keys("<C-r><lt><Left><TAB><cr>").is_ok());
}

#[test]
fn operate_and_put_are_registered() {
    let registry = registry();
    for (id, context) in [
        (
            "vim.operate",
            "editor && (vim.mode == normal || vim.visual)",
        ),
        ("vim.put", "editor && vim.mode == normal"),
    ] {
        let meta = registry.get(&id.parse().unwrap()).expect(id);
        assert_eq!(
            meta.context,
            Some(ContextPredicate::parse(context).unwrap()),
            "{id}"
        );
        assert!(meta.mutates_buffer, "{id}");
        assert!(meta.args_schema.is_some(), "{id}");
    }
    // Not while an operator is pending: `dd` is one invocation, not `d`
    // then `d`.
    let mut operator = VimState::new();
    operator.feed(VimKey::Char('d'));
    let mut vim = HeadlessVim::new("one\ntwo", (0, 0));
    let pending = commit("dd").context(context(&operator));
    assert!(matches!(
        vim.dispatch(&pending),
        Err(DispatchError::NotInContext { .. })
    ));
    vim.feed("dd").unwrap();
    assert_eq!(vim.text(), "two");
}

#[test]
fn card_3_commands_are_registered() {
    let registry = registry();
    for id in [
        "vim.insert",
        "vim.join",
        "vim.replace_char",
        "vim.toggle_case",
        "vim.undo",
        "vim.redo",
        "vim.repeat",
    ] {
        let meta = registry.get(&id.parse().unwrap()).expect(id);
        assert_eq!(
            meta.context,
            Some(ContextPredicate::parse("editor && vim.mode == normal").unwrap()),
            "{id}"
        );
        assert!(meta.mutates_buffer, "{id}");
        assert!(meta.args_schema.is_some(), "{id}");
    }
    let meta = registry.get(&"vim.normal_mode".parse().unwrap()).unwrap();
    assert_eq!(
        meta.context,
        Some(ContextPredicate::parse("editor && vim.mode == insert").unwrap())
    );
    // The keys commit them.
    for (keys, command) in [
        ("3o", "vim.insert"),
        ("2J", "vim.join"),
        ("2rx", "vim.replace_char"),
        ("~", "vim.toggle_case"),
        ("2u", "vim.undo"),
        ("<C-r>", "vim.redo"),
        ("3.", "vim.repeat"),
        ("C", "vim.operate"),
        ("s", "vim.operate"),
        ("S", "vim.operate"),
    ] {
        assert_eq!(commit(keys).command.as_str(), command, "{keys}");
    }
}

#[test]
fn undo_and_redo_run_as_the_invoking_actor() {
    let ai = || Actor::Ai {
        agent: "agent".into(),
        session: "session".into(),
    };
    let text = "one two\nnext";
    let mut vim = HeadlessVim::new(text, (0, 0));
    let context = context(&VimState::new());
    let dispatch = |vim: &mut HeadlessVim, keys: &str, actor: Actor| {
        vim.dispatch(&commit(keys).context(context.clone()).actor(actor))
            .unwrap()
    };
    dispatch(&mut vim, "dw", ai());
    assert_eq!(vim.text(), "two\nnext");

    // The user's undo leaves the agent's edit alone.
    dispatch(&mut vim, "u", Actor::User);
    assert_eq!(vim.text(), "two\nnext");

    dispatch(&mut vim, "u", ai());
    vim.model_mut().sync_display();
    assert_eq!(vim.text(), text);

    dispatch(&mut vim, "<C-r>", Actor::User);
    assert_eq!(vim.text(), text);
    dispatch(&mut vim, "<C-r>", ai());
    vim.model_mut().sync_display();
    assert_eq!(vim.text(), "two\nnext");
}

#[test]
fn a_command_after_ctrl_o_that_does_nothing_still_repaints() {
    // `<Left>` at column 0 after `<C-o>`, as the keymap binds it: it moves
    // nothing, but insert mode resumes, which the view must paint.
    let mut vim = HeadlessVim::new("abc", (0, 0));
    vim.feed("i<C-o>").unwrap();
    assert!(vim.state().insert_pending());
    let left = fe_command::Invocation::of("vim.move")
        .unwrap()
        .args(serde_json::json!({ "motion": "left" }))
        .context(context(vim.state()));
    let dispatched = vim.dispatch(&left).unwrap();
    assert_eq!(vim.mode(), "insert");
    assert_eq!(dispatched.effect, Effect::Changed);
    // Outside `<C-o>` it stays nothing.
    let mut vim = HeadlessVim::new("abc", (0, 0));
    let left = left.context(context(vim.state()));
    assert_eq!(vim.dispatch(&left).unwrap().effect, Effect::Nothing);
}

#[test]
fn line_undo_alternates_between_cloned_states() {
    let mut vim = HeadlessVim::new("zero\nabc\ndef", (1, 0));
    vim.feed("x").unwrap();
    assert_eq!(vim.text(), "zero\nbc\ndef");
    // A clone shares the buffer, so what it does through `U` is the
    // original's to see: its next `U` puts the deleted text back.
    let clone = vim.state().clone();
    let original = std::mem::replace(vim.state_mut(), clone);
    vim.feed("U").unwrap();
    assert_eq!(vim.text(), "zero\nabc\ndef");
    *vim.state_mut() = original;
    vim.feed("U").unwrap();
    assert_eq!(vim.text(), "zero\nbc\ndef");
    let clone = vim.state().clone();
    let original = std::mem::replace(vim.state_mut(), clone);
    vim.feed("U").unwrap();
    assert_eq!(vim.text(), "zero\nabc\ndef");
    *vim.state_mut() = original;
    vim.feed("U").unwrap();
    assert_eq!(vim.text(), "zero\nbc\ndef");
}
