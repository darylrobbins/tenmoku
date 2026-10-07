//! The pending-input reducer: keys in, at most one invocation out.

use fe_command::{CommandId, Invocation, Source};
use serde::Serialize;

use crate::{
    VimKey,
    args::{Motion, MoveArgs, OperateArgs, Operator, PutArgs, Target, TextObject},
};

/// The whole incomplete command: counts, `"{reg}`, an operator and a prefix
/// awaiting its next key. The default is nothing pending.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pending {
    /// The product of the counts already finished (`2` of `2d3`).
    count: Option<u32>,
    /// The count being typed.
    typing: Option<u32>,
    register: Option<char>,
    operator: Option<Operator>,
    prefix: Option<Prefix>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Prefix {
    /// `"`, awaiting a register name.
    Register,
    G,
    /// `f F t T`: the motion, its char to be filled in.
    Find(Motion),
    /// `i` or `a` after an operator.
    Object {
        inner: bool,
    },
}

/// What a [`Pending`] is waiting on, for `vim.pending_kind` and which-key.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PendingKind {
    Count,
    Register,
    Operator,
    G,
    Find,
    TextObject,
}

impl PendingKind {
    /// Its `vim.pending_kind` value: `count`, `register`, `operator`, `g`,
    /// `find` or `text_object`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::Register => "register",
            Self::Operator => "operator",
            Self::G => "g",
            Self::Find => "find",
            Self::TextObject => "text_object",
        }
    }
}

/// What a key did.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// The command isn't complete: keep this and read the next key.
    Wait(Pending),
    /// The command is complete. Its source is [`Source::Keyboard`] and its
    /// context empty: the caller stamps that.
    Commit(Invocation),
    /// `<Esc>`, or a key that doesn't continue what was pending: drop it all
    /// and run nothing.
    Cancel,
    /// Nothing was pending and the key isn't vim's: let it propagate.
    Forward,
}

impl Pending {
    pub fn new() -> Self {
        Self::default()
    }

    /// What it waits on, by the last thing typed; `None` if nothing is
    /// pending.
    pub fn kind(&self) -> Option<PendingKind> {
        Some(match self.prefix {
            Some(Prefix::Register) => PendingKind::Register,
            Some(Prefix::G) => PendingKind::G,
            Some(Prefix::Find(_)) => PendingKind::Find,
            Some(Prefix::Object { .. }) => PendingKind::TextObject,
            None if self.typing.is_some() => PendingKind::Count,
            None if self.operator.is_some() => PendingKind::Operator,
            None if self.register.is_some() => PendingKind::Register,
            None if self.count.is_some() => PendingKind::Count,
            None => return None,
        })
    }

    /// Whether an operator waits for its target: `vim.mode` is
    /// `operator_pending`.
    pub fn has_operator(&self) -> bool {
        self.operator.is_some()
    }

    fn end_count(&mut self) {
        if let Some(typed) = self.typing.take() {
            self.count = Some(
                self.count
                    .map_or(typed, |count| count.saturating_mul(typed)),
            );
        }
    }

    /// A key that doesn't continue the command.
    fn reject(&self) -> Step {
        if *self == Self::default() {
            Step::Forward
        } else {
            Step::Cancel
        }
    }

    fn commit(mut self, target: Target) -> Step {
        self.end_count();
        match (self.operator, target) {
            (Some(operator), target) => invocation(
                "vim.operate",
                OperateArgs {
                    operator,
                    target,
                    count: self.count,
                    register: self.register,
                },
            ),
            (None, Target::Motion(motion)) => invocation(
                "vim.move",
                MoveArgs {
                    motion,
                    extend: false,
                    count: self.count,
                },
            ),
            (None, Target::TextObject(_)) => Step::Cancel,
        }
    }

    /// The key after a prefix.
    fn complete(self, prefix: Prefix, key: VimKey) -> Step {
        let ch = match key {
            VimKey::Char(ch) => ch,
            VimKey::Tab if matches!(prefix, Prefix::Find(_)) => '\t',
            _ => return Step::Cancel,
        };
        match prefix {
            Prefix::Register if is_register(ch) => Step::Wait(Pending {
                register: Some(ch),
                ..self
            }),
            Prefix::G if ch == 'g' => self.commit(Target::Motion(Motion::DocumentStart)),
            Prefix::Find(motion) => self.commit(Target::Motion(match motion {
                Motion::FindForward(_) => Motion::FindForward(ch),
                Motion::FindBackward(_) => Motion::FindBackward(ch),
                Motion::TillForward(_) => Motion::TillForward(ch),
                _ => Motion::TillBackward(ch),
            })),
            Prefix::Object { inner } => match text_object(ch, inner) {
                Some(object) => self.commit(Target::TextObject(object)),
                None => Step::Cancel,
            },
            _ => Step::Cancel,
        }
    }
}

