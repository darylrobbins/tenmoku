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
/// are in with no count, carrying what was pending. With a `count` it's the
/// counted motion.
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

/// `gs` or `gS`, the chars typed so far: while labels wait, both.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct JumpPrefix {
    forward: bool,
    first: Option<char>,
    query: Query,
}

impl JumpPrefix {
    fn new(forward: bool) -> Self {
        Self {
            forward,
            first: None,
            query: Query::default(),
        }
    }

    /// The next typed char: the prefix still waiting for the second, or
    /// the motion once both are in.
    pub(crate) fn then(self, ch: char) -> ControlFlow<Motion, Self> {
        match self.first {
            None => ControlFlow::Continue(Self {
                first: Some(ch),
                query: self.query.push(ch),
                ..self
            }),
            Some(first) => ControlFlow::Break(Motion::Jump(JumpMotion {
                chars: JumpChars([first, ch]),
                forward: self.forward,
            })),
        }
    }

    /// The prefix whose labels wait for `jump`.
    fn labelling(jump: JumpMotion) -> Self {
        let [c1, c2] = jump.chars.0;
        Self {
            forward: jump.forward,
            first: Some(c1),
            query: Query::default().push(c1).push(c2),
        }
    }
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
            "Vim Jump",
            |target: &mut VimTarget<'_>, actor: &Actor, args: JumpArgs| run(target, actor, args),
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Jumps to a match of two chars in the view, forward or backward, as gs and gS do without a count: straight to it if it's the only one, or to the nearest with safe labels on the rest (not after an operator), otherwise labelling them all and waiting for a label. With a count it's the counted jump over the whole text. With an operator it acts on the text up to the target.",
        )
        .context(
            ContextPredicate::parse("editor && (vim.mode == normal || vim.visual)")
                .expect("vim's context predicate parses"),
        )
        .mutates_buffer(true);
}

/// The jump's state on a [`VimState`].
#[derive(Clone, Debug, Default)]
pub(crate) struct Jumps {
    /// Labels waiting.
    session: Option<Session>,
    /// The last jump's chars and direction, for `<CR>` as `{c1}`.
    last: Option<JumpMotion>,
    /// `s` and `S` start the jump.
    on_s: bool,
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
    /// After an autojump, where the jump started and the count it went.
    start: Option<(Start, u32)>,
}

/// Labels waiting for a key.
#[derive(Clone, Debug)]
struct Session {
    jump: JumpMotion,
    register: Option<char>,
    operator: Option<Operator>,
    /// The targets in the view, nearest first: each one's offset, its count
    /// from where the jump started, and its label.
    targets: Vec<(usize, u32, Option<Label>)>,
    page: usize,
    pages: usize,
    /// After an autojump, where the jump started.
    autojumped: Option<Start>,
    /// What the labels hold for (after an autojump, with the caret on the
    /// first target).
    basis: Basis,
}

