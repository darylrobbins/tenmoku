//! Snippets, as LSP completions, user snippets and emmet insert them: the
//! LSP snippet grammar
//! (<https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#snippet_syntax>),
//! inserted with its placeholders filled and its whitespace adjusted to the
//! row, and a session walking its tabstops.
//!
//! - **Parsing** never fails: anything malformed, such as an unclosed `${`,
//!   is literal text. A tabstop number repeated is a mirror of the first
//!   with a placeholder, showing its text; a transform
//!   (`${1/regex/format/flags}`) is kept as a mirror with no transform
//!   applied. No variable is resolved: one with a default inserts the
//!   default, one without becomes a placeholder holding its name, visited
//!   after the numbered tabstops, as VS Code does with a variable it
//!   doesn't know. Hostile input is bounded: placeholders nest at most
//!   [`MAX_DEPTH`] deep, as do mirrors of mirrors, parsing does at most
//!   [`WORK_PER_BYTE`] steps a byte before every further `$` is literal,
//!   and mirrors add at most [`MAX_MIRROR_BYTES`] of text.
//! - **Inserting** ([`EditorModel::insert_snippet`]) prefixes each line
//!   after the first with the row's indentation and writes each `\t` as the
//!   indent unit (and `\r\n` or `\r` as `\n`, as the buffer keeps them),
//!   then selects every occurrence of the first stop.
//! - **The session** follows the stops' occurrences through edits. Each
//!   edit belongs to one occurrence of the stop being visited, which grows
//!   with it, as do the placeholders it's nested in; other occurrences
//!   don't grow with text typed at their ends, and one an edit cuts into is
//!   gone. Typing in one occurrence of the stop visited, as after a click
//!   into it, is copied into its mirrors, in the same undo step. Tab and
//!   Shift-Tab (`editor.tab`, `editor.outdent`) move between stops;
//!   reaching `$0`, or the snippet's end without one, ends it. So does the
//!   selections leaving every occurrence of the stop visited, an edit
//!   outside them, or `editor.single_selection`.

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    ops::Range,
    rc::Rc,
};

use fe_buffer::{Bias, Buffer, BufferSnapshot, ResolvedSelection, Text, TransactionId, Version};

use crate::EditorModel;

/// A parsed snippet: its text with the placeholders filled, and its
/// tabstops.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snippet {
    text: String,
    tabstops: Vec<Tabstop>,
    /// Every tabstop's occurrences, in document order: each one's start,
    /// and before the occurrences nested in it.
    occurrences: Vec<Occurrence>,
}

/// What a tabstop is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StopId {
    /// `$n`; `0` is the final stop.
    Number(u32),
    /// A variable with no default, which holds its name.
    Variable(String),
}

/// A tabstop: where its occurrences are in the snippet's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tabstop {
    pub id: StopId,
    /// Byte ranges in [`Snippet::text`], in order: the placeholder and its
    /// mirrors.
    pub ranges: Vec<Range<usize>>,
    /// A choice's options, the first of which is its text; empty if it
    /// isn't one.
    pub choices: Vec<String>,
}

/// An occurrence of a tabstop.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Occurrence {
    /// The tabstop's index in visiting order.
    stop: usize,
    range: Range<usize>,
    /// The occurrence it's nested in, as the snippet is written.
    parent: Option<usize>,
}

impl Snippet {
    pub fn parse(source: &str) -> Snippet {
        let mut parser = Parser {
            source,
            pos: 0,
            depth: 0,
            failed: HashSet::new(),
            work: source.len().saturating_mul(WORK_PER_BYTE) + 4096,
        };
        let pieces = parser.sequence(false);
        render(&pieces)
    }

    /// The text it inserts, placeholders filled.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The tabstops in visiting order: `1`, `2` and on, then variables'
    /// placeholders, then `0`, if it has one.
    pub fn tabstops(&self) -> &[Tabstop] {
        &self.tabstops
    }

    /// Whether it has the final stop, `$0`, which is then the last tabstop.
    fn has_final(&self) -> bool {
        self.tabstops
            .last()
            .is_some_and(|stop| stop.id == StopId::Number(0))
    }
}

/// How deep placeholders nest, and mirrors show mirrors, before a `$` is
/// literal text or a mirror is empty, so a hostile snippet can't overflow
/// the stack.
pub const MAX_DEPTH: usize = 64;

/// How many steps parsing takes a byte of the source, at most, before every
/// further `$` is literal text, so malformed input that's reparsed over and
/// over can't stall the editor.
pub const WORK_PER_BYTE: usize = 16;

/// How much text mirrors add, at most, to the snippet and to the
/// placeholders' texts kept for them, before further mirrors are empty, so
/// mirrors of mirrors can't blow a snippet up.
pub const MAX_MIRROR_BYTES: usize = 4 << 20;

/// A parsed piece of a snippet.
enum Piece {
    Text(String),
    Stop {
        number: u32,
        /// A placeholder's text; `None` for a bare tabstop or a transform.
        content: Option<Vec<Piece>>,
        choices: Vec<String>,
    },
    Variable {
        name: String,
        default: Option<Vec<Piece>>,
    },
}

