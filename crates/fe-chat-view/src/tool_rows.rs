// SPDX-License-Identifier: Apache-2.0

//! Tool rows and permission rows in the transcript (the AI agents record,
//! `docs/decisions/50t8wt9we2-ai-agents.md`, § 5, § 6 "What the user sees"
//! and § 7, card AG9).
//!
//! The host says what a turn's tools did, and the view draws it between the
//! turn's thinking and its reply:
//!
//! - a [`ToolRow`] per call, set and updated in place by its id
//!   ([`ChatView::set_tool_row`]): its kind's icon, its title, its status,
//!   its locations as file links, its output folded, its diffs as compact
//!   read-only previews, and Review for a proposal;
//! - a plan as a checklist, updated in place ([`ChatView::set_plan`]);
//! - a [`PermissionPrompt`] under the row it is about
//!   ([`ChatView::ask_permission`]), with Allow Once, Allow for This Chat
//!   and Deny, answered as [`ToolEvent::PermissionAnswered`].
//!
//! The rows are plain data, so the API tool loop and Claude Agent's ACP
//! updates both fill them, and this crate depends on neither.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use fe_chat::permissions::Class;
use fe_editor::TextDiff;
use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, prelude::FluentBuilder as _, px, relative,
    svg,
};
use kiln_l10n::{Text, text, tr};
use kiln_ui::{Button, Icon, UiTheme, h_flex, v_flex};

use gpui::EventEmitter;

use crate::{
    ChatView, ChatViewEvent,
    reply::{self, DiffKind, DiffTints},
};

/// How many rows a diff preview shows before it says how many more.
pub const PREVIEW_ROWS: usize = 40;

/// What kind of tool a row is for, which picks its icon: the API chat's
/// tools and ACP's `ToolKind`s.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolKind {
    Read,
    Search,
    List,
    Edit,
    Execute,
    Fetch,
    Think,
    Other,
}

impl ToolKind {
    fn icon(self) -> Icon {
        match self {
            ToolKind::Read => Icon::FileText,
            ToolKind::Search => Icon::Search,
            ToolKind::List => Icon::ListTree,
            ToolKind::Edit => Icon::FileDiff,
            ToolKind::Execute => Icon::SquareTerminal,
            ToolKind::Fetch => Icon::Cloud,
            ToolKind::Think => Icon::MessageSquare,
            ToolKind::Other => Icon::Puzzle,
        }
    }
}

/// Where a call is: waiting (for permission, or its turn), running, or how
/// it ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolStatus {
    Pending,
    Running,
    Done,
    Failed,
    Denied,
}

impl ToolStatus {
    /// What the row says of it.
    pub fn label(self) -> String {
        match self {
            ToolStatus::Pending => tr!("chat-view-tool-pending"),
            ToolStatus::Running => tr!("chat-view-tool-running"),
            ToolStatus::Done => tr!("chat-view-tool-done"),
            ToolStatus::Failed => tr!("chat-view-tool-failed"),
            ToolStatus::Denied => tr!("chat-view-tool-denied"),
        }
        .to_string()
    }

    fn icon(self) -> Icon {
        match self {
            ToolStatus::Pending => Icon::CircleDot,
            ToolStatus::Running => Icon::LoaderCircle,
            ToolStatus::Done => Icon::CircleCheck,
            ToolStatus::Failed => Icon::CircleX,
            ToolStatus::Denied => Icon::X,
        }
    }

    /// Whether the call ran: its title is then in the past tense.
    fn ran(self) -> bool {
        matches!(self, ToolStatus::Done | ToolStatus::Failed)
    }
}

/// How a command ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CommandOutcome {
    Exited {
        code: i32,
        elapsed: Duration,
    },
    /// Killed at its timeout, or by Stop.
    Killed {
        elapsed: Duration,
    },
}

/// What a row's title says: the API chat's tools' own (§ 6 "What the user
/// sees"), or the title Claude Agent gave its call.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ToolTitle {
    /// `read_file`: "Read `src/lib.rs` rows 1–200".
    ReadFile {
        path: String,
        /// The first and last rows read, 1-based, when not the whole file.
        rows: Option<(u64, u64)>,
    },
    /// `search`: "Searched `foo` · 12 files".
    Search {
        pattern: String,
        /// How many files matched, once known.
        files: Option<u64>,
    },
    /// `list_files`: "Listed `src/`", or the project folder with no `dir`.
    ListFiles { dir: Option<String> },
    /// `propose_edit`: "Proposed 3 changes to `src/x.rs`".
    ProposeEdit { path: String, changes: u64 },
    /// `run_command`: "Ran `cargo test` · exit 0 · 3.2 s".
    RunCommand {
        command: String,
        outcome: Option<CommandOutcome>,
    },
    /// An ACP tool call's title, as the adapter gave it.
    Titled(String),
}

