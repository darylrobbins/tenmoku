// SPDX-License-Identifier: Apache-2.0

//! Keyboard reach for the panels (A11Y-16b, the accessibility design's
//! section 7), driven by keys:
//!
//! - the Changes panel: `git_panel.stage_row`, `git_panel.unstage_row` and
//!   `git_panel.discard_row` on the selected row, with vim off;
//! - the search panel: `search.dismiss_file` and `search.replace_in_file`
//!   on the selected row's file;
//! - AI Changes: `ai_changes.accept_file` and `ai_changes.reject_file` on
//!   the selected file, `ai_changes.apply_preview` and
//!   `ai_changes.cancel_preview` on a refactor's preview;
//! - a commit's change to all its files: F7 and Shift-F7, collapse and
//!   expand all, `commit_diff.toggle_section` and
//!   `commit_diff.expand_hidden`;
//! - the Graph: Up, Down and Enter in a row's menu, `git_log.focus_filter`
//!   with vim off, Escape in the filter clearing it and then giving the
//!   keys back to the list, and `git_log.open_commit_link`;
//! - the Keyboard Shortcuts editor: Up, Down and Enter in a line's menu,
//!   and `shortcuts.copy_command_id`, `shortcuts.reset_binding` and
//!   `shortcuts.show_same_keybindings` on the selected line; its inputs'
//!   placeholders follow a change of language (T560 F1);
//! - the Settings editor: Up and Down between a list's items,
//!   `settings.add_list_item`, `settings.remove_list_item`,
//!   `settings.reset_to_default` and `settings.switch_scope`.
//!
//! Commands with no key are run from the command palette, by keys.

use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
    sync::{Mutex, MutexGuard},
    time::{Duration, Instant},
};

use fe_buffer::{Origin, ProposedChange};
use fe_command::l10n::catalogue::{
    __private::inventory, LanguageIdentifier, Registered, ResourceFile, Resources,
};
use fe_editor::Changeset;
use fe_gpui::{EditorSettings, EditorTheme, EditorView};
use fe_lsp::{FileEdits, PositionEncoding};
use fe_workbench::{ConfigDirOverride, Launch, Workbench};
use gpui::{
    Bounds, ClipboardItem, Entity, Focusable as _, Modifiers, MouseButton, Pixels, TestAppContext,
    VisualTestContext, px,
};
use kiln_shell::{Picker, command_palette::CommandPalette};
use lsp_types::{Position, Range, TextEdit};

static SERIAL: Mutex<()> = Mutex::new(());

/// Runs the rest of a test with no other of this binary running, in
/// en-US: with nothing naming a config directory, the workbench neither
/// reads the user's settings nor watches them.
fn serially() -> MutexGuard<'static, ()> {
    let guard = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for var in [
        "TENMOKU_CONFIG_DIR",
        "FASTEDIT_CONFIG_DIR",
        "XDG_CONFIG_HOME",
        "HOME",
        "APPDATA",
        "USERPROFILE",
    ] {
        // SAFETY: every test takes the lock before it starts gpui, so no
        // other thread is running to read the environment.
        unsafe { std::env::remove_var(var) };
    }
    fe_command::l10n::catalogue::set_locales(&[]);
    guard
}

/// Runs `git` in `dir`, with the developer's own config shut out.
fn git(dir: &Path, args: &[&str]) -> String {
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
    String::from_utf8(output.stdout).unwrap()
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    cx.run_until_parked();
}

/// Presses `keys`, a key at a time.
fn press(keys: &str, cx: &mut VisualTestContext) {
    for key in keys.split_whitespace() {
        cx.simulate_keystrokes(key);
        draw(cx);
    }
}

/// Runs the command titled `title` from the command palette, by keys:
/// typed, then Down to it among those it matches, and Enter.
fn palette(title: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("cmd-shift-p");
    draw(cx);
    cx.simulate_input(title);
    draw(cx);
    let titles: Vec<String> = cx.update(|window, cx| {
        let shell = (window.root::<Workbench>().flatten())
            .expect("the window's root is a workbench")
            .read(cx)
            .shell()
            .clone();
        let layer = shell.read(cx).modals().read(cx);
        let picker: Entity<Picker<CommandPalette>> =
            (layer.modal_view().expect("the palette shows").clone())
                .downcast()
                .expect("it's the command palette");
        (picker.read(cx).delegate.matches().iter())
            .map(|entry| entry.title.to_string())
            .collect()
    });
    let at = (titles.iter().position(|listed| listed == title))
        .unwrap_or_else(|| panic!("the palette lists {title}: {titles:?}"));
    for _ in 0..at {
        cx.simulate_keystrokes("down");
    }
    cx.simulate_keystrokes("enter");
    draw(cx);
}

/// Where the window draws what `selector` names, if it does.
fn bounds(selector: &str, cx: &mut VisualTestContext) -> Option<Bounds<Pixels>> {
    cx.debug_bounds(Box::leak(selector.to_owned().into_boxed_str()))
}

/// Draws until `done` holds or a few seconds pass, for what's read on
/// threads of its own (the Graph's commits, the search's matches):
/// whether it held.
fn until(cx: &mut VisualTestContext, done: impl Fn(&mut VisualTestContext) -> bool) -> bool {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        draw(cx);
        if done(cx) {
            return true;
        }
        if Instant::now() > until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// How many of `prefix0`, `prefix1`, ... the window draws.
fn count(prefix: &str, cx: &mut VisualTestContext) -> usize {
    (0..200)
        .filter(|ix| bounds(&format!("{prefix}{ix}"), cx).is_some())
        .count()
}

fn right_click(selector: &str, cx: &mut VisualTestContext) {
    let at = bounds(selector, cx)
        .unwrap_or_else(|| panic!("{selector} is drawn"))
        .center();
    cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
    draw(cx);
}

fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
    cx.read_from_clipboard().and_then(|item| item.text())
}