struct Parser<'a> {
    source: &'a str,
    pos: usize,
    depth: usize,
    /// Where a `$` failed to parse: it fails wherever it's met, so it's
    /// parsed once, not once per enclosing attempt.
    failed: HashSet<usize>,
    /// The steps parsing a `$` may still take.
    work: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.source[self.pos..].chars().next()
    }

    fn eat(&mut self, ch: char) -> bool {
        let found = self.peek() == Some(ch);
        if found {
            self.pos += ch.len_utf8();
        }
        found
    }

    /// Takes a step of parsing a `$`; `false` once there are none left.
    fn step(&mut self) -> bool {
        self.work = self.work.saturating_sub(1);
        self.work > 0
    }

    /// Pieces up to the end, or inside a placeholder up to its `}`, which
    /// is left for the caller.
    fn sequence(&mut self, inside: bool) -> Vec<Piece> {
        let mut pieces = Vec::new();
        let mut text = String::new();
        while let Some(ch) = self.peek() {
            if inside && !self.step() {
                break;
            }
            match ch {
                '}' if inside => break,
                '\\' => {
                    self.pos += 1;
                    match self.peek() {
                        Some(escaped @ ('$' | '}' | '\\')) => {
                            self.pos += 1;
                            text.push(escaped);
                        }
                        _ => text.push('\\'),
                    }
                }
                '$' => {
                    let start = self.pos;
                    match self.dollar() {
                        Some(piece) => {
                            if !text.is_empty() {
                                pieces.push(Piece::Text(std::mem::take(&mut text)));
                            }
                            pieces.push(piece);
                        }
                        None => {
                            self.failed.insert(start);
                            self.pos = start + 1;
                            text.push('$');
                        }
                    }
                }
                ch => {
                    self.pos += ch.len_utf8();
                    text.push(ch);
                }
            }
        }
        if !text.is_empty() {
            pieces.push(Piece::Text(text));
        }
        pieces
    }

    /// A tabstop, placeholder, choice or variable at the `$` here; `None`,
    /// wherever it got to, if there isn't one.
    fn dollar(&mut self) -> Option<Piece> {
        if self.failed.contains(&self.pos) || self.depth >= MAX_DEPTH || self.work <= 1 {
            return None;
        }
        self.pos += 1;
        if let Some(number) = self.number() {
            return Some(Piece::Stop {
                number,
                content: None,
                choices: Vec::new(),
            });
        }
        if let Some(name) = self.name() {
            return Some(Piece::Variable {
                name,
                default: None,
            });
        }
        if !self.eat('{') {
            return None;
        }
        if let Some(number) = self.number() {
            let (content, choices) = match self.peek()? {
                '}' => (None, Vec::new()),
                ':' => {
                    self.pos += 1;
                    (Some(self.placeholder()?), Vec::new())
                }
                '|' => {
                    self.pos += 1;
                    let choices = self.choices()?;
                    let text = vec![Piece::Text(choices[0].clone())];
                    return Some(Piece::Stop {
                        number,
                        content: Some(text),
                        choices,
                    });
                }
                '/' => {
                    self.pos += 1;
                    self.transform()?;
                    return Some(Piece::Stop {
                        number,
                        content: None,
                        choices: Vec::new(),
                    });
                }
                _ => return None,
            };
            self.eat('}').then_some(())?;
            return Some(Piece::Stop {
                number,
                content,
                choices,
            });
        }
        let name = self.name()?;
        let default = match self.peek()? {
            '}' => None,
            ':' => {
                self.pos += 1;
                Some(self.placeholder()?)
            }
            '/' => {
                self.pos += 1;
                self.transform()?;
                return Some(Piece::Variable {
                    name,
                    default: None,
                });
            }
            _ => return None,
        };
        self.eat('}').then_some(())?;
        Some(Piece::Variable { name, default })
    }

    /// A placeholder's pieces, up to its `}`, which is left; `None` if it
    /// isn't closed.
    fn placeholder(&mut self) -> Option<Vec<Piece>> {
        self.depth += 1;
        let pieces = self.sequence(true);
        self.depth -= 1;
        (self.work > 0 && self.peek() == Some('}')).then_some(pieces)
    }

    fn number(&mut self) -> Option<u32> {
        let digits = self.source[self.pos..]
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        let number = self.source[self.pos..self.pos + digits].parse().ok()?;
        self.pos += digits;
        Some(number)
    }

    fn name(&mut self) -> Option<String> {
        let rest = &self.source[self.pos..];
        if !rest
            .bytes()
            .next()
            .is_some_and(|byte| byte == b'_' || byte.is_ascii_alphabetic())
        {
            return None;
        }
        let len = rest
            .bytes()
            .take_while(|&byte| byte == b'_' || byte.is_ascii_alphanumeric())
            .count();
        self.pos += len;
        Some(rest[..len].to_owned())
    }

    /// A choice's options after its first `|`, through its `|}`.
    fn choices(&mut self) -> Option<Vec<String>> {
        let mut choices = Vec::new();
        let mut choice = String::new();
        loop {
            if !self.step() {
                return None;
            }
            match self.peek()? {
                '\\' => {
                    self.pos += 1;
                    match self.peek() {
                        Some(escaped @ ('$' | '}' | '\\' | ',' | '|')) => {
                            self.pos += 1;
                            choice.push(escaped);
                        }
                        _ => choice.push('\\'),
                    }
                }
                ',' => {
                    self.pos += 1;
                    choices.push(std::mem::take(&mut choice));
                }
                '|' => {
                    self.pos += 1;
                    self.eat('}').then_some(())?;
                    choices.push(choice);
                    return Some(choices);
                }
                ch => {
                    self.pos += ch.len_utf8();
                    choice.push(ch);
                }
            }
        }
    }

    /// Skips a transform after its first `/`: the regex, the format, whose
    /// `${1:/upcase}` and the like hold `/` and `}`, and the flags,
    /// through its `}`.
    fn transform(&mut self) -> Option<()> {
        for format in [false, true] {
            loop {
                if !self.step() {
                    return None;
                }
                match self.peek()? {
                    '\\' => {
                        self.pos += 1;
                        if let Some(ch) = self.peek() {
                            self.pos += ch.len_utf8();
                        }
                    }
                    '/' => {
                        self.pos += 1;
                        break;
                    }
                    '$' if format && self.source[self.pos..].starts_with("${") => {
                        self.past_brace()?;
                    }
                    ch => self.pos += ch.len_utf8(),
                }
            }
        }
        self.past_brace()
    }

    /// Moves past the next `}`.
    fn past_brace(&mut self) -> Option<()> {
        loop {
            if !self.step() {
                return None;
            }
            let ch = self.peek()?;
            self.pos += ch.len_utf8();
            if ch == '}' {
                return Some(());
            }
        }
    }
}

