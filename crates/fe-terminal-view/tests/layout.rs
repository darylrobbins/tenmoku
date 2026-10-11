//! Which cells paint what, from hand-built snapshots.

use fe_terminal::{Cell, CellFlags, Color, Cursor, CursorShape, Modes, Snapshot, indexed_rgb};
use fe_terminal_view::TerminalStyle;
use fe_terminal_view::layout::{self, CursorLayout, Run};
use gpui::{Hsla, Rgba, px, rgb};

fn style() -> TerminalStyle {
    let background: Hsla = rgb(0x101010).into();
    TerminalStyle {
        font_family: "Menlo".into(),
        font_size: px(14.),
        line_height: TerminalStyle::LINE_HEIGHT,
        background,
        foreground: rgb(0xd0d0d0).into(),
        cursor: rgb(0xffcc00).into(),
        selection: rgb(0x334466).into(),
        ansi: TerminalStyle::ansi_for(background),
    }
}

fn cell(ch: char) -> Cell {
    Cell {
        ch,
        extra: None,
        fg: Color::Foreground,
        bg: Color::Background,
        flags: CellFlags::empty(),
    }
}

fn styled(ch: char, fg: Color, bg: Color, flags: CellFlags) -> Cell {
    Cell {
        ch,
        extra: None,
        fg,
        bg,
        flags,
    }
}

/// One row of `cells`, padded with blanks to `cols`, with the cursor at
/// (`row`, `col`) shaped `shape`.
fn snapshot(rows: Vec<Vec<Cell>>, cols: usize, cursor: (usize, usize, CursorShape)) -> Snapshot {
    let rows = rows
        .into_iter()
        .map(|mut row| {
            row.resize(cols, cell(' '));
            row
        })
        .collect();
    Snapshot {
        rows,
        cols,
        cursor: Cursor {
            row: cursor.0,
            col: cursor.1,
            shape: cursor.2,
        },
        display_offset: 0,
        history_size: 0,
        selection: Vec::new(),
        modes: Modes::default(),
    }
}

fn hsla(r: u8, g: u8, b: u8) -> Hsla {
    Rgba {
        r: f32::from(r) / 255.,
        g: f32::from(g) / 255.,
        b: f32::from(b) / 255.,
        a: 1.,
    }
    .into()
}

fn run(col: usize, cells: usize, text: &str, fg: Hsla) -> Run {
    Run {
        col,
        cells,
        text: text.to_owned(),
        fg,
        bold: false,
        italic: false,
        underline: false,
        strikeout: false,
    }
}

const HIDDEN: (usize, usize, CursorShape) = (0, 0, CursorShape::Hidden);

