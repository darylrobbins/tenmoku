// SPDX-License-Identifier: LicenseRef-Proprietary

//! Context predicates, such as `editor && !read_only` or
//! `vim.mode == normal`, and the context stacks they are evaluated against.
//!
//! The grammar keeps to the shape of GPUI's key-context predicates (minus
//! its `>` descendant operator), so card 1b can hand a binding's predicate
//! to GPUI. It adds `.` in identifiers and quoted values; see
//! `docs/decisions/command-model.md`.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    str::FromStr,
};

use serde::{Deserialize, Serialize};

/// When a command applies, as parsed by [`ContextPredicate::parse`].
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum ContextPredicate {
    /// True if any frame has it as its name or as a flag.
    Identifier(String),
    /// `key == value`, read from the innermost frame that sets `key`.
    Equal(String, String),
    /// `key != value`. True when `key` is unset.
    NotEqual(String, String),
    Not(Box<ContextPredicate>),
    And(Box<ContextPredicate>, Box<ContextPredicate>),
    Or(Box<ContextPredicate>, Box<ContextPredicate>),
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{message} at byte {offset}")]
pub struct PredicateError {
    /// Byte offset into the source where parsing failed.
    pub offset: usize,
    pub message: String,
}

impl ContextPredicate {
    /// Parses identifiers (`[A-Za-z_][A-Za-z0-9_.]*`), `key == value`,
    /// `key != value` (the value a bare `[A-Za-z0-9_.-]+` word or a quoted
    /// string), `!p`, `p && q`, `p || q` and parentheses. `!` binds tightest,
    /// then `&&`, then `||`; both binary operators are left-associative.
    pub fn parse(source: &str) -> Result<Self, PredicateError> {
        let mut parser = Parser { source, pos: 0 };
        let predicate = parser.or()?;
        parser.skip_whitespace();
        if parser.pos < source.len() {
            return Err(parser.error("unexpected input"));
        }
        Ok(predicate)
    }

    pub fn identifier(name: impl Into<String>) -> Self {
        Self::Identifier(name.into())
    }

    pub fn eval(&self, stack: &ContextStack) -> bool {
        match self {
            Self::Identifier(name) => stack.frames.iter().any(|frame| frame.has(name)),
            Self::Equal(key, value) => stack.get(key) == Some(value.as_str()),
            Self::NotEqual(key, value) => stack.get(key) != Some(value.as_str()),
            Self::Not(inner) => !inner.eval(stack),
            Self::And(left, right) => left.eval(stack) && right.eval(stack),
            Self::Or(left, right) => left.eval(stack) || right.eval(stack),
        }
    }

    /// How tightly it binds when printed: higher binds tighter.
    fn precedence(&self) -> u8 {
        match self {
            Self::Or(..) => 0,
            Self::And(..) => 1,
            Self::Not(_) => 2,
            Self::Equal(..) | Self::NotEqual(..) => 3,
            Self::Identifier(_) => 4,
        }
    }

    /// Prints `self`, in parentheses if it binds looser than `min`.
    fn fmt_at(&self, f: &mut fmt::Formatter<'_>, min: u8) -> fmt::Result {
        if self.precedence() < min {
            write!(f, "({self})")
        } else {
            write!(f, "{self}")
        }
    }
}

/// Prints a normalised form that parses back to an equal predicate.
/// Operands of `!` other than identifiers are parenthesised, as GPUI's
/// parser needs for comparisons.
impl fmt::Display for ContextPredicate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Identifier(name) => f.write_str(name),
            Self::Equal(key, value) => write!(f, "{key} == {}", Value(value)),
            Self::NotEqual(key, value) => write!(f, "{key} != {}", Value(value)),
            Self::Not(inner) => {
                f.write_str("!")?;
                inner.fmt_at(f, 4)
            }
            Self::And(left, right) => {
                left.fmt_at(f, 1)?;
                f.write_str(" && ")?;
                right.fmt_at(f, 2)
            }
            Self::Or(left, right) => {
                left.fmt_at(f, 0)?;
                f.write_str(" || ")?;
                right.fmt_at(f, 1)
            }
        }
    }
}

/// A comparison's value: bare if it's a bare word, else quoted.
struct Value<'a>(&'a str);

impl fmt::Display for Value<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.0.is_empty() && self.0.chars().all(is_bare_char) {
            return f.write_str(self.0);
        }
        f.write_str("\"")?;
        for ch in self.0.chars() {
            if matches!(ch, '"' | '\\') {
                f.write_str("\\")?;
            }
            write!(f, "{ch}")?;
        }
        f.write_str("\"")
    }
}

impl FromStr for ContextPredicate {
    type Err = PredicateError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        Self::parse(source)
    }
}

impl TryFrom<String> for ContextPredicate {
    type Error = PredicateError;

    fn try_from(source: String) -> Result<Self, Self::Error> {
        Self::parse(&source)
    }
}

impl From<ContextPredicate> for String {
    fn from(predicate: ContextPredicate) -> Self {
        predicate.to_string()
    }
}

fn is_identifier_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn is_identifier_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '.'
}

fn is_bare_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-')
}

