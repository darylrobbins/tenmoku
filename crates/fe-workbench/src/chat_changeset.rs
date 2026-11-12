//! Review Changes on a turn (§ 8 of the AI chat record): every file edit a
//! reply proposes, loaded together into one [`Changeset`] in the AI Changes
//! panel, so a reply for several files is reviewed, accepted or reverted as
//! one.
//!
//! The panel dispatches `chat.review_changes`, args `{ "chat", "turn" }`,
//! as `chat.propose_edit` is: an [`Invocation`] by [`Actor::Ai`] from
//! [`Source::Script`], through [`registry`], and told to the app's
//! invocation observers, so it's recorded and can be replayed
//! ([`ChatPanel::run_review_changes`]).
//!
//! The handler reads each of the turn's code blocks that names a file
//! (`path=`) as Apply does. Each file is found as Apply finds it
//! ([`locate`]: the root, links and `..` checked) and opened in the active
//! pane, an open buffer reused; each block is proposed against its buffer as
//! it is now by the step Apply proposes by ([`propose_whole`]). A file
//! that isn't there, whose folder is, and whose block is a whole file,
//! becomes a new file of the changeset. A block that can't be proposed, for
//! the reasons Apply gives, is left out with a toast naming its file, and
//! the rest load; with none loaded, the panel isn't shown. A later Review
//! of the same turn replaces its changeset, and the proposals it put on
//! the editors.

use std::{cell::RefCell, collections::HashMap, path::PathBuf, rc::Rc};

use fe_buffer::{Origin, ProposedChange};
use fe_chat::{EditBlockError, EditBody, info_path, parse_edit_block};
use fe_command::{Actor, CommandTarget, Dispatched, Effect, Invocation, Registry, Source, Value};
use fe_editor::Changeset;
use gpui::{App, AppContext as _, Context, Entity, Subscription, WeakEntity, Window};
use kiln::{schemars, serde};
use kiln_l10n::tr;

use super::{AGENT, ChatPanel, Located, editor_at, lf, locate, origin_of, propose_whole, refused};
use crate::{
    Pane, Workbench,
    workbench::toast::{self, Toast},
};

/// The command Review Changes dispatches.
pub(crate) const REVIEW_CHANGES: &str = "chat.review_changes";

/// `chat.review_changes`'s args: the code blocks of turn `turn`'s reply in
/// chat `chat`.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(crate = "kiln::serde")]
#[schemars(crate = "kiln::schemars")]
pub(crate) struct ReviewChanges {
    pub(crate) chat: String,
    pub(crate) turn: usize,
}

/// The [`CommandTarget`] of Review Changes: what was asked, by whom.
pub(crate) struct ReviewCommands;

impl CommandTarget for ReviewCommands {
    type Tx = ();
    type Target<'a> = Option<(ReviewChanges, Actor)>;
}

/// `chat.review_changes`.
pub(crate) fn registry() -> Registry<ReviewCommands> {
    let mut registry = Registry::new();
    registry
        .register_with_args(
            REVIEW_CHANGES,
            &tr!("workbench-chat-review-title"),
            |target: &mut Option<(ReviewChanges, Actor)>, actor: &Actor, args: ReviewChanges| {
                *target = Some((args, actor.clone()));
                Ok(Dispatched::new(Effect::Nothing))
            },
        )
        .expect("the chat's command IDs are valid and distinct")
        .description(&tr!("workbench-chat-review-description"));
    registry
}

/// The proposals each reviewed turn put on the editors, by turn.
pub(super) type Reviewed = Rc<RefCell<HashMap<String, Vec<Rc<RefCell<ProposedChange>>>>>>;

/// A block for a file, to propose.
struct FileBlock {
    rel: String,
    body: EditBody,
}

/// A block's file, found.
struct Found {
    block: FileBlock,
    located: Result<Located, String>,
    /// Whether the file is there, and its folder if it isn't.
    exists: bool,
    folder: bool,
}

