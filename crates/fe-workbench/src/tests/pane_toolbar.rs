//! The pane toolbar's breadcrumbs, its pickers and the tab bar's buttons.

use std::{fs, path::Path, time::Duration};

use fe_command::{ContextFrame, ContextStack, Invocation, Source};
use fe_gpui::{EditorView, commands::RunCommand};
use fe_ui::{CrumbKind, TAB_HEIGHT, TOOLBAR_HEIGHT, file_icon};
use gpui::{
    Bounds, Entity, Focusable as _, Modifiers, Pixels, TestAppContext, VisualTestContext, px, size,
};

use super::{open, open_root};
use crate::{Launch, Pane, Workbench, pane_toolbar::NavButtons};

const MOD_RS: &str = "\
mod outer {
    pub struct S;
    impl S {
        fn a() {}
        fn b() {
            let x = 1;
        }
    }
}

fn main() {}
";

/// `src/util/mod.rs`, with siblings for the entries picker, and more.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let util = dir.path().join("src/util");
    for sub in ["Inner", "apple"] {
        fs::create_dir_all(util.join(sub)).unwrap();
    }
    fs::write(util.join("mod.rs"), MOD_RS).unwrap();
    for name in ["zeta.rs", "Banana.rs", "alpha.txt", "Inner/deep.rs"] {
        fs::write(util.join(name), "x\n").unwrap();
    }
    fs::write(dir.path().join("README.md"), "# hi\n").unwrap();
    dir
}

/// A workbench rooted at `dir`, focused as the app focuses it, showing
/// `src/util/mod.rs` with its outline landed.
fn opened<'a>(
    cx: &'a mut TestAppContext,
    dir: &tempfile::TempDir,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    open_file(&workbench, &dir.path().join("src/util/mod.rs"), cx);
    (workbench, cx)
}

fn open_file(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    let pane = active(workbench, cx);
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path.to_owned(), window, cx)));
    settle(cx);
}