#[test]
fn colours() {
    let style = style();
    assert_eq!(layout::colour(Color::Foreground, &style), style.foreground);
    assert_eq!(layout::colour(Color::Background, &style), style.background);
    for i in 0..16 {
        assert_eq!(
            layout::colour(Color::Named(i), &style),
            style.ansi[usize::from(i)]
        );
        // 0–15 sent as palette indices are the style's too.
        assert_eq!(
            layout::colour(Color::Indexed(i), &style),
            style.ansi[usize::from(i)]
        );
    }
    for i in [16, 100, 196, 231, 232, 255] {
        let (r, g, b) = indexed_rgb(i);
        assert_eq!(layout::colour(Color::Indexed(i), &style), hsla(r, g, b));
    }
    assert_eq!(
        layout::colour(Color::Rgb(12, 200, 99), &style),
        hsla(12, 200, 99)
    );

    // Light backgrounds get the light ANSI colours.
    assert_eq!(
        TerminalStyle::ansi_for(rgb(0xffffff).into()),
        TerminalStyle::ansi_light()
    );
    assert_eq!(
        TerminalStyle::ansi_for(rgb(0x000000).into()),
        TerminalStyle::ansi_dark()
    );
    assert_ne!(TerminalStyle::ansi_light(), TerminalStyle::ansi_dark());

    let red = Color::Named(1);
    let blue = Color::Rgb(0, 0, 255);
    let row = vec![
        // Plain.
        styled('a', red, Color::Background, CellFlags::empty()),
        // Inverse of explicit colours.
        styled('b', red, blue, CellFlags::INVERSE),
        // Inverse of the defaults paints the foreground behind the background.
        styled(
            'c',
            Color::Foreground,
            Color::Background,
            CellFlags::INVERSE,
        ),
        // Dim.
        styled('d', red, Color::Background, CellFlags::DIM),
        // Hidden: its background still paints.
        styled('e', red, blue, CellFlags::HIDDEN),
        styled('f', Color::Indexed(196), Color::Background, CellFlags::BOLD),
    ];
    let rows = layout::rows(&snapshot(vec![row], 8, HIDDEN), &style, true);
    let mut dim = style.ansi[1];
    dim.a = 0.7;
    let (r, g, b) = indexed_rgb(196);
    let mut bold = run(5, 1, "f", hsla(r, g, b));
    bold.bold = true;
    assert_eq!(
        rows[0].runs,
        [
            run(0, 1, "a", style.ansi[1]),
            run(1, 1, "b", hsla(0, 0, 255)),
            run(2, 1, "c", style.background),
            run(3, 1, "d", dim),
            bold,
        ]
    );
    assert_eq!(
        rows[0].backgrounds,
        [
            (1..2, style.ansi[1]),
            (2..3, style.foreground),
            (4..5, hsla(0, 0, 255)),
        ]
    );
    assert!(rows[0].runs.iter().all(|run| !run.text.contains('e')));
}

#[test]
fn runs_and_backgrounds() {
    let style = style();
    let green = Color::Named(2);
    let grey = Color::Indexed(240);
    let mut wide = styled('漢', Color::Foreground, Color::Background, CellFlags::WIDE);
    wide.bg = grey;
    let mut spacer = styled(' ', Color::Foreground, grey, CellFlags::WIDE_SPACER);
    spacer.bg = grey;
    let mut accent = cell('e');
    accent.extra = Some("\u{301}".to_owned());
    let row = vec![
        // 0–4: "ls -l" in the default style, one run with its inner blank.
        cell('l'),
        cell('s'),
        cell(' '),
        cell('-'),
        cell('l'),
        // 5–6: blanks start nothing.
        cell(' '),
        cell(' '),
        // 7–9: green on grey.
        styled('o', green, grey, CellFlags::empty()),
        styled('k', green, grey, CellFlags::empty()),
        styled('!', green, grey, CellFlags::empty()),
        // 10–11: a wide char on the same grey, and its spacer.
        wide,
        spacer,
        // 12–13: default again, with a combining mark.
        cell('x'),
        accent,
        // 14: an underlined blank draws its underline.
        styled(
            ' ',
            Color::Foreground,
            Color::Background,
            CellFlags::UNDERLINE,
        ),
    ];
    let rows = layout::rows(&snapshot(vec![row, Vec::new()], 20, HIDDEN), &style, true);
    let grey = layout::colour(grey, &style);
    let green = style.ansi[2];
    let mut underlined = run(14, 1, " ", style.foreground);
    underlined.underline = true;
    assert_eq!(
        rows[0].runs,
        [
            run(0, 5, "ls -l", style.foreground),
            run(7, 3, "ok!", green),
            run(10, 2, "漢", style.foreground),
            run(12, 2, "xe\u{301}", style.foreground),
            underlined,
        ]
    );
    // The grey runs on under the wide char and its spacer.
    assert_eq!(rows[0].backgrounds, [(7..12, grey)]);
    assert_eq!(rows[1], Default::default());

    // A run breaks where the style does, and a wide char is a run of its own
    // even in the same style.
    let row = vec![
        cell('a'),
        styled('b', Color::Foreground, Color::Background, CellFlags::BOLD),
        styled('c', Color::Foreground, Color::Background, CellFlags::BOLD),
        styled('語', Color::Foreground, Color::Background, CellFlags::WIDE),
        styled(
            ' ',
            Color::Foreground,
            Color::Background,
            CellFlags::WIDE_SPACER,
        ),
        cell('d'),
        styled('e', Color::Named(4), Color::Background, CellFlags::empty()),
    ];
    let rows = layout::rows(&snapshot(vec![row], 8, HIDDEN), &style, true);
    let starts: Vec<_> = rows[0]
        .runs
        .iter()
        .map(|r| (r.col, r.cells, r.text.as_str()))
        .collect();
    assert_eq!(
        starts,
        [
            (0, 1, "a"),
            (1, 2, "bc"),
            (3, 2, "語"),
            (5, 1, "d"),
            (6, 1, "e")
        ]
    );
}

