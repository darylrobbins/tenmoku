//! A vimrc subset read into `Remap` records: the map, noremap, unmap and
//! mapclear families for n, x and o, `let mapleader` and `"` comments, each
//! line read as nvim reads it. Anything else is a `VimrcIssue` at its line,
//! never run. See `docs/decisions/vim-remaps.md` §1(c); nothing here does
//! I/O.

use crate::key::{VimKey, parse_keys};
use crate::remap::{Remap, RemapMode, RemapSet};

/// What a vimrc's text loads as.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Vimrc {
    pub remaps: Vec<Remap>,
    /// The 1-based line each of `remaps` was written on, in step with it.
    pub lines: Vec<usize>,
    pub issues: Vec<VimrcIssue>,
}

/// A problem with one line, which is skipped (or, for a note, loads anyway).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VimrcIssue {
    /// 1-based.
    pub line: usize,
    pub message: String,
}

const N: RemapMode = RemapMode::Normal;
const X: RemapMode = RemapMode::Visual;
const O: RemapMode = RemapMode::OperatorPending;
const I: RemapMode = RemapMode::Insert;
const C: RemapMode = RemapMode::CmdLine;
const NXO: &[RemapMode] = &[N, X, O];
const IC: &[RemapMode] = &[I, C];

const SELECT: &str = "Select mode isn't in fastedit";
const LANGUAGE: &str = "language remaps aren't supported";
const TERMINAL: &str = "terminal remaps aren't supported";

