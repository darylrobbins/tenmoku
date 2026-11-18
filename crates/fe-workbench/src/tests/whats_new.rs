//! The What's New tab: `app.show_whats_new`, the launch's offer once per
//! upgrade and never on a new machine, and the shipped notes' rules.

use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_command::Invocation;
use fe_config::{
    first_run::{self, Outcome},
    whats_new::{self, MARKER_FILE, parse_version},
};
use fe_gpui::{EditorSettings, EditorTheme, EditorView, commands::RunCommand};
use fe_markdown::{Block, BlockKind, Document, Inline};
use fe_markdown_view::PaintedRuns;
use futures::channel::oneshot;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext, WindowHandle};
use kiln_shell::PaneItem as _;

use crate::{
    Launch, SetupLaunch, Workbench, offer_setup, offer_whats_new,
    setup_wizard::SetupPaths,
    whats_new::{NotesSeam, RELEASE_NOTES, WhatsNew},
};

/// Notes for 0.2.2, a draft for 0.2.4 and 0.2.3's, out of order.
fn fixture_notes() -> Vec<(String, String)> {
    [
        ("0.2.2", "## Fixed\n\n- Two fix.\n"),
        ("0.2.4", "## New\n\n- Four draft.\n"),
        (
            "0.2.3",
            "## New\n\n- Three feature.\n\n## Fixed\n\n- Three fix.\n",
        ),
    ]
    .map(|(version, text)| (version.to_owned(), text.to_owned()))
    .into()
}

/// The fixture notes, with `running` the version running.
fn seam(running: &str, cx: &mut TestAppContext) {
    let seam = NotesSeam {
        notes: fixture_notes(),
        running: running.to_owned(),
    };
    cx.update(|cx| cx.set_global(seam));
}

struct Dirs {
    _dir: tempfile::TempDir,
    config: PathBuf,
    state: PathBuf,
}

impl Dirs {
    /// The What's New marker's text, if there's one.
    fn marker(&self) -> Option<String> {
        fs::read_to_string(self.state.join(MARKER_FILE)).ok()
    }

    /// A machine that has used Tenmoku: its setup marker.
    fn used(&self) {
        first_run::record(&self.state, Outcome::Completed).unwrap();
    }

    fn seen(&self, version: &str) {
        whats_new::record(&self.state, version).unwrap();
    }
}

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
}

/// New temporary config and state directories, for the offer to use; the
/// state directory isn't made.
fn fresh(cx: &mut TestAppContext) -> Dirs {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config");
    let state = dir.path().join("state");
    fs::create_dir(&config).unwrap();
    let paths = SetupPaths {
        settings: Some(config.join("settings.toml")),
        state: Some(state.clone()),
    };
    cx.update(|cx| cx.set_global(paths));
    Dirs {
        _dir: dir,
        config,
        state,
    }
}

