//! The editor's commands, registered on [`fe_command`] so keymaps, the
//! palette, scripts, tests and agents can run them without GPUI.
//!
//! IDs are `editor.` plus the snake_case of the GPUI action names, and each
//! does what the GPUI view did for its action. What the model can't do
//! alone (the viewport's height, the clipboard, syntax folds) comes from an
//! [`EditorHost`].

use std::ops::{Range, RangeInclusive};

use fe_buffer::{Origin, ResolvedSelection, Text, TransactionId, Version};
use fe_command::{
    Actor, CommandError, CommandTarget, ContextFrame, ContextPredicate, ContextStack, Dispatched,
    Effect, Registry,
};
use fe_display::Fold;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

use crate::{EditorModel, Movement};

/// The context frame name an editor has focus in, and every editor
/// command's context predicate.
pub const CONTEXT: &str = "editor";

/// What a command needs from where the model is shown.
pub trait EditorHost {
    /// How many rows paging moves.
    fn page_rows(&self) -> u32;
    fn read_clipboard(&mut self) -> Option<String>;
    fn write_clipboard(&mut self, text: String);
    /// The fold candidate at `row`, from syntax; `None` falls back to the
    /// indented block.
    fn fold_range_at(&self, row: u32) -> Option<RangeInclusive<u32>>;
    /// Every fold candidate in `rows`; none falls back to indented blocks.
    fn fold_ranges(&self, rows: Range<u32>) -> Vec<RangeInclusive<u32>>;
    /// What the view shows, in the text's rows; `None` (the default) if
    /// that isn't known.
    fn viewport(&self) -> Option<Viewport> {
        None
    }
    /// Changes what the view shows; the default does nothing.
    fn scroll(&mut self, request: ScrollRequest) {
        let _ = request;
    }
}

/// What a view shows: its first visible row of the text and how many whole
/// rows it has room for (rows past the text's end count, as Vim's `~`
/// rows do).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Viewport {
    pub top_row: u32,
    pub rows: u32,
}

/// A change to what the view shows, for [`EditorHost::scroll`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ScrollRequest {
    /// Show this row of the text first, even one near or at the text's
    /// end, leaving the rows after it empty.
    TopRow(u32),
}

/// A host for tests, scripts and agents: an in-memory clipboard, 20 page
/// rows and no syntax, so folds follow indentation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeadlessHost {
    pub clipboard: Option<String>,
    pub page_rows: u32,
}

impl Default for HeadlessHost {
    fn default() -> Self {
        Self {
            clipboard: None,
            page_rows: 20,
        }
    }
}

impl HeadlessHost {
    pub fn new() -> Self {
        Self::default()
    }
}

impl EditorHost for HeadlessHost {
    fn page_rows(&self) -> u32 {
        self.page_rows
    }

    fn read_clipboard(&mut self) -> Option<String> {
        self.clipboard.clone()
    }

    fn write_clipboard(&mut self, text: String) {
        self.clipboard = Some(text);
    }

    fn fold_range_at(&self, _: u32) -> Option<RangeInclusive<u32>> {
        None
    }

    fn fold_ranges(&self, _: Range<u32>) -> Vec<RangeInclusive<u32>> {
        Vec::new()
    }
}

/// What editor commands run against: a model and its host, borrowed for
/// one dispatch.
pub struct EditorTarget<'a> {
    pub model: &'a mut EditorModel,
    pub host: &'a mut dyn EditorHost,
}

impl<'a> EditorTarget<'a> {
    pub fn new(model: &'a mut EditorModel, host: &'a mut dyn EditorHost) -> Self {
        Self { model, host }
    }
}

/// The [`CommandTarget`] of the editor's [`Registry`].
pub struct Editor;

impl CommandTarget for Editor {
    type Tx = TransactionId;
    type Target<'a> = EditorTarget<'a>;
}