#[derive(Copy, Clone)]
enum Command {
    Map(&'static [RemapMode], bool),
    Unmap(&'static [RemapMode]),
    Mapclear(&'static [RemapMode]),
    /// `vmap` and `vnoremap`: Visual only, with a note.
    VisualSelect(bool),
    /// A mode fastedit has no remaps for, with why.
    Unsupported(&'static str),
}

/// Each command's full name, the shortest abbreviation nvim accepts, and
/// what it is.
const COMMANDS: &[(&str, usize, Command)] = &[
    ("map", 3, Command::Map(NXO, true)),
    ("noremap", 2, Command::Map(NXO, false)),
    ("nmap", 2, Command::Map(&[N], true)),
    ("nnoremap", 2, Command::Map(&[N], false)),
    ("xmap", 2, Command::Map(&[X], true)),
    ("xnoremap", 2, Command::Map(&[X], false)),
    ("omap", 2, Command::Map(&[O], true)),
    ("onoremap", 3, Command::Map(&[O], false)),
    ("vmap", 2, Command::VisualSelect(true)),
    ("vnoremap", 2, Command::VisualSelect(false)),
    ("unmap", 3, Command::Unmap(NXO)),
    ("nunmap", 3, Command::Unmap(&[N])),
    ("xunmap", 2, Command::Unmap(&[X])),
    ("ounmap", 2, Command::Unmap(&[O])),
    ("vunmap", 2, Command::Unmap(&[X])),
    ("mapclear", 4, Command::Mapclear(NXO)),
    ("nmapclear", 5, Command::Mapclear(&[N])),
    ("xmapclear", 5, Command::Mapclear(&[X])),
    ("omapclear", 5, Command::Mapclear(&[O])),
    ("vmapclear", 5, Command::Mapclear(&[X])),
    ("imap", 2, Command::Map(&[I], true)),
    ("inoremap", 3, Command::Map(&[I], false)),
    ("iunmap", 2, Command::Unmap(&[I])),
    ("imapclear", 5, Command::Mapclear(&[I])),
    ("cmap", 2, Command::Map(&[C], true)),
    ("cnoremap", 3, Command::Map(&[C], false)),
    ("cunmap", 2, Command::Unmap(&[C])),
    ("cmapclear", 5, Command::Mapclear(&[C])),
    ("smap", 4, Command::Unsupported(SELECT)),
    ("snoremap", 4, Command::Unsupported(SELECT)),
    ("sunmap", 4, Command::Unsupported(SELECT)),
    ("smapclear", 5, Command::Unsupported(SELECT)),
    ("lmap", 2, Command::Unsupported(LANGUAGE)),
    ("lnoremap", 2, Command::Unsupported(LANGUAGE)),
    ("lunmap", 2, Command::Unsupported(LANGUAGE)),
    ("lmapclear", 5, Command::Unsupported(LANGUAGE)),
    ("tmap", 3, Command::Unsupported(TERMINAL)),
    ("tnoremap", 3, Command::Unsupported(TERMINAL)),
    ("tunmap", 5, Command::Unsupported(TERMINAL)),
    ("tmapclear", 5, Command::Unsupported(TERMINAL)),
];

/// Reads `text` line by line. Each record is checked as `RemapSet::new`
/// checks it, and one it rejects is left out, with its issue at its line.
pub fn parse_vimrc(text: &str) -> Vimrc {
    parse_vimrc_with_leader(text, &[VimKey::Char('\\')])
}

/// [`parse_vimrc`], with `<leader>` starting as `leader` rather than `\`,
/// as when a preset sets `mapleader` before the vimrc runs; a `let
/// mapleader` changes it from there.
pub fn parse_vimrc_with_leader(text: &str, leader: &[VimKey]) -> Vimrc {
    let mut parser = Parser {
        leader: leader.iter().copied().map(notation).collect(),
        vimrc: Vimrc::default(),
    };
    for (index, line) in text.lines().enumerate() {
        parser.line(index + 1, line);
    }
    parser.vimrc
}

struct Parser {
    /// In vim notation.
    leader: String,
    vimrc: Vimrc,
}

impl Parser {
    fn line(&mut self, line: usize, text: &str) {
        if text.trim_start_matches([' ', '\t']).starts_with('\\') {
            self.issue(line, "line continuation isn't supported");
            return;
        }
        let mut rest = Some(text);
        while let Some(text) = rest {
            rest = self.command(line, text);
        }
    }

    /// Runs the command at the start of `text`; the text after the `|`
    /// that ends it, if any, is the next command.
    fn command<'a>(&mut self, line: usize, text: &'a str) -> Option<&'a str> {
        let text = text.trim_start_matches([' ', '\t', ':']);
        if text.is_empty() || text.starts_with('"') {
            return None;
        }
        let name_len = text
            .find(|c: char| !c.is_ascii_alphabetic())
            .unwrap_or(text.len());
        let name = &text[..name_len];
        let (bang, after) = match text[name_len..].strip_prefix('!') {
            Some(after) => (true, after),
            None => (false, &text[name_len..]),
        };
        if name == "let" && !bang {
            return self.let_(line, after);
        }
        let found = COMMANDS
            .iter()
            .find(|(full, min, _)| name.len() >= *min && full.starts_with(name));
        let Some(&(full, _, command)) = found else {
            let word = text.split([' ', '\t', '|']).next().unwrap_or(text);
            self.issue(line, format!("not supported: {word}"));
            return None;
        };
        // The mapclear family takes a trailing `"` comment; the map and
        // unmap families read `"` as part of their keys.
        let comments = full.ends_with("mapclear");
        let command = if bang {
            match bang_command(command) {
                Some(command) => command,
                None => {
                    self.issue(line, format!("not supported: {full}!"));
                    return None;
                }
            }
        } else {
            command
        };
        let (args, rest) = split_bar(after, comments);
        match command {
            Command::Map(modes, recursive) => {
                self.map(line, modes, recursive, &args);
            }
            Command::Unmap(modes) => self.unmap(line, modes, &args),
            Command::Mapclear(modes) => self.mapclear(line, modes, &args),
            Command::VisualSelect(recursive) => {
                if self.map(line, &[X], recursive, &args) {
                    self.issue(
                        line,
                        format!("{full} applies to Visual mode only: {SELECT}"),
                    );
                }
            }
            Command::Unsupported(why) => self.issue(line, why),
        }
        rest
    }

    /// Adds one record; false if the line gave an issue instead.
    fn map(&mut self, line: usize, modes: &[RemapMode], recursive: bool, args: &str) -> bool {
        let Some(args) = self.map_arguments(line, args) else {
            return false;
        };
        let from_len = args.find([' ', '\t']).unwrap_or(args.len());
        let to = args[from_len..].trim_start_matches([' ', '\t']);
        if to.is_empty() {
            self.issue(line, "listing maps isn't supported");
            return false;
        }
        let remap = Remap {
            modes: modes.to_vec(),
            from: with_leader(&args[..from_len], &self.leader),
            to: with_leader(to, &self.leader),
            recursive,
        };
        let (_, issues) = RemapSet::new(std::slice::from_ref(&remap), "\\");
        if let Some(issue) = issues.into_iter().next() {
            self.issue(line, issue.message);
            return false;
        }
        self.vimrc.remaps.push(remap);
        self.vimrc.lines.push(line);
        true
    }

    /// Takes `modes` off the earlier records whose left side is the same
    /// keys. In nvim, a trailing blank isn't part of an unmap's left side.
    fn unmap(&mut self, line: usize, modes: &[RemapMode], args: &str) {
        let Some(args) = self.map_arguments(line, args) else {
            return;
        };
        let from = args.trim_end_matches([' ', '\t']);
        if from.is_empty() {
            self.issue(line, "unmap needs a left side");
            return;
        }
        let from = match parse_keys(&with_leader(from, &self.leader)) {
            Ok(keys) => keys,
            Err(error) => return self.issue(line, error.to_string()),
        };
        self.retain_remaps(|remap| {
            if parse_keys(&remap.from).is_ok_and(|keys| keys == from) {
                remap.modes.retain(|mode| !modes.contains(mode));
            }
        });
    }

    fn mapclear(&mut self, line: usize, modes: &[RemapMode], args: &str) {
        match args.trim_matches([' ', '\t']) {
            "" => {}
            "<buffer>" => return self.issue(line, "<buffer> isn't supported"),
            other => return self.issue(line, format!("trailing characters: {other}")),
        }
        self.retain_remaps(|remap| remap.modes.retain(|mode| !modes.contains(mode)));
    }

    /// Edits each record with `edit`, dropping those left without a mode,
    /// and their lines.
    fn retain_remaps(&mut self, mut edit: impl FnMut(&mut Remap)) {
        let mut keep = self.vimrc.remaps.iter_mut().map(|remap| {
            edit(remap);
            !remap.modes.is_empty()
        });
        let keep: Vec<bool> = keep.by_ref().collect();
        let mut at = keep.iter();
        self.vimrc.remaps.retain(|_| *at.next().unwrap());
        let mut at = keep.iter();
        self.vimrc.lines.retain(|_| *at.next().unwrap());
    }

    /// `args` after the leading `<silent>`-style arguments, or `None` after
    /// an issue for one that isn't supported.
    fn map_arguments<'a>(&mut self, line: usize, mut args: &'a str) -> Option<&'a str> {
        loop {
            args = args.trim_start_matches([' ', '\t']);
            let Some(name) = args
                .strip_prefix('<')
                .and_then(|after| after.split_once('>'))
                .map(|(name, _)| name)
            else {
                return Some(args);
            };
            match name {
                "silent" | "nowait" => {}
                "buffer" | "expr" | "script" | "unique" | "special" => {
                    self.issue(line, format!("<{name}> isn't supported"));
                    return None;
                }
                _ => return Some(args),
            }
            args = &args[name.len() + 2..];
        }
    }