#[test]
fn cursor() {
    let style = style();
    let red = Color::Named(1);
    let row = vec![
        cell('a'),
        styled('b', red, Color::Rgb(0, 0, 255), CellFlags::empty()),
        cell('c'),
        styled('字', Color::Foreground, Color::Background, CellFlags::WIDE),
        styled(
            ' ',
            Color::Foreground,
            Color::Background,
            CellFlags::WIDE_SPACER,
        ),
    ];
    let at = |col, shape| snapshot(vec![row.clone()], 8, (0, col, shape));

    // Focused block: filled, the character drawn in the cell's background,
    // and left out of the row's runs.
    let block = at(0, CursorShape::Block);
    assert_eq!(
        layout::cursor(&block, &style, true),
        CursorLayout::Block {
            row: 0,
            col: 0,
            cells: 1,
            color: style.cursor,
            glyph: Some(run(0, 1, "a", style.background)),
        }
    );
    let texts = |snapshot: &Snapshot, focused| -> Vec<String> {
        layout::rows(snapshot, &style, focused)[0]
            .runs
            .iter()
            .map(|run| run.text.clone())
            .collect()
    };
    assert_eq!(texts(&block, true), ["b", "c", "字"]);
    assert_eq!(texts(&block, false), ["a", "b", "c", "字"]);
    // On a coloured cell the glyph takes that cell's background.
    match layout::cursor(&at(1, CursorShape::Block), &style, true) {
        CursorLayout::Block { glyph, .. } => {
            assert_eq!(glyph.unwrap().fg, hsla(0, 0, 255));
        }
        other => panic!("{other:?}"),
    }
    // On a wide char it's two cells; on a blank it draws no glyph.
    assert_eq!(
        layout::cursor(&at(3, CursorShape::Block), &style, true),
        CursorLayout::Block {
            row: 0,
            col: 3,
            cells: 2,
            color: style.cursor,
            glyph: Some(run(3, 2, "字", style.background)),
        }
    );
    assert_eq!(
        layout::cursor(&at(6, CursorShape::Block), &style, true),
        CursorLayout::Block {
            row: 0,
            col: 6,
            cells: 1,
            color: style.cursor,
            glyph: None,
        }
    );

    // Unfocused: a hollow block, whatever the shape.
    for shape in [
        CursorShape::Block,
        CursorShape::Beam,
        CursorShape::Underline,
    ] {
        assert_eq!(
            layout::cursor(&at(3, shape), &style, false),
            CursorLayout::Hollow {
                row: 0,
                col: 3,
                cells: 2,
                color: style.cursor,
            }
        );
    }

    assert_eq!(
        layout::cursor(&at(2, CursorShape::Beam), &style, true),
        CursorLayout::Beam {
            row: 0,
            col: 2,
            color: style.cursor,
        }
    );
    assert_eq!(
        layout::cursor(&at(3, CursorShape::Underline), &style, true),
        CursorLayout::Underline {
            row: 0,
            col: 3,
            cells: 2,
            color: style.cursor,
        }
    );
    // Beam and underline leave the character to the row.
    assert_eq!(
        texts(&at(0, CursorShape::Beam), true),
        ["a", "b", "c", "字"]
    );
    for focused in [true, false] {
        assert_eq!(
            layout::cursor(&at(2, CursorShape::Hidden), &style, focused),
            CursorLayout::Hidden
        );
    }
}
