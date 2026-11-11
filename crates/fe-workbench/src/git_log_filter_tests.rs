//! Filtering the Git Log's commits from the box at its top: typing reads
//! the list again once it stops, Escape and the clear button list every
//! commit again, and the graph is laid out over the commits listed, on
//! repositories built with the `git` CLI in temporary directories.

use std::{path::Path, process::Command, sync::Arc};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_git::{CommitInfo, LogCommit, LogFilter, graph::GraphRow};
use fe_gpui::commands::RunCommand;
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use super::{
    FILTER_DELAY, GitLogPanel,
    git_log_open_tests::{draw, fixture, lines, panel, press, row_of, selected, window},
};

/// The filter box's text.
fn box_text(panel: &Entity<GitLogPanel>, cx: &mut VisualTestContext) -> String {
    let input = panel.read_with(cx, |panel, _| panel.filter_input.clone());
    input.read_with(cx, |input, _| input.text().to_owned())
}

/// Whether the filter box has focus, and whether the list does.
fn focus(panel: &Entity<GitLogPanel>, cx: &mut VisualTestContext) -> (bool, bool) {
    cx.update(|window, cx| {
        let panel = panel.read(cx);
        let input = panel.filter_input.focus_handle(cx).is_focused(window);
        (input, panel.focus.is_focused(window))
    })
}

/// How many times the list has been read.
fn reads(panel: &Entity<GitLogPanel>, cx: &mut VisualTestContext) -> u64 {
    panel.read_with(cx, |panel, _| panel.read)
}

/// Lets `by` pass, then lets what it set off land and draws.
fn wait(by: std::time::Duration, cx: &mut VisualTestContext) {
    cx.executor().advance_clock(by);
    draw(cx);
}

/// The commit summaries `lines` lists, without their ids.
fn summaries(lines: &[String]) -> Vec<String> {
    (lines.iter())
        .map(|line| {
            line.split_once("  ")
                .map_or(line.as_str(), |(_, rest)| rest)
        })
        .map(str::to_owned)
        .collect()
}

#[gpui::test]
fn box_typing_rereads_the_list(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let (workbench, mut cx) = window(cx, dir.path());
    let cx = &mut cx;
    let panel = panel(&workbench, cx);
    let all = lines(&workbench, cx);
    assert_eq!(all.len(), 6, "{all:?}");
    let before = reads(&panel, cx);

    // Typing reads nothing until it has stopped for the delay.
    press("/", cx);
    assert_eq!(focus(&panel, cx), (true, false));
    cx.simulate_input("wo");
    wait(FILTER_DELAY / 2, cx);
    cx.simulate_input("rk");
    wait(FILTER_DELAY / 2, cx);
    assert_eq!(box_text(&panel, cx), "work");
    assert_eq!(lines(&workbench, cx), all, "not read again yet");
    assert_eq!(reads(&panel, cx), before);
    wait(FILTER_DELAY, cx);
    assert_eq!(
        reads(&panel, cx),
        before + 1,
        "read once, for the last edit"
    );
    let filtered = lines(&workbench, cx);
    let mut kept = summaries(&filtered);
    kept.sort();
    assert_eq!(kept, ["main work", "topic work"]);
    // In the order the whole list has them.
    let order: Vec<_> = (all.iter())
        .filter(|line| filtered.contains(line))
        .collect();
    assert_eq!(filtered.iter().collect::<Vec<_>>(), order);
    assert_eq!(selected(&workbench, cx), Some(0));

    // Escape in the box empties it and lists every commit, at once, with
    // the keys back on the list.
    press("escape", cx);
    assert_eq!(box_text(&panel, cx), "");
    assert_eq!(lines(&workbench, cx), all);
    assert_eq!(focus(&panel, cx), (false, true));
    press("j", cx);
    assert_eq!(selected(&workbench, cx), Some(1));

    // The clear button does too.
    press("/", cx);
    cx.simulate_input("rename");
    wait(FILTER_DELAY, cx);
    assert_eq!(summaries(&lines(&workbench, cx)), ["rename"]);
    let clear = cx
        .debug_bounds("icon-button:git-log-filter-clear")
        .expect("the clear button shows while the box has text");
    cx.simulate_click(clear.center(), Modifiers::none());
    draw(cx);
    assert_eq!(box_text(&panel, cx), "");
    assert_eq!(lines(&workbench, cx), all);
    assert!(
        cx.debug_bounds("icon-button:git-log-filter-clear")
            .is_none(),
        "no clear button for an empty box"
    );
    // Its delayed read, from the box's emptying, reads nothing more.
    let after = reads(&panel, cx);
    wait(FILTER_DELAY * 2, cx);
    assert_eq!(reads(&panel, cx), after);
}

