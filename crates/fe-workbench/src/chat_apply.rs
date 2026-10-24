//! What a reply asks of the workspace (the AI chat record's § 8): its code
//! blocks applied as proposed changes, and its links to files opened.
//!
//! - **Apply** on a block ([`ChatViewEvent::ApplyBlock`]) is the AI's
//!   doing (record § 9): the panel dispatches `chat.propose_edit`, args
//!   `{ "chat", "turn", "block" }`, as an [`Invocation`] by
//!   [`Actor::Ai`] `{ agent: "fastedit.chat", session: <chat id> }` from
//!   [`Source::Script`], through [`registry`]: a `#[kiln::command]` method
//!   takes no args and can't see the actor. The app's invocation observers
//!   are told of it ([`notify_observers`]), so it's recorded and can be
//!   replayed ([`ChatPanel::run_propose_edit`]).
//! - **The handler** finds the fence in the reply's Markdown, reads it with
//!   [`parse_edit_block`] and proposes it, as the actor's [`Origin`],
//!   against its buffer as it is now, unsaved edits included:
//!   - With a `path`, in the file under the root, opened in the active
//!     pane. Off the main thread, the path is resolved a part at a time
//!     ([`real_path`]): every symlink followed, a dangling one's target
//!     too, and each `..` taken from what's resolved, and must land inside
//!     the root, resolved too, whether the file is there yet or not
//!     ([`locate`]); an editor already on that file, by whatever path,
//!     is the one opened, so its buffer is matched. A whole-file
//!     replacement is diffed against the whole text
//!     ([`ProposedChange::from_text`]), and sections are made at each
//!     SEARCH text's one exact match ([`ProposedChange::from_edits`]).
//!   - With none, over the selection the turn was sent with, which the
//!     turn owns from the moment it's made ([`ChatPanel::note_turns`]).
//!     The chip's selection is anchored in its buffer as it's gathered.
//!     The view records how it made each turn, as it makes it: sent from
//!     the composer, by a key, a command or the Send button, or a Retry of
//!     another turn. A sent turn takes the anchors of the chip it went with
//!     (turns are read before the panel hands the view a new chip), and a
//!     retry keeps those of the turn it sends again; the composer is never
//!     consulted. Either only if the turn's content holds that selection's
//!     block whole and it was gathered under the current root and grants
//!     generation; Apply reads only the turn's own. So edits since move
//!     it, a selection cut to fit isn't replaced, the same text selected
//!     elsewhere never stands in, and a chip gathered later never stands
//!     for an earlier turn. With no selection either, a toast says the
//!     block names no file.
//!
//!   What's found off the main thread stands only for the chat, root and
//!   trust it was asked under: a new chat or a change to either drops it,
//!   and the Applies waiting on reads.
//!
//!   It's shown with [`EditorView::set_proposals`] beside any shown
//!   already, and its first hunk revealed through the pane, so a tab
//!   showing its Markdown preview shows the text. A file not open yet is
//!   applied to once its read lands, each Apply waiting on its own. A
//!   SEARCH text found nowhere or more than once, sections changing the
//!   same text, markers that don't match up, a file read-only (the view,
//!   or the file's permissions for this process) or binary, a path outside
//!   the root: a toast names the file and why, and nothing is proposed.
//! - **Links** ([`ChatViewEvent::OpenPath`]) open the file under the root
//!   at their row, located as a block's file is; one whose real path is
//!   outside the root does nothing.
//!
//! [`ChatViewEvent::ApplyBlock`]: fe_chat_view::ChatViewEvent::ApplyBlock
//! [`ChatViewEvent::OpenPath`]: fe_chat_view::ChatViewEvent::OpenPath
//! [`notify_observers`]: kiln::platform::commands::notify_observers

