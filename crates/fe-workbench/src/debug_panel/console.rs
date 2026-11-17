//! The Debug tool window's Console tab (docs/decisions/r1b9dw3g0g-debugger.md
//! §5 "Debug Console"; `docs/design/design-system.md`, Debugger, "Console";
//! card D10): the adapter's `output` events as rows by category, and a
//! one-line input that evaluates with context `repl` in the selected frame,
//! each result an expandable row of the console's own list.
//!
//! - **Output.** Each line of an event's text is a row, in small code, 18
//!   px: program output in ink, its error stream in shinsha with a 2 px
//!   edge at its left, the adapter's own messages (`console`, or no
//!   category) faint, `important` ones in kiseto with ▲. Telemetry isn't
//!   shown. Text that doesn't end its line is continued by the next event
//!   of its category, as a stream writes it. The newest
//!   [`MAX_ENTRIES`] rows and evaluations are kept.
//! - **Evaluation.** Enter echoes `› expression` in muted and asks the
//!   adapter with context `repl`, in the frame Call Stack has selected
//!   (with no frame while the program runs). Its result is a value row
//!   whose children expand as Variables' do, fetched once, while its stop
//!   lasts: a stop's references are good for that stop only, so after it
//!   the row keeps its value and no longer expands. A failure is in
//!   shinsha with ✕. Results are read out as they come.
//! - **History.** Up and down in the input recall what was evaluated, the
//!   newest first; down past the newest gives back what was being typed.
//! - **Each session** starts with an empty console; the history stays.
//! - **Keys.** Tab goes from the row of tabs to the output list and the
//!   input; in the list up and down select a row, right and left expand
//!   and collapse a result, enter toggles it; escape in the input goes back
//!   to the list.

use std::collections::{HashMap, VecDeque};

use fe_dap::{
    Epoch, StopRef,
    protocol::{EvaluateArguments, EvaluateBody, OutputBody},
};
use fe_ui::{ActiveTheme as _, CODE_SMALL, Role, States, TextInput, TextInputEvent};
use gpui::{
    AnyElement, App, AppContext as _, Context, ElementId, Entity, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, UniformListScrollHandle, Window, div, prelude::FluentBuilder as _, px,
    uniform_list,
};
use kiln_l10n::{text, tr, verbatim};
use kiln_shell::{DockView as _, PanelId};

use super::{
    DebugPanel, NodePath, PANEL, Part, Tab, ValueRow,
    rows::{Value, chevron, code_family, property_color, tooltip},
    stop::{Counts, Fetch, failure},
};
use crate::Workbench;

/// How many output rows and evaluations the console keeps, the oldest
/// going first.
pub(crate) const MAX_ENTRIES: usize = 5000;

/// How many evaluations the history keeps.
pub(crate) const HISTORY_MAX: usize = 100;

/// The selector tests find the output list by.
pub(crate) const CONSOLE_ID: &str = "debug-panel-console";

/// What kind of output a row is, by the event's `category`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Category {
    /// The adapter's own messages: `console`, or no category.
    Console,
    /// The program's output: `stdout`, or a category of the adapter's own.
    Stdout,
    /// The program's error stream.
    Stderr,
    /// What the adapter wants seen.
    Important,
}

impl Category {
    /// An event's category; `None` for telemetry, which isn't shown.
    pub(crate) fn of(category: Option<&str>) -> Option<Self> {
        match category {
            None | Some("console") => Some(Category::Console),
            Some("stderr") => Some(Category::Stderr),
            Some("important") => Some(Category::Important),
            Some("telemetry") => None,
            Some(_) => Some(Category::Stdout),
        }
    }

    /// How its rows are drawn.
    pub(crate) fn style(self, cx: &App) -> RowStyle {
        let c = cx.theme().colors;
        match self {
            Category::Console => RowStyle {
                ink: c.text.faint,
                edge: None,
                mark: false,
            },
            Category::Stdout => RowStyle {
                ink: c.text.primary,
                edge: None,
                mark: false,
            },
            Category::Stderr => RowStyle {
                ink: c.status.error,
                edge: Some(c.status.error),
                mark: false,
            },
            Category::Important => RowStyle {
                ink: c.status.warning,
                edge: None,
                mark: true,
            },
        }
    }
}

