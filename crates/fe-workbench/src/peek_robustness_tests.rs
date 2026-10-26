//! Peek Definition's targets where the text isn't simple: files in other
//! encodings and line endings, targets picked before their baselines are
//! read, a view resized under the peek, and target buffers edited around
//! a definition, multi-line ones too.

use std::{
    fs,
    ops::Range,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, atomic::AtomicBool},
};

use fe_editor::decorations::DecorationKind;
use fe_gpui::EditorView;
use fe_nav::{
    Definition, Definitions, Location, NavError, NavRequest, NavigationProvider, Precision,
    Readiness, Tier, WorkspaceSymbol,
};
use gpui::{
    Bounds, Entity, EntityInputHandler as _, Focusable as _, Modifiers, Pixels, TestAppContext,
    VisualTestContext, px, size,
};

use crate::peek::{PeekView, ROWS};
use crate::{Workbench, tests::open_root};

const MAIN: &str = "mod util;\n\nfn main() {\n    util::helper();\n}\n";

/// `helper` on row 20.
fn util() -> String {
    let mut text: String = (0..20).map(|n| format!("// line {n}\n")).collect();
    text.push_str("pub fn helper() {}\n");
    text.extend((21..40).map(|n| format!("// line {n}\n")));
    text
}

const OTHER: &str = "fn helper() {\n    // the other one\n}\n";

/// A definition over three rows, from `helper` to its `)`.
const MULTI: &str = "// top\npub fn helper(\n    a: u8,\n) {}\n// end\n";

/// Answers every definitions request with its items.
struct Fake(Arc<Mutex<Vec<Definition>>>);

impl NavigationProvider for Fake {
    fn name(&self) -> &str {
        "fake"
    }

    fn precision(&self) -> Precision {
        Precision::Exact
    }

    fn readiness(&self, _: Option<&str>) -> Readiness {
        Readiness::Ready
    }

    fn definitions(&self, _: &NavRequest, _: &AtomicBool) -> Result<Definitions, NavError> {
        let items = self.0.lock().unwrap().clone();
        Ok(Definitions {
            word: Some("helper".into()),
            unambiguous: items.len() == 1,
            items,
            partial: false,
        })
    }

    fn workspace_symbols(
        &self,
        _: &str,
        _: usize,
        _: &AtomicBool,
    ) -> Result<Vec<WorkspaceSymbol>, NavError> {
        Ok(Vec::new())
    }
}

/// `range` of `text`, the file at `rel` as the editor loads it, as a
/// definition on disk.
fn definition(rel: &str, text: &str, range: Range<usize>) -> Definition {
    let row = text[..range.start].matches('\n').count() as u32;
    Definition {
        location: Location {
            rel: rel.into(),
            range,
            row,
            snapshot: None,
        },
        name: "helper".into(),
        kind: None,
        container: None,
        tier: Tier::Project,
    }
}

/// `helper` in `text`.
fn name_in(text: &str) -> Range<usize> {
    let start = text.find("helper").unwrap();
    start..start + "helper".len()
}

/// [`MULTI`]'s definition, from `helper` to the `)` starting a row.
fn multi_range(text: &str) -> Range<usize> {
    let start = text.find("helper").unwrap();
    start..text.find("\n)").unwrap() + 2
}

struct Opened<'a> {
    _dir: tempfile::TempDir,
    root: PathBuf,
    workbench: Entity<Workbench>,
    answers: Arc<Mutex<Vec<Definition>>>,
    cx: &'a mut VisualTestContext,
}

fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// A workbench on `src/main.rs` (`main`), `src/util.rs` and `src/other.rs`,
/// showing `main.rs` focused with its caret in `helper`; the fake answers
/// with `helper` in `util.rs`.
fn opened<'a>(cx: &'a mut TestAppContext, main: &str) -> Opened<'a> {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), main).unwrap();
    fs::write(root.join("src/util.rs"), util()).unwrap();
    fs::write(root.join("src/other.rs"), OTHER).unwrap();
    let (workbench, cx) = open_root(cx, &root);
    cx.simulate_resize(size(px(1000.), px(800.)));
    let answers = Arc::new(Mutex::new(vec![definition(
        "src/util.rs",
        &util(),
        name_in(&util()),
    )]));
    let navigation = workbench.read_with(cx, |workbench, _| workbench.navigation().clone());
    navigation.add(Arc::new(Fake(answers.clone())), 10);
    cx.update(|window, _| window.activate_window());
    show_main(&workbench, &root, cx);
    Opened {
        _dir: dir,
        root,
        workbench,
        answers,
        cx,
    }
}

