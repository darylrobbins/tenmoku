//! Peek Definition in a headless workbench, with a fake provider's answer:
//! alt-f12 on `helper` in `main.rs` opens the peek under the caret's row.

use std::{
    fs,
    ops::Range,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use fe_editor::decorations::DecorationKind;
use fe_gpui::{EditorView, HostBlock, HostBlockKind};
use fe_nav::{
    Definition, Definitions, Location, NavError, NavRequest, NavigationProvider, Precision,
    Readiness, Tier, WorkspaceSymbol,
};
use gpui::{
    AppContext as _, Bounds, Entity, EntityInputHandler as _, Focusable as _, Modifiers,
    MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, TestAppContext, VisualTestContext,
    px, size,
};

use super::{BLOCK_ID, PeekView, ROWS};
use crate::{Workbench, tests::open_root};

const MAIN: &str = "mod util;\n\nfn main() {\n    util::helper();\n}\nfn other() {}\n";

/// `helper` on row 20, so the excerpt scrolls to it.
fn util() -> String {
    let mut text: String = (0..20).map(|n| format!("// line {n}\n")).collect();
    text.push_str("pub fn helper() {}\n");
    text.extend((21..40).map(|n| format!("// line {n}\n")));
    text
}

const OTHER: &str = "fn helper() {\n    // the other one\n}\n";

/// Answers every definitions request with its items, or fails while its
/// switch is on.
struct Fake(Arc<Mutex<Vec<Definition>>>, Arc<AtomicBool>);

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
        if self.1.load(Ordering::Relaxed) {
            return Err(NavError::Unavailable("down".into()));
        }
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

/// `helper` in `text`, the file at `rel`, as a definition.
fn definition(rel: &str, text: &str) -> Definition {
    let start = text.find("helper").unwrap();
    let row = text[..start].matches('\n').count() as u32;
    Definition {
        location: Location {
            rel: rel.into(),
            range: start..start + "helper".len(),
            row,
            snapshot: None,
        },
        name: "helper".into(),
        kind: None,
        container: None,
        tier: Tier::Project,
    }
}

struct Opened<'a> {
    _dir: tempfile::TempDir,
    root: PathBuf,
    workbench: Entity<Workbench>,
    answers: Arc<Mutex<Vec<Definition>>>,
    /// Makes the fake fail.
    fails: Arc<AtomicBool>,
    cx: &'a mut VisualTestContext,
}

fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// A workbench on `src/main.rs`, `src/util.rs` and `src/other.rs`, showing
/// `main.rs` focused with its caret in `helper`, and the fake answering
/// with `helper` in `util.rs`.
fn opened(cx: &mut TestAppContext) -> Opened<'_> {
    opened_on(cx, MAIN)
}

/// As [`opened`], with `main` for `main.rs`.
fn opened_on<'a>(cx: &'a mut TestAppContext, main: &str) -> Opened<'a> {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), main).unwrap();
    fs::write(root.join("src/util.rs"), util()).unwrap();
    fs::write(root.join("src/other.rs"), OTHER).unwrap();
    let (workbench, cx) = open_root(cx, &root);
    cx.simulate_resize(size(px(1000.), px(800.)));
    let path = root.join("src/main.rs");
    cx.update(|window, cx| {
        let pane = workbench.read(cx).pane().clone();
        pane.update(cx, |pane, cx| pane.open(path, window, cx));
    });
    settle(cx);
    let answers = Arc::new(Mutex::new(vec![definition("src/util.rs", &util())]));
    let navigation = workbench.read_with(cx, |workbench, _| workbench.navigation().clone());
    let fails = Arc::new(AtomicBool::new(false));
    navigation.add(Arc::new(Fake(answers.clone(), fails.clone())), 10);
    let editor = editor(&workbench, cx);
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&editor.focus_handle(cx), cx);
    });
    editor.update(cx, |editor, cx| {
        editor
            .model_mut()
            .set_cursor(main.find("helper").unwrap() + 2);
        cx.notify();
    });
    settle(cx);
    Opened {
        _dir: dir,
        root,
        workbench,
        answers,
        fails,
        cx,
    }
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

fn excerpt(peek: &Entity<PeekView>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    peek.read_with(cx, |peek, _| {
        peek.excerpt().cloned().ok_or(peek.error.clone())
    })
    .expect("the excerpt is shown")
}

fn text_of(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.model().text().to_string())
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

/// The caret's box at `offset` in `editor`'s last frame.
fn caret_box(
    editor: &Entity<EditorView>,
    offset: usize,
    cx: &mut VisualTestContext,
) -> Bounds<Pixels> {
    editor.update_in(cx, |editor, window, cx| {
        editor
            .bounds_for_range(offset..offset, Bounds::default(), window, cx)
            .expect("on screen")
    })
}

fn peek_definition(cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("alt-f12");
    settle(cx);
}

fn focused(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

fn double_click(position: Point<Pixels>, cx: &mut VisualTestContext) {
    for click_count in [1, 2] {
        cx.simulate_event(MouseDownEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::none(),
            click_count,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::none(),
            click_count,
        });
    }
    settle(cx);
}

fn path(root: &Path, rel: &str) -> PathBuf {
    root.join(rel)
}

