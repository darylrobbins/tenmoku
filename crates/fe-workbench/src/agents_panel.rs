//! The Agents panel (docs/designs/agent-lanes.md § Rail: robot icon with a
//! silver-blue count): a sidebar panel listing the window's lanes, one
//! [`LaneSummary`] row each, in order of number. A row shows the lane's
//! number (`lane 2`, in the agent's silver-blue, `agent.base`), its name and
//! state, then its files, pending hunks and conflicts, and its buttons:
//!
//! - **Review**, while lane 1 has hunks pending: shows the AI Changes
//!   panel, its review. Another lane's review is `agent.review_lane`'s, a
//!   later card's (card 19), so its row has none yet.
//! - **Stop**, while lane 1 is Working: stops the chat's reply, which ends
//!   its turn. Stopping another lane's agent is the agent seam's (card 20).
//! - **Clear**, on a lane that has ended (Accepted, Rejected or Abandoned)
//!   other than lane 1, which never closes (§ Lifecycle › Closing). An
//!   Abandoned lane with hunks pending asks first, then rejects them
//!   ([`Workbench::reject_lane_work`]). Clearing removes the lane from the
//!   list but not its number from the record; removing its worktree is the
//!   worktree card's (card 18).
//!
//! An Accepted, Rejected or Abandoned lane stays listed, dimmed, until it is
//! cleared. The selected row is the user's, in the list's selection
//! colours (the accent while the panel has focus), never silver-blue
//! (§ Colour rule). Up and Down, and `j` and `k` with vim on, move it.
//!
//! The rail's Agents item, in the top group after Changes, is badged with
//! [`AgentsPanel::rail_count`]: the lanes Working or with hunks pending
//! ([`LaneCounts::pending`] holds both, so it is the design's
//! `LaneCounts::working + LaneCounts::pending` without counting a Working
//! lane twice), in [`BadgeTone::Agent`]; none at zero. The dock sets it as
//! the panel redraws ([`crate::dock::Dock`]). `agent.show_panel` shows the
//! panel; its text is the catalogue's (`l10n/en-US/dock.ftl`).
//!
//! [`LaneCounts::pending`]: fe_editor::lane::LaneCounts::pending

use fe_editor::lane::{LaneId, LaneState};
use fe_ui::{ActiveTheme as _, BadgeTone, Button, Icon, UI_SMALL};
use gpui::{
    App, Context, Div, Entity, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    KeyContext, KeyDownEvent, ParentElement as _, PromptButton, PromptLevel, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, WeakEntity, Window, div,
    prelude::FluentBuilder as _, px, svg,
};
use kiln::Cx;
use kiln_l10n::{text, tr};
use kiln_shell::dock::RailBadge;

use crate::{
    Workbench,
    chat_panel::ChatPanel,
    dock::{DockOp, SidebarPanel},
    lanes::{LaneStore, LaneSummary, chat_lane},
};

/// The command that shows the panel.
pub(crate) const SHOW_COMMAND: &str = "agent.show_panel";

/// The panel in the sidebar: always on the rail, after Changes; focused
/// when shown, for its keys.
pub(crate) const SIDEBAR: SidebarPanel = SidebarPanel {
    panel: crate::DockPanel::new("agents"),
    name: "Agents",
    icon: Icon::Bot,
    show_command: SHOW_COMMAND,
    show_op: DockOp::ShowAgents,
    order: 45,
    always_on_rail: true,
    focus_on_show: true,
};

/// The key context the panel's element sets.
pub(crate) const KEY_CONTEXT: &str = "agents_panel";

/// A row's element ID and selector, before its lane's number.
const ROW: &str = "agents-row";

/// How much a lane that has ended is dimmed.
const DIMMED: f32 = 0.55;

/// A row's buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LaneAction {
    Review,
    Stop,
    Clear,
}

impl LaneAction {
    fn id(self) -> &'static str {
        match self {
            LaneAction::Review => "agents-review",
            LaneAction::Stop => "agents-stop",
            LaneAction::Clear => "agents-clear",
        }
    }
}

/// A lane's row as the panel shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LaneRow {
    pub(crate) summary: LaneSummary,
    /// Whether it has ended: Accepted, Rejected or Abandoned.
    pub(crate) dimmed: bool,
    pub(crate) actions: Vec<LaneAction>,
}