use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    ops::Range,
    path::{Component, Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use fe_buffer::{Anchor, Bias, BufferId, BufferSnapshot, Origin, ProposedChange};
use fe_chat::{
    AttachmentKind, ContextItem, ContextKind, EditBlockError, EditBody, info_path,
    parse_edit_block, render_user_content,
};
use fe_command::{Actor, CommandTarget, Dispatched, Effect, Invocation, Registry, Source, Value};
use fe_gpui::EditorView;
use fe_markdown::{Block, BlockKind};
use gpui::{App, AppContext as _, Context, Entity, EntityId, Subscription, Task, Window};
use kiln::{schemars, serde};

use super::ChatPanel;
use crate::{
    Pane, Workbench,
    pane::Reveal,
    workbench::toast::{self, Toast},
};

#[path = "chat_changeset.rs"]
mod chat_changeset;

/// The agent the chat's actions are by.
pub(crate) const AGENT: &str = "fastedit.chat";

/// The command Apply dispatches.
pub(crate) const PROPOSE_EDIT: &str = "chat.propose_edit";

/// How far into a file a NUL makes it binary.
const BINARY_PROBE: usize = 8 << 10;

/// `chat.propose_edit`'s args: code block `block` of turn `turn`'s reply in
/// chat `chat`.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(crate = "kiln::serde")]
#[schemars(crate = "kiln::schemars")]
pub(crate) struct ProposeEdit {
    pub(crate) chat: String,
    pub(crate) turn: usize,
    pub(crate) block: usize,
}

/// The [`CommandTarget`] of the chat's own commands: what was asked, by
/// whom.
pub(crate) struct ChatCommands;

impl CommandTarget for ChatCommands {
    type Tx = ();
    type Target<'a> = Option<(ProposeEdit, Actor)>;
}

/// The chat's commands that take args or see the actor:
/// `chat.propose_edit`.
pub(crate) fn registry() -> Registry<ChatCommands> {
    let mut registry = Registry::new();
    registry
        .register_with_args(
            PROPOSE_EDIT,
            "Propose a Chat Edit",
            |target: &mut Option<(ProposeEdit, Actor)>, actor: &Actor, args: ProposeEdit| {
                *target = Some((args, actor.clone()));
                Ok(Dispatched::new(Effect::Nothing))
            },
        )
        .expect("the chat's command IDs are valid and distinct")
        .description("Proposes a chat reply's code block as a change in its file.");
    registry
}

/// A selection kept as anchors in its buffer.
#[derive(Clone, Debug)]
pub(super) struct Anchored {
    buffer: BufferId,
    range: Range<Anchor>,
}

/// A selection gathered for the chip, or a turn sent with it: where it
/// is, the block of a turn's content it's sent as, which identifies it,
/// and the panel's root and grants generation it was gathered under.
#[derive(Clone, Debug)]
struct Sent {
    anchored: Anchored,
    block: Value,
    generation: u64,
}

/// The panel's state for applying blocks.
pub(super) struct Applies {
    commands: Registry<ChatCommands>,
    reviews: Registry<chat_changeset::ReviewCommands>,
    /// The proposals each reviewed turn put on editors.
    reviewed: chat_changeset::Reviewed,
    /// The selection the context's chip is of, as it was gathered.
    chip: Option<Sent>,
    /// The current chat's turns sent with a selection, by index: that
    /// selection.
    turns: HashMap<usize, Sent>,
    /// How many of the current chat's turns have been looked at.
    seen: usize,
    /// Blocks to apply once their files are read into a pane, in the order
    /// asked.
    waiting: Vec<Waiting>,
    /// Counts the chats, and the root's context changes: a file located
    /// under an earlier one isn't applied to, nor a link opened.
    chats: u64,
}

/// A block to apply once its file is in an editor.
struct Waiting {
    pane: EntityId,
    path: PathBuf,
    edit: Edit,
    /// The pane reading it, watched.
    _pane: Subscription,
}

/// A parsed block to propose, by whom, where, and its file as the toasts
/// name it.
struct Edit {
    body: EditBody,
    origin: Origin,
    /// The selection it replaces; the whole file if none.
    within: Option<Anchored>,
    shown: String,
}

impl Applies {
    pub(super) fn new() -> Applies {
        Applies {
            commands: registry(),
            reviews: chat_changeset::registry(),
            reviewed: Rc::default(),
            chip: None,
            turns: HashMap::new(),
            seen: 0,
            waiting: Vec::new(),
            chats: 0,
        }
    }

    /// Forgets the turns, for a new chat.
    pub(super) fn new_chat(&mut self) {
        self.turns.clear();
        self.seen = 0;
        self.stale();
    }

    /// The chat or the root's context changed: nothing asked before is
    /// applied, nor any link opened.
    pub(super) fn stale(&mut self) {
        self.waiting.clear();
        self.chats += 1;
    }
}

/// The actor's edits' origin: the AI's maps to [`Origin::Ai`].
fn origin_of(actor: &Actor) -> Origin {
    match actor {
        Actor::User => Origin::User,
        Actor::Ai { agent, session } => Origin::Ai {
            agent: Arc::from(agent.as_str()),
            session: Arc::from(session.as_str()),
        },
        Actor::Lsp => Origin::Lsp,
        Actor::Format => Origin::Format,
        Actor::Host(host) => Origin::Host(Arc::from(host.as_str())),
    }
}

/// Code block `index` of `blocks`, counted in document order, those in
/// quotes and lists too, as the view counts them: its info string and code.
fn fence(blocks: &[Block], index: usize) -> Option<(String, String)> {
    fn walk<'a>(blocks: &'a [Block], out: &mut Vec<(&'a str, &'a str)>) {
        for block in blocks {
            match &block.kind {
                BlockKind::Code { info, code, .. } => out.push((info, code)),
                BlockKind::Quote(blocks) => walk(blocks, out),
                BlockKind::List { items, .. } => {
                    for item in items {
                        walk(&item.blocks, out);
                    }
                }
                _ => {}
            }
        }
    }
    let mut fences = Vec::new();
    walk(blocks, &mut fences);
    let (info, code) = fences.get(index)?;
    Some(((*info).to_owned(), (*code).to_owned()))
}

