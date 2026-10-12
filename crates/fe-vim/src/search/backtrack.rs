//! A backtracking matcher for the patterns one `regex` pattern can't match
//! in nvim's order: an unbounded (or long) repeat of a group whose ways end
//! differently (`\%(a\|.\)*`) or whose first way is empty (`\(\|a\|b\)\+`).
//! There the exits sit inside the cycle, so the order alternatives and
//! greedy-or-lazy stops are tried in interleaves without bound.
//!
//! The pattern's tokens become a small program, run with an explicit stack
//! of choices still to try (nothing recurses, so a long row can't overflow
//! the stack) under a step budget and the operation's [`Deadline`]. What
//! each character-taking atom matches is the `regex` the translator writes
//! for it, anchored; only the order of choices is the matcher's own. As in
//! the translation, how the last atom ended (on a base written without its
//! composing characters or not) decides how the next one is read, so that
//! state travels with the position and is restored with it.

use std::collections::HashMap;

use regex::Regex;

use super::{
    Deadline, MAX_GROUPS, Opened, Path, PatternError, Token, Translator, is_mark, unique_names,
};

/// The most instructions a program may have: counts are written out.
const MAX_PROGRAM: usize = 200_000;
/// The most text, summed over its distinct leaves, a program's regexes may be
/// compiled from, and the most each may compile to: a leaf with composing
/// marks is tens of kilobytes compiled, so this bounds what a pattern retains.
const MAX_LEAF_TEXT: usize = 64 << 10;
const MAX_LEAF_COMPILED: usize = 8 << 20;
/// The most choices a search may leave to try, and the most steps one
/// search from one start may take, before it gives up.
const MAX_STACK: usize = 1 << 20;
const MAX_STEPS: u64 = 20_000_000;
/// How often, in steps, the clock is read.
const CLOCK_EVERY: u64 = 4096;

/// One slot for each end of each group; group 0 is the whole match.
const SLOTS: usize = 2 * (MAX_GROUPS + 1);
const NONE: usize = usize::MAX;

/// A pattern's tree, before it is a program.
enum Node {
    /// An atom that takes characters: for each way the last atom ended
    /// (`false`, `true`: on a bare base), its regex and how it ends.
    Leaf([(usize, bool); 2]),
    /// `^`, `$`, `\<` and `\>`.
    Look(Look),
    Seq(Vec<Node>),
    Alt(Vec<Node>),
    Group(Option<usize>, Box<Node>),
    Repeat {
        node: Box<Node>,
        min: u32,
        max: Option<u32>,
        lazy: bool,
    },
}

#[derive(Clone, Copy, Debug)]
enum Look {
    LineStart,
    LineEnd,
    WordStart,
    WordEnd,
}

#[derive(Clone, Copy, Debug)]
enum Inst {
    /// Takes the characters regex `.0` matches at the position.
    Leaf([(usize, bool); 2]),
    Look(Look),
    /// Try `.0`; if that fails, `.1`.
    Split(usize, usize),
    Jump(usize),
    /// Records the position in a slot of the captures.
    Save(usize),
    /// Records the position in a register: where a time of a loop began.
    Mark(usize),
    /// Ends a time of a loop: on if it took something, else out of the loop
    /// (the next instruction), as an empty time does in nvim.
    Progress {
        reg: usize,
        back: usize,
    },
    Match,
}

/// A compiled pattern.
#[derive(Debug)]
pub(super) struct Program {
    insts: Vec<Inst>,
    regexes: Vec<Regex>,
    regs: usize,
    /// How many nodes have been emitted, copies of a counted repeat
    /// included, which costs work even where a node emits nothing.
    work: usize,
}

/// A match's groups: `slots[2 * n]` to `slots[2 * n + 1]`, [`NONE`] if
/// group `n` took no part.
pub(super) type Slots = [usize; SLOTS];

pub(super) fn empty_slots() -> Slots {
    [NONE; SLOTS]
}

/// Group `n`'s range in `slots`, if it took part.
pub(super) fn group(slots: &Slots, n: usize) -> Option<std::ops::Range<usize>> {
    match (slots[2 * n], slots[2 * n + 1]) {
        (NONE, _) | (_, NONE) => None,
        (start, end) => Some(start..end),
    }
}

