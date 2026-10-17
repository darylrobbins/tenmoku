//! The pending-input reducer: keys in, at most one invocation out.

use std::ops::ControlFlow;

use fe_command::{CommandId, Invocation, Source};
use serde::Serialize;

use crate::{
    VimKey,
    args::{
        CommandLineArgs, CommandLineKind, CountArgs, InsertArgs, InsertAt, MarkArgs, MarkMotion,
        Motion, MoveArgs, OperateArgs, Operator, PlayArgs, PutArgs, RecordArgs, ReplaceCharArgs,
        SelectArgs, SwapArgs, Target, TextObject, VisualArgs, VisualKind,
    },
    feature,
    jump::JumpPrefix,
    marks,
};

/// The whole incomplete command: counts, `"{reg}`, an operator and a prefix
/// awaiting its next key. The default is nothing pending, in normal mode.
///
/// It also says whether a visual mode is on, which [`step`] can't see
/// otherwise: there an operator acts on the selection at once, `i` and `a`
/// start text objects, and `<Esc>` leaves the mode. [`VimState`] sets that
/// before each key ([`Pending::visual`] starts from it); it isn't pending
/// input, so [`Pending::kind`] ignores it.
///
/// [`VimState`]: crate::VimState
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pending {
    /// The product of the counts already finished (`2` of `2d3`).
    count: Option<u32>,
    /// The count being typed.
    typing: Option<u32>,
    register: Option<char>,
    operator: Option<Operator>,
    prefix: Option<Prefix>,
    visual: bool,
    /// The visual mode is blockwise.
    block: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Prefix {
    /// `"`, awaiting a register name.
    Register,
    G,
    /// `z`, awaiting a feature's key.
    Z,
    /// `Z`, awaiting a feature's key.
    BigZ,
    /// `[` or, if `forward`, `]`, awaiting a feature's key.
    Bracket {
        forward: bool,
    },
    /// `f F t T`: the motion, its char to be filled in.
    Find(Motion),
    /// `i` or `a` after an operator.
    Object {
        inner: bool,
    },
    /// `r`, awaiting the replacement.
    Replace,
    /// `m`, awaiting the mark's name.
    SetMark,
    /// `'`, or `` ` `` if `exact`, awaiting the mark's name.
    Mark {
        exact: bool,
    },
    /// `q`, awaiting the register to record into.
    Record,
    /// `@`, awaiting the register to play.
    Play,
    /// `gs` or `gS`, awaiting the chars to jump to.
    Jump(JumpPrefix),
}

/// What a [`Pending`] is waiting on, for `vim.pending_kind` and which-key.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PendingKind {
    Count,
    Register,
    Operator,
    G,
    /// `z`.
    Z,
    /// `Z`.
    BigZ,
    /// `[` or `]`.
    Bracket,
    Find,
    TextObject,
    Replace,
    /// `m`.
    SetMark,
    /// `'` or `` ` ``.
    Mark,
    /// `gs` or `gS`.
    Jump,
}

impl PendingKind {
    /// Its `vim.pending_kind` value: `count`, `register`, `operator`, `g`, `z`,
    /// `find`, `text_object`, `replace`, `set_mark`, `mark` or `jump`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::Register => "register",
            Self::Operator => "operator",
            Self::G => "g",
            Self::Z => "z",
            Self::BigZ => "big_z",
            Self::Bracket => "bracket",
            Self::Find => "find",
            Self::TextObject => "text_object",
            Self::Replace => "replace",
            Self::SetMark => "set_mark",
            Self::Mark => "mark",
            Self::Jump => "jump",
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

    /// Nothing pending, in a visual mode.
    pub fn visual() -> Self {
        Self {
            visual: true,
            ..Self::default()
        }
    }

    /// Whether it's read in a visual mode.
    pub fn is_visual(&self) -> bool {
        self.visual
    }

