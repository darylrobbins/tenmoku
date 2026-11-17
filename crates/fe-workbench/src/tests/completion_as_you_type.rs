//! Quick suggestions in the workbench
//! (docs/decisions/mgacx379x8-completion-as-you-type.md): typing a word asks
//! the file's server, as invoked (`triggerKind` 1, no trigger character),
//! once `editor.completion.delay_ms` passes with nothing typed since; not
//! in a comment, in vim's normal mode, or with `auto_trigger = false`,
//! where `ctrl-space` still asks. `enter` after a menu that opened itself
//! types its new line; `tab` accepts.

use std::{fs, sync::Arc, time::Duration};

use fe_config::{
    Settings, SettingsEvent,
    settings::{CompletionSettings, CompletionWords},
};
use fe_gpui::EditorView;
use fe_lsp::fake::{Record, Reply, Scripted};
use gpui::{Entity, Focusable as _, TestAppContext};
use lsp_types::{
    LSPAny,
    request::{Completion, Request as _},
};

use super::lsp::Lsp;

pub(super) const DELAY: Duration = Duration::from_millis(50);

/// The completion requests the server got, their params.
pub(super) fn asked(s: &Lsp) -> Vec<LSPAny> {
    (s.fake.records().iter())
        .filter_map(|record| match record {
            Record::Request { method, params, .. } if method == Completion::METHOD => {
                Some(params.clone())
            }
            _ => None,
        })
        .collect()
}

/// The server's answer: `labels`, each over `line`'s first two chars.
pub(super) fn reply(labels: &[&str], line: u32) -> Scripted {
    let items: Vec<LSPAny> = (labels.iter())
        .map(|label| {
            serde_json::json!({
                "label": label,
                "kind": 3,
                "textEdit": {
                    "range": {
                        "start": { "line": line, "character": 0 },
                        "end": { "line": line, "character": 2 }
                    },
                    "newText": label
                }
            })
        })
        .collect();
    Scripted::now(Reply::Result(
        serde_json::json!({ "isIncomplete": false, "items": items }),
    ))
}

/// Opens `name`, writing `text` there first, with the caret at its end,
/// focused; for a file the server serves, once it has it open.
pub(super) fn open_at_end(s: &mut Lsp, name: &str, text: &str, served: bool) -> Entity<EditorView> {
    fs::write(s.path(name), text).unwrap();
    s.open(name);
    if served {
        let uri = s.uri(name);
        s.fake
            .wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
    }
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("a tab is shown");
    let len = text.len();
    s.cx.update(|window, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().set_cursor(len);
            cx.notify();
        });
        window.focus(&editor.focus_handle(cx), cx);
    });
    s.draw();
    editor
}

/// Lets `time` pass on the editor's clock, and draws.
pub(super) fn wait(s: &mut Lsp, time: Duration) {
    s.cx.executor().advance_clock(time);
    s.draw();
}

/// Waits for the server to have `name` as `text`: whatever was asked
/// before that edit, it has had too.
pub(super) fn synced(s: &mut Lsp, name: &str, text: &str) {
    let uri = s.uri(name);
    s.fake
        .wait_for_document(&uri, "the edit", |r| r.is_some_and(|r| r.text == text));
}

/// Reloads the settings with `completion` for `[editor.completion]`.
pub(super) fn set_completion(s: &mut Lsp, completion: CompletionSettings) {
    let mut settings = Settings::default();
    settings.editor.completion = completion;
    let event = SettingsEvent {
        settings: Arc::new(settings),
        previous: Arc::new(Settings::default()),
        diagnostics: Vec::new(),
    };
    let workbench = s.workbench.clone();
    workbench.update(&mut s.cx, |w, cx| w.reload_settings(&event, cx));
    s.draw();
}

pub(super) fn text(s: &Lsp, editor: &Entity<EditorView>) -> String {
    editor.read_with(&s.cx, |view, _| view.model().text().to_string())
}

pub(super) fn shown(s: &Lsp, editor: &Entity<EditorView>) -> bool {
    editor.read_with(&s.cx, |view, _| view.completion_shown())
}

pub(super) fn labels(s: &Lsp, editor: &Entity<EditorView>) -> Vec<String> {
    (editor
        .read_with(&s.cx, |view, _| view.completion_matches())
        .into_iter())
    .map(|item| item.label)
    .collect()
}

const MAIN: &str = "fn main() {}\n";

#[gpui::test]
fn typing_a_word_asks_the_server_once_the_delay_passes(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    s.fake
        .set_reply(Completion::METHOD, reply(&["main_loop"], 1));
    let editor = open_at_end(&mut s, "src/main.rs", MAIN, true);

    // A second key within the delay waits again.
    s.cx.simulate_keystrokes("m");
    wait(&mut s, DELAY / 2);
    s.cx.simulate_keystrokes("a");
    wait(&mut s, DELAY - Duration::from_millis(1));
    synced(&mut s, "src/main.rs", "fn main() {}\nma");
    assert_eq!(asked(&s).len(), 0);
    wait(&mut s, Duration::from_millis(1));
    s.until("the menu", |s| shown(s, &editor));
    let asked = asked(&s);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0]["context"]["triggerKind"].as_u64(), Some(1));
    assert!(asked[0]["context"].get("triggerCharacter").is_none());
    let position = &asked[0]["position"];
    assert_eq!(
        (position["line"].as_u64(), position["character"].as_u64()),
        (Some(1), Some(2))
    );
    assert!(editor.read_with(&s.cx, |view, _| view.completion_opened_itself()));

    // `enter` types its new line, and closes it.
    s.cx.simulate_keystrokes("enter");
    s.draw();
    assert_eq!(text(&s, &editor), "fn main() {}\nma\n");
    assert!(!shown(&s, &editor));
}

