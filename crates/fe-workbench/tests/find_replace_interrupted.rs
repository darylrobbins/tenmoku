//! The find bar's replaces near and past the search time limit: an
//! interrupted one changes nothing, not the text, the selection or vim's
//! mode, and says so; stepping to the next match looks at no more of a row
//! than it needs. The limit is process-wide, so these tests lower it in
//! a binary of their own, one at a time ([`serially`]), where no other
//! test's search runs meanwhile.

use std::{
    fs,
    ops::Range,
    path::PathBuf,
    sync::{Mutex, MutexGuard},
    time::{Duration, Instant},
};

use fe_buffer::{Buffer, Text};
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{
    EditorSettings, EditorTheme, EditorView, commands::RunCommand, set_vim_enabled,
    set_vim_time_limit,
};
use fe_vim::search::{
    DEFAULT_TIME_LIMIT, Deadline, FindQuery, Pattern, matches_reaching_until, regex_calls,
};
use fe_workbench::{Launch, Pane, Workbench};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

static LIMIT: Mutex<()> = Mutex::new(());

/// Holds [`LIMIT`] and puts the default limit back when dropped.
struct Serial(#[allow(dead_code)] MutexGuard<'static, ()>);

impl Drop for Serial {
    fn drop(&mut self) {
        set_vim_time_limit(DEFAULT_TIME_LIMIT);
    }
}

/// Runs the rest of a test with no other test of this binary running, the
/// limit `limit` until the guard drops: each test takes it first.
///
/// Outside the crate's own tests nothing points the workbench at a config
/// directory of its own: with none, it neither reads the user's settings
/// nor watches them on a thread the test scheduler would reject. So the
/// guard also clears what would name one, with no other test's thread
/// running to read the environment meanwhile.
fn serially(limit: Duration) -> Serial {
    let guard = LIMIT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for var in ["FASTEDIT_CONFIG_DIR", "XDG_CONFIG_HOME", "HOME"] {
        // SAFETY: every test takes the lock before it starts gpui, so no
        // other thread is running to read the environment.
        unsafe { std::env::remove_var(var) };
    }
    set_vim_time_limit(limit);
    Serial(guard)
}

/// Sets the limit to `limit` while [`serially`]'s guard is held, putting
/// `then` back when dropped.
fn limited(limit: Duration, then: Duration) -> impl Drop {
    struct Restore(Duration);
    impl Drop for Restore {
        fn drop(&mut self) {
            set_vim_time_limit(self.0);
        }
    }
    set_vim_time_limit(limit);
    Restore(then)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// Runs `command` as a key would: on the focused element, in the context
/// its key contexts make. Without `draw`, nothing runs after it.
fn dispatch(command: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let context: ContextStack = window
            .context_stack()
            .iter()
            .map(|context| ContextFrame::new(context.primary().map_or("", |entry| &entry.key)))
            .collect();
        let run = RunCommand(Invocation::of(command).unwrap().context(context));
        window.dispatch_action(Box::new(run), cx);
    });
}

fn run(command: &str, cx: &mut VisualTestContext) {
    dispatch(command, cx);
    draw(cx);
}

/// A workbench showing a file holding `text`, its editor focused.
fn opened<'a>(
    text: &str,
    cx: &'a mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<Pane>,
    Entity<EditorView>,
    &'a mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let path: PathBuf = dir.path().join("a.txt");
    fs::write(&path, text).unwrap();
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
    });
    let launch = Launch {
        // No root, so no project watcher on a thread of its own, which the
        // test scheduler would find running beside it.
        root: None,
        file: Some((path, Some(Buffer::new(text)))),
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    draw(cx);
    let editor = pane
        .read_with(cx, |pane, _| pane.active_editor().cloned())
        .expect("a tab is shown");
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);
    (dir, pane, editor, cx)
}

fn text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.model().text().to_string())
}

fn selection(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Range<usize> {
    editor.read_with(cx, |editor, _| editor.model().primary_selection().range)
}

fn select(editor: &Entity<EditorView>, range: Range<usize>, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, cx| editor.select_and_reveal(range, cx));
    draw(cx);
}

