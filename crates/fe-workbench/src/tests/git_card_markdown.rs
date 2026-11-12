//! The commit card draws its message body as Markdown, as the LSP hover
//! does (T362): blocks, highlighted code and links, while the summary and
//! trailers stay as they were.

use std::{fs, path::Path, process::Command, time::Duration};

use fe_gpui::{EditorSettings, EditorTheme};
use fe_markdown_view::PaintedRuns;
use fe_syntax::Highlight;
use fe_ui::ROW_HEIGHT;
use gpui::{
    Entity, Focusable as _, Modifiers, Pixels, TestAppContext, VisualTestContext, point, px,
};

use crate::{Launch, Workbench, workbench::git_commands::git_log::GitLogPanel};

/// A repository at a temporary root whose only commit has `message`.
fn repo(message: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .current_dir(root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("run git");
        assert!(output.status.success(), "{args:?}");
    };
    git(&["init", "-q"]);
    fs::write(root.join("a.rs"), "x\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", message]);
    dir
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// The Git Log of `root` and its card for the only commit, drawn.
fn card(
    cx: &mut TestAppContext,
    root: &Path,
) -> (Entity<Workbench>, Entity<GitLogPanel>, VisualTestContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
        PaintedRuns::set(cx);
    });
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let window = cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let workbench = window.root(cx).unwrap();
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    draw(&mut vcx);
    vcx.update(|window, cx| {
        use fe_command::{ContextFrame, ContextStack, Invocation};
        let context = ContextStack::new().with(ContextFrame::new("workbench"));
        let run =
            fe_gpui::commands::RunCommand(Invocation::of("git.log").unwrap().context(context));
        window.dispatch_action(Box::new(run), cx);
    });
    draw(&mut vcx);
    let panel = workbench.read_with(&vcx, |workbench, cx| {
        workbench
            .git_log_panel(cx)
            .expect("the dock has its Git Log")
    });
    let list = panel.read_with(&vcx, |panel, _| panel.list_bounds());
    let row = point(list.left() + px(40.), list.top() + ROW_HEIGHT * 0.5);
    vcx.simulate_mouse_move(row, None, Modifiers::none());
    vcx.executor().advance_clock(Duration::from_millis(500));
    draw(&mut vcx);
    draw(&mut vcx);
    let built = panel.read_with(&vcx, |panel, _| panel.card_probe().borrow().built);
    assert_eq!(built, 1, "the card was built");
    (workbench, panel, vcx)
}

fn bounds(cx: &mut VisualTestContext, block: usize) -> Option<gpui::Bounds<Pixels>> {
    cx.debug_bounds(Box::leak(format!("md-block-{block}").into_boxed_str()))
}

/// Every text the Markdown renderer drew, with the colour of each run.
fn painted(cx: &mut VisualTestContext) -> Vec<(String, Vec<gpui::TextRun>)> {
    cx.update(|_, cx| PaintedRuns::take(cx))
}

#[gpui::test]
fn a_list_and_inline_code_in_the_body_are_drawn_as_blocks(cx: &mut TestAppContext) {
    let dir = repo("subject\n\nintro\n\n- one\n- two `code`\n");
    let (_workbench, _panel, mut cx) = card(cx, dir.path());
    let cx = &mut cx;
    let paragraph = bounds(cx, 0).expect("the paragraph");
    let list = bounds(cx, 1).expect("the list");
    assert!(list.top() >= paragraph.bottom(), "{paragraph:?} {list:?}");
    assert!(bounds(cx, 2).is_none(), "the list is one block");
    let texts = painted(cx);
    let code = texts.iter().find(|(text, _)| text.contains("two code"));
    let (_, runs) = code.expect("the item's text, without its backticks");
    assert!(runs.len() >= 2, "the code span has a run of its own");
}

#[gpui::test]
fn a_fenced_block_is_highlighted_in_the_editor_theme(cx: &mut TestAppContext) {
    let dir = repo("subject\n\n```rust\nfn f()\n```\n");
    let (workbench, _panel, mut cx) = card(cx, dir.path());
    let cx = &mut cx;
    let theme = workbench.read_with(cx, |w, cx| w.pane().read(cx).theme().clone());
    let keyword = Highlight::from_capture("keyword").expect("a highlight");
    let keyword = theme.syntax_color(keyword).expect("a keyword colour");
    assert_ne!(keyword, theme.foreground);
    let texts = painted(cx);
    let (_, runs) = (texts.iter())
        .find(|(text, _)| text.contains("fn f()"))
        .expect("the code is drawn");
    assert_eq!(
        runs.first().map(|run| (run.len, run.color)),
        Some((2, keyword)),
        "{runs:?}"
    );
}

