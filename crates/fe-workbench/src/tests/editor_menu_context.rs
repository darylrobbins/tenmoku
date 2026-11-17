//! The editor menu's items from T1898 ([`crate::editor_menu`]): Code
//! Actions… once the server offers some at the point, and the last group's
//! Run Test, Toggle Breakpoint, Git Blame and Ask Agent.

use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
};

use fe_buffer::Origin;
use fe_chat::ContextKind;
use fe_gpui::EditorView;
use fe_lsp::fake::{Record, Reply, Scripted, initialize_result};
use gpui::{
    Entity, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, PlatformInput, TestAppContext,
    VisualTestContext,
};
use kiln::core::secrets::MemoryStore;
use lsp_types::{
    CodeActionProviderCapability, OneOf, PositionEncodingKind, ServerCapabilities,
    request::{CodeActionRequest, Request},
};
use serde_json::{Value, json};

use super::{
    editor_menu::{every_provider, lsp_editor, ready, right_click, script, served, titles},
    find_bar::{draw, editor, opened, pane},
    lsp::Lsp,
};
use crate::{
    DockPanel, Launch, TerminalPanel, Workbench, chat_panel::KeySource,
    editor_menu::code_actions_answered, task_commands::TestFiles, task_defs::ProjectFiles,
    tests::open,
};

/// `helper` defined on row 0 and called on row 3, as `editor_menu`'s.
const MAIN: &str = "fn helper() {}\n\nfn main() {\n    helper();\n}\n";

/// Capabilities offering every item's provider, code actions too.
fn with_code_actions() -> ServerCapabilities {
    ServerCapabilities {
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        ..every_provider()
    }
}

/// The code action requests the server got.
fn asked(s: &Lsp) -> Vec<Value> {
    (s.fake.records().into_iter())
        .filter_map(|record| match record {
            Record::Request { method, params, .. } if method == CodeActionRequest::METHOD => {
                Some(params)
            }
            _ => None,
        })
        .collect()
}

fn reply(s: &Lsp, result: Value, delay: Duration) {
    let reply = Scripted {
        reply: Reply::Result(result),
        delay,
    };
    s.fake.set_reply(CodeActionRequest::METHOD, reply);
}

/// Waits for `n` answers to the menu's code action requests to have been
/// handled since the test began.
fn until_answered(s: &mut Lsp, n: usize) {
    s.until("the menu's code actions answered", |s| {
        s.cx.update(|_, cx| code_actions_answered(cx)) >= n
    });
}

/// The open menu's second group: Rename Symbol's.
fn refactoring(editor: &Entity<EditorView>, s: &mut Lsp) -> Vec<String> {
    titles(editor, &mut s.cx)[1].clone()
}

fn quick_fix() -> Value {
    json!([{ "title": "Fix it", "kind": "quickfix" }])
}

/// A fake-clocked workbench over `MAIN`, its server offering code actions,
/// `src/main.rs` shown once it has initialized.
fn served_on_fake_clock(cx: &mut TestAppContext) -> (Lsp, Entity<EditorView>) {
    let mut s = Lsp::with_fake_clock(cx, script(with_code_actions()), Default::default());
    s.set_trust(true);
    fs::write(s.path("src/main.rs"), MAIN).unwrap();
    s.open("src/main.rs");
    let editor = lsp_editor(&mut s);
    s.until("the server to initialize", |s| ready(s, &editor));
    (s, editor)
}

