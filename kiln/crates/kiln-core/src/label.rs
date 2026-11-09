// SPDX-License-Identifier: Apache-2.0

//! Key sequences as menus and tooltips print them: in macOS symbols, such
//! as `⇧⌘F`, or spelled out elsewhere, such as `Ctrl+Shift+F`.

use kiln_l10n::tr;

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
        Key::Space => {
            label.push_str(&tr!("kiln-core-key-space"));
            return label;
        }
        Key::Click => {
            label.push_str(&tr!("kiln-core-key-click"));
            return label;
        }
        Key::PageUp => "⇞",
        Key::PageDown => "⇟",
        Key::Home => "↖",
        Key::End => "↘",
    };
    label.push_str(symbol);
    label
}

/// `keys` spelled out, as Linux and Windows print them: each keystroke's
/// modifiers in the order Ctrl, Alt, Shift, Super (`cmd`), then its key
/// (letters uppercase, named keys by name), joined by `+`; the keystrokes
/// of a chord separated by a space. The `fn` modifier isn't shown.
pub fn pc_label(keys: &KeySequence) -> String {
    let labels: Vec<String> = keys.keystrokes().iter().map(pc_keystroke_label).collect();
    labels.join(" ")
}

/// `keys` as this platform prints them: [`macos_label`] on macOS,
/// [`pc_label`] elsewhere.
pub fn shortcut_label(keys: &KeySequence) -> String {
    if crate::Os::current() == crate::Os::MacOs {
        macos_label(keys)
    } else {
        pc_label(keys)
    }
}

fn pc_keystroke_label(keystroke: &Keystroke) -> String {
    let modifiers = keystroke.modifiers;
    let key = match keystroke.key {
        Key::Char(ch) => ch.to_uppercase().collect(),
        Key::F(n) => format!("F{n}"),
        Key::Left => tr!("kiln-core-key-left").to_string(),
        Key::Right => tr!("kiln-core-key-right").to_string(),
        Key::Up => tr!("kiln-core-key-up").to_string(),
        Key::Down => tr!("kiln-core-key-down").to_string(),
        Key::Enter => tr!("kiln-core-key-enter").to_string(),
        Key::Escape => tr!("kiln-core-key-escape").to_string(),
        Key::Backspace => tr!("kiln-core-key-backspace").to_string(),
        Key::Delete => tr!("kiln-core-key-delete").to_string(),
        Key::Tab => tr!("kiln-core-key-tab").to_string(),
        Key::Space => tr!("kiln-core-key-space").to_string(),
        Key::Click => tr!("kiln-core-key-click").to_string(),
        Key::PageUp => tr!("kiln-core-key-page-up").to_string(),
        Key::PageDown => tr!("kiln-core-key-page-down").to_string(),
        Key::Home => tr!("kiln-core-key-home").to_string(),
        Key::End => tr!("kiln-core-key-end").to_string(),
    };
    let mut parts = Vec::new();
    if modifiers.ctrl {
        parts.push(tr!("kiln-core-key-ctrl").to_string());
    }
    if modifiers.alt {
        parts.push(tr!("kiln-core-key-alt").to_string());
    }
    if modifiers.shift {
        parts.push(tr!("kiln-core-key-shift").to_string());
    }
    if modifiers.cmd {
        parts.push(tr!("kiln-core-key-super").to_string());
    }
    parts.push(key);
    parts.join("+")
}