#[gpui::test]
fn box_no_matches_says_so(cx: &mut TestAppContext) {
    let (dir, ids) = fixture();
    let (workbench, mut cx) = window(cx, dir.path());
    let cx = &mut cx;
    let panel = panel(&workbench, cx);
    let shown = |cx: &mut VisualTestContext| panel.read_with(cx, |panel, _| panel.shown_message());

    press("/", cx);
    cx.simulate_input("no such commit");
    wait(FILTER_DELAY, cx);
    assert!(lines(&workbench, cx).is_empty());
    assert_eq!(shown(cx), Some("No commits match".to_owned()));
    assert_eq!(selected(&workbench, cx), None);

    // By id prefix, any case; and the whole list again after.
    let (_, edit) = ids.iter().find(|(summary, _)| *summary == "edit").unwrap();
    press("escape", cx);
    assert_eq!(shown(cx), None);
    press("/", cx);
    let input = panel.read_with(cx, |panel, _| panel.filter_input.clone());
    input.update(cx, |input, cx| input.insert(&edit[..6].to_uppercase(), cx));
    press("enter", cx);
    assert_eq!(summaries(&lines(&workbench, cx)), ["edit"]);
}

#[gpui::test]
fn vim_slash_focuses_the_box_and_enter_returns(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let (workbench, mut cx) = window(cx, dir.path());
    let cx = &mut cx;
    let panel = panel(&workbench, cx);
    let all = lines(&workbench, cx);
    assert_eq!(focus(&panel, cx), (false, true));
    press("j", cx);
    assert_eq!(selected(&workbench, cx), Some(1));

    // `/` focuses the box; the list's vim keys type there.
    press("/", cx);
    assert_eq!(focus(&panel, cx), (true, false));
    cx.simulate_input("jkgg");
    press("q o", cx);
    assert_eq!(box_text(&panel, cx), "jkggqo");
    assert_eq!(
        selected(&workbench, cx),
        Some(1),
        "the list's keys didn't move"
    );
    assert_eq!(lines(&workbench, cx), all, "nothing opened");
    press(
        "backspace backspace backspace backspace backspace backspace",
        cx,
    );
    assert_eq!(box_text(&panel, cx), "");

    // Enter reads at once and gives the keys back to the list.
    cx.simulate_input("work");
    press("enter", cx);
    assert_eq!(focus(&panel, cx), (false, true));
    let filtered = lines(&workbench, cx);
    assert_eq!(filtered.len(), 2, "{filtered:?}");
    assert_eq!(selected(&workbench, cx), Some(0));
    press("j", cx);
    assert_eq!(selected(&workbench, cx), Some(1));

    // `/` again selects the box's text, which typing replaces.
    press("/", cx);
    cx.simulate_input("merge");
    press("enter", cx);
    assert_eq!(box_text(&panel, cx), "merge");
    assert_eq!(summaries(&lines(&workbench, cx)), ["merge"]);

    // Running the command again keeps the filter.
    send("git.log", cx);
    draw(cx);
    assert_eq!(summaries(&lines(&workbench, cx)), ["merge"]);
    press("escape", cx);
    press("/", cx);
    press("escape", cx);
    assert_eq!(lines(&workbench, cx), all);

    // With vim off, `/` is the list's no more.
    cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, false));
    draw(cx);
    press("/", cx);
    assert_eq!(focus(&panel, cx), (false, true));
    assert_eq!(box_text(&panel, cx), "");
    let edit = row_of(&lines(&workbench, cx), "edit");
    assert!(edit > 0);
}

/// Runs the command `id`, as the palette does.
fn send(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
}

/// A commit `id` with `parents`, for laying out by hand.
fn entry(id: &str, parents: &[&str]) -> LogCommit {
    LogCommit {
        commit: Arc::new(CommitInfo {
            id: id.to_owned(),
            short: id.to_owned(),
            author: "t".to_owned(),
            email: "t@t".to_owned(),
            time: 0,
            offset_minutes: 0,
            summary: format!("x {id}"),
            message: format!("x {id}"),
        }),
        parents: parents.iter().map(|parent| parent.to_string()).collect(),
        refs: Vec::new(),
    }
}

/// The panel's graph rows, by commit id.
fn graph(panel: &Entity<GitLogPanel>, cx: &mut VisualTestContext) -> Vec<(String, GraphRow)> {
    panel.read_with(cx, |panel, _| panel.graph())
}

/// Whether `row`'s commit's lane goes on below it.
fn goes_on(row: &GraphRow) -> bool {
    row.bottom.iter().any(|line| line.from == row.column)
}

/// The columns of the lines leaving `row`'s dot.
fn leaving(row: &GraphRow) -> Vec<usize> {
    (row.bottom.iter())
        .filter(|line| line.from == row.column)
        .map(|line| line.to)
        .collect()
}