fn set_clipboard(text: &str, cx: &mut VisualTestContext) {
    cx.write_to_clipboard(ClipboardItem::new_string(text.to_owned()));
}

/// A workbench on `root`, focused, vim off unless `vim`.
fn workbench<'a>(
    root: &Path,
    vim: bool,
    cx: &'a mut TestAppContext,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    // The project's watcher thread wakes the scheduler.
    cx.executor().allow_parking();
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
        fe_gpui::set_vim_enabled(cx, vim);
    });
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    // Focus listeners hear only from an active window.
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    draw(cx);
    (workbench, cx)
}

/// Opens `path` in the active pane: its editor.
fn open(
    workbench: &Entity<Workbench>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    let path = path.to_owned();
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.open_document(path, window, cx)
        });
    });
    draw(cx);
    workbench.read_with(cx, |workbench, cx| {
        workbench.active_editor(cx).expect("an editor").clone()
    })
}

/// The text of the open editor of `path`, else of the file on disk.
fn text_of(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) -> String {
    let open = workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        (pane.tabs().iter())
            .filter_map(|tab| tab.item().downcast::<EditorView>())
            .find(|editor| editor.read(cx).model().path() == Some(path))
            .map(|editor| editor.read(cx).model().text().to_string())
    });
    open.unwrap_or_else(|| fs::read_to_string(path).unwrap())
}

mod git_panel {
    use super::*;

    /// A repository with `a.rs` staged and `b.rs` and `c.rs` changed: the
    /// rows a.rs under Staged Changes, then b.rs and c.rs under Changes.
    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q"]);
        for file in ["a.rs", "b.rs", "c.rs"] {
            fs::write(root.join(file), "head\n").unwrap();
        }
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "init"]);
        fs::write(root.join("a.rs"), "index\n").unwrap();
        git(root, &["add", "a.rs"]);
        fs::write(root.join("b.rs"), "working\n").unwrap();
        fs::write(root.join("c.rs"), "working\n").unwrap();
        dir
    }

    fn staged(root: &Path) -> Vec<String> {
        let names = git(root, &["diff", "--cached", "--name-only"]);
        names.lines().map(str::to_owned).collect()
    }

    /// With vim off, the selected row's Stage, Unstage and Discard are
    /// palette commands, as fugitive's `s`, `u` and `X` are with vim on.
    #[gpui::test]
    fn the_selected_row_stages_unstages_and_discards(cx: &mut TestAppContext) {
        let _serial = serially();
        let dir = fixture();
        let root = &dunce::canonicalize(dir.path()).unwrap();
        let (workbench, cx) = workbench(root, false, cx);
        press("ctrl-shift-g", cx);
        let panel = workbench.read_with(cx, |workbench, cx| {
            workbench.dock().read(cx).changes().clone()
        });
        cx.update(|window, cx| window.focus(&panel.focus_handle(cx), cx));
        draw(cx);
        assert_eq!(staged(root), ["a.rs"]);

        // b.rs, the second row: staged.
        press("down down", cx);
        palette("Git: Stage Selected Row", cx);
        assert_eq!(staged(root), ["a.rs", "b.rs"]);

        // a.rs, the first: unstaged.
        press("up up up", cx);
        palette("Git: Unstage Selected Row", cx);
        assert_eq!(staged(root), ["b.rs"]);

        // c.rs, the last of b.rs, a.rs and c.rs: discarded, once asked.
        press("down down down", cx);
        palette("Git: Discard Selected Row", cx);
        assert!(cx.has_pending_prompt(), "discarding asks first");
        cx.simulate_prompt_answer("Discard File");
        draw(cx);
        assert_eq!(fs::read_to_string(root.join("c.rs")).unwrap(), "head\n");
        assert_eq!(fs::read_to_string(root.join("a.rs")).unwrap(), "index\n");
    }
}

mod search_panel {
    use super::*;

    /// The selected file's Replace and × buttons are palette commands: the
    /// first file replaced in, the next dismissed, the one in its place
    /// replaced in.
    #[gpui::test]
    fn the_selected_file_is_dismissed_and_replaced_in(cx: &mut TestAppContext) {
        let _serial = serially();
        let dir = tempfile::tempdir().unwrap();
        let root = &dunce::canonicalize(dir.path()).unwrap();
        let files = ["a.txt", "b.txt", "c.txt"].map(|name| root.join(name));
        for file in &files {
            fs::write(file, "foo\n").unwrap();
        }
        let (_workbench, cx) = workbench(root, false, cx);

        // The query, run; the replacement.
        palette("Find in Files", cx);
        cx.simulate_input("foo");
        press("enter", cx);
        palette("Replace in Files…", cx);
        cx.simulate_input("bar");
        // A row per file, each with its line under it.
        let found = until(cx, |cx| count("search-dismiss:", cx) == 3);
        assert!(found, "{} files found", count("search-dismiss:", cx));
        assert_eq!(count("search-replace-file:", cx), 3);

        let replaced = |cx: &mut VisualTestContext| {
            draw(cx);
            (files.iter())
                .filter(|file| fs::read_to_string(file).unwrap() == "bar\n")
                .count()
        };

        // From the results, the first file's line selected: replaced in,
        // and found no more.
        palette("Focus Search Results", cx);
        palette("Replace in Selected File", cx);
        assert!(until(cx, |cx| replaced(cx) == 1));
        assert!(until(cx, |cx| count("search-dismiss:", cx) == 2));

        // The next dismissed: Replace in Selected File acts on the file in
        // its place, and the dismissed one is left as it was.
        palette("Focus Search Results", cx);
        palette("Dismiss Selected File", cx);
        assert_eq!(count("search-dismiss:", cx), 1, "a file fewer");
        palette("Replace in Selected File", cx);
        assert!(until(cx, |cx| replaced(cx) == 2));
        let left: Vec<_> = (files.iter())
            .filter(|file| fs::read_to_string(file).unwrap() == "foo\n")
            .collect();
        assert_eq!(left.len(), 1, "the dismissed file isn't replaced in");
    }