#[gpui::test]
fn peek_opens_below_the_caret_row(cx: &mut TestAppContext) {
    let Opened {
        _dir,
        workbench,
        cx,
        ..
    } = opened(cx);
    let editor = editor(&workbench, cx);
    let below = MAIN.find("}\n").unwrap();
    let caret_row = caret_box(&editor, MAIN.find("util::").unwrap(), cx);
    let before = caret_box(&editor, below, cx);
    let line_height = before.top() - caret_row.top();

    peek_definition(cx);
    let peek = peek(&workbench, cx).expect("a peek opened");
    let blocks = editor.read_with(cx, |editor, _| editor.blocks().to_vec());
    assert_eq!(
        blocks,
        [HostBlock {
            above: 4,
            rows: ROWS,
            kind: HostBlockKind::Reserve,
            id: BLOCK_ID,
        }]
    );
    // The rows after the caret's move down by the peek's.
    let after = caret_box(&editor, below, cx);
    assert_eq!(after.top(), before.top() + line_height * ROWS as f32);
    let reserved = editor.read_with(cx, |editor, _| editor.reserved_blocks().to_vec());
    assert_eq!(reserved.len(), 1);
    assert_eq!(reserved[0].bounds.top(), before.top());
    assert_eq!(reserved[0].bounds.size.height, line_height * ROWS as f32);

    // The target's lines, the definition highlighted and selected, in view
    // with rows above it.
    let excerpt = excerpt(&peek, cx);
    let util = util();
    assert_eq!(text_of(&excerpt, cx), util);
    let start = util.find("helper").unwrap();
    let name = start..start + "helper".len();
    let (highlights, selection, top, read_only) = excerpt.read_with(cx, |excerpt, _| {
        let model = excerpt.model();
        let highlights: Vec<Range<usize>> = (model.decorations(0..util.len()).into_iter())
            .filter(|decoration| decoration.kind == DecorationKind::Highlight)
            .map(|decoration| decoration.range)
            .collect();
        let top = excerpt.viewport_rows().map(|(top, _)| top);
        (
            highlights,
            model.selections()[0].range.clone(),
            top,
            excerpt.is_read_only(),
        )
    });
    assert_eq!(highlights, std::slice::from_ref(&name));
    assert_eq!(selection, name);
    assert_eq!(top, Some(17));
    assert!(read_only);
    let header = peek.read_with(cx, |peek, cx| peek.header(cx));
    assert_eq!(header.as_deref(), Some("util.rs 21:8"));
    assert!(focused(&excerpt, cx));
    // The excerpt sits in the reserved rows.
    let shown = caret_box(&excerpt, start, cx);
    assert!(
        reserved[0].bounds.contains(&shown.origin),
        "{shown:?} in {reserved:?}"
    );
}

#[gpui::test]
fn esc_closes_and_restores_focus(cx: &mut TestAppContext) {
    let Opened {
        _dir,
        workbench,
        cx,
        ..
    } = opened(cx);
    let editor = editor(&workbench, cx);
    let below = MAIN.find("}\n").unwrap();
    let before = caret_box(&editor, below, cx);
    let caret = |cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, _| editor.model().selections()[0].range.clone())
    };
    let was = caret(cx);
    peek_definition(cx);
    let peek = peek_of(&workbench, cx);
    assert!(focused(&excerpt(&peek, cx), cx));

    cx.simulate_keystrokes("escape");
    settle(cx);
    assert!(self::peek(&workbench, cx).is_none());
    assert_eq!(editor.read_with(cx, |editor, _| editor.blocks().len()), 0);
    assert!(editor.read_with(cx, |editor, _| editor.reserved_blocks().is_empty()));
    assert_eq!(caret_box(&editor, below, cx), before);
    assert_eq!(caret(cx), was);
    assert!(focused(&editor, cx));
}

fn peek_of(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<PeekView> {
    peek(workbench, cx).expect("a peek opened")
}

#[gpui::test]
fn enter_opens_target(cx: &mut TestAppContext) {
    let Opened {
        _dir,
        root,
        workbench,
        cx,
        ..
    } = opened(cx);
    let main = path(&root, "src/main.rs");
    let util_path = path(&root, "src/util.rs");
    let util = util();
    let start = util.find("helper").unwrap();
    let name = start..start + "helper".len();
    let origin = at(&workbench, cx);

    // Enter in the excerpt.
    peek_definition(cx);
    assert!(peek(&workbench, cx).is_some());
    cx.simulate_keystrokes("enter");
    settle(cx);
    assert_eq!(at(&workbench, cx), (util_path.clone(), name.clone()));
    assert!(peek(&workbench, cx).is_none());
    let opened = editor(&workbench, cx);
    assert!(focused(&opened, cx));

    // Back where it was, then a double-click on a word in the excerpt.
    cx.simulate_keystrokes("ctrl--");
    settle(cx);
    assert_eq!(at(&workbench, cx), origin);
    let editor = editor(&workbench, cx);
    assert_eq!(editor.read_with(cx, |editor, _| editor.blocks().len()), 0);
    peek_definition(cx);
    let peek = peek_of(&workbench, cx);
    let excerpt = excerpt(&peek, cx);
    let word = util.find("fn helper").unwrap();
    let position = caret_box(&excerpt, word + 1, cx).center();
    double_click(position, cx);
    assert_eq!(at(&workbench, cx), (util_path, word..word + 2));
    assert!(self::peek(&workbench, cx).is_none());

    // Editing below the peek leaves it; editing above it closes it.
    cx.simulate_keystrokes("ctrl--");
    settle(cx);
    let editor = self::editor(&workbench, cx);
    assert_eq!(at(&workbench, cx).0, main);
    peek_definition(cx);
    assert!(self::peek(&workbench, cx).is_some());
    let edit = |at: usize, editor: &Entity<EditorView>, cx: &mut VisualTestContext| {
        editor.update(cx, |editor, cx| {
            editor
                .model_mut()
                .buffer_mut()
                .edit([(at..at, "x")], fe_buffer::Origin::User);
            cx.notify();
        });
        settle(cx);
    };
    let other = MAIN.find("fn other").unwrap();
    edit(other, &editor, cx);
    assert!(self::peek(&workbench, cx).is_some(), "an edit below");
    edit(0, &editor, cx);
    assert!(self::peek(&workbench, cx).is_none(), "an edit above");
    assert_eq!(editor.read_with(cx, |editor, _| editor.blocks().len()), 0);

    // Moving the caret off its row closes it too.
    peek_definition(cx);
    assert!(self::peek(&workbench, cx).is_some());
    editor.update(cx, |editor, cx| {
        let text = editor.model().text();
        let row_end = text.row_start(3) + text.row_len(3) as usize;
        editor.model_mut().set_cursor(row_end);
        cx.notify();
    });
    settle(cx);
    assert!(self::peek(&workbench, cx).is_some(), "still on its row");
    editor.update(cx, |editor, cx| {
        editor.model_mut().set_cursor(0);
        cx.notify();
    });
    settle(cx);
    assert!(self::peek(&workbench, cx).is_none(), "off its row");
    assert_eq!(editor.read_with(cx, |editor, _| editor.blocks().len()), 0);
}

#[gpui::test]
fn several_definitions_list(cx: &mut TestAppContext) {
    let Opened {
        _dir,
        workbench,
        answers,
        cx,
        ..
    } = opened(cx);
    let editor = editor(&workbench, cx);

    // One: no list.
    peek_definition(cx);
    let peek = peek_of(&workbench, cx);
    assert_eq!(
        peek.read_with(cx, |peek, _| peek.list()),
        Vec::<gpui::SharedString>::new()
    );
    assert!(cx.debug_bounds("peek-row-0").is_none());
    cx.simulate_keystrokes("escape");
    settle(cx);

    // Two: a row each, the first shown; a click on the second shows it.
    answers
        .lock()
        .unwrap()
        .push(definition("src/other.rs", OTHER));
    peek_definition(cx);
    let peek = peek_of(&workbench, cx);
    assert_eq!(
        peek.read_with(cx, |peek, _| peek.list()),
        ["src/util.rs:21", "src/other.rs:1"]
    );
    assert_eq!(text_of(&excerpt(&peek, cx), cx), util());
    let row = cx
        .debug_bounds("peek-row-1")
        .expect("the second row is drawn");
    cx.simulate_click(row.center(), Modifiers::none());
    settle(cx);
    let shown = excerpt(&peek, cx);
    assert_eq!(text_of(&shown, cx), OTHER);
    assert_eq!(
        peek.read_with(cx, |peek, cx| peek.header(cx)).as_deref(),
        Some("other.rs 1:4")
    );
    assert!(focused(&shown, cx), "the excerpt keeps focus");
    cx.simulate_keystrokes("escape");
    settle(cx);

    // None: nothing opens, not even the definitions picker, focus stays,
    // and a toast says so.
    answers.lock().unwrap().clear();
    let before = at(&workbench, cx);
    assert!(focused(&editor, cx));
    peek_definition(cx);
    assert!(self::peek(&workbench, cx).is_none());
    assert_eq!(editor.read_with(cx, |editor, _| editor.blocks().len()), 0);
    assert_eq!(at(&workbench, cx), before);
    let modal = workbench.read_with(cx, |workbench, cx| {
        workbench.shell().read(cx).modals().read(cx).modal()
    });
    assert_eq!(modal, None);
    assert!(focused(&editor, cx));
    assert_eq!(toasts(&workbench, cx), ["No definition found for `helper`"]);
}

/// What each toast shown says, top first.
fn toasts(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    let toasts = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    toasts.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    })
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