    /// `let mapleader = "…"`, or an issue for any other `let`.
    fn let_<'a>(&mut self, line: usize, args: &'a str) -> Option<&'a str> {
        let args = args.trim_start_matches([' ', '\t']);
        let name_len = args
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':'))
            .unwrap_or(args.len());
        let name = &args[..name_len];
        let value = args[name_len..]
            .trim_start_matches([' ', '\t'])
            .strip_prefix('=')
            .map(|value| value.trim_start_matches([' ', '\t']));
        let (Some(value), "mapleader" | "g:mapleader") = (value, name) else {
            self.issue(line, format!("not supported: let {name}"));
            return None;
        };
        let (leader, after) = match string(value) {
            Some(Ok(parsed)) => parsed,
            Some(Err(message)) => {
                self.issue(line, message);
                return None;
            }
            None => {
                self.issue(line, "mapleader must be a quoted string");
                return None;
            }
        };
        let after = after.trim_start_matches([' ', '\t']);
        let rest = match after.chars().next() {
            None | Some('"') => None,
            Some('|') => Some(&after[1..]),
            Some(_) => {
                self.issue(line, format!("trailing characters: {after}"));
                return None;
            }
        };
        self.leader = if leader.is_empty() {
            "\\".to_string()
        } else {
            leader.into_iter().map(notation).collect()
        };
        rest
    }

    fn issue(&mut self, line: usize, message: impl Into<String>) {
        self.vimrc.issues.push(VimrcIssue {
            line,
            message: message.into(),
        });
    }
}