/// Whether `path` names a file under the root it's joined to, as written.
fn within_root(path: &str) -> bool {
    !path.is_empty()
        && (Path::new(path).components())
            .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
        && !(path.split(['/', '\\'])).any(|part| part == "..")
}

/// A file a block or link is for, found: the path to open it by, and
/// whether its permissions allow writing it.
struct Located {
    path: PathBuf,
    read_only: bool,
}

/// How many symlinks [`real_path`] follows before it gives up, as Linux's
/// `ELOOP` limit.
const MAX_LINKS: usize = 40;

/// `path` as the file system resolves it, a part at a time: each symlink
/// followed, a relative target from the link's folder, and each `..` taken
/// from the part resolved so far, so through no link and no `..`. A part
/// that isn't there, and what follows it, is joined on as named; a `..`
/// after one, a loop, or a part that can't be read gives `None`.
fn real_path(path: &Path) -> Option<PathBuf> {
    // The parts still to resolve, the next last.
    let mut parts: Vec<PathBuf> = Vec::new();
    let push = |parts: &mut Vec<PathBuf>, path: &Path| {
        let start = parts.len();
        parts.extend(
            path.components()
                .map(|part| PathBuf::from(part.as_os_str())),
        );
        parts[start..].reverse();
    };
    push(&mut parts, path);
    let mut real = PathBuf::new();
    let (mut links, mut missing) = (0, false);
    while let Some(part) = parts.pop() {
        match part.components().next()? {
            Component::Prefix(_) | Component::RootDir => {
                let prefix = (real.components().next())
                    .filter(|first| matches!(first, Component::Prefix(_)));
                let prefix = prefix.map(|prefix| PathBuf::from(prefix.as_os_str()));
                real = match part.components().next()? {
                    Component::RootDir => prefix.unwrap_or_default().join(&part),
                    _ => part,
                };
            }
            Component::CurDir => {}
            Component::ParentDir if missing => return None,
            Component::ParentDir => {
                real.pop();
            }
            Component::Normal(name) if missing => real.push(name),
            Component::Normal(name) => {
                let next = real.join(name);
                match fs::symlink_metadata(&next) {
                    Ok(meta) if meta.file_type().is_symlink() => {
                        links += 1;
                        if links > MAX_LINKS {
                            return None;
                        }
                        // From the link's folder, which `real` is.
                        push(&mut parts, &fs::read_link(&next).ok()?);
                    }
                    Ok(_) => real = next,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        missing = true;
                        real = next;
                    }
                    Err(_) => return None,
                }
            }
        }
    }
    real.is_absolute().then_some(real)
}