    /// Dismissing a file from one of its matches, not its last file,
    /// selects the file after it, however many matches it had: in the
    /// tree, whose files are in name order, a.txt's three, then b.txt's
    /// and c.txt's one each (T584 review F3).
    #[gpui::test]
    fn dismissing_from_a_match_selects_the_next_file(cx: &mut TestAppContext) {
        let _serial = serially();
        let dir = tempfile::tempdir().unwrap();
        let root = &dunce::canonicalize(dir.path()).unwrap();
        let [a, b, c] = ["a.txt", "b.txt", "c.txt"].map(|name| root.join(name));
        fs::write(&a, "foo\nfoo\nfoo\n").unwrap();
        fs::write(&b, "foo\n").unwrap();
        fs::write(&c, "foo\n").unwrap();
        let (_workbench, cx) = workbench(root, false, cx);
        palette("Find in Files", cx);
        cx.simulate_input("foo");
        press("enter", cx);
        palette("Replace in Files…", cx);
        cx.simulate_input("bar");
        assert!(until(cx, |cx| count("search-dismiss:", cx) == 3));
        palette("View as Tree", cx);

        // a.txt's first match, then its last.
        palette("Focus Search Results", cx);
        press("down down", cx);
        palette("Dismiss Selected File", cx);
        assert_eq!(count("search-dismiss:", cx), 2);
        palette("Replace in Selected File", cx);
        let read = |path: &Path| fs::read_to_string(path).unwrap();
        assert!(until(cx, |_| read(&b) == "bar\n"), "b.txt is replaced in");
        assert_eq!(read(&c), "foo\n");
        assert_eq!(read(&a), "foo\nfoo\nfoo\n");
    }

    /// Dismissing a folder's last file takes the folder too: the folder
    /// after it is selected, not the one after that. In the tree, `a/`,
    /// `b/` and `c/`, collapsed but for `a/` (T584 review F5).
    #[gpui::test]
    fn dismissing_a_folders_last_file_selects_the_next_folder(cx: &mut TestAppContext) {
        let _serial = serially();
        let dir = tempfile::tempdir().unwrap();
        let root = &dunce::canonicalize(dir.path()).unwrap();
        let files = ["a", "b", "c"].map(|name| {
            fs::create_dir(root.join(name)).unwrap();
            let file = root.join(name).join(format!("{name}.txt"));
            fs::write(&file, "foo\n").unwrap();
            file
        });
        let (_workbench, cx) = workbench(root, false, cx);
        palette("Find in Files", cx);
        cx.simulate_input("foo");
        press("enter", cx);
        palette("Replace in Files…", cx);
        cx.simulate_input("bar");
        assert!(until(cx, |cx| count("search-dismiss:", cx) == 3));
        palette("View as Tree", cx);
        palette("Collapse All", cx);

        // a/, opened, then a.txt.
        palette("Focus Search Results", cx);
        press("right right", cx);
        palette("Dismiss Selected File", cx);
        palette("Replace in Selected File", cx);
        let read = |path: &Path| fs::read_to_string(path).unwrap();
        assert!(
            until(cx, |_| read(&files[1]) == "bar\n"),
            "b/ is replaced in"
        );
        assert_eq!(read(&files[0]), "foo\n");
        assert_eq!(read(&files[2]), "foo\n");
    }

    /// A file is found again by its path, not its label: three names not
    /// UTF-8 whose labels two share, `\x80.txt`, `\x80a.txt` and
    /// `\x81.txt`, in the tree in that order. Dismissing the middle one
    /// selects the last, not the first (T584 review F6). Linux only: APFS
    /// refuses names that aren't UTF-8.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    fn files_whose_labels_collide_stay_apart(cx: &mut TestAppContext) {
        use std::{ffi::OsStr, os::unix::ffi::OsStrExt as _};

        let _serial = serially();
        let dir = tempfile::tempdir().unwrap();
        let root = &dunce::canonicalize(dir.path()).unwrap();
        let [first, middle, last] = [&b"\x80.txt"[..], b"\x80a.txt", b"\x81.txt"]
            .map(|name| root.join(OsStr::from_bytes(name)));
        for file in [&first, &middle, &last] {
            fs::write(file, "foo\n").unwrap();
        }
        let (_workbench, cx) = workbench(root, false, cx);
        palette("Find in Files", cx);
        cx.simulate_input("foo");
        press("enter", cx);
        palette("Replace in Files…", cx);
        cx.simulate_input("bar");
        assert!(until(cx, |cx| count("search-dismiss:", cx) == 3));
        palette("View as Tree", cx);