/// An output row's look: its text's colour, the colour of its 2 px left
/// edge if it has one, and whether ▲ marks it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RowStyle {
    pub(crate) ink: Hsla,
    pub(crate) edge: Option<Hsla>,
    pub(crate) mark: bool,
}

/// One thing the console holds: a line of output, or an evaluation.
enum Entry {
    Output {
        category: Category,
        text: String,
    },
    Evaluation {
        id: u64,
        expression: String,
        /// The stop it was evaluated in, whose references its result's are.
        epoch: Option<Epoch>,
        /// Boxed: an output row is far smaller.
        value: Fetch<Box<EvaluateBody>>,
    },
}

/// A row of the console's list.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ConsoleRow {
    /// A line of output.
    Output { category: Category, text: String },
    /// `› expression`, as it was evaluated.
    Echo { expression: String },
    /// An evaluation not answered yet.
    Evaluating,
    /// Its result: it expands while its stop lasts (its reference is 0
    /// after).
    Result {
        id: u64,
        value: Value,
        expanded: bool,
    },
    /// Why it has none.
    Failed { error: SharedString },
    /// A child of an expanded result, as Variables draws it.
    Child(ValueRow),
}

impl ConsoleRow {
    /// Its children's reference and counts, if it has children now.
    fn reference(&self) -> Option<(i64, Counts)> {
        match self {
            ConsoleRow::Result { value, .. } => {
                (value.reference > 0).then_some((value.reference, value.counts))
            }
            ConsoleRow::Child(row) => row.reference(),
            _ => None,
        }
    }

    pub(crate) fn expandable(&self) -> bool {
        self.reference().is_some()
    }

    pub(crate) fn expanded(&self) -> bool {
        match self {
            ConsoleRow::Result { expanded, .. } => *expanded,
            ConsoleRow::Child(row) => row.expanded(),
            _ => false,
        }
    }

    /// What a screen reader names it.
    pub(crate) fn name(&self) -> SharedString {
        match self {
            ConsoleRow::Output { text, .. } if text.trim().is_empty() => {
                tr!("workbench-debug-console-blank")
            }
            ConsoleRow::Output {
                category: Category::Stderr,
                text,
            } => tr!("workbench-debug-console-stderr-name", text = text.as_str()),
            ConsoleRow::Output {
                category: Category::Important,
                text,
            } => tr!(
                "workbench-debug-console-important-name",
                text = text.as_str()
            ),
            ConsoleRow::Output { text, .. } => text.clone().into(),
            ConsoleRow::Echo { expression } => echo(expression),
            ConsoleRow::Evaluating => tr!("workbench-debug-panel-loading"),
            ConsoleRow::Result { value, .. } => result_name(value),
            ConsoleRow::Failed { error } => tr!(
                "workbench-debug-console-failed-name",
                error = error.as_ref()
            ),
            ConsoleRow::Child(row) => row.name(),
        }
    }

    /// Its depth in the list: a result's children below it.
    fn level(&self) -> usize {
        match self {
            ConsoleRow::Child(row) => row.depth() + 1,
            _ => 1,
        }
    }
}

fn echo(expression: &str) -> SharedString {
    tr!("workbench-debug-console-echo", expression = expression)
}

/// A result's name and announcement: its value, and its type if it has
/// one.
fn result_name(value: &Value) -> SharedString {
    match &value.r#type {
        Some(kind) => tr!(
            "workbench-debug-console-result-name",
            value = value.text.as_str(),
            kind = kind.as_str()
        ),
        None => value.text.clone().into(),
    }
}

/// A result's value, its children's reference dropped unless it's of the
/// stop shown.
fn value_of(body: &EvaluateBody, live: bool) -> Value {
    Value {
        text: body.result.clone(),
        r#type: body.r#type.clone().filter(|kind| !kind.is_empty()),
        reference: if live { body.variables_reference } else { 0 },
        counts: Counts {
            indexed: body.indexed_variables,
            named: body.named_variables,
        },
    }
}

