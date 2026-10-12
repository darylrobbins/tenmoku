//! A scripted fake AI: plays ghost text, a streamed edit, a proposed change
//! and touched-row decorations on an [`EditorModel`] from a script, with no
//! network and no model, so a session is reproducible. The reference for how
//! an embedding host drives fastedit's AI seams together. GPUI-free; a host
//! renders the model, paints [`ScriptedAi::ghost_text`] and
//! [`ScriptedAi::proposals`], and paces the steps.
//!
//! # Script
//!
//! One step per line, parsed by [`parse`]. Blank lines and lines starting
//! with `#` are skipped. `<text>` is the rest of the line after one space,
//! with `\n`, `\t` and `\\` unescaped. Rows and columns are 1-based, columns
//! in chars.
//!
//! | Line | Does |
//! |---|---|
//! | `wait <ms>` | nothing headless; the host pauses that long |
//! | `caret <row>:<col>` | [`EditorModel::set_cursor`] there, clamped to the row |
//! | `type <text>` | the user types: [`EditorModel::insert`] once per char, a `\n` through [`EditorModel::newline`] |
//! | `suggest <text>` | a [`GhostText`] at the caret |
//! | `accept` / `accept-word` / `accept-line` | [`GhostText::accept`] / [`accept_word`](GhostText::accept_word) / [`accept_line`](GhostText::accept_line) |
//! | `dismiss` | drops the suggestion |
//! | `stream` | [`EditStream::begin`] on the primary selection |
//! | `push <text>` | [`EditStream::push`] of that chunk |
//! | `finish` | [`EditStream::finish`] |
//! | `propose <first>-<last> <text>` | [`ProposedChange::from_text`] replacing rows `first..=last`, line breaks included, with `<text>` |
//! | `accept-hunk <n>` / `reject-hunk <n>` | the latest proposal's `n`th hunk (1-based, text order): [`ProposedChange::accept_as`] the user / [`ProposedChange::reject`] |
//! | `undo` / `redo` | [`EditorModel::undo`] / [`redo`](EditorModel::redo): the user's edits only |
//! | `undo-ai` / `redo-ai` | the buffer's undo / redo of the latest AI transaction |
//!
//! Suggestions, streams and proposals are under
//! `Origin::Ai { agent: "script", session }`, the session `suggest-<n>`,
//! `stream-<n>` or `propose-<n>` counting from 1. Accepting a suggestion or
//! a hunk is the user's edit, so the user's undo takes it back; a stream
//! stays the AI's, one transaction that only `undo-ai` reverts.
//!
//! One example per verb:
//!
//! ```text
//! # Pause half a second before the next step.
//! wait 500
//! caret 3:5
//! type let x = 1;\n
//! suggest println!("{x}");
//! accept-word
//! accept-line
//! accept
//! dismiss
//! stream
//! push fn helper() {\n
//! finish
//! propose 2-4     let a = 1;\n    let b = 2;\n    let c = 3;\n
//! accept-hunk 1
//! reject-hunk 2
//! undo
//! redo
//! undo-ai
//! redo-ai
//! ```
//!
//! [`DEMO_SCRIPT`] plays every verb against [`DEMO_TEXT`].

use std::{
    cell::RefCell, collections::HashSet, fmt, ops::Range, rc::Rc, sync::Arc, time::Duration,
};

use fe_buffer::{
    Bias, BufferEvent, EditKind, EditStream, HunkId, HunkStatus, Origin, ProposalError,
    ProposedChange, StreamError, Subscription, TransactionId, UndoError,
};

use crate::{
    EditorModel,
    decorations::{Decoration, DecorationKind, DecorationSet, Tint},
    ghost_text::GhostText,
};

/// A Rust snippet for [`DEMO_SCRIPT`] to play against.
pub const DEMO_TEXT: &str = include_str!("scripted_ai/demo_text.txt");

/// A session against [`DEMO_TEXT`] using every verb, commented step by step.
pub const DEMO_SCRIPT: &str = include_str!("scripted_ai/demo.script");

/// The agent every scripted origin names.
const AGENT: &str = "script";