#[gpui::test]
fn tab_accepts_a_menu_that_opened_itself(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    s.fake
        .set_reply(Completion::METHOD, reply(&["main_loop"], 1));
    let editor = open_at_end(&mut s, "src/main.rs", MAIN, true);
    s.cx.simulate_keystrokes("m a");
    wait(&mut s, DELAY);
    s.until("the menu", |s| shown(s, &editor));
    s.cx.simulate_keystrokes("tab");
    s.draw();
    assert_eq!(text(&s, &editor), "fn main() {}\nmain_loop");
    assert!(!shown(&s, &editor));
}

#[gpui::test]
fn nothing_is_asked_by_itself_in_a_comment(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    s.fake
        .set_reply(Completion::METHOD, reply(&["main_loop"], 1));
    let editor = open_at_end(&mut s, "src/main.rs", "fn main() {}\n// ", true);
    assert!(editor.read_with(&s.cx, |view, _| view.syntax().is_some()));
    s.cx.simulate_keystrokes("m a");
    wait(&mut s, DELAY * 4);
    // Anything asked before the space would have reached the server first.
    s.cx.simulate_keystrokes("space");
    synced(&mut s, "src/main.rs", "fn main() {}\n// ma ");
    assert_eq!(asked(&s).len(), 0);
    assert!(!shown(&s, &editor));
}

#[gpui::test]
fn auto_trigger_off_leaves_ctrl_space(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    set_completion(
        &mut s,
        CompletionSettings {
            auto_trigger: false,
            words: CompletionWords::Off,
            ..CompletionSettings::default()
        },
    );
    s.fake
        .set_reply(Completion::METHOD, reply(&["main_loop"], 1));
    let editor = open_at_end(&mut s, "src/main.rs", MAIN, true);
    s.cx.simulate_keystrokes("m a");
    wait(&mut s, DELAY * 4);
    s.cx.simulate_keystrokes("i");
    synced(&mut s, "src/main.rs", "fn main() {}\nmai");
    assert_eq!(asked(&s).len(), 0);
    s.cx.simulate_keystrokes("backspace");
    synced(&mut s, "src/main.rs", "fn main() {}\nma");

    s.cx.simulate_keystrokes("ctrl-space");
    s.until("the menu", |s| shown(s, &editor));
    assert_eq!(asked(&s).len(), 1);
    assert!(!editor.read_with(&s.cx, |view, _| view.completion_opened_itself()));
    // Asked for, `enter` accepts.
    s.cx.simulate_keystrokes("enter");
    s.draw();
    assert_eq!(text(&s, &editor), "fn main() {}\nmain_loop");
}

#[gpui::test]
fn vim_s_normal_mode_never_asks(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    s.fake
        .set_reply(Completion::METHOD, reply(&["main_loop"], 1));
    let editor = open_at_end(&mut s, "src/main.rs", "fn main() {}\nxx", true);
    s.cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, true));
    s.draw();
    // Normal mode's keys, then typed in insert mode and left at once.
    s.cx.simulate_keystrokes("b e");
    wait(&mut s, DELAY * 4);
    s.cx.simulate_keystrokes("a m a escape");
    wait(&mut s, DELAY * 4);
    s.cx.simulate_keystrokes("x");
    synced(&mut s, "src/main.rs", "fn main() {}\nxxm");
    assert_eq!(asked(&s).len(), 0);
    assert!(!shown(&s, &editor));
}

#[gpui::test]
fn a_split_editor_keeps_the_settings(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    // `never`: an editor without the settings would accept on `enter`.
    set_completion(
        &mut s,
        CompletionSettings {
            accept_on_enter: fe_config::settings::AcceptOnEnter::Never,
            ..CompletionSettings::default()
        },
    );
    let first = open_at_end(&mut s, "notes.txt", "alphabet\n", false);
    s.run("pane.split_right");
    let split = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("the split is shown");
    assert_ne!(split, first);
    s.cx.update(|window, cx| window.focus(&split.focus_handle(cx), cx));
    s.draw();

    // Asked by itself, with the buffer's words.
    s.cx.simulate_keystrokes("a l");
    wait(&mut s, DELAY);
    s.until("the menu", |s| shown(s, &split));
    assert!(split.read_with(&s.cx, |view, _| view.completion_opened_itself()));
    assert_eq!(labels(&s, &split), ["alphabet"]);
    // Moved, `enter` still types its new line.
    s.cx.simulate_keystrokes("down enter");
    s.draw();
    assert_eq!(text(&s, &split), "alphabet\nal\n");
    assert!(!shown(&s, &split));
}