        // The first file's match, then the middle file's row.
        palette("Focus Search Results", cx);
        press("down", cx);
        palette("Dismiss Selected File", cx);
        assert_eq!(count("search-dismiss:", cx), 2);
        palette("Replace in Selected File", cx);
        let read = |path: &Path| fs::read_to_string(path).unwrap();
        assert!(
            until(cx, |_| read(&last) == "bar\n"),
            "the last is replaced in"
        );
        assert_eq!(read(&first), "foo\n");
        assert_eq!(read(&middle), "foo\n");
    }
}

mod ai_changes {
    use super::*;

    const TWO_FNS: &str = "fn one() {\n    1\n}\n\nfn two() {\n    2\n}\n";
    const PROPOSED: &str = "fn one() {\n    10\n}\n\nfn two() {\n    20\n}\n";

    fn ai() -> Origin {
        Origin::Ai {
            agent: "claude".into(),
            session: "s".into(),
        }
    }

    /// A turn's proposals over `editors`: each file's text, all of it,
    /// replaced with [`PROPOSED`].
    fn changeset(
        editors: &[(PathBuf, Entity<EditorView>)],
        cx: &mut VisualTestContext,
    ) -> Changeset {
        let mut changeset = Changeset::new("turn", ai());
        for (path, editor) in editors {
            let (buffer, proposal) = editor.read_with(cx, |editor, _| {
                let buffer = editor.model().buffer();
                let snapshot = buffer.snapshot();
                let proposal =
                    ProposedChange::from_text(&snapshot, 0..snapshot.len(), PROPOSED, ai());
                (buffer.id(), proposal)
            });
            changeset.add_file(path.clone(), buffer, Rc::new(RefCell::new(proposal)));
        }
        changeset
    }

    /// A refactor's edit of `path`, closed: `text` inserted at its start.
    fn insert_at_start(path: &Path, text: &str) -> Vec<FileEdits> {
        let edit = TextEdit {
            range: Range {
                start: Position::new(0, 0),
                end: Position::new(0, 0),
            },
            new_text: text.into(),
        };
        vec![FileEdits::closed(
            path.to_owned(),
            vec![edit],
            PositionEncoding::Utf16,
        )]
    }

    /// The selected file's Accept and Reject buttons, and a preview's Apply
    /// and Cancel, are palette commands in the panel.
    #[gpui::test]
    fn files_are_decided_and_previews_applied_and_cancelled(cx: &mut TestAppContext) {
        let _serial = serially();
        let dir = tempfile::tempdir().unwrap();
        let root = &dunce::canonicalize(dir.path()).unwrap();
        let paths = ["a.rs", "b.rs"].map(|name| root.join(name));
        for path in &paths {
            fs::write(path, TWO_FNS).unwrap();
        }
        let closed = root.join("c.rs");
        fs::write(&closed, "fn c() {}\n").unwrap();
        let (workbench, cx) = workbench(root, false, cx);
        let editors: Vec<_> = (paths.iter())
            .map(|path| (path.clone(), open(&workbench, path, cx)))
            .collect();
        let changeset = changeset(&editors, cx);
        cx.update(|window, cx| {
            workbench.update(cx, |workbench, cx| {
                workbench.show_ai_changeset(changeset, window, cx)
            })
        });
        draw(cx);
        palette("Show AI Changes", cx);
        let texts = |cx: &mut VisualTestContext| -> Vec<String> {
            (paths.iter())
                .map(|path| text_of(&workbench, path, cx))
                .collect()
        };

        // The first file accepted.
        press("alt-]", cx);
        palette("AI Changes: Accept Selected File", cx);
        let accepted = texts(cx);
        assert_eq!(
            accepted.iter().filter(|text| *text == PROPOSED).count(),
            1,
            "{accepted:?}"
        );
        // The second rejected: decided, so Accept does nothing to it after.
        press("alt-]", cx);
        palette("AI Changes: Reject Selected File", cx);
        palette("AI Changes: Accept Selected File", cx);
        assert_eq!(texts(cx), accepted);

        // A refactor's preview of c.rs, applied.
        let source = editors[0]
            .1
            .read_with(cx, |editor, _| editor.model().buffer().id());
        let preview = |text: &str, cx: &mut VisualTestContext| {
            let edits = insert_at_start(&closed, text);
            cx.update(|window, cx| {
                workbench.update(cx, |workbench, cx| {
                    workbench.preview_file_edits("Rename", source, edits, window, cx)
                })
            })
            .expect("previewed");
            draw(cx);
            assert!(
                bounds("ai-changes-apply", cx).is_some(),
                "the preview shows"
            );
        };
        preview("// one\n", cx);
        palette("AI Changes: Apply Preview", cx);
        assert!(
            bounds("ai-changes-apply", cx).is_none(),
            "applied, it's gone"
        );
        assert_eq!(text_of(&workbench, &closed, cx), "// one\nfn c() {}\n");

        // Another, cancelled.
        preview("// two\n", cx);
        palette("AI Changes: Cancel Preview", cx);
        assert!(
            bounds("ai-changes-apply", cx).is_none(),
            "cancelled, it's gone"
        );
        assert_eq!(text_of(&workbench, &closed, cx), "// one\nfn c() {}\n");
    }
}