/// The snippet `pieces` make.
fn render(pieces: &[Piece]) -> Snippet {
    let mut renderer = Renderer {
        budget: MAX_MIRROR_BYTES,
        ..Renderer::default()
    };
    renderer.find_definers(pieces);
    renderer.render(pieces);
    let Renderer {
        text,
        choices,
        recorded,
        variables,
        ..
    } = renderer;
    // Visiting order: numbers but 0, then variables, then 0.
    let numbers: BTreeSet<u32> = (recorded.iter())
        .filter_map(|(key, _, _)| match key {
            Key::Number(number) => Some(*number),
            Key::Variable(_) => None,
        })
        .collect();
    let mut ids: Vec<StopId> = (numbers.iter())
        .filter(|&&number| number != 0)
        .map(|&number| StopId::Number(number))
        .collect();
    let numbered = ids.len();
    ids.extend(variables.into_iter().map(StopId::Variable));
    let mut index: HashMap<Key, usize> = HashMap::new();
    for (ix, id) in ids.iter().enumerate() {
        let key = match id {
            StopId::Number(number) => Key::Number(*number),
            StopId::Variable(_) => Key::Variable(ix - numbered),
        };
        index.insert(key, ix);
    }
    if numbers.contains(&0) {
        index.insert(Key::Number(0), ids.len());
        ids.push(StopId::Number(0));
    }
    let mut tabstops: Vec<Tabstop> = ids
        .into_iter()
        .map(|id| Tabstop {
            choices: match id {
                StopId::Number(number) => choices.get(&number).cloned().unwrap_or_default(),
                StopId::Variable(_) => Vec::new(),
            },
            id,
            ranges: Vec::new(),
        })
        .collect();
    let occurrences = recorded
        .into_iter()
        .map(|(key, range, parent)| {
            let stop = index[&key];
            tabstops[stop].ranges.push(range.clone());
            Occurrence {
                stop,
                range,
                parent,
            }
        })
        .collect();
    Snippet {
        text,
        tabstops,
        occurrences,
    }
}

/// A tabstop as rendering records it: a number, or the `n`th variable
/// without a default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Key {
    Number(u32),
    Variable(usize),
}

#[derive(Default)]
struct Renderer<'a> {
    /// Each number's placeholder: the first with text, depth first.
    definers: HashMap<u32, &'a [Piece]>,
    choices: HashMap<u32, Vec<String>>,
    text: String,
    /// Each occurrence, in document order, with the one it's nested in.
    recorded: Vec<(Key, Range<usize>, Option<usize>)>,
    /// The variables without a default, in order.
    variables: Vec<String>,
    /// The occurrences being rendered, innermost last.
    open: Vec<usize>,
    /// The numbers whose placeholder is being rendered: a mirror of one
    /// inside it is empty.
    rendering: Vec<u32>,
    /// Each number's placeholder's text, as its mirrors show it.
    plain: HashMap<u32, Rc<str>>,
    /// What mirrors may still add, to the text and to `plain`.
    budget: usize,
}

impl<'a> Renderer<'a> {
    fn find_definers(&mut self, pieces: &'a [Piece]) {
        for piece in pieces {
            match piece {
                Piece::Text(_) => {}
                Piece::Stop {
                    number,
                    content,
                    choices,
                } => {
                    if let Some(content) = content {
                        if !self.definers.contains_key(number) {
                            self.definers.insert(*number, content);
                            if !choices.is_empty() {
                                self.choices.insert(*number, choices.clone());
                            }
                        }
                        self.find_definers(content);
                    }
                }
                Piece::Variable { default, .. } => {
                    if let Some(default) = default {
                        self.find_definers(default);
                    }
                }
            }
        }
    }

