use std::path::Path;

use fe_terminal::{
    Cell, CellFlags, Color, Cursor, CursorShape, Link, LinkTarget, Modes, Snapshot, link_at,
};

/// A one-row snapshot of `text`, one cell per base character, each combining
/// mark (U+0300-U+036F) in the previous cell's `extra`.
fn snapshot(text: &str) -> Snapshot {
    let mut row: Vec<Cell> = Vec::new();
    for c in text.chars() {
        if ('\u{300}'..='\u{36f}').contains(&c) {
            row.last_mut()
                .unwrap()
                .extra
                .get_or_insert_with(String::new)
                .push(c);
            continue;
        }
        row.push(Cell {
            ch: c,
            extra: None,
            fg: Color::Foreground,
            bg: Color::Background,
            flags: CellFlags::empty(),
            link: None,
        });
    }
    let cols = row.len();
    Snapshot {
        rows: vec![row],
        cols,
        cursor: Cursor {
            row: 0,
            col: 0,
            shape: CursorShape::Hidden,
        },
        display_offset: 0,
        history_size: 0,
        selection: Vec::new(),
        modes: Modes::default(),
        links: Vec::new(),
    }
}

/// The link at every cell of `cols`, which must all be the same.
fn link_over(snapshot: &Snapshot, cols: std::ops::Range<usize>, cwd: &Path) -> Link {
    let first = link_at(snapshot, 0, cols.start, cwd).expect("link at first cell");
    assert_eq!(first.cols, cols);
    for col in cols {
        assert_eq!(
            link_at(snapshot, 0, col, cwd).as_ref(),
            Some(&first),
            "col {col}"
        );
    }
    first
}

#[test]
fn url_keeps_combining_marks() {
    let snapshot = snapshot("see https://x.test/cafe\u{301}/menu");
    let link = link_over(&snapshot, 4..28, Path::new("/"));
    assert_eq!(
        link.target,
        LinkTarget::Url("https://x.test/cafe\u{301}/menu".to_owned())
    );
}

#[test]
fn url_ending_in_combining_mark_keeps_it() {
    let snapshot = snapshot("https://x.test/e\u{301}");
    let cols = snapshot.cols;
    let link = link_over(&snapshot, 0..cols, Path::new("/"));
    assert_eq!(
        link.target,
        LinkTarget::Url("https://x.test/e\u{301}".to_owned())
    );
}

#[test]
fn path_with_combining_mark_links_to_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("cafe\u{301}.rs"), "").unwrap();
    let snapshot = snapshot("--> cafe\u{301}.rs:3:7 here");
    let link = link_over(&snapshot, 4..15, dir.path());
    assert_eq!(
        link.target,
        LinkTarget::File {
            path: dir.path().join("cafe\u{301}.rs"),
            line: Some(3),
            column: Some(7),
        }
    );
}