/// The console: its entries, its input and its history.
pub(crate) struct Console {
    entries: VecDeque<Entry>,
    /// The category of the last output while its last line is open: the
    /// next event of that category continues it.
    open: Option<Category>,
    pub(super) input: Entity<TextInput>,
    history: Vec<String>,
    /// The history entry the input shows, while recalling.
    recall: Option<usize>,
    /// What was typed before recalling.
    draft: String,
    next_id: u64,
    /// The evaluations under way, by id: given up with their entry.
    tasks: HashMap<u64, Task<()>>,
    pub(super) scroll: UniformListScrollHandle,
    _submitted: Subscription,
}

impl Console {
    pub(super) fn new(cx: &mut Context<DebugPanel>) -> Self {
        let ui = fe_ui::ui_theme(cx);
        let input = cx.new(|cx| {
            TextInput::new(text!("workbench-debug-console-placeholder"), &ui, cx)
                .height(fe_ui::height::CONTROL)
        });
        let submitted = cx.subscribe(&input, |panel, input, event, cx| {
            if *event == TextInputEvent::Submitted {
                let expression = input.read(cx).text().trim().to_owned();
                input.update(cx, |input, cx| input.clear(cx));
                panel.evaluate_in_console(expression, cx);
            }
        });
        Console {
            entries: VecDeque::new(),
            open: None,
            input,
            history: Vec::new(),
            recall: None,
            draft: String::new(),
            next_id: 0,
            tasks: HashMap::new(),
            scroll: UniformListScrollHandle::new(),
            _submitted: submitted,
        }
    }

    /// Empties it for a new session: the history stays.
    pub(super) fn clear(&mut self) {
        self.entries.clear();
        self.tasks.clear();
        self.open = None;
    }

    /// The history, oldest first.
    #[cfg(test)]
    pub(crate) fn history(&self) -> &[String] {
        &self.history
    }

    /// Whether every evaluation is answered.
    fn answered(&self) -> bool {
        self.tasks.is_empty()
    }
}

impl DebugPanel {
    /// The console's input.
    #[cfg(test)]
    pub(crate) fn console_input(&self) -> &Entity<TextInput> {
        &self.console.input
    }

    /// The console's history, oldest first.
    #[cfg(test)]
    pub(crate) fn console_history(&self) -> &[String] {
        self.console.history()
    }

    /// Whether the session is one to evaluate in: started and not ended.
    /// The input shows only then.
    pub(super) fn console_live(&self, cx: &App) -> bool {
        (self.debugger()).is_some_and(|debugger| {
            let debugger = debugger.read(cx);
            debugger.starting().is_none() && debugger.state().is_some_and(|state| !state.is_ended())
        })
    }

    /// An `output` event: its lines join the console, the first continuing
    /// the last row if that line was left open by its category. The list
    /// follows its end if it was there.
    pub(crate) fn console_output(&mut self, body: &OutputBody, cx: &mut Context<Self>) {
        let Some(category) = Category::of(body.category.as_deref()) else {
            return;
        };
        let text = body.output.replace("\r\n", "\n");
        if text.is_empty() {
            return;
        }
        let follow = self.console.scroll.is_scrolled_to_end().unwrap_or(true);
        let ends = text.ends_with('\n');
        let lines = text.strip_suffix('\n').unwrap_or(&text);
        for (ix, line) in lines.split('\n').enumerate() {
            let line = line.strip_suffix('\r').unwrap_or(line);
            let open = (ix == 0 && self.console.open == Some(category))
                .then(|| self.console.entries.back_mut())
                .flatten();
            match open {
                Some(Entry::Output { text, .. }) => text.push_str(line),
                _ => self.console.entries.push_back(Entry::Output {
                    category,
                    text: line.to_owned(),
                }),
            }
        }
        self.console.open = (!ends).then_some(category);
        self.trim_console();
        if follow {
            self.console.scroll.scroll_to_bottom();
        }
        cx.notify();
    }