/// A file a call touched, root-relative, and the row, 1-based: a link that
/// opens it ([`ChatViewEvent::OpenPath`]).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ToolLocation {
    pub path: String,
    pub row: Option<u32>,
}

/// A change a call made or proposes, shown as a compact read-only diff
/// under its row: information only, since the review is elsewhere (§ 8).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DiffPreview {
    /// Root-relative.
    pub path: String,
    /// The text before, `None` for a new file.
    pub old_text: Option<String>,
    pub new_text: String,
}

/// One tool call in the transcript.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolRow {
    /// The call's id: the `tool_use` block's, or ACP's `toolCallId`. A row
    /// set again with the same id replaces it in place.
    pub id: String,
    pub kind: ToolKind,
    pub title: ToolTitle,
    pub status: ToolStatus,
    pub locations: Vec<ToolLocation>,
    /// What it printed, folded until opened: a command's, or the content
    /// an update carried.
    pub output: Option<String>,
    pub diffs: Vec<DiffPreview>,
    /// Whether it offers Review ([`ToolEvent::ReviewProposal`]): a
    /// proposal waiting in the AI Changes panel.
    pub review: bool,
}

impl ToolRow {
    /// A row with nothing under its title.
    pub fn new(
        id: impl Into<String>,
        kind: ToolKind,
        title: ToolTitle,
        status: ToolStatus,
    ) -> Self {
        ToolRow {
            id: id.into(),
            kind,
            title,
            status,
            locations: Vec::new(),
            output: None,
            diffs: Vec::new(),
            review: false,
        }
    }

    /// What its title says, in the past tense once the call ran.
    pub fn title_text(&self) -> String {
        let ran = self.status.ran();
        match &self.title {
            ToolTitle::ReadFile { path, rows } => match rows {
                Some((first, last)) => tr!(
                    "chat-view-tool-read-rows",
                    path = path.as_str(),
                    first = *first,
                    last = *last
                ),
                None => tr!("chat-view-tool-read", path = path.as_str()),
            },
            ToolTitle::Search { pattern, files } => match (ran, files) {
                (true, Some(files)) => tr!(
                    "chat-view-tool-searched-files",
                    pattern = pattern.as_str(),
                    files = *files
                ),
                (true, None) => tr!("chat-view-tool-searched", pattern = pattern.as_str()),
                (false, _) => tr!("chat-view-tool-search", pattern = pattern.as_str()),
            },
            ToolTitle::ListFiles { dir } => {
                let dir = match dir {
                    Some(dir) => tr!("chat-view-tool-dir", dir = dir.as_str()),
                    None => tr!("chat-view-tool-project"),
                };
                if ran {
                    tr!("chat-view-tool-listed", dir = dir)
                } else {
                    tr!("chat-view-tool-list", dir = dir)
                }
            }
            ToolTitle::ProposeEdit { path, changes } => {
                if ran {
                    tr!(
                        "chat-view-tool-proposed",
                        path = path.as_str(),
                        changes = *changes
                    )
                } else {
                    tr!("chat-view-tool-propose", path = path.as_str())
                }
            }
            ToolTitle::RunCommand { command, outcome } => match (ran, outcome) {
                (true, Some(CommandOutcome::Exited { code, elapsed })) => tr!(
                    "chat-view-tool-ran-exit",
                    command = command.as_str(),
                    code = *code,
                    seconds = seconds(*elapsed).as_str()
                ),
                (true, Some(CommandOutcome::Killed { elapsed })) => tr!(
                    "chat-view-tool-ran-killed",
                    command = command.as_str(),
                    seconds = seconds(*elapsed).as_str()
                ),
                (true, None) => tr!("chat-view-tool-ran", command = command.as_str()),
                (false, _) => tr!("chat-view-tool-run", command = command.as_str()),
            },
            ToolTitle::Titled(title) => return title.clone(),
        }
        .to_string()
    }
}

/// A duration as a row shows it: seconds to a tenth.
fn seconds(elapsed: Duration) -> String {
    let tenths = (elapsed.as_millis() + 50) / 100;
    format!("{}.{}", tenths / 10, tenths % 10)
}

/// Where a plan's step is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlanStatus {
    Pending,
    InProgress,
    Completed,
}

impl PlanStatus {
    /// What the checklist says of it, to a screen reader and in tests.
    pub fn label(self) -> String {
        match self {
            PlanStatus::Pending => tr!("chat-view-plan-pending"),
            PlanStatus::InProgress => tr!("chat-view-plan-in-progress"),
            PlanStatus::Completed => tr!("chat-view-plan-completed"),
        }
        .to_string()
    }

