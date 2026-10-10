//! Key sequences as menus and tooltips print them: in macOS symbols, such
//! as `⇧⌘F`, or spelled out elsewhere, such as `Ctrl+Shift+F`.

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
        Key::Click => "Click",
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
    if cfg!(target_os = "macos") {
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
        Key::Left => "Left".into(),
        Key::Right => "Right".into(),
        Key::Up => "Up".into(),
        Key::Down => "Down".into(),
        Key::Enter => "Enter".into(),
        Key::Escape => "Esc".into(),
        Key::Backspace => "Backspace".into(),
        Key::Delete => "Delete".into(),
        Key::Tab => "Tab".into(),
        Key::Space => "Space".into(),
        Key::Click => "Click".into(),
        Key::PageUp => "PageUp".into(),
        Key::PageDown => "PageDown".into(),
        Key::Home => "Home".into(),
        Key::End => "End".into(),
    };
    [
        ("Ctrl", modifiers.ctrl),
        ("Alt", modifiers.alt),
        ("Shift", modifiers.shift),
        ("Super", modifiers.cmd),
    ]
    .into_iter()
    .filter_map(|(name, held)| held.then_some(name.to_owned()))
    .chain(std::iter::once(key))
    .collect::<Vec<_>>()
    .join("+")
}
