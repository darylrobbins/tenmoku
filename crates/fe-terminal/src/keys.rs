//! xterm key and paste encoding: what a key press sends to the pty.
//!
//! Sequences follow xterm's defaults (`modifyCursorKeys` 2, `modifyFunctionKeys`
//! 2): a modified cursor, editing or function key carries its modifiers as a
//! `;{1 + mods}` parameter, an unmodified cursor key is `SS3` in application
//! cursor mode, and Alt on a character key prefixes ESC.

use crate::Modes;

const ESC: u8 = 0x1b;

/// A key the terminal encodes. Characters arrive already shifted (`'A'`, not
/// shift-`'a'`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Char(char),
    Enter,
    Tab,
    Backspace,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    /// F1 to F12.
    F(u8),
}

/// The modifiers held with a key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Mods {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

impl Mods {
    pub const NONE: Mods = Mods {
        shift: false,
        alt: false,
        ctrl: false,
    };
    pub const SHIFT: Mods = Mods {
        shift: true,
        alt: false,
        ctrl: false,
    };
    pub const ALT: Mods = Mods {
        shift: false,
        alt: true,
        ctrl: false,
    };
    pub const CTRL: Mods = Mods {
        shift: false,
        alt: false,
        ctrl: true,
    };

    fn is_empty(self) -> bool {
        self == Mods::NONE
    }

    /// xterm's modifier parameter: 1 plus shift 1, alt 2, ctrl 4.
    fn param(self) -> u8 {
        1 + u8::from(self.shift) + 2 * u8::from(self.alt) + 4 * u8::from(self.ctrl)
    }
}

/// The bytes `key` with `mods` sends, or `None` for a key xterm sends nothing
/// for (F13 and up).
pub fn encode(key: Key, mods: Mods, modes: &Modes) -> Option<Vec<u8>> {
    let alt_prefixed = |bytes: &[u8]| {
        let mut out = Vec::with_capacity(bytes.len() + 1);
        if mods.alt {
            out.push(ESC);
        }
        out.extend_from_slice(bytes);
        out
    };
    let bytes = match key {
        Key::Char(c) => {
            let control = if mods.ctrl { control_code(c) } else { None };
            match control {
                Some(code) => alt_prefixed(&[code]),
                None => alt_prefixed(c.encode_utf8(&mut [0; 4]).as_bytes()),
            }
        }
        Key::Enter => alt_prefixed(b"\r"),
        Key::Tab if mods.shift => b"\x1b[Z".to_vec(),
        Key::Tab => alt_prefixed(b"\t"),
        Key::Backspace if mods.ctrl => alt_prefixed(&[0x08]),
        Key::Backspace => alt_prefixed(&[0x7f]),
        Key::Escape => alt_prefixed(&[ESC]),
        Key::Up => cursor_key(b'A', mods, modes),
        Key::Down => cursor_key(b'B', mods, modes),
        Key::Right => cursor_key(b'C', mods, modes),
        Key::Left => cursor_key(b'D', mods, modes),
        Key::Home => cursor_key(b'H', mods, modes),
        Key::End => cursor_key(b'F', mods, modes),
        Key::Insert => tilde_key(2, mods),
        Key::Delete => tilde_key(3, mods),
        Key::PageUp => tilde_key(5, mods),
        Key::PageDown => tilde_key(6, mods),
        Key::F(n @ 1..=4) => {
            let letter = b'P' + (n - 1);
            if mods.is_empty() {
                vec![ESC, b'O', letter]
            } else {
                format!("\x1b[1;{}{}", mods.param(), letter as char).into_bytes()
            }
        }
        Key::F(n @ 5..=12) => {
            const CODES: [u8; 8] = [15, 17, 18, 19, 20, 21, 23, 24];
            tilde_key(CODES[usize::from(n - 5)], mods)
        }
        Key::F(_) => return None,
    };
    Some(bytes)
}

/// The control code Ctrl turns `c` into: letters to 0x01–0x1a, space and `@`
/// to NUL, `[ \ ] ^ _` to 0x1b–0x1f, and xterm's digit aliases.
fn control_code(c: char) -> Option<u8> {
    let c = c.to_ascii_lowercase();
    Some(match c {
        'a'..='z' => c as u8 - b'a' + 1,
        ' ' | '@' | '2' => 0x00,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        '_' | '/' | '7' => 0x1f,
        '8' | '?' => 0x7f,
        _ => return None,
    })
}

/// An arrow, Home or End: `SS3 X` in application cursor mode, `CSI X`
/// otherwise, and `CSI 1;{mods} X` when modified.
fn cursor_key(letter: u8, mods: Mods, modes: &Modes) -> Vec<u8> {
    if !mods.is_empty() {
        format!("\x1b[1;{}{}", mods.param(), letter as char).into_bytes()
    } else if modes.app_cursor {
        vec![ESC, b'O', letter]
    } else {
        vec![ESC, b'[', letter]
    }
}

/// `CSI n ~`, or `CSI n;{mods} ~` when modified.
fn tilde_key(n: u8, mods: Mods) -> Vec<u8> {
    if mods.is_empty() {
        format!("\x1b[{n}~").into_bytes()
    } else {
        format!("\x1b[{n};{}~", mods.param()).into_bytes()
    }
}

const PASTE_START: &str = "\x1b[200~";
const PASTE_END: &str = "\x1b[201~";

/// The bytes pasting `text` sends. With bracketed paste on it is wrapped in
/// `ESC[200~`…`ESC[201~`, and any end marker inside it is removed so the paste
/// can't end itself early; otherwise line breaks become `\r`, as typed.
pub fn encode_paste(text: &str, bracketed: bool) -> Vec<u8> {
    if bracketed {
        let mut inner = text.to_owned();
        // Removing one marker can join the pieces of another.
        while inner.contains(PASTE_END) {
            inner = inner.replace(PASTE_END, "");
        }
        format!("{PASTE_START}{inner}{PASTE_END}").into_bytes()
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}
