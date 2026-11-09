//! Jump: `gs{c1}{c2}` and `gS{c1}{c2}`, the two-char jump, as a motion and
//! as a labelled session (a feature module; see [`crate::feature`]).
//!
//! In normal mode, the visual modes and after an operator, `gs` and `gS`
//! (with whatever count, register or operator is pending) wait for two chars
//! (`vim.pending_kind` `jump`; `<Esc>` cancels with nothing changed). With a
//! count they then commit `vim.move` (extending in a visual mode) or
//! `vim.operate` with the motion `{"jump": {"chars": "ab", "forward":
//! true}}` ([`JumpMotion`]) and the count. It goes to the `count`th match
//! after the caret (`gs`), or before it (`gS`), over the whole text with no
//! wrap, landing on `{c1}`; [`label::targets`] says what matches. With no
//! such match it fails as a failed motion does: nothing changes, not even
//! the jump mark.
//!
//! As an operator's target `gs` is inclusive and `gS` exclusive, as `f` and
//! `F` are, and a delete writes register `1` even within a row, as after
//! `` ` ``. Both are jumps, setting `''`. `.` finds the match again from
//! the caret it repeats at.
//!
//! # The labelled session
//!
//! Without a count, [`VimState::feed`] commits `vim.jump` ([`JumpArgs`])
//! instead. It finds the targets in the view's rows ([`view_rows`] of
//! [`EditorHost::viewport`], or of `page_rows() + 2` rows from the first
//! where the host doesn't know it, as `vim.scroll` takes it); matches
//! outside them, or in rows a fold hides, get no label.
//!
//! - No target: it fails as the motion does.
//! - One target, or an autojump (not after an operator, the targets after
//!   the first all fitting [`label::SAFE`]): it runs the counted motion to
//!   the first, as a committed `vim.move` or `vim.operate` does, so `.`,
//!   registers and marks see a counted jump.
//! - Otherwise the caret stays and the labels ([`label::label`]) wait, with
//!   the jump still pending (`vim.pending_kind` `jump`), so the host routes
//!   every key here; a click's `<Esc>` ends it.
//!
//! While labels wait, [`VimState::feed`] reads the key first:
//!
//! - A label commits `vim.move` (extending in a visual mode) or
//!   `vim.operate` with the motion, the pending register and as count the
//!   target's index from where the jump started: after an autojump, the
//!   label's jump goes from there too, so `''` is where the jump started.
//! - `<Space>` and `<BS>` page through the labels when there are more
//!   targets than labels, round from the last page to the first.
//! - `<Esc>` ends it: after an autojump the caret stays on the first target,
//!   otherwise nothing changes.
//! - Any other key, after an autojump, ends it and is read as itself in
//!   normal or a visual mode; otherwise it cancels.
//!
//! `<CR>` as `{c1}` jumps with the last jump's chars, in its direction.
//!
//! With [`VimState::set_jump_on_s`], `s` and `S` (no prefix pending, in
//! normal mode, a visual mode or after an operator) start the jump as `gs`
//! and `gS` do. Macros record the jump resolved ([`crate::macros`]).
//!
//! # The flash style
//!
//! With [`VimState::set_jump_style`] [`JumpStyle::Flash`], as flash.nvim
//! does, the jump takes one char, `gs{c1}` (or `s{c1}`), and the motion
//! matches that alone ([`label::targets`]). Without a count `gs` commits
//! `vim.jump` with `both`: the targets are the matches before and after
//! the caret ([`label::around`]), nearest first; `gS` takes those before it
//! only. One target it goes to at once; more, [`label::flash`] labels them
//! all (no autojump) and they wait as above. A label commits the motion in
//! its target's direction, counted from where the jump started, so `.`,
//! registers, marks and a recording see `{count}gs{c1}` after the caret or
//! `{count}gS{c1}` before it, which replays in the flash style.
//!
//! The host paints [`VimState::jump_overlay`] and shows
//! [`VimState::jump_query`].

pub mod label;

#[cfg(test)]
mod rows_tests;

use std::ops::{ControlFlow, Range};