/// One line of a script; see the module docs. Rows and columns are 1-based,
/// as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Wait(Duration),
    Caret { row: u32, col: u32 },
    Type(String),
    Suggest(String),
    Accept,
    AcceptWord,
    AcceptLine,
    Dismiss,
    Stream,
    Push(String),
    Finish,
    Propose { first: u32, last: u32, text: String },
    AcceptHunk(usize),
    RejectHunk(usize),
    Undo,
    Redo,
    UndoAi,
    RedoAi,
}

/// A script line that doesn't parse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptError {
    /// 1-based.
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ScriptError {}

/// Parses `script`, one [`Step`] per line that isn't blank or a `#`
/// comment.
///
/// # Errors
/// The first line that doesn't parse: an unknown verb, a missing or
/// malformed argument, or an argument to a verb that takes none.
pub fn parse(script: &str) -> Result<Vec<Step>, ScriptError> {
    script
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .map(|(ix, line)| {
            parse_line(line).map_err(|message| ScriptError {
                line: ix + 1,
                message,
            })
        })
        .collect()
}

fn parse_line(line: &str) -> Result<Step, String> {
    let (verb, given) = match line.split_once(' ') {
        Some((verb, arg)) => (verb, Some(arg)),
        None => (line, None),
    };
    let arg = |what: &str| given.ok_or_else(|| format!("`{verb}` needs {what}"));
    let none = |step: Step| match given {
        Some(_) => Err(format!("`{verb}` takes no argument")),
        None => Ok(step),
    };
    match verb {
        "wait" => Ok(Step::Wait(Duration::from_millis(number(
            arg("<ms>")?,
            "<ms>",
        )?))),
        "caret" => {
            let arg = arg("<row>:<col>")?;
            let (row, col) = arg
                .split_once(':')
                .ok_or_else(|| format!("expected <row>:<col>, got `{arg}`"))?;
            Ok(Step::Caret {
                row: position(row, "<row>")?,
                col: position(col, "<col>")?,
            })
        }
        "type" => Ok(Step::Type(unescape(arg("<text>")?)?)),
        "suggest" => Ok(Step::Suggest(unescape(arg("<text>")?)?)),
        "accept" => none(Step::Accept),
        "accept-word" => none(Step::AcceptWord),
        "accept-line" => none(Step::AcceptLine),
        "dismiss" => none(Step::Dismiss),
        "stream" => none(Step::Stream),
        "push" => Ok(Step::Push(unescape(arg("<text>")?)?)),
        "finish" => none(Step::Finish),
        "propose" => {
            let arg = arg("<first>-<last> <text>")?;
            let (rows, text) = arg
                .split_once(' ')
                .ok_or_else(|| "`propose` needs <first>-<last> <text>".to_owned())?;
            let (first, last) = rows
                .split_once('-')
                .ok_or_else(|| format!("expected <first>-<last>, got `{rows}`"))?;
            let (first, last) = (position(first, "<first>")?, position(last, "<last>")?);
            if last < first {
                return Err(format!("row range {first}-{last} ends before it starts"));
            }
            Ok(Step::Propose {
                first,
                last,
                text: unescape(text)?,
            })
        }
        "accept-hunk" => Ok(Step::AcceptHunk(position(arg("<n>")?, "<n>")?)),
        "reject-hunk" => Ok(Step::RejectHunk(position(arg("<n>")?, "<n>")?)),
        "undo" => none(Step::Undo),
        "redo" => none(Step::Redo),
        "undo-ai" => none(Step::UndoAi),
        "redo-ai" => none(Step::RedoAi),
        _ => Err(format!("unknown verb `{verb}`")),
    }
}

fn number<T: std::str::FromStr>(arg: &str, what: &str) -> Result<T, String> {
    arg.parse()
        .map_err(|_| format!("{what} must be a number, got `{arg}`"))
}

/// A 1-based number: at least 1.
fn position<T: std::str::FromStr + Default + PartialEq>(
    arg: &str,
    what: &str,
) -> Result<T, String> {
    let n: T = number(arg, what)?;
    if n == T::default() {
        return Err(format!("{what} counts from 1, got 0"));
    }
    Ok(n)
}

fn unescape(text: &str) -> Result<String, String> {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some(other) => return Err(format!("unknown escape `\\{other}`")),
            None => return Err("a `\\` ends the line".to_owned()),
        }
    }
    Ok(out)
}

