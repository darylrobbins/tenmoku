//! Spelling squiggles in a real workbench, on a paused clock, with a speller
//! that says which words are wrong and records what it's asked.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::{Settings, SettingsEvent, settings::SpellScope};
use fe_editor::decorations::{Decoration, DecorationKind, Tint};
use fe_gpui::{
    EditorSettings, EditorView,
    commands::{HostBlock, HostBlockKind, RunCommand},
};
use gpui::{BackgroundExecutor, Entity, Focusable as _, TestAppContext, VisualTestContext};
use tempfile::TempDir;

use crate::{
    Launch, StatusSide, Workbench,
    dock::problems_panel::ITEM,
    pane_group::Direction,
    spell::{Dictionary, NoSpeller, Speller},
    spell_squiggles::{Landing, MARGIN, Piece, REST, add_rows, missing_rows},
};

/// Every word asked about, and whether each ask was on the main thread.
#[derive(Clone, Default)]
struct Asked(Arc<Mutex<Vec<(String, bool)>>>);

impl Asked {
    fn words(&self) -> Vec<String> {
        let asked = self.0.lock().unwrap();
        asked.iter().map(|(word, _)| word.clone()).collect()
    }

    fn any_on_main_thread(&self) -> bool {
        self.0.lock().unwrap().iter().any(|(_, main)| *main)
    }

    fn clear(&self) {
        self.0.lock().unwrap().clear();
    }
}

/// Finds `wrong` misspelled, and every other word correct.
struct Fake {
    wrong: Vec<&'static str>,
    asked: Asked,
    executor: BackgroundExecutor,
}

impl Speller for Fake {
    fn is_misspelled(&self, word: &str, _language: &str) -> bool {
        let main = self.executor.is_main_thread();
        self.asked.0.lock().unwrap().push((word.to_owned(), main));
        self.wrong.contains(&word)
    }

    fn suggest(&self, _word: &str, _language: &str) -> Vec<String> {
        Vec::new()
    }
}

struct Files(TempDir);

