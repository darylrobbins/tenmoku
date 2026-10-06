//! Keystrokes such as `cmd-shift-left`, and sequences of them such as
//! `cmd-k cmd-0`.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// The modifiers held for a keystroke.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modifiers {
    pub cmd: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// The `fn` key.
    pub function: bool,
}

impl Modifiers {
    /// The modifiers' names and flags, in the order they print in.
    fn named(&self) -> [(&'static str, bool); 5] {
        [
            ("cmd", self.cmd),
            ("ctrl", self.ctrl),
            ("alt", self.alt),
            ("shift", self.shift),
            ("fn", self.function),
        ]
    }

    fn flag(&mut self, name: &str) -> Option<&mut bool> {
        match name {
            "cmd" => Some(&mut self.cmd),
            "ctrl" => Some(&mut self.ctrl),
            "alt" => Some(&mut self.alt),
            "shift" => Some(&mut self.shift),
            "fn" => Some(&mut self.function),
            _ => None,
        }
    }
}

/// A key, without its modifiers.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    /// A printable character other than a space, such as `a`, `-` or `[`.
    /// Letters are lowercase.
    Char(char),
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Backspace,
    Delete,
    Enter,
    Tab,
    Escape,
    Space,
    /// `f1` to `f20`.
    F(u8),
}

const NAMED: [(&str, Key); 14] = [
    ("left", Key::Left),
    ("right", Key::Right),
    ("up", Key::Up),
    ("down", Key::Down),
    ("home", Key::Home),
    ("end", Key::End),
    ("pageup", Key::PageUp),
    ("pagedown", Key::PageDown),
    ("backspace", Key::Backspace),
    ("delete", Key::Delete),
    ("enter", Key::Enter),
    ("tab", Key::Tab),
    ("escape", Key::Escape),
    ("space", Key::Space),
];

impl Key {
    /// Parses a key name, ignoring case: see [`Keystroke::parse`].
    pub fn parse(name: &str) -> Option<Self> {
        let mut chars = name.chars();
        if let (Some(ch), None) = (chars.next(), chars.next()) {
            return printable(ch).then(|| Key::Char(lowercase(ch)));
        }
        let name = name.to_ascii_lowercase();
        if let Some((_, key)) = NAMED.iter().find(|(named, _)| *named == name) {
            return Some(*key);
        }
        let number = name.strip_prefix('f')?;
        // No sign or leading zero, as in `f+1` or `f01`.
        if number.starts_with('0') || !number.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        number
            .parse()
            .ok()
            .filter(|n| (1..=20).contains(n))
            .map(Key::F)
    }
}

/// Whether `ch` names a key by itself: printable and not whitespace, which
/// has names (`space`, `tab`, `enter`).
fn printable(ch: char) -> bool {
    !ch.is_control() && !ch.is_whitespace()
}

/// `ch` in lowercase, if that's one character.
fn lowercase(ch: char) -> char {
    let mut lower = ch.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(lower), None) => lower,
        _ => ch,
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::Char(ch) => write!(f, "{ch}"),
            Key::F(n) => write!(f, "f{n}"),
            key => {
                let (name, _) = NAMED
                    .iter()
                    .find(|(_, named)| named == key)
                    .expect("every other key is named");
                f.write_str(name)
            }
        }
    }
}

/// A key with modifiers, such as `cmd-shift-left`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Keystroke {
    pub modifiers: Modifiers,
    pub key: Key,
}

/// Why a keystroke or key sequence didn't parse.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct KeystrokeError {
    /// Byte offset into the source of the offending part.
    pub offset: usize,
    pub message: String,
}

impl Keystroke {
    pub fn new(modifiers: Modifiers, key: Key) -> Self {
        Self { modifiers, key }
    }

