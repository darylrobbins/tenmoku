//! The API chat's tool loop (the agents record's § 6 and § 7, card AG10):
//! a [`ToolLoop`] beside each chat's [`ChatView`] runs the calls its turns'
//! replies make, round after round, until a reply asks for none.
//!
//! - **Tools** are declared only in trusted folders, with
//!   `agents.tool_loop` on ([`ToolLoop::offer_tools`]): a chat's first send
//!   freezes them, and a chat begun without them stays without them.
//! - **A round.** When a reply ends asking for tools
//!   ([`ChatViewEvent::ToolCallsWaiting`]), each call is validated into its
//!   [`ToolInput`] and decided: the chat's grants are the ceiling
//!   (`process.spawn:<program>` for `run_command`), then the [`Policy`],
//!   built from `[agents.permissions]` at each turn's start with the
//!   chat's "Allow for this chat" answers kept in it. `allow` runs the
//!   call, `deny` answers it "Denied by your settings", and `ask` shows a
//!   permission row under its tool row ([`ChatView::ask_permission`]) until
//!   the user answers. Independent calls run at once; a turn's
//!   `propose_edit`s run one after another, in the calls' order, as each
//!   builds on the file's view the last left. Every result goes into the
//!   round as it comes ([`ChatView::add_tool_result`]); once all are in,
//!   the next round is sent ([`ChatView::next_round`]). Every decided
//!   request is a line of the audit trail ([`agent_audit`]), a command's
//!   once it has ended.
//! - **Limits** (`[agents]`): a reply that would start a round past
//!   `max_rounds` has its calls answered "The round limit was reached",
//!   and none starts after `turn_time_limit_secs`; the turn then ends with
//!   its result message last, and **Continue** ([`ToolLoop::continue_turn`])
//!   sends it. A command runs at most `command_timeout_secs`.
//! - **Stop** ends the turn ([`ChatViewEvent::TurnFinished`]): commands'
//!   process groups are killed, searches and listings cancelled, reads and
//!   proposals dropped, and the prompts closed as refused. The view answers
//!   every call without a result "Stopped by the user".
//! - **Trust generations.** Each withdrawal of the tools counts one
//!   ([`ToolLoop::withdraw`]): the folder's trust was withdrawn, or its
//!   root changed. A result, a proposal or an answer lands only under the
//!   generation its turn began in. On a withdrawal the turn ends at once:
//!   the stream is dropped, the work is killed or cancelled, and every call
//!   but a finished `propose_edit` is answered "Folder trust was withdrawn",
//!   what it gathered discarded. Nothing is sent: the next send carries the
//!   user's text after those results, then the `tool_removal` message.
//! - **The turn's end** hands its proposals to the AI Changes panel
//!   ([`agent_tools::end_turn`], which calls `Workbench::set_changeset`).
//!
//! The results' text is the model's, in English, as `fe_chat::tools`'
//! descriptions promise; what the user reads is in the rows, which
//! fe-chat-view words.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ffi::OsString,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use fe_buffer::Origin;
use fe_chat::{
    ChatId, InputError, LIST_FILES, NotRun, PROPOSE_EDIT, READ_FILE, RUN_COMMAND, SEARCH, ToolCall,
    ToolInput, ToolResult,
    permissions::{Class, Level, Policy, Reason, Request},
    tool_set,
};
use fe_chat_view::{
    ChatView, ChatViewEvent, CommandOutcome, PermissionAnswer, PermissionChoices, PermissionPrompt,
    ToolEvent, ToolKind, ToolRow, ToolStatus, ToolTitle,
};
use fe_command::{Actor, Invocation, Source};
use fe_config::agent_settings::{AgentLevel, AgentPermissions, AgentSettings};
use gpui::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, Subscription, Task, WeakEntity, Window,
};
use kiln::command::Grants;

use crate::{
    Workbench,
    agent_audit::{self, API_CHAT, Destination, Entry, Killed, Verdict},
    agent_command::{self, Decider, Decision, End, Limits, Prepared, Ran, RunningCommand},
    agent_tools::{self, ChatProposals, ToolOutcome},
};

#[cfg(test)]
#[path = "chat_tool_loop_tests.rs"]
mod tests;

// The model's text.
const DENIED_BY_SETTINGS: &str = "Denied by your settings";
const DENIED_BY_USER: &str = "Denied by the user";
const NOT_GRANTED: &str = "Denied: the chat isn't allowed to run this program";

/// One chat's tool loop: see the module docs. Made with its view, and
/// dropped with it, which drops (and so cancels) whatever it runs.
pub(crate) struct ToolLoop {
    view: WeakEntity<ChatView>,
    window: AnyWindowHandle,
    workbench: Option<WeakEntity<Workbench>>,
    /// The ceiling: what the chat may have Tenmoku do.
    grants: Grants,
    /// The levels and patterns of `[agents.permissions]`, set again at
    /// each turn's start, and the chat's "Allow for this chat" answers.
    policy: Policy,
    proposals: Rc<RefCell<ChatProposals>>,
    /// The root the chat's tools were declared under, while it has them.
    chat_root: Option<PathBuf>,
    /// Counts the withdrawals of the tools: work begun under an earlier
    /// count never lands.
    generation: u64,
    /// Numbers each call run, so what a call's work comes to lands only on
    /// that call, never on a later one with its id.
    runs: u64,
    turn: Option<LoopTurn>,
    /// The last turn's proposals being handed to the AI Changes panel: the
    /// next turn's proposals begin, and its edits run, once it's done.
    handing_in: Option<Task<()>>,
    _view: [Subscription; 3],
}

