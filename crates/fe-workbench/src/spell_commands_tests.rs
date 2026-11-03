//! The spelling commands in a real workbench, on a paused clock, with a speller
//! that says which words are wrong and what they might have been.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::{Settings, SettingsEvent};
use fe_editor::decorations::DecorationKind;
use fe_gpui::{EditorSettings, EditorTheme, EditorView, commands::RunCommand};
use gpui::{BackgroundExecutor, Entity, Focusable as _, TestAppContext, VisualTestContext};
use tempfile::TempDir;

use crate::{
    Launch, Workbench,
    pane_group::Direction,
    persist::save_toggles_to,
    spell::{Dictionary, Speller},
    spell_commands::{NO_MISSPELLING_HERE, NO_MISSPELLINGS, READ_ONLY},
    spell_squiggles::REST,
    workbench::toast::Level,
};

/// Finds `WRONG` misspelled, in any case but a capitalised first letter on
/// `Paris`, with `SUGGESTIONS` for each.
struct Fake {
    executor: BackgroundExecutor,
    /// Set on each `suggest` asked on the main thread.
    on_main_thread: Arc<AtomicBool>,
}

const WRONG: &[&str] = &["teh", "Teh", "wrod", "nosuggest"];
const SUGGESTIONS: &[&str] = &["the", "tea", "ten", "the"];

impl Speller for Fake {
    fn is_misspelled(&self, word: &str, _language: &str) -> bool {
        WRONG.contains(&word)
    }

    fn suggest(&self, word: &str, _language: &str) -> Vec<String> {
        if self.executor.is_main_thread() {
            self.on_main_thread.store(true, Ordering::SeqCst);
        }
        if word == "nosuggest" {
            return Vec::new();
        }
        SUGGESTIONS.iter().map(|it| (*it).to_owned()).collect()
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Long enough for whatever is to be checked to be.
fn settle(cx: &mut VisualTestContext) {
    draw(cx);
    for _ in 0..3 {
        cx.executor().advance_clock(REST);
        cx.run_until_parked();
        draw(cx);
    }
}

fn run(id: &str, cx: &mut VisualTestContext) {
    let context = ContextStack::new()
        .with(ContextFrame::new("workbench"))
        .with(ContextFrame::new("editor"));
    let run = RunCommand(Invocation::of(id).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// A folder of files, and a workbench with the fake speller and a dictionary in
/// the folder.
struct Spelling<'a> {
    dir: TempDir,
    workbench: Entity<Workbench>,
    cx: &'a mut VisualTestContext,
    on_main_thread: Arc<AtomicBool>,
}

fn give_speller(
    workbench: &Entity<Workbench>,
    dictionary: &Path,
    cx: &mut VisualTestContext,
) -> Arc<AtomicBool> {
    let on_main_thread = Arc::new(AtomicBool::default());
    let speller = Fake {
        executor: cx.executor(),
        on_main_thread: on_main_thread.clone(),
    };
    let dictionary = Dictionary::load(dictionary.to_owned());
    workbench.update(cx, |workbench, cx| {
        workbench.set_speller(speller, cx);
        workbench.set_spell_dictionary(dictionary, cx);
    });
    on_main_thread
}

fn spelling(cx: &mut TestAppContext) -> Spelling<'_> {
    let dir = tempfile::tempdir().unwrap();
    let launch = Launch {
        root: None,
        file: None,
    };
    let (workbench, cx, _) = crate::tests::open(cx, launch);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let on_main_thread = give_speller(&workbench, &dir.path().join("dictionary.txt"), cx);
    Spelling {
        dir,
        workbench,
        cx,
        on_main_thread,
    }
}

/// Shows `path` in `workbench`'s active pane, checked, and returns its editor.
fn show(
    workbench: &Entity<Workbench>,
    path: PathBuf,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    settle(cx);
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("the file is shown")
}

fn squiggled(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<String> {
    editor.read_with(cx, |view, _| {
        let model = view.model();
        let text = model.text().to_string();
        (model.decorations(0..text.len()).into_iter())
            .filter(|it| matches!(it.kind, DecorationKind::Underline { .. }))
            .map(|it| text[it.range].to_owned())
            .collect()
    })
}

fn text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |view, _| view.model().buffer().text().to_string())
}

fn caret(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> usize {
    editor.read_with(cx, |view, _| view.model().primary_selection().head())
}

/// Puts `editor`'s caret `into` bytes past the start of the first `word`.
fn put_caret(
    editor: &Entity<EditorView>,
    word: &str,
    into: usize,
    cx: &mut VisualTestContext,
) -> usize {
    let at = text(editor, cx).find(word).expect("the word") + into;
    editor.update(cx, |view, cx| {
        view.model_mut().set_cursor(at);
        cx.notify();
    });
    at
}

impl Spelling<'_> {
    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.dir.path().join(name);
        fs::write(&path, text).unwrap();
        path
    }

