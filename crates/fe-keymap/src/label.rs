//! Key sequences as macOS prints them in menus and tooltips, such as `⇧⌘F`.

use crate::{Key, KeySequence, Keystroke};

/// `keys` in macOS symbols: each keystroke's modifiers in the order ⌃⌥⇧⌘,
/// then its key (letters uppercase, named keys as their symbols, function
/// keys as `F1`), the keystrokes of a chord separated by a space. The `fn`
/// modifier isn't shown.
pub fn macos_label(keys: &KeySequence) -> String {
    let labels: Vec<String> = keys.keystrokes().iter().map(keystroke_label).collect();
    labels.join(" ")
}

fn keystroke_label(keystroke: &Keystroke) -> String {
    let modifiers = keystroke.modifiers;
    let mut label: String = [
        ('⌃', modifiers.ctrl),
        ('⌥', modifiers.alt),
        ('⇧', modifiers.shift),
        ('⌘', modifiers.cmd),
    ]
    .into_iter()
    .filter_map(|(symbol, held)| held.then_some(symbol))
    .collect();
    let symbol = match keystroke.key {
        Key::Char(ch) => {
            label.extend(ch.to_uppercase());
            return label;
        }
        Key::F(n) => {
            label.push_str(&format!("F{n}"));
            return label;
        }
        Key::Left => "←",
        Key::Right => "→",
        Key::Up => "↑",
        Key::Down => "↓",
        Key::Enter => "↩",
        Key::Escape => "⎋",
        Key::Backspace => "⌫",
        Key::Delete => "⌦",
        Key::Tab => "⇥",
        Key::Space => "Space",
        Key::PageUp => "⇞",
        Key::PageDown => "⇟",
        Key::Home => "↖",
        Key::End => "↘",
    };
    label.push_str(symbol);
    label
}