    fn icon(self) -> Icon {
        match self {
            PlanStatus::Pending => Icon::Square,
            PlanStatus::InProgress => Icon::LoaderCircle,
            PlanStatus::Completed => Icon::CircleCheck,
        }
    }
}

/// A step of an agent's plan.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PlanEntry {
    pub text: String,
    pub status: PlanStatus,
}

/// What a permission prompt offers.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PermissionChoices {
    /// Allow Once, Allow for This Chat and Deny.
    Standard,
    /// Allow Once and Deny: for a request whose answer is never remembered.
    Once,
    /// The options an ACP request offered with no `allow_once` among them,
    /// as offered; the user's pick is sent as is.
    Offered(Vec<OfferedOption>),
}

/// One of an ACP request's own options.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OfferedOption {
    /// The option's id, sent back as picked.
    pub id: String,
    pub label: String,
}

/// A request waiting for the user's answer (§ 7 "One model for both
/// agents"): a row in the transcript under the tool row it is about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionPrompt {
    pub class: Class,
    /// What it targets: a path, a pattern, a command, a URL or a tool's
    /// title. For a command, the resolved program's path.
    pub target: String,
    /// A command's working directory.
    pub cwd: Option<String>,
    /// Whether to say "This runs in your project folder and can change
    /// files": the API chat's `run_command`.
    pub in_project: bool,
    pub choices: PermissionChoices,
}

impl PermissionPrompt {
    /// A prompt for `class` on `target` with the standard choices.
    pub fn new(class: Class, target: impl Into<String>) -> Self {
        PermissionPrompt {
            class,
            target: target.into(),
            cwd: None,
            in_project: false,
            choices: PermissionChoices::Standard,
        }
    }

    /// What it asks.
    pub fn question(&self) -> String {
        let target = self.target.as_str();
        match self.class {
            Class::Read => tr!("chat-view-permission-read", target = target),
            Class::Search => tr!("chat-view-permission-search", target = target),
            Class::ProposeEdit => tr!("chat-view-permission-propose-edit", target = target),
            Class::RunCommand => tr!("chat-view-permission-run-command", target = target),
            Class::Fetch => tr!("chat-view-permission-fetch", target = target),
            Class::Other => tr!("chat-view-permission-other", target = target),
        }
        .to_string()
    }

    /// Whether `answer` is one it offers. [`PermissionAnswer::Deny`] and
    /// [`PermissionAnswer::Stopped`] always are: for offered options the
    /// host sends them as a refusal or as cancelled.
    pub fn offers(&self, answer: &PermissionAnswer) -> bool {
        match (&self.choices, answer) {
            (_, PermissionAnswer::Deny | PermissionAnswer::Stopped) => true,
            (PermissionChoices::Standard, PermissionAnswer::AllowOnce)
            | (PermissionChoices::Standard, PermissionAnswer::AllowForChat)
            | (PermissionChoices::Once, PermissionAnswer::AllowOnce) => true,
            (PermissionChoices::Offered(options), PermissionAnswer::Option(id)) => {
                options.iter().any(|option| option.id == *id)
            }
            _ => false,
        }
    }
}

/// How a permission prompt was answered.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PermissionAnswer {
    /// Allow Once, or Enter in the empty composer.
    AllowOnce,
    /// Allow for This Chat: the host remembers it for the chat.
    AllowForChat,
    /// Deny, or Escape.
    Deny,
    /// One of an ACP request's own options, by id.
    Option(String),
    /// Stop closed it, as refused.
    Stopped,
}

/// What the user did with the tool rows and permission prompts: a
/// [`ChatView`] emits these beside its [`ChatViewEvent`]s, so a host
/// subscribes to them on their own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolEvent {
    /// The user answered the permission prompt about call `tool` of turn
    /// `turn` ([`ChatView::ask_permission`]): by its buttons, Enter, Escape
    /// or Stop.
    PermissionAnswered {
        turn: usize,
        tool: String,
        answer: PermissionAnswer,
    },
    /// The user pressed Review on call `tool` of turn `turn`: its row
    /// offers it ([`ToolRow::review`]).
    ReviewProposal { turn: usize, tool: String },
}

impl EventEmitter<ToolEvent> for ChatView {}

/// A prompt waiting, and the call it is about.
#[derive(Clone, Debug)]
struct Waiting {
    turn: usize,
    tool: String,
    prompt: PermissionPrompt,
}

/// The view's tool rows, plans and prompts, by turn.
#[derive(Default)]
pub(crate) struct Tools {
    rows: HashMap<usize, Vec<ToolRow>>,
    plans: HashMap<usize, Vec<PlanEntry>>,
    /// In the order asked: Enter and Escape answer the first.
    waiting: Vec<Waiting>,
    /// The rows whose output is open, by turn and id.
    open: HashSet<(usize, String)>,
    /// Each row's diffs' preview rows, by turn and id, made when the row
    /// is set rather than each frame: `None` for one too large to diff.
    previews: HashMap<(usize, String), Vec<Option<Preview>>>,
}