#[gpui::test]
fn code_actions_item_appears_after_rename_when_the_server_offers_actions(cx: &mut TestAppContext) {
    let (mut s, editor) = served_on_fake_clock(cx);
    let delay = Duration::from_secs(3);
    reply(&s, quick_fix(), delay);
    let call = MAIN.find("helper();").unwrap();

    // The menu opens at once, without it.
    right_click(&editor, call + 2, &mut s.cx);
    assert_eq!(
        refactoring(&editor, &mut s),
        ["Rename Symbol", "Format Document"]
    );
    s.until("the request", |s| asked(s).len() == 1);
    let sent = &asked(&s)[0];
    // Invoked, of no kind, at the caret the click put.
    assert_eq!(sent["context"]["triggerKind"], 1);
    assert!(sent["context"].get("only").is_none_or(Value::is_null));
    let at = json!({ "line": 3, "character": 6 });
    assert_eq!(sent["range"], json!({ "start": at, "end": at }));

    // Once the actions come, right after Rename Symbol.
    s.clock.wait_for_sleep(delay);
    s.clock.advance(delay);
    until_answered(&mut s, 1);
    assert_eq!(
        refactoring(&editor, &mut s),
        ["Rename Symbol", "Code Actions…", "Format Document"]
    );

    // Choosing it lists them.
    reply(&s, quick_fix(), Duration::ZERO);
    let item = s.cx.debug_bounds("menu-item:Code Actions…").expect("drawn");
    s.cx.simulate_click(item.center(), Modifiers::none());
    s.until("the picker", |s| {
        s.workbench.read_with(&s.cx, |w, _| {
            w.palette == Some(crate::workbench::Palette::CodeActions)
        })
    });
}

#[gpui::test]
fn no_code_actions_item_for_only_source_actions_or_none(cx: &mut TestAppContext) {
    let (mut s, editor) = served(cx, with_code_actions());
    let call = MAIN.find("helper();").unwrap();
    let answers = [
        json!([{ "title": "Organize imports", "kind": "source.organizeImports" }]),
        json!([{ "title": "Fix it", "kind": "quickfix", "disabled": { "reason": "no" } }]),
        json!([]),
        Value::Null,
    ];
    for (ix, answer) in answers.into_iter().enumerate() {
        reply(&s, answer.clone(), Duration::ZERO);
        right_click(&editor, call + 2, &mut s.cx);
        until_answered(&mut s, ix + 1);
        assert_eq!(
            refactoring(&editor, &mut s),
            ["Rename Symbol", "Format Document"],
            "{answer}"
        );
        s.cx.simulate_keystrokes("escape");
        draw(&mut s.cx);
    }
    // A failed request adds nothing either.
    let error = Scripted::now(Reply::Error {
        code: -32603,
        message: "no".into(),
    });
    s.fake.set_reply(CodeActionRequest::METHOD, error);
    right_click(&editor, call + 2, &mut s.cx);
    until_answered(&mut s, 5);
    assert_eq!(
        refactoring(&editor, &mut s),
        ["Rename Symbol", "Format Document"]
    );
    s.cx.simulate_keystrokes("escape");
    draw(&mut s.cx);
    assert_eq!(asked(&s).len(), 5);
}

#[gpui::test]
fn a_server_without_a_code_action_provider_is_not_asked(cx: &mut TestAppContext) {
    let (mut s, editor) = served(cx, every_provider());
    let call = MAIN.find("helper();").unwrap();
    right_click(&editor, call + 2, &mut s.cx);
    assert_eq!(
        refactoring(&editor, &mut s),
        ["Rename Symbol", "Format Document"]
    );
    s.draw();
    assert_eq!(asked(&s), Vec::<Value>::new());
    assert_eq!(s.cx.update(|_, cx| code_actions_answered(cx)), 0);
}

