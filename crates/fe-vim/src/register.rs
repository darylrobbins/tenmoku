//! Vim's registers, by `:help registers`. One [`Registers`] is shared by
//! every editor in a host, and non-vim commands may reach it too.

use fe_editor::commands::EditorHost;

use crate::substitute::LastSubstitute;

/// A register's text, and whether it holds whole lines (its text then ends
/// in `\n`) or a block.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Register {
    pub text: String,
    pub linewise: bool,
    /// A block's width in display columns (nvim's `getregtype()` is
    /// `<C-v>{width}`): its text is its rows joined by `\n`.
    pub block_width: Option<usize>,
}

impl Register {
    pub fn new(text: impl Into<String>, linewise: bool) -> Self {
        Self {
            text: text.into(),
            linewise,
            block_width: None,
        }
    }

    /// A block `width` columns wide, of `rows`.
    pub fn block(rows: &[String], width: usize) -> Self {
        Self {
            text: rows.join("\n"),
            linewise: false,
            block_width: Some(width),
        }
    }
}

/// What writes a register.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Write {
    Yank,
    /// A delete or change. `small` if it was charwise within one row: it
    /// writes `-`, unless a register was named. `numbered` if it shifts `1`
    /// to `2` … `9` and writes `1`, named register or not: a linewise or
    /// multi-row delete, or one over `%` (even within a row).
    Delete {
        small: bool,
        numbered: bool,
    },
}

/// A name that isn't a register.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} isn't a register")]
pub struct RegisterError(pub char);

/// `" 0 1-9 - a-z`, plus `_` (which discards) and `+ *` (one slot: the host's
/// clipboard). `A-Z` append to `a-z`. The read-only `/` holds the last
/// search pattern, which with its direction is the host's last search, as
/// every editor shares it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Registers {
    numbered: [Option<Register>; 10],
    small: Option<Register>,
    named: [Option<Register>; 26],
    /// The register `"` reads: the last one written, `+` for the clipboard.
    unnamed: Option<char>,
    /// `/`: the last search pattern, as characters.
    search: Option<Register>,
    /// Whether the last search went forward (`/`, not `?`).
    search_forward: bool,
    /// The last `:s`, which `:&` repeats; shared like the last search.
    pub(crate) substitute: LastSubstitute,
}

/// The clipboard's name in [`Registers::unnamed`].
const CLIPBOARD: char = '+';

impl Registers {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether `name` is a register a command may write: `" 0-9 - a-z A-Z _
    /// + *`. Not `/`, which only [`Registers::read`] and
    /// [`Registers::get`] reach.
    pub fn check(name: char) -> Result<(), RegisterError> {
        match name {
            '"' | '-' | '_' | '+' | '*' => Ok(()),
            _ if name.is_ascii_alphanumeric() => Ok(()),
            _ => Err(RegisterError(name)),
        }
    }

    /// A register's content, `"` following the last write and `A-Z` reading
    /// `a-z`, `/` the last search pattern. `None` if it's empty, or for `_ +
    /// *` (and `"` pointing at the clipboard), which only
    /// [`Registers::read`] reaches.
    pub fn get(&self, name: char) -> Option<&Register> {
        match name {
            '/' => self.search.as_ref(),
            '"' => self.get(self.unnamed.filter(|&name| name != CLIPBOARD)?),
            '-' => self.small.as_ref(),
            '0'..='9' => self.numbered[name as usize - '0' as usize].as_ref(),
            'a'..='z' | 'A'..='Z' => {
                let ix = name.to_ascii_lowercase() as usize - 'a' as usize;
                self.named[ix].as_ref()
            }
            _ => None,
        }
    }