    /// Renders `pieces`, recording where their stops are.
    fn render(&mut self, pieces: &'a [Piece]) {
        for piece in pieces {
            match piece {
                Piece::Text(text) => self.text.push_str(text),
                Piece::Stop {
                    number, content, ..
                } => {
                    let start = self.text.len();
                    let id = self.recorded.len();
                    let parent = self.open.last().copied();
                    self.recorded
                        .push((Key::Number(*number), start..start, parent));
                    let definer = self.definers.get(number).copied();
                    let own = definer
                        .zip(content.as_deref())
                        .is_some_and(|(definer, content)| std::ptr::eq(content, definer));
                    if own && !self.rendering.contains(number) {
                        self.open.push(id);
                        self.rendering.push(*number);
                        self.render(definer.expect("own"));
                        self.rendering.pop();
                        self.open.pop();
                    } else {
                        // A mirror shows its placeholder's text, but not
                        // the stops inside it.
                        let text = self.plain(*number, 0);
                        if self.spend(text.len()) {
                            self.text.push_str(&text);
                        }
                    }
                    self.recorded[id].1.end = self.text.len();
                }
                Piece::Variable {
                    default: Some(default),
                    ..
                } => self.render(default),
                Piece::Variable {
                    name,
                    default: None,
                } => {
                    let start = self.text.len();
                    self.text.push_str(name);
                    let key = Key::Variable(self.variables.len());
                    self.variables.push(name.clone());
                    let parent = self.open.last().copied();
                    self.recorded.push((key, start..self.text.len(), parent));
                }
            }
        }
    }

    /// Takes `len` bytes from the budget, if it has them; once it
    /// hasn't, it's spent.
    fn spend(&mut self, len: usize) -> bool {
        let fits = len <= self.budget;
        self.budget = if fits { self.budget - len } else { 0 };
        fits
    }

    /// The text of `number`'s placeholder, empty if it has none, is being
    /// rendered, is `depth` mirrors deep past [`MAX_DEPTH`], or the budget
    /// is spent.
    fn plain(&mut self, number: u32, depth: usize) -> Rc<str> {
        if let Some(text) = self.plain.get(&number) {
            return Rc::clone(text);
        }
        let definer = self.definers.get(&number).copied();
        let Some(definer) = definer
            .filter(|_| self.budget > 0 && depth < MAX_DEPTH && !self.rendering.contains(&number))
        else {
            return Rc::from("");
        };
        self.rendering.push(number);
        let mut text = String::new();
        self.plain_into(definer, depth, &mut text);
        self.rendering.pop();
        let text: Rc<str> = text.into();
        self.plain.insert(number, Rc::clone(&text));
        text
    }

    fn plain_into(&mut self, pieces: &'a [Piece], depth: usize, out: &mut String) {
        for piece in pieces {
            let text = match piece {
                Piece::Text(text) => Rc::from(text.as_str()),
                Piece::Stop { number, .. } => self.plain(*number, depth + 1),
                Piece::Variable {
                    default: Some(default),
                    ..
                } => {
                    self.plain_into(default, depth, out);
                    continue;
                }
                Piece::Variable {
                    name,
                    default: None,
                } => Rc::from(name.as_str()),
            };
            if self.spend(text.len()) {
                out.push_str(&text);
            }
        }
    }
}

/// A snippet's text as inserted on a row, and its occurrences in it.
pub(crate) struct Expansion {
    text: String,
    /// As [`Snippet`] has them, in this text.
    occurrences: Vec<Occurrence>,
    /// How many tabstops it has, with `$0`.
    stops: usize,
    has_final: bool,
}