/// Shows `main.rs`, focused, its caret in `helper`.
fn show_main(workbench: &Entity<Workbench>, root: &Path, cx: &mut VisualTestContext) {
    let main = show_file(workbench, root, "src/main.rs", cx);
    main.update(cx, |main, cx| {
        let at = main.model().text().to_string().find("helper").unwrap();
        main.model_mut().set_cursor(at + 2);
        cx.notify();
    });
    settle(cx);
}

/// Opens `rel` in the pane, focused, and returns its editor.
fn show_file(
    workbench: &Entity<Workbench>,
    root: &Path,
    rel: &str,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    cx.update(|window, cx| {
        let pane = workbench.read(cx).pane().clone();
        pane.update(cx, |pane, cx| pane.open(root.join(rel), window, cx));
    });
    settle(cx);
    let editor = editor(workbench, cx);
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    settle(cx);
    editor
}

fn editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a file is shown")
}

fn peek(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<Entity<PeekView>> {
    workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        pane.peek.as_ref().map(|peek| peek.view.clone())
    })
}

fn peek_definition(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<PeekView> {
    cx.simulate_keystrokes("alt-f12");
    settle(cx);
    peek(workbench, cx).expect("a peek opened")
}

fn excerpt(peek: &Entity<PeekView>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    peek.read_with(cx, |peek, _| {
        peek.excerpt().cloned().ok_or(peek.error.clone())
    })
    .expect("the excerpt is shown")
}

/// The active editor's file and its selection.
fn at(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (PathBuf, Range<usize>) {
    let editor = editor(workbench, cx);
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        (
            model.path().unwrap().to_owned(),
            model.selections()[0].range.clone(),
        )
    })
}

/// The excerpt's selection, and its highlights on each row from the
/// selection's first to its last.
fn shown(
    excerpt: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> (Range<usize>, Vec<Vec<Range<usize>>>) {
    excerpt.read_with(cx, |excerpt, _| {
        let model = excerpt.model();
        let text = model.text();
        let selection = model.selections()[0].range.clone();
        let rows =
            text.offset_to_point(selection.start).row..=text.offset_to_point(selection.end).row;
        let highlights = rows
            .map(|row| {
                let start = text.row_start(row);
                let end = start + text.row_len(row) as usize;
                (model.decorations(start..end).into_iter())
                    .filter(|decoration| decoration.kind == DecorationKind::Highlight)
                    .map(|decoration| decoration.range)
                    .collect()
            })
            .collect();
        (selection, highlights)
    })
}

/// Asserts the excerpt selects and highlights `range` exactly, on each of
/// its rows.
fn assert_shows(
    excerpt: &Entity<EditorView>,
    range: &Range<usize>,
    case: &str,
    cx: &mut VisualTestContext,
) {
    let (selection, highlights) = shown(excerpt, cx);
    assert_eq!(&selection, range, "{case}: selected");
    for (row, on_row) in highlights.iter().enumerate() {
        assert_eq!(
            on_row,
            std::slice::from_ref(range),
            "{case}: highlighted on row {row}"
        );
    }
}

fn edit(editor: &Entity<EditorView>, range: Range<usize>, text: &str, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, cx| {
        (editor.model_mut().buffer_mut()).edit([(range, text)], fe_buffer::Origin::User);
        cx.notify();
    });
    settle(cx);
}

fn enter_in(excerpt: &Entity<EditorView>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.focus(&excerpt.focus_handle(cx), cx));
    settle(cx);
    cx.simulate_keystrokes("enter");
    settle(cx);
}

/// Replacements, each in the text the ones before it made.
type Edits<'a> = Vec<(Range<usize>, &'a str)>;

/// One target: `rel`, written as `bytes`, loading as `text`, with the
/// definition at `range` of it; once open, `edits` make it `now`, where
/// it's at `expected`.
struct Case<'a> {
    name: &'a str,
    rel: &'a str,
    bytes: Vec<u8>,
    text: &'a str,
    range: Range<usize>,
    edits: Edits<'a>,
    expected: Range<usize>,
}

