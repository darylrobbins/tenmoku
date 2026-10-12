//! The time limit ([`fe_vim::search::set_time_limit`]) on a counted search
//! with an `e` offset, which steps from match to match row by row: one
//! deadline covers every row it lists, and a row cut short isn't taken as
//! listed. The limit is process-wide, so each test sets its own and the
//! tests run one at a time.

use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use fe_vim::HeadlessVim;
use fe_vim::search::{DEFAULT_TIME_LIMIT, set_time_limit};

static LIMIT: Mutex<()> = Mutex::new(());

struct Limited(#[allow(dead_code)] MutexGuard<'static, ()>);

impl Drop for Limited {
    fn drop(&mut self) {
        set_time_limit(DEFAULT_TIME_LIMIT);
    }
}

fn limited(limit: Duration) -> Limited {
    let guard = LIMIT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    set_time_limit(limit);
    Limited(guard)
}

/// Each match of `a` scans to the row's end for a `z`: listing a row's
/// matches is quadratic in its length.
const SLOW: &str = r"a.*z\|a";

/// A row too slow to list within the limit leaves the cursor where it was,
/// rather than landing on a match from the part listed.
#[test]
fn a_row_cut_short_is_not_taken_as_listed() {
    let _limit = limited(Duration::from_millis(100));
    let text = "a".repeat(200_000);
    let mut vim = HeadlessVim::new(&text, (0, 0));
    let started = Instant::now();
    vim.feed(&format!("150000/{SLOW}/e<CR>")).unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(vim.cursor(), (0, 0));
}

/// Rows each quick to list but slow together stop at the limit too.
#[test]
fn the_limit_covers_every_row_listed() {
    let _limit = limited(Duration::from_millis(100));
    let row = "a".repeat(3_000);
    let text = vec![row.as_str(); 1_000].join("\n");
    let mut vim = HeadlessVim::new(&text, (0, 0));
    let started = Instant::now();
    vim.feed(&format!("5000000/{SLOW}/e<CR>")).unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(vim.cursor(), (0, 0));
}