    /// The same pending input, read in a visual mode of `kind`, or not.
    pub(crate) fn in_visual(self, kind: Option<VisualKind>) -> Self {
        Self {
            visual: kind.is_some(),
            block: kind == Some(VisualKind::Block),
            ..self
        }
    }

    /// Nothing is pending (in either mode).
    fn is_empty(&self) -> bool {
        self.kind().is_none()
    }

    /// Nothing but a register is pending: no count, operator or prefix.
    fn at_most_register(&self) -> bool {
        self.prefix.is_none()
            && self.operator.is_none()
            && self.count.is_none()
            && self.typing.is_none()
    }

    /// What it waits on, by the last thing typed; `None` if nothing is
    /// pending.
    pub fn kind(&self) -> Option<PendingKind> {
        Some(match self.prefix {
            Some(Prefix::Register | Prefix::Record | Prefix::Play) => PendingKind::Register,
            Some(Prefix::G) => PendingKind::G,
            Some(Prefix::Z) => PendingKind::Z,
            Some(Prefix::BigZ) => PendingKind::BigZ,
            Some(Prefix::Bracket { .. }) => PendingKind::Bracket,
            Some(Prefix::Find(_)) => PendingKind::Find,
            Some(Prefix::Object { .. }) => PendingKind::TextObject,
            Some(Prefix::Replace) => PendingKind::Replace,
            Some(Prefix::SetMark) => PendingKind::SetMark,
            Some(Prefix::Mark { .. }) => PendingKind::Mark,
            Some(Prefix::Jump(_)) => PendingKind::Jump,
            None if self.typing.is_some() => PendingKind::Count,
            None if self.operator.is_some() => PendingKind::Operator,
            None if self.register.is_some() => PendingKind::Register,
            None if self.count.is_some() => PendingKind::Count,
            None => return None,
        })
    }

