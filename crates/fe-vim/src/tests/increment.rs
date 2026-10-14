//! Tests of the increment feature.
use fe_buffer::ResolvedSelection;
use fe_command::{CommandId, Invocation};

use crate::{
    HeadlessVim, VimState, VisualKind, context,
    increment::{Extent, IncrementArgs},
};

/// `keys` over `text` from `cursor`: the text and the caret.
fn run(text: &str, cursor: (u32, usize), keys: &str) -> (String, (u32, usize)) {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.feed(keys).unwrap();
    (vim.text(), vim.cursor())
}

#[test]
fn numbers() {
    assert_eq!(run("x 9 y", (0, 0), "<C-a>"), ("x 10 y".into(), (0, 3)));
    assert_eq!(run("-3", (0, 0), "5<C-x>"), ("-8".into(), (0, 1)));
    assert_eq!(run("0xff", (0, 0), "<C-a>").0, "0x100");
    assert_eq!(run("0xFE", (0, 0), "<C-a>").0, "0xFF");
    assert_eq!(run("0b101", (0, 0), "<C-a>").0, "0b110");
    assert_eq!(run("007", (0, 0), "<C-a>").0, "008");
    assert_eq!(run("a 1 2", (0, 4), "<C-x>").0, "a 1 1");
    assert_eq!(run("x 9 y", (0, 0), "<C-a>.").0, "x 11 y");
    assert_eq!(run("1", (0, 0), "2<C-a>.").0, "5");

    // 64 bits: the count is capped as nvim's is, a wrap keeps the sign.
    let max = "18446744073709551615";
    assert_eq!(run(max, (0, 0), "<C-x>").0, "18446744073709551614");
    assert_eq!(run(max, (0, 0), "<C-a>").0, format!("-{max}"));
    assert_eq!(
        run(max, (0, 0), "4294967295<C-a>").0,
        "-18446744072709551617"
    );

    // No number: nothing changes, and no undo step is left.
    let mut vim = HeadlessVim::new("5\nx", (0, 0));
    vim.feed("<C-a>j<C-a>u").unwrap();
    assert_eq!(vim.text(), "5\nx");
    vim.feed("<C-r>").unwrap();
    assert_eq!(vim.text(), "6\nx");

    // Visual: the first number on each row, and progressively with `g`.
    assert_eq!(run("1\n2\n3", (0, 0), "Vjj<C-a>").0, "2\n3\n4");
    assert_eq!(run("1\n1\n1", (0, 0), "Vjjg<C-a>").0, "2\n3\n4");
    assert_eq!(run("5\n5\n5", (0, 0), "Vjj2g<C-x>").0, "3\n1\n-1");
    let mut vim = HeadlessVim::new("1\n1", (0, 0));
    vim.feed("Vj<C-a>").unwrap();
    assert_eq!(vim.mode(), "normal");
}

/// A buffer with carets at `offsets`.
fn with_carets(text: &str, offsets: &[usize]) -> HeadlessVim {
    let mut vim = HeadlessVim::new(text, (0, 0));
    vim.model_mut()
        .set_selections(offsets.iter().map(|&offset| ResolvedSelection {
            range: offset..offset,
            reversed: false,
        }));
    vim
}

fn heads(vim: &HeadlessVim) -> Vec<usize> {
    vim.model().selections().iter().map(|s| s.head()).collect()
}

#[test]
fn each_caret_changes_its_own_number() {
    let mut vim = with_carets("1 2", &[0, 2]);
    vim.feed("<C-a>").unwrap();
    assert_eq!(vim.text(), "2 3");
    assert_eq!(heads(&vim), [0, 2]);

    // Two carets in one number change it once, and share its landing.
    let mut vim = with_carets("1 20", &[2, 3]);
    vim.feed("<C-a>").unwrap();
    assert_eq!(vim.text(), "1 21");
    assert_eq!(heads(&vim), [3]);

    // One before a number, one on a row without.
    let mut vim = with_carets("a 9 b\nx", &[0, 6]);
    vim.feed("<C-a>").unwrap();
    assert_eq!(vim.text(), "a 10 b\nx");
    assert_eq!(heads(&vim), [3, 7]);
}