/// `main.rs` of `rows` rows, `call` on row `at`.
fn long_main(rows: usize, at: usize, call: &str) -> String {
    (0..rows)
        .map(|row| match row == at {
            true => format!("{call}\n"),
            false => format!("// row {row}\n"),
        })
        .collect()
}

fn scenario_bottom_row(wrapped: bool) {
    let mut cx = TestAppContext::single();
    let call = match wrapped {
        false => "    util::helper();".to_owned(),
        true => format!("    util::helper(); // {}", "wide ".repeat(100)),
    };
    let main = long_main(80, 50, &call);
    let Opened {
        _dir,
        workbench,
        cx,
        ..
    } = opened_on(&mut cx, &main);
    let editor = editor(&workbench, cx);
    editor.update(cx, |editor, _| editor.model_mut().set_soft_wrap(wrapped));
    settle(cx);
    let rows = editor.read_with(cx, |editor, _| editor.viewport_rows().unwrap().1);
    // Row 50 starts on the last row in view.
    editor.update(cx, |editor, cx| {
        editor.scroll_to_buffer_line(51. - rows as f32, cx)
    });
    settle(cx);
    let call_at = main.find("helper").unwrap();
    let top = editor.read_with(cx, |editor, _| editor.viewport_rows().unwrap().0);
    assert_eq!(
        top + rows - 1,
        50,
        "wrapped {wrapped}: row 50 at the bottom"
    );

    peek_definition(cx);
    assert!(peek(&workbench, cx).is_some());
    let (bounds, clip) = reserved(&editor, cx);
    assert!(
        bounds.bottom() <= clip.bottom(),
        "wrapped {wrapped}: {bounds:?} in {clip:?}"
    );
    assert!(bounds.top() >= clip.top());
    let caret = caret_box(&editor, call_at, cx);
    assert!(
        caret.top() >= clip.top(),
        "wrapped {wrapped}: the caret's row stays"
    );
    let excerpt = excerpt(&peek_of(&workbench, cx), cx);
    let shown = caret_box(&excerpt, util().find("helper").unwrap(), cx);
    assert!(
        clip.contains(&shown.origin),
        "wrapped {wrapped}: the excerpt shows"
    );
}

#[test]
fn peek_on_the_bottom_row_scrolls_into_view() {
    scenario_bottom_row(false);
    scenario_bottom_row(true);
}

#[gpui::test]
fn a_late_answer_opens_nothing_once_the_caret_moved_or_the_text_changed(cx: &mut TestAppContext) {
    let Opened {
        _dir,
        workbench,
        cx,
        ..
    } = opened(cx);
    let editor = editor(&workbench, cx);
    let hold = |cx: &mut VisualTestContext| {
        let (release, hold) = async_channel::bounded::<()>(1);
        workbench.update(cx, |workbench, _| {
            workbench.nav_state.hold_answer = Some(hold)
        });
        peek_definition(cx);
        assert!(peek(&workbench, cx).is_none(), "held");
        release
    };
    let caret = MAIN.find("helper").unwrap() + 2;

    // The caret moved, on its row.
    let release = hold(cx);
    editor.update(cx, |editor, cx| {
        editor.model_mut().set_cursor(caret + 1);
        cx.notify();
    });
    drop(release);
    settle(cx);
    assert!(peek(&workbench, cx).is_none(), "the caret moved");
    assert!(focused(&editor, cx));

    // The text changed.
    let release = hold(cx);
    editor.update(cx, |editor, cx| {
        let end = MAIN.len();
        (editor.model_mut().buffer_mut()).edit([(end..end, "x")], fe_buffer::Origin::User);
        cx.notify();
    });
    drop(release);
    settle(cx);
    assert!(peek(&workbench, cx).is_none(), "the text changed");

    // Neither: it opens.
    let release = hold(cx);
    drop(release);
    settle(cx);
    assert!(peek(&workbench, cx).is_some());
}