#[gpui::test]
fn graph_lays_out_the_filtered_rows(cx: &mut TestAppContext) {
    // main: base, x one, plain, x two; side off x one: x side; x merge of
    // x two and x side. Filtered by `x`.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let git = |args: &[&str]| run_git(root, args);
    git(&["init", "-q"]);
    let commit = |message: &str| {
        git(&["commit", "-q", "--allow-empty", "-m", message]);
        git(&["rev-parse", "HEAD"])
    };
    commit("base");
    let one = commit("x one");
    commit("plain");
    let two = commit("x two");
    git(&["checkout", "-q", "-b", "side", &one]);
    let side = commit("x side");
    git(&["checkout", "-q", "main"]);
    git(&["merge", "-q", "--no-ff", "side", "-m", "x merge"]);
    let merge = git(&["rev-parse", "HEAD"]);

    let (workbench, mut cx) = window(cx, root);
    let cx = &mut cx;
    let panel = panel(&workbench, cx);
    press("/", cx);
    cx.simulate_input("x");
    press("enter", cx);
    let rows = graph(&panel, cx);
    let ids: Vec<&str> = rows.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids.len(), 4, "{:?}", summaries(&lines(&workbench, cx)));
    assert_eq!(ids[0], merge);
    assert_eq!(ids[3], one);
    let row = |id: &str| &rows.iter().find(|(other, _)| other == id).unwrap().1;

    // The merge runs to both its parents, both listed.
    assert_eq!(leaving(row(&merge)).len(), 2);
    // x two's parent, plain, isn't listed: its lane ends at its dot.
    assert!(!goes_on(row(&two)), "{:?}", row(&two));
    // x side's runs on to x one, where it ends: base isn't listed.
    assert!(goes_on(row(&side)));
    let into_one = row(&one)
        .top
        .iter()
        .filter(|line| line.to == row(&one).column);
    assert_eq!(into_one.count(), 1, "{:?}", row(&one));
    assert!(row(&one).bottom.is_empty(), "no lane is left open below");
    // No lane waits on a commit that isn't listed.
    let widest = panel.read_with(cx, |panel, _| panel.lanes);
    assert_eq!(widest, 2);
    // It draws.
    draw(cx);
    let seen = panel.read_with(cx, |panel, _| panel.rows_seen());
    assert!(seen.borrow().segments.contains_key(&0));
}

#[gpui::test]
fn graph_joins_a_parent_listed_by_a_later_page(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let (workbench, mut cx) = window(cx, dir.path());
    let cx = &mut cx;
    let panel = panel(&workbench, cx);
    // A filtered list whose first page's last commit's parent comes in the
    // second, and whose other parent never comes.
    panel.update(cx, |panel, _| {
        panel.filter = LogFilter::new("x");
        panel.clear_rows();
        panel.tail = super::Tail::None;
        panel.append(vec![entry("c", &["b", "gone"]), entry("d", &["gone"])]);
    });
    let first = graph(&panel, cx);
    assert!(!goes_on(&first[0].1), "{:?}", first[0].1);
    assert!(!goes_on(&first[1].1), "{:?}", first[1].1);
    assert_eq!(panel.read_with(cx, |panel, _| panel.lanes), 1);

    panel.update(cx, |panel, _| panel.append(vec![entry("b", &["a"])]));
    let second = graph(&panel, cx);
    let ids: Vec<&str> = second.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["c", "d", "b"]);
    // c's lane now runs to b, past d; d's still ends; b's does too.
    assert_eq!(leaving(&second[0].1), [0]);
    assert!(!goes_on(&second[1].1));
    assert_eq!(second[2].1.column, 0);
    assert!(second[2].1.bottom.is_empty());
    assert_eq!(panel.read_with(cx, |panel, _| panel.lanes), 2);
    draw(cx);
}

/// Runs `git` in `dir` with the developer's own config shut out; its
/// output, trimmed.
fn run_git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

// The box's caret keys move its caret and delete forward, as the other
// boxes' do: the keymap binds them where the panel hosts it.
#[gpui::test]
fn box_caret_keys_edit_inside_the_query(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let (workbench, mut cx) = window(cx, dir.path());
    let cx = &mut cx;
    let panel = panel(&workbench, cx);
    press("/", cx);
    cx.simulate_input("wrk");
    press("left left", cx);
    cx.simulate_input("o");
    assert_eq!(box_text(&panel, cx), "work");
    press("home delete", cx);
    assert_eq!(box_text(&panel, cx), "ork");
    press("end backspace", cx);
    cx.simulate_input("ks");
    press("right", cx);
    assert_eq!(box_text(&panel, cx), "orks");
    press("backspace", cx);
    assert_eq!(box_text(&panel, cx), "ork");
    assert_eq!(focus(&panel, cx), (true, false), "the keys stay in the box");
    wait(FILTER_DELAY, cx);
    let mut kept = summaries(&lines(&workbench, cx));
    kept.sort();
    assert_eq!(kept, ["main work", "topic work"]);
}