/// Peeks at `case`'s target closed: the excerpt shows it as on disk; then
/// opens it and makes its edits, and Enter opens at it there. Then peeks
/// at it open in its tab: the excerpt shows it as edited, and Enter opens
/// at it.
fn round_trip(case: Case, opened: &mut Opened) {
    let (root, workbench) = (opened.root.clone(), opened.workbench.clone());
    let (answers, cx) = (&opened.answers, &mut *opened.cx);
    let name = case.name;
    let path = root.join(case.rel);
    fs::write(&path, &case.bytes).unwrap();
    *answers.lock().unwrap() = vec![definition(case.rel, case.text, case.range.clone())];

    // Closed.
    let peek = peek_definition(&workbench, cx);
    let shown = excerpt(&peek, cx);
    assert_eq!(
        shown.read_with(cx, |shown, _| shown.model().text().to_string()),
        case.text,
        "{name}: read as the editor loads it"
    );
    assert_shows(&shown, &case.range, &format!("{name} closed"), cx);
    let target = show_file(&workbench, &root, case.rel, cx);
    for (range, text) in &case.edits {
        edit(&target, range.clone(), text, cx);
    }
    show_main(&workbench, &root, cx);
    assert!(self::peek(&workbench, cx).is_some(), "{name}: still open");
    enter_in(&shown, cx);
    assert_eq!(
        at(&workbench, cx),
        (path.clone(), case.expected.clone()),
        "{name}: opened from closed"
    );

    // Open in a tab, edited.
    show_main(&workbench, &root, cx);
    let peek = peek_definition(&workbench, cx);
    let shown = excerpt(&peek, cx);
    assert_shows(&shown, &case.expected, &format!("{name} open"), cx);
    enter_in(&shown, cx);
    assert_eq!(
        at(&workbench, cx),
        (path, case.expected.clone()),
        "{name}: opened from open"
    );
    show_main(&workbench, &root, cx);
}

fn utf16(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    bytes
}

#[gpui::test]
fn encoded_targets(cx: &mut TestAppContext) {
    let mut opened = opened(cx, MAIN);
    let text = "// é top\n// row\npub fn helper() {}\n";
    let range = name_in(text);
    let crlf = text.replace('\n', "\r\n");
    let bom = [b"\xEF\xBB\xBF".as_slice(), text.as_bytes()].concat();
    let latin1: Vec<u8> = text.chars().map(|c| c as u8).collect();
    let cases = [
        ("CRLF", "src/crlf.rs", crlf.into_bytes()),
        ("BOM", "src/bom.rs", bom),
        ("UTF-16", "src/utf16.rs", utf16(text)),
        ("latin-1", "src/latin1.rs", latin1),
    ];
    for (name, rel, bytes) in cases {
        // Open, it's edited above the definition, on its row too.
        let at = "// new\n".len() + text.find("pub").unwrap();
        let edits = vec![(0..0, "// new\n"), (at..at + 3, "pub(crate)")];
        let now = format!("// new\n{}", text.replacen("pub", "pub(crate)", 1));
        let case = Case {
            name,
            rel,
            bytes,
            text,
            range: range.clone(),
            edits,
            expected: name_in(&now),
        };
        round_trip(case, &mut opened);
    }
}

