//! Keyboard input for a [`TerminalView`]: which keystrokes the view encodes
//! for the pty and which it leaves to the input handler or its parents, the
//! input handler that takes typed and composed text, and copy and paste.

use std::ops::Range;

use fe_terminal::keys::{self, Key, Mods};
use gpui::{
    Bounds, ClipboardItem, Context, EntityInputHandler, KeyDownEvent, Keystroke, Pixels,
    UTF16Selection, Window, point, size,
};

use crate::TerminalView;
use crate::element::cell_size;

/// Option is not meta on macOS: option-letter composes a character, as
/// Terminal.app's default does.
const OPTION_COMPOSES: bool = cfg!(target_os = "macos");

/// What a key press does in a terminal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum KeyInput {
    /// Copies the selection to the clipboard.
    Copy,
    /// Pastes the clipboard's text.
    Paste,
    /// Writes this key's encoding in the current modes.
    Key(Key, Mods),
    /// Writes these bytes.
    Bytes(&'static [u8]),
    /// Not the terminal's: printable text the input handler takes, so input
    /// methods and dead keys work, or an app command, which propagates.
    Pass,
}

/// The platform's copy and paste keys: cmd-c and cmd-v on macOS,
/// ctrl-shift-c and ctrl-shift-v elsewhere.
fn clipboard_key(keystroke: &Keystroke) -> Option<KeyInput> {
    let m = &keystroke.modifiers;
    let held = if cfg!(target_os = "macos") {
        m.platform && !m.control && !m.alt && !m.shift
    } else {
        m.control && m.shift && !m.alt && !m.platform
    };
    if !held || m.function {
        return None;
    }
    match keystroke.key.as_str() {
        "c" => Some(KeyInput::Copy),
        "v" => Some(KeyInput::Paste),
        _ => None,
    }
}

/// What `keystroke` does in a terminal.
pub(crate) fn key_input(keystroke: &Keystroke) -> KeyInput {
    if let Some(input) = clipboard_key(keystroke) {
        return input;
    }
    let m = &keystroke.modifiers;
    // The platform's modifier is the app's: the palette, closing a pane.
    if m.platform {
        return KeyInput::Pass;
    }
    let mods = Mods {
        shift: m.shift,
        alt: m.alt,
        ctrl: m.control,
    };
    let key = match keystroke.key.as_str() {
        "enter" => Key::Enter,
        "tab" => Key::Tab,
        "backspace" => Key::Backspace,
        "escape" => Key::Escape,
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "insert" => Key::Insert,
        "delete" => Key::Delete,
        name => {
            if let Some(n) = name.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
                return match n {
                    1..=12 => KeyInput::Key(Key::F(n), mods),
                    _ => KeyInput::Pass,
                };
            }
            return char_input(
                keystroke,
                Mods {
                    alt: m.alt && !OPTION_COMPOSES,
                    ..mods
                },
            );
        }
    };
    // Option-arrow moves by word and option-backspace deletes one, as in
    // Terminal.app, whose shell bindings expect meta.
    if OPTION_COMPOSES && m.alt && !m.control {
        match key {
            Key::Left => return KeyInput::Bytes(b"\x1bb"),
            Key::Right => return KeyInput::Bytes(b"\x1bf"),
            Key::Backspace => return KeyInput::Bytes(b"\x1b\x7f"),
            _ => {}
        }
    }
    KeyInput::Key(key, mods)
}

/// A character key: encoded with ctrl or alt (where alt is meta), and
/// otherwise the input handler's.
fn char_input(keystroke: &Keystroke, mods: Mods) -> KeyInput {
    if !mods.ctrl && !mods.alt {
        return KeyInput::Pass;
    }
    let c = match keystroke.key.as_str() {
        "space" => ' ',
        key => {
            let mut chars = key.chars();
            let (Some(c), None) = (chars.next(), chars.next()) else {
                return KeyInput::Pass;
            };
            // With alt alone the platform's shifted character is the one
            // sent; ctrl's codes don't care about case.
            let typed = keystroke.key_char.as_deref().filter(|_| !mods.ctrl);
            match typed
                .map(|typed| typed.chars().collect::<Vec<_>>())
                .as_deref()
            {
                Some(&[typed]) => typed,
                _ if mods.shift => c.to_uppercase().next().unwrap_or(c),
                _ => c,
            }
        }
    };
    KeyInput::Key(Key::Char(c), mods)
}

