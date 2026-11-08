//! Follow-ups to T106's review: the disk copy of the request's file leaves
//! the cap to the other files, the scan of the request's text checks for a
//! cancel however long its word, and the search never reads through a
//! directory swapped for a symlink.

#[cfg(unix)]
use std::sync::atomic::Ordering;
use std::{cell::Cell, fs, path::PathBuf, sync::atomic::AtomicBool, time::Instant};

use fe_buffer::Text;

use super::{
    tests::{CANCEL_AT, CANCEL_CHECKS},
    *,
};

thread_local! {
    /// How many occurrences of its word a [`Finder`]'s scan of the
    /// request's text has rejected: not whole, or overlapping a hit.
    pub(super) static REJECTED: Cell<usize> = const { Cell::new(0) };
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// A request from `rel` under `root`, its text `text`, with the caret at
/// its start.
fn request(root: &Path, rel: &str, language: Option<&'static str>, text: &str) -> NavRequest {
    NavRequest {
        root: root.to_path_buf(),
        rel: PathBuf::from(rel),
        language,
        text: Text::new(text),
        offset: 0,
        buffer: None,
    }
}

/// The references `find` reports, and its summary.
fn references(
    root: &Path,
    verify_files: usize,
    req: &NavRequest,
) -> (Vec<Reference>, ReferencesSummary) {
    let mut refs = Vec::new();
    let summary = find(
        root,
        &ReferenceOptions { verify_files },
        req,
        &AtomicBool::new(false),
        &mut |batch| refs.extend(batch.into_iter().flat_map(|file| file.refs)),
    )
    .unwrap();
    (refs, summary)
}

fn finder<'a>(word: &str, cancel: &'a AtomicBool) -> Finder<'a> {
    Finder {
        word: word.to_owned(),
        cancel,
        verify_left: 1_000,
        hits_used: 0,
        delivered: false,
        summary: ReferencesSummary::default(),
    }
}

/// The request's file on disk has far more hits than the cap, and one in
/// its text: the other files' hits are all reported up to the cap, and
/// the result is truncated only when one of theirs is left out.
#[test]
fn a_large_disk_copy_leaves_the_cap_to_the_other_files() {
    let count = |refs: &[Reference], rel: &str| {
        (refs.iter())
            .filter(|r| r.location.rel == Path::new(rel))
            .count()
    };
    // Without other files, then with a few, then with as many as fit
    // beside the request's one hit, then with one more.
    for others in [0, 5, MAX_HITS - 1, MAX_HITS] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "req.txt", &"helper\n".repeat(30_000));
        let (one, two) = (others / 2, others - others / 2);
        for (rel, hits) in [("one.txt", one), ("two.txt", two)] {
            if hits > 0 {
                write(root, rel, &"helper\n".repeat(hits));
            }
        }
        let req = request(root, "req.txt", None, "helper\n");
        let (refs, summary) = references(root, 0, &req);
        assert_eq!(count(&refs, "req.txt"), 1, "{others}");
        let reported = count(&refs, "one.txt") + count(&refs, "two.txt");
        if others < MAX_HITS {
            assert_eq!(reported, others);
            assert!(!summary.truncated, "{others}: nothing was left out");
        } else {
            assert_eq!(reported, MAX_HITS - 1);
            assert!(summary.truncated);
        }
    }
}

/// A selected word longer than many chunks, whole twice: the scan checks
/// for a cancel at least once a chunk of the text, matching or counting
/// rows, and stops at the check that sees one.
#[test]
fn a_long_selected_word_is_scanned_with_checks_for_a_cancel() {
    let word = "x".repeat(8 * SCAN_CHUNK);
    let text = format!("{word} {word}\n");
    let req = request(Path::new("/nonexistent"), "a.txt", None, &text);
    let cancel = AtomicBool::new(false);
    CANCEL_CHECKS.set(0);
    let (found, hits) = finder(&word, &cancel).own_file(&req);
    assert_eq!((found.map(|file| file.refs.len()), hits), (Some(2), 2));
    let checks = CANCEL_CHECKS.get();
    assert!(checks >= text.len() / SCAN_CHUNK, "{checks} checks");
    // Cancelled while it builds its matcher, in the first match, and in
    // the second.
    for at in [2, 12, 30] {
        let cancel = AtomicBool::new(false);
        CANCEL_CHECKS.set(0);
        CANCEL_AT.set(at);
        let (found, _) = finder(&word, &cancel).own_file(&req);
        CANCEL_AT.set(0);
        assert!(found.is_none(), "{at}");
        assert_eq!(CANCEL_CHECKS.get(), at, "no more work once cancelled");
    }
}