/// A diff preview's rows, at most [`PREVIEW_ROWS`] not counting gaps, and
/// how many more there are.
type Preview = (Vec<(DiffKind, String)>, usize);

impl ChatView {
    /// Sets a tool row of turn `turn`: in place of the one with its id, or
    /// after the turn's others.
    pub fn set_tool_row(&mut self, turn: usize, row: ToolRow, cx: &mut Context<Self>) {
        let rows = self.tools.rows.entry(turn).or_default();
        let at = match rows.iter().position(|old| old.id == row.id) {
            Some(at) if rows[at] == row => return,
            Some(at) => at,
            None => {
                rows.push(row.clone());
                rows.len() - 1
            }
        };
        let key = (turn, row.id.clone());
        let unchanged = rows[at].diffs == row.diffs && self.tools.previews.contains_key(&key);
        if !unchanged {
            let previews = row.diffs.iter().map(preview_rows).collect();
            self.tools.previews.insert(key, previews);
        }
        rows[at] = row;
        cx.notify();
    }

    /// Turn `turn`'s tool rows, in the order first set.
    pub fn tool_rows(&self, turn: usize) -> &[ToolRow] {
        self.tools.rows.get(&turn).map_or(&[], Vec::as_slice)
    }

    /// Sets turn `turn`'s plan, in place of the one before; none clears it.
    pub fn set_plan(&mut self, turn: usize, entries: Vec<PlanEntry>, cx: &mut Context<Self>) {
        if entries.is_empty() {
            self.tools.plans.remove(&turn);
        } else {
            self.tools.plans.insert(turn, entries);
        }
        cx.notify();
    }

    /// Turn `turn`'s plan.
    pub fn plan(&self, turn: usize) -> &[PlanEntry] {
        self.tools.plans.get(&turn).map_or(&[], Vec::as_slice)
    }

    /// Whether tool row `tool` of turn `turn` shows its output.
    pub fn tool_output_open(&self, turn: usize, tool: &str) -> bool {
        self.tools.open.contains(&(turn, tool.to_owned()))
    }

    /// Opens or folds tool row `tool`'s output, as clicking its toggle does.
    pub fn set_tool_output_open(
        &mut self,
        turn: usize,
        tool: &str,
        open: bool,
        cx: &mut Context<Self>,
    ) {
        let key = (turn, tool.to_owned());
        if open {
            self.tools.open.insert(key);
        } else {
            self.tools.open.remove(&key);
        }
        cx.notify();
    }

    /// Asks the user about call `tool` of turn `turn`: a prompt under its
    /// row, in place of one already asked about it, until it's answered
    /// ([`ToolEvent::PermissionAnswered`]) or withdrawn. A prompt with
    /// no row yet is drawn after the turn's rows, so it's never hidden.
    pub fn ask_permission(
        &mut self,
        turn: usize,
        tool: impl Into<String>,
        prompt: PermissionPrompt,
        cx: &mut Context<Self>,
    ) {
        let tool = tool.into();
        match (self.tools.waiting.iter_mut()).find(|w| w.turn == turn && w.tool == tool) {
            Some(waiting) => waiting.prompt = prompt,
            None => self.tools.waiting.push(Waiting { turn, tool, prompt }),
        }
        cx.notify();
    }

    /// The prompt waiting about call `tool` of turn `turn`.
    pub fn permission(&self, turn: usize, tool: &str) -> Option<&PermissionPrompt> {
        (self.tools.waiting.iter())
            .find(|w| w.turn == turn && w.tool == tool)
            .map(|w| &w.prompt)
    }

    /// The prompts waiting, as `(turn, tool)`, in the order asked.
    pub fn waiting_permissions(&self) -> Vec<(usize, String)> {
        (self.tools.waiting.iter())
            .map(|w| (w.turn, w.tool.clone()))
            .collect()
    }

    /// Takes the prompt about call `tool` of turn `turn` away unanswered,
    /// as when the host decided it otherwise. Emits nothing.
    pub fn withdraw_permission(&mut self, turn: usize, tool: &str, cx: &mut Context<Self>) {
        let before = self.tools.waiting.len();
        (self.tools.waiting).retain(|w| !(w.turn == turn && w.tool == tool));
        if self.tools.waiting.len() != before {
            cx.notify();
        }
    }