/// `tokens` as a program.
pub(super) fn compile(tokens: &[Token], ignore_case: bool) -> Result<Program, PatternError> {
    let mut builder = Builder {
        tr: Translator {
            tokens,
            next: 0,
            depth: 0,
            groups: 0,
            ignore_case,
            budget: 0,
            inexact: false,
        },
        regexes: Vec::new(),
        known: HashMap::new(),
        text: 0,
    };
    let tree = builder.alternation(Opened::Pattern)?;
    let mut program = Program {
        insts: Vec::new(),
        regexes: builder.regexes,
        regs: 0,
        work: 0,
    };
    program.insts.push(Inst::Save(0));
    program.emit(&tree)?;
    program.insts.push(Inst::Save(1));
    program.insts.push(Inst::Match);
    Ok(program)
}

struct Builder<'a> {
    tr: Translator<'a>,
    regexes: Vec<Regex>,
    known: HashMap<String, usize>,
    /// The text of the regexes so far.
    text: usize,
}

fn too_large() -> PatternError {
    PatternError::TooLarge("a repeat of alternatives that end in different ways".into())
}

impl Builder<'_> {
    /// [`Translator::alternation`]'s shape: branches up to the `\)` closing
    /// `opened` (left to the caller) or the end.
    fn alternation(&mut self, opened: Opened) -> Result<Node, PatternError> {
        let mut branches = vec![self.branch(opened)?];
        while matches!(self.tr.peek(), Some(Token::Alt)) {
            self.tr.next += 1;
            branches.push(self.branch(Opened::Alt)?);
        }
        Ok(match branches.len() {
            1 => branches.remove(0),
            _ => Node::Alt(branches),
        })
    }

    fn branch(&mut self, opened: Opened) -> Result<Node, PatternError> {
        let mut seq = Vec::new();
        let mut first = true;
        let mut star_is_char = false;
        while let Some(token) = self.tr.peek() {
            let star_here = std::mem::replace(&mut star_is_char, false);
            match token {
                Token::Alt | Token::Close => break,
                Token::Flag => {
                    self.tr.next += 1;
                    continue;
                }
                _ => {}
            }
            let (node, star) = self.piece(opened, first, star_here)?;
            star_is_char = star;
            first = false;
            seq.push(node);
        }
        Ok(Node::Seq(seq))
    }

    /// An atom and the multi after it, as [`Translator::piece`] reads them;
    /// and whether a bare `*` that follows is itself.
    fn piece(
        &mut self,
        opened: Opened,
        first: bool,
        star_here: bool,
    ) -> Result<(Node, bool), PatternError> {
        let (atom, start_of_line) = self.atom(opened, first, star_here)?;
        let multi = match self.tr.peek() {
            Some(&Token::Multi { multi, bare })
                if !(start_of_line && bare && multi == super::Multi::Star) =>
            {
                multi
            }
            Some(Token::Multi { .. }) => return Ok((atom, true)),
            _ => return Ok((atom, false)),
        };
        self.tr.next += 1;
        if matches!(self.tr.peek(), Some(Token::Multi { .. })) {
            return Err(PatternError::Invalid("a multi can't follow a multi"));
        }
        let (min, max, lazy) = match multi {
            super::Multi::Star => (0, None, false),
            super::Multi::Plus => (1, None, false),
            super::Multi::Optional => (0, Some(1), false),
            super::Multi::Brace { min, max, lazy } => (min, max, lazy),
        };
        let node = Node::Repeat {
            node: Box::new(atom),
            min,
            max,
            lazy,
        };
        Ok((node, false))
    }

    fn atom(
        &mut self,
        opened: Opened,
        first: bool,
        star_here: bool,
    ) -> Result<(Node, bool), PatternError> {
        let token = self.tr.peek().cloned().expect("an atom to build");
        match token {
            Token::Open { capture } => {
                self.tr.next += 1;
                let number = capture.then(|| {
                    self.tr.groups += 1;
                    self.tr.groups
                });
                let inner = self.alternation(if capture {
                    Opened::Capture
                } else {
                    Opened::Group
                })?;
                // `alternation` returned at the closing `\)`.
                self.tr.next += 1;
                Ok((Node::Group(number, Box::new(inner)), false))
            }
            Token::WordStart => {
                self.tr.next += 1;
                Ok((Node::Look(Look::WordStart), false))
            }
            Token::WordEnd => {
                self.tr.next += 1;
                Ok((Node::Look(Look::WordEnd), false))
            }
            Token::Caret { anywhere } if anywhere || first => {
                self.tr.next += 1;
                Ok((Node::Look(Look::LineStart), true))
            }
            Token::Dollar { anywhere } => {
                let mut rest = self.tr.tokens[self.tr.next + 1..].iter();
                let last = matches!(
                    rest.find(|token| **token != Token::Flag),
                    None | Some(Token::Alt | Token::Close)
                );
                if anywhere || last {
                    self.tr.next += 1;
                    return Ok((Node::Look(Look::LineEnd), false));
                }
                Ok((self.leaf(opened, first, star_here)?, false))
            }
            _ => Ok((self.leaf(opened, first, star_here)?, false)),
        }
    }

    /// The atom at `self.tr.next`, which takes characters, translated for
    /// either way the last atom ended.
    fn leaf(&mut self, opened: Opened, first: bool, star_here: bool) -> Result<Node, PatternError> {
        let at = (self.tr.next, self.tr.groups);
        let mut ways = [(0, false); 2];
        for after_bare in [false, true] {
            (self.tr.next, self.tr.groups) = at;
            let (paths, _) = self.tr.atom(opened, first, star_here, after_bare)?;
            let [Path { text, bare }] = &paths[..] else {
                unreachable!("an atom without a group is one path");
            };
            ways[usize::from(after_bare)] = (self.regex(text)?, *bare);
        }
        Ok(Node::Leaf(ways))
    }

    /// The index of the anchored regex for `text`.
    fn regex(&mut self, text: &str) -> Result<usize, PatternError> {
        if let Some(&known) = self.known.get(text) {
            return Ok(known);
        }
        self.text += text.len();
        if self.text > MAX_LEAF_TEXT {
            return Err(too_large());
        }
        let mut builder = regex::RegexBuilder::new(&format!("^(?:{})", unique_names(text)));
        builder.size_limit(MAX_LEAF_COMPILED);
        let regex = super::build(builder, |error| PatternError::TooLarge(error.to_string()))?;
        self.regexes.push(regex);
        self.known.insert(text.to_string(), self.regexes.len() - 1);
        Ok(self.regexes.len() - 1)
    }
}