/// The turn the loop is running.
struct LoopTurn {
    /// The turn's index in the conversation.
    index: usize,
    chat: ChatId,
    /// The generation it began in.
    generation: u64,
    root: PathBuf,
    /// When it began, or went on with Continue or Retry.
    started: Instant,
    /// Its committed rounds then: the rounds it has made since count.
    rounds_at_start: usize,
    max_rounds: usize,
    time_limit: Duration,
    command_limits: Limits,
    /// Where its audit lines go, taken while the folder was trusted.
    audit: Option<Destination>,
    /// The calls of the round under way, by id.
    calls: HashMap<String, Call>,
    /// The order the calls came in.
    order: Vec<String>,
    /// Whether the chat's proposals began this turn: not while the last
    /// turn's are being handed in.
    proposals_begun: bool,
}

/// A call of the round under way.
struct Call {
    /// The run it is ([`ToolLoop::runs`]).
    run: u64,
    input: ToolInput,
    row: ToolRow,
    /// The policy's request, once known (a command's, once prepared).
    request: Option<Request>,
    prepared: Option<Prepared>,
    state: CallState,
}

enum CallState {
    /// A command's program being resolved.
    Preparing(#[allow(dead_code, reason = "held to keep it running")] Task<()>),
    /// Waiting for the user's answer.
    Asking,
    /// A `propose_edit` allowed, waiting for the one before it.
    Queued,
    Running {
        task: Task<()>,
        /// Cancels a search, a listing or a command.
        cancel: Option<Arc<AtomicBool>>,
        /// Why a command was killed, as its audit line says.
        killed: Option<Rc<Cell<Killed>>>,
    },
    Done,
}

impl ToolLoop {
    /// The loop of `view`'s chat, with `grants` as its ceiling.
    pub(crate) fn new(
        view: &Entity<ChatView>,
        grants: Grants,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ToolLoop {
        let events = cx.subscribe_in(
            view,
            window,
            |this, _, event: &ChatViewEvent, window, cx| match event {
                ChatViewEvent::ToolCallsWaiting { turn } => this.run_round(*turn, window, cx),
                ChatViewEvent::TurnFinished => this.turn_finished(cx),
                _ => {}
            },
        );
        let tools =
            cx.subscribe_in(
                view,
                window,
                |this, _, event: &ToolEvent, window, cx| match event {
                    ToolEvent::PermissionAnswered { turn, tool, answer } => {
                        this.answered(*turn, tool, answer, window, cx)
                    }
                    ToolEvent::ReviewProposal { .. } => this.review(window, cx),
                },
            );
        let observed = cx.observe(view, |this, view, cx| this.follow(&view, cx));
        ToolLoop {
            view: view.downgrade(),
            window: window.window_handle(),
            workbench: None,
            grants,
            policy: Policy::new(),
            proposals: Rc::new(RefCell::new(ChatProposals::new())),
            chat_root: None,
            generation: 0,
            runs: 0,
            turn: None,
            handing_in: None,
            _view: [events, tools, observed],
        }
    }

    /// Follows `workbench`, whose root and trust decide the tools: an empty
    /// chat is offered them now, and one that has them keeps them only
    /// under the root and trust it began with.
    pub(crate) fn attach(&mut self, workbench: &Entity<Workbench>, cx: &mut Context<Self>) {
        self.workbench = Some(workbench.downgrade());
        if self.chat_root.is_none() {
            self.chat_root = self.trusted_root(cx);
        }
        self.folder_changed(cx);
    }

    /// The grants changed.
    #[cfg(test)]
    pub(crate) fn set_grants(&mut self, grants: Grants) {
        self.grants = grants;
    }

    /// The root, or its trust, changed: an empty chat is offered the tools
    /// as they stand now; a chat with tools whose folder lost its trust,
    /// or whose root changed, has them withdrawn.
    pub(crate) fn folder_changed(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.view.upgrade() else {
            return;
        };
        let conversation = view.read(cx).conversation();
        if conversation.turns.is_empty() {
            self.offer_tools(cx);
            return;
        }
        if !conversation.has_tools() {
            return;
        }
        let now = self.trusted_root(cx);
        if now.is_none() || now != self.chat_root {
            self.withdraw(cx);
        }
    }

    /// Declares the tools of a chat not yet sent: the five in a trusted
    /// folder with `agents.tool_loop` on, else none.
    pub(crate) fn offer_tools(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.view.upgrade() else {
            return;
        };
        let settings = agent_settings(cx);
        let root = self.trusted_root(cx).filter(|_| settings.tool_loop);
        let tools = if root.is_some() {
            tool_set()
        } else {
            Vec::new()
        };
        view.update(cx, |view, _| view.set_tools(tools, settings.task_budget));
        self.chat_root = root;
    }

    /// The workbench's root, if it has one and it's trusted.
    fn trusted_root(&self, cx: &App) -> Option<PathBuf> {
        let workbench = self.workbench.as_ref()?.upgrade()?;
        let workbench = workbench.read(cx);
        workbench
            .root()
            .filter(|_| workbench.root_trusted())
            .map(Path::to_owned)
    }

    /// Withdraws the chat's tools: the folder's trust was withdrawn, or its
    /// root changed. A turn under way ends at once, its work killed or
    /// cancelled and nothing of it sent (see the module docs).
    pub(crate) fn withdraw(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        self.chat_root = None;
        self.finish_turn(Killed::TrustWithdrawn, cx);
        if let Some(view) = self.view.upgrade() {
            view.update(cx, |view, cx| view.withdraw_tools(cx));
        }
    }

    /// The chat is replaced: a turn under way ends as Stop ends it, its
    /// work killed or cancelled, and its proposals still go to the AI
    /// Changes panel, as does a hand-in under way, once the loop is gone.
    pub(crate) fn shut_down(&mut self, cx: &mut Context<Self>) {
        self.finish_turn(Killed::Stop, cx);
        if let Some(handing_in) = self.handing_in.take() {
            handing_in.detach();
        }
    }

    /// Whether the last turn ended at the round or time limit and can go on.
    pub(crate) fn can_continue(&self, cx: &App) -> bool {
        self.view
            .upgrade()
            .is_some_and(|view| view.read(cx).can_continue())
    }

    /// Continue: the last turn, ended at the round or time limit, sends its
    /// result message, with its limits counted again from now.
    pub(crate) fn continue_turn(&mut self, cx: &mut Context<Self>) {
        if self.trusted_root(cx).is_none() || self.trusted_root(cx) != self.chat_root {
            self.withdraw(cx);
            return;
        }
        if let Some(view) = self.view.upgrade() {
            view.update(cx, |view, cx| view.continue_turn(cx));
        }
    }

    /// A turn of the view's ended: the loop's ends with it, unless it's
    /// still streaming (an earlier turn's end, reported after it began).
    fn turn_finished(&mut self, cx: &mut Context<Self>) {
        let streaming = self.view.upgrade().and_then(|view| {
            let conversation = view.read(cx).conversation();
            conversation
                .streaming_turn()
                .map(|_| conversation.turns.len() - 1)
        });
        if streaming.is_some() && streaming == self.turn.as_ref().map(|turn| turn.index) {
            return;
        }
        self.finish_turn(Killed::Stop, cx);
    }

    /// Begins following a turn as it starts streaming: sent, retried or
    /// continued.
    fn follow(&mut self, view: &Entity<ChatView>, cx: &mut Context<Self>) {
        let conversation = view.read(cx).conversation();
        let Some(turn) = conversation.streaming_turn() else {
            return;
        };
        let index = conversation.turns.len() - 1;
        if self.turn.as_ref().is_some_and(|turn| turn.index == index) {
            return;
        }
        if !conversation.has_tools() {
            return;
        }
        let (chat, rounds) = (conversation.id, turn.rounds.len());
        self.begin(index, chat, rounds, cx);
    }

    /// Starts turn `index` of chat `chat`, which has `rounds` committed
    /// rounds, under the root and generation now. A chat whose folder isn't
    /// trusted, or isn't the one its tools were declared under, has them
    /// withdrawn instead.
    fn begin(&mut self, index: usize, chat: ChatId, rounds: usize, cx: &mut Context<Self>) {
        self.finish_turn(Killed::Stop, cx);
        let root = self.trusted_root(cx);
        let Some(root) = root.filter(|root| Some(root) == self.chat_root.as_ref()) else {
            self.withdraw(cx);
            return;
        };
        let settings = agent_settings(cx);
        set_policy(&mut self.policy, &settings.effective_permissions());
        let session = chat.to_string();
        let origin = Origin::Ai {
            agent: API_CHAT.into(),
            session: session.as_str().into(),
        };
        let proposals_begun = self.handing_in.is_none();
        if proposals_begun {
            (self.proposals.borrow_mut()).begin_turn(format!("{session}:{index}"), origin);
        }
        let audit = agent_audit::destination(&root, cx);
        self.turn = Some(LoopTurn {
            index,
            chat,
            generation: self.generation,
            root,
            started: Instant::now(),
            rounds_at_start: rounds,
            max_rounds: settings.max_rounds.max(1) as usize,
            time_limit: Duration::from_secs(u64::from(settings.turn_time_limit_secs)),
            command_limits: Limits::secs(settings.command_timeout_secs),
            audit,
            calls: HashMap::new(),
            order: Vec::new(),
            proposals_begun,
        });
    }

    /// The reply of turn `index` asks for tools: its calls are decided and
    /// run, unless the round or time limit says it ends.
    fn run_round(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.view.upgrade() else {
            return;
        };
        let conversation = view.read(cx).conversation();
        let calls = conversation.pending_calls();
        let Some(turn) = conversation.streaming_turn() else {
            return;
        };
        if calls.is_empty() || index + 1 != conversation.turns.len() {
            return;
        }
        let (chat, rounds) = (conversation.id, turn.rounds.len());
        if self.turn.as_ref().is_none_or(|turn| turn.index != index) {
            self.begin(index, chat, rounds, cx);
        }
        if self.trusted_root(cx) != self.chat_root || self.chat_root.is_none() {
            self.withdraw(cx);
            return;
        }
        let Some(turn) = self.turn.as_mut() else {
            return;
        };
        // This reply is round `round` of the turn; running its calls would
        // start the next one.
        let round = rounds - turn.rounds_at_start + 1;
        let limit = if round >= turn.max_rounds {
            Some(NotRun::RoundLimit)
        } else if turn.started.elapsed() >= turn.time_limit {
            Some(NotRun::TimeLimit)
        } else {
            None
        };
        if let Some(why) = limit {
            for call in &calls {
                let row = row_of(
                    call,
                    ToolInput::parse(&call.name, &call.input).ok().as_ref(),
                );
                set_row(
                    &view,
                    index,
                    row_ended(row, ToolStatus::Failed, why.text()),
                    cx,
                );
            }
            view.update(cx, |view, cx| view.end_round(why, cx));
            return;
        }
        turn.calls.clear();
        turn.order = calls.iter().map(|call| call.id.clone()).collect();
        for call in calls {
            self.start_call(call, window, cx);
        }
        self.advance(window, cx);
    }

    /// The call `id` of the round under way.
    fn call_mut(&mut self, id: &str) -> Option<&mut Call> {
        self.turn.as_mut()?.calls.get_mut(id)
    }

    /// Call `id`, if it's still run `run`: its turn and round still under
    /// way, under the generation it began in.
    fn current(&mut self, id: &str, run: u64) -> Option<&mut Call> {
        let generation = self.generation;
        let turn = self
            .turn
            .as_mut()
            .filter(|turn| turn.generation == generation)?;
        turn.calls.get_mut(id).filter(|call| call.run == run)
    }

    /// Validates `call` and decides it, or answers it at once if it can't
    /// run.
    fn start_call(&mut self, call: ToolCall, window: &mut Window, cx: &mut Context<Self>) {
        let Some(turn) = self.turn.as_ref() else {
            return;
        };
        let (index, chat, root) = (turn.index, turn.chat, turn.root.clone());
        self.runs += 1;
        let run = self.runs;
        let Some(view) = self.view.upgrade() else {
            return;
        };
        let input = match ToolInput::parse(&call.name, &call.input) {
            Ok(input) => input,
            Err(error) => {
                let content = match error {
                    InputError::UnknownTool(name) => format!("There is no tool named `{name}`"),
                    InputError::Invalid(_) => {
                        serde_json::json!({ "INVALID_JSON": call.input }).to_string()
                    }
                };
                let row = row_ended(row_of(&call, None), ToolStatus::Failed, &content);
                set_row(&view, index, row, cx);
                view.update(cx, |view, cx| {
                    view.add_tool_result(ToolResult::error(&call.id, content), cx)
                });
                return;
            }
        };
        let row = row_of(&call, Some(&input));
        set_row(&view, index, row.clone(), cx);
        let id = call.id;
        let mut entry = Call {
            run,
            input: input.clone(),
            row,
            request: None,
            prepared: None,
            state: CallState::Asking,
        };
        if let ToolInput::RunCommand(command) = input {
            // Resolving the program reads the disk, and the login `PATH`
            // runs the login shell the first time.
            let path = command_path(cx);
            let prepare = cx.background_spawn(async move {
                let path = path.or_else(agent_command::login_path);
                let folders = [root.clone()];
                agent_command::prepare(&command, &root, &folders, path.as_deref())
            });
            let call_id = id.clone();
            entry.state = CallState::Preparing(cx.spawn_in(window, async move |this, cx| {
                let prepared = prepare.await;
                this.update_in(cx, |this, window, cx| {
                    this.prepared(&call_id, run, prepared, window, cx)
                })
                .ok();
            }));
            if let Some(turn) = self.turn.as_mut() {
                turn.calls.insert(id, entry);
            }
            return;
        }
        let (class, target) = plain_target(&entry.input);
        let request = Request::plain(chat, class, target);
        let (level, reason) = self.policy.decide_with_reason(&request);
        entry.request = Some(request);
        if let Some(turn) = self.turn.as_mut() {
            turn.calls.insert(id.clone(), entry);
        }
        let decision = Decision {
            level,
            by: decider(reason),
        };
        self.decided(&id, decision, window, cx);
    }

    /// A command's program was resolved, or why it can't run: it's decided
    /// now, if it's still run `run` of call `id`.
    fn prepared(
        &mut self,
        id: &str,
        run: u64,
        prepared: Result<Prepared, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.current(id, run).is_none() {
            return;
        }
        let Some(chat) = self.turn.as_ref().map(|turn| turn.chat) else {
            return;
        };
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(why) => {
                self.land(id, run, ToolResult::error(id, why), ToolStatus::Failed, cx);
                self.advance(window, cx);
                return;
            }
        };
        let decision = agent_command::decide(&self.grants, &self.policy, chat, &prepared);
        let Some(call) = self.call_mut(id) else {
            return;
        };
        call.request = Some(prepared.request(chat));
        call.prepared = Some(prepared);
        call.state = CallState::Asking;
        self.decided(id, decision, window, cx);
    }