fn replay(args: IncrementArgs) -> Invocation {
    let command = CommandId::parse("vim.increment").unwrap();
    Invocation::new(command)
        .args(serde_json::to_value(args).unwrap())
        .context(context(&VimState::new()))
}

#[test]
fn replay_extents_are_checked() {
    for (kind, rows, cols) in [
        (VisualKind::Line, 0, None),
        (VisualKind::Char, 0, Some(1)),
        (VisualKind::Char, 1, Some(0)),
        (VisualKind::Block, 1, Some(0)),
    ] {
        let mut vim = HeadlessVim::new("1\n1", (0, 0));
        let args = IncrementArgs {
            delta: 1,
            count: None,
            progressive: false,
            replay: Some(Extent { kind, rows, cols }),
        };
        let invocation = replay(args);
        assert!(
            vim.dispatch(&invocation).is_err(),
            "{kind:?} {rows} {cols:?}"
        );
        assert_eq!(vim.text(), "1\n1");
    }

    // Huge ones are cut to the text.
    for kind in [VisualKind::Line, VisualKind::Char, VisualKind::Block] {
        let mut vim = HeadlessVim::new("1\n1", (0, 0));
        let args = IncrementArgs {
            delta: 1,
            count: None,
            progressive: false,
            replay: Some(Extent {
                kind,
                rows: u32::MAX,
                cols: Some(usize::MAX),
            }),
        };
        let invocation = replay(args);
        vim.dispatch(&invocation).unwrap();
        assert_eq!(vim.text(), "2\n2");
    }
}

/// CPU time this thread has used. Unlike wall clock it doesn't grow while the thread is
/// descheduled, which is what heavy machine load does to a timing: the 10x longer run is
/// descheduled far more often than the short one, so a wall-clock ratio drifts past any bound.
fn thread_cpu_time() -> std::time::Duration {
    #[repr(C)]
    struct Timespec {
        tv_sec: i64,
        tv_nsec: i64,
    }
    unsafe extern "C" {
        fn clock_gettime(clock: i32, out: *mut Timespec) -> i32;
    }
    #[cfg(target_os = "macos")]
    const CLOCK_THREAD_CPUTIME_ID: i32 = 16;
    #[cfg(not(target_os = "macos"))]
    const CLOCK_THREAD_CPUTIME_ID: i32 = 3;
    let mut now = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `now` is a valid, writable timespec for the call to fill in.
    let status = unsafe { clock_gettime(CLOCK_THREAD_CPUTIME_ID, &mut now) };
    assert_eq!(status, 0, "clock_gettime failed");
    std::time::Duration::new(now.tv_sec as u64, now.tv_nsec as u32)
}

#[test]
fn a_block_over_a_long_row_is_quick() {
    // Compares the fastest CPU-time sample of a 10x longer row, so load drops out:
    // the ratio is ~10 for linear work and ~100 for quadratic. Short and long runs
    // alternate so they see the same cache state.
    fn run(len: usize) -> std::time::Duration {
        let row = format!("{}7", "x".repeat(len));
        let text = format!("{row}\n{row}");
        let mut vim = HeadlessVim::new(&text, (0, 0));
        let started = thread_cpu_time();
        vim.feed("<C-v>j<C-a>").unwrap();
        let elapsed = thread_cpu_time() - started;
        assert_eq!(vim.text(), text);
        elapsed
    }
    let mut short = std::time::Duration::MAX;
    let mut long = std::time::Duration::MAX;
    for _ in 0..6 {
        short = short.min(run(10_000));
        long = long.min(run(100_000));
    }
    assert!(long < short * 50, "10x the row took {long:?} vs {short:?}");
}