impl Expansion {
    /// `snippet`, each line after the first prefixed with `indent`, each
    /// `\t` written as `unit`, and each `\r\n` or `\r` as `\n`.
    pub(crate) fn new(snippet: &Snippet, indent: &str, unit: &str) -> Self {
        let source = snippet.text();
        let mut text = String::with_capacity(source.len());
        // Where each char of the source, and its end, went; stops only
        // start and end at chars.
        let mut at = vec![0; source.len() + 1];
        let mut chars = source.char_indices().peekable();
        while let Some((ix, ch)) = chars.next() {
            at[ix] = text.len();
            match ch {
                '\t' => text.push_str(unit),
                // The `\n` it comes before stands for both.
                '\r' if chars.peek().is_some_and(|&(_, next)| next == '\n') => {}
                '\n' | '\r' => {
                    text.push('\n');
                    text.push_str(indent);
                }
                ch => text.push(ch),
            }
        }
        at[source.len()] = text.len();
        let occurrences = (snippet.occurrences.iter())
            .map(|occurrence| Occurrence {
                range: at[occurrence.range.start]..at[occurrence.range.end],
                ..occurrence.clone()
            })
            .collect();
        Expansion {
            text,
            occurrences,
            stops: snippet.tabstops().len(),
            has_final: snippet.has_final(),
        }
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// The tabstops other than `$0`.
    fn visited(&self) -> usize {
        self.stops - usize::from(self.has_final)
    }

    /// What's selected once it's inserted at `start`: the first stop's
    /// occurrences, or else `$0`'s, or else a caret at its end.
    pub(crate) fn first_selections(&self, start: usize) -> Vec<ResolvedSelection> {
        let stop = if self.visited() > 0 {
            Some(0)
        } else {
            self.has_final.then(|| self.stops - 1)
        };
        let end = self.text.len();
        let ranges: Vec<_> = match stop {
            Some(stop) => (self.occurrences.iter())
                .filter(|occurrence| occurrence.stop == stop)
                .map(|occurrence| occurrence.range.clone())
                .collect(),
            None => std::iter::once(end..end).collect(),
        };
        ranges
            .into_iter()
            .map(|range| selection(range.start + start..range.end + start))
            .collect()
    }
}

/// The indentation of the row `offset` of `text` is on, up to `offset`.
pub(crate) fn row_indent(text: &Text, offset: usize) -> String {
    let row_start = text.row_start(text.offset_to_point(offset).row);
    let before = text.slice_to_string(row_start..offset);
    let len = before
        .find(|c: char| !c.is_whitespace())
        .unwrap_or(before.len());
    before[..len].to_owned()
}

/// A snippet inserted, its stops followed through the edits since: see
/// the module docs.
pub(crate) struct SnippetSession {
    /// The version the offsets are in.
    version: Version,
    /// Every stop's occurrences, in document order, the last stop's where
    /// the session ends: `$0`, or the snippet's end.
    occurrences: Vec<Tracked>,
    /// How many stops there are, the last included.
    stops: usize,
    /// The stop being visited, never the last.
    current: usize,
    /// The edits about to be made from the text of a version, one each as
    /// asked for, before the buffer joins those at one offset, and who
    /// each belongs to.
    planned: Option<(Version, Vec<Owned>)>,
}

struct Tracked {
    stop: usize,
    range: Range<usize>,
    parent: Option<usize>,
    /// Whether an edit cut into it.
    gone: bool,
}

/// An edit the session follows: what it replaced, how long its text is,
/// and the occurrence it belongs to: of the stop visited, or a mirror its
/// text is copied into.
#[derive(Clone, Debug)]
struct Owned {
    old: Range<usize>,
    len: usize,
    owner: usize,
}

/// Where `have` differs from `want`: the length of their common start, and
/// where in `have` their common end (not overlapping it) starts.
fn differing(have: &str, want: &str) -> (usize, usize) {
    let start = (have.char_indices().zip(want.chars()))
        .find(|((_, a), b)| a != b)
        .map_or(have.len().min(want.len()), |((ix, _), _)| ix);
    let room = have.len().min(want.len()) - start;
    let end = (have[start..].chars().rev().zip(want[start..].chars().rev()))
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum::<usize>()
        .min(room);
    (start, have.len() - end)
}

/// `edits`, sorted, as the buffer applies them: those that overlap, or
/// insert at one offset, joined.
fn joined(edits: &[Owned]) -> Vec<(Range<usize>, usize)> {
    let mut joined: Vec<(Range<usize>, usize)> = Vec::with_capacity(edits.len());
    for Owned {
        old: range, len, ..
    } in edits
    {
        if let Some((last, last_len)) = joined.last_mut()
            && (range.start < last.end
                || (last.start == last.end
                    && range.start == range.end
                    && range.start == last.start))
        {
            last.end = last.end.max(range.end);
            *last_len += len;
            continue;
        }
        joined.push((range.clone(), *len));
    }
    joined.retain(|(range, len)| range.start < range.end || *len > 0);
    joined
}

impl SnippetSession {
    /// The occurrences of `stop` that are left, by index.
    fn of(&self, stop: usize) -> impl Iterator<Item = usize> + '_ {
        (self.occurrences.iter().enumerate())
            .filter(move |(_, occurrence)| occurrence.stop == stop && !occurrence.gone)
            .map(|(ix, _)| ix)
    }

    fn ranges(&self, stop: usize) -> Vec<Range<usize>> {
        self.of(stop)
            .map(|ix| self.occurrences[ix].range.clone())
            .collect()
    }

    /// Brings the stops up to `snapshot`. Returns the occurrences the
    /// edits belonged to, in order; `None` if one landed outside the
    /// current stop's occurrences, or the edits can't be told.
    fn advance(&mut self, snapshot: &BufferSnapshot) -> Option<Vec<usize>> {
        if self.version < snapshot.oldest_version() {
            return None;
        }
        let planned = self.planned.take();
        let mut edited = Vec::new();
        for (ix, batch) in snapshot.edits_since(self.version).enumerate() {
            let made: Vec<(Range<usize>, usize)> = (batch.iter())
                .map(|edit| (edit.old.clone(), edit.new.len()))
                .collect();
            // The first batch, as planned, if it's what the plan makes.
            let owned = match &planned {
                Some((version, planned))
                    if ix == 0 && *version == self.version && joined(planned) == made =>
                {
                    planned.clone()
                }
                _ => self.owners(&made)?,
            };
            for edit in &owned {
                if !edited.contains(&edit.owner) {
                    edited.push(edit.owner);
                }
            }
            self.follow(&owned);
        }
        self.version = snapshot.version();
        Some(edited)
    }