    /// Acts on what was decided of call `id`: runs it, asks the user, or
    /// answers it denied.
    fn decided(
        &mut self,
        id: &str,
        decision: Decision,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(turn) = self.turn.as_ref() else {
            return;
        };
        let index = turn.index;
        let Some(call) = turn.calls.get(id) else {
            return;
        };
        let (run, command, prompt) = (call.run, call.prepared.is_some(), prompt_for(call));
        match decision.level {
            Level::Allow => self.allowed(id, decision.by, window, cx),
            Level::Deny => {
                let text = match decision.by {
                    Decider::Hard if command => NOT_GRANTED,
                    _ => DENIED_BY_SETTINGS,
                };
                self.audit(id, Verdict::Deny, &decision.by, cx);
                let result = ToolResult::error(id, text);
                self.land(id, run, result, ToolStatus::Denied, cx);
                self.advance(window, cx);
            }
            Level::Ask => {
                if let Some(view) = self.view.upgrade() {
                    let id = id.to_owned();
                    view.update(cx, |view, cx| view.ask_permission(index, id, prompt, cx));
                }
            }
        }
    }

    /// The user answered the prompt about call `tool` of turn `index`.
    fn answered(
        &mut self,
        index: usize,
        tool: &str,
        answer: &PermissionAnswer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(turn) = self.turn.as_ref() else {
            return;
        };
        // An answer from an earlier turn or generation doesn't land.
        if turn.index != index || turn.generation != self.generation {
            return;
        }
        let Some(call) = turn.calls.get(tool) else {
            return;
        };
        if !matches!(call.state, CallState::Asking) {
            return;
        }
        let (run, request) = (call.run, call.request.clone());
        match answer {
            // Stop closed it: the turn's end answers it.
            PermissionAnswer::Stopped => {}
            PermissionAnswer::AllowOnce => self.allowed(tool, Decider::User, window, cx),
            PermissionAnswer::AllowForChat => {
                if let Some(request) = &request {
                    self.policy.remember(request);
                }
                self.allowed(tool, Decider::User, window, cx);
            }
            PermissionAnswer::Deny | PermissionAnswer::Option(_) => {
                self.audit(tool, Verdict::Deny, &Decider::User, cx);
                let result = ToolResult::error(tool, DENIED_BY_USER);
                self.land(tool, run, result, ToolStatus::Denied, cx);
                self.advance(window, cx);
            }
        }
    }

