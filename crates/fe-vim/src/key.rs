use kiln_l10n::tr;
/// A key as the reducer reads it. Unlike `fe_keymap::Keystroke`, it keeps
/// what was typed: `Char('A')`, `Char('$')` and `Char('é')` are distinct.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum VimKey {
    Char(char),
    /// Control plus a lowercase letter, as in `<C-r>`.
    Ctrl(char),
    Esc,
    Enter,
    Backspace,
    Tab,
    Left,
    Right,
    Up,
    Down,
}

/// A `<name>` that names no key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyError(pub String);

impl std::error::Error for KeyError {}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&tr!("vim-unknown-key", key = &self.0))
    }
}

/// Reads vim key notation: plain characters, plus `<Esc>`, `<CR>`, `<BS>`,
/// `<Tab>`, `<C-x>`, `<lt>`, `<Space>`, `<Bar>`, `<Bslash>` and the arrows `<Left>` `<Right>` `<Up>`
/// `<Down>`, case-insensitive inside `<…>`. As in Vim, a `<` that doesn't
/// start a `<name>` (letters, digits and `-`) is itself, so `<<` is two
/// `<`s.
pub fn parse_keys(keys: &str) -> Result<Vec<VimKey>, KeyError> {
    let mut parsed = Vec::new();
    let mut rest = keys;
    while let Some(ch) = rest.chars().next() {
        let name = rest
            .strip_prefix('<')
            .and_then(|after| after.split_once('>'))
            .map(|(name, _)| name)
            .filter(|name| {
                !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            });
        match name {
            Some(name) => {
                parsed.push(named(name)?);
                rest = &rest[name.len() + 2..];
            }
            None => {
                parsed.push(VimKey::Char(ch));
                rest = &rest[ch.len_utf8()..];
            }
        }
    }
    Ok(parsed)
}

fn named(name: &str) -> Result<VimKey, KeyError> {
    let lower = name.to_ascii_lowercase();
    let key = match lower.as_str() {
        "esc" => VimKey::Esc,
        "cr" | "enter" | "return" => VimKey::Enter,
        "bs" => VimKey::Backspace,
        "tab" => VimKey::Tab,
        "lt" => VimKey::Char('<'),
        "space" => VimKey::Char(' '),
        "bar" => VimKey::Char('|'),
        "bslash" => VimKey::Char('\\'),
        "left" => VimKey::Left,
        "right" => VimKey::Right,
        "up" => VimKey::Up,
        "down" => VimKey::Down,
        _ => match lower.strip_prefix("c-").map(|rest| rest.as_bytes()) {
            Some(&[letter]) if letter.is_ascii_lowercase() => VimKey::Ctrl(letter as char),
            _ => return Err(KeyError(name.to_string())),
        },
    };
    Ok(key)
}