/// `map!`, `noremap!`, `unmap!` and `mapclear!`: insert and command-line.
fn bang_command(command: Command) -> Option<Command> {
    match command {
        Command::Map(NXO, recursive) => Some(Command::Map(IC, recursive)),
        Command::Unmap(NXO) => Some(Command::Unmap(IC)),
        Command::Mapclear(NXO) => Some(Command::Mapclear(IC)),
        _ => None,
    }
}

/// The command's text up to an unescaped `|` (`:help map-bar`), or a `"`
/// when the command takes `comments`, with the `\` of each `\|` (or `\"`)
/// taken out, and the text after that `|`. A CTRL-V keeps the character
/// after it.
fn split_bar(text: &str, comments: bool) -> (String, Option<&str>) {
    let mut taken = String::new();
    let mut chars = text.char_indices();
    while let Some((at, ch)) = chars.next() {
        let ends = ch == '|' || (comments && ch == '"');
        match ch {
            '\x16' => {
                taken.push(ch);
                if let Some((_, next)) = chars.next() {
                    taken.push(next);
                }
            }
            _ if ends && taken.ends_with('\\') => {
                taken.pop();
                taken.push(ch);
            }
            '|' => return (taken, Some(&text[at + 1..])),
            _ if ends => return (taken, None),
            _ => taken.push(ch),
        }
    }
    (taken, None)
}

/// `text` with each `<leader>` (any case) replaced by `leader`, as nvim
/// does when it reads the line. A `<` the leader would otherwise close
/// into a `<name>` is written `<lt>`, since nvim reads it as itself.
fn with_leader(text: &str, leader: &str) -> String {
    const TAG: &str = "<leader>";
    let lower = text.to_ascii_lowercase();
    let mut out = String::new();
    let mut at = 0;
    while let Some(found) = lower[at..].find(TAG) {
        let piece = &text[at..at + found];
        let open = piece.rfind('<').filter(|&open| {
            piece[open + 1..]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
        });
        match open {
            Some(open) => {
                out.push_str(&piece[..open]);
                out.push_str("<lt>");
                out.push_str(&piece[open + 1..]);
            }
            None => out.push_str(piece),
        }
        out.push_str(leader);
        at += found + TAG.len();
    }
    out.push_str(&text[at..]);
    out
}