/// Where `rel` is under `root`, really, unless that's outside it; opened
/// by the path of an editor already on the same file, of `open`, so its
/// buffer, unsaved edits and all, is the one used. Reads the disk.
fn locate(root: &Path, rel: &str, open: &[PathBuf]) -> Result<Located, &'static str> {
    const OUTSIDE: &str = "it's outside the folder";
    if !within_root(rel) {
        return Err(OUTSIDE);
    }
    let full = root.join(rel);
    let real_root = fs::canonicalize(root).map_err(|_| "the folder can't be read")?;
    let real = real_path(&full).ok_or("its path can't be resolved")?;
    if !real.starts_with(&real_root) {
        return Err(OUTSIDE);
    }
    let path = (open.iter())
        .find(|path| crate::open::file_identity(path).as_ref() == Some(&real))
        .cloned()
        // Opened by the path vetted, through no link: one retargeted since
        // can't take the read elsewhere.
        .unwrap_or(real.clone());
    Ok(Located {
        path,
        read_only: is_read_only(&real),
    })
}

/// Whether the file at `path` is there and can't be written: its
/// permissions allow no writing, or not by this process. Reads the disk.
fn is_read_only(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.permissions().readonly() || !writable(path))
}

#[cfg(unix)]
fn writable(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt as _;
    let Ok(path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `path` is a NUL-terminated string that outlives the call.
    unsafe { libc::access(path.as_ptr(), libc::W_OK) == 0 }
}

#[cfg(not(unix))]
fn writable(_: &Path) -> bool {
    true
}

/// The editor of a tab of `pane`'s on `path`.
fn editor_at(pane: &Entity<Pane>, path: &Path, cx: &App) -> Option<Entity<EditorView>> {
    let pane = pane.read(cx);
    (pane.active_editor().into_iter())
        .chain(pane.tabs().iter().filter_map(|tab| tab.editor()))
        .find(|editor| editor.read(cx).model().path() == Some(path))
        .cloned()
}

/// `text` with `\r\n` as `\n`, as buffers keep it.
fn lf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// Where in `text` each section's search text is, once, as edits of
/// `text` offset by `base`; else why not, for the toast.
fn resolve_sections(
    text: &str,
    base: usize,
    sections: &[(String, String)],
) -> Result<Vec<(Range<usize>, String)>, String> {
    let mut edits = Vec::with_capacity(sections.len());
    for (ix, (search, replace)) in sections.iter().enumerate() {
        let search = lf(search);
        let first = search.lines().next().unwrap_or_default().trim();
        let which = format!("SEARCH {} (\"{first}\")", ix + 1);
        if search.is_empty() {
            return Err(format!("{which} is empty"));
        }
        let mut found = text.match_indices(search.as_str()).map(|(at, _)| at);
        let Some(at) = found.next() else {
            return Err(format!("{which} isn't in it"));
        };
        // Overlapping matches count too.
        let step = search.chars().next().map_or(1, char::len_utf8);
        if text[at + step..].contains(search.as_str()) {
            return Err(format!("{which} is in it more than once"));
        }
        edits.push((base + at..base + at + search.len(), lf(replace)));
    }
    let mut ranges: Vec<Range<usize>> = edits.iter().map(|(range, _)| range.clone()).collect();
    ranges.sort_by_key(|range| (range.start, range.end));
    if (ranges.windows(2)).any(|pair| pair[0].end > pair[1].start || pair[0] == pair[1]) {
        return Err("two of its sections change the same text".to_owned());
    }
    Ok(edits)
}

/// Why `editor` can't take a proposal, if it can't.
fn refusal(editor: &EditorView) -> Option<&'static str> {
    if editor.is_read_only() {
        return Some("it's read-only");
    }
    let text = editor.model().buffer().text().clone();
    let probe = text.clip_offset(BINARY_PROBE.min(text.len()), Bias::Left);
    (text.slice_to_string(0..probe))
        .contains('\0')
        .then_some("it's binary")
}