#[gpui::test]
fn escape_in_the_editor_closes_it(cx: &mut TestAppContext) {
    let Opened {
        _dir,
        workbench,
        cx,
        ..
    } = opened(cx);
    let editor = editor(&workbench, cx);
    peek_definition(cx);
    // Back in the editor, on the caret's row: the peek stays.
    let on_row = caret_box(&editor, MAIN.find("util::").unwrap(), cx).center();
    cx.simulate_click(on_row, Modifiers::none());
    settle(cx);
    assert!(focused(&editor, cx));
    assert!(peek(&workbench, cx).is_some());

    cx.simulate_keystrokes("escape");
    settle(cx);
    assert!(peek(&workbench, cx).is_none());
    assert_eq!(editor.read_with(cx, |editor, _| editor.blocks().len()), 0);
    assert!(focused(&editor, cx));
}

#[gpui::test]
fn moving_the_tab_to_another_pane_closes_it(cx: &mut TestAppContext) {
    let Opened {
        _dir,
        workbench,
        cx,
        ..
    } = opened(cx);
    let editor = editor(&workbench, cx);
    peek_definition(cx);
    assert!(peek(&workbench, cx).is_some());
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.move_tab(crate::Direction::Right, window, cx)
        })
    });
    settle(cx);
    settle(cx);
    let peeks = workbench.read_with(cx, |workbench, cx| {
        (workbench.panes(cx).iter())
            .filter(|pane| pane.read(cx).peek.is_some())
            .count()
    });
    assert_eq!(peeks, 0);
    assert_eq!(editor.read_with(cx, |editor, _| editor.blocks().len()), 0);
    assert!(editor.read_with(cx, |editor, _| editor.reserved_blocks().is_empty()));
}

#[gpui::test]
fn a_folded_row_peeks_after_its_fold(cx: &mut TestAppContext) {
    let Opened {
        _dir,
        workbench,
        cx,
        ..
    } = opened(cx);
    let editor = editor(&workbench, cx);
    // `fn main() {`, folded.
    editor.update(cx, |editor, cx| {
        editor
            .model_mut()
            .set_cursor(MAIN.find("main").unwrap() + 1);
        cx.notify();
    });
    settle(cx);
    cx.simulate_keystrokes("cmd-alt-[");
    settle(cx);
    let folds = editor.read_with(cx, |editor, _| editor.model().display().folds().len());
    assert_eq!(folds, 1);

    peek_definition(cx);
    let peek = peek_of(&workbench, cx);
    let blocks = editor.read_with(cx, |editor, _| editor.blocks().to_vec());
    // The first row shown after it: the fold leaves the closing brace.
    let after = MAIN[..MAIN.find("}\n").unwrap()].matches('\n').count() as u32;
    assert_eq!((after, blocks[0].above), (4, 4), "after the fold");
    let (bounds, clip) = reserved(&editor, cx);
    assert!(clip.contains(&bounds.origin));
    let excerpt = excerpt(&peek, cx);
    let shown = caret_box(&excerpt, util().find("helper").unwrap(), cx);
    assert!(bounds.contains(&shown.origin));
}

#[gpui::test]
fn in_a_diff_it_goes_to_the_definition(cx: &mut TestAppContext) {
    let Opened {
        _dir,
        root,
        workbench,
        cx,
        ..
    } = opened(cx);
    let main = root.join("src/main.rs");
    let diff = cx.update(|window, cx| {
        let pane = workbench.read(cx).pane().clone();
        let side = |title: &str, path: Option<PathBuf>| {
            fe_gpui::DiffSide::new(
                title.to_owned(),
                fe_editor::EditorModel::new(fe_buffer::Buffer::new(MAIN), path),
            )
        };
        let diff = cx.new(|cx| {
            let (left, right) = (side("HEAD", None), side("main.rs", Some(main)));
            fe_gpui::DiffView::new("main.rs", left, right, cx)
        });
        pane.update(cx, |pane, cx| pane.add_item(Box::new(diff.clone()), cx));
        let right = diff.read(cx).right().clone();
        right.update(cx, |right, cx| {
            right
                .model_mut()
                .set_cursor(MAIN.find("helper").unwrap() + 2);
            cx.notify();
        });
        window.focus(&right.focus_handle(cx), cx);
        diff
    });
    settle(cx);
    peek_definition(cx);
    assert!(peek(&workbench, cx).is_none());
    let right = diff.read_with(cx, |diff, _| diff.right().clone());
    assert_eq!(right.read_with(cx, |right, _| right.blocks().len()), 0);
    let start = util().find("helper").unwrap();
    assert_eq!(
        at(&workbench, cx),
        (root.join("src/util.rs"), start..start + "helper".len())
    );
}

#[gpui::test]
fn a_target_edited_before_its_row_is_picked_shows_where_it_is_now(cx: &mut TestAppContext) {
    let Opened {
        _dir,
        root,
        workbench,
        answers,
        cx,
        ..
    } = opened(cx);
    // `other.rs` open in a tab of its own, then `main.rs` shown again.
    let open = |rel: &str, cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            let pane = workbench.read(cx).pane().clone();
            pane.update(cx, |pane, cx| pane.open(root.join(rel), window, cx));
        });
        settle(cx);
        let editor = self::editor(&workbench, cx);
        cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
        settle(cx);
        editor
    };
    let other = open("src/other.rs", cx);
    let (buffer, version) = other.read_with(cx, |other, _| {
        let snapshot = other.model().snapshot();
        (snapshot.id(), snapshot.version())
    });
    let mut in_other = definition("src/other.rs", OTHER);
    in_other.location.snapshot = Some((buffer, version));
    answers.lock().unwrap().push(in_other);
    let main = open("src/main.rs", cx);
    main.update(cx, |main, cx| {
        main.model_mut()
            .set_cursor(MAIN.find("helper").unwrap() + 2);
        cx.notify();
    });
    settle(cx);
    peek_definition(cx);
    let peek = peek_of(&workbench, cx);

    // Edited above its definition after the peek opened.
    other.update(cx, |other, cx| {
        (other.model_mut().buffer_mut()).edit([(0..0, "// new\n")], fe_buffer::Origin::User);
        cx.notify();
    });
    settle(cx);
    let row = cx
        .debug_bounds("peek-row-1")
        .expect("the second row is drawn");
    cx.simulate_click(row.center(), Modifiers::none());
    settle(cx);
    let shown = excerpt(&peek, cx);
    let now = format!("// new\n{OTHER}");
    assert_eq!(text_of(&shown, cx), now);
    let start = now.find("helper").unwrap();
    let selection = shown.read_with(cx, |shown, _| shown.model().selections()[0].range.clone());
    assert_eq!(selection, start..start + "helper".len());
    assert_eq!(
        peek.read_with(cx, |peek, cx| peek.header(cx)).as_deref(),
        Some("other.rs 2:4")
    );
}