/// A quoted string at the start of `text`, as the keys its value types,
/// and the text after its closing quote; `None` if it isn't one. The value
/// is read as nvim reads it: escapes give bytes, which are then UTF-8, and
/// a NUL ends it.
fn string(text: &str) -> Option<Result<(Vec<VimKey>, &str), String>> {
    let mut value = Value::default();
    if let Some(body) = text.strip_prefix('\'') {
        let mut chars = body.char_indices().peekable();
        while let Some((at, ch)) = chars.next() {
            if ch == '\'' && chars.next_if(|&(_, next)| next == '\'').is_none() {
                return Some(value.keys().map(|keys| (keys, &body[at + 1..])));
            }
            value.push_char(ch);
        }
        return None;
    }
    let body = text.strip_prefix('"')?;
    let bytes = body.as_bytes();
    let mut problem = None;
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'"' => {
                let after = &body[at + 1..];
                return Some(match problem {
                    Some(message) => Err(message),
                    None => value.keys().map(|keys| (keys, after)),
                });
            }
            b'\\' if at + 1 < bytes.len() => {
                at += 1;
                let next = bytes[at];
                at += 1;
                match next {
                    b'x' | b'X' | b'u' | b'U' => {
                        let max = match next {
                            b'x' | b'X' => 2,
                            b'u' => 4,
                            _ => 8,
                        };
                        let (number, len) = hex(&bytes[at..], max);
                        at += len;
                        match (len, next) {
                            (0, _) => value.push_byte(next),
                            (_, b'x' | b'X') => value.push_byte(number as u8),
                            _ => value.push_code(number),
                        }
                    }
                    b'0'..=b'7' => {
                        let mut number = u32::from(next - b'0');
                        for _ in 0..2 {
                            match bytes.get(at) {
                                Some(&digit @ b'0'..=b'7') => {
                                    number = number * 8 + u32::from(digit - b'0');
                                    at += 1;
                                }
                                _ => break,
                            }
                        }
                        value.push_byte(number as u8);
                    }
                    b'<' => match special(&body[at - 1..]) {
                        Some((len, key)) => {
                            at += len - 1;
                            match key {
                                Special::Char(ch) => value.push_char(ch),
                                Special::Key(key) => value.push_key(key),
                                Special::Unsupported(name) if !value.ended && problem.is_none() => {
                                    problem = Some(format!("mapleader: <{name}> isn't supported"));
                                }
                                Special::Unsupported(_) => {}
                                Special::Invalid(name) => {
                                    if problem.is_none() {
                                        problem = Some(format!("mapleader: <{name}> is invalid"));
                                    }
                                }
                            }
                        }
                        None => value.push_byte(b'<'),
                    },
                    b'b' => value.push_byte(0x08),
                    b'e' => value.push_byte(0x1b),
                    b'f' => value.push_byte(0x0c),
                    b'n' => value.push_byte(b'\n'),
                    b'r' => value.push_byte(b'\r'),
                    b't' => value.push_byte(b'\t'),
                    _ => {
                        // Any other escaped character is itself.
                        let len = utf8_len(next);
                        value.push_bytes(&bytes[at - 1..(at - 1 + len).min(bytes.len())]);
                        at += len - 1;
                    }
                }
            }
            byte => {
                value.push_byte(byte);
                at += 1;
            }
        }
    }
    None
}

/// A string's value as it is read: bytes, and the keys that aren't bytes.
#[derive(Default)]
struct Value {
    keys: Vec<VimKey>,
    bytes: Vec<u8>,
    /// A NUL ended the value; the rest of the string is read but dropped.
    ended: bool,
    /// Bytes that aren't UTF-8, or a code that isn't a character: nvim
    /// keeps them raw, and fe-vim has no key for them.
    invalid: bool,
}

impl Value {
    fn push_byte(&mut self, byte: u8) {
        if byte == 0 {
            self.ended = true;
        }
        if !self.ended {
            self.bytes.push(byte);
        }
    }

    fn push_bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.push_byte(byte);
        }
    }

    fn push_char(&mut self, ch: char) {
        self.push_bytes(ch.encode_utf8(&mut [0; 4]).as_bytes());
    }

    /// A `\u` or `\U` value, which nvim reads as a C `int`: a negative one
    /// writes its low byte alone.
    fn push_code(&mut self, code: u32) {
        if (code as i32) < 0 {
            self.push_byte(code as u8);
        } else if let Some(ch) = char::from_u32(code) {
            self.push_char(ch);
        } else if !self.ended {
            self.invalid = true;
        }
    }

    fn push_key(&mut self, key: VimKey) {
        if !self.ended {
            self.flush();
            self.keys.push(key);
        }
    }

    fn flush(&mut self) {
        match String::from_utf8(std::mem::take(&mut self.bytes)) {
            Ok(text) => self.keys.extend(text.chars().map(char_key)),
            Err(_) => self.invalid = true,
        }
    }

    /// The keys, or an issue when fe-vim can't type the value.
    fn keys(mut self) -> Result<Vec<VimKey>, String> {
        self.flush();
        if self.invalid {
            return Err("mapleader isn't valid UTF-8".to_string());
        }
        Ok(self.keys)
    }
}

/// Up to `max` hex digits at the start of `bytes`, as a number, and how
/// many there were.
fn hex(bytes: &[u8], max: usize) -> (u32, usize) {
    let mut number = 0;
    let mut len = 0;
    while len < max {
        let Some(digit) = bytes.get(len).and_then(|&b| (b as char).to_digit(16)) else {
            break;
        };
        number = number * 16 + digit;
        len += 1;
    }
    (number, len)
}