/// What the changeset holds for a block, in the order asked.
enum Entry {
    Existing(PathBuf, Rc<RefCell<ProposedChange>>, fe_buffer::BufferId),
    New(PathBuf, String),
}

impl ChatPanel {
    /// Review Changes on turn `turn`: dispatches `chat.review_changes` by
    /// the chat's AI actor.
    pub(in crate::chat_panel) fn review_turn(
        &mut self,
        turn: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let chat = self.view.read(cx).conversation().id.to_string();
        let args = Value::from_iter([
            ("chat", Value::from(chat.as_str())),
            ("turn", Value::from(turn)),
        ]);
        let actor = Actor::Ai {
            agent: AGENT.to_owned(),
            session: chat,
        };
        let invocation = (Invocation::of(REVIEW_CHANGES).expect("a valid command ID"))
            .args(args)
            .actor(actor)
            .source(Source::Script);
        self.run_review_changes(&invocation, window, cx);
    }

    /// Review Changes on the chat's latest turn, as its button: nothing if
    /// there is none.
    pub(crate) fn review_latest(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let turns = self.view.read(cx).conversation().turns.len();
        if let Some(turn) = turns.checked_sub(1) {
            self.review_turn(turn, window, cx);
        }
    }

    /// Runs `invocation` of `chat.review_changes`, telling the app's
    /// invocation observers, then loads the turn's blocks.
    pub(crate) fn run_review_changes(
        &mut self,
        invocation: &Invocation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut asked = None;
        if let Err(error) = self.applies.reviews.dispatch(&mut asked, invocation) {
            log::warn!("chat: {error}");
            return;
        }
        kiln::platform::commands::notify_observers(invocation, cx);
        if let Some((args, actor)) = asked {
            self.review_changes(&args, &actor, window, cx);
        }
    }

    /// `chat.review_changes`'s handler: see the module docs.
    fn review_changes(
        &mut self,
        args: &ReviewChanges,
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
        let document = fe_markdown::parse(&turn.text());
        let mut blocks: Vec<FileBlock> = Vec::new();
        let mut index = 0;
        while let Some((info, code)) = super::fence(&document.blocks, index) {
            index += 1;
            match parse_edit_block(&info, &code) {
                Ok(block) => {
                    let Some(rel) = block.path else {
                        continue;
                    };
                    if blocks.iter().any(|seen| seen.rel == rel) {
                        refused(&rel, &tr!("workbench-chat-why-another-block"), cx);
                        continue;
                    }
                    blocks.push(FileBlock {
                        rel,
                        body: block.body,
                    });
                }
                Err(EditBlockError::Path(path)) => {
                    refused(&path, &tr!("workbench-chat-why-outside"), cx);
                }
                Err(error @ EditBlockError::Malformed { .. }) => {
                    if let Some(file) = info_path(&info) {
                        refused(file, &error.to_string(), cx);
                    }
                }
            }
        }
        let Some(workbench) = self.workbench() else {
            return;
        };
        let turn_id = format!("{}:{}", args.chat, args.turn);
        let root = workbench.read(cx).root().map(std::path::Path::to_owned);
        let Some(root) = root.filter(|_| !blocks.is_empty()) else {
            for block in &blocks {
                refused(&block.rel, &tr!("workbench-chat-why-no-folder"), cx);
            }
            // Nothing to load: what an earlier Review put up still goes.
            let reviewed = self.applies.reviewed.clone();
            window.defer(cx, move |_, cx| {
                workbench.update(cx, |workbench, cx| {
                    invalidate(workbench, &reviewed, &turn_id, cx)
                });
            });
            return;
        };
        let open = self.open_paths(&workbench, cx);
        let reviewed = self.applies.reviewed.clone();
        let found = cx.background_spawn(async move {
            blocks
                .into_iter()
                .map(|block| {
                    let located = locate(&root, &block.rel, &open);
                    let (exists, folder) = match &located {
                        Ok(located) => (
                            located.path.exists() || open.contains(&located.path),
                            (located.path.parent()).is_some_and(|folder| folder.is_dir()),
                        ),
                        Err(_) => (false, false),
                    };
                    Found {
                        block,
                        located,
                        exists,
                        folder,
                    }
                })
                .collect::<Vec<_>>()
        });
        let origin = origin_of(actor);
        let chats = self.applies.chats;
        cx.spawn_in(window, async move |panel, cx| {
            let found = found.await;
            (panel.update_in(cx, |panel, window, cx| {
                if panel.applies.chats != chats {
                    return;
                }
                let Some((workbench, pane, files)) = panel.open_for_review(found, window, cx)
                else {
                    return;
                };
                let load = Load {
                    workbench,
                    pane: pane.clone(),
                    files,
                    turn_id,
                    origin,
                    reviewed,
                };
                if !load.reading(cx) {
                    return load.run_when_current(cx.weak_entity(), chats, window, cx);
                }
                // Once every file's read into the pane, or given up on.
                let mut load = Some(load);
                let held: Rc<RefCell<Option<Subscription>>> = Rc::default();
                let watch = cx.observe_in(&pane, window, {
                    let held = held.clone();
                    move |panel, _, window, cx| {
                        let current = panel.applies.chats == chats;
                        let ready = load.as_ref().is_some_and(|load| !load.reading(cx));
                        if ready || !current {
                            held.borrow_mut().take();
                            if let Some(load) = load.take().filter(|_| ready && current) {
                                load.run_when_current(cx.weak_entity(), chats, window, cx);
                            }
                        }
                    }
                });
                *held.borrow_mut() = Some(watch);
            }))
            .ok();
        })
        .detach();
    }

