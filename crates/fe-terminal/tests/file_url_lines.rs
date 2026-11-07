use std::path::PathBuf;

use fe_terminal::{
    Cell, CellFlags, Color, Cursor, CursorShape, LinkTarget, Modes, Snapshot, link_at,
};

fn snapshot(text: &str) -> Snapshot {
    let row: Vec<Cell> = text
        .chars()
        .map(|ch| Cell {
            ch,
            extra: None,
            fg: Color::Foreground,
            bg: Color::Background,
            flags: CellFlags::empty(),
            link: None,
        })
        .collect();
    Snapshot {
        cols: row.len(),
        rows: vec![row],
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

#[test]
fn file_urls_take_a_line_and_column_suffix() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("index.mjs");
    std::fs::write(&file, "x\n").unwrap();
    // As Node prints it: `file:///C:/…` on Windows.
    #[cfg(unix)]
    let url = format!("file://{}", file.display());
    #[cfg(windows)]
    let url = format!("file:///{}", file.display().to_string().replace('\\', "/"));
    let prefix = "    at main (".chars().count();
    for (suffix, line, column) in [
        (":3:9", Some(3), Some(9)),
        (":3", Some(3), None),
        ("", None, None),
    ] {
        let url = format!("{url}{suffix}");
        let snap = snapshot(&format!("    at main ({url})"));
        let link = link_at(&snap, 0, prefix + 2, dir.path()).expect("link");
        assert_eq!(link.cols, prefix..prefix + url.chars().count(), "{suffix}");
        assert_eq!(
            link.target,
            LinkTarget::File {
                path: PathBuf::from(&file),
                line,
                column
            },
            "{suffix}"
        );
    }
}
