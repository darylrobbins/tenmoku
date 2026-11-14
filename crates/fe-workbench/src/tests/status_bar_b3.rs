//! The status bar in B3 (`docs/design/design-system.md`, Workbench anatomy
//! and Signals): 24 px; on the left the vim mode as a pill in its mode's
//! colour (NORMAL in the accent, INSERT in celadon, VISUAL in jun under
//! Yohen), `recording @a` after it, then the error and warning counts, each
//! with its glyph; on the right the language server, position, indent,
//! encoding, line ending and language, in that order; and no branch.

use std::{fs, path::Path, process::Command};

use fe_editor::diagnostics::DiagnosticCounts;
use fe_gpui::{EditorView, set_vim_enabled};
use fe_ui::{ActiveTheme as _, Theme, ThemeData, height};
use gpui::{
    AppContext as _, Bounds, Entity, Focusable as _, Hsla, Pixels, Rgba, SharedString,
    TestAppContext, VisualTestContext, px,
};
use kiln::core::theme::ThemeFile;
use kiln_shell::{Icon, StatusItem as KilnItem, StatusTone};

use super::open_root;
use crate::{
    StatusItem, StatusSide, Workbench,
    dock::problems_panel::{ITEM as ERRORS, WARNINGS_ITEM as WARNINGS},
    lsp::{self, lsp_status_item},
};

/// Yohen, Tenmoku's dark theme, as the chrome's colours: the editor's
/// default palette is Yohen's already.
fn use_yohen(cx: &mut VisualTestContext) {
    let source = include_str!("../../../fe-config/themes/yohen.toml");
    let (file, _) = ThemeFile::parse(Path::new("yohen.toml"), source, &[]);
    let data = ThemeData::from_file(&file.expect("Yohen parses"));
    cx.update(|_, cx| fe_ui::set_theme(cx, Theme::new(data)));
    draw(cx);
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Runs `git` in `dir`, with the developer's own config shut out.
fn git(dir: &Path, args: &[&str]) {
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
    assert!(output.status.success(), "git {args:?} failed");
}

/// A git repository holding a committed `a.rs`, open in a workbench under
/// Yohen, focused, with vim on.
fn opened(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    Entity<EditorView>,
    &mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.rs");
    fs::write(&path, "fn a() {}\nfn b() {}\n").unwrap();
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["add", "a.rs"]);
    git(dir.path(), &["commit", "-q", "-m", "a"]);
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    use_yohen(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    let git = workbench.read_with(cx, |workbench, _| workbench.git().clone());
    git.update(cx, |git, cx| git.refresh(cx));
    draw(cx);
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a.rs is shown");
    (dir, workbench, editor, cx)
}

fn bounds(selector: String, cx: &mut VisualTestContext) -> Option<Bounds<Pixels>> {
    // `debug_bounds` takes a `&'static str`.
    cx.debug_bounds(selector.leak())
}

/// The bar's items now, sorted, as it draws them.
fn bar_items(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<KilnItem> {
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    cx.read_entity(&bar, |bar, cx| bar.items(cx))
}

/// The ids of the items the last frame drew on `side`, left to right by
/// where they were drawn.
fn drawn_ids(
    workbench: &Entity<Workbench>,
    side: StatusSide,
    cx: &mut VisualTestContext,
) -> Vec<SharedString> {
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    let drawn = bar.read_with(cx, |bar, _| bar.drawn(side));
    let mut placed: Vec<_> = (drawn.into_iter())
        .filter_map(|(id, _)| Some((bounds(format!("status-item:{id}"), cx)?.left(), id)))
        .collect();
    placed.sort_by(|a, b| a.0.partial_cmp(&b.0).expect("bounds are numbers"));
    placed.into_iter().map(|(_, id)| id).collect()
}

fn find<'a>(items: &'a [KilnItem], id: &str) -> &'a KilnItem {
    (items.iter().find(|item| item.id == id)).unwrap_or_else(|| panic!("{id} shown"))
}

/// `color` to 8-bit sRGB, which two paths from one hex both land on.
fn rgb(color: Hsla) -> [u8; 3] {
    let Rgba { r, g, b, .. } = color.to_rgb();
    [r, g, b].map(|c| (c * 255.).round() as u8)
}

fn hex(value: u32) -> [u8; 3] {
    [(value >> 16) as u8, (value >> 8) as u8, value as u8]
}