fn window(cx: &mut TestAppContext) -> WindowHandle<Workbench> {
    let window = cx.add_window(|window, cx| {
        window.activate_window();
        let launch = Launch {
            root: None,
            file: None,
        };
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.run_until_parked();
    window
}

/// The launch's offers, as `main` makes them for its first window, with
/// nothing left for the launch to open.
fn offer(window: WindowHandle<Workbench>, launch: SetupLaunch, cx: &mut TestAppContext) {
    cx.update(|cx| {
        offer_setup(window, launch, cx);
        offer_whats_new(window, launch, std::future::ready(()), cx);
    });
}

fn run(window: WindowHandle<Workbench>, command: &str, cx: &mut TestAppContext) {
    let invocation = Invocation::of(command).unwrap();
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    let focus = window
        .read_with(&vcx, |workbench, cx| workbench.focus_handle(cx))
        .unwrap();
    vcx.update(|window, cx| {
        window.focus(&focus, cx);
        window.dispatch_action(Box::new(RunCommand(invocation)), cx)
    });
    vcx.run_until_parked();
}

/// Every What's New tab in the window, by pane.
fn tabs(window: WindowHandle<Workbench>, cx: &mut TestAppContext) -> Vec<Entity<WhatsNew>> {
    cx.run_until_parked();
    window
        .read_with(cx, |workbench, cx| {
            (workbench.panes(cx).iter())
                .flat_map(|pane| {
                    (pane.read(cx).tabs().iter())
                        .filter_map(|tab| tab.item().downcast_ref::<WhatsNew>().cloned())
                        .collect::<Vec<_>>()
                })
                .collect()
        })
        .unwrap()
}

/// The one What's New tab, which is the active pane's active tab, and
/// focused.
fn shown(window: WindowHandle<Workbench>, cx: &mut TestAppContext) -> Entity<WhatsNew> {
    let tabs = tabs(window, cx);
    assert_eq!(tabs.len(), 1, "one What's New tab");
    let item = tabs[0].clone();
    window
        .update(cx, |workbench, window, cx| {
            let pane = workbench.pane().read(cx);
            let active = &pane.tabs()[pane.active().unwrap()];
            assert_eq!(active.item().downcast_ref::<WhatsNew>(), Some(&item));
            assert!(item.focus_handle(cx).contains_focused(window, cx));
        })
        .unwrap();
    item
}

/// The document's headings and list items, as `# heading` and `- item`
/// lines, and its paragraphs as they read.
fn outline(document: &Document) -> Vec<String> {
    let mut lines = Vec::new();
    for block in &document.blocks {
        match &block.kind {
            BlockKind::Heading { level, content, .. } => lines.push(format!(
                "{} {}",
                "#".repeat(usize::from(*level)),
                Inline::plain_text(content)
            )),
            BlockKind::Paragraph(content) => lines.push(Inline::plain_text(content)),
            BlockKind::List { items, .. } => {
                for item in items {
                    for block in &item.blocks {
                        if let BlockKind::Paragraph(content) = &block.kind {
                            lines.push(format!("- {}", Inline::plain_text(content)));
                        }
                    }
                }
            }
            other => lines.push(format!("{other:?}")),
        }
    }
    lines
}

fn heading(version: &str) -> String {
    format!("# {} {version}", fe_config::APP_NAME)
}

#[gpui::test]
fn the_command_opens_one_tab_newest_first(cx: &mut TestAppContext) {
    init(cx);
    seam("0.2.3", cx);
    let first = window(cx);
    assert!(tabs(first, cx).is_empty());
    cx.update(PaintedRuns::set);
    run(first, "app.show_whats_new", cx);
    let item = shown(first, cx);
    let document = item.read_with(cx, |item, cx| item.document(cx));
    assert_eq!(
        outline(&document),
        [
            heading("0.2.3"),
            "## New".to_owned(),
            "- Three feature.".to_owned(),
            "## Fixed".to_owned(),
            "- Three fix.".to_owned(),
            heading("0.2.2"),
            "## Fixed".to_owned(),
            "- Two fix.".to_owned(),
        ]
    );
    // As drawn: 0.2.3's heading and notes, then 0.2.2's, and no draft.
    let drawn: Vec<String> = cx
        .update(PaintedRuns::take)
        .into_iter()
        .map(|(text, _)| text)
        .collect();
    let at = |text: &str| {
        (drawn.iter())
            .position(|drawn| drawn.contains(text))
            .unwrap_or_else(|| panic!("{text:?} isn't drawn in {drawn:?}"))
    };
    let order = [
        at(&heading("0.2.3")[2..]),
        at("Three feature."),
        at("Three fix."),
        at(&heading("0.2.2")[2..]),
        at("Two fix."),
    ];
    assert!(order.is_sorted(), "{order:?} in {drawn:?}");
    assert!(
        !drawn
            .iter()
            .any(|text| text.contains("0.2.4") || text.contains("Four")),
        "{drawn:?}"
    );
    let title = item.read_with(cx, |item, cx| {
        kiln_shell::PaneItem::tab_title(item, cx).to_string()
    });
    assert_eq!(title, "What's New");

    // Again, with another tab active: that tab, and no second one.
    run(first, "workbench.new_file", cx);
    let active_is_new = first
        .read_with(cx, |workbench, cx| {
            let pane = workbench.pane().read(cx);
            let active = &pane.tabs()[pane.active().unwrap()];
            active.item().downcast_ref::<WhatsNew>().is_some()
        })
        .unwrap();
    assert!(!active_is_new, "New File's tab is active");
    run(first, "app.show_whats_new", cx);
    assert_eq!(shown(first, cx), item);

    // A running version with no file: its no-notes line, above the earlier
    // versions.
    seam("0.2.5", cx);
    let second = window(cx);
    run(second, "app.show_whats_new", cx);
    let document = shown(second, cx).read_with(cx, |item, cx| item.document(cx));
    let lines = outline(&document);
    assert_eq!(
        lines[..3],
        [
            heading("0.2.5"),
            "There are no release notes for this version.".to_owned(),
            heading("0.2.4"),
        ]
    );
    assert!(lines.contains(&heading("0.2.3")));
    assert!(lines.contains(&heading("0.2.2")));
}

#[gpui::test]
fn an_upgrade_opens_it_once(cx: &mut TestAppContext) {
    init(cx);
    seam("0.2.3", cx);
    let dirs = fresh(cx);
    dirs.used();
    dirs.seen("0.2.2");
    let seen = |version: &str| Some(format!("seen = \"{version}\"\n"));

    // A window that closes before the tab has drawn records nothing.
    let closed = window(cx);
    offer(closed, SetupLaunch::Normal, cx);
    closed
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    assert_eq!(dirs.marker(), seen("0.2.2"));

    let first = window(cx);
    offer(first, SetupLaunch::Normal, cx);
    // Decided off the main thread: nothing shows or is written yet.
    assert_eq!(dirs.marker(), seen("0.2.2"));
    cx.update(PaintedRuns::set);
    let item = shown(first, cx);
    assert!(!first.read_with(cx, |w, _| w.is_setup_showing()).unwrap());
    let drawn = cx.update(PaintedRuns::take);
    assert!(
        (drawn.iter()).any(|(text, _)| text.contains(&heading("0.2.3")[2..])),
        "{drawn:?}"
    );
    assert_eq!(dirs.marker(), seen("0.2.3"));
    drop(item);

    // A second workbench offered from the same state dir opens nothing.
    let second = window(cx);
    offer(second, SetupLaunch::Normal, cx);
    assert!(tabs(second, cx).is_empty());
    assert_eq!(dirs.marker(), seen("0.2.3"));

    // The version is written only once the tab has drawn: not while it
    // opens.
    dirs.seen("0.2.2");
    let third = window(cx);
    let state = dirs.state.clone();
    let marker = dirs.state.join(MARKER_FILE);
    third
        .update(cx, |workbench, window, cx| {
            workbench.show_whats_new_once(state, "0.2.3".to_owned(), window, cx);
            assert_eq!(
                fs::read_to_string(&marker).ok(),
                seen("0.2.2"),
                "written before the tab drew"
            );
        })
        .unwrap();
    shown(third, cx);
    assert_eq!(dirs.marker(), seen("0.2.3"));
}

/// The kind of a restored editor that waits for the test's release.
const HELD_EDITOR: &str = "test.held_editor";

/// Has [`HELD_EDITOR`] items restore as editors once the test sends to the
/// sender each pushes.
fn hold_restores(cx: &mut TestAppContext) -> Rc<RefCell<Vec<oneshot::Sender<()>>>> {
    let holds: Rc<RefCell<Vec<oneshot::Sender<()>>>> = Rc::default();
    let pushed = holds.clone();
    cx.update(|cx| {
        let restore: kiln_shell::layout::Restore = Rc::new(move |state, window, cx| {
            let editor =
                kiln_shell::layout::restorer(EditorView::KIND, cx).expect("editors restore");
            let restoring = editor(state, window, cx);
            let (release, held) = oneshot::channel::<()>();
            pushed.borrow_mut().push(release);
            cx.spawn(async move |_| {
                held.await.ok();
                restoring.await
            })
        });
        kiln_shell::layout::register_kind(HELD_EDITOR, restore, cx);
    });
    holds
}

/// A window on `root`.
fn window_on(root: &Path, cx: &mut TestAppContext) -> WindowHandle<Workbench> {
    let root = root.to_owned();
    let window = cx.add_window(move |window, cx| {
        window.activate_window();
        let launch = Launch {
            root: Some(root),
            file: None,
        };
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.run_until_parked();
    window
}

/// Opens `path` in the window's active pane, as the launch's files open.
fn open_file(window: WindowHandle<Workbench>, path: PathBuf, cx: &mut TestAppContext) {
    window
        .update(cx, |workbench, window, cx| {
            let pane = workbench.pane().clone();
            pane.update(cx, |pane, cx| pane.open(path, window, cx));
        })
        .unwrap();
}

/// Each tab title in the window's active pane.
fn titles(window: WindowHandle<Workbench>, cx: &mut TestAppContext) -> Vec<String> {
    window
        .read_with(cx, |workbench, cx| {
            (workbench.pane().read(cx).tabs().iter())
                .map(|tab| tab.item().tab_title(cx).to_string())
                .collect()
        })
        .unwrap()
}

/// Every item `kind` in a saved layout's `value`, as [`HELD_EDITOR`].
fn hold_kinds(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            if map.get("kind").and_then(|kind| kind.as_str()) == Some(EditorView::KIND) {
                map.insert("kind".into(), HELD_EDITOR.into());
            }
            map.values_mut().for_each(hold_kinds);
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(hold_kinds),
        _ => {}
    }
}

#[gpui::test]
fn the_offer_waits_for_the_launch_to_settle(cx: &mut TestAppContext) {
    init(cx);
    seam("0.2.3", cx);
    let holds = hold_restores(cx);
    // Each order: the restored layout's files in first, or the launch's.
    for restored_first in [false, true] {
        let dirs = fresh(cx);
        dirs.used();
        dirs.seen("0.2.2");
        let root = dirs.config.parent().unwrap().join("project");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("a.txt"), "alpha\n").unwrap();
        fs::write(root.join("b.txt"), "beta\n").unwrap();

        // A saved layout with a.txt, whose read the test holds.
        let saved = window_on(&root, cx);
        open_file(saved, root.join("a.txt"), cx);
        cx.run_until_parked();
        let mut entry = saved
            .read_with(cx, |workbench, cx| workbench.layout_entry(cx))
            .unwrap();
        hold_kinds(&mut entry);
        saved
            .update(cx, |_, window, _| window.remove_window())
            .unwrap();
        cx.run_until_parked();

        // The launch: its window restores the layout and opens b.txt, and
        // the decision lands first.
        let first = window_on(&root, cx);
        first
            .update(cx, |workbench, window, cx| {
                workbench.restore_layout(&entry, window, cx)
            })
            .unwrap();
        let restoring = |cx: &mut TestAppContext| {
            first
                .read_with(cx, |workbench, _| workbench.restoring())
                .unwrap()
        };
        assert!(restoring(cx));
        let (settle, settled) = oneshot::channel::<()>();
        cx.update(|cx| {
            offer_setup(first, SetupLaunch::Normal, cx);
            let settled = async {
                settled.await.ok();
            };
            offer_whats_new(first, SetupLaunch::Normal, settled, cx);
        });
        open_file(first, root.join("b.txt"), cx);
        cx.run_until_parked();
        assert!(
            tabs(first, cx).is_empty(),
            "decided, but the launch isn't in"
        );
        assert_eq!(dirs.marker().as_deref(), Some("seen = \"0.2.2\"\n"));

        let release = |cx: &mut TestAppContext| {
            for release in holds.borrow_mut().drain(..) {
                release.send(()).ok();
            }
            cx.run_until_parked();
        };
        if restored_first {
            release(cx);
            assert!(!restoring(cx));
            assert!(tabs(first, cx).is_empty(), "the launch's file isn't in");
            settle.send(()).unwrap();
        } else {
            settle.send(()).unwrap();
            cx.run_until_parked();
            assert!(tabs(first, cx).is_empty(), "the layout's file isn't in");
            assert!(restoring(cx));
            release(cx);
        }

        // Over both files, active and focused, and only then recorded.
        shown(first, cx);
        assert!(!restoring(cx));
        let titles = titles(first, cx);
        for name in ["a.txt", "b.txt", "What's New"] {
            assert!(
                titles.iter().any(|title| title == name),
                "{name} in {titles:?}"
            );
        }
        assert_eq!(dirs.marker().as_deref(), Some("seen = \"0.2.3\"\n"));
    }
}

#[gpui::test]
fn no_tab_on_a_new_machine_a_quiet_launch_or_a_downgrade(cx: &mut TestAppContext) {
    init(cx);
    seam("0.2.3", cx);

    // A new machine: the wizard, no tab, and the running version recorded.
    let dirs = fresh(cx);
    let first = window(cx);
    offer(first, SetupLaunch::Normal, cx);
    assert!(tabs(first, cx).is_empty());
    assert!(first.read_with(cx, |w, _| w.is_setup_showing()).unwrap());
    assert_eq!(dirs.marker().as_deref(), Some("seen = \"0.2.3\"\n"));

    // A launch that mustn't be interrupted: nothing, and nothing written,
    // on a new machine or after an upgrade.
    let dirs = fresh(cx);
    let quiet = window(cx);
    offer(quiet, SetupLaunch::Never, cx);
    assert!(tabs(quiet, cx).is_empty());
    assert_eq!(dirs.marker(), None);
    let dirs = fresh(cx);
    dirs.used();
    dirs.seen("0.2.2");
    let quiet = window(cx);
    offer(quiet, SetupLaunch::Never, cx);
    assert!(tabs(quiet, cx).is_empty());
    assert_eq!(dirs.marker().as_deref(), Some("seen = \"0.2.2\"\n"));

    // No state directory: it could never be recorded.
    let dirs = fresh(cx);
    fs::write(
        dirs.config.join("settings.toml"),
        "[editor]\nfont_size = 15\n",
    )
    .unwrap();
    cx.update(|cx| {
        cx.set_global(SetupPaths {
            settings: Some(dirs.config.join("settings.toml")),
            state: None,
        })
    });
    let stateless = window(cx);
    offer(stateless, SetupLaunch::Normal, cx);
    assert!(tabs(stateless, cx).is_empty());
    assert!(!dirs.state.exists());

    // A downgrade: nothing, the marker as it was.
    let dirs = fresh(cx);
    dirs.used();
    dirs.seen("0.3.0");
    let older = window(cx);
    offer(older, SetupLaunch::Normal, cx);
    assert!(tabs(older, cx).is_empty());
    assert_eq!(dirs.marker().as_deref(), Some("seen = \"0.3.0\"\n"));

    // An upgrade from a build before What's New: a setup marker and none of
    // its own.
    let dirs = fresh(cx);
    dirs.used();
    let upgraded = window(cx);
    offer(upgraded, SetupLaunch::Normal, cx);
    shown(upgraded, cx);
    assert_eq!(dirs.marker().as_deref(), Some("seen = \"0.2.3\"\n"));

    // Or from one before the wizard: no marker at all, but files in use.
    let dirs = fresh(cx);
    fs::write(
        dirs.config.join("settings.toml"),
        "[editor]\nfont_size = 15\n",
    )
    .unwrap();
    let older_still = window(cx);
    offer(older_still, SetupLaunch::Normal, cx);
    shown(older_still, cx);
    assert!(
        !older_still
            .read_with(cx, |w, _| w.is_setup_showing())
            .unwrap()
    );
    assert_eq!(dirs.marker().as_deref(), Some("seen = \"0.2.3\"\n"));
}

/// The sections a release's notes may have, in their order.
const SECTIONS: [&str; 3] = ["New", "Improved", "Fixed"];

/// Why `inlines` break the notes' rules: an image or raw HTML.
fn inline_faults(inlines: &[Inline], faults: &mut Vec<String>) {
    for inline in inlines {
        match inline {
            Inline::Image { dest, .. } => faults.push(format!("an image, {dest}")),
            Inline::Html(html) => faults.push(format!("raw HTML, {html}")),
            Inline::Emphasis(content)
            | Inline::Strong(content)
            | Inline::Strikethrough(content)
            | Inline::Link { content, .. } => inline_faults(content, faults),
            Inline::Text(_) | Inline::Code(_) | Inline::SoftBreak | Inline::HardBreak => {}
        }
    }
}

/// Why a list item's `blocks` break the rules.
fn item_faults(blocks: &[Block], faults: &mut Vec<String>) {
    for block in blocks {
        match &block.kind {
            BlockKind::Paragraph(content) => inline_faults(content, faults),
            BlockKind::List {
                start: None, items, ..
            } => {
                for item in items {
                    item_faults(&item.blocks, faults);
                }
            }
            other => faults.push(format!("in a bullet, {other:?}")),
        }
    }
}

/// Why `text` isn't a release's notes: see `whats_new.rs`'s rules.
fn faults(text: &str) -> Vec<String> {
    let mut faults = Vec::new();
    if text.contains("]]>") {
        faults.push("]]>, which ends the appcast's CDATA".to_owned());
    }
    // fe-markdown draws some tags as formatting, so they're looked for in
    // the source: a `<` that starts a tag, a comment or a declaration,
    // outside code spans. An autolink is a URL in brackets.
    for line in text.lines() {
        let prose = line.split('`').step_by(2).collect::<Vec<_>>().join(" ");
        for (at, _) in prose.match_indices('<') {
            let rest = &prose[at + 1..];
            let tag = rest.starts_with(|c: char| c.is_ascii_alphabetic() || "/!?".contains(c));
            let autolink = ["http://", "https://", "mailto:"]
                .iter()
                .any(|scheme| rest.starts_with(scheme));
            if tag && !autolink {
                faults.push(format!("raw HTML: {line}"));
            }
        }
    }
    let document = fe_markdown::parse(text);
    let mut blocks = document.blocks.iter();
    let mut last = None;
    let mut sections = 0;
    while let Some(block) = blocks.next() {
        let BlockKind::Heading {
            level: 2, content, ..
        } = &block.kind
        else {
            faults.push(format!("not a ## section heading: {:?}", block.kind));
            continue;
        };
        let name = Inline::plain_text(content);
        match SECTIONS.iter().position(|section| *section == name) {
            Some(at) if last.is_none_or(|last| at > last) => last = Some(at),
            Some(_) => faults.push(format!("## {name} out of order or twice")),
            None => faults.push(format!("## {name} isn't New, Improved or Fixed")),
        }
        sections += 1;
        match blocks.next().map(|block| &block.kind) {
            Some(BlockKind::List {
                start: None, items, ..
            }) if !items.is_empty() => {
                for item in items {
                    if item.task.is_some() {
                        faults.push("a task item".to_owned());
                    }
                    item_faults(&item.blocks, &mut faults);
                }
            }
            other => faults.push(format!("## {name} isn't a bullet list: {other:?}")),
        }
    }
    if sections == 0 {
        faults.push("no section".to_owned());
    }
    faults
}

#[test]
fn the_shipped_notes_are_well_formed() {
    // Every file in release-notes/, embedded, newest first.
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../release-notes");
    let mut files: Vec<String> = (fs::read_dir(&dir).unwrap())
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| !name.starts_with('.'))
        .collect();
    files.sort();
    let mut embedded: Vec<String> = (RELEASE_NOTES.iter())
        .map(|(version, _)| format!("{version}.md"))
        .collect();
    embedded.sort();
    assert_eq!(embedded, files);
    let versions: Vec<_> = (RELEASE_NOTES.iter())
        .map(|(version, _)| parse_version(version).unwrap_or_else(|| panic!("{version}")))
        .collect();
    assert!(versions.is_sorted_by(|a, b| a > b), "{versions:?}");
    for (version, text) in RELEASE_NOTES {
        let (major, minor, patch) = parse_version(version).unwrap();
        assert_eq!(*version, format!("{major}.{minor}.{patch}"));
        assert_eq!(
            *text,
            fs::read_to_string(dir.join(format!("{version}.md"))).unwrap()
        );
        assert_eq!(
            faults(text),
            Vec::<String>::new(),
            "release-notes/{version}.md"
        );
    }
    // Curated from v0.2.2..v0.2.3.
    let notes = (RELEASE_NOTES.iter())
        .find(|(version, _)| *version == "0.2.3")
        .map(|(_, text)| *text)
        .expect("release-notes/0.2.3.md");
    assert!(notes.contains("Check for Updates"), "{notes}");
    assert!(notes.contains("Prisma"), "{notes}");
}

/// What [`faults`] rejects.
#[test]
fn malformed_notes_are_caught() {
    let bad = [
        "# Tenmoku 0.2.3\n\n## New\n\n- A.\n",
        "## Fixed\n\n- A.\n\n## New\n\n- B.\n",
        "## New\n\n- A.\n\n## New\n\n- B.\n",
        "## Changed\n\n- A.\n",
        "## New\n\nA paragraph.\n",
        "## New\n\n1. Numbered.\n",
        "## New\n\n- ![shot](shot.png)\n",
        "## New\n\n- A <b>bold</b> one.\n",
        "## New\n\n- A <!-- comment -->.\n",
        "<div>\n\n## New\n\n- A.\n\n</div>\n",
        "## New\n\n- Ends ]]> the CDATA.\n",
        "## New\n\n- [ ] A task.\n",
        "",
    ];
    for text in bad {
        assert_ne!(faults(text), Vec::<String>::new(), "{text:?}");
    }
    assert_eq!(
        faults(
            "## New\n\n- A [link](https://example.com), <https://example.com> and `Vec<T>`.\n\n\
             ## Improved\n\n- B, 1 < 2.\n\n## Fixed\n\n- C.\n"
        ),
        Vec::<String>::new()
    );
}