use fe_buffer::{ResolvedSelection, TransactionId, Version};
use fe_command::{Actor, CommandError, ContextPredicate, Dispatched, Effect, Registry};
use fe_editor::{EditorModel, commands::Viewport};
use fe_text::Text;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use kiln_l10n::tr;

use crate::{
    Mode, MoveArgs, OperateArgs, Operator, Pending, PendingKind, Step, Target, VimCommands, VimKey,
    VimState, VimTarget,
    args::{JumpChars, JumpMotion, Motion},
    commands,
    feature::Feature,
    motion::Cursor,
    operator,
    reducer::invocation,
    scroll,
    state::Goals,
    visual::Visual,
};

use label::Label;

type Handled = Result<Dispatched<TransactionId>, CommandError>;

pub(crate) const FEATURE: Feature = Feature {
    g,
    register,
    ..Feature::NONE
};

/// `vim.jump`'s args: the jump [`VimState::feed`] commits once both chars
/// (one in the flash style) are in with no count, carrying what was
/// pending. With a `count` it's the counted motion. With `both` and no
/// count it labels the matches before the caret too, whatever `forward`
/// is.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct JumpArgs {
    #[schemars(with = "String")]
    pub chars: JumpChars,
    pub forward: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub register: Option<char>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator: Option<Operator>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub both: bool,
}

impl JumpArgs {
    fn motion(&self) -> JumpMotion {
        JumpMotion {
            chars: self.chars,
            forward: self.forward,
        }
    }
}

/// The chars typed into a jump, as text: none, `{c1}`, or both.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
struct Query {
    bytes: [u8; 8],
    len: u8,
}

impl Query {
    fn push(mut self, ch: char) -> Self {
        let len = self.len as usize;
        self.len += ch.encode_utf8(&mut self.bytes[len..]).len() as u8;
        self
    }

    fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len as usize]).expect("chars encode as UTF-8")
    }
}

/// `gs` or `gS`, the chars typed so far: while labels wait, all of them.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct JumpPrefix {
    forward: bool,
    /// The jump takes one char (the flash style's), which [`key`] decides
    /// as the char comes, the trigger not knowing the style.
    one: bool,
    first: Option<char>,
    query: Query,
}

impl JumpPrefix {
    fn new(forward: bool) -> Self {
        Self {
            forward,
            one: false,
            first: None,
            query: Query::default(),
        }
    }

    /// The next typed char: the prefix still waiting for the second, or
    /// the motion once all are in.
    pub(crate) fn then(self, ch: char) -> ControlFlow<Motion, Self> {
        let chars = match (self.one, self.first) {
            (true, _) => JumpChars::one(ch),
            (false, None) => {
                return ControlFlow::Continue(Self {
                    first: Some(ch),
                    query: self.query.push(ch),
                    ..self
                });
            }
            (false, Some(first)) => JumpChars::pair(first, ch),
        };
        ControlFlow::Break(Motion::Jump(JumpMotion {
            chars,
            forward: self.forward,
        }))
    }

    /// The prefix whose labels wait for `jump`.
    fn labelling(jump: JumpMotion) -> Self {
        Self {
            forward: jump.forward,
            one: jump.chars.second.is_none(),
            first: Some(jump.chars.first),
            query: jump.chars.iter().fold(Query::default(), Query::push),
        }
    }

    /// The prefix that, given the last char of `jump`, is `jump`.
    fn before_last(jump: JumpMotion) -> (Self, char) {
        let prefix = Self::new(jump.forward);
        match jump.chars.second {
            Some(c2) => {
                let first = jump.chars.first;
                let prefix = Self {
                    first: Some(first),
                    query: prefix.query.push(first),
                    ..prefix
                };
                (prefix, c2)
            }
            None => (
                Self {
                    one: true,
                    ..prefix
                },
                jump.chars.first,
            ),
        }
    }
}

/// How the jump reads its target ([`VimState::set_jump_style`]).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum JumpStyle {
    /// Two chars, then labels, as leap.nvim does.
    #[default]
    Leap,
    /// One char, then labels on every match in the view, both ways, as
    /// flash.nvim does.
    Flash,
}