/// `body` proposed over `range` of `snapshot`, as `origin`: the step Apply
/// and Review Changes share, so their errors read alike; else why not, for
/// the toast.
fn propose(
    snapshot: &BufferSnapshot,
    range: Range<usize>,
    body: EditBody,
    origin: Origin,
) -> Result<ProposedChange, String> {
    match body {
        EditBody::Replace(text) => Ok(ProposedChange::from_text(snapshot, range, &text, origin)),
        EditBody::Sections(sections) => {
            let text = snapshot.text().slice_to_string(range.clone());
            let edits = resolve_sections(&text, range.start, &sections)?;
            Ok(ProposedChange::from_edits(snapshot, edits, origin))
        }
    }
}

/// `body` proposed over all of `editor`'s text, unless it takes no
/// proposal: [`refusal`], then [`propose`].
fn propose_whole(
    editor: &EditorView,
    body: EditBody,
    origin: Origin,
) -> Result<ProposedChange, String> {
    if let Some(why) = refusal(editor) {
        return Err(why.to_owned());
    }
    let snapshot = editor.model().buffer().snapshot();
    propose(&snapshot, 0..snapshot.len(), body, origin)
}

/// A toast that the block couldn't be applied to `file`, and why.
fn refused(file: &str, why: &str, cx: &mut App) {
    toast::show(
        cx,
        Toast::error(format!("Can't apply the block to {file}: {why}")),
    );
}

impl ChatPanel {
    /// Notes the selection `items`' chip is of: the active editor's, and
    /// the block it would be sent as.
    pub(super) fn note_chip(&mut self, items: &[ContextItem], workbench: &Workbench, cx: &App) {
        let item = (items.iter()).find(|item| matches!(item.kind, ContextKind::Selection { .. }));
        let editor = workbench.active_editor(cx);
        self.applies.chip = item.zip(editor).map(|(item, editor)| {
            let model = editor.read(cx).model();
            // The selection's offsets are in the model's snapshot.
            let snapshot = model.snapshot();
            let range = model.primary_selection().range;
            // Text typed at either end isn't the selection's.
            let anchored = Anchored {
                buffer: snapshot.id(),
                range: snapshot.anchor_after(range.start)..snapshot.anchor_before(range.end),
            };
            let block = render_user_content("", std::slice::from_ref(item)).swap_remove(0);
            Sent {
                anchored,
                block,
                generation: self.generation,
            }
        });
    }

    /// Gives each turn made since last looked the selection it was sent
    /// with, once, by how the view says it made the turn
    /// ([`ChatView::was_sent`], [`ChatView::retry_of`]): a turn sent from
    /// the composer takes the anchors of the chip it went with, and a
    /// retry those of the turn it sent again. The panel reads new turns
    /// before it hands the view a new chip, so a sent turn's chip is the
    /// one the view held as it was sent. Only a selection whose block the
    /// turn holds whole, so not one cut to fit, and one gathered under the
    /// current generation; any other turn has none, and Apply reads only a
    /// turn's own.
    ///
    /// [`ChatView::was_sent`]: fe_chat_view::ChatView::was_sent
    /// [`ChatView::retry_of`]: fe_chat_view::ChatView::retry_of
    pub(super) fn note_turns(&mut self, cx: &App) {
        let view = self.view.read(cx);
        let turns = &view.conversation().turns;
        for (index, turn) in turns.iter().enumerate().skip(self.applies.seen) {
            let from = if view.was_sent(index) {
                self.applies.chip.as_ref()
            } else {
                (view.retry_of(index)).and_then(|of| self.applies.turns.get(&of))
            };
            let selected = (turn.attachments.iter())
                .any(|attachment| attachment.kind == AttachmentKind::Selection);
            let from = (from.filter(|_| selected))
                .filter(|from| from.generation == self.generation)
                .filter(|from| turn.user_content.contains(&from.block));
            if let Some(from) = from.cloned() {
                self.applies.turns.insert(index, from);
            }
        }
        self.applies.seen = turns.len();
    }

