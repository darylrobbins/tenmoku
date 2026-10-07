//! Key sequences printed in macOS symbols.

use fe_keymap::{KeySequence, macos_label};

fn label(source: &str) -> String {
    macos_label(&KeySequence::parse(source).unwrap())
}

#[test]
fn macos_labels() {
    assert_eq!(label("cmd-shift-f"), "⇧⌘F");
    assert_eq!(label("ctrl-alt-cmd-left"), "⌃⌥⌘←");
    assert_eq!(label("cmd-k cmd-w"), "⌘K ⌘W");
    assert_eq!(label("alt-z"), "⌥Z");
    assert_eq!(label("f12"), "F12");
    assert_eq!(label("cmd-\\"), "⌘\\");
    let named = [
        ("enter", "↩"),
        ("escape", "⎋"),
        ("backspace", "⌫"),
        ("delete", "⌦"),
        ("tab", "⇥"),
        ("space", "Space"),
        ("pageup", "⇞"),
        ("pagedown", "⇟"),
        ("home", "↖"),
        ("end", "↘"),
    ];
    for (key, symbol) in named {
        assert_eq!(label(key), symbol, "{key}");
    }
    assert_eq!(label("right"), "→");
    assert_eq!(label("shift-up"), "⇧↑");
    assert_eq!(label("down"), "↓");
}