/// The bar's label, and whether it's an error.
fn label(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> Option<(String, bool)> {
    pane.read_with(cx, |pane, cx| pane.find_label(cx))
        .map(|(text, error)| (text.to_string(), error))
}

fn too_long() -> Option<(String, bool)> {
    Some(("Search took too long".to_string(), true))
}

/// Shows the bar with its replace row from the editor, then types `query`
/// (a regex with `regex`) and `replacement`.
fn replace_row(
    pane: &Entity<Pane>,
    query: &str,
    regex: bool,
    replacement: &str,
    cx: &mut VisualTestContext,
) {
    run("find.show_replace", cx);
    cx.simulate_input(replacement);
    draw(cx);
    // Hidden from the replace input, focus goes to the query; shown again,
    // it stays there.
    run("find.toggle_replace", cx);
    run("find.toggle_replace", cx);
    if regex {
        run("find.toggle_regex", cx);
    }
    run("text_input.clear", cx);
    cx.simulate_input(query);
    draw(cx);
    let _ = pane;
}

#[gpui::test]
fn replace_all_past_the_time_limit_changes_nothing(cx: &mut TestAppContext) {
    let _serial = serially(DEFAULT_TIME_LIMIT);
    let (_dir, pane, editor, cx) = opened("foo foo\n", cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    let mode = |cx: &mut VisualTestContext| editor.read_with(cx, |editor, _| editor.vim_mode());

    // `2iz`, its session open; then the replace row, typed into.
    cx.simulate_keystrokes("2 i z");
    draw(cx);
    replace_row(&pane, "foo", false, "bar", cx);
    assert_eq!(mode(cx), Some("insert"));

    let caret = selection(&editor, cx);
    let limit = limited(Duration::ZERO, DEFAULT_TIME_LIMIT);
    run("find.replace_all", cx);
    drop(limit);
    assert_eq!(text(&editor, cx), "zfoo foo\n");
    assert_eq!(label(&pane, cx), too_long());
    assert_eq!(selection(&editor, cx), caret);
    assert_eq!(mode(cx), Some("insert"), "the session is still open");

    // Within the limit, the same replace replaces, the session still open.
    run("find.replace_all", cx);
    assert_eq!(text(&editor, cx), "zbar bar\n");
    assert_eq!(mode(cx), Some("insert"));
}

/// The length of the row of `a`s [`slow_text`] makes, which takes seconds
/// to step through to its end, and the limit the tests on it run under:
/// well under that, well over the few matches a step from its start needs.
const SLOW_ROW: usize = 20_000;
const SLOW_LIMIT: Duration = Duration::from_millis(400);

/// The limit the tests on the long rows below set the bar up under: so
/// short that the searches the typed query starts give up at once.
const SETUP_LIMIT: Duration = Duration::from_millis(1);

/// The regex that's slow on [`slow_text`]'s rows of `a`s: each match, one
/// `a`, is found only once `a.*z` from it has looked to the row's end for a
/// `z`, so stepping through a row's matches takes its length squared.
const SLOW_QUERY: &str = "a.*z|a";

/// `b`, a row of [`SLOW_ROW`] `a`s, and `a`.
fn slow_text() -> String {
    format!("b\n{}\na\n", "a".repeat(SLOW_ROW))
}

/// A limit that never passes, for the steps whose work is counted instead.
const NO_LIMIT: Duration = Duration::MAX;

/// The most regex calls a step to the next match may make on a row of
/// [`SLOW_ROW`] `a`s: a quarter of the one for each match that stepping
/// through the row makes, many times the few hundred the editor's own look
/// at the matches around it takes.
const FEW_CALLS: u64 = SLOW_ROW as u64 / 4;

/// The most regex calls the search for the long row's first match may make:
/// the call that finds it and one more, against the one for each of
/// [`MANY_A`] matches that a search going on past it makes.
const WANTED_CALLS: u64 = 5;

#[gpui::test]
fn replace_on_an_expensive_row_steps_only_as_far_as_it_needs(cx: &mut TestAppContext) {
    let _serial = serially(SETUP_LIMIT);
    let (_dir, pane, editor, cx) = opened(&slow_text(), cx);
    select(&editor, 2..3, cx);
    replace_row(&pane, SLOW_QUERY, true, "c", cx);
    assert_eq!(selection(&editor, cx), 2..3);

    let limit = limited(NO_LIMIT, SETUP_LIMIT);
    let before = regex_calls();
    dispatch("find.replace_next", cx);
    let calls = regex_calls() - before;
    drop(limit);
    draw(cx);
    assert!(text(&editor, cx).starts_with("b\nca"));
    assert_eq!(selection(&editor, cx), 3..4, "the next a");
    assert_ne!(label(&pane, cx), too_long());
    assert!(
        calls <= FEW_CALLS,
        "{calls} regex calls, of the row's {SLOW_ROW}"
    );
}

#[gpui::test]
fn a_row_cut_short_by_the_limit_is_an_interruption(cx: &mut TestAppContext) {
    let _serial = serially(SLOW_LIMIT);
    let original = slow_text();
    let (_dir, pane, editor, cx) = opened(&original, cx);
    replace_row(&pane, SLOW_QUERY, true, "c", cx);

    // A caret near the long row's end, on no match: the next match is
    // there, but stepping to it passes the limit. The `a` on the next row
    // isn't it.
    let near_end = 2 + SLOW_ROW - 2;
    select(&editor, near_end..near_end, cx);
    run("find.replace_next", cx);
    assert_eq!(label(&pane, cx), too_long());
    assert_eq!(selection(&editor, cx), near_end..near_end);
    assert!(text(&editor, cx) == original);
}

#[gpui::test]
fn a_search_over_several_rows(cx: &mut TestAppContext) {
    let _serial = serially(SLOW_LIMIT);
    let original = slow_text();
    let (_dir, pane, editor, cx) = opened(&original, cx);
    replace_row(&pane, SLOW_QUERY, true, "c", cx);

    // From the first row, which has none, to the first match of the next.
    select(&editor, 0..0, cx);
    run("find.replace_next", cx);
    assert_eq!(selection(&editor, cx), 2..3);
    // From the last row's end round to the first row with one: the long
    // row's first match, found without stepping through the rest.
    let end = original.len() - 1;
    select(&editor, end..end, cx);
    run("find.replace_next", cx);
    assert_eq!(selection(&editor, cx), 2..3);
    assert!(text(&editor, cx) == original);

    // Out of time from the start, it changes nothing, and says so.
    select(&editor, 0..0, cx);
    let limit = limited(Duration::ZERO, SLOW_LIMIT);
    run("find.replace_next", cx);
    drop(limit);
    assert_eq!(label(&pane, cx), too_long());
    assert_eq!(selection(&editor, cx), 0..0);
    assert!(text(&editor, cx) == original);
}

/// A regex each call of which, on [`long_text`]'s long row, takes long
/// enough to measure (tens of milliseconds): it tries 50 words to the
/// row's end from each start before taking an `a`, or finding none.
const COSTLY_QUERY: &str = r"(?:\w+\W*){50}z|a";

/// How many `b`s follow [`long_text`]'s `a`s on its long row.
const LONG_ROW: usize = 4_000;

/// How many `a`s [`many_text`]'s long row has, each a match that takes a
/// costly call to find: finding them all takes more calls than a step to the first should.
const MANY_A: usize = 40;

/// `b`; a long row, `aaa` and [`LONG_ROW`] `b`s, whose three matches each
/// take a costly call to find (and a fourth call finds there's no other);
/// and `a`.
fn long_text() -> String {
    format!("b\naaa{}\na\n", "b".repeat(LONG_ROW))
}

/// `b`, a long row of [`MANY_A`] `a`s and [`LONG_ROW`] `b`s, and `a`.
fn many_text() -> String {
    format!("b\n{}{}\na\n", "a".repeat(MANY_A), "b".repeat(LONG_ROW))
}

/// How long one regex call takes to find [`COSTLY_QUERY`]'s first match on
/// the long row, the longest of a few tries: a step that makes one must
/// have that long.
fn one_call() -> Duration {
    let pattern = Pattern::from_query(&FindQuery {
        text: COSTLY_QUERY.into(),
        regex: true,
        match_case: false,
        whole_word: false,
    })
    .unwrap();
    let text = Text::new(&long_text());
    let row = 2..2 + 3 + LONG_ROW;
    (0..3)
        .map(|_| {
            let start = Instant::now();
            let mut deadline = Deadline::never();
            let found = matches_reaching_until(&text, 1, row.clone(), &pattern, 1, &mut deadline);
            assert_eq!(found.first(), Some(&(2..3)));
            start.elapsed()
        })
        .max()
        .unwrap()
}

#[gpui::test]
fn the_wanted_match_ends_the_search(cx: &mut TestAppContext) {
    let _serial = serially(DEFAULT_TIME_LIMIT);
    // Short enough that each count of the long row's matches, which takes
    // their number of calls, is soon cut short.
    let _setup = limited(SETUP_LIMIT, DEFAULT_TIME_LIMIT);
    let (_dir, pane, editor, cx) = opened(&many_text(), cx);
    select(&editor, 0..0, cx);
    replace_row(&pane, COSTLY_QUERY, true, "c", cx);
    select(&editor, 0..0, cx);

    // From the first row, which has none, to the long row's first match:
    // a call finds it, and the search stops there. One that went on to the
    // matches after it would make a call for each.
    let limit = limited(NO_LIMIT, SETUP_LIMIT);
    let before = regex_calls();
    dispatch("find.replace_next", cx);
    let calls = regex_calls() - before;
    drop(limit);
    draw(cx);
    assert_ne!(label(&pane, cx), too_long());
    assert_eq!(selection(&editor, cx), 2..3);
    assert!(
        calls <= WANTED_CALLS,
        "{calls} regex calls, of the row's {MANY_A}"
    );
}

#[gpui::test]
fn a_match_found_past_the_limit_is_an_interruption(cx: &mut TestAppContext) {
    let _serial = serially(DEFAULT_TIME_LIMIT);
    let call = one_call();
    let _setup = limited(call * 4, DEFAULT_TIME_LIMIT);
    let original = long_text();
    let (_dir, pane, editor, cx) = opened(&original, cx);
    replace_row(&pane, COSTLY_QUERY, true, "c", cx);
    select(&editor, 0..0, cx);

    // The one call that finds the long row's first match passes the limit
    // before it returns: the match isn't taken.
    let limit = limited(call / 4, call * 4);
    run("find.replace_next", cx);
    drop(limit);
    assert_eq!(label(&pane, cx), too_long());
    assert_eq!(selection(&editor, cx), 0..0);
    assert!(text(&editor, cx) == original);
}

#[gpui::test]
fn an_interrupted_replace_all_keeps_a_visual_selection(cx: &mut TestAppContext) {
    let _serial = serially(DEFAULT_TIME_LIMIT);
    let (_dir, pane, editor, cx) = opened("one\ntwo\n", cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    let mode = |cx: &mut VisualTestContext| editor.read_with(cx, |editor, _| editor.vim_mode());
    cx.simulate_keystrokes("v j");
    draw(cx);
    let visual = selection(&editor, cx);
    replace_row(&pane, "zzz", false, "x", cx);
    assert_eq!(mode(cx), Some("visual"));
    assert_eq!(selection(&editor, cx), visual);

    let limit = limited(Duration::ZERO, DEFAULT_TIME_LIMIT);
    run("find.replace_all", cx);
    drop(limit);
    assert_eq!(label(&pane, cx), too_long());
    assert_eq!(mode(cx), Some("visual"));
    assert_eq!(selection(&editor, cx), visual);
}