    /// Apply on code block `block` of turn `turn`'s reply: dispatches
    /// `chat.propose_edit` by the chat's AI actor.
    pub(super) fn apply_block(
        &mut self,
        turn: usize,
        block: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let chat = self.view.read(cx).conversation().id.to_string();
        let args = Value::from_iter([
            ("chat", Value::from(chat.as_str())),
            ("turn", Value::from(turn)),
            ("block", Value::from(block)),
        ]);
        let actor = Actor::Ai {
            agent: AGENT.to_owned(),
            session: chat,
        };
        let invocation = (Invocation::of(PROPOSE_EDIT).expect("a valid command ID"))
            .args(args)
            .actor(actor)
            .source(Source::Script);
        self.run_propose_edit(&invocation, window, cx);
    }

    /// Runs `invocation` of `chat.propose_edit`, telling the app's
    /// invocation observers, then proposes the block it names.
    pub(crate) fn run_propose_edit(
        &mut self,
        invocation: &Invocation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut asked = None;
        if let Err(error) = self.applies.commands.dispatch(&mut asked, invocation) {
            log::warn!("chat: {error}");
            return;
        }
        kiln::platform::commands::notify_observers(invocation, cx);
        if let Some((args, actor)) = asked {
            self.propose_edit(&args, &actor, window, cx);
        }
    }

    /// `chat.propose_edit`'s handler: see the module docs.
    fn propose_edit(
        &mut self,
        args: &ProposeEdit,
        actor: &Actor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let conversation = self.view.read(cx).conversation();
        if args.chat != conversation.id.to_string() {
            log::info!("chat: {} isn't the chat shown", args.chat);
            return;
        }
        let Some(turn) = conversation.turns.get(args.turn) else {
            return;
        };
        let selection = (turn.attachments.iter())
            .find(|attachment| attachment.kind == AttachmentKind::Selection)
            .map(|attachment| attachment.label.clone());
        let document = fe_markdown::parse(&turn.text());
        let Some((info, code)) = fence(&document.blocks, args.block) else {
            return;
        };
        let block = match parse_edit_block(&info, &code) {
            Ok(block) => block,
            Err(EditBlockError::Path(path)) => {
                return refused(&path, "it's outside the folder", cx);
            }
            Err(error @ EditBlockError::Malformed { .. }) => {
                let file = info_path(&info).unwrap_or("the selection");
                return refused(file, &error.to_string(), cx);
            }
        };
        let origin = origin_of(actor);
        match block.path {
            Some(path) => {
                let edit = Edit {
                    body: block.body,
                    origin,
                    within: None,
                    shown: path.clone(),
                };
                self.apply_to_file(&path, edit, window, cx);
            }
            None => {
                let Some(label) = selection else {
                    let message = "This block doesn't say which file it's for";
                    return toast::show(cx, Toast::warning(message));
                };
                let Some(sent) = self.applies.turns.get(&args.turn) else {
                    let why = "the selection it was sent with was cut to fit, or isn't known";
                    return refused(&label, why, cx);
                };
                let edit = Edit {
                    body: block.body,
                    origin,
                    within: Some(sent.anchored.clone()),
                    shown: label,
                };
                self.apply_to_selection(edit, window, cx);
            }
        }
    }