#[gpui::test]
fn target_edits_keep_range(cx: &mut TestAppContext) {
    let mut opened = opened(cx, MAIN);
    let range = multi_range(MULTI);
    let at = |text: &str| MULTI.find(text).unwrap();
    let after = |text: &str| at(text) + text.len();
    let large = "// pad\n".repeat(fe_editor::text_diff::MAX_DIFF_BYTES / 7 + 1);
    // More than a token diff takes.
    let rows_above = "// pad\n".repeat(10_000);
    // Each makes MULTI into the text given, the definition at its range.
    let cases: Vec<(&str, Edits, String)> = vec![
        ("unedited", vec![], MULTI.to_owned()),
        (
            "rows above",
            vec![(0..0, "// a\n// b\n")],
            format!("// a\n// b\n{MULTI}"),
        ),
        (
            "before it on its first row",
            vec![(at("pub")..after("pub"), "pub(crate)")],
            MULTI.replacen("pub", "pub(crate)", 1),
        ),
        (
            "inside it",
            vec![(after("a: u8")..after("a: u8"), ", b: u16")],
            MULTI.replacen("a: u8", "a: u8, b: u16", 1),
        ),
        (
            "after it on its last row",
            vec![(after(")")..after(")"), " -> u8")],
            MULTI.replacen(")", ") -> u8", 1),
        ),
        (
            "joining its rows",
            vec![(after("(")..at("a:"), "")],
            MULTI.replacen("(\n    a", "(a", 1),
        ),
        (
            "splitting a row",
            vec![(after("a: u8,")..after("a: u8,"), "\n    b: u16,")],
            MULTI.replacen("a: u8,", "a: u8,\n    b: u16,", 1),
        ),
        (
            "before it and inside it, in one hunk",
            vec![
                (at("u8")..after("u8"), "u16"),
                (at("pub")..after("pub"), "pub(crate)"),
            ],
            MULTI
                .replacen("pub", "pub(crate)", 1)
                .replacen("a: u8", "a: u16", 1),
        ),
        (
            "a like-named definition before it on its row",
            vec![(at("pub")..at("pub"), "pub fn hello() {} ")],
            MULTI.replacen("pub", "pub fn hello() {} pub", 1),
        ),
        (
            "many rows above it, and before it on its row, in one hunk",
            vec![
                (at("pub")..after("pub"), "pub(crate)"),
                (at("pub")..at("pub"), rows_above.as_str()),
            ],
            MULTI.replacen("pub", &format!("{rows_above}pub(crate)"), 1),
        ),
        (
            "too large to diff",
            vec![(0..0, large.as_str())],
            format!("{large}{MULTI}"),
        ),
    ];
    for (ix, (name, edits, now)) in cases.into_iter().enumerate() {
        let rel = format!("src/multi{ix}.txt");
        let case = Case {
            name,
            rel: &rel,
            bytes: MULTI.as_bytes().to_vec(),
            text: MULTI,
            range: range.clone(),
            edits,
            expected: multi_range(&now),
        };
        round_trip(case, &mut opened);
    }

    // Its row changed on both sides of it: where the diff can't tell, it's
    // found again.
    let text = "// top\npub fn helper() {}\n";
    let now = "// top\npub(crate) fn helper<T>() {}\n";
    let case = Case {
        name: "both sides of it on its row",
        rel: "src/lost.txt",
        bytes: text.as_bytes().to_vec(),
        text,
        range: name_in(text),
        edits: vec![(7..10, "pub(crate)"), (27..27, "<T>")],
        expected: name_in(now),
    };
    round_trip(case, &mut opened);
}

#[gpui::test]
fn select_before_load(cx: &mut TestAppContext) {
    let Opened {
        _dir,
        root,
        workbench,
        answers,
        cx,
    } = opened(cx, MAIN);
    answers
        .lock()
        .unwrap()
        .push(definition("src/other.rs", OTHER, name_in(OTHER)));
    let (release, hold) = async_channel::bounded::<()>(1);
    crate::peek::HOLD_BASELINES.with(|held| *held.borrow_mut() = Some(hold));
    let peek = peek_definition(&workbench, cx);
    let first = excerpt(&peek, cx);
    assert_eq!(
        first.read_with(cx, |first, _| first.model().text().to_string()),
        util()
    );

    // `other.rs` opened and edited, then its row picked, before the
    // baselines are read: neither the first target nor a stale range
    // shows, and nothing opens.
    let other = show_file(&workbench, &root, "src/other.rs", cx);
    edit(&other, 0..0, "// new\n// rows\n", cx);
    show_main(&workbench, &root, cx);
    let row = cx
        .debug_bounds("peek-row-1")
        .expect("the second row is drawn");
    cx.simulate_click(row.center(), Modifiers::none());
    settle(cx);
    let (excerpt_shown, header, opening) = peek.read_with(cx, |peek, cx| {
        (
            peek.excerpt().is_some(),
            peek.header(cx),
            peek.opening(cx).is_some(),
        )
    });
    assert!(!excerpt_shown && header.is_none() && !opening);
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.open_from_peek(window, cx))
    });
    settle(cx);
    assert!(self::peek(&workbench, cx).is_some(), "nothing opened");
    assert_eq!(at(&workbench, cx).0, root.join("src/main.rs"));

    // Once read, it shows where it is now.
    drop(release);
    settle(cx);
    let shown = excerpt(&peek, cx);
    let now = format!("// new\n// rows\n{OTHER}");
    assert_eq!(
        shown.read_with(cx, |shown, _| shown.model().text().to_string()),
        now
    );
    assert_shows(&shown, &name_in(&now), "loaded", cx);
    assert_eq!(
        peek.read_with(cx, |peek, cx| peek.header(cx)).as_deref(),
        Some("other.rs 3:4")
    );
    enter_in(&shown, cx);
    assert_eq!(
        at(&workbench, cx),
        (root.join("src/other.rs"), name_in(&now))
    );
}