    /// Opens the existing files `found` names in the active pane, and
    /// toasts for those that can't be proposed to, so their blocks are left
    /// out: the workbench, the pane, and the blocks to propose, in order,
    /// the new files among them as `None`.
    #[allow(clippy::type_complexity)]
    fn open_for_review(
        &mut self,
        found: Vec<Found>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<(
        Entity<Workbench>,
        Entity<Pane>,
        Vec<(PathBuf, Option<FileBlock>)>,
    )> {
        let workbench = self.workbench()?;
        let pane = workbench.read(cx).pane().clone();
        let mut files: Vec<(PathBuf, Option<FileBlock>)> = Vec::new();
        for found in found {
            let Found {
                block,
                located,
                exists,
                folder,
            } = found;
            let located = match located {
                Ok(located) if located.read_only => {
                    refused(&block.rel, &tr!("workbench-chat-why-read-only"), cx);
                    continue;
                }
                Ok(located) => located,
                Err(why) => {
                    refused(&block.rel, &why, cx);
                    continue;
                }
            };
            // Two names for one file: the first is the one reviewed.
            if files.iter().any(|(path, _)| *path == located.path) {
                refused(&block.rel, &tr!("workbench-chat-why-another-block"), cx);
                continue;
            }
            if exists {
                pane.update(cx, |pane, cx| pane.open(located.path.clone(), window, cx));
                files.push((located.path, Some(block)));
            } else if !folder {
                refused(&block.rel, &tr!("workbench-chat-why-no-parent"), cx);
            } else if matches!(block.body, EditBody::Sections(_)) {
                refused(&block.rel, &tr!("workbench-chat-why-nothing-to-search"), cx);
            } else {
                files.push((located.path, Some(block)));
            }
        }
        Some((workbench, pane, files))
    }
}

/// The blocks to propose once their files are read.
struct Load {
    workbench: Entity<Workbench>,
    pane: Entity<Pane>,
    files: Vec<(PathBuf, Option<FileBlock>)>,
    turn_id: String,
    origin: Origin,
    reviewed: Reviewed,
}

impl Load {
    /// Whether any of the files is still being read into the pane.
    fn reading(&self, cx: &App) -> bool {
        let pane = self.pane.read(cx);
        pane.reading()
            .any(|reading| self.files.iter().any(|(path, _)| *path == reading))
    }