    /// Answers the prompt about call `tool` of turn `turn` with `answer`,
    /// as its buttons do: it goes, and the view emits
    /// [`ToolEvent::PermissionAnswered`]. Does nothing, and gives
    /// `false`, if no such prompt waits or it doesn't offer `answer`.
    pub fn answer_permission(
        &mut self,
        turn: usize,
        tool: &str,
        answer: PermissionAnswer,
        cx: &mut Context<Self>,
    ) -> bool {
        let at = (self.tools.waiting.iter())
            .position(|w| w.turn == turn && w.tool == tool && w.prompt.offers(&answer));
        let Some(at) = at else {
            return false;
        };
        let waiting = self.tools.waiting.remove(at);
        cx.emit(ToolEvent::PermissionAnswered {
            turn: waiting.turn,
            tool: waiting.tool,
            answer,
        });
        cx.notify();
        true
    }

    /// Answers the first prompt waiting with `answer`, if it offers it:
    /// Enter and Escape.
    pub(crate) fn answer_first_permission(
        &mut self,
        answer: PermissionAnswer,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(first) = self.tools.waiting.first() else {
            return false;
        };
        let (turn, tool) = (first.turn, first.tool.clone());
        self.answer_permission(turn, &tool, answer, cx)
    }

    /// Closes every prompt waiting as refused, as Stop does: each is
    /// answered [`PermissionAnswer::Stopped`].
    pub(crate) fn close_permissions(&mut self, cx: &mut Context<Self>) {
        for waiting in std::mem::take(&mut self.tools.waiting) {
            cx.emit(ToolEvent::PermissionAnswered {
                turn: waiting.turn,
                tool: waiting.tool,
                answer: PermissionAnswer::Stopped,
            });
        }
        cx.notify();
    }

    /// Turn `turn`'s plan, tool rows and prompts, if it has any. Its
    /// `debug_selector` is `chat-tools-<turn>`; see the rows' own below.
    pub(crate) fn render_tools(
        &self,
        turn: usize,
        ui: &UiTheme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let rows = self.tool_rows(turn);
        let plan = self.plan(turn);
        let orphans: Vec<&Waiting> = (self.tools.waiting.iter())
            .filter(|w| w.turn == turn && !rows.iter().any(|row| row.id == w.tool))
            .collect();
        if rows.is_empty() && plan.is_empty() && orphans.is_empty() {
            return None;
        }
        let theme = self.composer().read(cx).theme();
        let style = reply::reply_style(theme, ui);
        let tints = DiffTints::new(theme);
        let plan = (!plan.is_empty()).then(|| render_plan(turn, plan, ui));
        let mut children: Vec<AnyElement> = Vec::new();
        for (k, row) in rows.iter().enumerate() {
            children.push(self.render_row(turn, k, row, &style, tints, ui, cx));
            if let Some(waiting) =
                (self.tools.waiting.iter()).find(|w| w.turn == turn && w.tool == row.id)
            {
                children.push(render_prompt(turn, k, waiting, ui, cx));
            }
        }
        for (n, waiting) in orphans.into_iter().enumerate() {
            children.push(render_prompt(turn, rows.len() + n, waiting, ui, cx));
        }
        Some(
            // Its own id scope: the rows' ids count from 0 in each turn.
            v_flex()
                .id(("chat-tools", turn))
                .debug_selector(move || format!("chat-tools-{turn}"))
                .gap_1()
                .children(plan)
                .children(children)
                .into_any_element(),
        )
    }

