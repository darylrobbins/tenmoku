//! The editor's right-click menu's items ([`crate::editor_menu`]): its
//! groups and keys, what a read-only, untitled or plain text editor gets,
//! the language server items' readiness and capabilities, and acting on the
//! editor clicked.

use std::{fs, ops::Range, path::PathBuf, time::Duration};

use fe_buffer::Origin;
use fe_gpui::{DiffView, EditorView};
use fe_lsp::fake::{InitializeReply, Reply, Script, Scripted, initialize_result};
use gpui::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext};
use lsp_types::{
    ImplementationProviderCapability, LSPAny, LSPObject, OneOf, PositionEncodingKind,
    ServerCapabilities, notification::Notification,
};

use super::{
    find_bar::{draw, editor, opened, pane, run},
    lsp::Lsp,
};

/// `helper` defined on row 0 and called on row 3.
const MAIN: &str = "fn helper() {}\n\nfn main() {\n    helper();\n}\n";

/// A right press and release over `offset` in `editor`, then a frame.
fn right_click(editor: &Entity<EditorView>, offset: usize, cx: &mut VisualTestContext) {
    let at = editor.update(cx, |editor, _| {
        editor.caret_bounds_at(offset).expect("on screen").center()
    });
    cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
    draw(cx);
    draw(cx);
}

/// The open menu's items, by group, as `(title, enabled)`.
fn groups(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<Vec<(String, bool)>> {
    editor.read_with(cx, |editor, _| {
        let mut groups = vec![Vec::new()];
        for entry in editor.editor_menu_entries().expect("the menu is open") {
            match entry {
                Some(item) => {
                    (groups.last_mut().unwrap()).push((item.title.to_string(), item.enabled))
                }
                None => groups.push(Vec::new()),
            }
        }
        groups
    })
}

/// The open menu's titles, by group.
fn titles(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<Vec<String>> {
    (groups(editor, cx).into_iter())
        .map(|group| group.into_iter().map(|(title, _)| title).collect())
        .collect()
}

/// The open menu's item titled `title`'s key.
fn shortcut(
    editor: &Entity<EditorView>,
    title: &str,
    cx: &mut VisualTestContext,
) -> Option<String> {
    editor.read_with(cx, |editor, _| {
        let entries = editor.editor_menu_entries().expect("the menu is open");
        let item = (entries.iter().flatten()).find(|item| item.title == title);
        item.expect("the item")
            .shortcut
            .as_ref()
            .map(ToString::to_string)
    })
}

/// `written`'s label, as menus print it on this platform.
fn label(written: &str) -> String {
    fe_keymap::shortcut_label(&fe_keymap::KeySequence::parse(written).unwrap())
}

fn strings(groups: &[&[&str]]) -> Vec<Vec<String>> {
    (groups.iter())
        .map(|group| group.iter().map(|title| title.to_string()).collect())
        .collect()
}

const NAVIGATION: &[&str] = &[
    "Go to Definition",
    "Peek Definition",
    "Go to Implementation",
    "Find References",
    "Peek References",
];
const CLIPBOARD: &[&str] = &["Cut", "Copy", "Paste"];
const PATH: &[&str] = &["Copy Path"];

/// Capabilities offering every item's provider.
fn every_provider() -> ServerCapabilities {
    ServerCapabilities {
        definition_provider: Some(OneOf::Left(true)),
        implementation_provider: Some(ImplementationProviderCapability::Simple(true)),
        references_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Left(true)),
        document_formatting_provider: Some(OneOf::Left(true)),
        document_range_formatting_provider: Some(OneOf::Left(true)),
        ..initialize_result(Some(PositionEncodingKind::UTF8)).capabilities
    }
}

/// rust-analyzer's fake answering `initialize` with `capabilities`.
fn script(capabilities: ServerCapabilities) -> Script {
    let mut result = initialize_result(Some(PositionEncodingKind::UTF8));
    result.capabilities = capabilities;
    Script {
        initialize: InitializeReply::Result(Box::new(result)),
        ..Script::default()
    }
}

fn lsp_editor(s: &mut Lsp) -> Entity<EditorView> {
    s.workbench
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("a tab is shown")
}

/// Whether some server `editor`'s buffer is on has initialized.
fn ready(s: &mut Lsp, editor: &Entity<EditorView>) -> bool {
    let buffer = editor.read_with(&s.cx, |editor, _| editor.model().buffer().id());
    s.workbench.read_with(&s.cx, |w, _| {
        let Some(manager) = w.lsp.manager() else {
            return false;
        };
        let manager = manager.lock().unwrap();
        (manager.servers_of(buffer).into_iter())
            .any(|server| manager.capabilities(server).is_some())
    })
}

/// A trusted workbench over `MAIN`, its server offering `capabilities`,
/// `src/main.rs` shown once its server has initialized.
fn served(cx: &mut TestAppContext, capabilities: ServerCapabilities) -> (Lsp, Entity<EditorView>) {
    let mut s = Lsp::with_scripts(cx, script(capabilities), Script::default());
    s.set_trust(true);
    fs::write(s.path("src/main.rs"), MAIN).unwrap();
    s.open("src/main.rs");
    let editor = lsp_editor(&mut s);
    s.until("the server to initialize", |s| ready(s, &editor));
    (s, editor)
}

fn select(editor: &Entity<EditorView>, range: Range<usize>, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, cx| {
        editor.model_mut().select_range(range, false);
        cx.notify();
    });
    draw(cx);
}