    /// Drops the oldest entries past [`MAX_ENTRIES`], the selected row
    /// moving up with the rows.
    fn trim_console(&mut self) {
        while self.console.entries.len() > MAX_ENTRIES {
            let Some(entry) = self.console.entries.front() else {
                return;
            };
            let (mut rows, mut missing) = (Vec::new(), Vec::new());
            self.entry_rows(entry, &mut rows, &mut missing);
            let gone = rows.len();
            if let Some(Entry::Evaluation { id, .. }) = self.console.entries.pop_front() {
                self.console.tasks.remove(&id);
            }
            match self.selected.get(&Part::Console).copied() {
                Some(ix) if ix >= gone => {
                    self.selected.insert(Part::Console, ix - gone);
                }
                Some(_) => {
                    self.selected.remove(&Part::Console);
                }
                None => {}
            }
        }
    }

    /// Evaluates `expression` with context `repl` in the selected frame, or
    /// with none while the program runs: its echo and then its result join
    /// the console, and it joins the history. Nothing for a blank one or
    /// with no session.
    pub(crate) fn evaluate_in_console(&mut self, expression: String, cx: &mut Context<Self>) {
        let expression = expression.trim().to_owned();
        if expression.is_empty() {
            return;
        }
        let console = &mut self.console;
        if console.history.last() != Some(&expression) {
            console.history.push(expression.clone());
            if console.history.len() > HISTORY_MAX {
                console.history.remove(0);
            }
        }
        console.recall = None;
        console.draft.clear();
        if !self.console_live(cx) {
            return;
        }
        let frame = self.live_session(cx).and_then(|(epoch, _)| {
            let frame = self.stop.as_ref()?.selected_frame()?;
            Some(StopRef::new(epoch, frame.id))
        });
        let arguments = EvaluateArguments {
            expression: expression.clone(),
            context: Some(verbatim!("repl").into()),
            ..Default::default()
        };
        let Some(debugger) = self.debugger() else {
            return;
        };
        let request =
            (debugger.read(cx).session()).map(|session| session.evaluate(&arguments, frame));
        let Some(request) = request else {
            return;
        };
        let id = self.console.next_id;
        self.console.next_id += 1;
        self.console.entries.push_back(Entry::Evaluation {
            id,
            expression,
            epoch: frame.map(|frame| frame.epoch),
            value: Fetch::Pending,
        });
        self.console.open = None;
        let task = cx.spawn(async move |panel, cx| {
            let reply = request.await;
            panel
                .update(cx, |panel, cx| panel.evaluated(id, reply, cx))
                .ok();
        });
        self.console.tasks.insert(id, task);
        self.trim_console();
        self.console.scroll.scroll_to_bottom();
        cx.notify();
    }

    /// Evaluation `id` was answered: its result shows, read out as its
    /// value, and its children are fetched if it's expanded already.
    fn evaluated(
        &mut self,
        id: u64,
        reply: Result<EvaluateBody, fe_dap::RequestError>,
        cx: &mut Context<Self>,
    ) {
        self.console.tasks.remove(&id);
        let follow = self.console.scroll.is_scrolled_to_end().unwrap_or(true);
        let entry = (self.console.entries.iter_mut())
            .find(|entry| matches!(entry, Entry::Evaluation { id: have, .. } if *have == id));
        let Some(Entry::Evaluation { value, .. }) = entry else {
            return;
        };
        let said = match reply {
            Ok(body) => {
                let said = result_name(&value_of(&body, false));
                *value = Fetch::Ready(Box::new(body));
                said
            }
            Err(error) => {
                let error = failure(&error);
                let said = tr!(
                    "workbench-debug-console-failed-name",
                    error = error.as_ref()
                );
                *value = Fetch::Failed(error);
                said
            }
        };
        fe_ui::announce(cx, said, fe_ui::Politeness::Polite);
        self.fetch_expanded(cx);
        if follow {
            self.console.scroll.scroll_to_bottom();
        }
        cx.notify();
    }

    /// The node path of result `id`, its children's under it.
    fn result_path(id: u64) -> NodePath {
        vec![verbatim!("repl").into(), id.to_string().into()]
    }

    /// The console's rows, and the expanded nodes whose children this stop
    /// hasn't fetched.
    pub(crate) fn console_rows(&self) -> (Vec<ConsoleRow>, Vec<(i64, Counts)>) {
        let (mut rows, mut missing) = (Vec::new(), Vec::new());
        for entry in &self.console.entries {
            self.entry_rows(entry, &mut rows, &mut missing);
        }
        (rows, missing)
    }