    /// Whether at most a count and a register are pending, which a
    /// replay's `q` drops.
    pub(crate) fn only_count_or_register(&self) -> bool {
        self.prefix.is_none() && self.operator.is_none()
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

    /// The finished count: `2` of `2d3` once the typed one has ended.
    pub(crate) fn count(&self) -> Option<u32> {
        self.count
    }

    pub(crate) fn register(&self) -> Option<char> {
        self.register
    }

    /// The operator waiting for its target.
    pub(crate) fn operator(&self) -> Option<Operator> {
        self.operator
    }

    /// Whether a prefix (`g`, `f`, `"`, ...) waits for its key.
    pub(crate) fn has_prefix(&self) -> bool {
        self.prefix.is_some()
    }

    /// The jump awaiting its chars or a label, if one is.
    pub(crate) fn jump(&self) -> Option<&JumpPrefix> {
        match &self.prefix {
            Some(Prefix::Jump(jump)) => Some(jump),
            _ => None,
        }
    }

    /// Whether the visual mode is blockwise.
    #[allow(dead_code, reason = "for the feature modules' hooks")]
    pub(crate) fn is_block(&self) -> bool {
        self.block
    }

    /// Starts `operator` (`gU`): waits for its target, the count ended.
    #[allow(dead_code, reason = "for the feature modules' hooks")]
    pub(crate) fn with_operator(mut self, operator: Operator) -> Step {
        self.end_count();
        self.operator = Some(operator);
        Step::Wait(self)
    }

    /// Commits `operator` over `target`, replacing any pending one, as an
    /// operator key in a visual mode does (`gUU` over [`Motion::Line`]).
    #[allow(dead_code, reason = "for the feature modules' hooks")]
    pub(crate) fn operate(mut self, operator: Operator, target: Target) -> Step {
        self.operator = Some(operator);
        self.commit(target)
    }

    /// Commits `motion`: `vim.move`, or `vim.operate` with the pending
    /// operator.
    #[allow(dead_code, reason = "for the feature modules' hooks")]
    pub(crate) fn motion(self, motion: Motion) -> Step {
        self.commit(Target::Motion(motion))
    }

    /// A jump's labels awaited, after `register` and `operator`.
    pub(crate) fn jump_labels(
        register: Option<char>,
        operator: Option<Operator>,
        jump: JumpPrefix,
    ) -> Self {
        Self {
            register,
            operator,
            prefix: Some(Prefix::Jump(jump)),
            ..Self::default()
        }
    }

    /// The same pending input, awaiting a jump's chars (or its label), the
    /// count ended.
    pub(crate) fn jumping(mut self, jump: JumpPrefix) -> Self {
        self.end_count();
        Self {
            prefix: Some(Prefix::Jump(jump)),
            ..self
        }
    }

    /// A key that doesn't continue the command, unless a feature takes it
    /// (with the count ended).
    fn reject(mut self, key: VimKey) -> Step {
        self.end_count();
        if let Some(step) = feature::key(&self, key) {
            return step;
        }
        if self.is_empty() {
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
                    extend: self.visual,
                    count: self.count,
                },
            ),
            (None, _) => Step::Cancel,
        }
    }

    /// `/` or `?` with no prefix waiting for a key: `vim.command_line`,
    /// carrying the count, register and operator. Anything else comes back
    /// as it was, and so does `?` after `g?`, for a feature's hook (`g??`).
    /// [`step`] doesn't read them: only a [`VimState`](crate::VimState) has
    /// a command line to open.
    pub(crate) fn command_line(mut self, key: VimKey) -> Result<Step, Self> {
        let kind = match key {
            VimKey::Char('/') => CommandLineKind::SearchForward,
            VimKey::Char('?') => CommandLineKind::SearchBackward,
            _ => return Err(self),
        };
        if self.prefix.is_some()
            || (kind == CommandLineKind::SearchBackward && self.operator == Some(Operator::Rot13))
        {
            return Err(self);
        }
        self.end_count();
        Ok(invocation(
            "vim.command_line",
            CommandLineArgs {
                kind,
                count: self.count,
                register: self.register,
                operator: self.operator,
            },
        ))
    }

    /// `:` with no operator pending: `vim.command_line` for an ex line,
    /// carrying the count in normal mode (`3:` starts it at `.,.+2`). A
    /// register is dropped, as is a count in a visual mode, whose line
    /// starts at `'<,'>`.
    fn ex_line(mut self) -> Step {
        self.end_count();
        invocation(
            "vim.command_line",
            CommandLineArgs {
                kind: CommandLineKind::Ex,
                count: self.count.filter(|_| !self.visual),
                register: None,
                operator: None,
            },
        )
    }

    /// The key after a prefix.
    fn complete(mut self, prefix: Prefix, key: VimKey) -> Step {
        if prefix == Prefix::Z {
            return feature::z(&self, key).unwrap_or(Step::Cancel);
        }
        if prefix == Prefix::BigZ {
            return feature::big_z(&self, key).unwrap_or(Step::Cancel);
        }
        if let Prefix::Bracket { forward } = prefix {
            self.end_count();
            return feature::bracket(&self, forward, key).unwrap_or(Step::Cancel);
        }
        let ch = match key {
            VimKey::Char(ch) => ch,
            VimKey::Tab if matches!(prefix, Prefix::Find(_) | Prefix::Replace) => '\t',
            _ => return self.unmatched(prefix, key),
        };
        match prefix {
            Prefix::Register if is_register(ch) => Step::Wait(Pending {
                register: Some(ch),
                ..self
            }),
            Prefix::G if ch == 'g' => self.commit(Target::Motion(Motion::DocumentStart)),
            // `gv`, which drops a count.
            Prefix::G if ch == 'v' && self.operator.is_none() => invocation("vim.reselect", ()),
            // `g&`, which is `:%s//~/&` and drops a count.
            Prefix::G if ch == '&' && self.operator.is_none() && !self.visual => {
                invocation("vim.substitute_all_lines", ())
            }
            Prefix::Find(motion) => self.commit(Target::Motion(match motion {
                Motion::FindForward(_) => Motion::FindForward(ch),
                Motion::FindBackward(_) => Motion::FindBackward(ch),
                Motion::TillForward(_) => Motion::TillForward(ch),
                _ => Motion::TillBackward(ch),
            })),
            Prefix::Object { inner } => match self.text_object(ch, inner) {
                Some(text_object) if self.visual => {
                    self.end_count();
                    invocation(
                        "vim.select",
                        SelectArgs {
                            text_object,
                            count: self.count,
                        },
                    )
                }
                Some(object) => self.commit(Target::TextObject(object)),
                None => Step::Cancel,
            },
            Prefix::Replace if self.visual => {
                self.operator = Some(Operator::Replace(ch));
                self.commit(Target::Selection)
            }
            Prefix::Replace => {
                self.end_count();
                invocation(
                    "vim.replace_char",
                    ReplaceCharArgs {
                        char: ch,
                        count: self.count,
                    },
                )
            }
            // A count is dropped.
            Prefix::SetMark if marks::settable(ch) => {
                invocation("vim.set_mark", MarkArgs { name: ch })
            }
            Prefix::Mark { exact } if marks::jumpable(ch) => {
                self.commit(Target::Motion(Motion::Mark(MarkMotion { name: ch, exact })))
            }
            Prefix::Record if ch.is_ascii_alphabetic() => {
                invocation("vim.record", RecordArgs { register: ch })
            }
            // A register before `@` is dropped.
            Prefix::Play if ch.is_ascii_lowercase() || ch == '@' || ch == ':' => invocation(
                "vim.play",
                PlayArgs {
                    register: ch,
                    count: self.count,
                },
            ),
            Prefix::Jump(jump) => match jump.then(ch) {
                ControlFlow::Continue(jump) => Step::Wait(self.jumping(jump)),
                ControlFlow::Break(motion) => self.commit(Target::Motion(motion)),
            },
            _ => self.unmatched(prefix, key),
        }
    }

    /// The text object `ch` names after `i` (`inner`) or `a`: one of the
    /// reducer's own, else the first feature's.
    fn text_object(&self, ch: char, inner: bool) -> Option<TextObject> {
        text_object(ch, inner).or_else(|| feature::object(self, ch, inner))
    }

    /// A key a prefix doesn't take: after `g` the features may, else it
    /// cancels.
    fn unmatched(self, prefix: Prefix, key: VimKey) -> Step {
        match prefix {
            Prefix::G => feature::g(&self, key).unwrap_or(Step::Cancel),
            _ => Step::Cancel,
        }
    }
}