/// Reads one key in normal or operator-pending mode. Pure: it sees only the
/// pending value and the key, never a model or buffer.
pub fn step(mut pending: Pending, key: VimKey) -> Step {
    if let Some(prefix) = pending.prefix.take() {
        return pending.complete(prefix, key);
    }
    let ch = match key {
        VimKey::Char(ch) => ch,
        VimKey::Esc => return Step::Cancel,
        _ => return pending.reject(),
    };
    // `0` is a motion unless a count is being typed.
    if let Some(digit) = ch.to_digit(10)
        && (digit != 0 || pending.typing.is_some())
    {
        let typed = pending.typing.unwrap_or(0);
        pending.typing = Some(typed.saturating_mul(10).saturating_add(digit));
        return Step::Wait(pending);
    }
    let prefix = match ch {
        '"' if pending.operator.is_none() => Prefix::Register,
        'g' => Prefix::G,
        'f' => Prefix::Find(Motion::FindForward(' ')),
        'F' => Prefix::Find(Motion::FindBackward(' ')),
        't' => Prefix::Find(Motion::TillForward(' ')),
        'T' => Prefix::Find(Motion::TillBackward(' ')),
        'i' | 'a' if pending.operator.is_some() => Prefix::Object { inner: ch == 'i' },
        _ => {
            if let Some(operator) = operator(ch) {
                return match pending.operator {
                    None => {
                        pending.end_count();
                        pending.operator = Some(operator);
                        Step::Wait(pending)
                    }
                    Some(pending_operator) if pending_operator == operator => {
                        pending.commit(Target::Motion(Motion::Line))
                    }
                    Some(_) => Step::Cancel,
                };
            }
            if let Some(motion) = motion(ch) {
                return pending.commit(Target::Motion(motion));
            }
            // `x X D` and `p P` stand alone: after an operator they cancel.
            if pending.operator.is_some() {
                return pending.reject();
            }
            if let Some(motion) = shorthand(ch) {
                pending.operator = Some(Operator::Delete);
                return pending.commit(Target::Motion(motion));
            }
            return match ch {
                'p' | 'P' => {
                    pending.end_count();
                    invocation(
                        "vim.put",
                        PutArgs {
                            before: ch == 'P',
                            count: pending.count,
                            register: pending.register,
                        },
                    )
                }
                _ => pending.reject(),
            };
        }
    };
    pending.end_count();
    pending.prefix = Some(prefix);
    Step::Wait(pending)
}

fn invocation(command: &str, args: impl Serialize) -> Step {
    let command = CommandId::parse(command).expect("vim command IDs are valid");
    let args = serde_json::to_value(args).expect("vim args serialize");
    Step::Commit(Invocation::new(command).args(args).source(Source::Keyboard))
}

fn is_register(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '"' | '-' | '_' | '+' | '*')
}

fn operator(ch: char) -> Option<Operator> {
    Some(match ch {
        'd' => Operator::Delete,
        'y' => Operator::Yank,
        'c' => Operator::Change,
        '>' => Operator::Indent,
        '<' => Operator::Outdent,
        _ => return None,
    })
}

fn motion(ch: char) -> Option<Motion> {
    Some(match ch {
        'h' => Motion::Left,
        'l' => Motion::Right,
        'k' => Motion::Up,
        'j' => Motion::Down,
        'w' => Motion::WordForward,
        'b' => Motion::WordBackward,
        'e' => Motion::WordEnd,
        'W' => Motion::BigWordForward,
        'B' => Motion::BigWordBackward,
        'E' => Motion::BigWordEnd,
        '0' => Motion::LineStart,
        '^' => Motion::FirstNonBlank,
        '$' => Motion::LineEnd,
        'G' => Motion::DocumentEnd,
        '%' => Motion::MatchPair,
        ';' => Motion::RepeatFind,
        ',' => Motion::RepeatFindReverse,
        _ => return None,
    })
}

/// `x X D`: `dl dh d$`.
fn shorthand(ch: char) -> Option<Motion> {
    Some(match ch {
        'x' => Motion::Right,
        'X' => Motion::Left,
        'D' => Motion::LineEnd,
        _ => return None,
    })
}

fn text_object(ch: char, inner: bool) -> Option<TextObject> {
    use TextObject::*;
    let (i, a) = match ch {
        'w' => (InnerWord, AWord),
        'W' => (InnerBigWord, ABigWord),
        '(' | ')' | 'b' => (InnerParen, AParen),
        '{' | '}' | 'B' => (InnerBrace, ABrace),
        '"' => (InnerDoubleQuote, ADoubleQuote),
        '\'' => (InnerSingleQuote, ASingleQuote),
        _ => return None,
    };
    Some(if inner { i } else { a })
}