    /// The occurrence of the stop visited each of `edits` belongs to;
    /// `None` if one is outside them all. Mirrors typed together each get
    /// as many, in order; otherwise, at a boundary two share, the one no
    /// edit has gone to yet.
    fn owners(&self, edits: &[(Range<usize>, usize)]) -> Option<Vec<Owned>> {
        let current: Vec<usize> = self.of(self.current).collect();
        let holds = |ix: usize, old: &Range<usize>| {
            let range = &self.occurrences[ix].range;
            range.start <= old.start && old.end <= range.end
        };
        let owned = |owners: Vec<usize>| {
            (edits.iter().zip(owners))
                .map(|((old, len), owner)| Owned {
                    old: old.clone(),
                    len: *len,
                    owner,
                })
                .collect()
        };
        if !current.is_empty() && !edits.is_empty() && edits.len().is_multiple_of(current.len()) {
            let each = edits.len() / current.len();
            let owners: Vec<usize> = (0..edits.len()).map(|ix| current[ix / each]).collect();
            if (edits.iter().zip(&owners)).all(|((old, _), &owner)| holds(owner, old)) {
                return Some(owned(owners));
            }
        }
        let mut owners: Vec<usize> = Vec::with_capacity(edits.len());
        for (old, _) in edits {
            let mut holding = current.iter().copied().filter(|&ix| holds(ix, old));
            let first = holding.next()?;
            let owner = std::iter::once(first)
                .chain(holding)
                .find(|ix| !owners.contains(ix))
                .unwrap_or(first);
            owners.push(owner);
        }
        Some(owned(owners))
    }

    /// Whether `ancestor` is `ix`, or has it nested in it.
    fn encloses(&self, ancestor: usize, mut ix: usize) -> bool {
        loop {
            if ix == ancestor {
                return true;
            }
            match self.occurrences[ix].parent {
                Some(parent) => ix = parent,
                None => return false,
            }
        }
    }

    /// Follows the occurrences through one batch of `edits`, sorted, apart
    /// and in the text before it.
    fn follow(&mut self, edits: &[Owned]) {
        for ix in 0..self.occurrences.len() {
            if self.occurrences[ix].gone {
                continue;
            }
            let range = self.occurrences[ix].range.clone();
            let (mut start, mut end) = (range.start as isize, range.end as isize);
            for edit in edits {
                let delta = edit.len as isize - edit.old.len() as isize;
                let old = &edit.old;
                // The edit is inside it, or one nested in it.
                if self.encloses(ix, edit.owner) {
                    end += delta;
                    continue;
                }
                let inside = range.start <= old.start
                    && old.end <= range.end
                    && *old != range
                    && !(old.is_empty() && (old.start == range.start || old.end == range.end));
                if inside {
                    end += delta;
                    continue;
                }
                let cuts = if range.is_empty() {
                    old.start < range.start && range.start < old.end
                } else {
                    old.start < range.end && range.start < old.end
                };
                if cuts {
                    self.occurrences[ix].gone = true;
                    break;
                }
                // Text inserted where it starts goes before it, but where
                // an empty one is, only if it comes after the edit's.
                let at = |offset: usize| {
                    old.end < offset
                        || (old.end == offset && old.start < offset)
                        || (old.is_empty()
                            && old.start == offset
                            && offset == range.start
                            && (!range.is_empty() || ix > edit.owner))
                };
                if at(range.start) {
                    start += delta;
                }
                if at(range.end) {
                    end += delta;
                }
            }
            let start = start.max(0) as usize;
            self.occurrences[ix].range = start..(end.max(0) as usize).max(start);
        }
    }

    /// The same `edits`, `owned` by occurrences of the stop visited, made in
    /// the mirrors of each and of the placeholders enclosing it that read
    /// as it does in `text`, unless an edit is in them already.
    fn copies(
        &self,
        text: &Text,
        owned: &[Owned],
        edits: &[(Range<usize>, String)],
    ) -> Vec<(Owned, String)> {
        // The occurrences the edits are in, enclosing ones included.
        let mut touched: Vec<usize> = Vec::new();
        for edit in owned {
            let mut at = Some(edit.owner);
            while let Some(ix) = at {
                if !touched.contains(&ix) {
                    touched.push(ix);
                }
                at = self.occurrences[ix].parent;
            }
        }
        let mut copies = Vec::new();
        for (edit, (_, new)) in owned.iter().zip(edits) {
            let mut at = Some(edit.owner);
            while let Some(source) = at {
                at = self.occurrences[source].parent;
                let from = self.occurrences[source].range.clone();
                let read = text.slice_to_string(from.clone());
                for ix in self.of(self.occurrences[source].stop) {
                    let range = &self.occurrences[ix].range;
                    if touched.contains(&ix) || !text.range_eq(range.clone(), &read) {
                        continue;
                    }
                    let start = range.start + (edit.old.start - from.start);
                    copies.push((
                        Owned {
                            old: start..start + edit.old.len(),
                            len: edit.len,
                            owner: ix,
                        },
                        new.clone(),
                    ));
                }
            }
        }
        copies
    }

    /// The edits that make the other occurrences of each stop that one of
    /// `edited`, or a placeholder enclosing it, is an occurrence of, match
    /// it in `text`: the first of them for each stop, as each differs from
    /// it, its common start and end kept. Sorted and apart.
    fn mirror_edits(&self, text: &Text, edited: &[usize]) -> Vec<(Owned, String)> {
        let mut touched: Vec<usize> = Vec::new();
        for &ix in edited {
            let mut at = Some(ix);
            while let Some(ix) = at {
                if !touched.contains(&ix) {
                    touched.push(ix);
                }
                at = self.occurrences[ix].parent;
            }
        }
        let mut edits: Vec<(Owned, String)> = Vec::new();
        let mut stops = Vec::new();
        for &source in &touched {
            let stop = self.occurrences[source].stop;
            if self.occurrences[source].gone || stops.contains(&stop) {
                continue;
            }
            stops.push(stop);
            let want = text.slice_to_string(self.occurrences[source].range.clone());
            for ix in self.of(stop).filter(|ix| !touched.contains(ix)) {
                let range = self.occurrences[ix].range.clone();
                let have = text.slice_to_string(range.clone());
                if have == want {
                    continue;
                }
                let (start, end) = differing(&have, &want);
                let old = range.start + start..range.end - (have.len() - end);
                let new = &want[start..want.len() - (have.len() - end)];
                let owner = ix;
                edits.push((
                    Owned {
                        old,
                        len: new.len(),
                        owner,
                    },
                    new.to_owned(),
                ));
            }
        }
        edits.sort_by_key(|(edit, _)| (edit.old.start, edit.old.end));
        let mut apart: Vec<(Owned, String)> = Vec::with_capacity(edits.len());
        for edit in edits {
            if apart
                .last()
                .is_none_or(|(last, _)| last.old.end <= edit.0.old.start)
            {
                apart.push(edit);
            }
        }
        apart
    }