fn utf8_len(first: u8) -> usize {
    match first {
        0xf0.. => 4,
        0xe0.. => 3,
        0xc0.. => 2,
        _ => 1,
    }
}

/// What a `\<…>` in a double-quoted string stands for.
enum Special {
    Char(char),
    Key(VimKey),
    /// A key nvim knows that fe-vim can't type, by the text in `<…>`.
    Unsupported(String),
    /// A `<char-N>` whose number nvim rejects, failing the whole `let`.
    Invalid(String),
}

const SHIFT: u8 = 1;
const CTRL: u8 = 2;
const OTHER: u8 = 4;
/// nvim's `2-`, `3-` and `4-` (double, triple and quadruple click).
const CLICK: u8 = 8;

/// The `<…>` at the start of `src`, read as nvim's `find_special_key`
/// reads it in a string, and its length; `None` if it is literal text.
fn special(src: &str) -> Option<(usize, Special)> {
    let b = src.as_bytes();
    let last = b.len() - 1;
    let ident = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    // Find the end of the modifier list and the name.
    let mut last_dash = 0;
    let mut bp = 1;
    while bp <= last && (b[bp] == b'-' || ident(b[bp])) {
        if b[bp] == b'-' {
            last_dash = bp;
            if bp < last {
                // Any character after a dash, like `<C-[>`; but `"` ends the
                // string, so `<C-">` is literal and `<C-\">` is `"`.
                let len = utf8_len(b[bp + 1]);
                if last - bp > len && b[bp + 1] != b'"' && b[bp + len + 1] == b'>' {
                    bp += len;
                } else if last - bp > 2
                    && b[bp + 1] == b'\\'
                    && b[bp + 2] == b'"'
                    && b[bp + 3] == b'>'
                {
                    bp += 2;
                }
            }
        }
        if last - bp > 3 && b[bp] == b't' && b[bp + 1] == b'_' {
            bp += 3;
        } else if last - bp > 4 && b[bp..bp + 5].eq_ignore_ascii_case(b"char-") {
            // The number may have a sign here; only an unsigned one is a
            // character below.
            // A sign with no digits after it is read alone.
            let sign = usize::from(b[bp + 5] == b'-');
            let after = &b[bp + 5 + sign..];
            let digits = match number(after) {
                Some((_, digits)) => digits,
                None if sign == 1 && !after.first().is_some_and(u8::is_ascii_alphanumeric) => 0,
                None => {
                    let text = src[1..].split('>').next().unwrap_or_default();
                    return Some((bp + 5, Special::Invalid(text.to_string())));
                }
            };
            bp += 5 + sign + digits;
            break;
        }
        bp += 1;
    }
    if bp > last || b[bp] != b'>' {
        return None;
    }
    let len = bp + 1;
    let text = &src[1..bp];
    let mut modifiers = 0;
    for &byte in &b[1..last_dash.max(1)] {
        if byte != b'-' {
            modifiers |= match byte.to_ascii_lowercase() {
                b's' => SHIFT,
                b'c' => CTRL,
                b'a' | b'm' | b'd' | b't' => OTHER,
                b'2' | b'3' | b'4' => CLICK,
                _ => return None,
            };
        }
    }
    let name = &b[last_dash + 1..bp];
    let key =
        if name.len() > 5 && name[..5].eq_ignore_ascii_case(b"char-") && name[5].is_ascii_digit() {
            // nvim converts the number to a C `int`: the low 32 bits, where
            // a negative value is a special key and 0 is no key at all.
            let (code, _) = number(&name[5..])?;
            match code as u32 as i32 {
                0 => return None,
                code => u32::try_from(code)
                    .ok()
                    .and_then(char::from_u32)
                    .map_or(Named::Other, Named::Char),
            }
        } else {
            let name = match name {
                [b'\\', b'"', ..] => &name[1..],
                _ => name,
            };
            let single = std::str::from_utf8(name).ok().and_then(|name| {
                let mut chars = name.chars();
                chars.next().filter(|_| chars.next().is_none())
            });
            match single {
                Some(ch) if modifiers != 0 => Named::Char(ch),
                // nvim leaves an unknown name after a click modifier as
                // text; it is too likely a typo for a mouse key to guess.
                _ => match key_name(name) {
                    Some(named) => named,
                    None if modifiers & CLICK != 0 => Named::Other,
                    None => return None,
                },
            }
        };
    let key = match key {
        Named::Char('\0') => return None,
        Named::Char(ch) => ch,
        Named::Key(_) | Named::Other if modifiers != 0 => {
            return Some((len, Special::Unsupported(text.to_string())));
        }
        Named::Key(key) => return Some((len, Special::Key(key))),
        Named::Other => return Some((len, Special::Unsupported(text.to_string()))),
    };
    // nvim's `extract_modifiers`, simplifying as a string does.
    let mut key = key;
    if modifiers & CTRL == 0 && modifiers & SHIFT != 0 && key.is_ascii_alphabetic() {
        key = key.to_ascii_uppercase();
        modifiers &= !SHIFT;
    }
    if modifiers & CTRL != 0 && (('?'..='_').contains(&key) || key.is_ascii_alphabetic()) {
        key = match key.to_ascii_uppercase() {
            '?' => '\x7f',
            '@' => return Some((len, Special::Unsupported(text.to_string()))),
            upper => ((upper as u8) & 0x1f) as char,
        };
        modifiers &= !CTRL;
    }
    if modifiers != 0 {
        return Some((len, Special::Unsupported(text.to_string())));
    }
    Some((len, Special::Char(key)))
}