    /// What a put of register `name` (`"` if `None`) inserts. The clipboard
    /// holds lines when its text ends in a line break, and never a block; its
    /// `\r\n` and `\r` read as `\n`. A block written to it is its rows
    /// joined by `\n`. `/` is the last search pattern, an error until a
    /// search sets it (nvim's E35).
    pub fn read(
        &self,
        name: Option<char>,
        host: &mut dyn EditorHost,
    ) -> Result<Option<Register>, RegisterError> {
        let name = name.unwrap_or('"');
        if name == '/' {
            return self.search.clone().map(Some).ok_or(RegisterError(name));
        }
        Self::check(name)?;
        let clipboard = match name {
            '+' | '*' => true,
            '"' => self.unnamed == Some(CLIPBOARD),
            _ => false,
        };
        if clipboard {
            let text = host.read_clipboard().unwrap_or_default();
            let text = text.replace("\r\n", "\n").replace('\r', "\n");
            let linewise = text.ends_with('\n');
            return Ok(Some(Register::new(text, linewise)));
        }
        Ok(self.get(name).cloned())
    }

    /// Stores `value` as `write` does, into `name` if one was given (`"`
    /// names `0`, as `:help quote_quote` says). `_` writes nothing and
    /// leaves `"` alone.
    pub fn write(
        &mut self,
        name: Option<char>,
        write: Write,
        value: Register,
        host: &mut dyn EditorHost,
    ) -> Result<(), RegisterError> {
        if let Some(name) = name {
            Self::check(name)?;
        }
        let name = name.map(|name| if name == '"' { '0' } else { name });
        let unnamed = match name {
            Some('_') => return Ok(()),
            Some('+' | '*') => {
                host.write_clipboard(value.text.clone());
                Some(CLIPBOARD)
            }
            Some(name) => Some(self.store(name, value.clone())),
            None => None,
        };
        self.unnamed = match write {
            Write::Yank => {
                if name.is_none() {
                    self.numbered[0] = Some(value);
                }
                unnamed.or(Some('0'))
            }
            Write::Delete { small, numbered } => {
                if small && name.is_none() {
                    self.small = Some(value.clone());
                }
                if numbered {
                    self.numbered[1..].rotate_right(1);
                    self.numbered[1] = Some(value);
                }
                // A named digit or `-` was shifted along: `"` reads `1`.
                match unnamed {
                    Some(name) if !numbered || !matches!(name, '0'..='9' | '-') => Some(name),
                    _ if numbered => Some('1'),
                    _ => Some('-'),
                }
            }
        };
        Ok(())
    }

    /// The last search: its pattern (the `/` register) and whether it went
    /// forward.
    pub fn last_search(&self) -> Option<(&str, bool)> {
        let search = self.search.as_ref()?;
        Some((search.text.as_str(), self.search_forward))
    }

    /// Makes `pattern` the last search, as `/` (`forward`) or `?` does.
    pub(crate) fn set_search(&mut self, pattern: &str, forward: bool) {
        self.search = Some(Register::new(pattern, false));
        self.search_forward = forward;
    }

    /// Writes register `name` itself, `A-Z` appending; returns the name
    /// `"` then reads.
    fn store(&mut self, name: char, value: Register) -> char {
        match name {
            '-' => self.small = Some(value),
            '0'..='9' => self.numbered[name as usize - '0' as usize] = Some(value),
            _ => {
                let lower = name.to_ascii_lowercase();
                let slot = &mut self.named[lower as usize - 'a' as usize];
                *slot = Some(match (name.is_ascii_uppercase(), slot.take()) {
                    (true, Some(old)) => append(old, value),
                    (_, _) => value,
                });
                return lower;
            }
        }
        name
    }
}

/// `old` with `new` appended, as nvim appends: whole lines if either is; to
/// a block, `new`'s rows as more rows, the block keeping its width; to
/// characters, its text (a block's rows joined by `\n`).
fn append(old: Register, new: Register) -> Register {
    if let (Some(width), false) = (old.block_width, new.linewise) {
        let rows: Vec<String> = [old.text, new.text].into();
        return Register::block(&rows, width);
    }
    if !old.linewise && !new.linewise {
        return Register::new(old.text + &new.text, false);
    }
    // Each row becomes a line, a block's or characters' empty last one too.
    let line = |register: Register| {
        let mut text = register.text;
        if !register.linewise || !text.ends_with('\n') {
            text.push('\n');
        }
        text
    };
    Register::new(line(old) + &line(new), true)
}