/// A repository with two commits: the second changes `a.txt`, thirty lines,
/// at its 2nd and 29th, with rows unchanged between them folded, and
/// `b.txt`'s one line.
fn two_commits() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    let lines: Vec<String> = (1..=30).map(|n| format!("line {n}")).collect();
    fs::write(root.join("a.txt"), lines.join("\n") + "\n").unwrap();
    fs::write(root.join("b.txt"), "one\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "first"]);
    let mut changed = lines.clone();
    changed[1] = "changed 2".into();
    changed[28] = "changed 29".into();
    fs::write(root.join("a.txt"), changed.join("\n") + "\n").unwrap();
    fs::write(root.join("b.txt"), "two\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "second"]);
    dir
}

/// Shows the Graph of `root`'s repository, focused, its commits listed,
/// with `a.txt` open, as the Graph lists the active file's repository.
fn graph(workbench: &Entity<Workbench>, root: &Path, cx: &mut VisualTestContext) {
    open(workbench, &root.join("a.txt"), cx);
    palette("Git: Log", cx);
    let listed = until(cx, |cx| bounds("git-log-row:1", cx).is_some());
    let listing = workbench.read_with(cx, |workbench, cx| workbench.git_log_listing(cx));
    assert!(listed, "the commits are listed: {listing:?}");
}

mod commit_diff {
    use super::*;

    /// The toolbar's buttons, a header's click and a fold's, by keys: F7
    /// and Shift-F7 as in a diff view, the rest from the palette.
    #[gpui::test]
    fn changes_sections_and_folds_by_keys(cx: &mut TestAppContext) {
        let _serial = serially();
        let dir = two_commits();
        let root = &dunce::canonicalize(dir.path()).unwrap();
        let (wb, cx) = workbench(root, false, cx);
        graph(&wb, root, cx);
        // HEAD's row, the first.
        press("up", cx);
        palette("Git: Open Changes", cx);
        until(cx, |cx| count("commit-diff-hunk:", cx) == 3);
        let hunks = |cx: &mut VisualTestContext| count("commit-diff-hunk:", cx);
        assert_eq!(hunks(cx), 3, "a.txt's two hunks and b.txt's");
        assert_eq!(count("commit-diff-header:", cx), 2);

        palette("Commit Diff: Collapse All", cx);
        assert_eq!(hunks(cx), 0);
        // F7 goes to the first change, expanding its section, a.txt's.
        press("f7", cx);
        assert_eq!(hunks(cx), 2);
        // Shift-F7 from there wraps to the last, b.txt's.
        press("shift-f7", cx);
        assert_eq!(hunks(cx), 3);
        // Its section, where navigation is, collapses and expands.
        palette("Commit Diff: Toggle Section", cx);
        assert_eq!(hunks(cx), 2);
        palette("Commit Diff: Toggle Section", cx);
        assert_eq!(hunks(cx), 3);
        palette("Commit Diff: Collapse All", cx);
        palette("Commit Diff: Expand All", cx);
        assert_eq!(hunks(cx), 3);

        // In a.txt, the rows folded between its changes show: F7 goes on
        // from b.txt's section to its change, then wraps to a.txt's first.
        press("f7 f7", cx);
        let folds = count("commit-diff-hidden:", cx);
        assert!(folds > 0, "a.txt's unchanged rows are folded");
        palette("Commit Diff: Show Hidden Lines", cx);
        assert_eq!(count("commit-diff-hidden:", cx), folds - 1);
    }
}

mod git_log {
    use super::*;

    /// Up and Down move a row's menu's highlight, wrapping, and Enter
    /// chooses it.
    #[gpui::test]
    fn the_menu_is_chosen_from_by_keys(cx: &mut TestAppContext) {
        let _serial = serially();
        let dir = two_commits();
        let root = &dunce::canonicalize(dir.path()).unwrap();
        let head = git(root, &["rev-parse", "HEAD"]).trim().to_owned();
        let (wb, cx) = workbench(root, false, cx);
        graph(&wb, root, cx);

        // Open Changes, Copy Commit Hash, Copy Subject: the second.
        right_click("git-log-row:0", cx);
        assert!(bounds("git-log-menu", cx).is_some());
        press("down down enter", cx);
        assert_eq!(clipboard(cx), Some(head));
        assert!(bounds("git-log-menu", cx).is_none(), "choosing closes it");

        // Up from none is the last; Escape closes it, choosing nothing.
        set_clipboard("-", cx);
        right_click("git-log-row:0", cx);
        press("up escape", cx);
        assert!(bounds("git-log-menu", cx).is_none());
        assert_eq!(clipboard(cx).as_deref(), Some("-"));
        right_click("git-log-row:0", cx);
        press("up enter", cx);
        assert_eq!(clipboard(cx).as_deref(), Some("second"));
        // Down wraps past the last to the first: Open Changes.
        right_click("git-log-row:0", cx);
        press("up down enter", cx);
        let opened = until(cx, |cx| bounds("commit-diff-header:0", cx).is_some());
        assert!(opened, "its changes open");
    }

    /// With vim off, `git_log.focus_filter` focuses the filter; Escape
    /// there clears it, and in the empty box gives the keys to the list.
    #[gpui::test]
    fn the_filter_is_reached_and_left_by_keys(cx: &mut TestAppContext) {
        let _serial = serially();
        let dir = two_commits();
        let root = &dunce::canonicalize(dir.path()).unwrap();
        let (wb, cx) = workbench(root, false, cx);
        graph(&wb, root, cx);
        let wait = |cx: &mut VisualTestContext| {
            cx.executor().advance_clock(Duration::from_secs(2));
            draw(cx);
        };
        let rows = |cx: &mut VisualTestContext| count("git-log-row:", cx);
        assert_eq!(rows(cx), 2);

        palette("Git: Focus Log Filter", cx);
        cx.simulate_input("second");
        wait(cx);
        assert_eq!(rows(cx), 1, "filtered from the box");
        // Escape clears it, the keys still in the box.
        press("escape", cx);
        wait(cx);
        assert_eq!(rows(cx), 2);
        cx.simulate_input("no such commit");
        wait(cx);
        assert_eq!(rows(cx), 0, "typed into the box");
        press("escape", cx);
        wait(cx);
        assert_eq!(rows(cx), 2);
        // Escape in the empty box: Enter in the list expands its commit.
        press("escape", cx);
        press("down enter", cx);
        assert!(rows(cx) > 2, "the commit's files are listed under it");
    }

    /// `git_log.open_commit_link` opens the selected commit's page on its
    /// remote's host.
    #[gpui::test]
    fn the_commit_page_opens_by_keys(cx: &mut TestAppContext) {
        let _serial = serially();
        let dir = two_commits();
        let root = &dunce::canonicalize(dir.path()).unwrap();
        git(
            root,
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/owner/repo.git",
            ],
        );
        let head = git(root, &["rev-parse", "HEAD"]).trim().to_owned();
        let (wb, cx) = workbench(root, false, cx);
        graph(&wb, root, cx);
        press("up", cx);
        palette("Git: Open Commit on Remote", cx);
        assert_eq!(
            cx.opened_url(),
            Some(format!("https://github.com/owner/repo/commit/{head}"))
        );
    }
}

