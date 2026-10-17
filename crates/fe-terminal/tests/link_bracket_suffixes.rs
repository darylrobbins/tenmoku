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
fn bracketed_and_dotted_locations_link_to_their_line_and_column() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    let file = dir.path().join("src/main.rs");
    std::fs::write(&file, "x\n").unwrap();
    let prefix = "error at ".chars().count();
    let path_len = "src/main.rs".chars().count();
    let cases: [(&str, Option<u32>, Option<u32>, usize); 15] = [
        ("(12, 5)", Some(12), Some(5), 7),
        ("(12,5)", Some(12), Some(5), 6),
        ("(12:5)", Some(12), Some(5), 6),
        ("[12,5]", Some(12), Some(5), 6),
        ("[12, 5]", Some(12), Some(5), 7),
        ("[12:5]", Some(12), Some(5), 6),
        (":12.5", Some(12), Some(5), 5),
        (":12:5", Some(12), Some(5), 5),
        ("(12)", Some(12), None, 4),
        ("[12]", Some(12), None, 4),
        (":12.", Some(12), None, 3),
        ("(12,  5)", None, None, 0),
        ("[12", None, None, 0),
        ("[]", None, None, 0),
        ("(12]", None, None, 0),
    ];
    for (suffix, line, column, len) in cases {
        let snap = snapshot(&format!("error at src/main.rs{suffix} here"));
        let link = link_at(&snap, 0, prefix + 2, dir.path()).expect(suffix);
        assert_eq!(link.cols, prefix..prefix + path_len + len, "{suffix}");
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