#[gpui::test]
fn lists_groups_in_order_with_their_shortcuts(cx: &mut TestAppContext) {
    let (mut s, editor) = served(cx, every_provider());
    let call = MAIN.find("helper();").unwrap();
    select(&editor, call..call + 6, &mut s.cx);
    right_click(&editor, call + 2, &mut s.cx);
    assert_eq!(
        titles(&editor, &mut s.cx),
        strings(&[
            NAVIGATION,
            &["Rename Symbol", "Format Document", "Format Selection"],
            CLIPBOARD,
            PATH,
        ])
    );
    let shown = groups(&editor, &mut s.cx);
    assert!(
        shown.iter().flatten().all(|(_, enabled)| *enabled),
        "{shown:?}"
    );
    // Each with the key that runs it in the editor.
    assert_eq!(
        shortcut(&editor, "Go to Definition", &mut s.cx),
        Some(label("f12"))
    );
    assert_eq!(shortcut(&editor, "Copy", &mut s.cx), Some(label("cmd-c")));
    assert_eq!(shortcut(&editor, "Paste", &mut s.cx), Some(label("cmd-v")));
}

#[gpui::test]
fn read_only_editor_leaves_out_cut_and_paste(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "alpha\nbeta\n")]);
    let tab = editor(&pane(&workbench, cx), cx);
    tab.update(cx, |tab, cx| tab.set_read_only(true, cx));
    draw(cx);
    right_click(&tab, 2, cx);
    assert_eq!(titles(&tab, cx), strings(&[&["Copy"], PATH]));
}

#[gpui::test]
fn untitled_buffer_leaves_out_the_path_items(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "alpha\nbeta\n")]);
    run("workbench.new_file", cx);
    let tab = editor(&pane(&workbench, cx), cx);
    assert!(tab.read_with(cx, |tab, _| tab.model().path().is_none()));
    right_click(&tab, 0, cx);
    assert_eq!(titles(&tab, cx), strings(&[CLIPBOARD]));
}

#[gpui::test]
fn lsp_items_are_disabled_until_a_server_is_ready(cx: &mut TestAppContext) {
    let delay = Duration::from_secs(7);
    let mut rust = script(every_provider());
    rust.initialize_delay = delay;
    let mut s = Lsp::with_fake_clock(cx, rust, Script::default());
    s.set_trust(true);
    fs::write(s.path("src/main.rs"), MAIN).unwrap();
    s.open("src/main.rs");
    let editor = lsp_editor(&mut s);
    s.clock.wait_for_sleep(delay);
    assert!(!ready(&mut s, &editor));

    // Starting: shown, disabled; the rest as ever.
    right_click(&editor, 3, &mut s.cx);
    let starting = &[
        NAVIGATION,
        &["Rename Symbol", "Format Document"],
        CLIPBOARD,
        PATH,
    ];
    assert_eq!(titles(&editor, &mut s.cx), strings(starting));
    let shown = groups(&editor, &mut s.cx);
    for (ix, group) in shown.iter().enumerate() {
        let enabled = ix >= 2;
        assert!(group.iter().all(|(_, on)| *on == enabled), "{shown:?}");
    }
    s.cx.simulate_keystrokes("escape");

    // Ready: enabled.
    s.clock.advance(delay);
    s.until("the server to initialize", |s| ready(s, &editor));
    right_click(&editor, 3, &mut s.cx);
    assert_eq!(titles(&editor, &mut s.cx), strings(starting));
    let shown = groups(&editor, &mut s.cx);
    assert!(shown.iter().flatten().all(|(_, on)| *on), "{shown:?}");
}

#[gpui::test]
fn lsp_items_follow_the_servers_capabilities(cx: &mut TestAppContext) {
    let capabilities = ServerCapabilities {
        definition_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Left(true)),
        // Said, and said to be off.
        references_provider: Some(OneOf::Left(false)),
        implementation_provider: Some(ImplementationProviderCapability::Simple(false)),
        ..initialize_result(Some(PositionEncodingKind::UTF8)).capabilities
    };
    let (mut s, editor) = served(cx, capabilities);
    select(&editor, 3..9, &mut s.cx);
    right_click(&editor, 5, &mut s.cx);
    assert_eq!(
        titles(&editor, &mut s.cx),
        strings(&[
            &["Go to Definition", "Peek Definition"],
            &["Rename Symbol"],
            CLIPBOARD,
            PATH,
        ])
    );
}

#[gpui::test]
fn plain_text_has_no_lsp_items(cx: &mut TestAppContext) {
    let (mut s, _) = served(cx, every_provider());
    fs::write(s.path("notes.txt"), "some notes\n").unwrap();
    s.open("notes.txt");
    let editor = lsp_editor(&mut s);
    right_click(&editor, 2, &mut s.cx);
    assert_eq!(titles(&editor, &mut s.cx), strings(&[CLIPBOARD, PATH]));
}