impl Files {
    fn new() -> Self {
        Self(tempfile::tempdir().unwrap())
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.path().join(name);
        fs::write(&path, text).unwrap();
        path
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Long enough for whatever is to be checked to be, retries included.
fn settle(cx: &mut VisualTestContext) {
    draw(cx);
    for _ in 0..6 {
        cx.executor().advance_clock(REST);
        cx.run_until_parked();
        draw(cx);
    }
}

fn workbench(cx: &mut TestAppContext) -> (Entity<Workbench>, &mut VisualTestContext) {
    let launch = Launch {
        root: None,
        file: None,
    };
    let (workbench, cx, _) = crate::tests::open(cx, launch);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    (workbench, cx)
}

/// A speller finding `wrong` misspelled, given to `workbench`.
fn give_speller(
    workbench: &Entity<Workbench>,
    wrong: &[&'static str],
    cx: &mut VisualTestContext,
) -> Asked {
    let asked = Asked::default();
    let speller = Fake {
        wrong: wrong.to_vec(),
        asked: asked.clone(),
        executor: cx.executor(),
    };
    workbench.update(cx, |workbench, cx| workbench.set_speller(speller, cx));
    asked
}

/// Shows `path` in the active pane, drawn, and returns its editor.
fn open(
    workbench: &Entity<Workbench>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = path.to_owned();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("the file is shown")
}

fn run(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// Replaces the first `old` in `editor` with `new`, as one edit.
fn replace(editor: &Entity<EditorView>, old: &str, new: &str, cx: &mut VisualTestContext) {
    editor.update(cx, |view, cx| {
        let start = view.model().text().to_string().find(old).expect("the text");
        view.model_mut()
            .select_range(start..start + old.len(), false);
        view.model_mut().insert(new);
        cx.notify();
    });
}

/// `editor`'s underlines.
fn underlines(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<Decoration> {
    editor.read_with(cx, |view, _| {
        let model = view.model();
        (model.decorations(0..model.text().len()).into_iter())
            .filter(|it| matches!(it.kind, DecorationKind::Underline { .. }))
            .collect()
    })
}

/// The text under `editor`'s squiggles.
fn squiggled(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<String> {
    let underlines = underlines(editor, cx);
    editor.read_with(cx, |view, _| {
        let text = view.model().text().to_string();
        (underlines.iter())
            .map(|it| text[it.range.clone()].to_owned())
            .collect()
    })
}

fn checks(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    workbench.read_with(cx, |workbench, _| workbench.spelling.checks)
}

/// Applies `editor` settings as a reload does.
fn reload(workbench: &Entity<Workbench>, editor: EditorSettings, cx: &mut VisualTestContext) {
    let event = SettingsEvent {
        settings: Arc::new(Settings {
            editor,
            ..Settings::default()
        }),
        previous: Arc::new(Settings::default()),
        diagnostics: Vec::new(),
    };
    workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
    cx.run_until_parked();
}

#[gpui::test]
fn misspelled_words_get_an_underline(cx: &mut TestAppContext) {
    let files = Files::new();
    let path = files.write("main.rs", "// teh quick\nfn teh() {}\n");
    let (workbench, cx) = workbench(cx);
    let asked = give_speller(&workbench, &["teh"], cx);
    let editor = open(&workbench, &path, cx);
    settle(cx);

    assert_eq!(
        underlines(&editor, cx),
        [Decoration::new(
            3..6,
            DecorationKind::Underline { wavy: true },
            Tint::Information,
        )],
        "the comment's word, not the code's"
    );
    assert!(asked.words().contains(&"quick".to_owned()));

    // It follows an edit until the next check, which finds what's there now.
    replace(&editor, "quick", "slow", cx);
    draw(cx);
    assert_eq!(squiggled(&editor, cx), ["teh"]);
    replace(&editor, "teh", "the", cx);
    settle(cx);
    assert_eq!(underlines(&editor, cx), []);
}

#[gpui::test]
fn a_deleted_word_leaves_no_underline(cx: &mut TestAppContext) {
    let files = Files::new();
    let path = files.write("notes.txt", "one teh two\n");
    let (workbench, cx) = workbench(cx);
    give_speller(&workbench, &["teh"], cx);
    let editor = open(&workbench, &path, cx);
    settle(cx);
    assert_eq!(squiggled(&editor, cx), ["teh"]);

    // Gone before the next check, which is a rest away.
    replace(&editor, "teh", "", cx);
    draw(cx);
    assert_eq!(underlines(&editor, cx), []);
    let found = workbench.read_with(cx, |workbench, cx| workbench.misspellings(&editor, cx));
    assert!(found.is_empty());
}

#[gpui::test]
fn checks_are_debounced_and_off_main_thread(cx: &mut TestAppContext) {
    assert_eq!(REST.as_millis(), 400);
    let files = Files::new();
    let path = files.write("notes.txt", "start\n");
    let (workbench, cx) = workbench(cx);
    let asked = give_speller(&workbench, &["teh"], cx);
    let editor = open(&workbench, &path, cx);
    settle(cx);
    let before = checks(&workbench, cx);
    assert_eq!(before, 1);

    // Ten edits within a rest of each other.
    let step = REST / 10;
    for _ in 0..10 {
        replace(&editor, "start", "start teh", cx);
        draw(cx);
        replace(&editor, "start teh", "start", cx);
        draw(cx);
        cx.executor().advance_clock(step);
        cx.run_until_parked();
        assert_eq!(checks(&workbench, cx), before, "still being edited");
    }
    // The rest counts from the last edit.
    cx.executor().advance_clock(REST - step - step / 2);
    cx.run_until_parked();
    assert_eq!(checks(&workbench, cx), before);
    cx.executor().advance_clock(step);
    cx.run_until_parked();
    assert_eq!(
        checks(&workbench, cx),
        before + 1,
        "one check for ten edits"
    );
    settle(cx);
    assert_eq!(checks(&workbench, cx), before + 1);

    // The speller was only asked in the background.
    assert!(asked.words().contains(&"start".to_owned()));
    assert!(!asked.any_on_main_thread());

    // A result for text that's gone is dropped, even with the edit not drawn
    // yet, when the model's snapshot hasn't caught up with its buffer. The clock
    // runs a check through as it fires, so this stages what it would land.
    let old = editor.read_with(cx, |view, _| view.model().snapshot().version());
    let id = editor.entity_id();
    let start_word = 0..5;
    let landing = |version| Landing {
        version,
        pieces: vec![Piece {
            rows: 0..1,
            bytes: 0..6,
            spellable: Vec::from_iter([start_word.clone()]),
        }],
        found: Vec::from_iter([start_word.clone()]),
    };
    cx.update(|_, cx| {
        editor.update(cx, |view, _| {
            view.model_mut().select_range(0..0, false);
            view.model_mut().insert("x");
        });
        workbench.update(cx, |workbench, cx| {
            workbench.spell_check_landed(id, landing(old), cx)
        });
    });
    cx.run_until_parked();
    assert_eq!(underlines(&editor, cx), []);

    // A later edit drawn, the old version is as stale.
    replace(&editor, "xstart", "started", cx);
    settle(cx);
    workbench.update(cx, |workbench, cx| {
        workbench.spell_check_landed(id, landing(old), cx)
    });
    cx.run_until_parked();
    assert_eq!(underlines(&editor, cx), []);

    // The current version's does land, so those two weren't dropped for
    // anything else.
    let now = editor.read_with(cx, |view, _| view.model().snapshot().version());
    workbench.update(cx, |workbench, cx| {
        workbench.spell_check_landed(id, landing(now), cx)
    });
    cx.run_until_parked();
    assert_eq!(underlines(&editor, cx).len(), 1);
}

/// A word of its own for each row: one the tokenizer takes.
fn row_word(row: u32) -> String {
    let letter = |n: u32| char::from(b'a' + (n % 26) as u8);
    let mut word = String::from("zq");
    word.extend([letter(row / 676), letter(row / 26), letter(row)]);
    word
}

fn rows_text(count: u32, each: impl Fn(&str) -> String) -> String {
    (0..count).map(|row| each(&row_word(row))).collect()
}

/// How many times each row was asked about.
fn asked_rows(asked: &Asked, rows: u32) -> HashMap<u32, usize> {
    let by_word: HashMap<String, u32> = (0..rows).map(|row| (row_word(row), row)).collect();
    let mut counts = HashMap::new();
    for word in asked.words() {
        *counts.entry(by_word[&word]).or_default() += 1;
    }
    counts
}

fn each_once(rows: impl IntoIterator<Item = u32>) -> HashMap<u32, usize> {
    rows.into_iter().map(|row| (row, 1)).collect()
}

/// The rows `editor` shows, as (first, how many): from what its frame laid out,
/// for rows that aren't wrapped or folded or under a block.
fn viewport(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> (u32, u32) {
    let laid_out = laid_out(editor, cx);
    let top = *laid_out.first().expect("drawn");
    (top, laid_out.len() as u32)
}

/// The buffer rows `editor`'s last frame laid out.
fn laid_out(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<u32> {
    editor.read_with(cx, |view, _| {
        (0..view.model().text().row_count())
            .filter(|&row| view.shows_buffer_row(row) == Some(true))
            .collect()
    })
}

#[gpui::test]
fn only_the_viewport_plus_margin_is_checked(cx: &mut TestAppContext) {
    assert_eq!(MARGIN, 50);
    const ROWS: u32 = 5000;
    let files = Files::new();
    let path = files.write("words.txt", &rows_text(ROWS, |word| format!("{word}\n")));
    let (workbench, cx) = workbench(cx);
    let asked = give_speller(&workbench, &[], cx);
    let editor = open(&workbench, &path, cx);
    settle(cx);

    let (top, shown) = viewport(&editor, cx);
    assert_eq!(top, 0);
    let first = 0..shown + MARGIN;
    assert_eq!(asked_rows(&asked, ROWS), each_once(first.clone()));

    // Scrolling far asks about what it exposes, the new rows once each.
    editor.update(cx, |view, cx| view.scroll_to_buffer_line(1000., cx));
    settle(cx);
    let (top, shown) = viewport(&editor, cx);
    assert_eq!(top, 1000);
    let second = top - MARGIN..top + shown + MARGIN;
    assert_eq!(
        asked_rows(&asked, ROWS),
        each_once(first.clone().chain(second.clone()))
    );

    // Scrolling back to what's checked asks nothing.
    let before = checks(&workbench, cx);
    editor.update(cx, |view, cx| view.scroll_to_buffer_line(0., cx));
    settle(cx);
    assert_eq!(checks(&workbench, cx), before);

    // A little past what's checked asks about only those rows.
    editor.update(cx, |view, cx| view.scroll_to_buffer_line(10., cx));
    settle(cx);
    let (_, shown) = viewport(&editor, cx);
    let third = first.end..10 + shown + MARGIN;
    assert!(!third.is_empty());
    assert_eq!(
        asked_rows(&asked, ROWS),
        each_once(first.chain(second).chain(third))
    );

    // An edit forgets them, and what's in view is asked about again.
    asked.clear();
    replace(&editor, &row_word(10), "edited", cx);
    settle(cx);
    let (top, shown) = viewport(&editor, cx);
    let words = asked.words();
    assert!(words.contains(&"edited".to_owned()));
    assert_eq!(
        words.len() as u32,
        top.saturating_sub(MARGIN) + shown + MARGIN + top
    );
}

#[gpui::test]
fn blocks_in_the_view_leave_fewer_rows_to_check(cx: &mut TestAppContext) {
    const ROWS: u32 = 1000;
    let files = Files::new();
    let path = files.write("words.txt", &rows_text(ROWS, |word| format!("{word}\n")));
    let (workbench, cx) = workbench(cx);
    let asked = give_speller(&workbench, &[], cx);
    let editor = open(&workbench, &path, cx);
    let fits = editor.read_with(cx, |view, _| view.viewport_rows().expect("drawn").1);
    let blocks = vec![HostBlock {
        above: 5,
        rows: fits - 10,
        kind: HostBlockKind::Filler,
        id: 1,
    }];
    editor.update(cx, |view, cx| view.set_blocks(blocks, cx));
    settle(cx);

    let shown = laid_out(&editor, cx);
    assert!(
        shown.len() as u32 <= 12,
        "{} rows beside the block",
        shown.len()
    );
    let end = shown.last().unwrap() + 1 + MARGIN;
    assert_eq!(asked_rows(&asked, ROWS), each_once(0..end));
}

#[gpui::test]
fn rows_in_a_fold_are_not_checked(cx: &mut TestAppContext) {
    const ROWS: u32 = 3000;
    let files = Files::new();
    let path = files.write("words.txt", &rows_text(ROWS, |word| format!("{word}\n")));
    let (workbench, cx) = workbench(cx);
    let asked = give_speller(&workbench, &[], cx);
    let editor = open(&workbench, &path, cx);
    editor.update(cx, |view, cx| {
        view.model_mut().fold_rows(5..=1999);
        cx.notify();
    });
    settle(cx);

    let shown = laid_out(&editor, cx);
    assert!(shown.contains(&2000), "past the fold: {shown:?}");
    let (before, after): (Vec<u32>, Vec<u32>) = shown.iter().partition(|&&row| row < 1000);
    let last_before = *before.last().unwrap();
    let wanted = (0..last_before + 1).chain(after[0]..after.last().unwrap() + 1 + MARGIN);
    assert_eq!(asked_rows(&asked, ROWS), each_once(wanted));
}

#[gpui::test]
fn a_long_comment_is_checked_by_rows(cx: &mut TestAppContext) {
    const ROWS: u32 = 1000;
    let files = Files::new();
    let text = format!("/*\n{}*/\n", rows_text(ROWS, |word| format!("{word}\n")));
    let path = files.write("long.rs", &text);
    let (workbench, cx) = workbench(cx);
    let asked = give_speller(&workbench, &[], cx);
    let editor = open(&workbench, &path, cx);
    settle(cx);

    let (top, shown) = viewport(&editor, cx);
    assert_eq!(top, 0);
    // The first row is the comment's opening, so words start a row down.
    let by_row = asked_rows(&asked, ROWS);
    assert_eq!(by_row, each_once(0..shown + MARGIN - 1));
}

#[gpui::test]
fn the_view_starting_partway_down_a_wrapped_row_counts(cx: &mut TestAppContext) {
    const ROWS: u32 = 300;
    let files = Files::new();
    let wrapped = "filler ".repeat(10000);
    let text = format!("{wrapped}\n{}", rows_text(ROWS, |word| format!("{word}\n")));
    let path = files.write("wrapped.txt", &text);
    let (workbench, cx) = workbench(cx);
    let asked = give_speller(&workbench, &[], cx);
    let editor = open(&workbench, &path, cx);
    editor.update(cx, |view, cx| {
        view.model_mut().set_soft_wrap(true);
        cx.notify();
    });
    draw(cx);
    let (wrapped_rows, shown) = editor.read_with(cx, |view, _| {
        let rows = view.model().display().row_for_buffer_row(1);
        (rows, view.viewport_rows().expect("drawn").1)
    });
    assert!(
        wrapped_rows > shown * 2,
        "{wrapped_rows} rows wrapped, {shown} shown"
    );

    // Near the end of the first row, with its last 3 display rows at the top.
    let line = 1. - 3. / wrapped_rows as f32;
    editor.update(cx, |view, cx| view.scroll_to_buffer_line(line, cx));
    settle(cx);
    assert_eq!(viewport(&editor, cx).0, 0);

    // Its words start on the row after it, row_word(0) on buffer row 1: what's
    // shown of them and the margin, exactly.
    let shown = laid_out(&editor, cx);
    assert_eq!(shown[0], 0);
    let last_shown = *shown.last().unwrap();
    assert!(last_shown > 1, "rows below it are shown: {shown:?}");
    let by_word: HashMap<String, u32> = (0..ROWS).map(|row| (row_word(row), row + 1)).collect();
    let words = asked.words();
    let found = words.iter().filter_map(|word| by_word.get(word).copied());
    assert_eq!(found.max(), Some(last_shown + MARGIN));
}

#[gpui::test]
fn a_language_needed_only_past_the_view_gets_compiled(cx: &mut TestAppContext) {
    // A test run by nextest has its own process, so nothing has compiled this.
    if fe_syntax::loaded_language("markdown_inline").is_some() {
        return;
    }
    let files = Files::new();
    // Nothing the view draws has inline text: only a row of the margin does.
    let path = files.write("notes.md", &format!("{}teh prose\n", "\n".repeat(60)));
    let (workbench, cx) = workbench(cx);
    give_speller(&workbench, &["teh"], cx);
    let editor = open(&workbench, &path, cx);
    let (_, shown) = viewport(&editor, cx);
    assert!(shown < 60, "the prose is past what's shown");
    settle(cx);
    assert!(fe_syntax::loaded_language("markdown_inline").is_some());
    assert_eq!(squiggled(&editor, cx), ["teh"]);
}

/// A workbench showing a.txt with its squiggle, whose split has come and whose
/// original has gone with no draw between, and the split's editor.
fn split_then_close_the_original<'a>(
    files: &Files,
    cx: &'a mut TestAppContext,
) -> (
    Entity<Workbench>,
    &'a mut VisualTestContext,
    Entity<EditorView>,
) {
    let path = files.write("a.txt", "teh\n");
    let (workbench, cx) = workbench(cx);
    give_speller(&workbench, &["teh"], cx);
    open(&workbench, &path, cx);
    settle(cx);
    let source = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    workbench.update_in(cx, |workbench, window, cx| {
        let new = (workbench.workspace().clone()).update(cx, |workspace, cx| {
            workspace.split(Direction::Right, window, cx)
        });
        workbench.activate(new, window, cx);
        source.update(cx, |pane, cx| pane.close_active(window, cx));
    });
    let copy = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("the split shows it");
    (workbench, cx, copy)
}

#[gpui::test]
fn a_split_keeps_one_provider_when_its_original_closes(cx: &mut TestAppContext) {
    let files = Files::new();
    let (workbench, cx, copy) = split_then_close_the_original(&files, cx);
    settle(cx);
    assert_eq!(squiggled(&copy, cx), ["teh"], "just the one");

    workbench.update(cx, |workbench, cx| workbench.ignore_spell_word("teh", cx));
    cx.run_until_parked();
    assert_eq!(underlines(&copy, cx), []);
}

#[gpui::test]
fn what_a_split_copied_goes_with_a_setting_off(cx: &mut TestAppContext) {
    let files = Files::new();
    let (workbench, cx, copy) = split_then_close_the_original(&files, cx);
    let off = EditorSettings {
        spell_check: false,
        ..EditorSettings::default()
    };
    reload(&workbench, off, cx);
    settle(cx);
    assert_eq!(underlines(&copy, cx), []);
}

#[gpui::test]
fn what_a_split_copied_goes_with_the_speller(cx: &mut TestAppContext) {
    let files = Files::new();
    let (workbench, cx, copy) = split_then_close_the_original(&files, cx);
    workbench.update(cx, |workbench, cx| workbench.set_speller(NoSpeller, cx));
    settle(cx);
    assert_eq!(underlines(&copy, cx), []);
}

#[gpui::test]
fn what_a_split_copied_goes_with_an_accepted_word(cx: &mut TestAppContext) {
    let files = Files::new();
    let (workbench, cx, copy) = split_then_close_the_original(&files, cx);
    workbench.update(cx, |workbench, cx| workbench.ignore_spell_word("teh", cx));
    assert_eq!(underlines(&copy, cx), [], "with no draw yet");
}

#[gpui::test]
fn setting_toggles_live(cx: &mut TestAppContext) {
    let files = Files::new();
    let path = files.write("main.rs", "// teh\nfn main() { let s = \"teh\"; }\n");
    let (workbench, cx) = workbench(cx);
    give_speller(&workbench, &["teh"], cx);
    let editor = open(&workbench, &path, cx);
    settle(cx);
    assert_eq!(underlines(&editor, cx).len(), 2);

    // Off: gone at once, and nothing's checked.
    let off = EditorSettings {
        spell_check: false,
        ..EditorSettings::default()
    };
    reload(&workbench, off, cx);
    assert_eq!(underlines(&editor, cx), []);
    let before = checks(&workbench, cx);
    replace(&editor, "main", "start", cx);
    settle(cx);
    assert_eq!(checks(&workbench, cx), before);
    assert!(!workbench.read_with(cx, |workbench, _| {
        workbench.spelling.has_task(editor.entity_id())
    }));
    assert_eq!(underlines(&editor, cx), []);

    // On: back, with the same editor.
    reload(&workbench, EditorSettings::default(), cx);
    settle(cx);
    assert_eq!(underlines(&editor, cx).len(), 2);

    // A scope gone: only its squiggles are.
    let strings = EditorSettings {
        spell_check_scopes: vec![SpellScope::Strings],
        ..EditorSettings::default()
    };
    reload(&workbench, strings, cx);
    settle(cx);
    let found = underlines(&editor, cx);
    assert_eq!(found.len(), 1);
    let text = editor.read_with(cx, |view, _| view.model().text().to_string());
    assert_eq!(found[0].range.start, text.rfind("teh").unwrap());
}

#[gpui::test]
fn dictionary_changes_recheck(cx: &mut TestAppContext) {
    let files = Files::new();
    let a = files.write("a.txt", "teh one\nqux\n");
    let b = files.write("b.txt", "teh two\nqux\n");
    let file = files.0.path().join("dictionary.txt");
    let (workbench, cx) = workbench(cx);
    give_speller(&workbench, &["teh", "qux"], cx);
    workbench.update(cx, |workbench, cx| {
        workbench.set_spell_dictionary(Dictionary::load(file.clone()), cx)
    });

    // One editor in a hidden tab, and one with a split of it beside it.
    let hidden = open(&workbench, &a, cx);
    settle(cx);
    let shown = open(&workbench, &b, cx);
    settle(cx);
    run("pane.split_right", cx);
    settle(cx);
    let copy = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .unwrap();
    assert_ne!(copy, shown);
    for editor in [&hidden, &shown, &copy] {
        assert_eq!(squiggled(editor, cx), ["teh", "qux"], "once each");
    }

    // Added, in another case: gone from all, before any check, and written.
    workbench.update(cx, |workbench, cx| workbench.add_spell_word("TEH", cx));
    cx.run_until_parked();
    for editor in [&hidden, &shown, &copy] {
        assert_eq!(squiggled(editor, cx), ["qux"]);
    }
    assert_eq!(fs::read_to_string(&file).unwrap(), "teh\n");
    settle(cx);
    for editor in [&shown, &copy] {
        assert_eq!(squiggled(editor, cx), ["qux"], "and the check agrees");
    }

    // Ignored: gone from all, and not written.
    workbench.update(cx, |workbench, cx| workbench.ignore_spell_word("qux", cx));
    cx.run_until_parked();
    for editor in [&hidden, &shown, &copy] {
        assert_eq!(squiggled(editor, cx), Vec::<String>::new());
    }
    settle(cx);
    assert_eq!(fs::read_to_string(&file).unwrap(), "teh\n");
    assert_eq!(squiggled(&shown, cx), Vec::<String>::new());

    // Accepted for this session, they outlast a dictionary without them.
    workbench.update(cx, |workbench, cx| {
        workbench.set_spell_dictionary(Dictionary::default(), cx)
    });
    settle(cx);
    assert_eq!(squiggled(&shown, cx), Vec::<String>::new());
}

#[gpui::test]
fn a_dictionary_without_a_files_word_brings_it_back(cx: &mut TestAppContext) {
    let files = Files::new();
    let a = files.write("a.txt", "teh one\n");
    let file = files.write("dictionary.txt", "teh\n");
    let (workbench, cx) = workbench(cx);
    give_speller(&workbench, &["teh"], cx);
    workbench.update(cx, |workbench, cx| {
        workbench.set_spell_dictionary(Dictionary::load(file.clone()), cx)
    });
    let editor = open(&workbench, &a, cx);
    settle(cx);
    assert_eq!(squiggled(&editor, cx), Vec::<String>::new());

    workbench.update(cx, |workbench, cx| {
        workbench.set_spell_dictionary(Dictionary::default(), cx)
    });
    settle(cx);
    assert_eq!(squiggled(&editor, cx), ["teh"]);
}

#[gpui::test]
fn no_speller_means_no_squiggles(cx: &mut TestAppContext) {
    let files = Files::new();
    let path = files.write("notes.txt", "teh\n");
    let (workbench, cx) = workbench(cx);

    let check = |workbench: &Entity<Workbench>, cx: &mut VisualTestContext| {
        let editor = open(workbench, &path, cx);
        settle(cx);
        replace(&editor, "teh", "teh teh", cx);
        settle(cx);
        assert_eq!(underlines(&editor, cx), []);
        assert_eq!(checks(workbench, cx), 0);
        let slots = workbench.read_with(cx, |workbench, _| workbench.spelling.slot_count());
        assert_eq!(slots, 0, "no provider and no task");
    };
    // As it starts, and when told so.
    check(&workbench, cx);
    workbench.update(cx, |workbench, cx| workbench.set_speller(NoSpeller, cx));
    check(&workbench, cx);
    assert!(workbench.read_with(cx, |workbench, _| workbench.speller().is_none()));

    // Taking a real one away takes what it did.
    give_speller(&workbench, &["teh"], cx);
    settle(cx);
    let editor = open(&workbench, &path, cx);
    assert_eq!(underlines(&editor, cx).len(), 3);
    workbench.update(cx, |workbench, cx| workbench.set_speller(NoSpeller, cx));
    cx.run_until_parked();
    assert_eq!(underlines(&editor, cx), []);
    let slots = workbench.read_with(cx, |workbench, _| workbench.spelling.slot_count());
    assert_eq!(slots, 0);
}

#[gpui::test]
fn squiggles_stay_out_of_the_problems(cx: &mut TestAppContext) {
    let files = Files::new();
    let path = files.write("main.rs", "// teh\n");
    let (workbench, cx) = workbench(cx);
    give_speller(&workbench, &["teh"], cx);
    let editor = open(&workbench, &path, cx);
    settle(cx);
    assert_eq!(underlines(&editor, cx).len(), 1);

    editor.read_with(cx, |view, _| assert!(view.model().diagnostics().is_empty()));
    let panel = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).problems().clone()
    });
    assert!(
        panel
            .read_with(cx, |panel, _| panel.shown_rows())
            .is_empty()
    );
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    let items = bar.read_with(cx, |bar, _| bar.drawn(StatusSide::Left));
    assert!(!items.iter().any(|(id, _)| id == ITEM));
}

#[gpui::test]
fn a_hidden_tab_is_not_checked_until_shown(cx: &mut TestAppContext) {
    let files = Files::new();
    let a = files.write("a.txt", "one\n");
    let b = files.write("b.txt", "two\n");
    let (workbench, cx) = workbench(cx);
    give_speller(&workbench, &["teh"], cx);
    let hidden = open(&workbench, &a, cx);
    settle(cx);
    open(&workbench, &b, cx);
    settle(cx);

    let before = checks(&workbench, cx);
    replace(&hidden, "one", "teh", cx);
    settle(cx);
    assert_eq!(checks(&workbench, cx), before);

    open(&workbench, &a, cx);
    settle(cx);
    assert_eq!(checks(&workbench, cx), before + 1);
    assert_eq!(squiggled(&hidden, cx), ["teh"]);
}

#[gpui::test]
fn a_new_language_checks_again(cx: &mut TestAppContext) {
    let files = Files::new();
    let path = files.write("notes.txt", "fn teh() {}\n");
    let (workbench, cx) = workbench(cx);
    give_speller(&workbench, &["teh"], cx);
    let editor = open(&workbench, &path, cx);
    settle(cx);
    assert_eq!(squiggled(&editor, cx), ["teh"]);

    editor.update(cx, |view, cx| view.set_language(Some("rust"), cx));
    settle(cx);
    assert_eq!(
        squiggled(&editor, cx),
        Vec::<String>::new(),
        "it's code now"
    );
}

#[test]
fn rows_left_to_check() {
    let rows = |start: u32, end: u32| start..end;
    assert_eq!(missing_rows(&rows(0, 10), &[]), vec![rows(0, 10)]);
    assert_eq!(missing_rows(&rows(5, 10), &[rows(0, 5)]), vec![rows(5, 10)]);
    assert_eq!(
        missing_rows(&rows(0, 10), &[rows(2, 4), rows(6, 8)]),
        vec![rows(0, 2), rows(4, 6), rows(8, 10)]
    );
    assert!(missing_rows(&rows(3, 7), &[rows(0, 20)]).is_empty());

    let mut checked = Vec::new();
    add_rows(&mut checked, rows(10, 20));
    add_rows(&mut checked, rows(0, 5));
    add_rows(&mut checked, rows(20, 25));
    add_rows(&mut checked, rows(3, 4));
    add_rows(&mut checked, rows(7, 7));
    assert_eq!(checked, vec![rows(0, 5), rows(10, 25)]);
}