struct Parser<'a> {
    source: &'a str,
    pos: usize,
}

impl Parser<'_> {
    fn or(&mut self) -> Result<ContextPredicate, PredicateError> {
        let mut left = self.and()?;
        while self.eat("||") {
            let right = self.and()?;
            left = ContextPredicate::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<ContextPredicate, PredicateError> {
        let mut left = self.unary()?;
        while self.eat("&&") {
            let right = self.unary()?;
            left = ContextPredicate::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<ContextPredicate, PredicateError> {
        self.skip_whitespace();
        // `!` but not the start of a `!=`, which has no left operand here.
        if self.rest().starts_with('!') && !self.rest().starts_with("!=") {
            self.pos += 1;
            return Ok(ContextPredicate::Not(Box::new(self.unary()?)));
        }
        if self.eat("(") {
            let inner = self.or()?;
            if !self.eat(")") {
                return Err(self.error("expected `)`"));
            }
            return Ok(inner);
        }
        let key = self.identifier()?;
        if self.eat("==") {
            Ok(ContextPredicate::Equal(key, self.value()?))
        } else if self.eat("!=") {
            Ok(ContextPredicate::NotEqual(key, self.value()?))
        } else {
            Ok(ContextPredicate::Identifier(key))
        }
    }

    fn identifier(&mut self) -> Result<String, PredicateError> {
        self.skip_whitespace();
        if !self.rest().starts_with(is_identifier_start) {
            return Err(self.error("expected an identifier"));
        }
        let len = self
            .rest()
            .find(|ch| !is_identifier_char(ch))
            .unwrap_or(self.rest().len());
        Ok(self.take(len).to_string())
    }

    fn value(&mut self) -> Result<String, PredicateError> {
        self.skip_whitespace();
        if !self.rest().starts_with('"') {
            let len = self
                .rest()
                .find(|ch| !is_bare_char(ch))
                .unwrap_or(self.rest().len());
            if len == 0 {
                return Err(self.error("expected a value"));
            }
            return Ok(self.take(len).to_string());
        }
        // A quoted string, with `\"` and `\\` escapes.
        let start = self.pos;
        self.pos += 1;
        let mut value = String::new();
        let mut chars = self.rest().char_indices();
        while let Some((ix, ch)) = chars.next() {
            match ch {
                '\\' => match chars.next() {
                    Some((_, escaped)) => value.push(escaped),
                    None => break,
                },
                '"' => {
                    self.pos += ix + 1;
                    return Ok(value);
                }
                ch => value.push(ch),
            }
        }
        self.pos = start;
        Err(self.error("unterminated string"))
    }

    fn rest(&self) -> &str {
        &self.source[self.pos..]
    }

    fn take(&mut self, len: usize) -> &str {
        let taken = &self.source[self.pos..self.pos + len];
        self.pos += len;
        taken
    }

    /// Skips whitespace, then consumes `token` if it's next.
    fn eat(&mut self, token: &str) -> bool {
        self.skip_whitespace();
        if self.rest().starts_with(token) {
            self.pos += token.len();
            true
        } else {
            false
        }
    }

    fn skip_whitespace(&mut self) {
        let rest = self.rest();
        self.pos += rest.len() - rest.trim_start().len();
    }

    fn error(&self, message: &str) -> PredicateError {
        let found = match self.rest().chars().next() {
            Some(ch) => format!("found {ch:?}"),
            None => "found the end".to_string(),
        };
        PredicateError {
            offset: self.pos,
            message: format!("{message}, {found}"),
        }
    }
}

/// The context a command is dispatched in: frames from outermost to
/// innermost, such as `[workbench, editor]`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContextStack {
    frames: Vec<ContextFrame>,
}

impl ContextStack {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `frame` as the new innermost frame.
    pub fn push(&mut self, frame: ContextFrame) {
        self.frames.push(frame);
    }

    /// [`ContextStack::push`] for building one up.
    pub fn with(mut self, frame: ContextFrame) -> Self {
        self.push(frame);
        self
    }

    pub fn frames(&self) -> &[ContextFrame] {
        &self.frames
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// `key`'s value in the innermost frame that sets it.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.frames.iter().rev().find_map(|frame| frame.get(key))
    }
}

impl FromIterator<ContextFrame> for ContextStack {
    fn from_iter<I: IntoIterator<Item = ContextFrame>>(frames: I) -> Self {
        Self {
            frames: frames.into_iter().collect(),
        }
    }
}

/// One level of context: a name, flags, and `key = value` pairs, as in
/// `ContextFrame::new("editor").flag("read_only").set("vim.mode", "normal")`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextFrame {
    pub name: String,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub flags: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub values: BTreeMap<String, String>,
}

impl ContextFrame {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            flags: BTreeSet::new(),
            values: BTreeMap::new(),
        }
    }

    pub fn flag(mut self, flag: impl Into<String>) -> Self {
        self.flags.insert(flag.into());
        self
    }

    pub fn set(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.values.insert(key.into(), value.into());
        self
    }

    /// Whether it has `identifier` as its name or a flag.
    pub fn has(&self, identifier: &str) -> bool {
        self.name == identifier || self.flags.contains(identifier)
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }
}