impl Program {
    fn push(&mut self, inst: Inst) -> Result<usize, PatternError> {
        if self.insts.len() >= MAX_PROGRAM {
            return Err(too_large());
        }
        self.insts.push(inst);
        Ok(self.insts.len() - 1)
    }

    fn emit(&mut self, node: &Node) -> Result<(), PatternError> {
        self.work += 1;
        if self.work > MAX_PROGRAM {
            return Err(too_large());
        }
        match node {
            Node::Leaf(ways) => {
                self.push(Inst::Leaf(*ways))?;
            }
            Node::Look(look) => {
                self.push(Inst::Look(*look))?;
            }
            Node::Seq(nodes) => {
                for node in nodes {
                    self.emit(node)?;
                }
            }
            Node::Alt(branches) => {
                // Split(next branch, this one) ... Jump(end).
                let mut jumps = Vec::new();
                for (at, branch) in branches.iter().enumerate() {
                    if at + 1 < branches.len() {
                        let split = self.push(Inst::Split(0, 0))?;
                        self.emit(branch)?;
                        jumps.push(self.push(Inst::Jump(0))?);
                        self.insts[split] = Inst::Split(split + 1, self.insts.len());
                    } else {
                        self.emit(branch)?;
                    }
                }
                let end = self.insts.len();
                for jump in jumps {
                    self.insts[jump] = Inst::Jump(end);
                }
            }
            Node::Group(number, inner) => {
                if let Some(number) = number {
                    self.push(Inst::Save(2 * number))?;
                }
                self.emit(inner)?;
                if let Some(number) = number {
                    self.push(Inst::Save(2 * number + 1))?;
                }
            }
            Node::Repeat {
                node,
                min,
                max,
                lazy,
            } => {
                for _ in 0..*min {
                    let before = self.insts.len();
                    self.emit(node)?;
                    if self.insts.len() == before {
                        // Nothing to repeat: more copies would be nothing too.
                        break;
                    }
                }
                match max {
                    None => self.star(node, *lazy)?,
                    Some(max) => self.optionals(node, max.saturating_sub(*min), *lazy)?,
                }
            }
        }
        Ok(())
    }

    /// `node`, any number of times: a time that takes nothing is the last.
    fn star(&mut self, node: &Node, lazy: bool) -> Result<(), PatternError> {
        let reg = self.regs;
        self.regs += 1;
        let split = self.push(Inst::Split(0, 0))?;
        self.push(Inst::Mark(reg))?;
        self.emit(node)?;
        self.push(Inst::Progress { reg, back: split })?;
        let end = self.insts.len();
        self.insts[split] = match lazy {
            false => Inst::Split(split + 1, end),
            true => Inst::Split(end, split + 1),
        };
        Ok(())
    }

    /// Up to `most` times `node`, each one inside the last's choice.
    fn optionals(&mut self, node: &Node, most: u32, lazy: bool) -> Result<(), PatternError> {
        let mut splits = Vec::new();
        for _ in 0..most {
            splits.push(self.push(Inst::Split(0, 0))?);
            self.emit(node)?;
        }
        let end = self.insts.len();
        for split in splits {
            self.insts[split] = match lazy {
                false => Inst::Split(split + 1, end),
                true => Inst::Split(end, split + 1),
            };
        }
        Ok(())
    }
}