/// A long word, then a run of its char two chunks longer than it, after
/// a word char: every candidate in the run overlaps the one before it and
/// isn't whole. A cancel while they're rejected stops the scan promptly,
/// rather than after the candidates of a whole chunk, each as long as the
/// word, are looked at.
#[test]
fn a_cancel_among_overlapping_rejected_candidates_is_prompt() {
    let word = "x".repeat(SCAN_CHUNK);
    let run = word.len() + 2 * SCAN_CHUNK;
    let text = format!("{word} y{}\n", "x".repeat(run));
    let req = request(Path::new("/nonexistent"), "a.txt", None, &text);

    let candidates = run - word.len() + 1;

    // Cancelled at each check in turn, up to the first one that comes
    // while the run's candidates are being rejected.
    let mut reached = false;
    for at in 1..=64 {
        let cancel = AtomicBool::new(false);
        CANCEL_CHECKS.set(0);
        CANCEL_AT.set(at);
        REJECTED.set(0);
        let started = Instant::now();
        let (found, _) = finder(&word, &cancel).own_file(&req);
        let took = started.elapsed();
        CANCEL_AT.set(0);
        assert!(found.is_none(), "{at}");
        assert_eq!(CANCEL_CHECKS.get(), at, "no more work once cancelled");
        assert!(took < Duration::from_secs(1), "{at}: {took:?}");
        let rejected = REJECTED.get();
        if rejected > 0 {
            assert!(rejected < candidates, "{at}: cancelled after the run");
            reached = true;
            break;
        }
    }
    assert!(reached, "no check while candidates were rejected");

    // Without a cancel: only the word itself, every candidate in the run
    // rejected, and a check at least once a chunk.
    let cancel = AtomicBool::new(false);
    CANCEL_CHECKS.set(0);
    REJECTED.set(0);
    let (found, hits) = finder(&word, &cancel).own_file(&req);
    assert_eq!((found.map(|file| file.refs.len()), hits), (Some(1), 1));
    assert_eq!(REJECTED.get(), candidates);
    let checks = CANCEL_CHECKS.get();
    assert!(checks >= text.len() / SCAN_CHUNK, "{checks} checks");
}

/// A directory swapped back and forth with a symlink to an outside tree
/// while references are found, with and without verifying: no hit or
/// preview comes from the outside files, whose lines say so. The directory
/// holds many files, so swaps land while the search reads them.
#[test]
// Swaps a folder for a symlink, which Windows makes only with a privilege.
#[cfg(unix)]
fn no_preview_is_read_through_a_directory_being_swapped() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dunce::canonicalize(dir.path()).unwrap();
    let (root, outside) = (dir.join("project"), dir.join("outside"));
    // In code inside, in a comment outside: a verified hit read from
    // outside would be dropped.
    for ix in 0..100 {
        write(&root, &format!("src/a/x{ix}.rs"), "fn f() { helper(); }\n");
        write(
            &outside,
            &format!("x{ix}.rs"),
            "// f() { helper(); } outside\n",
        );
    }
    let req_text = "fn helper() {}\n";
    write(&root, "req.rs", req_text);
    let (a, aside) = (root.join("src/a"), root.join("src/a.aside"));
    let req = NavRequest {
        offset: req_text.find("helper").unwrap(),
        buffer: None,
        ..request(&root, "req.rs", Some("rust"), req_text)
    };
    let done = AtomicBool::new(false);
    thread::scope(|scope| {
        let _stop = SetOnDrop(&done);
        scope.spawn(|| {
            // Each state held a while, so the search reads through both.
            let hold = Duration::from_micros(300);
            while !done.load(Ordering::Relaxed) {
                fs::rename(&a, &aside).unwrap();
                std::os::unix::fs::symlink(&outside, &a).unwrap();
                thread::sleep(hold);
                fs::remove_file(&a).unwrap();
                fs::rename(&aside, &a).unwrap();
                thread::sleep(hold);
            }
        });
        for verify_files in [0, 1_000] {
            let mut inside = 0;
            for _ in 0..20 {
                let (refs, summary) = references(&root, verify_files, &req);
                assert_eq!(summary.word.as_deref(), Some("helper"));
                assert_eq!(summary.dropped, 0, "read from outside");
                for r in &refs {
                    assert!(!r.preview.contains("outside"), "{r:?}");
                }
                // Under `src/a` or `src/a.aside`.
                inside += (refs.iter())
                    .filter(|r| r.location.rel != Path::new("req.rs"))
                    .count();
            }
            assert!(inside > 0, "never caught the directory in place");
        }
    });
}

#[path = "references_preview_tests.rs"]
mod preview_tests;