    /// Call `id` may run: a `propose_edit` waits for every one before it
    /// in the calls' order ([`ToolLoop::advance`]), a command's audit line
    /// waits for its end, anything else runs now.
    fn allowed(&mut self, id: &str, by: Decider, window: &mut Window, cx: &mut Context<Self>) {
        let Some(call) = self.call_mut(id) else {
            return;
        };
        match call.input {
            ToolInput::RunCommand(_) => self.run_command(id, by, window, cx),
            ToolInput::ProposeEdit(_) => {
                call.state = CallState::Queued;
                self.audit(id, Verdict::Allow, &by, cx);
                self.advance(window, cx);
            }
            _ => {
                self.audit(id, Verdict::Allow, &by, cx);
                self.run(id, window, cx);
            }
        }
    }

    /// Runs call `id`, not a command, landing its outcome under its
    /// turn's generation.
    fn run(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workbench) = self.workbench.as_ref().and_then(WeakEntity::upgrade) else {
            return;
        };
        let Some(turn) = self.turn.as_ref() else {
            return;
        };
        let (root, chat) = (turn.root.clone(), turn.chat);
        let Some((run, input)) = turn
            .calls
            .get(id)
            .map(|call| (call.run, call.input.clone()))
        else {
            return;
        };
        let name = tool_name(&input);
        let cancel = Arc::new(AtomicBool::new(false));
        let work: Task<ToolOutcome> = match input {
            ToolInput::ReadFile(input) => {
                agent_tools::read_file(&workbench, &self.proposals, input, window, cx)
            }
            ToolInput::ProposeEdit(input) => {
                agent_tools::propose_edit(&workbench, &self.proposals, input, window, cx)
            }
            ToolInput::ListFiles(input) => {
                let cancel = cancel.clone();
                cx.background_spawn(async move { agent_tools::list_files(&root, &input, cancel) })
            }
            ToolInput::Search(input) => {
                let cancel = cancel.clone();
                cx.background_spawn(async move { agent_tools::search(&root, &input, cancel) })
            }
            ToolInput::RunCommand(_) => return,
        };
        let held = held(name, cx);
        let call_id = id.to_owned();
        let task = cx.spawn_in(window, async move |this, cx| {
            let outcome = work.await;
            if let Some(held) = held {
                let _ = held.recv().await;
            }
            this.update_in(cx, |this, window, cx| {
                this.ran(&call_id, run, outcome, window, cx)
            })
            .ok();
        });
        self.running(id, task, Some(cancel), None, cx);
        notify_invocation(chat, name, cx);
    }