/// `range`, in UTF-16 code units of `text`, as a byte range, clamped to it.
fn byte_range(text: &str, range: Range<usize>) -> Range<usize> {
    let mut units = 0;
    let mut start = text.len();
    let mut end = text.len();
    for (i, c) in text.char_indices() {
        if units >= range.start && start == text.len() {
            start = i;
        }
        if units >= range.end {
            end = i;
            break;
        }
        units += c.len_utf16();
    }
    start..end.max(start)
}

/// `marked` with `range` (UTF-16 units; all of it when `None`) replaced by
/// `new_text`, and the selection (relative to `new_text`; a caret at its end
/// when `None`) in the result's units.
fn compose(
    marked: &str,
    range: Option<Range<usize>>,
    new_text: &str,
    new_selected: Option<Range<usize>>,
) -> (String, Range<usize>) {
    let range = range.unwrap_or(0..utf16_len(marked));
    let bytes = byte_range(marked, range);
    let start = utf16_len(&marked[..bytes.start]);
    let mut text = String::with_capacity(marked.len() + new_text.len());
    text.push_str(&marked[..bytes.start]);
    text.push_str(new_text);
    text.push_str(&marked[bytes.end..]);
    let inserted = utf16_len(new_text);
    let selected = new_selected.unwrap_or(inserted..inserted);
    let total = utf16_len(&text);
    let from = (start + selected.start.min(inserted)).min(total);
    let to = (start + selected.end.min(inserted)).min(total).max(from);
    (text, from..to)
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

impl TerminalView {
    pub(crate) fn key_down(
        &mut self,
        event: &KeyDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(terminal) = &self.terminal else {
            return;
        };
        match key_input(&event.keystroke) {
            KeyInput::Copy => {
                // Nothing selected: the key isn't the terminal's.
                let Some(text) = terminal.selected_text() else {
                    return;
                };
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
            KeyInput::Paste => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    self.paste_text(&text, cx);
                }
            }
            KeyInput::Key(key, mods) => {
                let modes = terminal.snapshot().modes;
                let Some(bytes) = keys::encode(key, mods, &modes) else {
                    return;
                };
                self.marked = None;
                self.write(bytes, cx);
            }
            KeyInput::Bytes(bytes) => {
                self.marked = None;
                self.write(bytes.to_vec(), cx);
            }
            KeyInput::Pass => return,
        }
        cx.stop_propagation();
    }

    /// Writes `text` as a paste, bracketed when the program asked for that.
    fn paste_text(&mut self, text: &str, cx: &mut Context<Self>) {
        let Some(terminal) = &self.terminal else {
            return;
        };
        let bracketed = terminal.snapshot().modes.bracketed_paste;
        let bytes = keys::encode_paste(text, bracketed);
        // A paste cancels the composition it lands in, so a later commit
        // can't send stale text after it.
        if !bytes.is_empty() && self.marked.take().is_some() {
            cx.notify();
        }
        self.write(bytes, cx);
    }

    /// Writes typed or pasted `bytes` to the pty, which scrolls to the
    /// bottom, and clears the selection.
    pub(crate) fn write(&mut self, bytes: Vec<u8>, cx: &mut Context<Self>) {
        let Some(terminal) = &self.terminal else {
            return;
        };
        if bytes.is_empty() {
            return;
        }
        terminal.clear_selection();
        terminal.input(bytes);
        cx.notify();
    }

    /// The text an input method is composing, painted at the cursor until
    /// it commits.
    pub fn marked_text(&self) -> Option<&str> {
        self.marked.as_deref()
    }
}

