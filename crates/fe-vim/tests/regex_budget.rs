//! The time limit on `:s` and searches ([`fe_vim::search::set_time_limit`]):
//! an adversarial pattern on a long row stops with `Interrupted`, and
//! linear work doesn't. The limit is process-wide, so each test sets its own
//! and the tests run one at a time.

use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use fe_vim::HeadlessVim;
use fe_vim::search::{DEFAULT_TIME_LIMIT, set_time_limit};

static LIMIT: Mutex<()> = Mutex::new(());

fn limited(limit: Duration) -> MutexGuard<'static, ()> {
    let guard = LIMIT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    set_time_limit(limit);
    guard
}

/// The adversarial pattern: each match of `a` scans to the row's end for a `z`.
const SLOW: &str = r"a.*z\|a";

#[test]
fn substitute_stops_at_the_limit() {
    let _guard = limited(Duration::from_millis(100));
    let text = "a".repeat(200_000);
    let mut vim = HeadlessVim::new(&text, (0, 0));
    let started = Instant::now();
    vim.feed(&format!(":s/{SLOW}/X/g<CR>")).unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(vim.text(), text);
    assert_eq!(vim.state().message(), Some("Interrupted"));
    assert!(
        !matches!(vim.model_mut().undo(), Ok(Some(_))),
        "an undo step"
    );
    assert_eq!(vim.text(), text);
}

#[test]
fn searches_stop_at_the_limit() {
    let _guard = limited(Duration::from_millis(100));
    let text = "a".repeat(200_000);
    let mut vim = HeadlessVim::new(&text, (0, 199_999));
    let started = Instant::now();
    vim.feed(&format!("?{SLOW}<CR>")).unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(vim.cursor(), (0, 199_999));
    assert_eq!(vim.state().message(), Some("Interrupted"));

    let mut vim = HeadlessVim::new(&text, (0, 150_000));
    let started = Instant::now();
    vim.feed(&format!("/{SLOW}<CR>")).unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(vim.cursor(), (0, 150_000));
    assert_eq!(vim.state().message(), Some("Interrupted"));

    let started = Instant::now();
    vim.feed("N").unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(vim.cursor(), (0, 150_000));
    assert_eq!(vim.state().message(), Some("Interrupted"));
}

#[test]
fn linear_work_is_not_interrupted() {
    let _guard = limited(DEFAULT_TIME_LIMIT);
    assert_eq!(DEFAULT_TIME_LIMIT, Duration::from_secs(2));
    let text = "a".repeat(20_000);
    let mut vim = HeadlessVim::new(&text, (0, 0));
    vim.feed(":s/a/X/g<CR>").unwrap();
    assert_eq!(vim.text(), "X".repeat(20_000));
    assert_eq!(vim.state().message(), Some("20000 substitutions on 1 line"));

    let mut vim = HeadlessVim::new("ab ab ab ab\nab", (0, 0));
    vim.feed("/a<CR>").unwrap();
    assert_eq!(vim.cursor(), (0, 3));
    vim.feed("?a<CR>").unwrap();
    assert_eq!(vim.cursor(), (0, 0));
    vim.feed("3n").unwrap();
    assert_eq!(vim.cursor(), (0, 6));
}

#[test]
fn the_limit_covers_the_whole_command_across_rows() {
    let _guard = limited(Duration::from_millis(100));
    let text = vec!["a".repeat(20_000); 200].join("\n");
    let mut vim = HeadlessVim::new(&text, (0, 0));
    let started = Instant::now();
    vim.feed(&format!(":%s/{SLOW}/X/g<CR>")).unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(vim.text(), text);
    assert_eq!(vim.state().message(), Some("Interrupted"));
}