/// The origin an actor's edits are stamped with.
pub fn origin(actor: &Actor) -> Origin {
    match actor {
        Actor::User => Origin::User,
        Actor::Ai { agent, session } => Origin::Ai {
            agent: agent.as_str().into(),
            session: session.as_str().into(),
        },
        Actor::Lsp => Origin::Lsp,
        Actor::Format => Origin::Format,
        Actor::Host(name) => Origin::Host(name.as_str().into()),
    }
}

/// The context a focused editor dispatches in, with nothing around it.
pub fn context() -> ContextStack {
    ContextStack::new().with(ContextFrame::new(CONTEXT))
}

/// `editor.move`'s motions: every [`Movement`] but paging, whose size
/// depends on the view.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    WordLeft,
    WordRight,
    LineStart,
    LineEnd,
    DocumentStart,
    DocumentEnd,
}

impl From<Motion> for Movement {
    fn from(motion: Motion) -> Self {
        match motion {
            Motion::Left => Movement::Left,
            Motion::Right => Movement::Right,
            Motion::Up => Movement::Up,
            Motion::Down => Movement::Down,
            Motion::WordLeft => Movement::WordLeft,
            Motion::WordRight => Movement::WordRight,
            Motion::LineStart => Movement::LineStart,
            Motion::LineEnd => Movement::LineEnd,
            Motion::DocumentStart => Movement::DocumentStart,
            Motion::DocumentEnd => Movement::DocumentEnd,
        }
    }
}

/// `editor.insert`'s args.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InsertArgs {
    pub text: String,
}

/// `editor.move`'s args.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MoveArgs {
    pub motion: Motion,
    /// Keep each selection's tail, extending it.
    #[serde(default)]
    pub extend: bool,
    /// How many times to move, up to [`MAX_MOVE_COUNT`].
    #[serde(default = "one", deserialize_with = "move_count")]
    #[schemars(range(max = MAX_MOVE_COUNT))]
    pub count: u32,
}

fn one() -> u32 {
    1
}

/// The most moves one `editor.move` makes; a larger count is bad args.
/// Invocations come from scripts, replays and agents as well as keys, so a
/// count can't be allowed to spin the model for billions of moves, and
/// making fewer moves than asked would misreport what ran.
pub const MAX_MOVE_COUNT: u32 = 10_000;

fn move_count<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let count = u32::deserialize(deserializer)?;
    if count > MAX_MOVE_COUNT {
        return Err(D::Error::custom(format!(
            "count {count} is more than {MAX_MOVE_COUNT}"
        )));
    }
    Ok(count)
}

pub(crate) type Run = Result<Effect, CommandError>;

/// A movement's command and its `select_` twin: IDs and titles, and the
/// movement for a page of so many rows.
type MovementCommands = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    fn(u32) -> Movement,
);