#[gpui::test]
fn a_late_code_action_answer_adds_nothing(cx: &mut TestAppContext) {
    let (mut s, editor) = served_on_fake_clock(cx);
    let delay = Duration::from_secs(3);
    let call = MAIN.find("helper();").unwrap();

    // Answered after the menu closed.
    reply(&s, quick_fix(), delay);
    right_click(&editor, call + 2, &mut s.cx);
    s.until("the request", |s| asked(s).len() == 1);
    s.cx.simulate_keystrokes("escape");
    draw(&mut s.cx);
    assert!(editor.read_with(&s.cx, |view, _| view.editor_menu_entries().is_none()));
    s.clock.wait_for_sleep(delay);
    s.clock.advance(delay);
    until_answered(&mut s, 1);
    assert!(editor.read_with(&s.cx, |view, _| view.editor_menu_entries().is_none()));

    // Answered while a later menu is open: that one's own request is
    // never answered, and the earlier answer adds nothing to it.
    reply(&s, quick_fix(), delay);
    right_click(&editor, call + 2, &mut s.cx);
    s.until("the request", |s| asked(s).len() == 2);
    s.cx.simulate_keystrokes("escape");
    draw(&mut s.cx);
    s.fake
        .set_reply(CodeActionRequest::METHOD, Scripted::now(Reply::Never));
    right_click(&editor, call + 3, &mut s.cx);
    s.until("the request", |s| asked(s).len() == 3);
    s.clock.wait_for_sleep(delay);
    s.clock.advance(delay);
    until_answered(&mut s, 2);
    assert_eq!(
        refactoring(&editor, &mut s),
        ["Rename Symbol", "Format Document"]
    );
    s.cx.simulate_keystrokes("escape");
    draw(&mut s.cx);

    // Answered for an older version of the text: an edit came first.
    reply(&s, quick_fix(), delay);
    right_click(&editor, call + 2, &mut s.cx);
    s.until("the request", |s| asked(s).len() == 4);
    editor.update(&mut s.cx, |view, cx| {
        let end = MAIN.len();
        (view.model_mut().buffer_mut())
            .edit([(end..end, "\n")], Origin::User)
            .unwrap();
        cx.notify();
    });
    s.clock.wait_for_sleep(delay);
    s.clock.advance(delay);
    until_answered(&mut s, 3);
    let entries = editor.read_with(&s.cx, |view, _| {
        (view
            .editor_menu_entries()
            .unwrap_or_default()
            .iter()
            .flatten())
        .map(|item| item.title.to_string())
        .collect::<Vec<_>>()
    });
    assert!(!entries.iter().any(|title| title == "Code Actions…"));
}

/// The open menu's titles by group once the server, offering code
/// actions and `capabilities`, has answered with a quick fix: the menu
/// right-clicked on the call to `helper`.
fn after_a_quick_fix(
    cx: &mut TestAppContext,
    capabilities: ServerCapabilities,
) -> Vec<Vec<String>> {
    let capabilities = ServerCapabilities {
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        ..capabilities
    };
    let (mut s, editor) = served(cx, capabilities);
    reply(&s, quick_fix(), Duration::ZERO);
    right_click(&editor, MAIN.find("helper();").unwrap() + 2, &mut s.cx);
    until_answered(&mut s, 1);
    titles(&editor, &mut s.cx)
}

/// The fake's capabilities: no provider of any menu item.
fn bare() -> ServerCapabilities {
    initialize_result(Some(PositionEncodingKind::UTF8)).capabilities
}

/// What the menu's last groups are, for a file with no test outside a
/// repository, the caret on the click.
const ENDING: [&[&str]; 3] = [
    &["Cut", "Copy", "Paste"],
    &["Copy Path", "Copy Reference"],
    &["Toggle Breakpoint", "Ask Agent About This Line"],
];

fn expected(lsp: &[&[&str]]) -> Vec<Vec<String>> {
    (lsp.iter().chain(&ENDING))
        .map(|group| group.iter().map(|title| title.to_string()).collect())
        .collect()
}

#[gpui::test]
fn without_rename_code_actions_end_the_format_group(cx: &mut TestAppContext) {
    let capabilities = ServerCapabilities {
        rename_provider: None,
        ..every_provider()
    };
    assert_eq!(
        after_a_quick_fix(cx, capabilities),
        expected(&[
            &[
                "Go to Definition",
                "Peek Definition",
                "Go to Implementation",
                "Find References",
                "Peek References",
            ],
            &["Format Document", "Code Actions…"],
        ])
    );
}

#[gpui::test]
fn with_navigation_alone_code_actions_get_a_group_after_it(cx: &mut TestAppContext) {
    let capabilities = ServerCapabilities {
        definition_provider: Some(OneOf::Left(true)),
        ..bare()
    };
    assert_eq!(
        after_a_quick_fix(cx, capabilities),
        expected(&[&["Go to Definition", "Peek Definition"], &["Code Actions…"]])
    );
}