/// Reads one key in normal, operator-pending or a visual mode (as `pending`
/// says). Pure: it sees only the pending value and the key, never a model or
/// buffer.
///
/// The named arrows are `h l k j` after a count or an operator, and in a
/// visual mode; with nothing pending in normal mode they're forwarded, to
/// the editor's own cursor keys.
///
/// `:` opens an ex command line (`vim.command_line` with `{"kind": "ex"}`)
/// unless an operator is pending, which it cancels.
///
/// `<C-j>` (`<NL>`) is `j`, as in nvim.
///
/// `q{a-zA-Z}` with nothing pending commits `vim.record` (a count or
/// register before `q` cancels), and `@{a-z}` or `@@`, after a count, commits
/// `vim.play`; both in normal mode only. Only [`VimState`](crate::VimState)
/// knows a recording is under way, so ending it with `q` is its own.
///
/// `v`, `V`, `<C-v>` and `<C-q>` start their visual mode when nothing but a
/// register is pending, dropping the register, as in nvim; a count before
/// them is `1v`'s reselect, and an operator or prefix cancels.
pub fn step(mut pending: Pending, key: VimKey) -> Step {
    if let Some(prefix) = pending.prefix.take() {
        return pending.complete(prefix, key);
    }
    let continues = pending.visual || !pending.is_empty();
    let ch = match key {
        VimKey::Char(ch) => ch,
        // In a visual mode it leaves the mode, dropping a count or register.
        VimKey::Esc if pending.visual => return invocation("vim.visual_exit", ()),
        VimKey::Esc => return Step::Cancel,
        VimKey::Left if continues => 'h',
        VimKey::Right if continues => 'l',
        VimKey::Up if continues => 'k',
        VimKey::Down if continues => 'j',
        // `<NL>` (a register's line break, as `@` plays it) is `j`.
        VimKey::Ctrl('j') => 'j',
        // `<C-q>` is `<C-v>`, as in nvim.
        VimKey::Ctrl('v' | 'q') if pending.visual || pending.at_most_register() => {
            return invocation(
                "vim.visual",
                VisualArgs {
                    kind: VisualKind::Block,
                },
            );
        }
        // In normal mode vim wins ctrl keys (§10).
        VimKey::Ctrl('r') if pending.operator.is_none() && !pending.visual => {
            pending.end_count();
            return invocation(
                "vim.redo",
                CountArgs {
                    count: pending.count,
                },
            );
        }
        _ => return pending.reject(key),
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
        'z' if pending.operator.is_none() && feature::has_z() => Prefix::Z,
        'Z' if pending.operator.is_none() && !pending.visual && feature::has_big_z() => {
            Prefix::BigZ
        }
        '[' | ']' if pending.operator.is_none() && feature::has_bracket() => {
            Prefix::Bracket { forward: ch == ']' }
        }
        'f' => Prefix::Find(Motion::FindForward(' ')),
        'F' => Prefix::Find(Motion::FindBackward(' ')),
        't' => Prefix::Find(Motion::TillForward(' ')),
        'T' => Prefix::Find(Motion::TillBackward(' ')),
        'i' | 'a' if pending.operator.is_some() || pending.visual => {
            Prefix::Object { inner: ch == 'i' }
        }
        'r' if pending.operator.is_none() => Prefix::Replace,
        'm' if pending.operator.is_none() => Prefix::SetMark,
        '\'' | '`' => Prefix::Mark { exact: ch == '`' },
        ':' if pending.operator.is_none() => return pending.ex_line(),
        // In normal mode only; a count before `q` cancels.
        'q' if pending.is_empty() && !pending.visual => Prefix::Record,
        '@' if pending.operator.is_none() && !pending.visual => Prefix::Play,
        // A register before it is dropped.
        'Q' if pending.operator.is_none() && !pending.visual => {
            pending.end_count();
            return invocation(
                "vim.play",
                PlayArgs {
                    register: 'Q',
                    count: pending.count,
                },
            );
        }
        _ if pending.visual => return pending.visual_key(ch, key),
        'v' | 'V' if pending.at_most_register() => return visual(ch),
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
            // The rest stand alone: after an operator they cancel.
            if pending.operator.is_some() {
                return pending.reject(key);
            }
            if let Some((operator, motion)) = shorthand(ch) {
                pending.operator = Some(operator);
                return pending.commit(Target::Motion(motion));
            }
            pending.end_count();
            if let Some(at) = insert_at(ch) {
                return invocation(
                    "vim.insert",
                    InsertArgs {
                        at,
                        count: pending.count,
                    },
                );
            }
            let counted = match ch {
                'J' => Some("vim.join"),
                'R' => Some("vim.replace_mode"),
                '~' => Some("vim.toggle_case"),
                'u' => Some("vim.undo"),
                '.' => Some("vim.repeat"),
                _ => None,
            };
            if let Some(command) = counted {
                return invocation(
                    command,
                    CountArgs {
                        count: pending.count,
                    },
                );
            }
            return match ch {
                // nvim's default mapping, `:&&`: the count is lines.
                '&' => invocation(
                    "vim.repeat_substitute",
                    CountArgs {
                        count: pending.count,
                    },
                ),
                'p' | 'P' => invocation(
                    "vim.put",
                    PutArgs {
                        before: ch == 'P',
                        count: pending.count,
                        register: pending.register,
                    },
                ),
                _ => pending.reject(key),
            };
        }
    };
    pending.end_count();
    pending.prefix = Some(prefix);
    Step::Wait(pending)
}

