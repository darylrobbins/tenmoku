//! Finding text in a file bigger than a few find blocks.

use std::fs::{self, OpenOptions};
use std::sync::atomic::AtomicBool;

use fe_fileview::{
    Direction::{self, Backward, Forward},
    FIND_BLOCK, FileView, FindOutcome, FindQuery, ReadError, find,
};

const BLOCK: usize = FIND_BLOCK as usize;

fn query(needle: &str, case_sensitive: bool) -> FindQuery {
    FindQuery {
        needle: needle.into(),
        case_sensitive,
    }
}

/// A file of `len` dots with `text` written at each offset.
fn file_with(len: usize, marks: &[(usize, &str)]) -> (tempfile::TempDir, FileView) {
    let mut bytes = vec![b'.'; len];
    for &(at, text) in marks {
        bytes[at..at + text.len()].copy_from_slice(text.as_bytes());
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.log");
    fs::write(&path, bytes).unwrap();
    (dir, FileView::open(&path).unwrap())
}

fn run(
    view: &FileView,
    q: &FindQuery,
    within: std::ops::Range<u64>,
    from: usize,
    direction: Direction,
) -> FindOutcome {
    find(
        view,
        q,
        within,
        from as u64,
        direction,
        &AtomicBool::new(false),
    )
    .unwrap()
}

fn found(start: usize, len: usize, wrapped: bool) -> FindOutcome {
    FindOutcome::Found {
        range: start as u64..(start + len) as u64,
        wrapped,
    }
}

#[test]
fn forward_and_backward() {
    let len = 3 * BLOCK + 1000;
    // One match straddles the first block edge, one the second.
    let edge1 = BLOCK - 2;
    let edge2 = 2 * BLOCK - 3;
    let marks = [
        (100, "needle"),
        (edge1, "needle"),
        (edge2, "needle"),
        (len - 50, "needle"),
    ];
    let (_dir, view) = file_with(len, &marks);
    let q = query("needle", true);
    let all = 0..len as u64;

    assert_eq!(
        run(&view, &q, all.clone(), 0, Forward),
        found(100, 6, false)
    );
    assert_eq!(
        run(&view, &q, all.clone(), 101, Forward),
        found(edge1, 6, false)
    );
    assert_eq!(
        run(&view, &q, all.clone(), edge1 + 1, Forward),
        found(edge2, 6, false)
    );
    assert_eq!(
        run(&view, &q, all.clone(), edge2 + 1, Forward),
        found(len - 50, 6, false)
    );

    assert_eq!(
        run(&view, &q, all.clone(), len, Backward),
        found(len - 50, 6, false)
    );
    assert_eq!(
        run(&view, &q, all.clone(), len - 50 + 5, Backward),
        found(edge2, 6, false)
    );
    assert_eq!(
        run(&view, &q, all.clone(), edge2 + 6, Backward),
        found(edge2, 6, false)
    );
    assert_eq!(
        run(&view, &q, all.clone(), edge2 + 5, Backward),
        found(edge1, 6, false)
    );
    assert_eq!(
        run(&view, &q, all.clone(), edge1, Backward),
        found(100, 6, false)
    );

    // Matches outside `within` are ignored, and a match must lie wholly inside.
    let within = 101..(len - 50 + 5) as u64;
    assert_eq!(
        run(&view, &q, within.clone(), 0, Forward),
        found(edge1, 6, false)
    );
    assert_eq!(
        run(&view, &q, within, len, Backward),
        found(edge2, 6, false)
    );
}

#[test]
fn wrap_case_and_empty() {
    let len = 2 * BLOCK + 10;
    let (_dir, view) = file_with(len, &[(500, "Foo"), (len - 600, "foo"), (len - 9, "ÀB")]);
    let all = 0..len as u64;
    let q = query("foo", true);

    assert_eq!(
        run(&view, &q, all.clone(), 0, Forward),
        found(len - 600, 3, false)
    );
    assert_eq!(
        run(&view, &q, all.clone(), len - 500, Forward),
        found(len - 600, 3, true)
    );
    assert_eq!(
        run(&view, &q, all.clone(), 0, Backward),
        found(len - 600, 3, true)
    );
    assert_eq!(
        run(&view, &q, all.clone(), len - 590, Backward),
        found(len - 600, 3, false)
    );

    // Only the match covering `from` exists inside `within`.
    let within = 500..503;
    assert_eq!(
        run(&view, &query("Foo", true), within.clone(), 501, Forward),
        found(500, 3, true)
    );
    assert_eq!(
        run(&view, &query("Foo", true), within, 501, Backward),
        found(500, 3, true)
    );

    // Case-insensitive folds ASCII only.
    let loose = query("FOO", false);
    assert_eq!(
        run(&view, &loose, all.clone(), 0, Forward),
        found(500, 3, false)
    );
    assert_eq!(
        run(&view, &query("àb", false), all.clone(), 0, Forward),
        FindOutcome::NotFound
    );
    assert_eq!(
        run(&view, &query("Àb", false), all.clone(), 0, Forward),
        found(len - 9, 3, false)
    );
    assert_eq!(
        run(&view, &query("FOO", true), all.clone(), 0, Forward),
        FindOutcome::NotFound
    );

    for needle in ["", "fo\no", "fo\ro", "\n"] {
        assert_eq!(
            run(&view, &query(needle, true), all.clone(), 0, Forward),
            FindOutcome::NotFound
        );
    }
    // Longer than `within`.
    assert_eq!(
        run(&view, &q, 500..502, 500, Forward),
        FindOutcome::NotFound
    );
    assert_eq!(run(&view, &q, 0..0, 0, Backward), FindOutcome::NotFound);
    // `within` is clamped to the file.
    assert_eq!(
        run(&view, &q, 0..u64::MAX, 0, Forward),
        found(len - 600, 3, false)
    );
}

#[test]
fn stops_and_leaves_the_cache() {
    let len = 3 * BLOCK;
    let (dir, view) = file_with(len, &[(len - 10, "needle")]);
    let q = query("needle", true);
    let stop = AtomicBool::new(true);
    assert_eq!(
        find(&view, &q, 0..len as u64, 0, Forward, &stop).unwrap(),
        FindOutcome::Stopped
    );

    let fresh = FileView::open(dir.path().join("big.log")).unwrap();
    assert_eq!(
        run(&fresh, &q, 0..len as u64, 0, Forward),
        found(len - 10, 6, false)
    );
    assert_eq!(
        run(&fresh, &q, 0..len as u64, len, Backward),
        found(len - 10, 6, false)
    );
    assert_eq!(
        run(&fresh, &query("zzz", true), 0..len as u64, 0, Forward),
        FindOutcome::NotFound
    );
    assert_eq!(fresh.cached_bytes(), 0);

    OpenOptions::new()
        .write(true)
        .open(dir.path().join("big.log"))
        .unwrap()
        .set_len(BLOCK as u64)
        .unwrap();
    let result = find(
        &fresh,
        &q,
        0..len as u64,
        0,
        Forward,
        &AtomicBool::new(false),
    );
    assert!(matches!(result, Err(ReadError::Changed)), "{result:?}");
}