#[gpui::test]
fn a_link_opens_through_the_preview_tabs_link_path(cx: &mut TestAppContext) {
    let dir = repo("subject\n\nSee [docs](https://example.com).\n");
    let (_workbench, _panel, mut cx) = card(cx, dir.path());
    let cx = &mut cx;
    let paragraph = bounds(cx, 0).expect("the paragraph");
    let y = paragraph.origin.y + px(8.);
    let mut x = paragraph.origin.x;
    while cx.opened_url().is_none() && x < paragraph.right() {
        cx.simulate_click(point(x, y), Modifiers::none());
        x += px(4.);
    }
    assert_eq!(cx.opened_url().as_deref(), Some("https://example.com"));
}

#[gpui::test]
fn a_plain_message_looks_as_before(cx: &mut TestAppContext) {
    let dir = repo("subject\n\nline one\nline two\n\npara\n");
    let (_workbench, panel, mut cx) = card(cx, dir.path());
    let cx = &mut cx;
    let probe = panel.read_with(cx, |panel, _| panel.card_probe());
    assert_eq!(
        probe.borrow().message,
        "subject\nline one\nline two\n\npara"
    );
    assert!(bounds(cx, 0).is_some() && bounds(cx, 1).is_some());
    assert!(bounds(cx, 2).is_none());
    // The lone newline is a break, not a space.
    let texts = painted(cx);
    assert!(
        texts.iter().any(|(text, _)| text == "line one\nline two"),
        "{:?}",
        texts.iter().map(|(text, _)| text).collect::<Vec<_>>()
    );
    assert!(texts.iter().any(|(text, _)| text == "para"));
}

#[gpui::test]
fn the_summary_stays_bold_plain_text(cx: &mut TestAppContext) {
    let dir = repo("**sub** `x`\n\nbody\n");
    let (_workbench, panel, mut cx) = card(cx, dir.path());
    let cx = &mut cx;
    let probe = panel.read_with(cx, |panel, _| panel.card_probe());
    assert_eq!(probe.borrow().message, "**sub** `x`\nbody");
    let sections = probe.borrow().scrolled.clone();
    let summary = sections.iter().find(|(name, _)| *name == "summary");
    let summary = summary.expect("the summary section").1;
    let body = bounds(cx, 0).expect("the body's paragraph");
    assert!(body.top() >= summary.bottom());
    // Only the body went through the Markdown renderer.
    let texts = painted(cx);
    assert!(
        texts.iter().all(|(text, _)| !text.contains("sub")),
        "{texts:?}"
    );
}

#[gpui::test]
fn a_very_long_body_still_scrolls_with_the_footer_kept(cx: &mut TestAppContext) {
    let body: Vec<String> = (1..=200).map(|n| format!("- item {n}")).collect();
    let dir = repo(&format!("long\n\n{}\n", body.join("\n")));
    let (_workbench, panel, mut cx) = card(cx, dir.path());
    let cx = &mut cx;
    let probe = panel.read_with(cx, |panel, _| panel.card_probe());
    let window = cx.update(|window, _| window.viewport_size());
    let parts = probe.borrow().drawn.clone();
    let names: Vec<_> = parts.iter().map(|(name, _)| *name).collect();
    assert_eq!(names, ["header", "scrolled", "totals", "footer"]);
    for (name, part) in &parts {
        assert!(
            part.top() >= px(0.) && part.bottom() <= window.height,
            "{name}: {part:?} in {window:?}"
        );
    }
    let scroll = probe.borrow().scroll.clone().unwrap();
    assert!(scroll.max_offset().y > px(1000.));
}

#[gpui::test]
fn a_trailer_line_is_not_drawn_twice(cx: &mut TestAppContext) {
    let dir = repo("subject\n\nbody text\n\nCo-authored-by: Ann Lee <ann@x>\n");
    let (_workbench, panel, mut cx) = card(cx, dir.path());
    let cx = &mut cx;
    let probe = panel.read_with(cx, |panel, _| panel.card_probe());
    assert_eq!(probe.borrow().trailers.len(), 1);
    let texts = painted(cx);
    assert!(texts.iter().any(|(text, _)| text == "body text"));
    assert!(
        texts
            .iter()
            .all(|(text, _)| !text.contains("Co-authored-by")),
        "{texts:?}"
    );
}