/// The view's text input. The terminal has no text of its own to edit: the
/// only document an input method sees is the text it is composing.
impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let marked = self.marked.as_deref()?;
        let bytes = byte_range(marked, range);
        let text = &marked[bytes.clone()];
        let start = utf16_len(&marked[..bytes.start]);
        *adjusted_range = Some(start..start + utf16_len(text));
        Some(text.to_owned())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let range = match self.marked.as_deref() {
            Some(_) => self.marked_selection.clone(),
            None => 0..0,
        };
        Some(UTF16Selection {
            range,
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_deref().map(|text| 0..utf16_len(text))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if self.marked.take().is_some() {
            cx.notify();
        }
    }

    fn paste(&mut self, item: ClipboardItem, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = item.text() {
            self.paste_text(&text, cx);
        }
    }

    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.marked.take().is_some() {
            cx.notify();
        }
        self.write(text.as_bytes().to_vec(), cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        new_selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (text, selection) = compose(
            self.marked.as_deref().unwrap_or(""),
            range,
            new_text,
            new_selected,
        );
        if !text.is_empty() {
            // Composing is input: it shows the cursor at the bottom, with
            // nothing selected, without writing the preedit.
            if let Some(terminal) = &self.terminal {
                terminal.scroll_to_bottom();
                terminal.clear_selection();
            }
        }
        self.marked = (!text.is_empty()).then_some(text);
        self.marked_selection = selection;
        cx.notify();
    }

    /// The cursor's cell, so the input method's window sits by it.
    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        element_bounds: Bounds<Pixels>,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let cursor = self.terminal.as_ref()?.snapshot().cursor;
        let cell = cell_size(&self.style, window);
        Some(Bounds::new(
            element_bounds.origin
                + point(
                    cell.width * cursor.col as f32,
                    cell.height * cursor.row as f32,
                ),
            size(cell.width, cell.height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        _: gpui::Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(source: &str) -> KeyInput {
        key_input(&Keystroke::parse(source).unwrap().with_simulated_ime())
    }

    #[test]
    fn printable_keys_are_the_input_handlers() {
        assert_eq!(input("a"), KeyInput::Pass);
        assert_eq!(input("shift-a"), KeyInput::Pass);
        assert_eq!(input("space"), KeyInput::Pass);
        assert_eq!(input("cmd-p"), KeyInput::Pass);
    }

    #[test]
    fn named_and_control_keys_are_encoded() {
        assert_eq!(input("ctrl-c"), KeyInput::Key(Key::Char('c'), Mods::CTRL));
        assert_eq!(input("enter"), KeyInput::Key(Key::Enter, Mods::NONE));
        assert_eq!(input("shift-tab"), KeyInput::Key(Key::Tab, Mods::SHIFT));
        assert_eq!(input("f5"), KeyInput::Key(Key::F(5), Mods::NONE));
        assert_eq!(input("f20"), KeyInput::Pass);
        assert_eq!(
            input("ctrl-space"),
            KeyInput::Key(Key::Char(' '), Mods::CTRL)
        );
    }

    #[test]
    fn option_is_meta_only_off_macos() {
        if OPTION_COMPOSES {
            assert_eq!(input("alt-left"), KeyInput::Bytes(b"\x1bb"));
            assert_eq!(input("alt-right"), KeyInput::Bytes(b"\x1bf"));
            assert_eq!(input("alt-backspace"), KeyInput::Bytes(b"\x1b\x7f"));
            assert_eq!(input("alt-x"), KeyInput::Pass);
        } else {
            assert_eq!(input("alt-left"), KeyInput::Key(Key::Left, Mods::ALT));
            assert_eq!(input("alt-x"), KeyInput::Key(Key::Char('x'), Mods::ALT));
            assert_eq!(
                input("alt-shift-x"),
                KeyInput::Key(
                    Key::Char('X'),
                    Mods {
                        shift: true,
                        alt: true,
                        ctrl: false
                    }
                )
            );
        }
    }

    #[test]
    fn clipboard_keys() {
        let (copy, paste) = if cfg!(target_os = "macos") {
            ("cmd-c", "cmd-v")
        } else {
            ("ctrl-shift-c", "ctrl-shift-v")
        };
        assert_eq!(input(copy), KeyInput::Copy);
        assert_eq!(input(paste), KeyInput::Paste);
    }

    #[test]
    fn composition_replaces_ranges_and_keeps_the_selection() {
        let compose = |marked, range, new: &str, sel| compose(marked, range, new, sel);
        assert_eq!(compose("ab", Some(1..2), "c", None), ("ac".into(), 2..2));
        assert_eq!(compose("ab", None, "xyz", Some(1..2)), ("xyz".into(), 1..2));
        assert_eq!(
            compose("ab", Some(1..1), "xy", Some(0..1)),
            ("axyb".into(), 1..2)
        );
        // A supplementary character is two units.
        assert_eq!(compose("a😀b", Some(1..3), "c", None), ("acb".into(), 2..2));
        assert_eq!(
            compose("a😀b", Some(3..4), "😀😀", Some(2..4)),
            ("a😀😀😀".into(), 5..7)
        );
        assert_eq!(compose("", Some(4..9), "x", Some(5..9)), ("x".into(), 1..1));
    }

    #[test]
    fn utf16_ranges() {
        assert_eq!(byte_range("héllo", 1..2), 1..3);
        assert_eq!(byte_range("日本", 0..9), 0..6);
        assert_eq!(byte_range("a😀b", 1..3), 1..5);
        assert_eq!(byte_range("ab", 5..9), 2..2);
    }
}