    fn entry_rows(
        &self,
        entry: &Entry,
        rows: &mut Vec<ConsoleRow>,
        missing: &mut Vec<(i64, Counts)>,
    ) {
        let (id, expression, epoch, value) = match entry {
            Entry::Output { category, text } => {
                return rows.push(ConsoleRow::Output {
                    category: *category,
                    text: text.clone(),
                });
            }
            Entry::Evaluation {
                id,
                expression,
                epoch,
                value,
            } => (*id, expression, *epoch, value),
        };
        rows.push(ConsoleRow::Echo {
            expression: expression.clone(),
        });
        let body = match value {
            Fetch::Pending => return rows.push(ConsoleRow::Evaluating),
            Fetch::Failed(error) => {
                return rows.push(ConsoleRow::Failed {
                    error: error.clone(),
                });
            }
            Fetch::Ready(body) => body,
        };
        // Its references are good while its stop is the one shown.
        let stop = (self.stop.as_ref()).filter(|stop| epoch == Some(stop.epoch));
        let value = value_of(body, stop.is_some());
        let path = Self::result_path(id);
        let expanded = value.reference > 0 && self.is_expanded(&path, false);
        let open = expanded.then_some((value.reference, value.counts));
        rows.push(ConsoleRow::Result {
            id,
            value,
            expanded,
        });
        if let (Some((reference, counts)), Some(stop)) = (open, stop) {
            let mut children = Vec::new();
            self.walk(stop, reference, counts, &path, 1, &mut children, missing);
            rows.extend(children.into_iter().map(ConsoleRow::Child));
        }
    }

    /// Expands or collapses a result or a child, fetching its children the
    /// first time this stop; "Show 100 more" shows more.
    pub(crate) fn click_console_row(&mut self, ix: usize, cx: &mut Context<Self>) {
        match self.console_rows().0.get(ix).cloned() {
            Some(ConsoleRow::Result {
                id,
                value,
                expanded,
            }) if value.reference > 0 => {
                self.expanded.insert(Self::result_path(id), !expanded);
                if !expanded {
                    self.fetch_children(value.reference, value.counts, cx);
                }
                cx.notify();
            }
            Some(ConsoleRow::Child(row @ ValueRow::More { .. })) => self.show_more(&row, cx),
            Some(ConsoleRow::Child(row)) if row.expandable() => self.toggle_node(&row, cx),
            _ => {}
        }
    }

    /// Right (`expand`) or left on the list's selected row.
    pub(super) fn expand_console_row(
        &mut self,
        ix: usize,
        expand: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        match self.console_rows().0.get(ix) {
            Some(row) if row.expandable() && row.expanded() != expand => {
                self.click_console_row(ix, cx);
                true
            }
            _ => false,
        }
    }

    /// Up (`by` -1) or down (1) in the input: the history entry before or
    /// after the one shown; down past the newest gives back what was being
    /// typed. Whether the key did anything.
    pub(super) fn recall(&mut self, by: isize, cx: &mut Context<Self>) -> bool {
        let console = &mut self.console;
        let len = console.history.len();
        if len == 0 {
            return false;
        }
        let next = match (console.recall, by < 0) {
            (None, true) => {
                console.draft = console.input.read(cx).text().to_owned();
                len - 1
            }
            (None, false) => return false,
            (Some(at), true) => at.saturating_sub(1),
            (Some(at), false) if at + 1 < len => at + 1,
            (Some(_), false) => {
                console.recall = None;
                let draft = std::mem::take(&mut console.draft);
                console
                    .input
                    .update(cx, |input, cx| input.set_text(draft, cx));
                return true;
            }
        };
        console.recall = Some(next);
        let text = console.history[next].clone();
        console
            .input
            .update(cx, |input, cx| input.set_text(text, cx));
        true
    }

    /// The input's accessible name: the frame it evaluates in, while one
    /// is selected.
    fn console_input_name(&self) -> Option<SharedString> {
        let frame = self.stop.as_ref()?.selected_frame()?;
        Some(tr!(
            "workbench-debug-console-input-name",
            frame = frame.name.as_str()
        ))
    }

