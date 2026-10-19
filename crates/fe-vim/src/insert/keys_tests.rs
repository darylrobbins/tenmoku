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