mod shortcuts {
    use super::*;

    /// The fake locale's text for the editor's two inputs' placeholders.
    static FAKE_LOCALE: Resources = Resources {
        crate_name: "a11y-reach-panels-fake-locale",
        manifest_dir: env!("CARGO_MANIFEST_DIR"),
        declared_in: file!(),
        files: &[ResourceFile {
            locale: "qps-ploc",
            path: "qps-ploc/shortcuts.ftl",
            source: "workbench-setup-shortcuts-search = ⟦Séárçh kéýbíñdíñgs⟧\n\
                     workbench-setup-shortcuts-column-when = ⟦Whéñ⟧\n",
        }],
        dynamic_prefixes: &[],
    };

    inventory::submit! { Registered(&FAKE_LOCALE) }

    const SEARCH: &str = "Search keybindings by command or keys";

    /// A workbench whose user keymap, in a config directory of its own,
    /// binds `toast.dismiss_all`, with the Keyboard Shortcuts tab open:
    /// the keymap's path.
    fn opened<'a>(dir: &Path, cx: &'a mut TestAppContext) -> (PathBuf, &'a mut VisualTestContext) {
        let config = dir.join("config");
        fs::create_dir_all(&config).unwrap();
        let keymap = config.join("keymap.toml");
        let user = "[[bindings]]\nkeys = \"ctrl-alt-shift-y\"\ncommand = \"toast.dismiss_all\"\n";
        fs::write(&keymap, user).unwrap();
        // SAFETY: the test holds the lock, and gpui hasn't started.
        unsafe { std::env::set_var("TENMOKU_CONFIG_DIR", &config) };
        let root = dir.join("project");
        fs::create_dir_all(&root).unwrap();
        let (_workbench, cx) = workbench(&root, false, cx);
        cx.update(|_, cx| cx.set_global(ConfigDirOverride(Some(config.clone()))));
        palette("Open Keyboard Shortcuts", cx);
        (keymap, cx)
    }

    /// Searches for `text`, then goes down into the list, its first line
    /// selected.
    fn find(text: &str, cx: &mut VisualTestContext) {
        press("secondary-f cmd-backspace", cx);
        cx.simulate_input(text);
        press("down", cx);
    }

    /// The labels of the inputs the window draws.
    fn input_labels(cx: &mut VisualTestContext) -> Vec<String> {
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
            (fe_ui::nodes(window, cx).into_iter())
                .filter(|node| {
                    matches!(node.role, fe_ui::Role::TextInput | fe_ui::Role::SearchInput)
                })
                .map(|node| node.label.to_string())
                .collect()
        })
    }

    /// The selected line's Copy Command ID, Reset Keybinding and Show Same
    /// Keybindings are palette commands.
    #[gpui::test]
    fn the_selected_lines_menu_items_are_commands(cx: &mut TestAppContext) {
        let _serial = serially();
        let dir = tempfile::tempdir().unwrap();
        let (keymap, cx) = opened(dir.path(), cx);

        find("toast.dismiss_all", cx);
        palette("Keyboard Shortcuts: Copy Command ID", cx);
        assert_eq!(clipboard(cx).as_deref(), Some("toast.dismiss_all"));
        palette("Keyboard Shortcuts: Reset Keybinding", cx);
        let reset = fs::read_to_string(&keymap).unwrap();
        assert!(!reset.contains("toast.dismiss_all"), "{reset}");

        // find.close's Escape: the others bound to Escape are found too.
        find("find.close", cx);
        assert!(bounds("shortcuts-row-picker.dismiss", cx).is_none());
        palette("Keyboard Shortcuts: Show Same Keybindings", cx);
        assert!(bounds("shortcuts-row-find.close", cx).is_some());
        assert!(bounds("shortcuts-row-picker.dismiss", cx).is_some());
    }

    /// Up and Down move a line's menu's highlight, wrapping, and Enter
    /// chooses it; the search's and the recorder's When placeholders, its
    /// inputs' names, follow a change of language.
    #[gpui::test]
    fn the_menu_is_chosen_from_by_keys(cx: &mut TestAppContext) {
        let _serial = serially();
        let dir = tempfile::tempdir().unwrap();
        let (_keymap, cx) = opened(dir.path(), cx);

        // Copy Command ID, the first.
        find("find.close", cx);
        set_clipboard("-", cx);
        right_click("shortcuts-row-find.close", cx);
        assert!(bounds("shortcuts-menu", cx).is_some());
        press("down enter", cx);
        assert_eq!(clipboard(cx).as_deref(), Some("find.close"));
        assert!(bounds("shortcuts-menu", cx).is_none(), "choosing closes it");

        // Show Same Keybindings, the last: Up from none.
        right_click("shortcuts-row-find.close", cx);
        press("up enter", cx);
        assert!(bounds("shortcuts-row-picker.dismiss", cx).is_some());

        // Add Keybinding, the second: the recorder, with its When field.
        find("find.close", cx);
        right_click("shortcuts-row-find.close", cx);
        press("up down down enter", cx);
        assert_eq!(input_labels(cx), [SEARCH, "When"]);

        // In the fake locale, and back, the recorder still open.
        let tags: Vec<LanguageIdentifier> = vec!["qps-ploc".parse().unwrap()];
        cx.update(|_, cx| kiln::platform::locale::set_locale(cx, &tags));
        draw(cx);
        assert_eq!(input_labels(cx), ["⟦Séárçh kéýbíñdíñgs⟧", "⟦Whéñ⟧"]);
        cx.update(|_, cx| kiln::platform::locale::set_locale(cx, &[]));
        draw(cx);
        assert_eq!(input_labels(cx), [SEARCH, "When"]);
    }
}