    fn open(&mut self, name: &str, text: &str) -> Entity<EditorView> {
        let path = self.file(name, text);
        show(&self.workbench, path, self.cx)
    }

    fn dictionary(&self) -> String {
        fs::read_to_string(self.dir.path().join("dictionary.txt")).unwrap_or_default()
    }

    fn toasts(&mut self) -> Vec<(Level, String)> {
        toasts(&self.workbench, self.cx)
    }

    fn suggestions(&mut self) -> Option<Vec<String>> {
        let picker =
            (self.workbench).read_with(self.cx, |workbench, cx| workbench.spell_picker(cx));
        Some(picker?.read_with(self.cx, |picker, _| picker.delegate.shown()))
    }

    /// What the open picker's status line says.
    fn status(&mut self) -> Option<String> {
        use crate::picker::PickerDelegate as _;
        let picker = (self.workbench)
            .read_with(self.cx, |workbench, cx| workbench.spell_picker(cx))
            .expect("the spelling picker shows");
        picker.read_with(self.cx, |picker, _| {
            picker.delegate.status().map(|it| it.to_string())
        })
    }

    fn focused(&mut self, editor: &Entity<EditorView>) -> bool {
        (self.cx).update(|window, cx| editor.focus_handle(cx).is_focused(window))
    }
}

fn toasts(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(Level, String)> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| (entry.toast.level, entry.toast.message.clone()))
            .collect()
    })
}

fn info(message: impl Into<String>) -> (Level, String) {
    (Level::Info, message.into())
}

#[gpui::test]
fn suggest_replaces_the_word(cx: &mut TestAppContext) {
    let mut s = spelling(cx);
    let editor = s.open("a.txt", "one Teh wrod two\n");
    (s.cx).update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    assert_eq!(squiggled(&editor, s.cx), ["Teh", "wrod"]);

    // The caret inside the word; suggestions in its case, each once.
    put_caret(&editor, "Teh", 1, s.cx);
    run("spell.suggest", s.cx);
    assert_eq!(s.suggestions().unwrap(), ["The", "Tea", "Ten"]);
    assert!(!s.on_main_thread.load(Ordering::SeqCst));

    // The list filters, as a picker's does.
    s.cx.simulate_input("en");
    draw(s.cx);
    assert_eq!(s.suggestions().unwrap(), ["Ten"]);
    s.cx.simulate_keystrokes("backspace backspace");
    draw(s.cx);

    // Enter replaces it, the squiggle goes with it at once, and focus comes back.
    s.cx.simulate_keystrokes("enter");
    assert_eq!(s.suggestions(), None);
    assert_eq!(text(&editor, s.cx), "one The wrod two\n");
    assert_eq!(squiggled(&editor, s.cx), ["wrod"]);
    assert!(s.focused(&editor));
    // A check of the new text doesn't bring it back.
    settle(s.cx);
    assert_eq!(squiggled(&editor, s.cx), ["wrod"]);

    // One edit: one undo takes it back, whole.
    editor.update(s.cx, |view, cx| {
        view.model_mut().undo().unwrap();
        cx.notify();
    });
    assert_eq!(text(&editor, s.cx), "one Teh wrod two\n");

    // At the word's end counts too.
    settle(s.cx);
    put_caret(&editor, "wrod", 4, s.cx);
    run("spell.suggest", s.cx);
    assert_eq!(s.suggestions().unwrap(), ["the", "tea", "ten"]);
    s.cx.simulate_keystrokes("escape");
    draw(s.cx);
    assert_eq!(s.suggestions(), None);
    assert!(s.focused(&editor));
    assert_eq!(text(&editor, s.cx), "one Teh wrod two\n");
    assert_eq!(s.toasts(), []);

    // On a correct word, there's nothing to fix.
    put_caret(&editor, "one", 1, s.cx);
    run("spell.suggest", s.cx);
    assert_eq!(s.suggestions(), None);
    assert_eq!(s.toasts(), [info(NO_MISSPELLING_HERE)]);
}