/// `s` and `S` after `g`.
fn g(pending: &Pending, key: VimKey) -> Option<Step> {
    let forward = match key {
        VimKey::Char('s') => true,
        VimKey::Char('S') => false,
        _ => return None,
    };
    Some(Step::Wait(
        pending.clone().jumping(JumpPrefix::new(forward)),
    ))
}

fn register(registry: &mut Registry<VimCommands>) {
    registry
        .register_with_args(
            "vim.jump",
            &crate::l10n::command_title("vim.jump"),
            |target: &mut VimTarget<'_>, actor: &Actor, args: JumpArgs| run(target, actor, args),
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.jump"))
        .context(
            ContextPredicate::parse(kiln_l10n::verbatim!(
                "editor && (vim.mode == normal || vim.visual)"
            ))
            .expect("vim's context predicate parses"),
        )
        .mutates_buffer(true);
}

/// The jump's state on a [`VimState`].
#[derive(Clone, Debug, Default)]
pub(crate) struct Jumps {
    /// Labels waiting.
    session: Option<Session>,
    /// The last jump's chars and direction, and whether it labelled both
    /// ways, for `<CR>` as `{c1}`.
    last: Option<(JumpMotion, bool)>,
    /// `s` and `S` start the jump.
    on_s: bool,
    style: JumpStyle,
    /// The label just picked: set as it commits, taken by the motion it
    /// commits ([`check_pick`]).
    picked: Option<Picked>,
}

/// A picked label, for the motion it commits to check.
#[derive(Clone, Debug)]
struct Picked {
    jump: JumpMotion,
    count: u32,
    /// What the labels were found for.
    basis: Basis,
    /// After an autojump, where the jump started and the jump it went.
    start: Option<(Start, u32, JumpMotion)>,
}

/// A session's target.
#[derive(Copy, Clone, Debug)]
struct Labelled {
    offset: usize,
    /// Its count from where the jump started, in its direction.
    count: u32,
    /// After the caret (the flash style labels both ways).
    forward: bool,
    label: Option<Label>,
}

/// Labels waiting for a key.
#[derive(Clone, Debug)]
struct Session {
    jump: JumpMotion,
    register: Option<char>,
    operator: Option<Operator>,
    /// The targets in the view, nearest first.
    targets: Vec<Labelled>,
    page: usize,
    pages: usize,
    /// After an autojump, where the jump started.
    autojumped: Option<Start>,
    /// What the labels hold for (after an autojump, with the caret on the
    /// first target).
    basis: Basis,
}

impl Session {
    /// The motion that reaches `target`, and its count.
    fn reach(&self, target: &Labelled) -> (JumpMotion, u32) {
        let jump = JumpMotion {
            forward: target.forward,
            ..self.jump
        };
        (jump, target.count)
    }

    /// Whether the labels still name what they show in `model`.
    fn holds(&self, model: &EditorModel, state: &VimState) -> bool {
        Basis::now(model, state) == self.basis
    }
}

/// What a session's labels were found for: the text's version, the caret
/// the jump goes from and every selection. Once any of them changes
/// (another origin's edit, a caret added, removed or moved), the labels
/// may not name what they show: they're stale.
#[derive(Clone, Debug, PartialEq)]
struct Basis {
    version: Version,
    caret: usize,
    selections: Vec<ResolvedSelection>,
}

impl Basis {
    fn now(model: &EditorModel, state: &VimState) -> Self {
        Self {
            version: model.snapshot().version(),
            caret: caret(model, state),
            selections: model.selections(),
        }
    }
}

/// The carets (or visual ends) and goal columns a jump started from.
#[derive(Clone, Debug)]
struct Start {
    selections: Vec<ResolvedSelection>,
    visual: Option<Visual>,
    goals: Option<(Version, Goals)>,
}

impl Start {
    fn save(target: &VimTarget<'_>) -> Self {
        Self {
            selections: target.model.selections(),
            visual: target.state.visual.clone(),
            goals: target.state.goals.clone(),
        }
    }
}

/// `vim.jump`.
fn run(target: &mut VimTarget<'_>, actor: &Actor, args: JumpArgs) -> Handled {
    if let Some(count) = args.count {
        return counted(target, actor, args, count);
    }
    let jump = args.motion();
    target.model.sync_display();
    let caret = caret(target.model, target.state);
    let view = scroll::viewport(&*target.host);
    let rows = view_rows(target.model, view, target.host.top_display_row());
    let text = target.model.text();
    let mut found = match args.both {
        true => label::around(text, rows, caret, jump.chars),
        false => label::targets(text, rows, caret, jump.chars, jump.forward),
    };
    found.retain(|&offset| shown(target.model, text.offset_to_point(offset).row));
    if found.is_empty() {
        target.state.failed = true;
        return Ok(Dispatched::new(Effect::Nothing));
    }
    let counts = counts(text, caret, jump.chars, &found);
    let labeled = match jump.chars.second {
        Some(_) => label::label(found.len(), args.operator.is_some()),
        None => label::flash(found.len()),
    };
    let pages = labeled.labels.iter().flatten().map(|l| l.page + 1).max();
    let mut session = Session {
        jump,
        register: args.register,
        operator: args.operator,
        targets: (found.into_iter().zip(counts))
            .zip(labeled.labels)
            .map(|((offset, count), label)| Labelled {
                offset,
                count,
                forward: offset > caret,
                label,
            })
            .collect(),
        page: 0,
        pages: pages.unwrap_or(1),
        autojumped: None,
        basis: Basis::now(target.model, target.state),
    };
    if !labeled.autojump {
        wait(target.state, session);
        return Ok(Dispatched::new(Effect::Notify));
    }
    let start = Start::save(target);
    let (first_jump, first) = session.reach(&session.targets[0]);
    target.state.macros.jump_landed(Some(first), first_jump);
    let args = JumpArgs {
        forward: first_jump.forward,
        ..args
    };
    let landed = counted(target, actor, args, first)?;
    if session.targets.len() > 1 {
        target.model.sync_display();
        session.autojumped = Some(start);
        session.basis = Basis::now(target.model, target.state);
        wait(target.state, session);
    }
    Ok(landed)
}

/// The text's rows a jump looks for targets in when `view` is what the view
/// shows: as many display rows as it has from `top_display_row`, where it
/// starts (partway down its top row if that's soft-wrapped; if `None` or
/// not on that row, the row's first), so a fold or a soft-wrapped row in
/// view takes one display row each, not one per text row. A host shows
/// labels over these rows ([`VimState::jump_overlay`]), so the letters
/// after `{c1}` are the labels after `{c2}`.
pub fn view_rows(model: &EditorModel, view: Viewport, top_display_row: Option<u32>) -> Range<u32> {
    let row_count = model.text().row_count();
    let top = view.top_row.min(row_count);
    if view.rows == 0 || top == row_count {
        return top..top;
    }
    let display = model.display();
    let first = top_display_row
        .filter(|&row| row < display.row_count() && display.row(row).buffer_row == top)
        .unwrap_or_else(|| display.row_for_buffer_row(top));
    let bottom = first.saturating_add(view.rows - 1);
    let end = match bottom < display.row_count() {
        true => display.row(bottom).buffer_row + 1,
        false => row_count,
    };
    top..end.max(top)
}

/// Whether text row `row` shows: one a fold hides takes no label.
fn shown(model: &EditorModel, row: u32) -> bool {
    model.display().hidden_rows(row).is_none()
}

/// Runs the jump as the motion counted `count`, through the code a
/// committed `vim.move` or `vim.operate` runs.
fn counted(target: &mut VimTarget<'_>, actor: &Actor, args: JumpArgs, count: u32) -> Handled {
    let motion = Motion::Jump(args.motion());
    match args.operator {
        Some(operator) => commands::run_operate(
            target,
            actor,
            OperateArgs {
                operator,
                target: Target::Motion(motion),
                count: Some(count),
                register: args.register,
            },
        ),
        None => {
            let extend = matches!(target.state.mode, Mode::Visual(_));
            let args = MoveArgs {
                motion,
                extend,
                count: Some(count),
            };
            Ok(commands::run_move(target, args))
        }
    }
}

/// Leaves `session`'s labels waiting, the jump pending.
fn wait(state: &mut VimState, session: Session) {
    let prefix = JumpPrefix::labelling(session.jump);
    state.pending = Pending::jump_labels(session.register, session.operator, prefix);
    state.jump.session = Some(session);
}

/// Each of `found` (matches of `chars` from `caret`, in any order, either
/// side of it) as the count the motion takes to reach it in its direction.
fn counts(text: &Text, caret: usize, chars: JumpChars, found: &[usize]) -> Vec<u32> {
    let mut counts = vec![u32::MAX; found.len()];
    for forward in [true, false] {
        let mut side: Vec<(usize, usize)> = (found.iter().enumerate())
            .filter(|&(_, &offset)| (offset > caret) == forward)
            .map(|(ix, &offset)| (offset, ix))
            .collect();
        // Nearest first in this direction, as the matches come.
        match forward {
            true => side.sort_unstable(),
            false => side.sort_unstable_by(|a, b| b.cmp(a)),
        }
        let mut side = side.into_iter().peekable();
        let all = label::matches(text, 0..text.row_count(), caret, chars, forward);
        for (n, offset) in all.enumerate() {
            let Some(&(next, ix)) = side.peek() else {
                break;
            };
            if offset == next {
                counts[ix] = u32::try_from(n + 1).unwrap_or(u32::MAX);
                side.next();
            }
        }
    }
    counts
}

/// While labels wait, the key [`VimState::feed`] reads before anything
/// else: what it did, or `None` for a key that isn't the session's (with
/// none waiting, or after an autojump, which it then ends).
pub(crate) fn label_key(state: &mut VimState, key: VimKey) -> Option<Step> {
    // A pick is for the key that made it only.
    state.jump.picked = None;
    let mut session = state.jump.session.take()?;
    // Something else ended the jump.
    state.pending.jump()?;
    if session.pages > 1 && matches!(key, VimKey::Char(' ') | VimKey::Backspace) {
        let by = match key {
            VimKey::Backspace => session.pages - 1,
            _ => 1,
        };
        session.page = (session.page + by) % session.pages;
        state.jump.session = Some(session);
        return Some(Step::Wait(state.pending.clone()));
    }
    state.pending = Pending::new();
    let picked = match key {
        VimKey::Char(ch) => session.targets.iter().find(|target| {
            target.label
                == Some(Label {
                    ch,
                    page: session.page,
                })
        }),
        _ => None,
    };
    if let Some(picked) = picked {
        let (jump, count) = session.reach(picked);
        let (first_jump, first) = session.reach(&session.targets[0]);
        state.jump.picked = Some(Picked {
            jump,
            count,
            basis: session.basis.clone(),
            start: (session.autojumped.take()).map(|start| (start, first, first_jump)),
        });
        let motion = Motion::Jump(jump);
        return Some(match session.operator {
            Some(operator) => invocation(
                "vim.operate",
                OperateArgs {
                    operator,
                    target: Target::Motion(motion),
                    count: Some(count),
                    register: session.register,
                },
            ),
            None => invocation(
                "vim.move",
                MoveArgs {
                    motion,
                    extend: matches!(state.mode, Mode::Visual(_)),
                    count: Some(count),
                },
            ),
        });
    }
    match (session.autojumped, key) {
        (Some(_), VimKey::Esc) => Some(Step::Cancel),
        (Some(_), _) => None,
        (None, key) => {
            state.macros.jump_cancelled();
            state.failed = key != VimKey::Esc;
            Some(Step::Cancel)
        }
    }
}

/// Keys [`VimState::feed`] reads for the jump before [`crate::step`]: `s`
/// and `S` starting it when opted in, `<CR>` as `{c1}`, and `{c2}` (`{c1}`
/// in the flash style), which commits `vim.jump` with no count (with one,
/// the motion, as `step` commits it). Anything else comes back.
pub(crate) fn key(state: &mut VimState, pending: Pending, key: VimKey) -> Result<Step, Pending> {
    let Some(&prefix) = pending.jump() else {
        let forward = match key {
            VimKey::Char('s') => true,
            VimKey::Char('S') => false,
            _ => return Err(pending),
        };
        if !state.jump.on_s || pending.has_prefix() {
            return Err(pending);
        }
        return Ok(Step::Wait(pending.jumping(JumpPrefix::new(forward))));
    };
    let flash = state.jump.style == JumpStyle::Flash;
    let ((prefix, last), both) = match (prefix.first, key) {
        (None, VimKey::Enter) => match state.jump.last {
            Some((last, both)) => (JumpPrefix::before_last(last), both),
            None => return Ok(Step::Cancel),
        },
        // `gs` labels both ways, `gS` before the caret only.
        (None, VimKey::Char(ch)) if flash => (
            (
                JumpPrefix {
                    one: true,
                    ..prefix
                },
                ch,
            ),
            prefix.forward,
        ),
        (Some(_), VimKey::Char(ch)) => ((prefix, ch), false),
        _ => return Err(pending),
    };
    let ControlFlow::Break(Motion::Jump(jump)) = prefix.then(last) else {
        unreachable!("a jump with all but its last char takes the last");
    };
    state.jump.last = Some((jump, both));
    if pending.count().is_some() {
        state.macros.jump_landed(None, jump);
        return Ok(crate::step(pending.jumping(prefix), VimKey::Char(last)));
    }
    Ok(invocation(
        "vim.jump",
        JumpArgs {
            chars: jump.chars,
            forward: jump.forward,
            count: None,
            register: pending.register(),
            operator: pending.operator(),
            both,
        },
    ))
}

/// After [`VimState::feed`] read a key with `before` pending: a trigger
/// just typed marks where the jump's keys begin in a recording, and a
/// cancelled jump takes them back out.
pub(crate) fn fed(state: &mut VimState, before: Option<PendingKind>, result: &Step) {
    match result {
        Step::Wait(pending)
            if before != Some(PendingKind::Jump)
                && pending.jump().is_some_and(|jump| jump.first.is_none()) =>
        {
            let trigger = match before {
                Some(PendingKind::G) => 2,
                _ => 1,
            };
            state.macros.jump_start(trigger);
        }
        Step::Cancel if before == Some(PendingKind::Jump) => state.macros.jump_cancelled(),
        _ => {}
    }
}

/// Before a `vim.move` or `vim.operate` runs: if a label was just picked,
/// checks that the text, the caret and every selection are still those its
/// labels were found for ([`Basis`]). If any changed (another origin's edit,
/// or a caret added, removed or moved), the labels
/// may not name what they showed, so it fails with nothing changed,
/// returning what the command returns: a recording keeps the autojump, if
/// there was one, and otherwise none of the jump's keys.
/// Otherwise the jump is recorded as landed, and after an autojump the
/// carets go back where the jump started, so the label's count runs from
/// there.
pub(crate) fn check_pick(target: &mut VimTarget<'_>) -> Option<Dispatched<TransactionId>> {
    let picked = target.state.jump.picked.take()?;
    target.model.sync_display();
    if Basis::now(target.model, target.state) != picked.basis {
        match &picked.start {
            Some((_, first, jump)) => target.state.macros.jump_landed(Some(*first), *jump),
            None => target.state.macros.jump_cancelled(),
        }
        return Some(target.state.fail(tr!("vim-jump-labels-stale")));
    }
    target
        .state
        .macros
        .jump_landed(Some(picked.count), picked.jump);
    if let Some((start, _, _)) = picked.start {
        target.model.set_selections(start.selections);
        target.state.goals = start.goals;
        if start.visual.is_some() {
            target.state.visual = start.visual;
            target.state.sync_visual(target.model);
        }
    }
    None
}

/// Where a jump starts: the vim cursor, or the primary caret.
fn caret(model: &EditorModel, state: &VimState) -> usize {
    let visual = state.visual_cursor(model);
    let head = visual.unwrap_or_else(|| model.primary_selection().head());
    let text = model.text();
    let cursor = Cursor::at(text, head);
    // A visual cursor at a row's end (after `$`) jumps from there.
    match visual.is_some() && head == operator::row_end(text, cursor.row) {
        true => head,
        false => cursor.offset(),
    }
}

/// The `count`th match of `jump` from `caret`, over the whole text.
pub(crate) fn nth(
    text: &Text,
    caret: usize,
    jump: JumpMotion,
    count: Option<u32>,
) -> Option<usize> {
    let n = count.unwrap_or(1).max(1) as usize;
    label::matches(text, 0..text.row_count(), caret, jump.chars, jump.forward).nth(n - 1)
}

/// Whether `motion`, if it's a jump, finds its match from the vim cursor or
/// primary caret: one that doesn't goes nowhere and sets no jump mark.
pub(crate) fn lands(target: &mut VimTarget<'_>, motion: Motion, count: Option<u32>) -> bool {
    let Motion::Jump(jump) = motion else {
        return true;
    };
    target.model.sync_display();
    let caret = caret(target.model, target.state);
    nth(target.model.text(), caret, jump, count).is_some()
}

/// What a host paints for a pending jump ([`VimState::jump_overlay`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JumpOverlay {
    /// In text order.
    pub labels: Vec<JumpLabel>,
}