mod settings {
    use super::*;

    const KEY: &str = "editor.code_actions_on_save";

    /// A list's items are reached with Up and Down, added and removed from
    /// the palette, the scope switched and the setting reset.
    #[gpui::test]
    fn list_items_reset_and_scope_by_keys(cx: &mut TestAppContext) {
        let _serial = serially();
        let dir = tempfile::tempdir().unwrap();
        let base = dunce::canonicalize(dir.path()).unwrap();
        let (root, config) = (base.join("project"), base.join("config"));
        fs::create_dir_all(root.join(".tenmoku")).unwrap();
        fs::create_dir_all(&config).unwrap();
        let project = fe_config::project_settings_path(&root);
        // The user's file is written where `TENMOKU_CONFIG_DIR` says.
        // SAFETY: the test holds the lock, and gpui hasn't started.
        unsafe { std::env::set_var("TENMOKU_CONFIG_DIR", &config) };
        let (_workbench, cx) = workbench(&root, false, cx);
        cx.update(|_, cx| cx.set_global(ConfigDirOverride(Some(config.clone()))));
        let read = || fs::read_to_string(&project).unwrap_or_default();

        palette("Open Settings (UI)", cx);
        cx.simulate_input("code_actions_on_save");
        press("down", cx);
        palette("Settings: Switch Scope", cx);

        // Add Item, on the selected row's list, then on the focused one's.
        palette("Settings: Add List Item", cx);
        assert!(bounds(&format!("setting-item-{KEY}-0"), cx).is_some());
        cx.simulate_input("x");
        press("enter", cx);
        assert_eq!(read(), "[editor]\ncode_actions_on_save = [\"x\"]\n");
        palette("Settings: Add List Item", cx);
        cx.simulate_input("y");
        press("enter", cx);
        assert_eq!(read(), "[editor]\ncode_actions_on_save = [\"x\", \"y\"]\n");

        // Up and Down between the items.
        press("up", cx);
        cx.simulate_input("z");
        press("enter", cx);
        assert_eq!(read(), "[editor]\ncode_actions_on_save = [\"xz\", \"y\"]\n");
        press("down", cx);
        cx.simulate_input("w");
        press("enter", cx);
        assert_eq!(
            read(),
            "[editor]\ncode_actions_on_save = [\"xz\", \"yw\"]\n"
        );

        // Remove Item: the focused one, focus going to the one before.
        palette("Settings: Remove List Item", cx);
        assert_eq!(read(), "[editor]\ncode_actions_on_save = [\"xz\"]\n");
        assert!(bounds(&format!("setting-item-{KEY}-1"), cx).is_none());
        cx.simulate_input("!");
        press("enter", cx);
        assert_eq!(read(), "[editor]\ncode_actions_on_save = [\"xz!\"]\n");

        // Reset to Default, from the list.
        press("escape", cx);
        palette("Settings: Reset to Default", cx);
        assert!(!read().contains("code_actions_on_save"), "{}", read());

        // The other scope: the user's, which writes there.
        palette("Settings: Switch Scope", cx);
        palette("Settings: Add List Item", cx);
        cx.simulate_input("u");
        press("enter", cx);
        let user = || fs::read_to_string(config.join("settings.toml")).unwrap_or_default();
        until(cx, |_| user().contains("code_actions_on_save"));
        assert!(
            user().contains("code_actions_on_save = [\"u\"]"),
            "{}",
            user()
        );
    }