/// A number at the start of `bytes` as nvim's strict `vim_str2nr` reads
/// it with every radix allowed, and its length: `0x` hex, `0o` or a leading
/// `0` octal (decimal when a digit is 8 or 9), `0b` binary, else decimal.
/// The value saturates. `None` where nvim gives E474: no digits, or a
/// letter or digit right after them.
fn number(bytes: &[u8]) -> Option<(u64, usize)> {
    let digit_at = |at: usize, radix: u32| bytes.get(at).and_then(|&b| (b as char).to_digit(radix));
    let (radix, start) = match bytes {
        [b'0', b'x' | b'X', ..] if digit_at(2, 16).is_some() => (16, 2),
        [b'0', b'o' | b'O', ..] if digit_at(2, 8).is_some() => (8, 2),
        [b'0', b'b' | b'B', ..] if digit_at(2, 2).is_some() => (2, 2),
        [b'0', rest @ ..] if rest.first().is_some_and(u8::is_ascii_digit) => {
            let digits = rest.iter().take_while(|b| b.is_ascii_digit());
            if digits.clone().all(|&b| b <= b'7') {
                (8, 1)
            } else {
                (10, 0)
            }
        }
        _ => (10, 0),
    };
    let mut value: u64 = 0;
    let mut len = start;
    while let Some(digit) = digit_at(len, radix) {
        value = value
            .saturating_mul(u64::from(radix))
            .saturating_add(u64::from(digit));
        len += 1;
    }
    if len == start || bytes.get(len).is_some_and(u8::is_ascii_alphanumeric) {
        return None;
    }
    Some((value, len))
}

/// What a key name stands for in nvim, before modifiers.
enum Named {
    Char(char),
    Key(VimKey),
    /// A key fe-vim has no `VimKey` for.
    Other,
}