    /// Whether a selection is inside an occurrence of the current stop.
    fn holds(&self, selections: &[ResolvedSelection]) -> bool {
        let ranges = self.ranges(self.current);
        selections.iter().any(|selection| {
            ranges.iter().any(|range| {
                range.start <= selection.range.start && selection.range.end <= range.end
            })
        })
    }
}

impl EditorModel {
    /// Replaces `range` with `snippet`'s text, as one transaction: each
    /// line after the first prefixed with the indentation of the row
    /// `range` starts on, and each `\t` written as the indent unit. If it
    /// has a tabstop other than `$0`, a session starts, with every
    /// occurrence of the first stop selected; otherwise the caret goes to
    /// `$0`, or its end. A session going on ends.
    pub fn insert_snippet(&mut self, range: Range<usize>, snippet: &Snippet) {
        self.sync_display();
        self.snippet = None;
        self.linked = None;
        let text = self.text();
        let start = text.clip_offset(range.start.min(range.end), Bias::Left);
        let end = text.clip_offset(range.end.max(start), Bias::Right);
        let indent = row_indent(text, start);
        let expansion = Expansion::new(snippet, &indent, &self.indent_unit());
        let selections = expansion.first_selections(start);
        let edits = vec![(start..end, expansion.text.clone())];
        let snapshot = self.apply_selecting(edits, selections, None);
        self.start_snippet(&snapshot, start, expansion);
    }

    /// Whether a snippet session is going on.
    pub fn in_snippet(&self) -> bool {
        self.snippet.is_some()
    }

    /// Ends the snippet session, if there's one.
    pub fn end_snippet(&mut self) {
        self.snippet = None;
    }

    /// Selects the next stop's occurrences, or at the last ends the
    /// session with the caret there. `false`, doing nothing, with no
    /// session.
    pub fn next_snippet_stop(&mut self) -> bool {
        self.sync_display();
        let Some(session) = &mut self.snippet else {
            return false;
        };
        let last = session.stops - 1;
        let next = (session.current + 1..last)
            .find(|&stop| session.of(stop).next().is_some())
            .unwrap_or(last);
        let ranges = session.ranges(next);
        if next == last {
            self.snippet = None;
            if ranges.is_empty() {
                return true;
            }
        } else {
            session.current = next;
        }
        self.set_selections(ranges.into_iter().map(selection));
        true
    }

    /// Selects the previous stop's occurrences, or the first's again at
    /// the first. `false`, doing nothing, with no session.
    pub fn prev_snippet_stop(&mut self) -> bool {
        self.sync_display();
        let Some(session) = &mut self.snippet else {
            return false;
        };
        let current = session.current;
        session.current = (0..current)
            .rev()
            .find(|&stop| session.of(stop).next().is_some())
            .unwrap_or(current);
        let ranges = session.ranges(session.current);
        self.set_selections(ranges.into_iter().map(selection));
        true
    }

    /// Ends the session if an edit landed outside the current stop's
    /// occurrences or the selections left them; otherwise brings it up to
    /// the snapshot. Runs whenever the text is caught up with or the
    /// selections change.
    pub(crate) fn follow_snippet(&mut self) {
        let Some(mut session) = self.snippet.take() else {
            return;
        };
        if session.advance(&self.snapshot).is_some() && session.holds(&self.selections()) {
            self.snippet = Some(session);
        }
    }