/// One label of a [`JumpOverlay`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JumpLabel {
    /// The byte offset of the matched `{c1}`, where the label goes.
    pub offset: usize,
    pub text: String,
    pub state: JumpLabelState,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum JumpLabelState {
    /// Waiting: typing it goes there.
    Live,
    /// On another page: `<Space>` or `<BS>` reaches it.
    Concealed,
}

impl VimState {
    /// Lets `s` and `S` start the jump forward and backward, as `gs` and
    /// `gS` do, with no prefix pending in normal mode, a visual mode or
    /// after an operator (`ds{c1}{c2}`), where they'd otherwise be `cl`,
    /// `cc`, or a visual `c` and linewise change. Off by default; `gs` and
    /// `gS` jump either way.
    pub fn set_jump_on_s(&mut self, on: bool) {
        self.jump.on_s = on;
    }

    /// How the jump reads its target from the next one on: two chars then
    /// labels ([`JumpStyle::Leap`], the default), or one char then labels
    /// on every match in the view, both ways ([`JumpStyle::Flash`]). Labels
    /// waiting go on as they started; a jump waiting for its first char
    /// reads it in the new style.
    pub fn set_jump_style(&mut self, style: JumpStyle) {
        self.jump.style = style;
    }

    /// The labels a host paints in `rows` of `model`'s text while a jump is
    /// pending: none until `{c2}` is typed (`{c1}` in the flash style);
    /// while labels wait, the current
    /// page's, then the other pages' as concealed. A target jumped to has
    /// none. `None` with no jump pending.
    pub fn jump_overlay(&self, model: &EditorModel, rows: Range<u32>) -> Option<JumpOverlay> {
        let prefix = self.pending.jump()?;
        let text = model.text();
        // Another origin's edit may have left a target past the end.
        let in_rows = |offset: usize| {
            offset <= text.len() && rows.contains(&text.offset_to_point(offset).row)
        };
        let labels = match (&self.jump.session, prefix.first) {
            // Stale labels aren't shown: picking one fails.
            (Some(session), _) if !session.holds(model, self) => Vec::new(),
            (Some(session), _) => session
                .targets
                .iter()
                .filter(|target| in_rows(target.offset))
                .filter_map(|&Labelled { offset, label, .. }| {
                    let label = label?;
                    Some(JumpLabel {
                        offset,
                        text: label.ch.to_string(),
                        state: match label.page == session.page {
                            true => JumpLabelState::Live,
                            false => JumpLabelState::Concealed,
                        },
                    })
                })
                .collect(),
            // After `{c1}` alone, nothing: no target is one keystroke away.
            (None, _) => Vec::new(),
        };
        let mut overlay = JumpOverlay { labels };
        overlay.labels.sort_by_key(|label| label.offset);
        Some(overlay)
    }

    /// The chars typed into a pending jump (`""`, `"a"`, then `"ab"` while
    /// labels wait; `"a"` while they wait in the flash style), for the host
    /// to show; `None` with none pending.
    pub fn jump_query(&self) -> Option<&str> {
        self.pending.jump().map(|prefix| prefix.query.as_str())
    }
}
