//! 'incsearch': [`fe_vim::VimState::search_preview`] gives the match a `/`
//! or `?` line would land on while it's typed, and submitting the line
//! lands there.

use std::ops::Range;
use std::time::{Duration, Instant};

use fe_vim::HeadlessVim;
use fe_vim::search::{DEFAULT_TIME_LIMIT, set_time_limit};

/// Rows starting at 0, 9 and 18, with `word` at 4..8, 13..17 and 24..28.
const TEXT: &str = "one word\ntwo word\nthree word\n";

/// The preview of `line` on the line `open` opens with the cursor at
/// `cursor`, and where submitting it then puts the cursor.
fn preview(
    text: &str,
    cursor: (u32, usize),
    open: &str,
    line: &str,
) -> (Option<Range<usize>>, (u32, usize)) {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.feed(open).unwrap();
    let found = vim.state().search_preview(vim.model(), line);
    vim.feed(&format!("{line}<CR>")).unwrap();
    (found, vim.cursor())
}

#[test]
fn search_preview_finds_the_landing_match() {
    // Forward and back.
    assert_eq!(preview(TEXT, (0, 0), "/", "word"), (Some(4..8), (0, 4)));
    assert_eq!(preview(TEXT, (1, 0), "?", "word"), (Some(4..8), (0, 4)));
    // The count's match.
    assert_eq!(preview(TEXT, (0, 0), "2/", "word"), (Some(13..17), (1, 4)));
    assert_eq!(preview(TEXT, (2, 0), "2?", "word"), (Some(4..8), (0, 4)));
    // An `e` offset takes matches by their last character: from a match's
    // start, that match. The offset moves the cursor, not the match.
    assert_eq!(preview(TEXT, (0, 4), "/", "word/e"), (Some(4..8), (0, 7)));
    assert_eq!(preview(TEXT, (0, 4), "/", "word"), (Some(13..17), (1, 4)));
    assert_eq!(preview(TEXT, (0, 4), "/", "word/e-1"), (Some(4..8), (0, 6)));
    // Round the end, and round the start.
    assert_eq!(preview(TEXT, (2, 6), "/", "word"), (Some(4..8), (0, 4)));
    assert_eq!(preview(TEXT, (0, 0), "?", "word"), (Some(24..28), (2, 6)));
    // `?`'s own delimiter ends its pattern.
    assert_eq!(preview(TEXT, (2, 0), "?", "word?e"), (Some(13..17), (1, 7)));
    // From the vim cursor in a visual mode, and after an operator.
    assert_eq!(preview(TEXT, (0, 0), "v/", "word"), (Some(4..8), (0, 4)));
    let mut vim = HeadlessVim::new(TEXT, (0, 0));
    vim.feed("d/").unwrap();
    assert_eq!(vim.state().search_preview(vim.model(), "two"), Some(9..12));
    // A caret past its row's last character (End, or a click there)
    // searches from that character, as submit does.
    for (open, found, landed) in [("?", 2..3, (1, 0)), ("/", 2..3, (1, 0))] {
        let mut vim = HeadlessVim::new("a\na", (0, 0));
        vim.model_mut().set_cursor(1);
        vim.feed(open).unwrap();
        let previewed = vim.state().search_preview(vim.model(), "a");
        vim.feed("a<CR>").unwrap();
        assert_eq!((previewed, vim.cursor()), (Some(found), landed), "{open}");
    }

    // Nothing to preview.
    let none = |line: &str| {
        let mut vim = HeadlessVim::new(TEXT, (0, 0));
        vim.feed("/word<CR>/").unwrap();
        vim.state().search_preview(vim.model(), line)
    };
    // An empty pattern would be the last search's: not previewed.
    assert_eq!(none(""), None);
    assert_eq!(none("/e"), None);
    assert_eq!(none(r"wo\(rd"), None);
    assert_eq!(none("nowhere"), None);
    assert_eq!(none("word/;/two"), None);
    assert_eq!(none("word/x"), None);
    // The preview changed nothing: `n` goes on from the submitted search.
    let mut vim = HeadlessVim::new(TEXT, (0, 0));
    vim.feed("/word<CR>/").unwrap();
    assert_eq!(vim.state().search_preview(vim.model(), "two"), Some(9..12));
    vim.feed("<Esc>n").unwrap();
    assert_eq!(vim.cursor(), (1, 4));
    // An ex line, and no line at all.
    let mut vim = HeadlessVim::new(TEXT, (0, 0));
    assert_eq!(vim.state().search_preview(vim.model(), "word"), None);
    vim.feed(":").unwrap();
    assert_eq!(vim.state().search_preview(vim.model(), "word"), None);

    // 'incsearch' off.
    let mut vim = HeadlessVim::new(TEXT, (0, 0));
    assert!(vim.state().incsearch());
    vim.state_mut().set_incsearch(false);
    vim.feed("/").unwrap();
    assert_eq!(vim.state().search_preview(vim.model(), "word"), None);
    vim.state_mut().set_incsearch(true);
    assert_eq!(vim.state().search_preview(vim.model(), "word"), Some(4..8));

    // A slow pattern gives up at the preview's own 100 ms, though a search
    // may run for a minute: each match of `a` scans to the row's end for a
    // `z`, from every `a` of a long row going back.
    set_time_limit(Duration::from_secs(60));
    let text = "a".repeat(200_000);
    let mut vim = HeadlessVim::new(&text, (0, 199_999));
    vim.feed("?").unwrap();
    let started = Instant::now();
    let found = vim.state().search_preview(vim.model(), r"a.*z\|a");
    let took = started.elapsed();
    set_time_limit(DEFAULT_TIME_LIMIT);
    assert_eq!(found, None);
    assert!(took < Duration::from_secs(5), "{took:?}");
    // On a short row the same pattern previews.
    let mut vim = HeadlessVim::new("aaaz", (0, 3));
    vim.feed("?").unwrap();
    assert_eq!(
        vim.state().search_preview(vim.model(), r"a.*z\|a"),
        Some(0..4)
    );
}
