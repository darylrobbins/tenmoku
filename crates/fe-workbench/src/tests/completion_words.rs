//! Words from open buffers in the workbench
//! (docs/decisions/mgacx379x8-completion-as-you-type.md §2): without a
//! server, plain text and config files offer the words of the open buffers
//! in their language, not the word being typed; with one, words follow its
//! items, its duplicates dropped; `editor.completion.words` says where they
//! come from.

use fe_config::settings::{CompletionSettings, CompletionWords};
use gpui::TestAppContext;
use lsp_types::request::{Completion, Request as _};

use super::{
    completion_as_you_type::{
        DELAY, asked, labels, open_at_end, reply, set_completion, shown, wait,
    },
    lsp::Lsp,
};

#[gpui::test]
fn plain_text_offers_the_open_buffers_words(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    open_at_end(&mut s, "src/lib.rs", "pub fn alpaca() {}\n", true);
    open_at_end(&mut s, "notes.txt", "alphabet alpine zebra\n", false);
    let editor = open_at_end(&mut s, "todo.txt", "algebra\n", false);
    s.cx.simulate_keystrokes("a l p");
    wait(&mut s, DELAY);
    s.until("the menu", |s| shown(s, &editor));
    // Not `alp`, typed, nor the Rust file's `alpaca`.
    assert_eq!(labels(&s, &editor), ["alphabet", "alpine"]);
    let items = editor.read_with(&s.cx, |view, _| view.completion_matches());
    assert_eq!(items[0].kind.as_deref(), Some("text"));
    assert_eq!(items[0].detail.as_deref(), Some("Word in notes.txt"));

    // This buffer's words come first.
    s.cx.simulate_keystrokes("escape backspace");
    s.cx.simulate_keystrokes("ctrl-space");
    s.until("the menu", |s| shown(s, &editor));
    assert_eq!(labels(&s, &editor), ["algebra", "alphabet", "alpine"]);
    let items = editor.read_with(&s.cx, |view, _| view.completion_matches());
    assert_eq!(items[0].detail, None);

    s.cx.simulate_keystrokes("tab");
    s.draw();
    assert_eq!(
        editor.read_with(&s.cx, |view, _| view.model().text().to_string()),
        "algebra\nalgebra"
    );
}

#[gpui::test]
fn a_config_file_offers_its_language_s_words(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    open_at_end(&mut s, "names.txt", "name_two\n", false);
    open_at_end(&mut s, "a.toml", "[package]\nname_one = 1\n", false);
    let editor = open_at_end(&mut s, "b.toml", "", false);
    s.until("TOML", |s| {
        editor.read_with(&s.cx, |view, _| view.syntax().is_some())
    });
    s.cx.simulate_keystrokes("n a m");
    wait(&mut s, DELAY);
    s.until("the menu", |s| shown(s, &editor));
    assert_eq!(labels(&s, &editor), ["name_one"]);
    let items = editor.read_with(&s.cx, |view, _| view.completion_matches());
    assert_eq!(items[0].detail.as_deref(), Some("Word in a.toml"));
}

#[gpui::test]
fn words_follow_the_server_s_items_without_its_duplicates(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    s.fake
        .set_reply(Completion::METHOD, reply(&["main_loop", "main"], 3));
    let text = "fn main() {\n    let marker = 1;\n}\n";
    let editor = open_at_end(&mut s, "src/main.rs", text, true);
    s.cx.simulate_keystrokes("m a");
    wait(&mut s, DELAY);
    s.until("the menu", |s| shown(s, &editor));
    assert_eq!(asked(&s).len(), 1);
    // `main` once, the server's; `marker` the buffer's, after.
    assert_eq!(labels(&s, &editor), ["main", "main_loop", "marker"]);
    let items = editor.read_with(&s.cx, |view, _| view.completion_matches());
    assert_eq!(items[0].kind.as_deref(), Some("function"));
    assert_eq!(items[2].kind.as_deref(), Some("text"));
}

#[gpui::test]
fn the_words_setting_says_where_they_come_from(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    open_at_end(&mut s, "notes.txt", "alphabet\n", false);
    let editor = open_at_end(&mut s, "todo.txt", "algebra\n", false);
    let ask = |s: &mut Lsp| {
        s.cx.simulate_keystrokes("ctrl-space");
        wait(s, DELAY);
    };
    let words = |s: &mut Lsp, words| {
        set_completion(
            s,
            CompletionSettings {
                auto_trigger: false,
                words,
                ..CompletionSettings::default()
            },
        );
    };

    words(&mut s, CompletionWords::OpenBuffers);
    s.cx.simulate_keystrokes("a l");
    ask(&mut s);
    s.until("the menu", |s| shown(s, &editor));
    assert_eq!(labels(&s, &editor), ["algebra", "alphabet"]);
    s.cx.simulate_keystrokes("escape");

    words(&mut s, CompletionWords::CurrentBuffer);
    ask(&mut s);
    s.until("the menu", |s| shown(s, &editor));
    assert_eq!(labels(&s, &editor), ["algebra"]);
    s.cx.simulate_keystrokes("escape");

    words(&mut s, CompletionWords::Off);
    ask(&mut s);
    for _ in 0..5 {
        wait(&mut s, DELAY);
    }
    assert!(!shown(&s, &editor));
    // Still typed as it was.
    s.cx.simulate_keystrokes("g");
    s.draw();
    assert!(!shown(&s, &editor));
}