    /// `edits`, sorted and apart, about to be made from the text as of the
    /// last catch-up, with `selections`, offsets in the text they make,
    /// and the same edits made in the mirrors after them of the
    /// occurrences they're in, as typing in each would: of the stop
    /// visited, and of the placeholders enclosing them. In one batch with
    /// the edits, they're in its undo step and its run of typing.
    /// Notes who each belongs to, before the buffer joins those at one
    /// offset. As they were with no session, or edits outside the stop
    /// visited.
    pub(crate) fn mirror_snippet_edits(
        &mut self,
        edits: Vec<(Range<usize>, String)>,
        selections: Vec<ResolvedSelection>,
    ) -> (Vec<(Range<usize>, String)>, Vec<ResolvedSelection>) {
        let version = self.snapshot.version();
        let Some(session) = self
            .snippet
            .as_mut()
            .filter(|session| session.version == version)
        else {
            return (edits, selections);
        };
        let made: Vec<(Range<usize>, usize)> = (edits.iter())
            .map(|(range, text)| (range.clone(), text.len()))
            .collect();
        let Some(owned) = session.owners(&made) else {
            return (edits, selections);
        };
        let text = self.snapshot.text();
        let copies = session.copies(text, &owned, &edits);
        // Every edit, by where it is, then by whose it is, so text
        // inserted at one offset goes in in the snippet's order.
        let mut all: Vec<(Owned, String, bool)> = (owned.into_iter().zip(edits))
            .map(|(owned, (_, text))| (owned, text, false))
            .chain(copies.into_iter().map(|(owned, text)| (owned, text, true)))
            .collect();
        all.sort_by_key(|(edit, _, _)| (edit.old.start, edit.old.end, edit.owner));
        let mut apart: Vec<(Owned, String, bool)> = Vec::with_capacity(all.len());
        for edit in all {
            match apart.last() {
                Some((last, _, _)) if edit.0.old.start < last.old.end => {
                    // A copy meeting an edit is dropped; the edits are
                    // apart.
                    if !edit.2 {
                        apart.pop();
                        apart.push(edit);
                    }
                }
                _ => apart.push(edit),
            }
        }
        // Only copies after every edit asked for go in with them, so the
        // offsets the caller worked out in the text those make stand (auto-
        // closed pairs' among them); those before are made after, apart.
        if let Some(last) = apart.iter().rposition(|(_, _, copy)| !copy) {
            let mut ix = 0;
            apart.retain(|(_, _, copy)| {
                ix += 1;
                !copy || ix - 1 > last
            });
        }
        // The selections move past the copies before them: where each
        // copy lands among the edits asked for.
        let mut shifts: Vec<(usize, isize)> = Vec::new();
        let mut delta = 0isize;
        for (edit, text, copy) in &apart {
            if *copy {
                let at = edit.old.start.checked_add_signed(delta).expect("sorted");
                shifts.push((at, text.len() as isize - edit.old.len() as isize));
            } else {
                delta += text.len() as isize - edit.old.len() as isize;
            }
        }
        let shift = |offset: usize| {
            let moved: isize = (shifts.iter())
                .filter(|(at, _)| *at < offset)
                .map(|(_, delta)| delta)
                .sum();
            offset.checked_add_signed(moved).expect("copies before it")
        };
        let selections = (selections.into_iter())
            .map(|selection| ResolvedSelection {
                range: shift(selection.range.start)..shift(selection.range.end),
                reversed: selection.reversed,
            })
            .collect();
        session.planned = Some((
            version,
            apart.iter().map(|(edit, _, _)| edit.clone()).collect(),
        ));
        let edits = (apart.into_iter())
            .map(|(edit, text, _)| (edit.old, text))
            .collect();
        (edits, selections)
    }

    /// Copies what an edit just made of an occurrence of the stop visited,
    /// or of a placeholder enclosing it, into the others of its stop that
    /// now differ, as more batches of `transaction` (or the one open), the
    /// edit's undo step. Run with the buffer as the edit left it, before
    /// catching up. Ends the session if the edit landed outside the stop.
    pub(crate) fn sync_snippet_mirrors(
        &mut self,
        buffer: &mut Buffer,
        transaction: Option<TransactionId>,
    ) {
        let Some(mut session) = self.snippet.take() else {
            return;
        };
        let Some(mut edited) = session.advance(&buffer.snapshot()) else {
            return;
        };
        // Each round copies into mirrors, which may be nested in
        // placeholders whose own mirrors the next round copies into.
        for _ in 0..MAX_DEPTH {
            let edits = session.mirror_edits(buffer.text(), &edited);
            let Some(id) = transaction.or(buffer.open_transaction()) else {
                break;
            };
            if edits.is_empty() {
                break;
            }
            let texts = edits
                .iter()
                .map(|(edit, text)| (edit.old.clone(), text.as_str()));
            if !matches!(buffer.edit_in(id, texts), Ok(Some(_))) {
                break;
            }
            let owned: Vec<Owned> = edits.into_iter().map(|(edit, _)| edit).collect();
            session.follow(&owned);
            session.version = buffer.version();
            edited = owned.iter().map(|edit| edit.owner).collect();
        }
        self.snippet = Some(session);
    }

    /// Starts a session for `expansion`, inserted at `start` in
    /// `snapshot`, if it has a stop other than `$0`. The selections are
    /// [`Expansion::first_selections`]'.
    pub(crate) fn start_snippet(
        &mut self,
        snapshot: &BufferSnapshot,
        start: usize,
        expansion: Expansion,
    ) {
        if expansion.visited() == 0 {
            return;
        }
        let end = start + expansion.text.len();
        let mut occurrences: Vec<Tracked> = (expansion.occurrences.into_iter())
            .map(|occurrence| Tracked {
                stop: occurrence.stop,
                range: occurrence.range.start + start..occurrence.range.end + start,
                parent: occurrence.parent,
                gone: false,
            })
            .collect();
        let mut stops = expansion.stops;
        if !expansion.has_final {
            occurrences.push(Tracked {
                stop: stops,
                range: end..end,
                parent: None,
                gone: false,
            });
            stops += 1;
        }
        self.snippet = Some(SnippetSession {
            version: snapshot.version(),
            occurrences,
            stops,
            current: 0,
            planned: None,
        });
        self.follow_snippet();
    }
}

fn selection(range: Range<usize>) -> ResolvedSelection {
    ResolvedSelection {
        range,
        reversed: false,
    }
}