#[gpui::test]
fn with_only_code_actions_they_are_the_first_group(cx: &mut TestAppContext) {
    let capabilities = ServerCapabilities {
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        ..bare()
    };
    assert_eq!(
        after_a_quick_fix(cx, capabilities),
        expected(&[&["Code Actions…"]])
    );
}

#[gpui::test]
fn with_range_formatting_and_no_selection_they_are_the_first_group(cx: &mut TestAppContext) {
    // Format Selection needs a selection, so no language server item shows.
    let capabilities = ServerCapabilities {
        document_range_formatting_provider: Some(OneOf::Left(true)),
        ..bare()
    };
    assert_eq!(
        after_a_quick_fix(cx, capabilities),
        expected(&[&["Code Actions…"]])
    );
}

/// A Cargo package's `src/lib.rs`: a function and its test module.
const LIB: &str = "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n\n\
                   #[cfg(test)]\nmod tests {\n    use super::*;\n\n    \
                   #[test]\n    fn adds() {\n        assert_eq!(add(1, 2), 3);\n    }\n}\n";

/// The disk, for the test command's reads.
struct Disk;

impl ProjectFiles for Disk {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        fs::read_to_string(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.is_file()
    }
}

/// A trusted workbench showing [`LIB`] in a package, its servers off,
/// its terminal panel starting no process.
fn package(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    Entity<EditorView>,
    &mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"p\"\n").unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    let path = dir.path().join("src/lib.rs");
    fs::write(&path, LIB).unwrap();
    let settings = fe_config::project_settings_path(dir.path());
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    // Nor gopls, for a test that sets the language to Go.
    let servers = "[lsp.servers.rust-analyzer]\nenabled = false\n\n\
                   [lsp.servers.gopls]\nenabled = false\n";
    fs::write(settings, servers).unwrap();
    cx.update(|cx| cx.set_global(TestFiles(Arc::new(Disk))));
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(fe_buffer::Buffer::new(LIB).into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    let terminal = workbench.read_with(cx, |w, _| w.terminal().clone());
    terminal.update(cx, |terminal, _| terminal.fake_sessions = true);
    workbench.update(cx, |w, cx| w.set_root_trust(true, cx));
    draw(cx);
    let editor = editor(&pane(&workbench, cx), cx);
    (dir, workbench, editor, cx)
}