    /// Runs command `id`, allowed by `by`: its audit line is written when
    /// it has ended, whatever became of its turn or of the loop.
    fn run_command(&mut self, id: &str, by: Decider, window: &mut Window, cx: &mut Context<Self>) {
        let Some(turn) = self.turn.as_ref() else {
            return;
        };
        let (limits, audit, chat) = (turn.command_limits, turn.audit.clone(), turn.chat);
        let Some((run, prepared)) =
            (turn.calls.get(id)).and_then(|call| Some((call.run, call.prepared.clone()?)))
        else {
            return;
        };
        let killed = Rc::new(Cell::new(Killed::Stop));
        let entry = Entry::command(chat, &prepared, Verdict::Allow, &by);
        let (call_id, why, handle) = (id.to_owned(), killed.clone(), window.window_handle());
        let started = RunningCommand::start(prepared, limits);
        let cancel = started.as_ref().ok().map(RunningCommand::canceller);
        let task = cx.spawn(async move |this, cx| {
            let ran = match started {
                Ok(running) => running.finished().await,
                Err(error) => Ran {
                    output: Default::default(),
                    end: End::NotStarted(error.to_string()),
                    timeout: limits.timeout,
                    elapsed: Duration::ZERO,
                },
            };
            // Written whether or not the loop is still there to take the
            // result: a chat replaced mid-command still records the kill.
            cx.update(|cx| {
                if let Some(audit) = &audit {
                    audit.record(&entry.ended(&ran, why.get()), cx);
                }
            });
            let _ = cx.update_window(handle, |_, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    this.command_ended(&call_id, run, ran, window, cx)
                });
            });
        });
        self.running(id, task, cancel, Some(killed), cx);
        notify_invocation(chat, RUN_COMMAND, cx);
    }

    /// Call `id` runs as `task`.
    fn running(
        &mut self,
        id: &str,
        task: Task<()>,
        cancel: Option<Arc<AtomicBool>>,
        killed: Option<Rc<Cell<Killed>>>,
        cx: &mut Context<Self>,
    ) {
        let Some(call) = self.call_mut(id) else {
            return;
        };
        call.state = CallState::Running {
            task,
            cancel,
            killed,
        };
        call.row.status = ToolStatus::Running;
        let row = call.row.clone();
        self.show_row(row, cx);
    }

    /// Command `id` ended: its result lands, if it's still run `run`.
    fn command_ended(
        &mut self,
        id: &str,
        run: u64,
        ran: Ran,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let outcome = match ran.end {
            End::Exited(code) => Some(CommandOutcome::Exited {
                code,
                elapsed: ran.elapsed,
            }),
            End::Killed(_) | End::Signalled(_) => Some(CommandOutcome::Killed {
                elapsed: ran.elapsed,
            }),
            End::NotStarted(_) => None,
        };
        let status = if ran.is_error() {
            ToolStatus::Failed
        } else {
            ToolStatus::Done
        };
        if let Some(call) = self.current(id, run)
            && let ToolTitle::RunCommand { outcome: shown, .. } = &mut call.row.title
        {
            *shown = outcome;
        }
        let result = ToolResult {
            tool_use_id: id.to_owned(),
            content: ran.result_text(),
            is_error: ran.is_error(),
        };
        self.land(id, run, result, status, cx);
        self.advance(window, cx);
    }

    /// Call `id`, not a command, came to `outcome`.
    fn ran(
        &mut self,
        id: &str,
        run: u64,
        outcome: ToolOutcome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let status = match outcome {
            Ok(_) => ToolStatus::Done,
            Err(_) => ToolStatus::Failed,
        };
        if let Some(call) = self.current(id, run) {
            note_outcome(&mut call.row, outcome.as_ref());
        }
        let result = match outcome {
            Ok(text) => ToolResult::ok(id, text),
            Err(why) => ToolResult::error(id, why),
        };
        self.land(id, run, result, status, cx);
        self.advance(window, cx);
    }

    /// Adds call `id`'s `result` to its round, if it's still run `run`
    /// ([`ToolLoop::current`]) and has no result yet.
    fn land(
        &mut self,
        id: &str,
        run: u64,
        result: ToolResult,
        status: ToolStatus,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.turn.as_ref().map(|turn| turn.index) else {
            return;
        };
        let Some(call) = self.current(id, run) else {
            return;
        };
        if matches!(call.state, CallState::Done) {
            return;
        }
        call.state = CallState::Done;
        call.row.status = status;
        if call.row.output.is_none() || status != ToolStatus::Done {
            call.row.output = Some(result.content.clone());
        }
        let row = call.row.clone();
        if let Some(view) = self.view.upgrade() {
            set_row(&view, index, row, cx);
            view.update(cx, |view, cx| view.add_tool_result(result, cx));
        }
    }

    /// Moves the round on: the next `propose_edit` runs once the one
    /// before it is done, and once every call has its result the next
    /// round is sent, unless the time is up.
    fn advance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(turn) = self.turn.as_mut() else {
            return;
        };
        // The first edit in the calls' order not yet done runs once it's
        // allowed, and once the last turn's proposals were handed in; the
        // ones after it wait, whatever was answered first.
        let next_edit = (turn.order.iter())
            .filter_map(|id| turn.calls.get(id).map(|call| (id, call)))
            .find(|(_, call)| {
                matches!(call.input, ToolInput::ProposeEdit(_))
                    && !matches!(call.state, CallState::Done)
            });
        if let Some((id, call)) = next_edit
            && matches!(call.state, CallState::Queued)
            && turn.proposals_begun
        {
            let id = id.clone();
            self.run(&id, window, cx);
            return;
        }
        if turn.calls.is_empty() && turn.order.is_empty() {
            return;
        }
        if (turn.calls.values()).any(|call| !matches!(call.state, CallState::Done)) {
            return;
        }
        let Some(view) = self.view.upgrade() else {
            return;
        };
        if !view.read(cx).conversation().pending_calls().is_empty() {
            return;
        }
        let out_of_time = turn.started.elapsed() >= turn.time_limit;
        turn.calls.clear();
        turn.order.clear();
        view.update(cx, |view, cx| {
            if out_of_time || !view.next_round(cx) {
                let why = if out_of_time {
                    NotRun::TimeLimit
                } else {
                    NotRun::Ended
                };
                view.end_round(why, cx);
            }
        });
    }

    /// The turn ended, or is ended now: its work is killed or cancelled
    /// for `why`, its prompts' rows say they closed, and its proposals go
    /// to the AI Changes panel.
    fn finish_turn(&mut self, why: Killed, cx: &mut Context<Self>) {
        let Some(mut turn) = self.turn.take() else {
            return;
        };
        let text = match why {
            Killed::TrustWithdrawn => NotRun::TrustWithdrawn.text(),
            _ => NotRun::Stopped.text(),
        };
        let view = self.view.upgrade();
        let ids: Vec<String> = turn.order.clone();
        for id in ids {
            let Some(mut call) = turn.calls.remove(&id) else {
                continue;
            };
            let state = std::mem::replace(&mut call.state, CallState::Done);
            match state {
                CallState::Done => {
                    // Its result stands, but for a withdrawal, whose rows
                    // say what was discarded: all but a finished proposal's.
                    let proposal = matches!(call.input, ToolInput::ProposeEdit(_));
                    if why == Killed::TrustWithdrawn && !proposal {
                        call.row.status = ToolStatus::Failed;
                        call.row.output = Some(text.to_owned());
                    } else {
                        continue;
                    }
                }
                CallState::Asking => {
                    if call.request.is_some() {
                        let (decider, verdict) = (Decider::User, Verdict::Cancelled);
                        audit_call(&turn, &call, verdict, &decider, cx);
                    }
                    call.row.status = ToolStatus::Denied;
                    call.row.output = Some(text.to_owned());
                }
                CallState::Preparing(_) | CallState::Queued => {
                    call.row.status = ToolStatus::Failed;
                    call.row.output = Some(text.to_owned());
                }
                CallState::Running {
                    task,
                    cancel,
                    killed,
                } => {
                    if let Some(cancel) = &cancel {
                        cancel.store(true, Ordering::SeqCst);
                    }
                    match killed {
                        // A command is killed through its canceller; its
                        // task goes on to write how it ended.
                        Some(killed) => {
                            killed.set(why);
                            task.detach();
                        }
                        None => drop(task),
                    }
                    call.row.status = ToolStatus::Failed;
                    call.row.output = Some(text.to_owned());
                }
            }
            if let Some(view) = &view {
                set_row(view, turn.index, call.row, cx);
            }
        }
        // The prompts close, as refused.
        if let Some(view) = &view {
            let index = turn.index;
            view.update(cx, |view, cx| {
                for (at, tool) in view.waiting_permissions() {
                    if at == index {
                        view.withdraw_permission(at, &tool, cx);
                    }
                }
            });
        }
        if turn.proposals_begun {
            self.hand_in(cx);
        }
    }

    /// Hands the turn's proposals to the AI Changes panel, once the window
    /// is free. Until it's done the next turn's proposals don't begin, so
    /// the hand-in ends only its own turn's.
    fn hand_in(&mut self, cx: &mut Context<Self>) {
        let (handle, proposals) = (self.window, self.proposals.clone());
        let Some(workbench) = self.workbench.as_ref().and_then(WeakEntity::upgrade) else {
            return;
        };
        self.handing_in = Some(cx.spawn(async move |this, cx| {
            let ended = cx.update_window(handle, |_, window, cx| {
                agent_tools::end_turn(&workbench, &proposals, window, cx)
            });
            if let Ok(ended) = ended {
                ended.await;
            }
            let _ = cx.update_window(handle, |_, window, cx| {
                let _ = this.update(cx, |this, cx| this.handed_in(window, cx));
            });
        }));
    }

    /// The last turn's proposals are handed in: the turn under way's
    /// begin, and its edits run.
    fn handed_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.handing_in = None;
        let Some(turn) = self.turn.as_mut().filter(|turn| !turn.proposals_begun) else {
            return;
        };
        turn.proposals_begun = true;
        let session = turn.chat.to_string();
        let origin = Origin::Ai {
            agent: API_CHAT.into(),
            session: session.as_str().into(),
        };
        let id = format!("{session}:{}", turn.index);
        self.proposals.borrow_mut().begin_turn(id, origin);
        self.advance(window, cx);
    }

    /// Review on a proposal's row shows the AI Changes panel.
    fn review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workbench) = self.workbench.as_ref().and_then(WeakEntity::upgrade) else {
            return;
        };
        workbench.update(cx, |workbench, cx| {
            workbench.run_dock_op(crate::dock::DockOp::ShowAiChanges, window, cx)
        });
    }

    /// Records the decision on call `id` in the turn's audit trail.
    fn audit(&mut self, id: &str, verdict: Verdict, by: &Decider, cx: &mut Context<Self>) {
        let Some(turn) = self.turn.as_ref() else {
            return;
        };
        if let Some(call) = turn.calls.get(id) {
            audit_call(turn, call, verdict, by, cx);
        }
    }

    /// Shows `row` in the turn's transcript.
    fn show_row(&self, row: ToolRow, cx: &mut Context<Self>) {
        if let (Some(view), Some(turn)) = (self.view.upgrade(), self.turn.as_ref()) {
            set_row(&view, turn.index, row, cx);
        }
    }
}

