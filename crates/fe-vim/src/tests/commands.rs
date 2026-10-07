use fe_command::{ContextPredicate, DispatchError, Effect};

use super::commit;
use crate::{FeedError, HeadlessVim, VimKey, VimState, context, parse_keys, registry};

#[test]
fn move_dispatches_through_the_registry() {
    let registry = registry();
    let id = "vim.move".parse().unwrap();
    let meta = registry.get(&id).expect("vim.move is registered");
    assert_eq!(
        meta.context,
        Some(ContextPredicate::parse("editor && vim.mode == normal").unwrap())
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

    assert_eq!(vim.feed("q"), Err(FeedError::Forwarded(VimKey::Char('q'))));
    assert!(matches!(vim.feed("<nope>"), Err(FeedError::Keys(_))));
    // `vim.operate` is card 2's.
    assert!(matches!(
        vim.feed("dd"),
        Err(FeedError::Dispatch(DispatchError::UnknownCommand(_)))
    ));
    assert_eq!(vim.mode(), "normal");
    assert!(parse_keys("<C-r><lt><Left><TAB><cr>").is_ok());
}