/// The open menu's last group.
fn last_group(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<String> {
    titles(editor, cx).pop().unwrap_or_default()
}

/// What the terminal's sessions ran.
fn started(terminal: &Entity<TerminalPanel>, cx: &mut VisualTestContext) -> Vec<String> {
    terminal.read_with(cx, |terminal, _| {
        (terminal.started_options.iter())
            .map(|options| {
                let (program, args) = options.shell.clone().expect("a task's command");
                [vec![program], args].concat().join(" ")
            })
            .collect()
    })
}

#[gpui::test]
fn run_test_shows_on_a_test_and_runs_it(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = package(cx);
    let terminal = workbench.read_with(cx, |w, _| w.terminal().clone());

    right_click(&editor, LIB.find("assert_eq!").unwrap(), cx);
    assert_eq!(last_group(&editor, cx)[0], "Run Test");
    let item = cx.debug_bounds("menu-item:Run Test").expect("drawn");
    cx.simulate_click(item.center(), Modifiers::none());
    draw(cx);
    draw(cx);
    assert_eq!(
        started(&terminal, cx),
        ["cargo test -p p tests::adds -- --exact --nocapture"]
    );

    // In the module, between its tests: the module's.
    right_click(&editor, LIB.find("use super").unwrap(), cx);
    assert_eq!(last_group(&editor, cx)[0], "Run Tests");
}

#[gpui::test]
fn no_run_test_outside_a_test_or_while_parsing(cx: &mut TestAppContext) {
    let (_dir, _workbench, editor, cx) = package(cx);
    let in_test = LIB.find("assert_eq!").unwrap();

    // Outside every test.
    right_click(&editor, LIB.find("a + b").unwrap(), cx);
    assert!(!last_group(&editor, cx).contains(&"Run Test".to_owned()));
    cx.simulate_keystrokes("escape");
    draw(cx);

    // Pressed with the parse behind an edit, which no frame has caught up.
    let at = editor.update(cx, |view, _| {
        view.caret_bounds_at(in_test).unwrap().center()
    });
    cx.update(|window, cx| {
        editor.update(cx, |view, cx| {
            let end = view.model().text().len();
            (view.model_mut().buffer_mut())
                .edit([(end..end, "\n")], Origin::User)
                .unwrap();
            cx.notify();
        });
        press(at, window, cx);
    });
    draw(cx);
    let group = last_group(&editor, cx);
    assert!(
        !group.iter().any(|title| title.starts_with("Run Test")),
        "{group:?}"
    );
    cx.simulate_keystrokes("escape");
    draw(cx);

    // Pressed while the file's language loads: Go, which nothing in this
    // process has loaded.
    cx.update(|window, cx| {
        editor.update(cx, |view, cx| view.set_language(Some("go"), cx));
        assert!(editor.read(cx).is_loading_language());
        press(at, window, cx);
    });
    draw(cx);
    let group = last_group(&editor, cx);
    assert!(
        !group.iter().any(|title| title.starts_with("Run Test")),
        "{group:?}"
    );

    // Rust again, caught up: it's there.
    cx.simulate_keystrokes("escape");
    editor.update(cx, |view, cx| view.set_language(Some("rust"), cx));
    draw(cx);
    draw(cx);
    right_click(&editor, in_test, cx);
    assert_eq!(last_group(&editor, cx)[0], "Run Test");
}

/// A right press and release at `at`, as the platform delivers them.
fn press(at: gpui::Point<gpui::Pixels>, window: &mut gpui::Window, cx: &mut gpui::App) {
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Right,
            position: at,
            modifiers: Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Right,
            position: at,
            modifiers: Modifiers::none(),
            click_count: 1,
        }),
        cx,
    );
}

