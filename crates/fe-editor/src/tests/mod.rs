use super::*;

mod laws;

/// Builds a model from text where `|` marks carets and `[`…`]` selections
/// (head at `]`).
pub(super) fn parse(marked: &str) -> EditorModel {
    let mut text = String::new();
    let mut selections = Vec::new();
    let mut open = None;
    for ch in marked.chars() {
        match ch {
            '|' => selections.push(caret(text.len())),
            '[' => open = Some(text.len()),
            ']' => selections.push(selection_from(open.take().unwrap(), text.len())),
            _ => text.push(ch),
        }
    }
    let mut model = EditorModel::new(Buffer::new(&text), None);
    model.set_selections(selections);
    model
}

/// Renders the model back into the same notation.
pub(super) fn render(model: &EditorModel) -> String {
    let text = model.text().to_string();
    let mut marks: Vec<(usize, char)> = Vec::new();
    for selection in model.selections() {
        if selection.range.is_empty() {
            marks.push((selection.range.start, '|'));
        } else {
            marks.push((selection.range.start, '['));
            marks.push((selection.range.end, ']'));
        }
    }
    marks.sort_by_key(|(offset, mark)| (*offset, *mark == '['));
    let mut out = String::new();
    let mut last = 0;
    for (offset, mark) in marks {
        out.push_str(&text[last..offset]);
        out.push(mark);
        last = offset;
    }
    out.push_str(&text[last..]);
    out
}

fn moved(marked: &str, movement: Movement, extend: bool) -> String {
    let mut model = parse(marked);
    model.move_cursors(movement, extend);
    render(&model)
}

#[test]
fn notation_round_trips() {
    assert_eq!(render(&parse("a|b [cd] e|")), "a|b [cd] e|");
}

#[test]
fn horizontal_moves_respect_graphemes() {
    assert_eq!(moved("a😀|b", Movement::Left, false), "a|😀b");
    // 'e' + combining acute accent is one grapheme.
    assert_eq!(moved("|e\u{301}x", Movement::Right, false), "e\u{301}|x");
    assert_eq!(moved("ab|\ncd", Movement::Right, false), "ab\n|cd");
    assert_eq!(moved("ab\n|cd", Movement::Left, false), "ab|\ncd");
    // Collapsing a selection goes to its edge rather than moving.
    assert_eq!(moved("a[bc]d", Movement::Left, false), "a|bcd");
    assert_eq!(moved("a[bc]d", Movement::Right, false), "abc|d");
    assert_eq!(moved("a|bcd", Movement::Right, true), "a[b]cd");
}

#[test]
fn word_moves() {
    assert_eq!(
        moved("|foo_bar.baz()", Movement::WordRight, false),
        "foo_bar|.baz()"
    );
    assert_eq!(
        moved("foo_bar|.baz()", Movement::WordRight, false),
        "foo_bar.|baz()"
    );
    assert_eq!(
        moved("let x|  = 1", Movement::WordRight, false),
        "let x  =| 1"
    );
    assert_eq!(
        moved("let x  = 1|", Movement::WordLeft, false),
        "let x  = |1"
    );
    assert_eq!(moved("a\n|b", Movement::WordLeft, false), "a|\nb");
}

#[test]
fn smart_home_and_end() {
    assert_eq!(
        moved("    let| x", Movement::LineStart, false),
        "    |let x"
    );
    assert_eq!(
        moved("    |let x", Movement::LineStart, false),
        "|    let x"
    );
    assert_eq!(moved("a|bc\nd", Movement::LineEnd, false), "abc|\nd");
}

#[test]
fn vertical_moves_keep_goal_column() {
    let mut model = parse("abcdef|\nab\nabcdefgh");
    model.move_cursors(Movement::Down, false);
    assert_eq!(render(&model), "abcdef\nab|\nabcdefgh");
    model.move_cursors(Movement::Down, false);
    assert_eq!(render(&model), "abcdef\nab\nabcdef|gh");
    model.move_cursors(Movement::Up, false);
    model.move_cursors(Movement::Up, false);
    assert_eq!(render(&model), "abcdef|\nab\nabcdefgh");
    // Past either end goes to the document's start or end.
    model.move_cursors(Movement::Up, false);
    assert_eq!(render(&model), "|abcdef\nab\nabcdefgh");
    model.move_cursors(Movement::PageDown(10), false);
    assert_eq!(render(&model), "abcdef\nab\nabcdefgh|");
}

#[test]
fn vertical_moves_line_up_through_tabs() {
    // Column 4 on the first row is just after the tab on the second.
    assert_eq!(moved("abcd|\n\tx", Movement::Down, false), "abcd\n\t|x");
    assert_eq!(moved("ab|\n\tx", Movement::Down, false), "ab\n|\tx");
}

#[test]
fn multi_cursor_insert_and_delete() {
    let mut model = parse("a|b\nc|d\n[ef]");
    model.insert("XY");
    assert_eq!(render(&model), "aXY|b\ncXY|d\nXY|");
    model.backspace();
    assert_eq!(render(&model), "aX|b\ncX|d\nX|");
    model.delete_forward();
    assert_eq!(render(&model), "aX|\ncX|\nX|");
}

#[test]
fn carets_on_the_same_char_merge() {
    let mut model = parse("ab||c");
    assert_eq!(model.selections().len(), 1);
    model.backspace();
    assert_eq!(render(&model), "a|c");
}

#[test]
fn backspace_removes_whole_graphemes() {
    let mut model = parse("a👍🏽|b");
    model.backspace();
    assert_eq!(render(&model), "a|b");
}