#[gpui::test]
fn a_short_view_keeps_the_definition_in_view(cx: &mut TestAppContext) {
    let Opened {
        _dir,
        workbench,
        cx,
        ..
    } = opened(cx);
    let editor = editor(&workbench, cx);
    let rows = editor.read_with(cx, |editor, _| editor.viewport_rows().unwrap().1);
    // About five rows of text.
    let height = cx.update(|window, _| window.viewport_size().height);
    let row = caret_box(&editor, 0, cx).size.height;
    cx.simulate_resize(size(px(1000.), height - row * (rows as f32 - 5.)));
    settle(cx);
    let rows = editor.read_with(cx, |editor, _| editor.viewport_rows().unwrap().1);
    assert!(rows <= 6, "{rows} rows");

    peek_definition(cx);
    let excerpt = excerpt(&peek_of(&workbench, cx), cx);
    let (top, shown) = excerpt.read_with(cx, |excerpt, _| excerpt.viewport_rows().unwrap());
    assert!(
        top <= 20 && 20 < top + shown.max(1),
        "row 20 in {top}+{shown}"
    );
    let (bounds, clip) = reserved(&editor, cx);
    let at = caret_box(&excerpt, util().find("helper").unwrap(), cx);
    let shown = bounds.intersect(&clip);
    assert!(shown.contains(&at.origin), "{at:?} in {shown:?}");
}

#[gpui::test]
fn an_append_to_the_last_row_closes_it(cx: &mut TestAppContext) {
    let main = "mod util;\n\nfn main() {}\n    util::helper();";
    let Opened {
        _dir,
        workbench,
        cx,
        ..
    } = opened_on(cx, main);
    let editor = editor(&workbench, cx);
    peek_definition(cx);
    let blocks = editor.read_with(cx, |editor, _| editor.blocks().to_vec());
    assert_eq!(blocks[0].above, 4, "after the last row");
    editor.update(cx, |editor, cx| {
        let end = main.len();
        (editor.model_mut().buffer_mut()).edit([(end..end, "x")], fe_buffer::Origin::User);
        cx.notify();
    });
    settle(cx);
    assert!(peek(&workbench, cx).is_none());
    assert_eq!(editor.read_with(cx, |editor, _| editor.blocks().len()), 0);
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

fn insert_at_start(editor: &Entity<EditorView>, text: &str, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, cx| {
        (editor.model_mut().buffer_mut()).edit([(0..0, text)], fe_buffer::Origin::User);
        cx.notify();
    });
    settle(cx);
}

#[gpui::test]
fn a_target_opened_and_edited_after_the_peek_shows_where_it_is_now(cx: &mut TestAppContext) {
    let Opened {
        _dir,
        root,
        workbench,
        answers,
        cx,
        ..
    } = opened(cx);
    answers
        .lock()
        .unwrap()
        .push(definition("src/other.rs", OTHER));
    peek_definition(cx);
    let peek = peek_of(&workbench, cx);

    // `other.rs` opened and edited above its definition, then `main.rs`
    // shown again, its peek with it.
    let other = show_file(&workbench, &root, "src/other.rs", cx);
    insert_at_start(&other, "// new\n// rows\n", cx);
    show_file(&workbench, &root, "src/main.rs", cx);
    assert!(self::peek(&workbench, cx).is_some());
    let row = cx
        .debug_bounds("peek-row-1")
        .expect("the second row is drawn");
    cx.simulate_click(row.center(), Modifiers::none());
    settle(cx);
    let shown = excerpt(&peek, cx);
    let now = format!("// new\n// rows\n{OTHER}");
    assert_eq!(text_of(&shown, cx), now);
    let start = now.find("helper").unwrap();
    let selection = shown.read_with(cx, |shown, _| shown.model().selections()[0].range.clone());
    assert_eq!(selection, start..start + "helper".len());
}

#[gpui::test]
fn enter_maps_a_read_excerpt_onto_the_file_opened_since(cx: &mut TestAppContext) {
    let Opened {
        _dir,
        root,
        workbench,
        cx,
        ..
    } = opened(cx);
    peek_definition(cx);
    let excerpt = excerpt(&peek_of(&workbench, cx), cx);
    // `util.rs` opened and edited above the definition after it was read.
    let util_editor = show_file(&workbench, &root, "src/util.rs", cx);
    insert_at_start(&util_editor, "// new\n", cx);
    show_file(&workbench, &root, "src/main.rs", cx);
    cx.update(|window, cx| window.focus(&excerpt.focus_handle(cx), cx));
    settle(cx);
    cx.simulate_keystrokes("enter");
    settle(cx);
    let now = format!("// new\n{}", util());
    let start = now.find("helper").unwrap();
    assert_eq!(
        at(&workbench, cx),
        (root.join("src/util.rs"), start..start + "helper".len())
    );
}