    /// Tool row `k` of turn `turn`: `chat-tool-<turn>-<k>`, its status
    /// `chat-tool-status-<turn>-<k>`, its links
    /// `chat-tool-location-<turn>-<k>-<n>`, its output's toggle
    /// `chat-tool-output-toggle-<turn>-<k>` and output
    /// `chat-tool-output-<turn>-<k>`, its diffs
    /// `chat-tool-diff-<turn>-<k>-<d>`, and Review
    /// `chat-tool-review-<turn>-<k>`.
    #[allow(clippy::too_many_arguments)]
    fn render_row(
        &self,
        turn: usize,
        k: usize,
        row: &ToolRow,
        style: &fe_markdown_view::MarkdownStyle,
        tints: DiffTints,
        ui: &UiTheme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let status_color = match row.status {
            ToolStatus::Failed | ToolStatus::Denied => ui.error_text,
            _ => ui.muted_text,
        };
        let review = row.review.then(|| {
            let id = row.id.clone();
            let ask = cx.listener(move |_, _, _, cx| {
                cx.emit(ToolEvent::ReviewProposal {
                    turn,
                    tool: id.clone(),
                })
            });
            Button::new("chat-tool-review", text!("chat-view-tool-review"))
                .small()
                .debug_selector(move || format!("chat-tool-review-{turn}-{k}"))
                .on_click(ask)
        });
        let header = h_flex()
            .gap_2()
            .items_center()
            .child(icon(row.kind.icon(), ui.icon))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(ui.text)
                    .child(row.title_text()),
            )
            .child(
                h_flex()
                    .debug_selector(move || format!("chat-tool-status-{turn}-{k}"))
                    .flex_none()
                    .gap_1()
                    .items_center()
                    .text_color(status_color)
                    .child(icon(row.status.icon(), status_color))
                    .child(row.status.label()),
            )
            .children(review);
        let locations = (!row.locations.is_empty()).then(|| {
            h_flex()
                .flex_wrap()
                .gap_2()
                .pl_6()
                .children(row.locations.iter().enumerate().map(|(n, location)| {
                    let (path, line) = (location.path.clone(), location.row);
                    let open = cx.listener(move |_, _, _, cx| {
                        cx.emit(ChatViewEvent::OpenPath {
                            path: path.clone(),
                            row: line,
                        })
                    });
                    let label = match location.row {
                        Some(line) => tr!(
                            "chat-view-tool-location-row",
                            path = location.path.as_str(),
                            row = line
                        )
                        .to_string(),
                        None => location.path.clone(),
                    };
                    div()
                        .id(("chat-tool-location", n))
                        .debug_selector(move || format!("chat-tool-location-{turn}-{k}-{n}"))
                        .cursor_pointer()
                        .text_color(ui.accent)
                        .child(label)
                        .on_click(open)
                }))
        });
        let output = (row.output.as_ref())
            .filter(|output| !output.is_empty())
            .map(|output| {
                let open = self.tool_output_open(turn, &row.id);
                let id = row.id.clone();
                let toggle = cx.listener(move |this, _, _, cx| {
                    let open = this.tool_output_open(turn, &id);
                    this.set_tool_output_open(turn, &id, !open, cx);
                });
                v_flex()
                    .pl_6()
                    .gap_1()
                    .child(
                        div()
                            .id("chat-tool-output-toggle")
                            .debug_selector(move || format!("chat-tool-output-toggle-{turn}-{k}"))
                            .cursor_pointer()
                            .text_color(ui.muted_text)
                            .child(if open {
                                tr!("chat-view-tool-output-open")
                            } else {
                                tr!("chat-view-tool-output-closed")
                            })
                            .on_click(toggle),
                    )
                    .when(open, |this| {
                        this.child(
                            div()
                                .id("chat-tool-output")
                                .debug_selector(move || format!("chat-tool-output-{turn}-{k}"))
                                .overflow_x_scroll()
                                .p_2()
                                .rounded_md()
                                .bg(style.code_background)
                                .text_color(ui.text)
                                .font_family(style.code_font_family.clone())
                                .text_size(style.code_font_size)
                                .line_height(relative(style.code_line_height))
                                .child(output.clone()),
                        )
                    })
            });
        let previews = self.tools.previews.get(&(turn, row.id.clone()));
        let diffs = (row.diffs.iter().enumerate())
            .map(|(d, diff)| {
                let preview = previews.and_then(|previews| previews.get(d)).cloned();
                render_diff(turn, k, d, diff, preview.flatten(), style, tints, ui)
            })
            .collect::<Vec<_>>();
        // Its own id scope, for its links, toggle, diffs and Review.
        v_flex()
            .id(("chat-tool", k))
            .debug_selector(move || format!("chat-tool-{turn}-{k}"))
            .gap_1()
            .px_2()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(ui.border)
            .child(header)
            .children(locations)
            .children(output)
            .children(diffs)
            .into_any_element()
    }
}

fn icon(icon: Icon, color: gpui::Hsla) -> impl IntoElement {
    svg()
        .path(icon.path())
        .flex_none()
        .size(px(14.))
        .text_color(color)
}

/// Turn `turn`'s plan as a checklist: `chat-plan-<turn>`, its steps
/// `chat-plan-<turn>-<n>`.
fn render_plan(turn: usize, plan: &[PlanEntry], ui: &UiTheme) -> AnyElement {
    let steps = plan.iter().enumerate().map(|(n, entry)| {
        let color = match entry.status {
            PlanStatus::Completed => ui.muted_text,
            PlanStatus::Pending | PlanStatus::InProgress => ui.text,
        };
        h_flex()
            .debug_selector(move || format!("chat-plan-{turn}-{n}"))
            .gap_2()
            .items_center()
            .text_color(color)
            .child(icon(entry.status.icon(), color))
            .child(div().flex_1().min_w_0().child(entry.text.clone()))
    });
    v_flex()
        .debug_selector(move || format!("chat-plan-{turn}"))
        .gap_1()
        .px_2()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(ui.border)
        .child(div().text_color(ui.muted_text).child(tr!("chat-view-plan")))
        .children(steps)
        .into_any_element()
}

