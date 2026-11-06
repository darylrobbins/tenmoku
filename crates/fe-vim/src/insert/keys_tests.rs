//! A count's copies of typing that held indent keys stop at the bytes a put
//! may insert ([`MAX_PUT`]), counting the indentation the keys write and
//! the rows `o` opens with it, as well as the text typed.

use std::time::{Duration, Instant};

use crate::{HeadlessVim, operator::MAX_PUT};

/// `keys` fed to `text` with the caret at its start: the text they leave,
/// and how long they took.
fn fed(text: &str, keys: &str) -> (String, Duration) {
    let mut vim = HeadlessVim::new(text, (0, 0));
    let started = Instant::now();
    vim.feed(keys).unwrap();
    (vim.text(), started.elapsed())
}

#[test]
fn opened_rows_copying_a_growing_indent_stop_at_the_budget() {
    // Each row opened copies the one before's indent, a shift width more:
    // ten thousand would be some 200 MB.
    let (text, took) = fed("foo", "10000o<C-t>x<Esc>");
    assert!(took < Duration::from_secs(60), "{took:?}");
    assert!(text.len() <= "foo".len() + MAX_PUT, "{}", text.len());
    let rows = text.split('\n').count();
    assert!((1000..10_001).contains(&rows), "{rows} rows");
    // Each row is a shift width further in than the one before.
    let indents: Vec<usize> = text
        .split('\n')
        .skip(1)
        .map(|row| row.len() - row.trim_start().len())
        .collect();
    assert!(indents.iter().enumerate().all(|(ix, &n)| n == 4 * (ix + 1)));
}

#[test]
fn rows_opened_above_stop_at_the_budget() {
    let (text, took) = fed("foo", "10000O<C-t>x<Esc>");
    assert!(took < Duration::from_secs(60), "{took:?}");
    assert!(text.len() <= "foo".len() + MAX_PUT, "{}", text.len());
    assert!(text.split('\n').count() < 10_001);
}

#[test]
fn replace_copies_count_the_indent_their_keys_write() {
    // The row's indent grows a shift width a copy, and each copy writes
    // all of it again: a hundred thousand would be some 20 GB.
    let (text, _) = fed("foo", "100000R<C-t>x<Esc>");
    let indent = text.len() - text.trim_start().len();
    let copies = indent / 4;
    assert!((1000..100_000).contains(&copies), "{copies} copies");
    // What they wrote, all of each copy's indent, fits.
    assert!(4 * copies * (copies + 1) / 2 <= MAX_PUT);
    assert_eq!(
        text.trim_start(),
        "x".repeat(copies) + &"foo"[copies.min(3)..]
    );
}

#[test]
fn a_small_count_makes_every_copy() {
    assert_eq!(
        fed("foo", "3o<C-t>x<Esc>").0,
        "foo\n    x\n        x\n            x"
    );
    assert_eq!(fed("foo bar", "3R<C-t>x<Esc>").0, "            xxx bar");
}

/// `keys` fed to `text` with a caret at each of `carets` (byte offsets):
/// the text they leave and the carets' heads.
fn fed_at(text: &str, carets: &[usize], keys: &str) -> (String, Vec<usize>) {
    let mut vim = HeadlessVim::new(text, (0, 0));
    let carets: Vec<_> = carets
        .iter()
        .map(|&at| crate::operator::caret(at))
        .collect();
    vim.model_mut().set_selections(carets);
    vim.feed(keys).unwrap();
    let heads = vim
        .model_mut()
        .selections()
        .iter()
        .map(|s| s.head())
        .collect();
    (vim.text(), heads)
}

#[test]
fn counted_copies_follow_carets_that_deletion_brings_together() {
    // Each copy's `<C-w>`s take the word before both carets, then the
    // carets meet: the copies go on at the one caret left.
    let (text, heads) = fed_at("a b c d", &[0, 6], "3a<C-w><C-w>x<Esc>");
    assert_eq!(text, "x");
    assert_eq!(heads, [0]);
}

#[test]
fn replace_mode_ctrl_w_goes_back_at_each_caret_on_its_own() {
    // Words of one and four characters before the carets: each goes back
    // over its own.
    let (text, _) = fed_at("abc a x\nlong y", &[5, 12], "R<C-w>Q<Esc>");
    assert_eq!(text, "abc Q x\nQong y");
    // A character typed past the row's end goes, one typed over another
    // puts that back, and then each caret only moves, as far as its word
    // (the blank before it first).
    let (text, heads) = fed_at("abc a\nlong y", &[4, 10], "Rxy<C-w>");
    assert_eq!((text.as_str(), heads), ("abc a\nlong y", vec![4, 10]));
    let (text, _) = fed_at("abc a\nlong y", &[4, 10], "Rxy<C-w><C-w>Q<Esc>");
    assert_eq!(text, "Qbc a\nQong y");
    // With nothing typed, one caret goes back over its word while the
    // other, at its row's start, goes to the end of the row above.
    let (text, heads) = fed_at("ab cd\nef gh", &[3, 6], "R<C-w>");
    assert_eq!((text.as_str(), heads), ("ab cd\nef gh", vec![0, 5]));
    // Each puts back what it typed over first, then moves.
    let (text, heads) = fed_at("ab cd\nef gh", &[3, 6], "Rx<C-w><C-w>");
    assert_eq!((text.as_str(), heads), ("ab cd\nef gh", vec![0, 5]));
}

#[test]
fn replace_copies_follow_carets_that_going_back_brings_together() {
    // The first copy's `<C-w>`s bring both carets to the text's start:
    // the copies go on at the one caret left.
    let (text, heads) = fed_at("a b c d", &[0, 6], "3R<C-w><C-w>x<Esc>");
    assert_eq!((text.as_str(), heads), ("x x c d", vec![0]));
}

/// A register recorded as a multiline plain insertion in replace mode, then
/// replaced by one-line text past the budget: replaying it must be refused
/// as inserting it is, not overwrite character by character.
fn oversized_register_replay(keys: &str) {
    let big = "Q".repeat(MAX_PUT + 1);
    let text = format!("foo\nbar\nbaz\n{big}");
    let mut vim = HeadlessVim::new(&text, (0, 0));
    vim.feed("vjy").unwrap();
    vim.feed("R<C-r>0<Esc>").unwrap();
    let recorded = vim.text();
    vim.feed("Gyiwgg2j").unwrap();
    let started = Instant::now();
    vim.feed(keys).unwrap();
    assert!(started.elapsed() < Duration::from_secs(60));
    assert_eq!(vim.text(), recorded, "{keys}");
}

#[test]
fn dot_replay_of_an_oversized_register_is_refused() {
    oversized_register_replay(".");
}

#[test]
fn counted_replay_of_an_oversized_register_is_refused() {
    oversized_register_replay("3.");
}