/// Runs and draws until the outline has landed and the crumbs show it.
fn settle(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Pane> {
    workbench.read_with(cx, |workbench, _| workbench.pane().clone())
}

fn panes(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Entity<Pane>> {
    workbench.read_with(cx, |workbench, _| {
        workbench
            .pane_group()
            .panes()
            .into_iter()
            .cloned()
            .collect()
    })
}

fn editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a file is shown")
}

fn focused(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

/// Puts `editor`'s caret at the start of the first `needle`, plus `skip`.
fn caret_at(editor: &Entity<EditorView>, needle: &str, skip: usize, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, cx| {
        let offset = editor.model().text().to_string().find(needle).unwrap() + skip;
        editor.model_mut().set_cursor(offset);
        cx.notify();
    });
    settle(cx);
}

fn caret(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> usize {
    editor.read_with(cx, |editor, _| crate::pane_toolbar::caret(editor))
}

/// The labels of the crumbs `pane` drew of `kind`.
fn crumbs(pane: &Entity<Pane>, kind: CrumbKind, cx: &mut VisualTestContext) -> Vec<String> {
    pane.read_with(cx, |pane, _| {
        let drawn = pane.drawn_crumbs().iter();
        let of_kind = drawn.filter(|(_, crumb_kind, _)| *crumb_kind == kind);
        of_kind.map(|(label, ..)| label.clone()).collect()
    })
}

/// The labels of every crumb `pane` drew.
fn all_crumbs(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> Vec<String> {
    pane.read_with(cx, |pane, _| {
        pane.drawn_crumbs()
            .iter()
            .map(|(label, ..)| label.clone())
            .collect()
    })
}

fn bounds(selector: String, cx: &mut VisualTestContext) -> Option<Bounds<Pixels>> {
    // `debug_bounds` takes a `&'static str`.
    cx.debug_bounds(selector.leak())
}

fn click(at: Bounds<Pixels>, cx: &mut VisualTestContext) {
    cx.simulate_click(at.center(), Modifiers::none());
    settle(cx);
}

fn click_crumb(ix: usize, label: &str, cx: &mut VisualTestContext) {
    let crumb = bounds(format!("crumb:{ix}:{label}"), cx).expect("drawn");
    click(crumb, cx);
}

/// The toolbar picker's rows, if it shows.
fn picker_rows(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<Vec<String>> {
    workbench.read_with(cx, |workbench, cx| {
        let picker = workbench.toolbar_picker()?;
        Some(picker.read(cx).delegate.labels())
    })
}

fn picker_selected(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    workbench.read_with(cx, |workbench, cx| {
        workbench
            .toolbar_picker()
            .expect("shown")
            .read(cx)
            .selected()
    })
}

fn invocations(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Invocation> {
    workbench.read_with(cx, |workbench, _| workbench.commands_run.clone())
}

fn mouse_runs(workbench: &Entity<Workbench>, command: &str, cx: &mut VisualTestContext) -> usize {
    let runs = invocations(workbench, cx);
    runs.iter()
        .filter(|run| run.command.as_str() == command && run.source == Source::Mouse)
        .count()
}

fn chains(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    workbench.read_with(cx, |workbench, _| workbench.chains_computed)
}

fn jobs(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).outline().read(cx).jobs_started()
    })
}

fn action(pane: &Entity<Pane>, ix: usize, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    pane.read_with(cx, |pane, _| pane.action_bounds(ix).expect("drawn"))
}

fn content(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    pane.read_with(cx, |pane, _| pane.content_bounds())
}

fn run(command: &str, cx: &mut VisualTestContext) {
    let context = ContextStack::new().with(ContextFrame::new("workbench"));
    let run = RunCommand(Invocation::of(command).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    settle(cx);
}

#[gpui::test]
fn layout_and_buttons(cx: &mut TestAppContext) {
    let dir = project();
    for ix in 0..30 {
        fs::write(dir.path().join(format!("tab_{ix:02}.txt")), "t\n").unwrap();
    }
    let (workbench, cx) = opened(cx, &dir);
    let pane = active(&workbench, cx);
    let pane_bounds = pane.read_with(cx, |pane, _| pane.bounds());
    let tab = pane.read_with(cx, |pane, _| pane.tab_bounds(0).expect("drawn"));
    assert_eq!(tab.top(), pane_bounds.top());
    assert!(tab.bottom() <= pane_bounds.top() + TAB_HEIGHT);

    let toolbar = bounds("pane-toolbar".into(), cx).expect("drawn");
    assert_eq!(toolbar.top(), pane_bounds.top() + TAB_HEIGHT);
    assert_eq!(toolbar.size.height, TOOLBAR_HEIGHT);
    assert_eq!(toolbar.left(), pane_bounds.left());
    assert_eq!(toolbar.size.width, pane_bounds.size.width);
    assert_eq!(content(&pane, cx).top(), toolbar.bottom());

    for (ix, label) in ["src", "util", "mod.rs"].into_iter().enumerate() {
        let crumb = bounds(format!("crumb:{ix}:{label}"), cx).expect(label);
        assert!(toolbar.contains(&crumb.center()), "{label}");
    }
    let drawn = pane.read_with(cx, |pane, _| pane.drawn_crumbs()[..3].to_vec());
    assert_eq!(
        drawn,
        [
            ("src".to_string(), CrumbKind::Path, None),
            ("util".to_string(), CrumbKind::Path, None),
            (
                "mod.rs".to_string(),
                CrumbKind::File,
                Some(file_icon("mod.rs"))
            ),
        ]
    );

    // Back and forward are disabled: clicks dispatch nothing.
    let before = invocations(&workbench, cx).len();
    for id in ["back", "forward"] {
        let button = bounds(format!("icon-button:{id}"), cx).expect(id);
        assert!(toolbar.contains(&button.center()));
        click(button, cx);
    }
    assert_eq!(invocations(&workbench, cx).len(), before);

    // The tab bar's buttons sit at its right end, and stay put as it
    // scrolls.
    let actions: Vec<_> = (0..3).map(|ix| action(&pane, ix, cx)).collect();
    let end = pane_bounds.right() - actions[2].right();
    assert!(end >= px(0.) && end <= px(4.), "{end:?} from the end");
    for (button, id) in actions
        .iter()
        .zip(["split-right", "split-down", "pane-menu"])
    {
        assert!(button.top() >= pane_bounds.top(), "{id}");
        assert!(button.bottom() <= pane_bounds.top() + TAB_HEIGHT, "{id}");
        assert_eq!(bounds(format!("icon-button:{id}"), cx), Some(*button));
    }
    for ix in 0..30 {
        open_file(&workbench, &dir.path().join(format!("tab_{ix:02}.txt")), cx);
    }
    let last = pane.read_with(cx, |pane, _| pane.tab_bounds(30).expect("drawn"));
    assert!(
        last.right() > actions[0].left(),
        "the tabs overflow the bar"
    );
    pane.update(cx, |pane, _| pane.scroll_tabs(px(400.)));
    draw(cx);
    let scrolled: Vec<_> = (0..3).map(|ix| action(&pane, ix, cx)).collect();
    assert_eq!(scrolled, actions);
}

#[gpui::test]
fn symbols_follow_the_caret(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = opened(cx, &dir);
    let pane = active(&workbench, cx);
    let editor = editor(&workbench, cx);
    let symbols = |cx: &mut VisualTestContext| crumbs(&pane, CrumbKind::Symbol, cx);

    caret_at(&editor, "let x", 0, cx);
    assert_eq!(symbols(cx), ["outer", "impl S", "b"]);
    caret_at(&editor, "fn main", 4, cx);
    assert_eq!(symbols(cx), ["main"]);
    caret_at(&editor, "\nfn main", 0, cx);
    assert_eq!(symbols(cx), Vec::<String>::new());

    // Hidden or not, the outline follows the editor, and so do the crumbs.
    cx.simulate_keystrokes("cmd-b");
    caret_at(&editor, "fn a", 3, cx);
    assert_eq!(symbols(cx), ["outer", "impl S", "a"]);
    cx.simulate_keystrokes("cmd-shift-e");
    caret_at(&editor, "pub struct", 11, cx);
    assert_eq!(symbols(cx), ["outer", "S"]);
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));

    // One chain per caret move, no outline job of the toolbar's own.
    let (jobs_before, chains_before) = (jobs(&workbench, cx), chains(&workbench, cx));
    for ix in 0..20 {
        caret_at(&editor, "fn", ix % 7, cx);
        caret_at(&editor, "mod", 0, cx);
    }
    assert_eq!(jobs(&workbench, cx), jobs_before);
    assert_eq!(chains(&workbench, cx), chains_before + 40);
    for _ in 0..5 {
        draw(cx);
    }
    assert_eq!(chains(&workbench, cx), chains_before + 40);

    // An edit moves the caret, then its outline lands: one chain each.
    editor.update(cx, |editor, cx| {
        editor.model_mut().insert("fn c() {}\n");
        cx.notify();
    });
    settle(cx);
    assert_eq!(jobs(&workbench, cx), jobs_before + 1);
    assert_eq!(chains(&workbench, cx), chains_before + 42);
    assert_eq!(symbols(cx), ["outer"]);

    // Plain text has no outline: its symbols go, though the caret stays.
    editor.update(cx, |editor, cx| editor.set_language(None, cx));
    settle(cx);
    assert_eq!(symbols(cx), Vec::<String>::new());
    editor.update(cx, |editor, cx| editor.set_language(Some("rust"), cx));
    settle(cx);
    assert_eq!(symbols(cx), ["outer"]);

    // A pane made active again keeps its chain while its outline is still
    // to come, but not once none can come: plain text or a language with no
    // outline, set before the outline lands, clear it with no caret move.
    cx.simulate_keystrokes("cmd-\\");
    settle(cx);
    assert_ne!(active(&workbench, cx), pane);
    for language in [None, Some("json")] {
        cx.update(|_, cx| pane.update(cx, |_, cx| cx.emit(crate::PaneEvent::Activated)));
        draw(cx);
        assert_eq!(active(&workbench, cx), pane);
        assert_eq!(symbols(cx), ["outer"], "kept while pending");
        editor.update(cx, |editor, cx| editor.set_language(language, cx));
        draw(cx);
        if language.is_none() {
            // JSON's grammar loads in the background: an outline could
            // still come until it has.
            assert_eq!(symbols(cx), Vec::<String>::new(), "plain text");
        }
        settle(cx);
        assert_eq!(symbols(cx), Vec::<String>::new(), "{language:?}");
        // Back to Rust, its outline landed, then the other pane active.
        editor.update(cx, |editor, cx| editor.set_language(Some("rust"), cx));
        settle(cx);
        assert_eq!(symbols(cx), ["outer"]);
        cx.simulate_keystrokes("cmd-k cmd-right");
        settle(cx);
        assert_ne!(active(&workbench, cx), pane);
    }
}

#[test]
fn matching_follows_lowercase_expansions() {
    use crate::toolbar_picker::subsequence;
    let lower = |query: &str| -> Vec<char> { query.chars().flat_map(char::to_lowercase).collect() };
    // `İ` lowercases to `i` and a combining dot.
    for query in ["İstanbul", "istanbul", "ISTANBUL", "İst"] {
        let found = subsequence(&lower(query), "İstanbul.rs");
        assert_eq!(found.as_ref().map(|found| found[0]), Some(0), "{query}");
    }
    assert_eq!(
        subsequence(&lower("İstanbul"), "İstanbul"),
        Some((0..8).collect())
    );
    assert_eq!(subsequence(&lower("ba"), "Banana.rs"), Some(vec![0, 1]));
    assert_eq!(subsequence(&lower("bz"), "Banana.rs"), None);
}

#[gpui::test]
fn path_crumbs_open_entry_pickers(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = opened(cx, &dir);
    let pane = active(&workbench, cx);
    let editor = editor(&workbench, cx);
    let entries = [
        "apple",
        "Inner",
        "alpha.txt",
        "Banana.rs",
        "mod.rs",
        "zeta.rs",
    ];
    let picker = |workbench: &Entity<Workbench>, cx: &mut VisualTestContext| {
        workbench.read_with(cx, |workbench, _| workbench.toolbar_picker().cloned())
    };

    click_crumb(1, "util", cx);
    assert_eq!(picker_rows(&workbench, cx).unwrap(), entries);
    assert_eq!(picker_selected(&workbench, cx), 0);
    let shown = picker(&workbench, cx).unwrap();
    shown.read_with(cx, |picker, _| {
        assert_eq!(picker.delegate.reads(), (1, 1), "read off the main thread");
        assert_eq!(picker.delegate.status_text(), Some("src/util".into()));
    });
    cx.simulate_keystrokes("escape");
    assert_eq!(picker_rows(&workbench, cx), None);
    assert!(focused(&editor, cx));

    click_crumb(2, "mod.rs", cx);
    assert_eq!(picker_rows(&workbench, cx).unwrap(), entries);
    assert_eq!(picker_selected(&workbench, cx), 4);

    cx.simulate_input("ba");
    settle(cx);
    assert_eq!(picker_rows(&workbench, cx).unwrap(), ["Banana.rs"]);
    let shown = picker(&workbench, cx).unwrap();
    shown.read_with(cx, |picker, _| {
        assert_eq!(picker.delegate.highlights(0), vec![0..2])
    });

    // A directory lists its entries, the query cleared.
    cx.simulate_keystrokes("backspace backspace");
    cx.simulate_input("inr");
    settle(cx);
    assert_eq!(picker_rows(&workbench, cx).unwrap(), ["Inner"]);
    cx.simulate_keystrokes("enter");
    settle(cx);
    assert_eq!(picker_rows(&workbench, cx).unwrap(), ["deep.rs"]);
    shown.read_with(cx, |picker, cx| {
        assert_eq!(picker.input().read(cx).text(), "");
        assert_eq!(picker.delegate.status_text(), Some("src/util/Inner".into()));
    });

    // A file opens in the pane, active and focused.
    cx.simulate_keystrokes("enter");
    settle(cx);
    assert_eq!(picker_rows(&workbench, cx), None);
    assert_eq!(active(&workbench, cx), pane);
    let deep = self::editor(&workbench, cx);
    let path = deep.read_with(cx, |deep, _| deep.model().path().map(Path::to_owned));
    assert_eq!(path, Some(dir.path().join("src/util/Inner/deep.rs")));
    assert!(focused(&deep, cx));
    assert_eq!(all_crumbs(&pane, cx), ["src", "util", "Inner", "deep.rs"]);
}

#[gpui::test]
fn symbol_crumbs_open_sibling_pickers(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = opened(cx, &dir);
    let pane = active(&workbench, cx);
    let editor = editor(&workbench, cx);
    caret_at(&editor, "let x", 0, cx);

    click_crumb(5, "b", cx);
    assert_eq!(picker_rows(&workbench, cx).unwrap(), ["a", "b"]);
    assert_eq!(picker_selected(&workbench, cx), 1);
    cx.simulate_keystrokes("up enter");
    settle(cx);
    assert_eq!(picker_rows(&workbench, cx), None);
    assert_eq!(caret(&editor, cx), MOD_RS.find("a()").unwrap());
    assert!(focused(&editor, cx));
    assert_eq!(
        crumbs(&pane, CrumbKind::Symbol, cx),
        ["outer", "impl S", "a"]
    );

    // Split, then edit the shared buffer from the other pane while the
    // picker is open: the pick is refused and the list rebuilt.
    caret_at(&editor, "let x", 0, cx);
    cx.simulate_keystrokes("cmd-\\");
    settle(cx);
    let right = active(&workbench, cx);
    assert_ne!(right, pane);
    let split = self::editor(&workbench, cx);
    caret_at(&split, "let x", 0, cx);
    assert_eq!(
        crumbs(&right, CrumbKind::Symbol, cx),
        ["outer", "impl S", "b"]
    );
    let mut crumb = bounds("crumb:5:b".into(), cx).unwrap();
    let (left_x, right_x) = (
        pane.read_with(cx, |pane, _| pane.bounds().left()),
        right.read_with(cx, |pane, _| pane.bounds().left()),
    );
    if crumb.left() < right_x {
        crumb.origin.x += right_x - left_x;
    }
    click(crumb, cx);
    assert_eq!(picker_rows(&workbench, cx).unwrap(), ["a", "b"]);
    cx.simulate_keystrokes("up");
    editor.update(cx, |editor, cx| {
        editor.model_mut().set_cursor(0);
        editor.model_mut().insert("// moved\n");
        cx.notify();
    });
    settle(cx);
    let jobs_before = jobs(&workbench, cx);
    cx.simulate_keystrokes("enter");
    settle(cx);
    assert_eq!(picker_rows(&workbench, cx).unwrap(), ["a", "b"], "refused");
    assert_eq!(picker_selected(&workbench, cx), 1, "rebuilt");
    assert_eq!(jobs(&workbench, cx), jobs_before);
    cx.simulate_keystrokes("up enter");
    settle(cx);
    assert_eq!(picker_rows(&workbench, cx), None);
    let a = "// moved\n".len() + MOD_RS.find("a()").unwrap();
    assert_eq!(caret(&split, cx), a);
    assert!(focused(&split, cx));
}

#[gpui::test]
fn buttons_and_pane_commands(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = opened(cx, &dir);

    // Enabled, back dispatches `nav.back`.
    workbench.update(cx, |workbench, cx| {
        let nav = NavButtons {
            back: true,
            forward: true,
        };
        workbench.set_nav_buttons(nav, cx)
    });
    settle(cx);
    click(bounds("icon-button:back".into(), cx).unwrap(), cx);
    assert_eq!(mouse_runs(&workbench, "nav.back", cx), 1);

    cx.simulate_keystrokes("cmd-\\");
    settle(cx);
    let [left, right] = panes(&workbench, cx).try_into().unwrap();
    assert_eq!(active(&workbench, cx), right);

    // Split Right in the inactive pane activates it and splits it.
    click(action(&left, 0, cx), cx);
    assert_eq!(panes(&workbench, cx).len(), 3);
    let new = active(&workbench, cx);
    assert_eq!(
        panes(&workbench, cx),
        [left.clone(), new.clone(), right.clone()]
    );
    assert_eq!(mouse_runs(&workbench, "pane.split_right", cx), 1);

    // Split Down likewise.
    click(action(&right, 1, cx), cx);
    assert_eq!(panes(&workbench, cx).len(), 4);
    assert_eq!(panes(&workbench, cx)[2], right);
    assert_eq!(mouse_runs(&workbench, "pane.split_down", cx), 1);

    // "…" lists the pane commands with their keys.
    click(action(&left, 2, cx), cx);
    let titles = [
        "Close Tab",
        "Close Pane",
        "Split Right",
        "Split Down",
        "Move Tab Left",
        "Move Tab Right",
        "Move Tab Up",
        "Move Tab Down",
        "Toggle Breadcrumbs",
    ];
    assert_eq!(picker_rows(&workbench, cx).unwrap(), titles);
    let shortcuts = workbench.read_with(cx, |workbench, cx| {
        let picker = workbench.toolbar_picker().unwrap();
        picker.read(cx).delegate.shortcuts()
    });
    let shortcut =
        |title: &str| shortcuts[titles.iter().position(|t| *t == title).unwrap()].clone();
    // The keymap is macOS's; its keys print as this platform prints them:
    // in symbols on macOS, spelled out elsewhere.
    let label = |mac: &str, other: &str| {
        Some(
            if cfg!(target_os = "macos") {
                mac
            } else {
                other
            }
            .to_owned(),
        )
    };
    assert_eq!(shortcut("Split Right"), label("⌘\\", "Super+\\"));
    assert_eq!(shortcut("Close Tab"), label("⌘W", "Super+W"));
    assert_eq!(shortcut("Close Pane"), None);
    assert_eq!(shortcut("Toggle Breadcrumbs"), None);

    // Close Pane closes the pane whose "…" was clicked.
    cx.simulate_input("clpa");
    settle(cx);
    assert_eq!(picker_rows(&workbench, cx).unwrap(), ["Close Pane"]);
    cx.simulate_keystrokes("enter");
    settle(cx);
    assert_eq!(picker_rows(&workbench, cx), None);
    let open = panes(&workbench, cx);
    assert_eq!(open.len(), 3);
    assert!(!open.contains(&left));
    let palette = invocations(&workbench, cx);
    let close = palette.last().unwrap();
    assert_eq!(
        (close.command.as_str(), close.source),
        ("pane.close", Source::Palette)
    );

    // Tooltips show the title and key.
    let split_right = action(&right, 0, cx);
    cx.simulate_mouse_move(split_right.center(), None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(500));
    draw(cx);
    let tooltip = right.read_with(cx, |pane, _| pane.last_tooltip());
    assert_eq!(tooltip, label("Split Right ⌘\\", "Split Right Super+\\"));
}

#[gpui::test]
fn toggle_hides_every_toolbar(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = opened(cx, &dir);
    cx.simulate_keystrokes("cmd-\\");
    settle(cx);
    let two = panes(&workbench, cx);
    let tops: Vec<_> = two.iter().map(|pane| content(pane, cx).top()).collect();

    run("view.toggle_breadcrumbs", cx);
    assert_eq!(bounds("pane-toolbar".into(), cx), None);
    for (pane, top) in two.iter().zip(&tops) {
        assert_eq!(content(pane, cx).top(), *top - TOOLBAR_HEIGHT);
        assert!(
            pane.read_with(cx, |pane, _| pane.action_bounds(0))
                .is_some()
        );
        assert_eq!(all_crumbs(pane, cx), Vec::<String>::new());
    }

    // A pane split while hidden has none.
    cx.simulate_keystrokes("cmd-\\");
    settle(cx);
    let three = panes(&workbench, cx);
    assert_eq!(three.len(), 3);
    let new = active(&workbench, cx);
    let new_top = new.read_with(cx, |pane, _| pane.bounds().top());
    assert_eq!(content(&new, cx).top(), new_top + TAB_HEIGHT);

    // No chains while hidden.
    let editor = editor(&workbench, cx);
    let before = chains(&workbench, cx);
    caret_at(&editor, "let x", 0, cx);
    caret_at(&editor, "fn main", 0, cx);
    assert_eq!(chains(&workbench, cx), before);

    run("view.toggle_breadcrumbs", cx);
    assert!(bounds("pane-toolbar".into(), cx).is_some());
    for pane in &three {
        let top = pane.read_with(cx, |pane, _| pane.bounds().top());
        assert_eq!(content(pane, cx).top(), top + TAB_HEIGHT + TOOLBAR_HEIGHT);
    }
    assert_eq!(crumbs(&new, CrumbKind::Symbol, cx), ["main"]);
}

#[gpui::test]
fn crumbs_per_pane_and_root(cx: &mut TestAppContext) {
    let dir = project();
    let deep = dir.path().join("a/b/c/d/e/f");
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("a_long_file_name.rs"), "fn x() {}\n").unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::create_dir(outside.path().join("elsewhere")).unwrap();
    let outside_file = outside.path().join("elsewhere/notes.txt");
    fs::write(&outside_file, "n\n").unwrap();

    let (workbench, cx) = opened(cx, &dir);
    let left = active(&workbench, cx);
    caret_at(&editor(&workbench, cx), "let x", 0, cx);
    cx.simulate_keystrokes("cmd-\\");
    settle(cx);
    let right = active(&workbench, cx);
    open_file(&workbench, &dir.path().join("README.md"), cx);
    assert_eq!(crumbs(&right, CrumbKind::File, cx), ["README.md"]);
    assert_eq!(crumbs(&right, CrumbKind::Path, cx), Vec::<String>::new());
    assert_eq!(
        all_crumbs(&left, cx),
        ["src", "util", "mod.rs", "outer", "impl S", "b"],
        "the inactive pane keeps its chain"
    );

    // Its symbol crumb lists the siblings once the outline follows it.
    click_crumb(5, "b", cx);
    assert_eq!(active(&workbench, cx), left);
    assert_eq!(picker_rows(&workbench, cx).unwrap(), ["a", "b"]);
    cx.simulate_keystrokes("escape");
    settle(cx);

    // A tab moved to the other pane shows its path there.
    cx.simulate_keystrokes("cmd-k cmd-left");
    settle(cx);
    assert_eq!(active(&workbench, cx), left);
    cx.simulate_keystrokes("ctrl-cmd-right");
    settle(cx);
    assert_eq!(
        panes(&workbench, cx),
        std::slice::from_ref(&right),
        "the emptied pane closed"
    );
    assert_eq!(
        all_crumbs(&right, cx),
        ["src", "util", "mod.rs", "outer", "impl S", "b"]
    );
    let left = right;

    // Outside the root: the parent directory's name, then the file's.
    open_file(&workbench, &outside_file, cx);
    assert_eq!(all_crumbs(&left, cx), ["elsewhere", "notes.txt"]);

    // Re-rooted at the file's own directory: the file name alone.
    open_file(&workbench, &dir.path().join("src/util/zeta.rs"), cx);
    assert_eq!(all_crumbs(&left, cx), ["src", "util", "zeta.rs"]);
    let util = dir.path().join("src/util");
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.set_root(util, window, cx))
    });
    settle(cx);
    assert_eq!(all_crumbs(&left, cx), ["zeta.rs"]);

    // In a narrow pane the file crumb stays wholly inside the toolbar.
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.set_root(dir.path().to_owned(), window, cx)
        })
    });
    cx.simulate_keystrokes("cmd-b");
    settle(cx);
    assert_eq!(panes(&workbench, cx).len(), 1);
    let pane = active(&workbench, cx);
    let width = pane.read_with(cx, |pane, _| pane.bounds().size.width);
    let window = cx.update(|window, _| window.bounds().size);
    cx.simulate_resize(size(window.width - width + px(320.), window.height));
    open_file(&workbench, &deep.join("a_long_file_name.rs"), cx);
    assert_eq!(
        pane.read_with(cx, |pane, _| pane.bounds().size.width),
        px(320.)
    );
    let toolbar = bounds("pane-toolbar".into(), cx).unwrap();
    let file = bounds("crumb:6:a_long_file_name.rs".into(), cx).unwrap();
    assert!(
        file.left() >= toolbar.left() && file.right() <= toolbar.right(),
        "{file:?} in {toolbar:?}"
    );
}

#[gpui::test]
fn the_welcome_buffer_is_untitled(cx: &mut TestAppContext) {
    let launch = Launch {
        root: None,
        file: None,
    };
    let (workbench, cx, _) = open(cx, launch);
    settle(cx);
    let pane = active(&workbench, cx);
    assert_eq!(all_crumbs(&pane, cx), ["untitled"]);
}