/// The prompt `waiting`, under tool row `k` of turn `turn`:
/// `chat-permission-<turn>-<k>`, its buttons
/// `chat-permission-allow-once-<turn>-<k>`,
/// `chat-permission-allow-chat-<turn>-<k>`, `chat-permission-deny-<turn>-<k>`
/// and, for offered options, `chat-permission-option-<turn>-<k>-<n>`.
fn render_prompt(
    turn: usize,
    k: usize,
    waiting: &Waiting,
    ui: &UiTheme,
    cx: &mut Context<ChatView>,
) -> AnyElement {
    let prompt = &waiting.prompt;
    let answer = |answer: PermissionAnswer| {
        let tool = waiting.tool.clone();
        cx.listener(move |this, _, _, cx| {
            this.answer_permission(turn, &tool, answer.clone(), cx);
        })
    };
    let mut buttons: Vec<AnyElement> = Vec::new();
    match &prompt.choices {
        PermissionChoices::Standard | PermissionChoices::Once => {
            buttons.push(
                Button::new(
                    "chat-permission-allow-once",
                    text!("chat-view-permission-allow-once"),
                )
                .small()
                .primary()
                .debug_selector(move || format!("chat-permission-allow-once-{turn}-{k}"))
                .on_click(answer(PermissionAnswer::AllowOnce))
                .into_any_element(),
            );
            if prompt.choices == PermissionChoices::Standard {
                buttons.push(
                    Button::new(
                        "chat-permission-allow-chat",
                        text!("chat-view-permission-allow-chat"),
                    )
                    .small()
                    .debug_selector(move || format!("chat-permission-allow-chat-{turn}-{k}"))
                    .on_click(answer(PermissionAnswer::AllowForChat))
                    .into_any_element(),
                );
            }
        }
        PermissionChoices::Offered(options) => {
            for (n, option) in options.iter().enumerate() {
                buttons.push(
                    Button::new(
                        ("chat-permission-option", n),
                        Text::verbatim(option.label.clone()),
                    )
                    .small()
                    .debug_selector(move || format!("chat-permission-option-{turn}-{k}-{n}"))
                    .on_click(answer(PermissionAnswer::Option(option.id.clone())))
                    .into_any_element(),
                );
            }
        }
    }
    if !matches!(prompt.choices, PermissionChoices::Offered(_)) {
        buttons.push(
            Button::new("chat-permission-deny", text!("chat-view-permission-deny"))
                .small()
                .debug_selector(move || format!("chat-permission-deny-{turn}-{k}"))
                .on_click(answer(PermissionAnswer::Deny))
                .into_any_element(),
        );
    }
    let cwd = (prompt.cwd.as_ref()).map(|cwd| {
        div()
            .text_color(ui.muted_text)
            .child(tr!("chat-view-permission-cwd", cwd = cwd.as_str()))
    });
    let in_project = prompt.in_project.then(|| {
        div()
            .text_color(ui.muted_text)
            .child(tr!("chat-view-permission-in-project"))
    });
    // Its own id scope, for its buttons.
    v_flex()
        .id(("chat-permission", k))
        .debug_selector(move || format!("chat-permission-{turn}-{k}"))
        .ml_6()
        .gap_1()
        .p_2()
        .rounded_md()
        .border_1()
        .border_color(ui.accent)
        .child(div().text_color(ui.text).child(prompt.question()))
        .children(cwd)
        .children(in_project)
        .child(h_flex().flex_wrap().gap_2().children(buttons))
        .into_any_element()
}