impl LaneRow {
    fn new(summary: LaneSummary) -> Self {
        let lead = summary.id == chat_lane();
        let dimmed = ended(summary.state);
        let mut actions = Vec::new();
        if lead && summary.pending > 0 {
            actions.push(LaneAction::Review);
        }
        if lead && summary.state == LaneState::Working {
            actions.push(LaneAction::Stop);
        }
        if !lead && dimmed {
            actions.push(LaneAction::Clear);
        }
        Self {
            summary,
            dimmed,
            actions,
        }
    }

    /// Its first line's lane number, `lane 2`.
    pub(crate) fn number(&self) -> SharedString {
        tr!("workbench-dock-agents-lane", number = self.summary.id.get())
    }

    /// Its state's name.
    pub(crate) fn state(&self) -> SharedString {
        match self.summary.state {
            LaneState::Working => tr!("workbench-dock-agents-working"),
            LaneState::PendingReview => tr!("workbench-dock-agents-pending-review"),
            LaneState::Accepted => tr!("workbench-dock-agents-accepted"),
            LaneState::Rejected => tr!("workbench-dock-agents-rejected"),
            LaneState::Abandoned => tr!("workbench-dock-agents-abandoned"),
        }
    }

    /// Its second line: its files, pending hunks and conflicts.
    pub(crate) fn counts(&self) -> SharedString {
        tr!(
            "workbench-dock-agents-counts",
            files = self.summary.files.len(),
            pending = self.summary.pending,
            conflicts = self.summary.conflicts,
        )
    }

    /// Its name to a screen reader: what its lines say.
    fn accessible_name(&self) -> SharedString {
        tr!(
            "workbench-dock-agents-row",
            lane = self.number().to_string(),
            name = &*self.summary.name,
            state = self.state().to_string(),
            counts = self.counts().to_string(),
        )
    }
}

/// Whether a lane in `state` has ended: what the panel dims and may clear.
fn ended(state: LaneState) -> bool {
    matches!(
        state,
        LaneState::Accepted | LaneState::Rejected | LaneState::Abandoned
    )
}

/// What the tests read of the last frame.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct TestHooks {
    /// The rows drawn, top to bottom.
    pub(crate) rows: Vec<LaneRow>,
    /// The colour each row's lane number was drawn in.
    pub(crate) number_colors: Vec<gpui::Hsla>,
    /// The selected row, as drawn.
    pub(crate) selected: Option<usize>,
}

/// The window's lanes, listed.
pub(crate) struct AgentsPanel {
    lanes: Entity<LaneStore>,
    workbench: WeakEntity<Workbench>,
    focus: FocusHandle,
    /// The selected row's lane.
    selected: Option<LaneId>,
    /// Redraws it as the lanes change.
    _lanes: Subscription,
    #[cfg(test)]
    pub(crate) test: TestHooks,
}