    /// Loads, once the panel is out of the update it's in (the workbench
    /// reads it as the changeset is shown), and only if the chat, root and
    /// trust are still those the Review was asked under.
    fn run_when_current(
        self,
        panel: WeakEntity<ChatPanel>,
        chats: u64,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.defer(cx, move |window, cx| {
            let current =
                (panel.read_with(cx, |panel, _| panel.applies.chats == chats)).unwrap_or(false);
            if current {
                load_changeset(self, window, cx);
            }
        });
    }
}

/// Proposes each of `files`' blocks in its editor in `pane` and shows the
/// changeset of those that could be, unless none could.
fn load_changeset(load: Load, window: &mut Window, cx: &mut App) {
    let Load {
        workbench,
        pane,
        files,
        turn_id,
        origin,
        reviewed,
    } = load;
    let mut entries = Vec::new();
    for (path, block) in files {
        let Some(FileBlock { rel, body }) = block else {
            continue;
        };
        let Some(editor) = editor_at(&pane, &path, cx) else {
            if path.exists() {
                refused(&rel, &tr!("workbench-chat-why-not-opened"), cx);
            } else if let EditBody::Replace(text) = body {
                entries.push(Entry::New(path, lf(&text)));
            }
            continue;
        };
        match propose_whole(editor.read(cx), body, origin.clone()) {
            Err(why) => refused(&rel, &why, cx),
            Ok(proposal) => {
                let snapshot = editor.read(cx).model().buffer().snapshot();
                if proposal.hunks(&snapshot).is_empty() {
                    let message = tr!("workbench-chat-no-change", file = rel.as_str()).to_string();
                    toast::show(cx, Toast::info(message));
                    continue;
                }
                let id = snapshot.id();
                entries.push(Entry::Existing(path, Rc::new(RefCell::new(proposal)), id));
            }
        }
    }
    // Replaced whether or not this Review loaded anything.
    workbench.update(cx, |workbench, cx| {
        invalidate(workbench, &reviewed, &turn_id, cx)
    });
    if entries.is_empty() {
        return;
    }
    let mut changeset = Changeset::new(turn_id.clone(), origin);
    let mut installed = Vec::new();
    for entry in entries {
        match entry {
            Entry::Existing(path, proposal, buffer) => {
                installed.push(proposal.clone());
                changeset.add_file(path, buffer, proposal)
            }
            Entry::New(path, text) => changeset.add_new_file(path, text),
        }
    }
    reviewed.borrow_mut().insert(turn_id, installed);
    workbench.update(cx, |workbench, cx| {
        workbench.set_changeset(changeset, window, cx);
    });
}

/// Takes what a Review of `turn_id` put up off the open editors, and its
/// changeset if it's the one shown, so a Review in its place isn't beside it.
fn invalidate(
    workbench: &mut Workbench,
    reviewed: &Reviewed,
    turn_id: &str,
    cx: &mut Context<Workbench>,
) {
    if let Some(old) = reviewed.borrow_mut().remove(turn_id) {
        for editor in workbench.open_editors(cx) {
            editor.update(cx, |editor, cx| {
                if !editor
                    .proposals()
                    .iter()
                    .any(|held| old.iter().any(|o| Rc::ptr_eq(held, o)))
                {
                    return;
                }
                let kept: Vec<_> = (editor.proposals().iter())
                    .filter(|held| !old.iter().any(|o| Rc::ptr_eq(held, o)))
                    .cloned()
                    .collect();
                editor.set_proposals(kept, cx);
            });
        }
    }
    if (workbench.ai_changes.as_ref()).is_some_and(|shown| shown.changeset.turn_id() == turn_id) {
        workbench.ai_changes = None;
        workbench.sync_ai_changes(cx);
    }
}

#[cfg(test)]
#[path = "chat_changeset_tests.rs"]
mod chat_changeset_tests;