#[test]
fn newline_keeps_indentation() {
    let mut model = parse("    let x = 1;|\nfn");
    model.newline();
    assert_eq!(render(&model), "    let x = 1;\n    |\nfn");
    // Splitting inside the indentation doesn't over-indent.
    let mut split = parse("  |  x");
    split.newline();
    assert_eq!(render(&split), "  \n  |  x");
}

#[test]
fn tab_inserts_to_next_stop() {
    let mut model = parse("ab|");
    model.tab();
    assert_eq!(render(&model), "ab  |");
}

#[test]
fn delete_word_left() {
    let mut model = parse("let value|");
    model.delete_word_left();
    assert_eq!(render(&model), "let |");
}

#[test]
fn copy_cut_paste() {
    let model1 = parse("a[bc]d [ef]");
    assert_eq!(model1.copy_text(), ("bc\nef".to_string(), false));

    // With only carets, whole rows are copied and cut.
    let mut model2 = parse("one\nt|wo\nthree");
    assert_eq!(model2.copy_text(), ("two\n".to_string(), true));
    assert_eq!(model2.cut(), "two\n");
    assert_eq!(render(&model2), "one\n|three");

    // One line per cursor is distributed.
    let mut model3 = parse("|a\n|b");
    model3.paste("1\n2");
    assert_eq!(render(&model3), "1|a\n2|b");
    let mut model4 = parse("x|");
    model4.paste("1\n2");
    assert_eq!(render(&model4), "x1\n2|");
}

#[test]
fn undo_places_carets_at_the_change() {
    let mut model = parse("hello |world");
    model.insert("big ");
    model.set_cursor(0);
    model.undo().unwrap();
    assert_eq!(render(&model), "hello |world");
    model.redo().unwrap();
    assert_eq!(render(&model), "hello big |world");
}

#[test]
fn word_and_line_ranges() {
    let model = parse("let foo_bar = 1;\nnext");
    assert_eq!(model.word_range_at(6), 4..11);
    assert_eq!(
        model.word_range_at(11),
        4..11,
        "just after a word selects it"
    );
    assert_eq!(model.line_range_at(3), 0..17);
    assert_eq!(model.line_range_at(18), 17..21);
}

#[test]
fn selections_follow_ai_edits() {
    let mut model = parse("fn a() {}\nfn b() {|}");
    let len = model.text().len();
    model.buffer_mut().edit(
        [(0..0, "// generated\n")],
        Origin::Ai {
            agent: "a".into(),
            session: "s".into(),
        },
    );
    assert_eq!(
        model.primary_selection().head(),
        len - 1 + "// generated\n".len()
    );
}

#[test]
fn save_round_trips_line_endings() {
    let dir = std::env::temp_dir().join(format!("fastedit-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("crlf.txt");
    std::fs::write(&path, "one\r\ntwo\r\n").unwrap();

    let mut model = EditorModel::open(&path).unwrap();
    assert_eq!(model.text().to_string(), "one\ntwo\n");
    model.set_cursor(3);
    model.insert("!");
    assert!(model.is_dirty());
    model.save().unwrap();
    assert!(!model.is_dirty());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "one!\r\ntwo\r\n");
    let leftovers: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
    assert_eq!(leftovers.len(), 1, "no temp file left behind");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn save_failures_leave_nothing_behind() {
    let mut untitled = EditorModel::new(Buffer::new("x"), None);
    assert_eq!(
        untitled.save().unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
    assert!(untitled.is_dirty() || untitled.path().is_none());

    let dir = std::env::temp_dir().join(format!("fastedit-missing-{}", std::process::id()));
    let mut model = EditorModel::new(Buffer::new("x"), Some(dir.join("nested/file.txt")));
    model.insert("y");
    assert!(model.save().is_err());
    assert!(model.is_dirty(), "a failed save keeps the buffer dirty");
    assert!(!dir.exists());
}

#[cfg(unix)]
#[test]
fn save_keeps_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("fastedit-perms-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("script.sh");
    std::fs::write(&path, "echo hi\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o750)).unwrap();
    let mut model = EditorModel::open(&path).unwrap();
    assert_eq!(model.path(), Some(path.as_path()));
    model.insert("# ");
    model.save().unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o750);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn tab_size_is_configurable() {
    let mut model = parse("a|");
    assert_eq!(model.tab_size(), 4);
    model.set_tab_size(2);
    model.tab();
    assert_eq!(render(&model), "a |");
    model.set_tab_size(0);
    assert_eq!(model.tab_size(), 1, "clamped");
    assert_eq!(
        model.buffer().text().to_string(),
        model.snapshot().text().to_string()
    );
}

#[cfg(unix)]
#[test]
fn save_writes_through_symlinks_and_keeps_private_files_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("fastedit-link-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let real = dir.join("secret.env");
    let link = dir.join("link.env");
    std::fs::write(&real, "KEY=1\n").unwrap();
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let mut model = EditorModel::open(&link).unwrap();
    model.set_cursor(0);
    model.insert("# ");
    model.save().unwrap();

    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink(),
        "link kept"
    );
    assert_eq!(std::fs::read_to_string(&real).unwrap(), "# KEY=1\n");
    let mode = std::fs::metadata(&real).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let entries = std::fs::read_dir(&dir).unwrap().count();
    assert_eq!(entries, 2, "no temp file left behind");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn save_creates_new_files() {
    let dir = std::env::temp_dir().join(format!("fastedit-new-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("new.txt");
    let mut model = EditorModel::new(Buffer::new(""), Some(path.clone()));
    model.insert("hello");
    model.save().unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
    std::fs::remove_dir_all(&dir).unwrap();
}