#[gpui::test]
fn a_replacement_takes_the_squiggle_from_a_split_too(cx: &mut TestAppContext) {
    let mut s = spelling(cx);
    let original = s.open("a.txt", "one teh wrod\n");
    (s.workbench).update_in(s.cx, |workbench, window, cx| {
        let new = (workbench.workspace().clone()).update(cx, |workspace, cx| {
            workspace.split(Direction::Right, window, cx)
        });
        workbench.activate(new, window, cx);
    });
    settle(s.cx);
    let split = (s.workbench)
        .read_with(s.cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("the split shows it");
    assert_ne!(split, original);
    assert_eq!(squiggled(&original, s.cx), ["teh", "wrod"]);
    assert_eq!(squiggled(&split, s.cx), ["teh", "wrod"]);

    put_caret(&split, "teh", 1, s.cx);
    run("spell.suggest", s.cx);
    s.cx.simulate_keystrokes("enter");
    assert_eq!(text(&original, s.cx), "one the wrod\n");
    assert_eq!(squiggled(&split, s.cx), ["wrod"]);
    assert_eq!(squiggled(&original, s.cx), ["wrod"]);
}

#[gpui::test]
fn no_word_at_a_caret_with_no_editor(cx: &mut TestAppContext) {
    let mut s = spelling(cx);
    run("spell.suggest", s.cx);
    assert_eq!(s.suggestions(), None);
    assert_eq!(s.toasts(), [info(NO_MISSPELLING_HERE)]);
}

#[gpui::test]
fn a_caret_between_two_misspellings_takes_the_one_it_starts(cx: &mut TestAppContext) {
    let mut s = spelling(cx);
    let editor = s.open("a.txt", "wrodTeh\n");
    assert_eq!(squiggled(&editor, s.cx), ["wrod", "Teh"]);
    put_caret(&editor, "Teh", 0, s.cx);
    run("spell.suggest", s.cx);
    assert_eq!(s.suggestions().unwrap(), ["The", "Tea", "Ten"]);

    // The one beside it keeps its squiggle.
    s.cx.simulate_keystrokes("enter");
    assert_eq!(text(&editor, s.cx), "wrodThe\n");
    assert_eq!(squiggled(&editor, s.cx), ["wrod"]);
}

#[gpui::test]
fn a_picker_with_no_suggestions_says_so(cx: &mut TestAppContext) {
    let mut s = spelling(cx);
    let editor = s.open("a.txt", "nosuggest\n");
    put_caret(&editor, "nosuggest", 2, s.cx);
    run("spell.suggest", s.cx);
    assert_eq!(s.suggestions().unwrap(), Vec::<String>::new());
    assert_eq!(s.status().as_deref(), Some("No suggestions"));
    s.cx.simulate_keystrokes("enter");
    assert_eq!(text(&editor, s.cx), "nosuggest\n");
}

#[gpui::test]
fn a_read_only_editor_is_left_alone(cx: &mut TestAppContext) {
    let mut s = spelling(cx);
    let editor = s.open("a.txt", "teh\n");
    put_caret(&editor, "teh", 1, s.cx);
    editor.update(s.cx, |view, cx| view.set_read_only(true, cx));
    run("spell.suggest", s.cx);
    assert_eq!(s.suggestions(), None);
    assert_eq!(s.toasts(), [info(READ_ONLY)]);
}

#[gpui::test]
fn an_edit_elsewhere_moves_the_word_it_replaces(cx: &mut TestAppContext) {
    let mut s = spelling(cx);
    let editor = s.open("a.txt", "one teh\n");
    put_caret(&editor, "teh", 1, s.cx);
    run("spell.suggest", s.cx);
    editor.update(s.cx, |view, cx| {
        view.model_mut().set_cursor(0);
        view.model_mut().insert("zero ");
        cx.notify();
    });
    s.cx.simulate_keystrokes("enter");
    assert_eq!(text(&editor, s.cx), "zero one the\n");
    assert_eq!(s.toasts(), []);
}

#[gpui::test]
fn an_edit_of_the_word_stops_the_replacement(cx: &mut TestAppContext) {
    let mut s = spelling(cx);
    let editor = s.open("a.txt", "one teh\n");
    let at = put_caret(&editor, "teh", 1, s.cx);
    run("spell.suggest", s.cx);
    editor.update(s.cx, |view, cx| {
        view.model_mut().set_cursor(at);
        view.model_mut().insert("x");
        cx.notify();
    });
    s.cx.simulate_keystrokes("enter");
    assert_eq!(text(&editor, s.cx), "one txeh\n");
    assert_eq!(
        s.toasts(),
        [info("teh changed while the suggestions were open")]
    );
}

#[gpui::test]
fn a_squiggle_split_by_typing_is_no_word(cx: &mut TestAppContext) {
    let mut s = spelling(cx);
    let editor = s.open("a.txt", "one teh\n");
    let at = put_caret(&editor, "teh", 1, s.cx);
    editor.update(s.cx, |view, cx| {
        view.model_mut().set_cursor(at);
        view.model_mut().insert(" ");
        cx.notify();
    });
    // Before the next check, so with its squiggle yet.
    run("spell.add_word", s.cx);
    assert_eq!(s.dictionary(), "");
    assert_eq!(s.toasts(), [info(NO_MISSPELLING_HERE)]);
}

#[gpui::test]
fn add_to_dictionary_is_global(cx: &mut TestAppContext) {
    let mut s = spelling(cx);
    let first = s.file("first.txt", "teh wrod\n");
    let first = show(&s.workbench, first, s.cx);
    let second = s.open("second.txt", "Teh here\n");
    assert_eq!(squiggled(&second, s.cx), ["Teh"]);
    // A hidden tab's editor has the first's squiggles yet.
    assert_eq!(squiggled(&first, s.cx), ["teh", "wrod"]);

    put_caret(&second, "Teh", 1, s.cx);
    run("spell.add_word", s.cx);
    assert_eq!(s.dictionary(), "teh\n");
    assert_eq!(s.toasts(), [info("Added Teh to dictionary")]);
    assert!(squiggled(&second, s.cx).is_empty());
    assert_eq!(squiggled(&first, s.cx), ["wrod"]);

    // It stays for the next session's dictionary.
    assert!(Dictionary::load(s.dir.path().join("dictionary.txt")).contains("TEH"));

    // With no word there, nothing is written.
    put_caret(&second, "here", 1, s.cx);
    run("spell.add_word", s.cx);
    assert_eq!(s.dictionary(), "teh\n");
    assert_eq!(
        s.toasts(),
        [info("Added Teh to dictionary"), info(NO_MISSPELLING_HERE)]
    );
}

#[gpui::test]
fn a_failed_add_says_so_and_counts_for_the_session(cx: &mut TestAppContext) {
    let mut s = spelling(cx);
    let editor = s.open("a.txt", "teh\n");
    // A file in the way of the dictionary's directory.
    let blocked = s.file("blocked", "");
    let dictionary = Dictionary::load(blocked.join("dictionary.txt"));
    (s.workbench).update(s.cx, |workbench, cx| {
        workbench.set_spell_dictionary(dictionary, cx);
    });
    settle(s.cx);
    put_caret(&editor, "teh", 1, s.cx);
    run("spell.add_word", s.cx);
    assert!(squiggled(&editor, s.cx).is_empty());
    assert_eq!(
        s.toasts(),
        [(Level::Error, "Can't add teh to dictionary".to_owned())]
    );
}

/// A second window, on the same app, handed a dictionary as the app hands one.
fn second_window(
    cx: &mut TestAppContext,
    dictionary: &Path,
) -> (Entity<Workbench>, VisualTestContext) {
    let (workbench, mut vcx) = bare_window(cx);
    give_speller(&workbench, dictionary, &mut vcx);
    (workbench, vcx)
}

/// A second window with a speller, but only the dictionary it starts with.
fn unhanded_window(cx: &mut TestAppContext) -> (Entity<Workbench>, VisualTestContext) {
    let (workbench, mut vcx) = bare_window(cx);
    let speller = Fake {
        executor: vcx.executor(),
        on_main_thread: Arc::default(),
    };
    workbench.update(&mut vcx, |workbench, cx| workbench.set_speller(speller, cx));
    (workbench, vcx)
}

fn bare_window(cx: &mut TestAppContext) -> (Entity<Workbench>, VisualTestContext) {
    let window = cx.add_window(move |window, cx| {
        let launch = Launch {
            root: None,
            file: None,
        };
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let workbench = window.root(cx).unwrap();
    let vcx = VisualTestContext::from_window(window.into(), cx);
    (workbench, vcx)
}

/// Each of `accept`'s commands reaches the other window's editors.
fn an_accepted_word_goes_from_other_windows(accept: &str, cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let dictionary = dir.path().join("dictionary.txt");
    let path = dir.path().join("a.txt");
    fs::write(&path, "teh wrod\n").unwrap();
    let launch = Launch {
        root: None,
        file: None,
    };
    let (workbench, here, _) = crate::tests::open(cx, launch);
    give_speller(&workbench, &dictionary, here);
    let editor = show(&workbench, path.clone(), here);
    let mut here = here.clone();
    let (_other, mut there) = second_window(cx, &dictionary);
    let far = show(&_other, path, &mut there);
    assert_eq!(squiggled(&far, &mut there), ["teh", "wrod"]);

    put_caret(&editor, "teh", 1, &mut here);
    run(accept, &mut here);
    assert_eq!(squiggled(&editor, &mut here), ["wrod"]);
    assert_eq!(squiggled(&far, &mut there), ["wrod"]);
    // Nor does its next check bring it back.
    settle(&mut there);
    assert_eq!(squiggled(&far, &mut there), ["wrod"]);
}

#[gpui::test]
fn an_added_word_goes_from_other_windows(cx: &mut TestAppContext) {
    an_accepted_word_goes_from_other_windows("spell.add_word", cx);
}

#[gpui::test]
fn an_ignored_word_goes_from_other_windows(cx: &mut TestAppContext) {
    an_accepted_word_goes_from_other_windows("spell.ignore_word", cx);
}

/// `accept` run in one window, and what's squiggled in `b.txt`, shown in a window
/// opened after, which may be handed a dictionary of its own, as the app hands one.
fn accept_then_open_a_window(
    accept: &str,
    hand_over: bool,
    cx: &mut TestAppContext,
) -> (TempDir, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    let dictionary = dir.path().join("dictionary.txt");
    let path = dir.path().join("a.txt");
    fs::write(&path, "teh wrod\n").unwrap();
    let later = dir.path().join("b.txt");
    fs::write(&later, "Teh wrod\n").unwrap();
    let launch = Launch {
        root: None,
        file: None,
    };
    let (workbench, here, _) = crate::tests::open(cx, launch);
    give_speller(&workbench, &dictionary, here);
    let editor = show(&workbench, path, here);
    put_caret(&editor, "teh", 1, here);
    run(accept, here);
    assert_eq!(squiggled(&editor, here), ["wrod"]);

    let (other, mut there) = if hand_over {
        second_window(cx, &dictionary)
    } else {
        unhanded_window(cx)
    };
    let far = show(&other, later, &mut there);
    let found = squiggled(&far, &mut there);
    (dir, found)
}

#[gpui::test]
fn an_ignored_word_stays_ignored_in_a_window_opened_later(cx: &mut TestAppContext) {
    let (dir, found) = accept_then_open_a_window("spell.ignore_word", true, cx);
    assert_eq!(found, ["wrod"]);
    // All without a word on disk, which the new window's dictionary read.
    assert!(!dir.path().join("dictionary.txt").exists());
}

#[gpui::test]
fn an_ignored_word_stays_ignored_with_no_dictionary_handed_over(cx: &mut TestAppContext) {
    let (dir, found) = accept_then_open_a_window("spell.ignore_word", false, cx);
    assert_eq!(found, ["wrod"]);
    assert!(!dir.path().join("dictionary.txt").exists());
}

#[gpui::test]
fn replacing_a_dictionary_keeps_the_sessions_words(cx: &mut TestAppContext) {
    let mut s = spelling(cx);
    let editor = s.open("a.txt", "teh wrod\n");
    put_caret(&editor, "teh", 1, s.cx);
    run("spell.ignore_word", s.cx);
    let dictionary = Dictionary::load(s.dir.path().join("dictionary.txt"));
    (s.workbench).update(s.cx, |workbench, cx| {
        workbench.set_spell_dictionary(dictionary, cx);
    });
    settle(s.cx);
    assert_eq!(squiggled(&editor, s.cx), ["wrod"]);
}

/// A word whose write failed counts for the session, in later windows too.
#[gpui::test]
fn a_failed_add_stays_accepted_in_a_window_opened_later(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let blocked = dir.path().join("blocked");
    fs::write(&blocked, "").unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "teh wrod\n").unwrap();
    let launch = Launch {
        root: None,
        file: None,
    };
    let (workbench, here, _) = crate::tests::open(cx, launch);
    give_speller(&workbench, &blocked.join("dictionary.txt"), here);
    let editor = show(&workbench, path.clone(), here);
    put_caret(&editor, "teh", 1, here);
    run("spell.add_word", here);
    assert_eq!(
        toasts(&workbench, here),
        [(Level::Error, "Can't add teh to dictionary".to_owned())]
    );

    let (other, mut there) = second_window(cx, &blocked.join("dictionary.txt"));
    let far = show(&other, path, &mut there);
    assert_eq!(squiggled(&far, &mut there), ["wrod"]);
}

#[gpui::test]
fn ignore_is_for_the_session(cx: &mut TestAppContext) {
    let mut s = spelling(cx);
    let first = s.file("first.txt", "teh wrod\n");
    let first = show(&s.workbench, first, s.cx);
    let second = s.open("second.txt", "Teh here\n");

    put_caret(&second, "Teh", 1, s.cx);
    run("spell.ignore_word", s.cx);
    assert!(squiggled(&second, s.cx).is_empty());
    assert_eq!(squiggled(&first, s.cx), ["wrod"]);
    assert_eq!(s.toasts(), []);

    // Nothing's written, so the next session's dictionary hasn't it.
    assert!(!s.dir.path().join("dictionary.txt").exists());
    assert!(!Dictionary::load(s.dir.path().join("dictionary.txt")).contains("teh"));

    // Checks stay quiet about it, in this session.
    settle(s.cx);
    assert!(squiggled(&second, s.cx).is_empty());
    let third = s.open("third.txt", "teh again\n");
    assert!(squiggled(&third, s.cx).is_empty());

    // With no word there, there's nothing to ignore.
    put_caret(&third, "again", 1, s.cx);
    run("spell.ignore_word", s.cx);
    assert_eq!(s.toasts(), [info(NO_MISSPELLING_HERE)]);
}

#[gpui::test]
fn toggle_writes_the_setting(cx: &mut TestAppContext) {
    let mut s = spelling(cx);
    let path = s.dir.path().join("settings.toml");
    (s.cx).update(|_, cx| save_toggles_to(cx, path.clone()));
    let saved = || {
        let store = fe_config::SettingsStore::load(Some(path.clone()), None);
        assert_eq!(store.diagnostics(), []);
        store.settings().editor.spell_check
    };
    let editor = s.open("a.txt", "teh\n");
    assert_eq!(squiggled(&editor, s.cx), ["teh"]);

    // Off: gone at once, and saved, leaving what else is in the file.
    run("spell.toggle", s.cx);
    assert!(squiggled(&editor, s.cx).is_empty());
    assert!(!saved());
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("spell_check = false")
    );

    // The reload the write brings changes nothing.
    let reload = |on: bool, workbench: &Entity<Workbench>, cx: &mut VisualTestContext| {
        let event = SettingsEvent {
            settings: Arc::new(Settings {
                editor: EditorSettings {
                    spell_check: on,
                    ..EditorSettings::default()
                },
                ..Settings::default()
            }),
            previous: Arc::new(Settings::default()),
            diagnostics: Vec::new(),
        };
        workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
    };
    reload(false, &s.workbench, s.cx);
    settle(s.cx);
    assert!(squiggled(&editor, s.cx).is_empty());

    // On: back, from the next check.
    run("spell.toggle", s.cx);
    settle(s.cx);
    assert_eq!(squiggled(&editor, s.cx), ["teh"]);
    assert!(saved());

    // It goes by the setting as reloaded, so a hand edit of the file counts.
    reload(false, &s.workbench, s.cx);
    assert!(squiggled(&editor, s.cx).is_empty());
    run("spell.toggle", s.cx);
    settle(s.cx);
    assert_eq!(squiggled(&editor, s.cx), ["teh"]);
    assert!(saved());
}

#[gpui::test]
fn next_and_previous_misspelling(cx: &mut TestAppContext) {
    let mut s = spelling(cx);
    let source = "one teh two wrod three teh\n";
    let editor = s.open("a.txt", source);
    let first = source.find("teh").unwrap();
    let second = source.find("wrod").unwrap();
    let third = source.rfind("teh").unwrap();

    editor.update(s.cx, |view, _| view.model_mut().set_cursor(0));
    for expected in [first, second, third, first] {
        run("spell.next", s.cx);
        assert_eq!(caret(&editor, s.cx), expected);
    }
    // From the first, back round to the last.
    for expected in [third, second, first, third] {
        run("spell.previous", s.cx);
        assert_eq!(caret(&editor, s.cx), expected);
    }
    // From inside one, next is the one after, and previous that one's start.
    put_caret(&editor, "wrod", 2, s.cx);
    run("spell.next", s.cx);
    assert_eq!(caret(&editor, s.cx), third);
    put_caret(&editor, "wrod", 2, s.cx);
    run("spell.previous", s.cx);
    assert_eq!(caret(&editor, s.cx), second);
    assert_eq!(s.toasts(), []);
}

#[gpui::test]
fn no_misspellings_to_go_to(cx: &mut TestAppContext) {
    let mut s = spelling(cx);
    let editor = s.open("a.txt", "all good here\n");
    let at = put_caret(&editor, "good", 1, s.cx);
    run("spell.next", s.cx);
    assert_eq!(caret(&editor, s.cx), at);
    assert_eq!(s.toasts(), [info(NO_MISSPELLINGS)]);
    run("spell.previous", s.cx);
    assert_eq!(caret(&editor, s.cx), at);
}

/// Out of the editor, its commands don't reach for the one behind the focus.
#[gpui::test]
fn the_word_commands_want_an_editor_context(cx: &mut TestAppContext) {
    let mut s = spelling(cx);
    let editor = s.open("a.txt", "teh\n");
    put_caret(&editor, "teh", 1, s.cx);
    let context = ContextStack::new().with(ContextFrame::new("workbench"));
    let run = RunCommand(
        Invocation::of("spell.ignore_word")
            .unwrap()
            .context(context),
    );
    (s.cx).update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(s.cx);
    assert_eq!(squiggled(&editor, s.cx), ["teh"]);
}