/// nvim's key names (`key_names_table`, matched ignoring case, with the
/// `x` keys as the keys they stand for), or `None` for a name it doesn't
/// know, which is literal text.
fn key_name(name: &[u8]) -> Option<Named> {
    // A termcap name: `t_` (in lower case) and the two characters that
    // name the key.
    if name.len() >= 4 && name.starts_with(b"t_") {
        return Some(match &name[2..4] {
            b"kb" => Named::Key(VimKey::Backspace),
            b"ku" => Named::Key(VimKey::Up),
            b"kd" => Named::Key(VimKey::Down),
            b"kl" => Named::Key(VimKey::Left),
            b"kr" => Named::Key(VimKey::Right),
            _ => Named::Other,
        });
    }
    let name = std::str::from_utf8(name).ok()?.to_ascii_lowercase();
    let named = match name.as_str() {
        "esc" | "escape" => Named::Char('\x1b'),
        "cr" | "return" | "enter" => Named::Char('\r'),
        "tab" => Named::Char('\t'),
        "nl" | "newline" | "linefeed" | "lf" => Named::Char('\n'),
        "space" => Named::Char(' '),
        "lt" => Named::Char('<'),
        "bar" => Named::Char('|'),
        "bslash" => Named::Char('\\'),
        "csi" => Named::Char('\u{9b}'),
        "bs" | "backspace" => Named::Key(VimKey::Backspace),
        "up" | "xup" => Named::Key(VimKey::Up),
        "down" | "xdown" => Named::Key(VimKey::Down),
        "left" | "xleft" => Named::Key(VimKey::Left),
        "right" | "xright" => Named::Key(VimKey::Right),
        "cmd" | "del" | "delete" | "drop" | "end" | "find" | "help" | "home" | "ignore" | "ins"
        | "insert" | "kcomma" | "kdel" | "kdivide" | "kdown" | "kend" | "kenter" | "kequal"
        | "khome" | "kinsert" | "kleft" | "kminus" | "kmultiply" | "korigin" | "kpagedown"
        | "kpageup" | "kpcomma" | "kpdiv" | "kpenter" | "kpequals" | "kplus" | "kpminus"
        | "kpmult" | "kpoint" | "kpperiod" | "kpplus" | "kright" | "kup" | "leftdrag"
        | "leftmouse" | "leftmousenm" | "leftrelease" | "leftreleasenm" | "middledrag"
        | "middlemouse" | "middlerelease" | "mouse" | "mousedown" | "mousemove" | "mouseup"
        | "nul" | "pagedown" | "pageup" | "plug" | "rightdrag" | "rightmouse" | "rightrelease"
        | "scrollwheeldown" | "scrollwheelleft" | "scrollwheelright" | "scrollwheelup"
        | "select" | "snr" | "undo" | "x1drag" | "x1mouse" | "x1release" | "x2drag" | "x2mouse"
        | "x2release" | "xend" | "xhome" | "zend" | "zhome" => Named::Other,
        other => {
            let numbered = |prefix: &str, max: u32| {
                other
                    .strip_prefix(prefix)
                    .filter(|n| !n.starts_with('0') || n.len() == 1)
                    .and_then(|n| n.parse::<u32>().ok())
                    .is_some_and(|n| n <= max)
            };
            let function_key = numbered("f", 63) && !other.ends_with("f0");
            let x_function_key = numbered("xf", 4) && !other.ends_with("f0");
            if function_key || x_function_key || numbered("k", 9) || numbered("kp", 9) {
                Named::Other
            } else {
                return None;
            }
        }
    };
    Some(named)
}

/// The key a typed character is, with the control characters that have
/// their own keys (`\r` is `<CR>`) as those keys.
fn char_key(ch: char) -> VimKey {
    match ch {
        '\r' => VimKey::Enter,
        '\t' => VimKey::Tab,
        '\x1b' => VimKey::Esc,
        '\x01'..='\x1a' => VimKey::Ctrl((ch as u8 + b'a' - 1) as char),
        _ => VimKey::Char(ch),
    }
}

/// One key in vim notation, so it reads back as itself.
pub(crate) fn notation(key: VimKey) -> String {
    match key {
        VimKey::Char(' ') => "<Space>".to_string(),
        VimKey::Char('<') => "<lt>".to_string(),
        VimKey::Char('|') => "<Bar>".to_string(),
        VimKey::Char(ch) => ch.to_string(),
        VimKey::Ctrl(ch) => format!("<C-{ch}>"),
        VimKey::Esc => "<Esc>".to_string(),
        VimKey::Enter => "<CR>".to_string(),
        VimKey::Backspace => "<BS>".to_string(),
        VimKey::Tab => "<Tab>".to_string(),
        VimKey::Left => "<Left>".to_string(),
        VimKey::Right => "<Right>".to_string(),
        VimKey::Up => "<Up>".to_string(),
        VimKey::Down => "<Down>".to_string(),
    }
}