#[gpui::test]
fn toggle_breakpoint_follows_breakpoint_handlers(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "zero\none\ntwo\n")]);
    let tab = editor(&pane(&workbench, cx), cx);
    assert!(tab.read_with(cx, |view, _| view.has_breakpoint_handlers()));

    // On row 1: the breakpoint goes there.
    right_click(&tab, 6, cx);
    assert!(last_group(&tab, cx).contains(&"Toggle Breakpoint".to_owned()));
    let item = cx
        .debug_bounds("menu-item:Toggle Breakpoint")
        .expect("drawn");
    cx.simulate_click(item.center(), Modifiers::none());
    draw(cx);
    let rows = |cx: &mut VisualTestContext| {
        tab.read_with(cx, |view, _| {
            (view.breakpoint_marks().into_iter())
                .filter(|mark| mark.breakpoint.is_some())
                .map(|mark| mark.row)
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(rows(cx), [1]);

    // Without the handlers, no item.
    tab.update(cx, |view, cx| view.set_breakpoint_handlers(None, cx));
    draw(cx);
    right_click(&tab, 6, cx);
    assert!(!last_group(&tab, cx).contains(&"Toggle Breakpoint".to_owned()));
}

/// Runs `git args` in `root`, as a fixed author.
fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Ada")
        .env("GIT_AUTHOR_EMAIL", "ada@example.com")
        .env("GIT_COMMITTER_NAME", "Ada")
        .env("GIT_COMMITTER_EMAIL", "ada@example.com")
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

#[gpui::test]
fn git_blame_item_shows_in_a_repository_with_its_state(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    fs::write(root.join("a.txt"), "alpha\nbeta\n").unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "a"]);
    let outside = tempfile::tempdir().unwrap();
    let elsewhere: PathBuf = dunce::canonicalize(outside.path()).unwrap().join("b.txt");
    fs::write(&elsewhere, "beta\n").unwrap();
    let launch = Launch {
        root: Some(root.clone()),
        file: Some((
            root.join("a.txt"),
            Some(fe_buffer::Buffer::new("alpha\nbeta\n").into()),
        )),
    };
    let (workbench, cx, _) = open(cx, launch);
    draw(cx);
    let a = root.join("a.txt");
    for _ in 0..500 {
        let found = workbench.read_with(cx, |w, cx| w.in_git_repository(&a, cx));
        if found {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
        draw(cx);
    }
    let tab = editor(&pane(&workbench, cx), cx);

    right_click(&tab, 2, cx);
    assert!(last_group(&tab, cx).contains(&"Annotate with Git Blame".to_owned()));
    let item = cx
        .debug_bounds("menu-item:Annotate with Git Blame")
        .expect("drawn");
    cx.simulate_click(item.center(), Modifiers::none());
    draw(cx);
    assert!(workbench.read_with(cx, |w, cx| w.blame_annotates(&tab, cx)));

    // Annotated: the item takes them away.
    right_click(&tab, 2, cx);
    let group = last_group(&tab, cx);
    assert!(group.contains(&"Hide Git Blame".to_owned()), "{group:?}");
    assert!(!group.contains(&"Annotate with Git Blame".to_owned()));
    let item = cx.debug_bounds("menu-item:Hide Git Blame").expect("drawn");
    cx.simulate_click(item.center(), Modifiers::none());
    draw(cx);
    assert!(!workbench.read_with(cx, |w, cx| w.blame_annotates(&tab, cx)));

    // A file outside the repository has neither.
    let pane = pane(&workbench, cx);
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(elsewhere, window, cx)));
    draw(cx);
    let other = editor(&pane, cx);
    right_click(&other, 2, cx);
    let group = last_group(&other, cx);
    assert!(
        !group.iter().any(|title| title.contains("Git Blame")),
        "{group:?}"
    );
}

#[gpui::test]
fn ask_agent_attaches_the_selection(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "alpha\nbeta\n")]);
    cx.update(|_, cx| {
        cx.set_global(KeySource {
            env: Some("key".into()),
            store: Arc::new(MemoryStore::new()),
        })
    });
    assert!(!workbench.read_with(cx, |w, _| w.root_trusted()));
    let tab = editor(&pane(&workbench, cx), cx);

    // Nothing selected: the line.
    right_click(&tab, 8, cx);
    assert_eq!(
        last_group(&tab, cx).last().map(String::as_str),
        Some("Ask Agent About This Line")
    );
    cx.simulate_keystrokes("escape");
    draw(cx);

    // Clicked inside a selection, which it keeps: attached.
    tab.update(cx, |view, cx| {
        view.model_mut().select_range(0..5, false);
        cx.notify();
    });
    draw(cx);
    right_click(&tab, 2, cx);
    assert_eq!(
        last_group(&tab, cx).last().map(String::as_str),
        Some("Ask Agent About Selection")
    );
    let item = cx
        .debug_bounds("menu-item:Ask Agent About Selection")
        .expect("drawn");
    cx.simulate_click(item.center(), Modifiers::none());
    draw(cx);
    draw(cx);
    let (shown, active) = workbench.read_with(cx, |w, cx| {
        let dock = w.dock().read(cx);
        (dock.is_visible(), dock.active())
    });
    assert_eq!((shown, active), (true, DockPanel::Chat));
    let panel = workbench.read_with(cx, |w, cx| w.dock().read(cx).chat().cloned());
    let chips = panel.expect("made").read_with(cx, |panel, cx| {
        (panel.view().read(cx).chips().into_iter())
            .map(|chip| chip.item)
            .map(|item| {
                assert!(matches!(item.kind, ContextKind::Selection { .. }));
                (item.label, item.text)
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(chips, [("a.txt:1-1".to_owned(), "alpha".to_owned())]);
}