impl Drop for ToolLoop {
    /// Searches and listings still running stop: dropping their tasks
    /// doesn't reach the walks on the background executor.
    fn drop(&mut self) {
        let Some(turn) = &self.turn else {
            return;
        };
        for call in turn.calls.values() {
            if let CallState::Running {
                cancel: Some(cancel),
                ..
            } = &call.state
            {
                cancel.store(true, Ordering::SeqCst);
            }
        }
    }
}

/// Tells the app's invocation observers of a call run for `chat`: by the
/// chat's AI actor, from a script (chat record § 9).
fn notify_invocation(chat: ChatId, tool: &str, cx: &App) {
    let Ok(invocation) = Invocation::of(&format!("chat.tool.{tool}")) else {
        return;
    };
    let invocation = invocation
        .actor(Actor::Ai {
            agent: API_CHAT.to_owned(),
            session: chat.to_string(),
        })
        .source(Source::Script);
    kiln::platform::commands::notify_observers(&invocation, cx);
}

/// Writes `call`'s decision to `turn`'s audit trail.
fn audit_call(turn: &LoopTurn, call: &Call, verdict: Verdict, by: &Decider, cx: &mut App) {
    let Some(audit) = &turn.audit else {
        return;
    };
    let entry = match (&call.prepared, &call.request) {
        (Some(prepared), _) => Entry::command(turn.chat, prepared, verdict, by),
        (None, Some(request)) => {
            let target = match &request.target {
                fe_chat::permissions::Target::Plain(text) => text.clone(),
                _ => String::new(),
            };
            Entry::new(
                API_CHAT,
                turn.chat,
                request.class,
                agent_audit::Target::Text(target),
                verdict,
                by,
            )
        }
        (None, None) => return,
    };
    audit.record(&entry, cx);
}