impl AgentsPanel {
    /// A panel listing `lanes`, the window's, whose buttons act on
    /// `workbench`.
    pub(crate) fn new(
        lanes: &Entity<LaneStore>,
        workbench: WeakEntity<Workbench>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            lanes: lanes.clone(),
            workbench,
            focus: cx.focus_handle(),
            selected: None,
            _lanes: cx.observe(lanes, |_, _, cx| cx.notify()),
            #[cfg(test)]
            test: TestHooks::default(),
        }
    }

    /// The rows it lists: every lane, in order of number.
    pub(crate) fn rows(&self, cx: &App) -> Vec<LaneRow> {
        (self.lanes.read(cx).summaries().into_iter())
            .map(LaneRow::new)
            .collect()
    }

    /// The rail's count: the lanes Working or with hunks pending.
    pub(crate) fn rail_count(&self, cx: &App) -> usize {
        self.lanes.read(cx).rail_count()
    }

    /// The rail item's badge: [`AgentsPanel::rail_count`] in silver-blue;
    /// none at zero.
    pub(crate) fn rail_badge(&self, cx: &App) -> Option<RailBadge> {
        let count = self.rail_count(cx);
        (count > 0).then(|| {
            let name = tr!("workbench-dock-agents-badge", count = count);
            RailBadge::count(count, BadgeTone::Agent, name)
        })
    }

    /// The selected row, if its lane is still listed.
    fn selected_ix(&self, rows: &[LaneRow]) -> Option<usize> {
        let selected = self.selected?;
        rows.iter().position(|row| row.summary.id == selected)
    }

    /// Moves the selection `by` rows, stopping at either end; with none,
    /// the first row.
    fn move_selection(&mut self, by: isize, cx: &mut Context<Self>) {
        let rows = self.rows(cx);
        let Some(last) = rows.len().checked_sub(1) else {
            return;
        };
        let ix = match self.selected_ix(&rows) {
            Some(ix) => ix.saturating_add_signed(by).min(last),
            None => 0,
        };
        self.selected = Some(rows[ix].summary.id);
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.modified() {
            return;
        }
        let vim = fe_gpui::vim_enabled(cx);
        match keystroke.key.as_str() {
            "up" => self.move_selection(-1, cx),
            "down" => self.move_selection(1, cx),
            "k" if vim => self.move_selection(-1, cx),
            "j" if vim => self.move_selection(1, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    /// Does `action` on lane `id`, as its button does. What the workbench
    /// does runs once the panel's update is out, as the dock it changes
    /// reads the panel.
    pub(crate) fn act(
        &mut self,
        id: LaneId,
        action: LaneAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workbench) = self.workbench.upgrade() else {
            return;
        };
        match action {
            LaneAction::Review => window.defer(cx, move |window, cx| {
                workbench.update(cx, |workbench, cx| {
                    workbench.review_lane(id, window, cx);
                })
            }),
            LaneAction::Stop => window.defer(cx, move |_, cx| {
                workbench.update(cx, |workbench, cx| workbench.stop_lane(id, cx))
            }),
            LaneAction::Clear => self.clear(id, window, cx),
        }
    }

    /// Clears lane `id`: at once if it has nothing pending, else, Abandoned
    /// with hunks undecided, once the user says to reject them.
    fn clear(&mut self, id: LaneId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(lane) = self.lanes.read(cx).lane(id) else {
            return;
        };
        if !(lane.state == LaneState::Abandoned && lane.pending > 0) {
            if let Some(workbench) = self.workbench.upgrade() {
                window.defer(cx, move |_, cx| {
                    workbench.update(cx, |workbench, cx| workbench.clear_lane(id, false, cx))
                });
            }
            return;
        }
        let (number, name, pending) = (id.get(), lane.name.to_string(), lane.pending);
        let detail = tr!(
            "workbench-dock-agents-clear-detail",
            lane = number,
            name = name.as_str(),
            count = pending,
        );
        let answer = window.prompt(
            PromptLevel::Warning,
            &tr!("workbench-dock-agents-clear-title", lane = number),
            Some(&detail),
            &[
                PromptButton::ok(tr!("workbench-dock-agents-clear-reject").to_string()),
                PromptButton::cancel(tr!("workbench-dock-agents-clear-cancel").to_string()),
            ],
            cx,
        );
        let workbench = self.workbench.clone();
        cx.spawn(async move |_, cx| {
            if answer.await != Ok(0) {
                return;
            }
            (workbench.update(cx, |workbench, cx| workbench.clear_lane(id, true, cx))).ok();
        })
        .detach();
    }

    /// Its element's key context.
    fn key_context(&self) -> KeyContext {
        let mut context = KeyContext::default();
        context.add(KEY_CONTEXT);
        context
    }

    /// `row`, selected or not.
    fn render_row(
        &mut self,
        row: &LaneRow,
        selected: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let ui = fe_ui::ui_theme(cx);
        let agent = cx.theme().colors.agent.base;
        #[cfg(test)]
        self.test.number_colors.push(agent);
        let focused = self.focus.contains_focused(window, cx);
        let id = row.summary.id;
        let selector = format!("{ROW}-{}", id.get());
        let buttons = (row.actions.iter()).map(|action| {
            let action = *action;
            let label = match action {
                LaneAction::Review => text!("workbench-dock-agents-review"),
                LaneAction::Stop => text!("workbench-dock-agents-stop"),
                LaneAction::Clear => text!("workbench-dock-agents-clear"),
            };
            let selector = format!("{}-{}", action.id(), id.get());
            div().flex_none().debug_selector(move || selector).child(
                Button::new((action.id(), id.get() as usize), label)
                    .small()
                    .on_click(
                        cx.listener(move |panel, _, window, cx| panel.act(id, action, window, cx)),
                    ),
            )
        });
        let buttons: Vec<Div> = buttons.collect();
        let line = |text: SharedString| {
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(text)
        };
        div()
            .id((ROW, id.get() as usize))
            .debug_selector(move || selector)
            .role(gpui::Role::ListItem)
            .aria_label(row.accessible_name())
            .w_full()
            .px_3()
            .py_1p5()
            .flex()
            .flex_col()
            .gap_1()
            .when(selected, |div| {
                div.bg(if focused {
                    ui.row_selected
                } else {
                    ui.row_selected_inactive
                })
            })
            .hover(move |div| div.bg(ui.row_hover))
            .on_click(cx.listener(move |panel, _, window, cx| {
                panel.selected = Some(id);
                window.focus(&panel.focus, cx);
                cx.notify();
            }))
            .child(
                div()
                    .when(row.dimmed, |div| div.opacity(DIMMED))
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                svg()
                                    .path(Icon::Bot.path())
                                    .size(px(14.))
                                    .flex_none()
                                    .text_color(agent),
                            )
                            .child(line(row.number()).flex_none().text_color(agent))
                            .child(line(SharedString::from(row.summary.name.to_string())).flex_1())
                            .child(line(row.state()).flex_none().text_color(ui.muted_text)),
                    )
                    .child(
                        line(row.counts())
                            .text_size(UI_SMALL.size)
                            .line_height(UI_SMALL.line_height)
                            .text_color(ui.muted_text),
                    ),
            )
            .when(!row.actions.is_empty(), |div_| {
                div_.child(div().flex().flex_wrap().gap_1().children(buttons))
            })
    }
}