#[gpui::test]
fn shrinking_the_view_refits_the_peek(cx: &mut TestAppContext) {
    let Opened {
        _dir,
        workbench,
        cx,
        ..
    } = opened(cx);
    let editor = editor(&workbench, cx);
    peek_definition(cx);
    let peek = peek_of(&workbench, cx);
    let rows = editor.read_with(cx, |editor, _| editor.viewport_rows().unwrap().1);
    let height = cx.update(|window, _| window.viewport_size().height);
    let row = caret_box(&editor, 0, cx).size.height;
    cx.simulate_resize(size(px(1000.), height - row * (rows as f32 - 5.)));
    settle(cx);
    settle(cx);
    let shown = editor.read_with(cx, |editor, _| editor.viewport_rows().unwrap().1);
    let blocks = editor.read_with(cx, |editor, _| editor.blocks().to_vec());
    assert_eq!(blocks[0].rows, shown - 1, "{shown} rows shown");
    let (bounds, clip) = reserved(&editor, cx);
    assert!(bounds.bottom() <= clip.bottom() && bounds.top() >= clip.top());
    let excerpt = excerpt(&peek, cx);
    let at = caret_box(&excerpt, util().find("helper").unwrap(), cx);
    assert!(
        bounds.intersect(&clip).contains(&at.origin),
        "{at:?} in {bounds:?}"
    );
}

#[gpui::test]
fn folding_moves_the_peek_to_the_row_shown_after_the_caret(cx: &mut TestAppContext) {
    let Opened {
        _dir,
        workbench,
        cx,
        ..
    } = opened(cx);
    let editor = editor(&workbench, cx);
    editor.update(cx, |editor, cx| {
        editor
            .model_mut()
            .set_cursor(MAIN.find("main").unwrap() + 1);
        cx.notify();
    });
    settle(cx);
    let above = |cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, _| {
            (editor.blocks().iter())
                .map(|block| block.above)
                .collect::<Vec<_>>()
        })
    };
    // Opened unfolded, then folded.
    peek_definition(cx);
    assert_eq!(above(cx), [3]);
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    cx.simulate_keystrokes("cmd-alt-[");
    settle(cx);
    assert!(peek(&workbench, cx).is_some());
    assert_eq!(above(cx), [4], "under the fold");
    // Unfolded again.
    cx.simulate_keystrokes("cmd-alt-]");
    settle(cx);
    assert!(peek(&workbench, cx).is_some());
    assert_eq!(above(cx), [3], "under the caret's row");
    let (bounds, _) = reserved(&editor, cx);
    let below = caret_box(&editor, MAIN.find("    util").unwrap(), cx);
    assert_eq!(below.top(), bounds.bottom(), "the body under the peek");
}

#[gpui::test]
fn a_definition_far_along_its_row_is_scrolled_to(cx: &mut TestAppContext) {
    let Opened {
        _dir,
        root,
        workbench,
        answers,
        cx,
        ..
    } = opened(cx);
    let wide = format!("// top\n{}fn helper() {{}}\n", " ".repeat(400));
    fs::write(root.join("src/wide.rs"), &wide).unwrap();
    *answers.lock().unwrap() = vec![definition("src/wide.rs", &wide)];
    let editor = editor(&workbench, cx);
    peek_definition(cx);
    let excerpt = excerpt(&peek_of(&workbench, cx), cx);
    let (bounds, clip) = reserved(&editor, cx);
    let at = caret_box(&excerpt, wide.find("helper").unwrap(), cx);
    assert!(
        bounds.intersect(&clip).contains(&at.origin),
        "{at:?} in {bounds:?}"
    );
    let top = excerpt.read_with(cx, |excerpt, _| excerpt.viewport_rows().unwrap().0);
    assert_eq!(top, 0, "the row above it still in view");
}

/// Holds the next definitions answer until the sender is dropped.
fn hold_answer(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> async_channel::Sender<()> {
    let (release, hold) = async_channel::bounded::<()>(1);
    workbench.update(cx, |workbench, _| {
        workbench.nav_state.hold_answer = Some(hold)
    });
    release
}

#[gpui::test]
fn dismissing_drops_a_peek_still_asked_for(cx: &mut TestAppContext) {
    let Opened {
        _dir,
        workbench,
        cx,
        ..
    } = opened(cx);
    for button in [false, true] {
        peek_definition(cx);
        assert!(peek(&workbench, cx).is_some());
        // Asked again, at the same caret, then dismissed before it lands.
        let release = hold_answer(&workbench, cx);
        peek_definition(cx);
        match button {
            false => cx.simulate_keystrokes("escape"),
            true => {
                let close = cx.debug_bounds("peek-close").expect("drawn");
                cx.simulate_click(close.center(), Modifiers::none());
            }
        }
        settle(cx);
        assert!(peek(&workbench, cx).is_none(), "button {button}");
        drop(release);
        settle(cx);
        assert!(
            peek(&workbench, cx).is_none(),
            "button {button}: not reopened"
        );
    }
}

#[gpui::test]
fn a_markdown_preview_hides_the_peek_and_drops_a_late_one(cx: &mut TestAppContext) {
    let Opened {
        _dir,
        root,
        workbench,
        cx,
        ..
    } = opened(cx);
    let text = "# Notes\n\nSee helper here.\n\nMore.\n";
    fs::write(root.join("src/notes.md"), text).unwrap();
    let notes = show_file(&workbench, &root, "src/notes.md", cx);
    notes.update(cx, |notes, cx| {
        notes
            .model_mut()
            .set_cursor(text.find("helper").unwrap() + 2);
        cx.notify();
    });
    settle(cx);
    peek_definition(cx);
    assert!(peek(&workbench, cx).is_some());
    assert!(cx.debug_bounds("peek-close").is_some());

    toggle_preview(cx);
    let previewed = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |workbench, cx| {
            let pane = workbench.pane().read(cx);
            pane.active_tab().is_some_and(|tab| tab.preview().is_some())
        })
    };
    assert!(previewed(cx));
    assert!(cx.debug_bounds("peek-close").is_none(), "hidden");
    toggle_preview(cx);
    assert!(!previewed(cx));
    assert!(cx.debug_bounds("peek-close").is_some(), "shown again");

    // A peek asked for before the preview showed doesn't open under it.
    cx.update(|window, cx| window.focus(&notes.focus_handle(cx), cx));
    cx.simulate_keystrokes("escape");
    settle(cx);
    assert!(peek(&workbench, cx).is_none());
    let release = hold_answer(&workbench, cx);
    peek_definition(cx);
    toggle_preview(cx);
    assert!(previewed(cx));
    drop(release);
    settle(cx);
    assert!(peek(&workbench, cx).is_none());
}