impl Pending {
    /// A key in a visual mode that isn't a prefix: an operator acts on the
    /// selection at once.
    fn visual_key(mut self, ch: char, key: VimKey) -> Step {
        use Operator::*;
        let (operator, target) = match ch {
            // A count or register is dropped.
            'v' | 'V' => return visual(ch),
            'o' => return invocation("vim.visual_swap", ()),
            'O' => return invocation("vim.visual_swap", SwapArgs { same_row: true }),
            'd' | 'x' => (Delete, Target::Selection),
            'y' => (Yank, Target::Selection),
            'c' | 's' => (Change, Target::Selection),
            '>' => (Indent, Target::Selection),
            '<' => (Outdent, Target::Selection),
            '~' => (ToggleCase, Target::Selection),
            'u' => (Lowercase, Target::Selection),
            'U' => (Uppercase, Target::Selection),
            'J' => (Join, Target::Selection),
            'D' if self.block => (Delete, Target::SelectedToEnd),
            'C' if self.block => (Change, Target::SelectedToEnd),
            'X' | 'D' => (Delete, Target::SelectedLines),
            'Y' => (Yank, Target::SelectedLines),
            'C' | 'S' | 'R' => (Change, Target::SelectedLines),
            'I' if self.block => (Insert, Target::Selection),
            'A' if self.block => (Append, Target::Selection),
            'p' | 'P' => {
                self.end_count();
                return invocation(
                    "vim.visual_put",
                    PutArgs {
                        before: ch == 'P',
                        count: self.count,
                        register: self.register,
                    },
                );
            }
            _ => {
                return match motion(ch) {
                    Some(motion) => self.commit(Target::Motion(motion)),
                    None => self.reject(key),
                };
            }
        };
        self.operator = Some(operator);
        self.commit(target)
    }
}