    /// Parses `-`-separated modifiers (`cmd`, `ctrl`, `alt`, `shift`, `fn`,
    /// in any order) then a key: a printable character, including `-` as in
    /// `cmd--`; `left`, `right`, `up`, `down`, `home`, `end`, `pageup`,
    /// `pagedown`, `backspace`, `delete`, `enter`, `tab`, `escape` or
    /// `space`; or `f1` to `f20`. Case doesn't matter.
    pub fn parse(source: &str) -> Result<Self, KeystrokeError> {
        let error = |offset, message: String| KeystrokeError { offset, message };
        // The key is after the last `-`, unless that `-` is the key.
        let (modifiers, key) = if source == "-" {
            ("", source)
        } else if let Some(modifiers) = source.strip_suffix("--") {
            (modifiers, "-")
        } else {
            source.rsplit_once('-').unwrap_or(("", source))
        };
        let key_offset = source.len() - key.len();
        let mut parsed = Modifiers::default();
        if key_offset > 0 {
            let mut offset = 0;
            for name in modifiers.split('-') {
                let flag = match parsed.flag(&name.to_ascii_lowercase()) {
                    Some(flag) => flag,
                    None if name.is_empty() => {
                        return Err(error(offset, "expected a modifier".into()));
                    }
                    None => {
                        let message = format!(
                            "unknown modifier {name:?}: expected cmd, ctrl, alt, shift or fn"
                        );
                        return Err(error(offset, message));
                    }
                };
                if *flag {
                    return Err(error(offset, format!("{name:?} is repeated")));
                }
                *flag = true;
                offset += name.len() + 1;
            }
        }
        if key.is_empty() {
            return Err(error(key_offset, "expected a key".into()));
        }
        let key =
            Key::parse(key).ok_or_else(|| error(key_offset, format!("unknown key {key:?}")))?;
        Ok(Self {
            modifiers: parsed,
            key,
        })
    }
}

/// Prints the normalised form: lowercase, with the modifiers in the order
/// `cmd`, `ctrl`, `alt`, `shift`, `fn`. It parses back to an equal keystroke.
impl fmt::Display for Keystroke {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (name, held) in self.modifiers.named() {
            if held {
                write!(f, "{name}-")?;
            }
        }
        write!(f, "{}", self.key)
    }
}

impl FromStr for Keystroke {
    type Err = KeystrokeError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        Self::parse(source)
    }
}

impl TryFrom<String> for Keystroke {
    type Error = KeystrokeError;

    fn try_from(source: String) -> Result<Self, Self::Error> {
        Self::parse(&source)
    }
}

impl From<Keystroke> for String {
    fn from(keystroke: Keystroke) -> Self {
        keystroke.to_string()
    }
}

/// Keystrokes pressed one after another, such as `cmd-k cmd-0`. Never
/// empty.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct KeySequence(Vec<Keystroke>);

impl KeySequence {
    /// Parses whitespace-separated keystrokes: see [`Keystroke::parse`].
    pub fn parse(source: &str) -> Result<Self, KeystrokeError> {
        let mut keystrokes = Vec::new();
        let mut rest = source;
        loop {
            rest = rest.trim_start();
            if rest.is_empty() {
                break;
            }
            let offset = source.len() - rest.len();
            let len = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let keystroke = Keystroke::parse(&rest[..len]).map_err(|error| KeystrokeError {
                offset: offset + error.offset,
                ..error
            })?;
            keystrokes.push(keystroke);
            rest = &rest[len..];
        }
        if keystrokes.is_empty() {
            return Err(KeystrokeError {
                offset: 0,
                message: "expected a keystroke".into(),
            });
        }
        Ok(Self(keystrokes))
    }

    pub fn keystrokes(&self) -> &[Keystroke] {
        &self.0
    }
}

impl From<Keystroke> for KeySequence {
    fn from(keystroke: Keystroke) -> Self {
        Self(vec![keystroke])
    }
}

/// The keystrokes' normalised forms, separated by spaces.
impl fmt::Display for KeySequence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (ix, keystroke) in self.0.iter().enumerate() {
            if ix > 0 {
                f.write_str(" ")?;
            }
            write!(f, "{keystroke}")?;
        }
        Ok(())
    }
}

impl FromStr for KeySequence {
    type Err = KeystrokeError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        Self::parse(source)
    }
}

impl TryFrom<String> for KeySequence {
    type Error = KeystrokeError;

    fn try_from(source: String) -> Result<Self, Self::Error> {
        Self::parse(&source)
    }
}

impl From<KeySequence> for String {
    fn from(keys: KeySequence) -> Self {
        keys.to_string()
    }
}