impl Session {
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
    let mut found = label::targets(text, rows, caret, jump.chars.0, jump.forward);
    found.retain(|&offset| shown(target.model, text.offset_to_point(offset).row));
    if found.is_empty() {
        target.state.failed = true;
        return Ok(Dispatched::new(Effect::Nothing));
    }
    let counts = counts(text, caret, jump, &found);
    let labeled = label::label(found.len(), args.operator.is_some());
    let pages = labeled.labels.iter().flatten().map(|l| l.page + 1).max();
    let mut session = Session {
        jump,
        register: args.register,
        operator: args.operator,
        targets: (found.into_iter().zip(counts))
            .zip(labeled.labels)
            .map(|((offset, count), label)| (offset, count, label))
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
    let first = session.targets[0].1;
    target.state.macros.jump_landed(Some(first), jump);
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

/// Each of `found` (matches of `jump` from `caret`, in order) as the count
/// the motion takes to reach it.
fn counts(text: &Text, caret: usize, jump: JumpMotion, found: &[usize]) -> Vec<u32> {
    let mut counts = Vec::with_capacity(found.len());
    let all = label::matches(text, 0..text.row_count(), caret, jump.chars.0, jump.forward);
    for (ix, offset) in all.enumerate() {
        if counts.len() == found.len() {
            break;
        }
        if offset == found[counts.len()] {
            counts.push(u32::try_from(ix + 1).unwrap_or(u32::MAX));
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
        VimKey::Char(ch) => session.targets.iter().find(|(_, _, label)| {
            *label
                == Some(Label {
                    ch,
                    page: session.page,
                })
        }),
        _ => None,
    };
    if let Some(&(_, count, _)) = picked {
        state.jump.picked = Some(Picked {
            jump: session.jump,
            count,
            basis: session.basis,
            start: session
                .autojumped
                .map(|start| (start, session.targets[0].1)),
        });
        let motion = Motion::Jump(session.jump);
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
/// and `S` starting it when opted in, `<CR>` as `{c1}`, and `{c2}`, which
/// commits `vim.jump` with no count (with one, the motion, as `step`
/// commits it). Anything else comes back.
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
    let (prefix, c2) = match (prefix.first, key) {
        (None, VimKey::Enter) => match state.jump.last {
            Some(last) => {
                let [c1, c2] = last.chars.0;
                let prefix = JumpPrefix {
                    forward: last.forward,
                    first: Some(c1),
                    query: Query::default().push(c1),
                };
                (prefix, c2)
            }
            None => return Ok(Step::Cancel),
        },
        (Some(_), VimKey::Char(ch)) => (prefix, ch),
        _ => return Err(pending),
    };
    let ControlFlow::Break(Motion::Jump(jump)) = prefix.then(c2) else {
        unreachable!("a jump with its first char takes the second");
    };
    state.jump.last = Some(jump);
    if pending.count().is_some() {
        state.macros.jump_landed(None, jump);
        return Ok(crate::step(pending.jumping(prefix), VimKey::Char(c2)));
    }
    Ok(invocation(
        "vim.jump",
        JumpArgs {
            chars: jump.chars,
            forward: jump.forward,
            count: None,
            register: pending.register(),
            operator: pending.operator(),
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
            Some((_, first)) => target.state.macros.jump_landed(Some(*first), picked.jump),
            None => target.state.macros.jump_cancelled(),
        }
        return Some(target.state.fail("jump: the labels are stale"));
    }
    target
        .state
        .macros
        .jump_landed(Some(picked.count), picked.jump);
    if let Some((start, _)) = picked.start {
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
    label::matches(text, 0..text.row_count(), caret, jump.chars.0, jump.forward).nth(n - 1)
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
    /// After `{c1}`: the label the match would get once `{c2}` is typed.
    Preview,
    /// After `{c1}`: the match typing its `{c2}` jumps straight to; its
    /// `text` is empty.
    Direct,
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

    /// The labels a host paints in `rows` of `model`'s text while a jump is
    /// pending: after `{c1}`, the label each match would get in its
    /// `{c2}` group ([`label::preview`]) over `rows`, which are then the
    /// view's [`view_rows`] for the labels to be the session's, or a direct marker on the one
    /// that `{c2}` jumps straight to; while labels wait, the current
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
                .filter(|&&(offset, _, _)| in_rows(offset))
                .filter_map(|&(offset, _, label)| {
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
            (None, Some(c1)) => {
                let caret = caret(model, self);
                let op = self.pending.has_operator();
                let shown = |row| shown(model, row);
                label::preview(text, rows, &shown, caret, c1, prefix.forward, op)
                    .into_iter()
                    .map(|(offset, label)| match label {
                        Some(label) => JumpLabel {
                            offset,
                            text: label.ch.to_string(),
                            state: JumpLabelState::Preview,
                        },
                        None => JumpLabel {
                            offset,
                            text: String::new(),
                            state: JumpLabelState::Direct,
                        },
                    })
                    .collect()
            }
            (None, None) => Vec::new(),
        };
        let mut overlay = JumpOverlay { labels };
        overlay.labels.sort_by_key(|label| label.offset);
        Some(overlay)
    }

    /// The chars typed into a pending jump (`""`, `"a"`, then `"ab"` while
    /// labels wait), for the host to show; `None` with none pending.
    pub fn jump_query(&self) -> Option<&str> {
        self.pending.jump().map(|prefix| prefix.query.as_str())
    }
}