    pub(super) fn render_console_tab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let live = self.console_live(cx);
        let len = self.console_rows().0.len();
        if len == 0 && !live {
            return self.empty(tr!("workbench-debug-console-no-session"), None, cx);
        }
        let c = cx.theme().colors;
        let name = self.console_input_name();
        (self.console.input).update(cx, |input, cx| input.set_name(name, cx));
        let list = div()
            .id(CONSOLE_ID_ELEMENT)
            .debug_selector(|| CONSOLE_ID.to_owned())
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .py_1()
            .child(
                uniform_list(
                    verbatim!("debug-console-rows"),
                    len,
                    cx.processor(|panel, range, window, cx| {
                        panel.render_console_rows(range, window, cx)
                    }),
                )
                .track_scroll(&self.console.scroll)
                .flex_1(),
            );
        let list = fe_ui::node(
            list,
            window,
            cx,
            Role::List,
            tr!("workbench-debug-console-output"),
        );
        let foot = div()
            .flex_none()
            .h(fe_ui::height::CONTROL)
            .px_2()
            .flex()
            .items_center()
            .gap_1()
            .border_t_1()
            .border_color(c.border.subtle);
        let foot = if live {
            foot.on_action(cx.listener(Self::on_input_command))
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|panel, _, _, _| panel.part = Part::ConsoleInput),
                )
                .child(
                    div()
                        .flex_none()
                        .font_family(code_family(cx))
                        .text_size(CODE_SMALL.size)
                        .text_color(c.text.muted)
                        .child(verbatim!("›")),
                )
                .child(div().flex_1().min_w_0().child(self.console.input.clone()))
        } else {
            foot.text_size(fe_ui::UI_SMALL.size)
                .text_color(c.text.muted)
                .child(tr!("workbench-debug-console-no-session"))
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(list)
            .child(foot)
            .into_any_element()
    }

    fn render_console_rows(
        &mut self,
        range: std::ops::Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let rows = self.console_rows().0;
        let c = cx.theme().colors;
        let ui = fe_ui::ui_theme(cx);
        let code = code_family(cx);
        let property = property_color(self, cx).unwrap_or(c.text.accent);
        let height = CODE_SMALL.line_height;
        range
            .filter_map(|ix| {
                let row = rows.get(ix)?.clone();
                let (_, cursor) = self.ground(Part::Console, ix, false, window, cx);
                let small = |text: SharedString, color: Hsla| div().text_color(color).child(text);
                let mut element = div()
                    .id(ElementId::NamedInteger(
                        verbatim!("debug-console-row").into(),
                        ix as u64,
                    ))
                    .h(height)
                    .w_full()
                    .pl_2()
                    .pr_2()
                    .flex()
                    .items_center()
                    .gap_1()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .font_family(code.clone())
                    .text_size(CODE_SMALL.size)
                    .when_some(cursor, |row, border| row.border_1().border_color(border))
                    .hover(move |row| row.bg(ui.row_hover))
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        panel.select_row(Part::Console, ix, window, cx);
                        panel.click_console_row(ix, cx);
                    }));
                let expandable = row.expandable().then(|| row.expanded());
                element = match &row {
                    ConsoleRow::Output { category, text } => {
                        let style = category.style(cx);
                        element
                            // The edge in the padding's first 2 px.
                            .when_some(style.edge, |row, edge| {
                                row.pl(px(8.) - fe_ui::glyph::STROKE)
                                    .border_l(fe_ui::glyph::STROKE)
                                    .border_color(edge)
                            })
                            .when(style.mark, |row| {
                                row.child(small(verbatim!("▲").into(), style.ink).flex_none())
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .text_color(style.ink)
                                    .child(text.clone()),
                            )
                            .when(!text.is_empty(), |row| row.tooltip(tooltip(text.clone())))
                    }
                    ConsoleRow::Echo { expression } => {
                        element.child(small(echo(expression), c.text.muted))
                    }
                    ConsoleRow::Evaluating => {
                        element.child(small(tr!("workbench-debug-panel-loading"), c.text.muted))
                    }
                    ConsoleRow::Result { value, .. } => element
                        .child(chevron(expandable, ui.chevron))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .text_color(c.text.primary)
                                .child(value.text.clone()),
                        )
                        .when_some(value.r#type.clone(), |row, kind| {
                            row.child(small(kind.into(), c.text.faint).flex_none())
                        })
                        .tooltip(tooltip(value.text.clone())),
                    ConsoleRow::Failed { error } => element.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_color(c.status.error)
                            .child(tr!(
                                "workbench-debug-panel-watch-failed",
                                error = error.as_ref()
                            )),
                    ),
                    ConsoleRow::Child(child) => {
                        let element = element
                            .pl(px(8. + 12. * child.depth() as f32))
                            .child(chevron(expandable, ui.chevron));
                        match child {
                            ValueRow::Variable { name, value, .. } => element
                                .child(small(name.clone().into(), property))
                                .child(small(verbatim!(" = ").into(), c.text.muted))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .text_color(c.text.primary)
                                        .child(value.text.clone()),
                                )
                                .when_some(value.r#type.clone(), |row, kind| {
                                    row.child(small(kind.into(), c.text.faint).flex_none())
                                })
                                .tooltip(tooltip(value.text.clone())),
                            ValueRow::More { .. } => {
                                element.child(small(child.name(), c.text.accent))
                            }
                            ValueRow::Message { text, error, .. } => element.child(small(
                                text.clone(),
                                if *error { c.status.error } else { c.text.muted },
                            )),
                            ValueRow::Watch { .. } | ValueRow::Scope { .. } => element,
                        }
                    }
                };
                let states = States {
                    expanded: expandable,
                    level: Some(row.level()),
                    ..Default::default()
                };
                Some(
                    fe_ui::node_with(element, window, cx, Role::ListItem, row.name(), states)
                        .into_any_element(),
                )
            })
            .collect()
    }

    /// Shows the Console tab, the keys and focus in its input if it's
    /// drawn, else in its list.
    pub(super) fn show_console(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pick_tab(Tab::Console, cx);
        let part = match self.console_live(cx) {
            true => Part::ConsoleInput,
            false => Part::Console,
        };
        self.focus_part(part, window, cx);
        cx.notify();
    }
}