/// Shows or hides the active editor's Markdown preview in its place.
fn toggle_preview(cx: &mut VisualTestContext) {
    let context: fe_command::ContextStack = ["workbench", "editor"]
        .iter()
        .map(|frame| fe_command::ContextFrame::new(*frame))
        .collect();
    let invocation = fe_command::Invocation::of("markdown.toggle_preview").unwrap();
    let run = fe_gpui::commands::RunCommand(invocation.context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    settle(cx);
}

#[gpui::test]
fn a_renamed_target_opens_where_it_is_now(cx: &mut TestAppContext) {
    let Opened {
        _dir,
        root,
        workbench,
        answers,
        cx,
        ..
    } = opened(cx);
    let other = show_file(&workbench, &root, "src/other.rs", cx);
    *answers.lock().unwrap() = vec![definition("src/other.rs", OTHER)];
    let main = show_file(&workbench, &root, "src/main.rs", cx);
    main.update(cx, |main, cx| {
        main.model_mut()
            .set_cursor(MAIN.find("helper").unwrap() + 2);
        cx.notify();
    });
    settle(cx);
    peek_definition(cx);
    let peek = peek_of(&workbench, cx);
    // Saved elsewhere, as Save As does.
    let renamed = root.join("src/renamed.rs");
    fs::write(&renamed, OTHER).unwrap();
    other.update(cx, |other, cx| {
        other.model_mut().set_path(Some(renamed.clone()));
        cx.notify();
    });
    settle(cx);
    assert_eq!(
        peek.read_with(cx, |peek, cx| peek.header(cx)).as_deref(),
        Some("renamed.rs 1:4")
    );
    let opening = peek.read_with(cx, |peek, cx| peek.opening(cx).map(|opening| opening.path));
    assert_eq!(opening, Some(renamed));
}

#[gpui::test]
fn one_baseline_per_file_and_large_files_unread(cx: &mut TestAppContext) {
    let Opened {
        _dir,
        root,
        workbench,
        answers,
        cx,
        ..
    } = opened(cx);
    let util = util();
    let mut second = definition("src/util.rs", &util);
    second.location.range = 0..2;
    second.location.row = 0;
    answers.lock().unwrap().push(second);
    answers
        .lock()
        .unwrap()
        .push(definition("src/other.rs", OTHER));
    peek_definition(cx);
    let peek = peek_of(&workbench, cx);
    let mut read: Vec<PathBuf> =
        peek.read_with(cx, |peek, _| peek.baselines.keys().cloned().collect());
    read.sort();
    assert_eq!(read, [root.join("src/other.rs"), root.join("src/util.rs")]);
    cx.simulate_keystrokes("escape");
    settle(cx);

    // As large as the pane opens in a viewer: neither shown nor read.
    crate::loader::test_support::lower_threshold(&root, 100, false);
    peek_definition(cx);
    let peek = peek_of(&workbench, cx);
    let (excerpt, error, read) = peek.read_with(cx, |peek, _| {
        (
            peek.excerpt().is_some(),
            peek.error.clone(),
            peek.baselines.len(),
        )
    });
    assert!(!excerpt);
    assert_eq!(
        error.as_deref(),
        Some("Couldn't show util.rs: too large to show here")
    );
    assert_eq!(read, 1, "other.rs only");
}

#[test]
fn baselines_stop_at_their_budget() {
    let dir = tempfile::tempdir().unwrap();
    let small: Vec<PathBuf> = (0..10)
        .map(|n| {
            let path = dir.path().join(format!("f{n}.rs"));
            fs::write(&path, "x".repeat(100)).unwrap();
            path
        })
        .collect();
    let read = |paths: Vec<PathBuf>, budget| {
        (super::baselines(paths, budget).into_iter())
            .map(|(path, _)| path)
            .collect::<Vec<_>>()
    };
    assert_eq!(read(small.clone(), 350), small[..3]);
    assert_eq!(read(small.clone(), 10_000), small);

    // Files it reads no baseline for, first, take none of the budget.
    let large = dir.path().join("large.rs");
    let over = fe_editor::text_diff::MAX_DIFF_BYTES + 1;
    fs::write(&large, "x".repeat(over)).unwrap();
    let missing = dir.path().join("missing.rs");
    let paths = [vec![large, missing], small.clone()].concat();
    assert_eq!(read(paths, 350), small[..3]);
}

#[gpui::test]
fn esc_before_the_first_answer_cancels_it(cx: &mut TestAppContext) {
    let Opened {
        _dir,
        workbench,
        cx,
        ..
    } = opened(cx);
    let editor = editor(&workbench, cx);
    let release = hold_answer(&workbench, cx);
    peek_definition(cx);
    assert!(peek(&workbench, cx).is_none(), "held");
    cx.simulate_keystrokes("escape");
    settle(cx);
    drop(release);
    settle(cx);
    assert!(peek(&workbench, cx).is_none(), "cancelled");
    assert!(focused(&editor, cx));
    // Escape is the editor's own again; a peek asked for now opens.
    let flagged = |cx: &mut VisualTestContext| {
        cx.update(|window, _| {
            (window.context_stack().iter()).any(|context| context.contains(super::SHOWN))
        })
    };
    assert!(!flagged(cx));
    peek_definition(cx);
    assert!(peek(&workbench, cx).is_some());
}

#[gpui::test]
fn an_empty_or_failed_answer_closes_an_open_peek(cx: &mut TestAppContext) {
    let Opened {
        _dir,
        workbench,
        answers,
        fails,
        cx,
        ..
    } = opened(cx);
    let editor = editor(&workbench, cx);
    // Empty, asked from the peek's excerpt: it closes, the editor focused.
    peek_definition(cx);
    assert!(peek(&workbench, cx).is_some());
    let empty = answers.lock().unwrap().drain(..).collect::<Vec<_>>();
    peek_definition(cx);
    assert!(peek(&workbench, cx).is_none(), "empty");
    assert_eq!(editor.read_with(cx, |editor, _| editor.blocks().len()), 0);
    assert!(focused(&editor, cx));
    assert_eq!(toasts(&workbench, cx), ["No definition found for `helper`"]);

    // Failed, asked from the editor: it closes, focus stays.
    *answers.lock().unwrap() = empty;
    peek_definition(cx);
    assert!(peek(&workbench, cx).is_some());
    let navigation = workbench.read_with(cx, |workbench, _| workbench.navigation().clone());
    navigation.remove(fe_nav::SearchNavigation::NAME);
    fails.store(true, Ordering::Relaxed);
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    settle(cx);
    peek_definition(cx);
    assert!(peek(&workbench, cx).is_none(), "failed");
    assert_eq!(editor.read_with(cx, |editor, _| editor.blocks().len()), 0);
    assert!(focused(&editor, cx));
    let shown = toasts(&workbench, cx);
    assert!(
        shown
            .last()
            .is_some_and(|toast| toast.starts_with("Go to definition failed")),
        "{shown:?}"
    );
}

#[gpui::test]
fn a_row_picked_before_its_baseline_shows_the_target_once_it_loads(cx: &mut TestAppContext) {
    let Opened {
        _dir,
        root,
        workbench,
        answers,
        cx,
        ..
    } = opened(cx);
    answers
        .lock()
        .unwrap()
        .push(definition("src/other.rs", OTHER));
    let (release, hold) = async_channel::bounded::<()>(1);
    super::HOLD_BASELINES.with(|held| *held.borrow_mut() = Some(hold));
    peek_definition(cx);
    let peek = peek_of(&workbench, cx);
    assert!(
        peek.read_with(cx, |peek, _| peek.baselines.is_empty()),
        "held"
    );

    // `other.rs` opened and edited above its definition, its row picked,
    // all before the baselines are read.
    let other = show_file(&workbench, &root, "src/other.rs", cx);
    insert_at_start(&other, "// new\n// rows\n", cx);
    show_file(&workbench, &root, "src/main.rs", cx);
    let row = cx
        .debug_bounds("peek-row-1")
        .expect("the second row is drawn");
    cx.simulate_click(row.center(), Modifiers::none());
    settle(cx);
    assert!(
        peek.read_with(cx, |peek, _| peek.baselines.is_empty()),
        "still held"
    );

    drop(release);
    settle(cx);
    let shown = excerpt(&peek, cx);
    let now = format!("// new\n// rows\n{OTHER}");
    assert_eq!(text_of(&shown, cx), now);
    let start = now.find("helper").unwrap();
    let selection = shown.read_with(cx, |shown, _| shown.model().selections()[0].range.clone());
    assert_eq!(selection, start..start + "helper".len());
}

/// Esc in vim's insert session ends it and cancels a peek still asked
/// for too.
#[gpui::test]
fn esc_ending_vim_insert_cancels_a_pending_peek(cx: &mut TestAppContext) {
    let Opened {
        _dir,
        workbench,
        cx,
        ..
    } = opened(cx);
    // Vim's insert session, the caret back where it asked from.
    cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, true));
    settle(cx);
    cx.simulate_keystrokes("a");
    settle(cx);
    let release = hold_answer(&workbench, cx);
    peek_definition(cx);
    cx.simulate_keystrokes("escape");
    settle(cx);
    let editor = editor(&workbench, cx);
    let mode = editor.read_with(cx, |editor, _| editor.vim_mode());
    assert_eq!(mode, Some("normal"), "insert ended");
    drop(release);
    settle(cx);
    assert!(peek(&workbench, cx).is_none(), "vim insert: cancelled");
}

