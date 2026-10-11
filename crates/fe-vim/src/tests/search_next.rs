use fe_command::Effect;

use super::invocation;
use crate::{HeadlessVim, Motion, OperateArgs, Operator, Target};

fn slash(vim: &HeadlessVim) -> Option<&str> {
    Some(vim.registers().get('/')?.text.as_str())
}

fn highlight(vim: &HeadlessVim) -> Option<String> {
    vim.state()
        .highlight()
        .map(|pattern| format!("{pattern:?}"))
}

#[test]
fn next_star_and_highlight() {
    // `n`, `2n` and `N` go over the last search; `?` makes `n` go back.
    let mut vim = HeadlessVim::new("foo a foo b foo c foo", (0, 0));
    vim.feed("/foo<CR>").unwrap();
    assert_eq!(vim.cursor(), (0, 6));
    vim.feed("n").unwrap();
    assert_eq!(vim.cursor(), (0, 12));
    vim.feed("2n").unwrap();
    assert_eq!(vim.cursor(), (0, 0));
    vim.feed("N").unwrap();
    assert_eq!(vim.cursor(), (0, 18));
    vim.feed("0?foo<CR>n").unwrap();
    assert_eq!(vim.cursor(), (0, 12));
    vim.feed("N").unwrap();
    assert_eq!(vim.cursor(), (0, 18));

    // With no last search, `n` is E35 and moves nothing.
    let mut vim = HeadlessVim::new("foo foo", (0, 2));
    vim.feed("n").unwrap();
    assert_eq!(vim.cursor(), (0, 2));
    assert_eq!(
        vim.state().message(),
        Some("E35: No previous regular expression")
    );
    assert!(vim.state().highlight().is_none());

    // `*` is whole-word and sets the `/` register; `#` goes backward.
    let mut vim = HeadlessVim::new("foo foobar foo", (0, 0));
    vim.feed("*").unwrap();
    assert_eq!(vim.cursor(), (0, 11));
    assert_eq!(slash(&vim), Some("\\<foo\\>"));
    assert_eq!(vim.registers().last_search(), Some(("\\<foo\\>", true)));
    vim.feed("#").unwrap();
    assert_eq!(vim.cursor(), (0, 0));
    assert_eq!(vim.registers().last_search(), Some(("\\<foo\\>", false)));
    vim.feed("n").unwrap();
    assert_eq!(vim.cursor(), (0, 11));

    // `#` from inside a word searches from the word's start.
    let mut vim = HeadlessVim::new("foo bar foo", (0, 9));
    vim.feed("#").unwrap();
    assert_eq!(vim.cursor(), (0, 0));

    // With nothing but blanks left on the row, `*` is E348.
    let mut vim = HeadlessVim::new("foo   \nfoo", (0, 4));
    vim.feed("*").unwrap();
    assert_eq!(vim.cursor(), (0, 4));
    assert_eq!(vim.state().message(), Some("E348: No string under cursor"));
    assert_eq!(slash(&vim), None);

    // The highlight is the last pattern after every kind of search.
    let mut vim = HeadlessVim::new("foo bar foo", (0, 0));
    assert_eq!(highlight(&vim), None);
    vim.feed("/bar<CR>").unwrap();
    let bar = highlight(&vim).unwrap();
    vim.feed("*").unwrap();
    let foo = highlight(&vim).unwrap();
    assert_ne!(foo, bar);
    vim.feed("/bar<CR>n").unwrap();
    assert_eq!(highlight(&vim), Some(bar));
    vim.feed("#").unwrap();
    assert_eq!(highlight(&vim), Some(foo));

    // `dn` and `d*` are exclusive and one change each.
    let mut vim = HeadlessVim::new("a foo b foo c", (0, 0));
    vim.feed("/foo<CR>0dn").unwrap();
    assert_eq!(vim.text(), "foo b foo c");
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "a foo b foo c");
    let mut vim = HeadlessVim::new("foo b foo c", (0, 0));
    vim.feed("d*").unwrap();
    assert_eq!(vim.text(), "foo c");
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "foo b foo c");

    // In a visual mode the cursor moves and the anchor stays.
    let mut vim = HeadlessVim::new("foo a foo b foo", (0, 0));
    vim.feed("/foo<CR>0vn").unwrap();
    assert_eq!(vim.cursor(), (0, 6));
    assert_eq!(vim.visual_anchor(), Some((0, 0)));
    vim.feed("*").unwrap();
    assert_eq!(vim.cursor(), (0, 12));
    assert_eq!(vim.visual_anchor(), Some((0, 0)));

    // `n` and `N` keep the direction `/` or `?` gave; `N` doesn't flip it.
    let mut vim = HeadlessVim::new("a foo b foo c foo d foo", (0, 0));
    vim.feed("/foo<CR>NN").unwrap();
    assert_eq!(vim.cursor(), (0, 14));
    assert_eq!(vim.registers().last_search(), Some(("foo", true)));
    vim.feed("Nn").unwrap();
    assert_eq!(vim.cursor(), (0, 14));
    vim.feed("0d2N").unwrap();
    assert_eq!(vim.registers().last_search(), Some(("foo", true)));

    // `.` finds `n`, `N`, `*` and `#` again, not what they found.
    let mut vim = HeadlessVim::new("foo x foo bar x bar", (0, 0));
    vim.feed("d*w.").unwrap();
    assert_eq!(vim.text(), "foo bar");

    // A word is a run of one class: punctuation after it isn't in it.
    let mut vim = HeadlessVim::new("foo++ ++", (0, 3));
    vim.feed("*").unwrap();
    assert_eq!(vim.registers().last_search(), Some(("++", true)));
    assert_eq!(vim.cursor(), (0, 6));
    let mut vim = HeadlessVim::new("abc漢字 x", (0, 0));
    vim.feed("*").unwrap();
    assert_eq!(vim.registers().last_search(), Some(("\\<abc\\>", true)));
    assert_eq!(vim.cursor(), (0, 0));
    let mut vim = HeadlessVim::new("abc漢字 abc漢字", (0, 0));
    vim.feed("*").unwrap();
    assert_eq!(vim.cursor(), (0, 10));
    vim.feed("#").unwrap();
    assert_eq!(vim.cursor(), (0, 0));

    // An invalid pattern leaves the last search and its highlight; a
    // search that stays put still notifies the host.
    let mut vim = HeadlessVim::new("foo bar", (0, 0));
    vim.feed("/foo<CR>/a\\(<CR>").unwrap();
    assert_eq!(vim.registers().last_search(), Some(("foo", true)));
    assert!(vim.state().highlight().is_some());
    let mut vim = HeadlessVim::new("foo bar", (0, 0));
    vim.feed("*").unwrap();
    assert!(vim.state().highlight().is_some());

    // A failed repeat keeps the direction too: `N` doesn't flip it.
    let mut vim = HeadlessVim::new("foo bar foo", (0, 0));
    vim.feed("?zzz<CR>").unwrap();
    vim.feed("N").unwrap();
    vim.feed("NdN").unwrap();
    assert_eq!(vim.registers().last_search(), Some(("zzz", false)));
    vim.feed("v").unwrap();
    vim.feed("N<Esc>").unwrap();
    assert_eq!(vim.registers().last_search(), Some(("zzz", false)));
    // With only an invalid pattern, there's no last search to repeat.
    let mut vim = HeadlessVim::new("foo bar foo", (0, 0));
    vim.feed("/a\\(<CR>NN").unwrap();
    assert_eq!(vim.registers().last_search(), None);

    // A yank that stays put still notifies the host.
    let mut vim = HeadlessVim::new("foo x", (0, 0));
    let yank = OperateArgs {
        operator: Operator::Yank,
        target: Target::Motion(Motion::SearchWordForward),
        count: None,
        register: None,
    };
    let dispatched = vim.dispatch(&invocation("vim.operate", yank)).unwrap();
    assert_eq!(dispatched.effect, Effect::Notify);
    assert!(vim.state().highlight().is_some());
}