const CONSOLE_ID_ELEMENT: &str = CONSOLE_ID;

impl Workbench {
    /// `debug.show_console`: shows the Debug tool window on its Console,
    /// its input focused; if it shows the Console already, hides the
    /// bottom panel, focus going back to the editor if it was in it.
    pub(crate) fn toggle_debug_console(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let panel = self.debug_panel.clone();
        let shown = self.terminal_dock().read(cx).dock().rail_highlighted();
        if shown == Some(PanelId(PANEL)) && panel.read(cx).tab == Tab::Console {
            return self.toggle_debug_panel(window, cx);
        }
        self.dock().update(cx, |dock, cx| dock.join_debug(cx));
        self.show_bottom_panel(PanelId(PANEL), window, cx);
        panel.update(cx, |panel, cx| panel.show_console(window, cx));
    }

    /// Evaluates `expression` in the Debug Console, as typing it and enter
    /// does, for the UI audit's captures; `expand` expands its result once
    /// it comes.
    #[doc(hidden)]
    pub fn evaluate_in_debug_console(
        &mut self,
        expression: &str,
        expand: bool,
        cx: &mut Context<Self>,
    ) {
        (self.debug_panel.clone()).update(cx, |panel, cx| {
            let id = panel.console.next_id;
            if expand {
                panel.expanded.insert(DebugPanel::result_path(id), true);
            }
            panel.evaluate_in_console(expression.to_owned(), cx);
        });
    }

    /// Whether the Debug Console has every evaluation answered and every
    /// expanded result's children fetched, for the UI audit's captures.
    #[doc(hidden)]
    pub fn debug_console_settled(&self, cx: &App) -> bool {
        let panel = self.debug_panel.read(cx);
        let (rows, missing) = panel.console_rows();
        panel.console.answered()
            && missing.is_empty()
            && !rows.iter().any(|row| {
                matches!(
                    row,
                    ConsoleRow::Child(ValueRow::Message { error: false, .. })
                )
            })
    }
}