impl Workbench {
    /// Stops lane `id`'s agent: lane 1's, the chat, as its Stop button
    /// does, every permission prompt refused and the reply stopped, ending
    /// its turn. Another lane's agent can't be reached yet (the agent
    /// seam's card).
    pub(crate) fn stop_lane(&mut self, id: LaneId, cx: &mut Context<Self>) {
        if id != chat_lane() {
            return;
        }
        let Some(chat) = self.dock().read(cx).chat().cloned() else {
            return;
        };
        chat.update(cx, ChatPanel::stop_turn);
    }

    /// Clears lane `id`, if it has ended and isn't lane 1. One Abandoned
    /// with hunks pending is cleared only once `confirmed`, its hunks
    /// rejected first; asked about, it is checked again as it is now.
    pub(crate) fn clear_lane(&mut self, id: LaneId, confirmed: bool, cx: &mut Context<Self>) {
        let Some(lane) = self.lanes.read(cx).lane(id) else {
            return;
        };
        if id == chat_lane() || !ended(lane.state) {
            return;
        }
        if lane.state == LaneState::Abandoned && lane.pending > 0 {
            if !confirmed {
                return;
            }
            let remaining = self.reject_lane_work(id, cx);
            if remaining > 0 {
                // Kept, so its work isn't lost undecided.
                log::warn!(
                    "lane {} keeps {remaining} hunks it couldn't reject",
                    id.get()
                );
                return;
            }
        }
        self.lanes.update(cx, |store, cx| {
            if store.clear(id).is_some() {
                cx.notify();
            }
        });
    }
}

impl Focusable for AgentsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for AgentsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = fe_ui::ui_theme(cx);
        let rows = self.rows(cx);
        let selected = self.selected_ix(&rows);
        #[cfg(test)]
        {
            self.test.rows = rows.clone();
            self.test.number_colors.clear();
            self.test.selected = selected;
        }
        let drawn: Vec<gpui::Stateful<Div>> = (rows.iter().enumerate())
            .map(|(ix, row)| self.render_row(row, selected == Some(ix), window, cx))
            .collect();
        div()
            .id("agents-panel")
            .track_focus(&self.focus)
            .key_context(self.key_context())
            .on_key_down(cx.listener(Self::on_key_down))
            .debug_selector(|| "sidebar-panel".into())
            .role(gpui::Role::List)
            .aria_label(tr!("workbench-dock-agents-title"))
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .bg(ui.panel_background)
            .text_color(ui.text)
            .text_size(px(12.))
            .children(drawn)
    }
}

/// The panel's commands' declaration: `Workbench`'s one `#[kiln::command]`
/// block is `workbench_commands.rs`'s, so they're declared here, on a type
/// no element attaches, and the workbench's root element handles them
/// ([`Workbench::attach_agents_commands`]).
#[allow(dead_code)]
struct AgentsCommands;

#[kiln::command(context = "workbench")]
#[allow(dead_code)]
impl AgentsCommands {
    /// Show Agents
    #[command(id = "agent.show_panel")]
    fn show_agents(&mut self, _: &mut Cx<Self>) {}
}

impl Workbench {
    /// `root` handling the panel's commands, in the workbench's key
    /// context.
    pub(crate) fn attach_agents_commands(root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(cx.listener(|workbench, _: &ShowAgents, window, cx| {
            workbench.run_dock_op(DockOp::ShowAgents, window, cx)
        }))
    }
}

#[cfg(test)]
#[path = "agents_panel_tests.rs"]
mod agents_panel_tests;