/// The editor's peek block, as its last frame placed it, and its frame.
fn reserved(
    editor: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> (Bounds<Pixels>, Bounds<Pixels>) {
    editor.read_with(cx, |editor, _| {
        let blocks = editor.reserved_blocks();
        assert_eq!(blocks.len(), 1, "{blocks:?}");
        (blocks[0].bounds, blocks[0].clip)
    })
}

fn row_height(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Pixels {
    editor.update_in(cx, |editor, window, cx| {
        let top = editor.viewport_rows().expect("drawn").0;
        let at = editor.model().text().row_start(top);
        let caret = editor.bounds_for_range(at..at, Bounds::default(), window, cx);
        caret.expect("on screen").size.height
    })
}

/// Resizes the window to show `rows` rows of `editor` fewer.
fn shrink_by(rows: f32, editor: &Entity<EditorView>, cx: &mut VisualTestContext) {
    let height = cx.update(|window, _| window.viewport_size().height);
    let row = row_height(editor, cx);
    cx.simulate_resize(size(px(1000.), height - row * rows));
    settle(cx);
    settle(cx);
}

#[gpui::test]
fn resize_refits(cx: &mut TestAppContext) {
    // Resized, still as many rows as it takes: shown whole again. The checks
    // after each resize are plain `assert!`s: a panic with a custom message
    // isn't counted as an assertion by the regression check.
    let call = "    util::helper();";
    let main: String = (0..80)
        .map(|row| match row {
            30 => format!("{call}\n"),
            _ => format!("// row {row}\n"),
        })
        .collect();
    let Opened {
        _dir,
        root,
        workbench,
        cx,
        ..
    } = opened(cx, &main);
    let editor = editor(&workbench, cx);
    editor.update(cx, |editor, cx| editor.scroll_to_buffer_line(20., cx));
    settle(cx);
    let (top, rows) = editor.read_with(cx, |editor, _| editor.viewport_rows().unwrap());
    assert_eq!(top, 20);
    assert!(rows >= 30, "{rows} rows");
    peek_definition(&workbench, cx);
    let (bounds, clip) = reserved(&editor, cx);
    assert!(bounds.bottom() <= clip.bottom(), "in view at first");
    // To 17 rows: row 30 and the peek's 12 under it run past the bottom.
    shrink_by(rows as f32 - 17., &editor, cx);
    let (top, rows) = editor.read_with(cx, |editor, _| editor.viewport_rows().unwrap());
    assert!(rows > ROWS && rows < 30 - 20 + ROWS + 1);
    let blocks = editor.read_with(cx, |editor, _| editor.blocks().to_vec());
    assert_eq!(blocks[0].rows, ROWS, "as many rows");
    assert!(top > 20);
    let (bounds, clip) = reserved(&editor, cx);
    assert!(bounds.bottom() <= clip.bottom() && bounds.top() >= clip.top());

    // Refit with fewer rows: the excerpt scrolls to the definition where
    // it is now, in `util.rs` open in a tab and edited above it.
    cx.update(|window, cx| workbench.update(cx, |workbench, cx| workbench.close_peek(window, cx)));
    cx.simulate_resize(size(px(1000.), px(800.)));
    settle(cx);
    let util_editor = show_file(&workbench, &root, "src/util.rs", cx);
    show_main(&workbench, &root, cx);
    let editor = self::editor(&workbench, cx);
    let peek = peek_definition(&workbench, cx);
    let excerpt = excerpt(&peek, cx);
    let rows_above = "// above\n".repeat(30);
    edit(&util_editor, 0..0, &rows_above, cx);
    assert_eq!(
        peek.read_with(cx, |peek, cx| peek.header(cx)).as_deref(),
        Some("util.rs 51:8"),
        "the header follows it"
    );
    let rows = editor.read_with(cx, |editor, _| editor.viewport_rows().unwrap().1);
    shrink_by(rows as f32 - 8., &editor, cx);
    let blocks = editor.read_with(cx, |editor, _| editor.blocks().to_vec());
    assert!(blocks[0].rows < ROWS);
    let (top, shown) = excerpt.read_with(cx, |excerpt, _| excerpt.viewport_rows().unwrap());
    assert!(top <= 50 && 50 < top + shown.max(1));
}

#[test]
fn map_through_keeps_columns_and_spans() {
    let reveal = |old: &str, new: &str, range: Range<usize>| {
        let new_text = fe_text::Text::new(new);
        crate::peek::map_through(old, &new_text, range).range(&new_text)
    };
    let old = MULTI;
    let range = multi_range(old);
    // An insertion at its start goes before it, and at its end after it.
    let new = MULTI.replacen("helper", "xhelper", 1);
    assert_eq!(
        reveal(old, &new, range.clone()),
        range.start + 1..range.end + 1
    );
    let new = MULTI.replacen(")", ")x", 1);
    assert_eq!(reveal(old, &new, range.clone()), range.clone());
    // Its rows gone: it's found where it's nearest.
    let new = "// top\n// end\npub fn helper(\n    a: u8,\n) {}\n";
    assert_eq!(reveal(old, new, range.clone()), multi_range(new));
    let new = "// top\n// end\n";
    assert_eq!(reveal(old, new, range.clone()), 7..7, "the row it was on");
    // Edits before it and inside it, in one hunk.
    let new = (MULTI.replacen("pub", "pub(crate)", 1)).replacen("a: u8", "a: u16", 1);
    assert_eq!(reveal(old, &new, range), multi_range(&new));

    // A like-named definition inserted before it on its row: `hel` they
    // share doesn't take its start.
    let old = "pub fn helper() {}\n";
    assert_eq!(
        reveal(old, "pub fn hello() {} pub fn helper() {}\n", 7..13),
        25..31
    );
    // Nor in a row too large to diff by token, by what it starts and ends with.
    let new = format!(
        "pub fn hello() {{}} /*{}*/ pub fn helper() {{}}\n",
        "x".repeat(70_000)
    );
    let at = new.find("helper").unwrap();
    assert_eq!(reveal(old, &new, 7..13), at..at + 6);
    // In a hunk too large to diff by token, by what it starts and ends with.
    let new = format!("{}pub(crate) fn helper() {{}}\n", "// pad\n".repeat(10_000));
    assert_eq!(reveal("pub fn helper() {}\n", &new, 7..13), 70014..70020);

    // Whole rows inserted at its ends stay outside it.
    let old = "fn helper() {}\n// next\n";
    let new = "fn helper() {}\n// inserted\n// next\n";
    assert_eq!(reveal(old, new, 3..15), 3..15);
    let new = "// inserted\nfn helper() {}\n// next\n";
    assert_eq!(reveal(old, new, 0..15), 12..27);
    // Its whole row moved: found where it went, not where it was.
    let old = "// top\nfn helper() {}\n// a\n// b\n";
    let new = "// top\n// a\n// b\nfn helper() {}\n";
    assert_eq!(reveal(old, new, 7..22), 17..32);

    // Too large to diff, its name elsewhere too: its own rows, and the
    // rows around them where those are in more than one place.
    let large = "// pad\n".repeat(fe_editor::text_diff::MAX_DIFF_BYTES / 7 + 1);
    let old = "// top\npub fn helper() {}\n";
    let others = "fn call() { helper(); }\nmod a {\npub fn helper() {}\n}\n";
    let new = format!("// helper docs\n{large}{others}{old}let helpers = 0;\n{large}");
    let at = new.find(old).unwrap() + 14;
    assert_eq!(reveal(old, &new, 14..20), at..at + 6);
    // Its rows gone too: not some other `helper`.
    let new = format!("// helper docs\n{large}{others}pub(crate) fn helper() {{}}\n{large}");
    let found = reveal(old, &new, 14..20);
    assert_eq!(found.len(), 0, "{found:?}");
    // Diffed: not a copy of its rows the diff kept, which is other text.
    let old = "// top\npub fn helper() {}\nmod a {\npub fn helper() {}\n}\n";
    let new = "// top\npub fn assist() {}\nmod a {\npub fn helper() {}\n}\n";
    assert_eq!(reveal(old, new, 14..20), 7..7, "its row");
}