    /// Where focus is: each frame's key context, the root's first.
    fn focus_contexts(cx: &mut VisualTestContext) -> Vec<String> {
        cx.update(|window, _| {
            (window.context_stack().iter())
                .filter_map(|context| context.primary().map(|entry| entry.key.to_string()))
                .collect()
        })
    }

    /// Whether the Settings tab's list of rows has focus: the tab, and no
    /// input in it.
    fn list_focused(cx: &mut VisualTestContext) -> bool {
        let contexts = focus_contexts(cx);
        contexts.iter().any(|context| context == "settings_editor")
            && !contexts.iter().any(|context| context == "text_input")
    }

    /// A workbench on a project whose file sets the list to `items`, the
    /// Settings tab open on it, its row selected, This project's values
    /// shown: the project's file.
    fn opened_on_list<'a>(
        dir: &Path,
        items: &[String],
        cx: &'a mut TestAppContext,
    ) -> (PathBuf, &'a mut VisualTestContext) {
        let (root, config) = (dir.join("project"), dir.join("config"));
        fs::create_dir_all(root.join(".tenmoku")).unwrap();
        fs::create_dir_all(&config).unwrap();
        let project = fe_config::project_settings_path(&root);
        let quoted: Vec<String> = items.iter().map(|item| format!("{item:?}")).collect();
        let text = format!("[editor]\ncode_actions_on_save = [{}]\n", quoted.join(", "));
        fs::write(&project, text).unwrap();
        // SAFETY: the test holds the lock, and gpui hasn't started.
        unsafe { std::env::set_var("TENMOKU_CONFIG_DIR", &config) };
        let (_workbench, cx) = workbench(&root, false, cx);
        cx.update(|_, cx| cx.set_global(ConfigDirOverride(Some(config.clone()))));
        palette("Open Settings (UI)", cx);
        cx.simulate_input("code_actions_on_save");
        press("down", cx);
        palette("Settings: Switch Scope", cx);
        (project, cx)
    }

    /// Switching the scope from a list's item, and resetting from one,
    /// remakes the inputs: focus goes to the list of rows, and the keys
    /// go on from there (T584 review F1).
    #[gpui::test]
    fn focus_survives_a_scope_switch_and_a_reset_from_an_item(cx: &mut TestAppContext) {
        let _serial = serially();
        let dir = tempfile::tempdir().unwrap();
        let base = dunce::canonicalize(dir.path()).unwrap();
        let items = ["a".to_owned(), "b".to_owned()];
        let (project, cx) = opened_on_list(&base, &items, cx);
        let read = || fs::read_to_string(&project).unwrap();
        assert!(bounds(&format!("setting-item-{KEY}-1"), cx).is_some());

        // From item b: the user's values, focus on the list.
        palette("Settings: Add List Item", cx);
        press("up", cx);
        assert!(!list_focused(cx), "in item b");
        palette("Settings: Switch Scope", cx);
        assert!(list_focused(cx), "{:?}", focus_contexts(cx));
        // And back, from the list.
        palette("Settings: Switch Scope", cx);
        assert!(bounds(&format!("setting-item-{KEY}-1"), cx).is_some());

        // Reset from item b, whose default list is empty: focus on the
        // list, from which Add Item goes on.
        palette("Settings: Add List Item", cx);
        press("up", cx);
        assert!(!list_focused(cx), "in item b");
        palette("Settings: Reset to Default", cx);
        assert!(!read().contains("code_actions_on_save"), "{}", read());
        assert!(list_focused(cx), "{:?}", focus_contexts(cx));
        palette("Settings: Add List Item", cx);
        cx.simulate_input("c");
        press("enter", cx);
        assert_eq!(read(), "[editor]\ncode_actions_on_save = [\"c\"]\n");
    }

    /// In a list wider than its row, the item the keys move to, or Add
    /// Item adds, scrolls into view (T584 review F2).
    #[gpui::test]
    fn the_focused_item_scrolls_into_view(cx: &mut TestAppContext) {
        let _serial = serially();
        let dir = tempfile::tempdir().unwrap();
        let base = dunce::canonicalize(dir.path()).unwrap();
        let items: Vec<String> = (0..12).map(|n| format!("item{n:02}")).collect();
        let (_project, cx) = opened_on_list(&base, &items, cx);
        let shown = |item: usize, cx: &mut VisualTestContext| {
            let list = bounds(&format!("setting-items-{KEY}"), cx).expect("the list is drawn");
            bounds(&format!("setting-item-{KEY}-{item}"), cx).is_some_and(|item| {
                item.left() >= list.left() - px(1.) && item.right() <= list.right() + px(1.)
            })
        };
        assert!(shown(0, cx));
        assert!(!shown(11, cx), "the list overflows its row");

        // Added at the end: in view.
        palette("Settings: Add List Item", cx);
        assert!(until(cx, |cx| shown(12, cx)), "the new item shows");
        assert!(!shown(0, cx));
        // Up to the first, each in view as it's reached.
        for item in (0..12).rev() {
            press("up", cx);
            assert!(until(cx, |cx| shown(item, cx)), "item {item} shows");
        }

        // Down to the end, out to the row and Enter: back to the first,
        // in view (T584 review F4).
        for item in 1..12 {
            press("down", cx);
            assert!(until(cx, |cx| shown(item, cx)), "item {item} shows");
        }
        assert!(!shown(0, cx));
        press("escape enter", cx);
        assert!(until(cx, |cx| shown(0, cx)), "the first item shows");
    }
}