/// What a step did, for the host to print: [`Display`](fmt::Display) is one
/// line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Report {
    /// The host pauses this long.
    Waited(Duration),
    /// The caret moved to this offset.
    Moved(usize),
    /// The user typed this many chars.
    Typed(usize),
    /// A suggestion of this many bytes is offered at this offset.
    Suggested {
        at: usize,
        len: usize,
    },
    /// A suggestion, or part of it, was accepted in this transaction.
    Accepted(TransactionId),
    Dismissed,
    /// A stream will replace this range.
    StreamBegun(Range<usize>),
    /// A chunk of this many bytes was pushed; the stream's region is now
    /// this.
    Pushed {
        bytes: usize,
        region: Range<usize>,
    },
    /// The stream ended, with its transaction if anything landed.
    Finished(Option<TransactionId>),
    /// A proposal of this many hunks was made.
    Proposed {
        hunks: usize,
    },
    /// A hunk landed in this transaction.
    HunkAccepted(TransactionId),
    HunkRejected,
    /// An undo (`ai`: of the AI's edits) reverted this transaction, if there
    /// was one to revert.
    Undone {
        ai: bool,
        transaction: Option<TransactionId>,
    },
    /// A redo, as [`Report::Undone`].
    Redone {
        ai: bool,
        transaction: Option<TransactionId>,
    },
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let who = |ai: bool| if ai { "the AI's" } else { "the user's" };
        match self {
            Report::Waited(pause) => write!(f, "wait {} ms", pause.as_millis()),
            Report::Moved(offset) => write!(f, "caret at offset {offset}"),
            Report::Typed(chars) => write!(f, "typed {chars} chars"),
            Report::Suggested { at, len } => write!(f, "suggested {len} bytes at offset {at}"),
            Report::Accepted(id) => write!(f, "accepted suggestion as {id:?}"),
            Report::Dismissed => write!(f, "dismissed suggestion"),
            Report::StreamBegun(range) => write!(f, "stream begun over {range:?}"),
            Report::Pushed { bytes, region } => {
                write!(f, "pushed {bytes} bytes; stream region {region:?}")
            }
            Report::Finished(Some(id)) => write!(f, "stream finished as {id:?}"),
            Report::Finished(None) => write!(f, "stream finished with nothing written"),
            Report::Proposed { hunks } => write!(f, "proposed {hunks} hunks"),
            Report::HunkAccepted(id) => write!(f, "accepted hunk as {id:?}"),
            Report::HunkRejected => write!(f, "rejected hunk"),
            Report::Undone { ai, transaction } => match transaction {
                Some(id) => write!(f, "undid {} {id:?}", who(*ai)),
                None => write!(f, "nothing of {} to undo", who(*ai)),
            },
            Report::Redone { ai, transaction } => match transaction {
                Some(id) => write!(f, "redid {} {id:?}", who(*ai)),
                None => write!(f, "nothing of {} to redo", who(*ai)),
            },
        }
    }
}

/// Why a step couldn't run. The step changed nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepError {
    /// `push` or `finish` with no stream open.
    NoStream,
    /// `stream` with one already open.
    StreamOpen,
    /// The stream can't write; it's dropped.
    Stream(StreamError),
    /// An accept or dismiss with no live suggestion.
    NoSuggestion,
    /// A hunk step before any proposal.
    NoProposal,
    /// The latest proposal has fewer hunks than this 1-based index.
    NoSuchHunk(usize),
    Proposal(ProposalError),
    /// The text has fewer rows than this 1-based row.
    RowPastEnd(u32),
    Undo(UndoError),
}