/// The `[agents]` settings now.
fn agent_settings(cx: &App) -> AgentSettings {
    kiln::platform::config::settings::<fe_config::Settings>(cx)
        .agents
        .clone()
}

/// Sets `policy`'s levels and patterns from `permissions`, keeping its
/// memory.
fn set_policy(policy: &mut Policy, permissions: &AgentPermissions) {
    let level = |level: AgentLevel| match level {
        AgentLevel::Allow => Level::Allow,
        AgentLevel::Ask => Level::Ask,
        AgentLevel::Deny => Level::Deny,
    };
    for (class, set) in [
        (Class::Read, permissions.read),
        (Class::Search, permissions.search),
        (Class::ProposeEdit, permissions.propose_edit),
        (Class::RunCommand, permissions.run_command),
        (Class::Fetch, permissions.fetch),
        (Class::Other, permissions.other),
    ] {
        policy.set_level(class, level(set));
    }
    policy
        .allow_commands(&permissions.commands_allow)
        .ask_commands(&permissions.commands_ask)
        .deny_commands(&permissions.commands_deny);
}

/// Who decided, by the policy's reason.
fn decider(reason: Reason) -> Decider {
    match reason {
        Reason::Class => Decider::Policy,
        Reason::Pattern(pattern) => Decider::Rule(pattern),
        Reason::Remembered => Decider::Session,
        Reason::Hard => Decider::Hard,
    }
}