#[gpui::test]
fn format_selection_needs_a_selection(cx: &mut TestAppContext) {
    let (mut s, editor) = served(cx, every_provider());
    right_click(&editor, 5, &mut s.cx);
    assert_eq!(
        titles(&editor, &mut s.cx)[1],
        ["Rename Symbol", "Format Document"]
    );
    s.cx.simulate_keystrokes("escape");
    draw(&mut s.cx);

    select(&editor, 3..9, &mut s.cx);
    right_click(&editor, 5, &mut s.cx);
    assert_eq!(
        titles(&editor, &mut s.cx)[1],
        ["Rename Symbol", "Format Document", "Format Selection"]
    );
}

/// rust-analyzer's readiness notification.
enum ServerStatusNotification {}

impl Notification for ServerStatusNotification {
    type Params = LSPAny;
    const METHOD: &'static str = "experimental/serverStatus";
}

fn object(fields: impl IntoIterator<Item = (&'static str, LSPAny)>) -> LSPAny {
    let fields = fields
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value));
    LSPAny::Object(fields.collect::<LSPObject>())
}

#[gpui::test]
fn go_to_definition_from_the_menu_jumps(cx: &mut TestAppContext) {
    let (mut s, editor) = served(cx, every_provider());
    let (uri, path) = (s.uri("src/main.rs"), s.path("src/main.rs"));
    s.fake.notify::<ServerStatusNotification>(object([
        ("health", "ok".into()),
        ("quiescent", true.into()),
    ]));
    s.wait_item(None);
    // `main`, on row 2, which search wouldn't answer for `helper`.
    let position = |column: u32| object([("line", 2.into()), ("character", column.into())]);
    let range = object([("start", position(3)), ("end", position(7))]);
    let location = object([("uri", uri.as_str().into()), ("range", range)]);
    s.fake.set_reply(
        "textDocument/definition",
        Scripted::now(Reply::Result(LSPAny::Array(vec![location]))),
    );

    let call = MAIN.find("helper();").unwrap();
    right_click(&editor, call + 2, &mut s.cx);
    let item =
        s.cx.debug_bounds("menu-item:Go to Definition")
            .expect("drawn");
    s.cx.simulate_click(item.center(), Modifiers::none());
    let main = MAIN.find("main").unwrap();
    s.until("the jump", |s| {
        let editor = lsp_editor(s);
        editor.read_with(&s.cx, |editor, _| {
            let model = editor.model();
            model.path() == Some(path.as_path()) && model.selections()[0].range == (main..main + 4)
        })
    });
}

#[gpui::test]
fn right_click_in_an_inactive_split_acts_on_it(cx: &mut TestAppContext) {
    let (dir, workbench, cx) = opened(cx, &[("a.txt", "alpha\n"), ("b.txt", "beta\n")]);
    let left = pane(&workbench, cx);
    run("pane.split_right", cx);
    let right = pane(&workbench, cx);
    assert_ne!(left, right);
    let a = dir.path().join("a.txt");
    cx.update(|window, cx| right.update(cx, |pane, cx| pane.open(a, window, cx)));
    draw(cx);
    let clicked = editor(&left, cx);
    let b: PathBuf = clicked.read_with(cx, |editor, _| editor.model().path().unwrap().to_owned());
    assert!(b.ends_with("b.txt"));

    right_click(&clicked, 2, cx);
    assert_eq!(pane(&workbench, cx), left, "the clicked pane is active");
    assert!(clicked.read_with(cx, |editor, _| editor.editor_menu_entries().is_some()));
    let copy_path = cx.debug_bounds("menu-item:Copy Path").expect("drawn");
    cx.simulate_click(copy_path.center(), Modifiers::none());
    draw(cx);
    let copied = cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()));
    assert_eq!(copied, Some(b.to_string_lossy().into_owned()));
}

#[gpui::test]
fn other_editors_get_only_the_clipboard(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "alpha\nbeta\n")]);
    let tab = editor(&pane(&workbench, cx), cx);
    tab.update(cx, |tab, cx| {
        (tab.model_mut().buffer_mut())
            .edit([(0..5, "ALPHA")], Origin::User)
            .unwrap();
        cx.notify();
    });
    draw(cx);
    run("diff.compare_saved", cx);
    let diff = workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        pane.active_item()
            .and_then(|item| item.downcast_ref::<DiffView>().cloned())
            .expect("a diff view is shown")
    });
    let [saved, working] =
        diff.read_with(cx, |diff, _| [diff.left().clone(), diff.right().clone()]);

    right_click(&saved, 2, cx);
    assert_eq!(titles(&saved, cx), strings(&[&["Copy"]]), "read-only");
    cx.simulate_keystrokes("escape");
    draw(cx);
    right_click(&working, 2, cx);
    assert_eq!(titles(&working, cx), strings(&[CLIPBOARD]));
}