/// Esc closing the completion menu, which a server fills, cancels a peek
/// still asked for too.
#[gpui::test]
fn esc_closing_the_completion_menu_cancels_a_pending_peek(cx: &mut TestAppContext) {
    use fe_lsp::fake::{Reply, Scripted};
    use lsp_types::request::{Completion, Request as _};

    let reply: lsp_types::LSPAny = r#"{
        "isIncomplete": false,
        "items": [{
            "label": "main_loop",
            "textEdit": {
                "range": { "start": { "line": 0, "character": 3 }, "end": { "line": 0, "character": 7 } },
                "newText": "main_loop"
            }
        }]
    }"#
    .parse()
    .unwrap();
    let mut s = crate::tests::lsp::Lsp::new(cx);
    s.fake
        .set_reply(Completion::METHOD, Scripted::now(Reply::Result(reply)));
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake
        .wait_for_document(&main, "didOpen", |r| r.is_some_and(|r| r.open));
    // Ahead of the server's own definitions.
    let text = "fn main() {}\n";
    let start = text.find("main").unwrap();
    let mut answer = definition("src/main.rs", "helper");
    answer.location.range = start..start + "main".len();
    answer.location.row = 0;
    let answers = Arc::new(Mutex::new(vec![answer]));
    let navigation = s.workbench.read_with(&s.cx, |w, _| w.navigation().clone());
    let fails = Arc::new(AtomicBool::new(false));
    navigation.add(Arc::new(Fake(answers, fails)), 1000);
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("main.rs is shown");
    s.cx.update(|window, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().set_cursor("fn main".len());
            cx.notify();
        });
        window.focus(&editor.focus_handle(cx), cx);
    });
    s.draw();
    // A peek opens there when nothing cancels it.
    s.cx.simulate_keystrokes("alt-f12");
    s.draw();
    assert!(peek(&s.workbench, &mut s.cx).is_some());
    s.cx.simulate_keystrokes("escape");
    s.draw();
    assert!(peek(&s.workbench, &mut s.cx).is_none());

    let release = hold_answer(&s.workbench, &mut s.cx);
    s.cx.simulate_keystrokes("alt-f12");
    s.cx.simulate_keystrokes("ctrl-space");
    s.until("the menu", |s| {
        editor.read_with(&s.cx, |view, _| view.completion_shown())
    });
    s.cx.simulate_keystrokes("escape");
    s.draw();
    assert!(
        !editor.read_with(&s.cx, |view, _| view.completion_shown()),
        "closed"
    );
    drop(release);
    s.draw();
    assert!(peek(&s.workbench, &mut s.cx).is_none(), "cancelled");
}