    /// Finds `rel` under the root, off the main thread, then proposes
    /// `edit` in it.
    fn apply_to_file(
        &mut self,
        rel: &str,
        edit: Edit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workbench) = self.workbench() else {
            return;
        };
        let Some(root) = workbench.read(cx).root().map(Path::to_owned) else {
            return refused(rel, "no folder is open", cx);
        };
        let open = self.open_paths(&workbench, cx);
        let rel = rel.to_owned();
        let located = cx.background_spawn(async move { locate(&root, &rel, &open) });
        self.when_located(located, edit, window, cx);
    }

    /// Proposes `edit` over the selection it's for, in its buffer.
    fn apply_to_selection(&mut self, edit: Edit, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workbench) = self.workbench() else {
            return;
        };
        let Some(within) = &edit.within else {
            return;
        };
        let editors = workbench.read(cx).file_editors(cx);
        let found = (editors.into_iter())
            .chain(workbench.read(cx).active_editor(cx).cloned())
            .find(|editor| editor.read(cx).model().buffer().id() == within.buffer);
        let Some(editor) = found else {
            return refused(&edit.shown, "its file is closed", cx);
        };
        let Some(path) = editor.read(cx).model().path().map(Path::to_owned) else {
            // Untitled: in the editor it's in.
            return show_proposal(&editor, None, edit, window, cx);
        };
        let located = cx.background_spawn(async move {
            let read_only = is_read_only(&path);
            Ok(Located { path, read_only })
        });
        self.when_located(located, edit, window, cx);
    }

    /// The paths of the files open in editors.
    fn open_paths(&self, workbench: &Entity<Workbench>, cx: &App) -> Vec<PathBuf> {
        (workbench.read(cx).file_editors(cx).iter())
            .filter_map(|editor| editor.read(cx).model().path().map(Path::to_owned))
            .collect()
    }

    /// Once `located` is found, opens it in the active pane and proposes
    /// `edit` there, unless it's read-only or the chat has changed.
    fn when_located(
        &mut self,
        located: Task<Result<Located, &'static str>>,
        edit: Edit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let chats = self.applies.chats;
        cx.spawn_in(window, async move |panel, cx| {
            let located = located.await;
            (panel.update_in(cx, |panel, window, cx| {
                if panel.applies.chats != chats {
                    return;
                }
                match located {
                    Err(why) => refused(&edit.shown, why, cx),
                    Ok(located) if located.read_only => refused(&edit.shown, "it's read-only", cx),
                    Ok(located) => panel.open_to_apply(located.path, edit, window, cx),
                }
            }))
            .ok();
        })
        .detach();
    }

    /// Opens `path` in the active pane, an open buffer reused, and proposes
    /// `edit` there, once it's read if it isn't open.
    fn open_to_apply(
        &mut self,
        path: PathBuf,
        edit: Edit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workbench) = self.workbench() else {
            return;
        };
        let pane = workbench.read(cx).pane().clone();
        pane.update(cx, |pane, cx| pane.open(path.clone(), window, cx));
        if let Some(editor) = editor_at(&pane, &path, cx) {
            return show_proposal(&editor, Some((&pane, &path)), edit, window, cx);
        }
        if !pane.read(cx).reading().any(|reading| reading == path) {
            return refused(&edit.shown, "it couldn't be opened in an editor", cx);
        }
        let watch = cx.observe_in(&pane, window, Self::file_read);
        self.applies.waiting.push(Waiting {
            pane: pane.entity_id(),
            path,
            edit,
            _pane: watch,
        });
    }

    /// `pane` changed while files were read into it for blocks: proposes
    /// each once its file's in an editor, or gives up on it once it isn't
    /// being read.
    fn file_read(&mut self, pane: Entity<Pane>, window: &mut Window, cx: &mut Context<Self>) {
        let mut ix = 0;
        while ix < self.applies.waiting.len() {
            let waiting = &self.applies.waiting[ix];
            if waiting.pane != pane.entity_id() {
                ix += 1;
                continue;
            }
            let editor = editor_at(&pane, &waiting.path, cx);
            let reading = (pane.read(cx).reading()).any(|path| path == waiting.path);
            if editor.is_none() && reading {
                ix += 1;
                continue;
            }
            let waiting = self.applies.waiting.remove(ix);
            match editor {
                Some(editor) => show_proposal(
                    &editor,
                    Some((&pane, &waiting.path)),
                    waiting.edit,
                    window,
                    cx,
                ),
                None => refused(
                    &waiting.edit.shown,
                    "it couldn't be opened in an editor",
                    cx,
                ),
            }
        }
    }

    /// A reply's link to `path` under the root: opens it in the active pane,
    /// at `row`, 1-based, if given, unless it's really outside the root.
    pub(super) fn open_link(
        &mut self,
        path: &str,
        row: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workbench) = self.workbench() else {
            return;
        };
        let Some(root) = workbench.read(cx).root().map(Path::to_owned) else {
            return;
        };
        let open = self.open_paths(&workbench, cx);
        let rel = path.to_owned();
        let located = cx.background_spawn(async move { locate(&root, &rel, &open) });
        let reveal = row.map(|row| Reveal {
            row: row.saturating_sub(1),
            columns: 0..0,
            within_row: false,
        });
        let (rel, chats) = (path.to_owned(), self.applies.chats);
        cx.spawn_in(window, async move |panel, cx| {
            let located = match located.await {
                Ok(located) => located,
                Err(why) => return log::info!("chat: not opening a link to {rel}: {why}"),
            };
            (panel.update_in(cx, |panel, window, cx| {
                // Not for another chat, root or trust.
                if panel.applies.chats != chats {
                    return;
                }
                let Some(workbench) = panel.workbench() else {
                    return;
                };
                let pane = workbench.read(cx).pane().clone();
                pane.update(cx, |pane, cx| {
                    pane.open_at(located.path, reveal, window, cx)
                });
            }))
            .ok();
        })
        .detach();
    }
}