/// The editor's registry, with every editor command.
pub fn registry() -> Registry<Editor> {
    let mut registry = Registry::new();
    let movements: [MovementCommands; 12] = [
        (
            "move_left",
            "Move Left",
            "select_left",
            "Select Left",
            |_| Movement::Left,
        ),
        (
            "move_right",
            "Move Right",
            "select_right",
            "Select Right",
            |_| Movement::Right,
        ),
        ("move_up", "Move Up", "select_up", "Select Up", |_| {
            Movement::Up
        }),
        (
            "move_down",
            "Move Down",
            "select_down",
            "Select Down",
            |_| Movement::Down,
        ),
        (
            "move_word_left",
            "Move Word Left",
            "select_word_left",
            "Select Word Left",
            |_| Movement::WordLeft,
        ),
        (
            "move_word_right",
            "Move Word Right",
            "select_word_right",
            "Select Word Right",
            |_| Movement::WordRight,
        ),
        (
            "move_line_start",
            "Move to Line Start",
            "select_line_start",
            "Select to Line Start",
            |_| Movement::LineStart,
        ),
        (
            "move_line_end",
            "Move to Line End",
            "select_line_end",
            "Select to Line End",
            |_| Movement::LineEnd,
        ),
        (
            "move_document_start",
            "Move to Document Start",
            "select_document_start",
            "Select to Document Start",
            |_| Movement::DocumentStart,
        ),
        (
            "move_document_end",
            "Move to Document End",
            "select_document_end",
            "Select to Document End",
            |_| Movement::DocumentEnd,
        ),
        (
            "page_up",
            "Page Up",
            "select_page_up",
            "Select Page Up",
            Movement::PageUp,
        ),
        (
            "page_down",
            "Page Down",
            "select_page_down",
            "Select Page Down",
            Movement::PageDown,
        ),
    ];
    for extend in [false, true] {
        for (move_id, move_title, select_id, select_title, movement) in movements {
            let (id, title) = if extend {
                (select_id, select_title)
            } else {
                (move_id, move_title)
            };
            add(&mut registry, id, title, false, move |model, host| {
                model.move_cursors(movement(host.page_rows()), extend);
                Ok(Effect::Changed)
            });
        }
    }
    add(
        &mut registry,
        "select_all",
        "Select All",
        false,
        |model, _| {
            model.select_all();
            Ok(Effect::Changed)
        },
    );
    add(
        &mut registry,
        "backspace",
        "Delete Backward",
        true,
        |model, _| {
            model.backspace();
            Ok(Effect::Changed)
        },
    );
    add(
        &mut registry,
        "delete",
        "Delete Forward",
        true,
        |model, _| {
            model.delete_forward();
            Ok(Effect::Changed)
        },
    );
    add(
        &mut registry,
        "delete_word_left",
        "Delete Word Left",
        true,
        |model, _| {
            model.delete_word_left();
            Ok(Effect::Changed)
        },
    );
    add(
        &mut registry,
        "newline",
        "Insert Newline",
        true,
        |model, _| {
            model.newline();
            Ok(Effect::Changed)
        },
    );
    add(&mut registry, "tab", "Insert Tab", true, |model, _| {
        model.tab();
        Ok(Effect::Changed)
    });
    add(&mut registry, "fold", "Fold", false, |model, host| {
        model.fold(&|row| host.fold_range_at(row));
        Ok(Effect::Changed)
    });
    add(&mut registry, "unfold", "Unfold", false, |model, _| {
        model.unfold();
        Ok(Effect::Changed)
    });
    add(
        &mut registry,
        "fold_all",
        "Fold All",
        false,
        |model, host| {
            let candidates = host.fold_ranges(0..model.text().row_count());
            model.fold_all(candidates);
            Ok(Effect::Changed)
        },
    );
    add(
        &mut registry,
        "unfold_all",
        "Unfold All",
        false,
        |model, _| {
            model.unfold_all();
            Ok(Effect::Changed)
        },
    );
    add(
        &mut registry,
        "toggle_soft_wrap",
        "Toggle Soft Wrap",
        false,
        |model, _| {
            model.toggle_soft_wrap();
            Ok(Effect::Changed)
        },
    );
    add(&mut registry, "copy", "Copy", false, |model, host| {
        let (text, _) = model.copy_text();
        host.write_clipboard(text);
        Ok(Effect::Nothing)
    });
    add(&mut registry, "cut", "Cut", true, |model, host| {
        let text = model.cut();
        host.write_clipboard(text);
        Ok(Effect::Changed)
    });
    add(&mut registry, "paste", "Paste", true, |model, host| {
        let Some(text) = host.read_clipboard() else {
            return Ok(Effect::Nothing);
        };
        model.paste(&text);
        Ok(Effect::Changed)
    });
    add(&mut registry, "undo", "Undo", true, |model, _| {
        model
            .undo()
            .map_err(|error| CommandError::new(format!("undo failed: {error}")))?;
        Ok(Effect::Changed)
    });
    add(&mut registry, "redo", "Redo", true, |model, _| {
        model
            .redo()
            .map_err(|error| CommandError::new(format!("redo failed: {error}")))?;
        Ok(Effect::Changed)
    });
    add(&mut registry, "save", "Save", false, |model, _| {
        model
            .save()
            .map_err(|error| CommandError::new(format!("save failed: {error}")))?;
        Ok(Effect::Notify)
    });

    registry
        .register_with_args(
            "editor.insert",
            "Insert Text",
            |target: &mut EditorTarget<'_>, actor: &Actor, args: InsertArgs| {
                run_as(target, actor, |model, _| {
                    model.insert(&args.text);
                    Ok(Effect::Changed)
                })
            },
        )
        .expect("editor command IDs are valid and distinct")
        .description("Replaces every selection with `text`, as typing does.")
        .context(ContextPredicate::identifier(CONTEXT))
        .mutates_buffer(true);
    registry
        .register_with_args(
            "editor.move",
            "Move",
            |target: &mut EditorTarget<'_>, actor: &Actor, args: MoveArgs| {
                run_as(target, actor, |model, _| {
                    // Caught up first, so the selections compared are
                    // those moving would start from.
                    model.sync_display();
                    for _ in 0..args.count {
                        let before = model.selections();
                        model.move_cursors(args.motion.into(), args.extend);
                        // Moving again from where a move left nothing
                        // changed would change nothing either.
                        if model.selections() == before {
                            break;
                        }
                    }
                    Ok(Effect::Changed)
                })
            },
        )
        .expect("editor command IDs are valid and distinct")
        .description(
            "Moves every caret `count` times (at most 10,000) by `motion`, extending the selections if `extend`.",
        )
        .context(ContextPredicate::identifier(CONTEXT));
    crate::multi_cursor::register(&mut registry);
    registry
}