/// A choice left to try, or a change to undo on the way back to one.
enum Frame {
    Branch { pc: usize, at: usize, bare: bool },
    Slot { slot: usize, old: usize },
    Reg { reg: usize, old: usize },
}

/// Why a search stopped short.
pub(super) struct GaveUp;

/// What a search keeps between starts, so a row's matches allocate once.
#[derive(Default)]
pub(super) struct Scratch {
    stack: Vec<Frame>,
    regs: Vec<usize>,
}

impl Program {
    /// The first match starting at or after `from` in `hay`, with its
    /// groups in `slots`. `Err` when the budget or `deadline` ran out
    /// first.
    pub(super) fn find_at(
        &self,
        hay: &str,
        from: usize,
        deadline: &Deadline,
        scratch: &mut Scratch,
        slots: &mut Slots,
    ) -> Result<bool, GaveUp> {
        let mut steps = 0;
        let mut at = from;
        loop {
            if self.run(hay, at, deadline, scratch, slots, &mut steps)? {
                return Ok(true);
            }
            match hay[at..].chars().next() {
                Some(ch) => at += ch.len_utf8(),
                None => return Ok(false),
            }
        }
    }

    fn run(
        &self,
        hay: &str,
        start: usize,
        deadline: &Deadline,
        scratch: &mut Scratch,
        slots: &mut Slots,
        steps: &mut u64,
    ) -> Result<bool, GaveUp> {
        let Scratch { stack, regs } = scratch;
        stack.clear();
        regs.clear();
        regs.resize(self.regs, NONE);
        *slots = empty_slots();
        let (mut pc, mut at, mut bare) = (0, start, false);
        loop {
            *steps += 1;
            if *steps > MAX_STEPS || stack.len() > MAX_STACK {
                deadline.give_up();
                return Err(GaveUp);
            }
            if (*steps).is_multiple_of(CLOCK_EVERY) && deadline.expired() {
                return Err(GaveUp);
            }
            let next = match self.insts[pc] {
                Inst::Leaf(ways) => {
                    let (regex, ends_bare) = ways[usize::from(bare)];
                    match self.regexes[regex].find(&hay[at..]) {
                        Some(found) => {
                            at += found.end();
                            bare = ends_bare;
                            Some(pc + 1)
                        }
                        None => None,
                    }
                }
                Inst::Look(look) => {
                    let ok = match look {
                        Look::LineStart => at == 0,
                        Look::LineEnd => at == hay.len(),
                        Look::WordStart => {
                            !word(hay[..at].chars().next_back()) && word(hay[at..].chars().next())
                        }
                        Look::WordEnd => {
                            word(hay[..at].chars().next_back()) && !word(hay[at..].chars().next())
                        }
                    };
                    if ok {
                        bare = false;
                        Some(pc + 1)
                    } else {
                        None
                    }
                }
                Inst::Split(first, second) => {
                    stack.push(Frame::Branch {
                        pc: second,
                        at,
                        bare,
                    });
                    Some(first)
                }
                Inst::Jump(to) => Some(to),
                Inst::Save(slot) => {
                    stack.push(Frame::Slot {
                        slot,
                        old: slots[slot],
                    });
                    slots[slot] = at;
                    Some(pc + 1)
                }
                Inst::Mark(reg) => {
                    stack.push(Frame::Reg {
                        reg,
                        old: regs[reg],
                    });
                    regs[reg] = at;
                    Some(pc + 1)
                }
                Inst::Progress { reg, back } => Some(if regs[reg] == at { pc + 1 } else { back }),
                Inst::Match => {
                    // No match ends inside a character.
                    if hay[at..].chars().next().is_none_or(|ch| !is_mark(ch)) {
                        return Ok(true);
                    }
                    None
                }
            };
            match next {
                Some(next) => pc = next,
                None => loop {
                    match stack.pop() {
                        None => return Ok(false),
                        Some(Frame::Slot { slot, old }) => slots[slot] = old,
                        Some(Frame::Reg { reg, old }) => regs[reg] = old,
                        Some(Frame::Branch {
                            pc: to,
                            at: back,
                            bare: was,
                        }) => {
                            (pc, at, bare) = (to, back, was);
                            break;
                        }
                    }
                },
            }
        }
    }
}

fn word(ch: Option<char>) -> bool {
    ch.is_some_and(regex_syntax::is_word_character)
}