/// The vim mode item's text and its pill's colour.
fn mode(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (String, [u8; 3]) {
    let items = bar_items(workbench, cx);
    let mode = find(&items, StatusItem::VIM_MODE);
    let fill = mode.fill.expect("the mode is filled");
    (mode.text.to_string(), rgb(fill.background))
}

#[gpui::test]
fn the_bar_is_24_px_and_the_mode_a_pill_inside_it(cx: &mut TestAppContext) {
    let (_dir, _workbench, _editor, cx) = opened(cx);
    let bar = bounds("status-bar".into(), cx).expect("the bar is drawn");
    let window = cx.update(|window, _| window.viewport_size());
    assert_eq!(bar.size.height, height::STATUS_BAR);
    assert_eq!(bar.size.height, px(24.));
    assert_eq!(bar.bottom(), window.height);

    let entry = bounds(format!("status-item:{}", StatusItem::VIM_MODE), cx).expect("the mode");
    let pill = bounds(format!("status-pill:{}", StatusItem::VIM_MODE), cx).expect("its pill");
    // At the bar's left edge, the pill inside the entry's one 8 px padding.
    assert_eq!(entry.left(), bar.left());
    assert_eq!(pill.left() - entry.left(), px(8.));
    assert_eq!(entry.right() - pill.right(), px(8.));
    // Inset from the bar's top and bottom: 16 px, centred below the line.
    assert_eq!(pill.size.height, px(16.));
    assert!(
        pill.top() > bar.top() && pill.bottom() < bar.bottom(),
        "{pill:?} in {bar:?}"
    );
    // An item that isn't filled draws no pill.
    assert_eq!(
        bounds(format!("status-pill:{}", StatusItem::CURSOR), cx),
        None
    );
}

#[gpui::test]
fn each_mode_in_its_colour(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx);
    let accent = cx.update(|_, cx| rgb(cx.theme().colors.accent.base));
    let theme = editor.read_with(cx, |editor, _| editor.theme().clone());

    // NORMAL: Yohen's accent, hare's fur.
    assert_eq!(mode(&workbench, cx), ("NORMAL".into(), accent));
    assert_eq!(accent, hex(0xe0874c));
    assert_eq!(rgb(theme.vim_mode_normal), accent);

    // INSERT: celadon.
    cx.simulate_keystrokes("i");
    draw(cx);
    assert_eq!(mode(&workbench, cx), ("INSERT".into(), hex(0x6fd49a)));
    assert_eq!(rgb(theme.vim_mode_insert), hex(0x6fd49a));
    cx.simulate_keystrokes("escape");
    draw(cx);

    // VISUAL, and its line and block forms: jun.
    for (keys, text) in [
        ("v", "VISUAL"),
        ("shift-v", "VISUAL LINE"),
        ("ctrl-v", "VISUAL BLOCK"),
    ] {
        cx.simulate_keystrokes(keys);
        draw(cx);
        assert_eq!(mode(&workbench, cx), (text.into(), hex(0xc7a5e6)), "{keys}");
        cx.simulate_keystrokes("escape");
        draw(cx);
    }
    assert_eq!(mode(&workbench, cx), ("NORMAL".into(), accent));
}

#[gpui::test]
fn left_the_mode_then_recording_then_the_counts_with_glyphs(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx);
    cx.simulate_keystrokes("q a");
    draw(cx);
    let counts = DiagnosticCounts {
        errors: 2,
        warnings: 1,
        ..Default::default()
    };
    workbench.update(cx, |workbench, cx| {
        workbench.show_problem_counts(counts, cx)
    });
    draw(cx);

    assert_eq!(
        drawn_ids(&workbench, StatusSide::Left, cx),
        [
            StatusItem::VIM_MODE,
            StatusItem::VIM_RECORDING,
            ERRORS,
            WARNINGS
        ]
    );
    let items = bar_items(&workbench, cx);
    assert_eq!(find(&items, StatusItem::VIM_RECORDING).text, "recording @a");

    // Each count with its glyph, so its meaning never rests on hue alone:
    // ✕ for errors, ▲ for warnings, drawn before the number.
    for (id, text, icon, tone) in [
        (ERRORS, "2", Icon::CircleX, StatusTone::Error),
        (WARNINGS, "1", Icon::TriangleAlert, StatusTone::Warning),
    ] {
        let item = find(&items, id);
        assert_eq!(
            (item.text.as_ref(), item.icon, item.tone),
            (text, Some(icon), tone)
        );
        let entry = bounds(format!("status-item:{id}"), cx).expect("the count is drawn");
        let glyph = bounds(format!("status-icon:{id}"), cx).expect("its glyph is drawn");
        assert_eq!(glyph.left() - entry.left(), px(8.), "{id}");
        assert!(glyph.right() < entry.right(), "{id}");
    }

    // No errors: the glyph stays, in the text's colour.
    let counts = DiagnosticCounts {
        warnings: 1,
        ..Default::default()
    };
    workbench.update(cx, |workbench, cx| {
        workbench.show_problem_counts(counts, cx)
    });
    draw(cx);
    let items = bar_items(&workbench, cx);
    let errors = find(&items, ERRORS);
    assert_eq!(
        (errors.text.as_ref(), errors.icon, errors.tone),
        ("0", Some(Icon::CircleX), StatusTone::Default)
    );
    assert!(bounds(format!("status-icon:{ERRORS}"), cx).is_some());
}

#[gpui::test]
fn right_the_server_position_indent_encoding_line_ending_language(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx);
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    let server = lsp_status_item("rust-analyzer: indexing".into(), StatusTone::Default);
    bar.update(cx, |bar, cx| bar.set_item(server, cx));
    draw(cx);
    assert_eq!(
        drawn_ids(&workbench, StatusSide::Right, cx),
        [
            lsp::ITEM,
            StatusItem::CURSOR,
            StatusItem::INDENT,
            StatusItem::ENCODING,
            StatusItem::LINE_ENDING,
            StatusItem::LANGUAGE,
            StatusItem::NOTIFICATIONS,
        ]
    );
    // The bell against the bar's right edge, the language beside it.
    let edge = bounds("status-bar".into(), cx).unwrap().right();
    let bell = bounds(format!("status-item:{}", StatusItem::NOTIFICATIONS), cx).unwrap();
    assert_eq!(bell.right(), edge);
    let language = bounds(format!("status-item:{}", StatusItem::LANGUAGE), cx).unwrap();
    assert_eq!(language.right(), bell.left());
}

#[gpui::test]
fn no_branch_item(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx);
    // In a repository: the title bar has the branch.
    let state = workbench.read_with(cx, |workbench, _| workbench.title_bar_state());
    assert_eq!(state.branch.as_deref(), Some("main"));

    let items = bar_items(&workbench, cx);
    assert!(
        items
            .iter()
            .all(|item| item.id != "git.branch" && item.icon != Some(Icon::GitBranch)),
        "{items:?}"
    );
    assert_eq!(bounds("status-item:git.branch".into(), cx), None);
    let shown: Vec<_> = (items.iter())
        .filter(|item| item.text.contains("main"))
        .collect();
    assert!(shown.is_empty(), "{shown:?}");
}