impl fmt::Display for StepError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StepError::NoStream => write!(f, "no stream is open"),
            StepError::StreamOpen => write!(f, "a stream is already open"),
            StepError::Stream(error) => write!(f, "stream dropped: {error}"),
            StepError::NoSuggestion => write!(f, "no live suggestion"),
            StepError::NoProposal => write!(f, "nothing has been proposed"),
            StepError::NoSuchHunk(n) => write!(f, "the latest proposal has no hunk {n}"),
            StepError::Proposal(error) => write!(f, "{error}"),
            StepError::RowPastEnd(row) => write!(f, "row {row} is past the end of the text"),
            StepError::Undo(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for StepError {}

/// Plays [`Step`]s on an [`EditorModel`]; see the module docs.
pub struct ScriptedAi {
    suggestion: Option<GhostText>,
    stream: Option<EditStream>,
    proposals: Vec<Rc<RefCell<ProposedChange>>>,
    /// Rows the AI wrote, or the user took from it: streamed regions and
    /// accepted hunks.
    touched: Rc<RefCell<DecorationSet>>,
    /// The transactions behind the touched rows: an undo of one takes its
    /// mark away, a redo puts it back.
    marked: HashSet<TransactionId>,
    /// The buffer's edits, to follow undos and redos of `marked`.
    events: Subscription,
    /// How many suggestions, streams and proposals have been made, for
    /// their sessions.
    suggestions: u32,
    streams: u32,
    proposed: u32,
}

impl ScriptedAi {
    /// A fake AI for `model`, with its touched rows registered as one of
    /// `model`'s decoration providers.
    pub fn new(model: &mut EditorModel) -> Self {
        let touched = Rc::new(RefCell::new(DecorationSet::new()));
        model.add_decoration_provider(touched.clone());
        let events = model.buffer_mut().subscribe();
        Self {
            suggestion: None,
            stream: None,
            proposals: Vec::new(),
            touched,
            marked: HashSet::new(),
            events,
            suggestions: 0,
            streams: 0,
            proposed: 0,
        }
    }

    /// The suggestion offered last, until it's accepted in full or
    /// dismissed. It shows only while live at the caret
    /// ([`GhostText::remaining`]).
    pub fn ghost_text(&self) -> Option<&GhostText> {
        self.suggestion.as_ref()
    }

    /// The open stream, from `stream` to `finish`.
    pub fn stream(&self) -> Option<&EditStream> {
        self.stream.as_ref()
    }

    /// Every proposal made, oldest first; hunk steps act on the last.
    pub fn proposals(&self) -> &[Rc<RefCell<ProposedChange>>] {
        &self.proposals
    }

    /// The touched rows, as registered with the model: each streamed
    /// region and accepted hunk, while its transaction isn't undone. Undos
    /// and redos the host makes itself are followed at the next step.
    pub fn touched(&self) -> Rc<RefCell<DecorationSet>> {
        self.touched.clone()
    }

    /// Runs `step` on `model`.
    ///
    /// # Errors
    /// A [`StepError`] if the step can't run; the text and the touched rows
    /// are then unchanged.
    pub fn step(&mut self, model: &mut EditorModel, step: &Step) -> Result<Report, StepError> {
        model.sync_display();
        self.catch_up(model);
        let report = self.run(model, step);
        self.catch_up(model);
        report
    }

    fn run(&mut self, model: &mut EditorModel, step: &Step) -> Result<Report, StepError> {
        match step {
            Step::Wait(pause) => Ok(Report::Waited(*pause)),
            Step::Caret { row, col } => {
                let text = model.text();
                let row0 = row.saturating_sub(1);
                if row0 >= text.row_count() {
                    return Err(StepError::RowPastEnd(*row));
                }
                let line = text.row_to_string(row0);
                let column = (line.char_indices().nth((*col as usize).saturating_sub(1)))
                    .map_or(line.len(), |(ix, _)| ix);
                let offset = text.row_start(row0) + column;
                model.set_cursor(offset);
                Ok(Report::Moved(offset))
            }
            Step::Type(text) => {
                for c in text.chars() {
                    match c {
                        '\n' => model.newline(),
                        c => model.insert(c.encode_utf8(&mut [0; 4])),
                    }
                }
                Ok(Report::Typed(text.chars().count()))
            }
            Step::Suggest(text) => {
                self.suggestions += 1;
                let at = model.primary_selection().head();
                let origin = origin(format!("suggest-{}", self.suggestions));
                self.suggestion = Some(GhostText::new(model.snapshot(), at, text.as_str(), origin));
                Ok(Report::Suggested {
                    at,
                    len: text.len(),
                })
            }
            Step::Accept => self.accept(model, GhostText::accept),
            Step::AcceptWord => self.accept(model, GhostText::accept_word),
            Step::AcceptLine => self.accept(model, GhostText::accept_line),
            Step::Dismiss => {
                self.suggestion.take().ok_or(StepError::NoSuggestion)?;
                Ok(Report::Dismissed)
            }
            Step::Stream => {
                if self.stream.is_some() {
                    return Err(StepError::StreamOpen);
                }
                self.streams += 1;
                let range = model.primary_selection().range;
                let origin = origin(format!("stream-{}", self.streams));
                self.stream = Some(EditStream::begin(&model.buffer(), range.clone(), origin));
                Ok(Report::StreamBegun(range))
            }
            Step::Push(chunk) => {
                let stream = self.stream.as_mut().ok_or(StepError::NoStream)?;
                if let Err(error) = stream.push(&mut model.buffer_mut(), chunk) {
                    self.stream = None;
                    return Err(StepError::Stream(error));
                }
                let region = stream.range(model.snapshot());
                if let Some(transaction) = stream.transaction() {
                    self.marked.insert(transaction);
                    self.mark(model, region.clone());
                }
                Ok(Report::Pushed {
                    bytes: chunk.len(),
                    region,
                })
            }
            Step::Finish => {
                let stream = self.stream.take().ok_or(StepError::NoStream)?;
                let region = stream.range(model.snapshot());
                let len = model.text().len();
                let transaction = stream.finish(&mut model.buffer_mut());
                // A held-back `\r` landed at the region's end.
                let grown = model.text().len().saturating_sub(len);
                if grown > 0 {
                    self.mark(model, region.start..region.end + grown);
                }
                Ok(Report::Finished(transaction))
            }
            Step::Propose { first, last, text } => {
                let snapshot = model.snapshot();
                let rows = snapshot.text().row_count();
                if *last > rows {
                    return Err(StepError::RowPastEnd(*last));
                }
                let start = snapshot.text().row_start(first.saturating_sub(1));
                let end = if *last < rows {
                    snapshot.text().row_start(*last)
                } else {
                    snapshot.text().len()
                };
                let end = end.max(start);
                self.proposed += 1;
                let origin = origin(format!("propose-{}", self.proposed));
                let proposal = ProposedChange::from_text(snapshot, start..end, text, origin);
                let hunks = proposal.hunks(snapshot).len();
                self.proposals.push(Rc::new(RefCell::new(proposal)));
                Ok(Report::Proposed { hunks })
            }
            Step::AcceptHunk(n) => {
                let (proposal, id) = self.hunk(model, *n)?;
                let accepted =
                    proposal
                        .borrow_mut()
                        .accept_as(&mut model.buffer_mut(), id, Origin::User);
                let transaction = accepted.map_err(StepError::Proposal)?;
                self.mark_hunk(model, id, transaction);
                Ok(Report::HunkAccepted(transaction))
            }
            Step::RejectHunk(n) => {
                let (proposal, id) = self.hunk(model, *n)?;
                proposal
                    .borrow_mut()
                    .reject(id)
                    .map_err(StepError::Proposal)?;
                Ok(Report::HunkRejected)
            }
            Step::Undo => {
                let transaction = model.undo().map_err(StepError::Undo)?;
                Ok(Report::Undone {
                    ai: false,
                    transaction,
                })
            }
            Step::Redo => {
                let transaction = model.redo().map_err(StepError::Undo)?;
                Ok(Report::Redone {
                    ai: false,
                    transaction,
                })
            }
            Step::UndoAi => {
                let transaction = model.buffer_mut().undo(Origin::is_ai);
                Ok(Report::Undone {
                    ai: true,
                    transaction: transaction.map_err(StepError::Undo)?,
                })
            }
            Step::RedoAi => {
                let transaction = model.buffer_mut().redo(Origin::is_ai);
                Ok(Report::Redone {
                    ai: true,
                    transaction: transaction.map_err(StepError::Undo)?,
                })
            }
        }
    }

    /// Marks `hunk`, which the host has just accepted, as touched where it
    /// landed. Call it before any other edit: the buffer's last edit is
    /// taken as the accept. Does nothing unless it's an accepted hunk of
    /// one of [`ScriptedAi::proposals`].
    pub fn note_accepted(&mut self, model: &EditorModel, hunk: HunkId) {
        if let Some(transaction) = self.catch_up(model) {
            self.mark_hunk(model, hunk, transaction);
        }
    }

    /// Marks accepted `hunk`, landed in `transaction`, as touched.
    fn mark_hunk(&mut self, model: &EditorModel, hunk: HunkId, transaction: TransactionId) {
        let landed = self.proposals.iter().find_map(|proposal| {
            (proposal.borrow().hunks(model.snapshot()).into_iter())
                .find(|view| view.id == hunk && view.status == HunkStatus::Accepted)
                .map(|view| view.range)
        });
        if let Some(range) = landed {
            self.marked.insert(transaction);
            self.mark(model, range);
        }
    }

    /// Follows the buffer's edits since the last call: an undo of a marked
    /// transaction takes its mark away, a redo marks where it lands again,
    /// as anchors can't follow text deleted and restored. Returns the last
    /// transaction edited, not undone or redone.
    fn catch_up(&mut self, model: &EditorModel) -> Option<TransactionId> {
        let snapshot = model.snapshot();
        let mut edited = None;
        // Each undo or redo of a marked transaction, in order, with the
        // span of its batches in `snapshot`.
        let mut changed: Vec<(TransactionId, EditKind, Range<usize>)> = Vec::new();
        for event in self.events.drain() {
            let BufferEvent::Edited {
                transaction,
                kind,
                to,
                edits,
                ..
            } = event
            else {
                continue;
            };
            if kind == EditKind::Edit {
                edited = Some(transaction);
                continue;
            }
            let start = edits.iter().map(|edit| edit.new.start).min();
            let end = edits.iter().map(|edit| edit.new.end).max();
            let (Some(start), Some(end)) = (start, end) else {
                continue;
            };
            // A model not caught up, or a log compacted past it, can't map
            // it.
            let mappable = snapshot.oldest_version() <= to && to <= snapshot.version();
            if !self.marked.contains(&transaction) || !mappable {
                continue;
            }
            let start = snapshot.map_offset(start, Bias::Right, to);
            let span = start..snapshot.map_offset(end, Bias::Left, to).max(start);
            match changed.last_mut() {
                Some((id, last_kind, last)) if *id == transaction && *last_kind == kind => {
                    *last = last.start.min(span.start)..last.end.max(span.end);
                }
                _ => changed.push((transaction, kind, span)),
            }
        }
        for (_, kind, span) in changed {
            let items = match kind {
                EditKind::Redo => vec![touched_row(span.clone())],
                _ => Vec::new(),
            };
            (self.touched.borrow_mut()).splice(snapshot, snapshot.version(), span, items);
        }
        edited
    }

    /// Accepts some of the live suggestion with `accept`; drops it once
    /// nothing of it is live.
    fn accept(
        &mut self,
        model: &mut EditorModel,
        accept: fn(&GhostText, &mut EditorModel) -> Option<TransactionId>,
    ) -> Result<Report, StepError> {
        let suggestion = self.suggestion.as_ref().ok_or(StepError::NoSuggestion)?;
        let transaction = accept(suggestion, model).ok_or(StepError::NoSuggestion)?;
        if !suggestion.is_live(model) {
            self.suggestion = None;
        }
        Ok(Report::Accepted(transaction))
    }

    /// The latest proposal and its `n`th (1-based) hunk.
    fn hunk(
        &self,
        model: &EditorModel,
        n: usize,
    ) -> Result<(Rc<RefCell<ProposedChange>>, HunkId), StepError> {
        let proposal = self.proposals.last().ok_or(StepError::NoProposal)?;
        let hunks = proposal.borrow().hunks(model.snapshot());
        let view =
            (n.checked_sub(1).and_then(|ix| hunks.get(ix))).ok_or(StepError::NoSuchHunk(n))?;
        Ok((proposal.clone(), view.id))
    }

    /// Covers `range`'s rows, replacing the marks inside it.
    fn mark(&mut self, model: &EditorModel, range: Range<usize>) {
        let snapshot = model.snapshot();
        self.touched.borrow_mut().splice(
            snapshot,
            snapshot.version(),
            range.clone(),
            vec![touched_row(range)],
        );
    }
}

fn touched_row(range: Range<usize>) -> Decoration {
    Decoration::new(range, DecorationKind::RowBackground, Tint::Accent)
}

fn origin(session: String) -> Origin {
    Origin::Ai {
        agent: Arc::from(AGENT),
        session: session.into(),
    }
}