/// Proposes `edit` in `editor`, over its selection or the whole text,
/// beside the proposals it shows, and shows its first hunk: through
/// `tab`, the pane and path the editor's tab is at, so the text shows if
/// the tab showed a preview.
fn show_proposal(
    editor: &Entity<EditorView>,
    tab: Option<(&Entity<Pane>, &Path)>,
    edit: Edit,
    window: &mut Window,
    cx: &mut App,
) {
    let Edit {
        body,
        origin,
        within,
        shown,
    } = edit;
    if let Some(why) = refusal(editor.read(cx)) {
        return refused(&shown, why, cx);
    }
    let snapshot = editor.read(cx).model().buffer().snapshot();
    let range = match &within {
        None => 0..snapshot.len(),
        Some(within) if within.buffer != snapshot.id() => {
            return refused(&shown, "its file is closed", cx);
        }
        Some(within) if within.range.start.version() < snapshot.oldest_version() => {
            return refused(&shown, "the selection it was sent with is lost", cx);
        }
        Some(within) => snapshot.resolve_range(&within.range),
    };
    let proposal = match propose(&snapshot, range, body, origin) {
        Ok(proposal) => proposal,
        Err(why) => return refused(&shown, &why, cx),
    };
    let Some(first) = proposal
        .hunks(&snapshot)
        .first()
        .map(|hunk| hunk.range.start)
    else {
        let message = format!("The block changes nothing in {shown}");
        return toast::show(cx, Toast::info(message));
    };
    editor.update(cx, |editor, cx| {
        let mut proposals = editor.proposals().to_vec();
        proposals.push(Rc::new(RefCell::new(proposal)));
        editor.set_proposals(proposals, cx);
    });
    match tab {
        // Shown as a place in the text is, the source in place of a
        // preview.
        Some((pane, path)) => pane.update(cx, |pane, cx| {
            pane.open_at(
                path.to_owned(),
                Some(Reveal::bytes(first..first)),
                window,
                cx,
            );
        }),
        None => editor.update(cx, |editor, cx| editor.select_and_reveal(first..first, cx)),
    }
}