/// Diff `d` of tool row `k` of turn `turn`, compact: the path, then each
/// hunk's removed rows and added rows with no context around them, a gap
/// between hunks, at most [`PREVIEW_ROWS`] rows and a line saying how many
/// more. Its `debug_selector` is `chat-tool-diff-<turn>-<k>-<d>`, its rows'
/// `chat-tool-diff-<turn>-<k>-<d>-removed-<r>` and `-added-<r>`, `r`
/// counted for each kind, and the line `chat-tool-diff-more-<turn>-<k>-<d>`.
#[allow(clippy::too_many_arguments)]
fn render_diff(
    turn: usize,
    k: usize,
    d: usize,
    preview: &DiffPreview,
    rows: Option<Preview>,
    style: &fe_markdown_view::MarkdownStyle,
    tints: DiffTints,
    ui: &UiTheme,
) -> AnyElement {
    let (rows, more) = match rows {
        Some((rows, more)) => (rows, more),
        None => {
            return v_flex()
                .debug_selector(move || format!("chat-tool-diff-{turn}-{k}-{d}"))
                .pl_6()
                .text_color(ui.muted_text)
                .child(preview.path.clone())
                .child(tr!("chat-view-tool-diff-too-large"))
                .into_any_element();
        }
    };
    let (mut removed, mut added) = (0, 0);
    let rows = rows.into_iter().map(|(kind, text)| {
        let (sign, tint, name, r) = match kind {
            DiffKind::Removed => {
                removed += 1;
                ("-", tints.removed, "removed", removed - 1)
            }
            DiffKind::Added => {
                added += 1;
                ("+", tints.added, "added", added - 1)
            }
            DiffKind::Gap => {
                let height = style.code_font_size * style.code_line_height;
                return div().h(height).into_any_element();
            }
        };
        div()
            .debug_selector(move || format!("chat-tool-diff-{turn}-{k}-{d}-{name}-{r}"))
            .flex()
            .flex_row()
            .pl(px(8.))
            .pr(px(16.))
            .bg(tint)
            .child(
                div()
                    .flex_none()
                    .w(style.code_font_size)
                    .text_color(style.muted)
                    .child(sign),
            )
            .child(text)
            .into_any_element()
    });
    let more = (more > 0).then(|| {
        div()
            .debug_selector(move || format!("chat-tool-diff-more-{turn}-{k}-{d}"))
            .pl(px(8.))
            .text_color(ui.muted_text)
            .child(tr!("chat-view-tool-diff-more", rows = more))
    });
    v_flex()
        .debug_selector(move || format!("chat-tool-diff-{turn}-{k}-{d}"))
        .ml_6()
        .rounded_md()
        .overflow_hidden()
        .bg(style.code_background)
        .child(
            div()
                .px_2()
                .text_color(ui.muted_text)
                .child(preview.path.clone()),
        )
        .child(
            div()
                .id(("chat-tool-diff", d))
                .overflow_x_scroll()
                .font_family(style.code_font_family.clone())
                .text_size(style.code_font_size)
                .line_height(relative(style.code_line_height))
                .text_color(ui.text)
                .child(
                    div()
                        .flex_none()
                        .min_w_full()
                        .flex()
                        .flex_col()
                        .children(rows.collect::<Vec<_>>()),
                ),
        )
        .children(more)
        .into_any_element()
}

/// A preview's rows, at most [`PREVIEW_ROWS`] of them not counting gaps,
/// and how many more there are; `None` when a side is too large to diff.
fn preview_rows(preview: &DiffPreview) -> Option<Preview> {
    let old = preview.old_text.as_deref().unwrap_or_default();
    let new = preview.new_text.as_str();
    let mut all: Vec<(DiffKind, &str)> = Vec::new();
    if old.is_empty() || new.is_empty() {
        // All one side: a new file, or one emptied.
        let (kind, text) = if old.is_empty() {
            (DiffKind::Added, new)
        } else {
            (DiffKind::Removed, old)
        };
        all.extend(text_rows(text).into_iter().map(|row| (kind, row)));
    } else {
        let diff = TextDiff::new(old, new);
        if diff.too_large() {
            return None;
        }
        let (left, right) = (all_rows(old), all_rows(new));
        for (h, hunk) in diff.hunks().iter().enumerate() {
            if h > 0 {
                all.push((DiffKind::Gap, ""));
            }
            let removed = (hunk.left.start as usize..hunk.left.end as usize)
                .filter_map(|row| left.get(row))
                .map(|row| (DiffKind::Removed, *row));
            let added = (hunk.right.start as usize..hunk.right.end as usize)
                .filter_map(|row| right.get(row))
                .map(|row| (DiffKind::Added, *row));
            all.extend(removed.chain(added));
        }
    }
    let mut rows = Vec::new();
    let mut shown = 0;
    let mut more = 0;
    for (kind, text) in all {
        if shown >= PREVIEW_ROWS {
            more += usize::from(kind != DiffKind::Gap);
            continue;
        }
        shown += usize::from(kind != DiffKind::Gap);
        rows.push((kind, text.to_owned()));
    }
    // A gap last leads nowhere.
    if rows.last().is_some_and(|(kind, _)| *kind == DiffKind::Gap) {
        rows.pop();
    }
    Some((rows, more))
}

/// `text`'s rows as [`TextDiff`] splits them: `\r\n`, `\r` and `\n` each
/// end one, so a text ending in a line break has an empty last row.
fn all_rows(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let (mut rows, mut start, mut at) = (Vec::new(), 0, 0);
    while at < bytes.len() {
        match bytes[at] {
            b'\n' => {
                rows.push(&text[start..at]);
                at += 1;
                start = at;
            }
            b'\r' => {
                rows.push(&text[start..at]);
                at += if bytes.get(at + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
                start = at;
            }
            _ => at += 1,
        }
    }
    rows.push(&text[start..]);
    rows
}

/// `text`'s rows, without the empty one after a last line break.
fn text_rows(text: &str) -> Vec<&str> {
    let mut rows = all_rows(text);
    if rows.len() > 1 && rows.last() == Some(&"") {
        rows.pop();
    }
    rows
}