/// The class and target of a call that isn't a command.
fn plain_target(input: &ToolInput) -> (Class, String) {
    match input {
        ToolInput::ReadFile(input) => (Class::Read, input.path.clone()),
        ToolInput::ListFiles(input) => (
            Class::Read,
            input.dir.clone().unwrap_or_else(|| ".".to_owned()),
        ),
        ToolInput::Search(input) => (Class::Search, input.pattern.clone()),
        ToolInput::ProposeEdit(input) => (Class::ProposeEdit, input.path.clone()),
        ToolInput::RunCommand(input) => (Class::RunCommand, input.program.clone()),
    }
}

/// The tool a validated input is for.
fn tool_name(input: &ToolInput) -> &'static str {
    match input {
        ToolInput::ReadFile(_) => READ_FILE,
        ToolInput::ListFiles(_) => LIST_FILES,
        ToolInput::Search(_) => SEARCH,
        ToolInput::ProposeEdit(_) => PROPOSE_EDIT,
        ToolInput::RunCommand(_) => RUN_COMMAND,
    }
}

/// The permission row for `call`, decided `ask`.
fn prompt_for(call: &Call) -> PermissionPrompt {
    match &call.prepared {
        Some(prepared) => {
            let mut words = vec![prepared.program.path().display().to_string()];
            words.extend(prepared.argv.iter().skip(1).cloned());
            PermissionPrompt {
                class: Class::RunCommand,
                target: words.join(" "),
                cwd: Some(prepared.cwd.display().to_string()),
                in_project: true,
                // A program given with a path is asked about every time.
                choices: if prepared.program.resolved().is_some() {
                    PermissionChoices::Standard
                } else {
                    PermissionChoices::Once
                },
            }
        }
        None => {
            let (class, target) = plain_target(&call.input);
            PermissionPrompt::new(class, target)
        }
    }
}

/// A call's row, pending: by its input when it validated.
fn row_of(call: &ToolCall, input: Option<&ToolInput>) -> ToolRow {
    let (kind, title) = match input {
        Some(ToolInput::ReadFile(input)) => (
            ToolKind::Read,
            ToolTitle::ReadFile {
                path: input.path.clone(),
                rows: match (input.start_line, input.end_line) {
                    (None, None) => None,
                    (start, end) => Some((start.unwrap_or(1), end.unwrap_or(u64::MAX))),
                },
            },
        ),
        Some(ToolInput::ListFiles(input)) => (
            ToolKind::List,
            ToolTitle::ListFiles {
                dir: input.dir.clone(),
            },
        ),
        Some(ToolInput::Search(input)) => (
            ToolKind::Search,
            ToolTitle::Search {
                pattern: input.pattern.clone(),
                files: None,
            },
        ),
        Some(ToolInput::ProposeEdit(input)) => (
            ToolKind::Edit,
            ToolTitle::ProposeEdit {
                path: input.path.clone(),
                changes: input.edits.len() as u64,
            },
        ),
        Some(ToolInput::RunCommand(input)) => {
            let mut words = vec![input.program.clone()];
            words.extend(input.args.iter().cloned());
            (
                ToolKind::Execute,
                ToolTitle::RunCommand {
                    command: words.join(" "),
                    outcome: None,
                },
            )
        }
        None => (ToolKind::Other, ToolTitle::Titled(call.name.clone())),
    };
    ToolRow::new(call.id.clone(), kind, title, ToolStatus::Pending)
}

/// `row` ended as `status`, with `output`.
fn row_ended(mut row: ToolRow, status: ToolStatus, output: &str) -> ToolRow {
    row.status = status;
    row.output = Some(output.to_owned());
    row
}

/// Fills in what a call's outcome tells its row: a search's files, a
/// proposal's changes and Review, a read's or listing's text folded.
fn note_outcome(row: &mut ToolRow, outcome: Result<&String, &String>) {
    let Ok(text) = outcome else {
        return;
    };
    match &mut row.title {
        ToolTitle::Search { files, .. } => {
            let mut seen: Vec<&str> = (text.lines())
                .filter_map(|line| line.split_once(':').map(|(path, _)| path))
                .filter(|path| !path.starts_with('['))
                .collect();
            seen.sort_unstable();
            seen.dedup();
            *files = Some(seen.len() as u64);
        }
        ToolTitle::ProposeEdit { changes, .. } => {
            // "Proposed N changes to …".
            if let Some(count) = (text.split_whitespace().nth(1)).and_then(|n| n.parse().ok()) {
                *changes = count;
            }
            row.review = true;
        }
        _ => {}
    }
    row.output = Some(text.clone());
}

/// Sets `row` among turn `index`'s rows.
fn set_row(view: &Entity<ChatView>, index: usize, row: ToolRow, cx: &mut App) {
    view.update(cx, |view, cx| view.set_tool_row(index, row, cx));
}

/// The `PATH` commands resolve on: the login shell's, unless a test set it.
fn command_path(cx: &App) -> Option<OsString> {
    #[cfg(test)]
    if let Some(CommandPath(path)) = cx.try_global::<CommandPath>() {
        return Some(path.clone());
    }
    let _ = cx;
    None
}

/// What a test holds a tool's outcome back with before it lands: it lands
/// once the test sends on the channel, or closes it.
fn held(tool: &str, cx: &App) -> Option<async_channel::Receiver<()>> {
    #[cfg(test)]
    if let Some(Held(holds)) = cx.try_global::<Held>() {
        return (holds.iter())
            .find(|(name, _)| *name == tool)
            .map(|(_, gate)| gate.clone());
    }
    let _ = (tool, cx);
    None
}

/// The `PATH` the loop's commands resolve on, in tests.
#[cfg(test)]
pub(crate) struct CommandPath(pub(crate) OsString);

#[cfg(test)]
impl gpui::Global for CommandPath {}

/// Tools whose outcomes wait for a send on their channel before they land,
/// in tests: a read held in flight.
#[cfg(test)]
pub(crate) struct Held(pub(crate) Vec<(&'static str, async_channel::Receiver<()>)>);

#[cfg(test)]
impl gpui::Global for Held {}