/// `vim.visual` for `v` or `V`.
fn visual(ch: char) -> Step {
    let kind = match ch {
        'v' => VisualKind::Char,
        _ => VisualKind::Line,
    };
    invocation("vim.visual", VisualArgs { kind })
}

pub(crate) fn invocation(command: &str, args: impl Serialize) -> Step {
    let command = CommandId::parse(command).expect("vim command IDs are valid");
    let args = serde_json::to_value(args).expect("vim args serialize");
    Step::Commit(Invocation::new(command).args(args).source(Source::Keyboard))
}

/// A register's name: `/` too, which a put reads and writes refuse.
fn is_register(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '"' | '-' | '_' | '+' | '*' | '/')
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
        'n' => Motion::SearchNext,
        'N' => Motion::SearchPrevious,
        '*' => Motion::SearchWordForward,
        '#' => Motion::SearchWordBackward,
        ';' => Motion::RepeatFind,
        ',' => Motion::RepeatFindReverse,
        _ => return None,
    })
}

/// `x X D` and `C s S`: `dl dh d$` and `c$ cl cc`.
fn shorthand(ch: char) -> Option<(Operator, Motion)> {
    Some(match ch {
        'x' => (Operator::Delete, Motion::Right),
        'X' => (Operator::Delete, Motion::Left),
        'D' => (Operator::Delete, Motion::LineEnd),
        'C' => (Operator::Change, Motion::LineEnd),
        's' => (Operator::Change, Motion::Right),
        'S' => (Operator::Change, Motion::Line),
        _ => return None,
    })
}

/// `i a I A o O`.
fn insert_at(ch: char) -> Option<InsertAt> {
    Some(match ch {
        'i' => InsertAt::Before,
        'a' => InsertAt::After,
        'I' => InsertAt::FirstNonBlank,
        'A' => InsertAt::LineEnd,
        'o' => InsertAt::LineBelow,
        'O' => InsertAt::LineAbove,
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