/// Registers `editor.<name>`, which takes no args and runs `run` as the
/// invoking actor.
pub(crate) fn add(
    registry: &mut Registry<Editor>,
    name: &str,
    title: &str,
    mutates_buffer: bool,
    run: impl Fn(&mut EditorModel, &mut dyn EditorHost) -> Run + 'static,
) {
    registry
        .register(
            &format!("editor.{name}"),
            title,
            move |target: &mut EditorTarget<'_>, actor: &Actor| run_as(target, actor, &run),
        )
        .expect("editor command IDs are valid and distinct")
        .context(ContextPredicate::identifier(CONTEXT))
        .mutates_buffer(mutates_buffer);
}

/// Runs `run` with the model's edits, undo and redo acting as `actor`, and
/// collects the transactions it produced.
///
/// `run` says [`Effect::Changed`] where it may change what's shown; that
/// stands only if the text, the selections, the folds or the wrap did
/// change (catching up with another model's edits counts). Otherwise the
/// command did nothing, such as moving left at the start, undoing with no
/// history or inserting nothing at carets.
fn run_as(
    target: &mut EditorTarget<'_>,
    actor: &Actor,
    run: impl FnOnce(&mut EditorModel, &mut dyn EditorHost) -> Run,
) -> Result<Dispatched<TransactionId>, CommandError> {
    let host = &mut *target.host;
    let before = shown(target.model);
    let (effect, transactions) = target.model.run_as(origin(actor), |model| run(model, host));
    let effect = match effect? {
        Effect::Changed if shown(target.model) == before => Effect::Nothing,
        effect => effect,
    };
    Ok(Dispatched::new(effect).with_transactions(transactions))
}

/// What a command can change that the view shows: the text, the
/// selections, the folds and the wrap.
#[derive(PartialEq)]
struct Shown {
    text: ShownText,
    selections: Vec<ResolvedSelection>,
    folds: Vec<Fold>,
    soft_wrap: bool,
    rows: u32,
}

fn shown(model: &EditorModel) -> Shown {
    Shown {
        text: ShownText(model.snapshot().version(), model.text().clone()),
        selections: model.selections(),
        folds: model.display().folds().to_vec(),
        soft_wrap: model.soft_wrap(),
        rows: model.display().row_count(),
    }
}

/// The text at a version. Every applied batch advances the version, even
/// one that puts back what was there (undoing a replacement of `x` by `x`),
/// so the same version is the same text but different versions are
/// compared by content. Cloning the text is cheap; it shares the rope.
struct ShownText(Version, Text);

impl PartialEq for ShownText {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
            || (self.1.len() == other.1.len()
                && self
                    .1
                    .chunks()
                    .flat_map(str::bytes)
                    .eq(other.1.chunks().flat_map(str::bytes)))
    }
}
