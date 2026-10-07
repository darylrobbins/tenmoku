//! Vim's registers, by `:help registers`. One [`Registers`] is shared by
//! every editor in a host, and non-vim commands may reach it too.

use fe_editor::commands::EditorHost;

/// A register's text, and whether it holds whole lines (its text then ends
/// in `\n`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Register {
    pub text: String,
    pub linewise: bool,
}

impl Register {
    pub fn new(text: impl Into<String>, linewise: bool) -> Self {
        Self {
            text: text.into(),
            linewise,
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
/// clipboard). `A-Z` append to `a-z`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Registers {
    numbered: [Option<Register>; 10],
    small: Option<Register>,
    named: [Option<Register>; 26],
    /// The register `"` reads: the last one written, `+` for the clipboard.
    unnamed: Option<char>,
}

/// The clipboard's name in [`Registers::unnamed`].
const CLIPBOARD: char = '+';

impl Registers {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether `name` is a register: `" 0-9 - a-z A-Z _ + *`.
    pub fn check(name: char) -> Result<(), RegisterError> {
        match name {
            '"' | '-' | '_' | '+' | '*' => Ok(()),
            _ if name.is_ascii_alphanumeric() => Ok(()),
            _ => Err(RegisterError(name)),
        }
    }

    /// A register's content, `"` following the last write and `A-Z` reading
    /// `a-z`. `None` if it's empty, or for `_ + *` (and `"` pointing at the
    /// clipboard), which only [`Registers::read`] reaches.
    pub fn get(&self, name: char) -> Option<&Register> {
        match name {
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
    /// holds lines when its text ends in a line break; its `\r\n` and `\r`
    /// read as `\n`.
    pub fn read(
        &self,
        name: Option<char>,
        host: &mut dyn EditorHost,
    ) -> Result<Option<Register>, RegisterError> {
        let name = name.unwrap_or('"');
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

/// `old` with `new` appended: whole lines if either is.
fn append(old: Register, new: Register) -> Register {
    if !old.linewise && !new.linewise {
        return Register::new(old.text + &new.text, false);
    }
    let line = |mut text: String| {
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text
    };
    Register::new(line(old.text) + &line(new.text), true)
}
