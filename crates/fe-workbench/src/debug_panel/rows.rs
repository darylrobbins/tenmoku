//! The Variables tab's rows: the call stack's threads and frames, the
//! watches and the variables, as flat lists of tree rows, and how each is
//! drawn (`docs/design/design-system.md`, "The Debug tool window").

use std::ops::Range;

use fe_dap::protocol::Variable;
use fe_ui::{ActiveTheme as _, CODE_SMALL, FamilySlot, FontFamilies, Icon, Role, States};
use gpui::{
    AnyElement, App, Context, ElementId, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px, svg, uniform_list,
};
use kiln_l10n::{tr, verbatim};

use super::{
    DebugPanel, NodePath, Part, frame_path,
    stop::{Counts, Fetch, Stop},
};

/// A row of the Watch or Variables tree.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ValueRow {
    /// The `index`th watch: its value, or why it has none, once evaluated.
    Watch {
        index: usize,
        expression: String,
        value: Option<Result<Value, SharedString>>,
        expanded: bool,
    },
    /// A scope of the selected frame, at the tree's top.
    Scope {
        name: String,
        reference: i64,
        counts: Counts,
        expanded: bool,
    },
    Variable {
        path: NodePath,
        depth: usize,
        name: String,
        value: Value,
        expanded: bool,
    },
    /// "Show 100 more" of `reference`'s children.
    More {
        reference: i64,
        depth: usize,
        remaining: usize,
    },
    /// Loading, a failure, or nothing there.
    Message {
        depth: usize,
        text: SharedString,
        error: bool,
    },
}

/// A value as a row shows it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Value {
    pub(crate) text: String,
    pub(crate) r#type: Option<String>,
    /// Its children's reference, 0 for none.
    pub(crate) reference: i64,
    pub(crate) counts: Counts,
}

impl Value {
    fn of(variable: &Variable) -> Self {
        Value {
            text: variable.value.clone(),
            r#type: variable.r#type.clone().filter(|kind| !kind.is_empty()),
            reference: variable.variables_reference,
            counts: Counts {
                indexed: variable.indexed_variables,
                named: variable.named_variables,
            },
        }
    }
}

impl ValueRow {
    pub(crate) fn path(&self) -> Option<NodePath> {
        match self {
            ValueRow::Watch { expression, .. } => Some(DebugPanel::watch_path(expression)),
            ValueRow::Scope { name, .. } => Some(DebugPanel::scope_path(name)),
            ValueRow::Variable { path, .. } => Some(path.clone()),
            _ => None,
        }
    }

    /// Its children's reference and counts, if it has children.
    pub(crate) fn reference(&self) -> Option<(i64, Counts)> {
        let (reference, counts) = match self {
            ValueRow::Watch {
                value: Some(Ok(value)),
                ..
            }
            | ValueRow::Variable { value, .. } => (value.reference, value.counts),
            ValueRow::Scope {
                reference, counts, ..
            } => (*reference, *counts),
            _ => return None,
        };
        (reference > 0).then_some((reference, counts))
    }

    pub(crate) fn expandable(&self) -> bool {
        self.reference().is_some()
    }

    pub(crate) fn expanded(&self) -> bool {
        match self {
            ValueRow::Watch { expanded, .. }
            | ValueRow::Scope { expanded, .. }
            | ValueRow::Variable { expanded, .. } => *expanded,
            _ => false,
        }
    }

    pub(super) fn depth(&self) -> usize {
        match self {
            ValueRow::Watch { .. } | ValueRow::Scope { .. } => 0,
            ValueRow::Variable { depth, .. }
            | ValueRow::More { depth, .. }
            | ValueRow::Message { depth, .. } => *depth,
        }
    }

    /// What a screen reader names it: a variable's "name, type, value".
    pub(crate) fn name(&self) -> SharedString {
        let named = |name: &str, value: &Value| match &value.r#type {
            Some(kind) => tr!(
                "workbench-debug-panel-variable-name",
                name = name,
                kind = kind.as_str(),
                value = value.text.as_str()
            ),
            None => tr!(
                "workbench-debug-panel-variable-name-untyped",
                name = name,
                value = value.text.as_str()
            ),
        };
        match self {
            ValueRow::Watch {
                expression, value, ..
            } => match value {
                Some(Ok(value)) => named(expression, value),
                Some(Err(error)) => tr!(
                    "workbench-debug-panel-watch-failed-name",
                    expression = expression.as_str(),
                    error = error.as_ref()
                ),
                None => expression.clone().into(),
            },
            ValueRow::Scope { name, .. } => name.clone().into(),
            ValueRow::Variable { name, value, .. } => named(name, value),
            ValueRow::More { .. } => more_label(),
            ValueRow::Message { text, .. } => text.clone(),
        }
    }
}

/// A row of the Call Stack tree.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum StackRow {
    Thread {
        id: i64,
        label: SharedString,
        expanded: bool,
    },
    Frame {
        thread: i64,
        index: usize,
        function: String,
        /// Its file's name and line, if it has a file.
        file: Option<(String, u32)>,
        /// No source, or `subtle`: drawn faint.
        faint: bool,
        /// The stopped thread's top frame.
        current: bool,
        selected: bool,
    },
    /// "Show N more frames" of thread `thread`, which has `remaining`
    /// more than fetched.
    More {
        thread: i64,
        remaining: Option<usize>,
    },
    Message {
        text: SharedString,
        error: bool,
    },
}

/// The label of a thread's row that fetches its next page of frames.
fn more_frames_label(remaining: Option<usize>) -> SharedString {
    match remaining {
        Some(remaining) => {
            let count = remaining.min(fe_dap::FRAME_PAGE as usize);
            tr!("workbench-debug-panel-show-more-frames", count = count)
        }
        None => tr!("workbench-debug-panel-show-more-frames-unknown"),
    }
}

impl StackRow {
    /// What a screen reader names it: a frame's "render, minimap.rs line
    /// 42, current frame".
    pub(crate) fn name(&self) -> SharedString {
        match self {
            StackRow::Thread { label, .. } => label.clone(),
            StackRow::Frame {
                function,
                file,
                current,
                ..
            } => match (file, current) {
                (Some((file, line)), true) => tr!(
                    "workbench-debug-panel-frame-name-current",
                    function = function.as_str(),
                    file = file.as_str(),
                    line = *line
                ),
                (Some((file, line)), false) => tr!(
                    "workbench-debug-panel-frame-name",
                    function = function.as_str(),
                    file = file.as_str(),
                    line = *line
                ),
                (None, true) => tr!(
                    "workbench-debug-panel-frame-name-no-source-current",
                    function = function.as_str()
                ),
                (None, false) => function.clone().into(),
            },
            StackRow::More { remaining, .. } => more_frames_label(*remaining),
            StackRow::Message { text, .. } => text.clone(),
        }
    }
}

fn more_label() -> SharedString {
    tr!("workbench-debug-panel-show-more", count = super::PAGE)
}

fn loading(depth: usize) -> ValueRow {
    ValueRow::Message {
        depth,
        text: tr!("workbench-debug-panel-loading"),
        error: false,
    }
}

impl DebugPanel {
    /// The Call Stack's rows.
    pub(crate) fn stack_rows(&self, _: &App) -> Vec<StackRow> {
        let Some(stop) = &self.stop else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        for thread in &stop.threads {
            let label = match (thread.id == stop.thread_id, &stop.reason) {
                (true, Some(reason)) => tr!(
                    "workbench-debug-panel-thread-stopped-reason",
                    name = thread.name.as_str(),
                    reason = reason.as_str()
                ),
                (true, None) => tr!(
                    "workbench-debug-panel-thread-stopped",
                    name = thread.name.as_str()
                ),
                (false, _) => tr!(
                    "workbench-debug-panel-thread-paused",
                    name = thread.name.as_str()
                ),
            };
            rows.push(StackRow::Thread {
                id: thread.id,
                label,
                expanded: thread.expanded,
            });
            if !thread.expanded {
                continue;
            }
            match &thread.frames {
                None | Some(Fetch::Pending) => rows.push(StackRow::Message {
                    text: tr!("workbench-debug-panel-loading"),
                    error: false,
                }),
                Some(Fetch::Failed(error)) => rows.push(StackRow::Message {
                    text: error.clone(),
                    error: true,
                }),
                Some(Fetch::Ready(frames)) => {
                    for (index, frame) in frames.iter().enumerate() {
                        let path = frame_path(frame);
                        let file = path.as_ref().map(|path| {
                            let name = path.file_name().unwrap_or(path.as_os_str());
                            let line = u32::try_from(frame.line.max(1)).unwrap_or(1);
                            (name.to_string_lossy().into_owned(), line)
                        });
                        let subtle = frame.presentation_hint.as_deref() == Some("subtle");
                        rows.push(StackRow::Frame {
                            thread: thread.id,
                            index,
                            function: frame.name.clone(),
                            file,
                            faint: path.is_none() || subtle,
                            current: thread.id == stop.thread_id && index == 0,
                            selected: stop.selected == (thread.id, index),
                        });
                    }
                    if thread.loading_more {
                        rows.push(StackRow::Message {
                            text: tr!("workbench-debug-panel-loading"),
                            error: false,
                        });
                    } else if thread.has_more() {
                        rows.push(StackRow::More {
                            thread: thread.id,
                            remaining: thread.remaining(),
                        });
                    }
                }
            }
        }
        rows
    }

    /// The Watch tree's rows, and the expanded nodes whose children this
    /// stop hasn't fetched.
    pub(crate) fn watch_rows(&self) -> (Vec<ValueRow>, Vec<(i64, Counts)>) {
        let (mut rows, mut missing) = (Vec::new(), Vec::new());
        for (index, expression) in self.watches.iter().enumerate() {
            let path = Self::watch_path(expression);
            let value = (self.stop.as_ref())
                .and_then(|stop| stop.watches.get(expression))
                .and_then(|value| match value {
                    Fetch::Pending => None,
                    Fetch::Ready(body) => Some(Ok(Value {
                        text: body.result.clone(),
                        r#type: body.r#type.clone().filter(|kind| !kind.is_empty()),
                        reference: body.variables_reference,
                        counts: Counts {
                            indexed: body.indexed_variables,
                            named: body.named_variables,
                        },
                    })),
                    Fetch::Failed(error) => Some(Err(error.clone())),
                });
            let row = ValueRow::Watch {
                index,
                expression: expression.clone(),
                value,
                expanded: self.is_expanded(&path, false),
            };
            let open = row.expanded().then(|| row.reference()).flatten();
            rows.push(row);
            if let (Some((reference, counts)), Some(stop)) = (open, &self.stop) {
                self.walk(stop, reference, counts, &path, 1, &mut rows, &mut missing);
            }
        }
        (rows, missing)
    }

    /// The Variables tree's rows, and the expanded nodes whose children
    /// this stop hasn't fetched.
    pub(crate) fn variable_rows(&self) -> (Vec<ValueRow>, Vec<(i64, Counts)>) {
        let (mut rows, mut missing) = (Vec::new(), Vec::new());
        let Some(stop) = &self.stop else {
            return (rows, missing);
        };
        match &stop.scopes {
            Fetch::Pending => rows.push(loading(0)),
            Fetch::Failed(error) => rows.push(ValueRow::Message {
                depth: 0,
                text: error.clone(),
                error: true,
            }),
            Fetch::Ready(scopes) if scopes.is_empty() => rows.push(ValueRow::Message {
                depth: 0,
                text: tr!("workbench-debug-panel-no-variables"),
                error: false,
            }),
            Fetch::Ready(scopes) => {
                for (ix, scope) in scopes.iter().enumerate() {
                    let path = Self::scope_path(&scope.name);
                    let expanded = self.is_expanded(&path, ix == 0);
                    let reference = scope.variables_reference;
                    let counts = Counts {
                        indexed: scope.indexed_variables,
                        named: scope.named_variables,
                    };
                    rows.push(ValueRow::Scope {
                        name: scope.name.clone(),
                        reference,
                        counts,
                        expanded,
                    });
                    if expanded && reference > 0 {
                        self.walk(stop, reference, counts, &path, 1, &mut rows, &mut missing);
                    }
                }
            }
        }
        (rows, missing)
    }

    /// `reference`'s children under `path`, at `depth`, as far as shown:
    /// those this stop fetched, the expanded ones' below them.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn walk(
        &self,
        stop: &Stop,
        reference: i64,
        counts: Counts,
        path: &NodePath,
        depth: usize,
        rows: &mut Vec<ValueRow>,
        missing: &mut Vec<(i64, Counts)>,
    ) {
        let Some(children) = stop.children.get(&reference) else {
            missing.push((reference, counts));
            return rows.push(loading(depth));
        };
        match &children.state {
            Fetch::Pending => return rows.push(loading(depth)),
            Fetch::Failed(error) if children.variables.is_empty() && children.named.is_empty() => {
                return rows.push(ValueRow::Message {
                    depth,
                    text: error.clone(),
                    error: true,
                });
            }
            _ => {}
        }
        // A paged node's named children, then its indexed ones as far as
        // shown.
        let shown = children.shown.min(children.variables.len());
        for variable in children.named.iter().chain(&children.variables[..shown]) {
            let mut child = path.clone();
            child.push(variable.name.clone().into());
            let expanded = self.is_expanded(&child, false);
            let value = Value::of(variable);
            let open = (expanded && value.reference > 0).then_some((value.reference, value.counts));
            rows.push(ValueRow::Variable {
                path: child.clone(),
                depth,
                name: variable.name.clone(),
                value,
                expanded,
            });
            if let Some((reference, counts)) = open {
                self.walk(stop, reference, counts, &child, depth + 1, rows, missing);
            }
        }
        if children.loading {
            rows.push(loading(depth));
        } else if let Fetch::Failed(error) = &children.state {
            rows.push(ValueRow::Message {
                depth,
                text: error.clone(),
                error: true,
            });
        } else if children.total() > shown {
            rows.push(ValueRow::More {
                reference,
                depth,
                remaining: children.total() - shown,
            });
        }
    }

    /// A click on row `ix` of the call stack: a thread expands or
    /// collapses, a frame is selected.
    pub(crate) fn click_stack_row(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.stack_rows(cx).get(ix).cloned() {
            Some(StackRow::Thread { id, .. }) => self.toggle_thread(id, cx),
            Some(StackRow::More { thread, .. }) => self.fetch_more_frames(thread, None, window, cx),
            Some(StackRow::Frame { thread, index, .. }) => {
                self.select_frame(thread, index, window, cx)
            }
            _ => {}
        }
    }

    /// A click on row `ix` of the Watch (`watch`) or Variables tree: a
    /// node expands or collapses, "Show 100 more" shows more.
    pub(crate) fn click_value_row(&mut self, part: Part, ix: usize, cx: &mut Context<Self>) {
        let rows = match part {
            Part::Watch => self.watch_rows().0,
            _ => self.variable_rows().0,
        };
        match rows.get(ix) {
            Some(row @ ValueRow::More { .. }) => {
                let row = row.clone();
                self.show_more(&row, cx);
            }
            Some(row) if row.expandable() => {
                let row = row.clone();
                self.toggle_node(&row, cx);
            }
            _ => {}
        }
    }

    pub(super) fn render_variables_tab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(empty) = self.variables_empty(cx) {
            return empty;
        }
        let c = cx.theme().colors;
        let stack_len = self.stack_rows(cx).len();
        let values_len = self.variable_rows().0.len();
        let watch = self.render_watch(window, cx);
        let stack = div()
            .id(STACK_ID_ELEMENT)
            .flex_none()
            .w(fe_ui::width::CALL_STACK_DEFAULT)
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(c.border.subtle)
            .child(
                uniform_list(
                    verbatim!("debug-call-stack-rows"),
                    stack_len,
                    cx.processor(|panel, range, window, cx| {
                        panel.render_stack_rows(range, window, cx)
                    }),
                )
                .track_scroll(&self.stack_scroll)
                .flex_1(),
            );
        let stack = fe_ui::node(
            stack,
            window,
            cx,
            Role::Tree,
            tr!("workbench-debug-panel-call-stack"),
        );
        let variables = div()
            .id(VARIABLES_ID_ELEMENT)
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                uniform_list(
                    verbatim!("debug-variable-rows"),
                    values_len,
                    cx.processor(|panel, range, window, cx| {
                        panel.render_value_rows(Part::Variables, range, window, cx)
                    }),
                )
                .track_scroll(&self.values_scroll)
                .flex_1(),
            );
        let variables = fe_ui::node(
            variables,
            window,
            cx,
            Role::Tree,
            tr!("workbench-debug-panel-variables"),
        );
        div()
            .size_full()
            .flex()
            .child(stack)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(watch)
                    .child(variables),
            )
            .into_any_element()
    }

    /// The Watch tree: a heading, its rows and the Add watch input.
    fn render_watch(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let c = cx.theme().colors;
        let count = self.watch_rows().0.len();
        let rows = self.render_value_rows(Part::Watch, 0..count, window, cx);
        // Each child keeps its height: past its cap the tree scrolls, its
        // rows don't shrink to fit.
        let heading = div()
            .flex_none()
            .h(fe_ui::height::ROW)
            .px_2()
            .flex()
            .items_center()
            .text_size(fe_ui::UI_SMALL.size)
            .text_color(c.text.muted)
            .child(tr!("workbench-debug-panel-watch"));
        let tree = div()
            .id(WATCH_ID_ELEMENT)
            .flex_none()
            .max_h(WATCH_MAX_HEIGHT)
            .overflow_y_scroll()
            .track_scroll(&self.watch_scroll)
            .flex()
            .flex_col()
            .border_b_1()
            .border_color(c.border.subtle)
            .child(heading)
            .children(rows)
            .child(
                div()
                    .flex_none()
                    .h(fe_ui::height::ROW)
                    .px_2()
                    .flex()
                    .items_center()
                    .on_action(cx.listener(Self::on_input_command))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|panel, _, _, _| panel.part = Part::WatchInput),
                    )
                    .child(div().flex_1().child(self.watch_input.clone())),
            );
        fe_ui::node(
            tree,
            window,
            cx,
            Role::Tree,
            tr!("workbench-debug-panel-watch-tree"),
        )
        .into_any_element()
    }

    /// A row's ground: the keys' row while the tree has them.
    pub(super) fn ground(
        &self,
        part: Part,
        ix: usize,
        selected: bool,
        window: &Window,
        cx: &App,
    ) -> (Option<Hsla>, Option<Hsla>) {
        let ui = fe_ui::ui_theme(cx);
        let focused = self.focus.contains_focused(window, cx);
        let cursor = focused && self.part == part && self.selected_in(part) == Some(ix);
        let background = selected.then_some(match focused {
            true => ui.row_selected,
            false => ui.row_selected_inactive,
        });
        (background, cursor.then_some(ui.row_focus_border))
    }

    fn render_stack_rows(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let rows = self.stack_rows(cx);
        let c = cx.theme().colors;
        let ui = fe_ui::ui_theme(cx);
        let code = code_family(cx);
        let accent = c.accent.base;
        range
            .filter_map(|ix| {
                let row = rows.get(ix)?.clone();
                let selected = matches!(row, StackRow::Frame { selected: true, .. });
                let (background, cursor) = self.ground(Part::Stack, ix, selected, window, cx);
                let name = row.name();
                let mut element = div()
                    .id(ElementId::NamedInteger(
                        verbatim!("debug-stack-row").into(),
                        ix as u64,
                    ))
                    .h(fe_ui::height::ROW)
                    .w_full()
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_1()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .when_some(background, |row, bg| row.bg(bg))
                    .when_some(cursor, |row, border| row.border_1().border_color(border))
                    .hover(move |row| row.bg(ui.row_hover))
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        panel.select_row(Part::Stack, ix, window, cx);
                        panel.click_stack_row(ix, window, cx);
                    }));
                let states = match &row {
                    StackRow::Thread { expanded, .. } => States {
                        expanded: Some(*expanded),
                        level: Some(1),
                        ..Default::default()
                    },
                    StackRow::Frame { selected, .. } => States {
                        selected: Some(*selected),
                        level: Some(2),
                        ..Default::default()
                    },
                    StackRow::More { .. } | StackRow::Message { .. } => States {
                        level: Some(2),
                        ..Default::default()
                    },
                };
                element = match &row {
                    StackRow::Thread {
                        label, expanded, ..
                    } => element
                        .child(chevron(Some(*expanded), ui.chevron))
                        .child(div().text_color(c.text.muted).child(label.clone())),
                    StackRow::Frame {
                        function,
                        file,
                        faint,
                        current,
                        selected,
                        ..
                    } => {
                        let ink = if *faint { c.text.faint } else { c.text.primary };
                        let arrow = match (current, selected) {
                            (true, _) => Some(verbatim!("▶")),
                            (false, true) => Some(verbatim!("▷")),
                            _ => None,
                        };
                        element
                            .pl(px(20.))
                            .child(
                                div()
                                    .w(px(12.))
                                    .flex_none()
                                    .text_color(accent)
                                    .text_size(px(10.))
                                    .when_some(arrow, |slot, arrow| slot.child(arrow)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .font_family(code.clone())
                                    .text_size(CODE_SMALL.size)
                                    .text_color(ink)
                                    .child(function.clone()),
                            )
                            .when_some(file.clone(), |row, (file, line)| {
                                row.child(
                                    div()
                                        .flex_none()
                                        .font_family(code.clone())
                                        .text_size(CODE_SMALL.size)
                                        .text_color(c.text.faint)
                                        .child(tr!(
                                            "workbench-debug-panel-frame-place",
                                            file = file,
                                            line = line
                                        )),
                                )
                            })
                    }
                    StackRow::More { remaining, .. } => element.pl(px(32.)).child(
                        div()
                            .text_color(c.text.accent)
                            .child(more_frames_label(*remaining)),
                    ),
                    StackRow::Message { text, error } => element.pl(px(20.)).child(
                        div()
                            .text_color(if *error { c.status.error } else { c.text.muted })
                            .child(text.clone()),
                    ),
                };
                Some(
                    fe_ui::node_with(element, window, cx, Role::TreeItem, name, states)
                        .into_any_element(),
                )
            })
            .collect()
    }

    fn render_value_rows(
        &mut self,
        part: Part,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let rows = match part {
            Part::Watch => self.watch_rows().0,
            _ => self.variable_rows().0,
        };
        let c = cx.theme().colors;
        let ui = fe_ui::ui_theme(cx);
        let code = code_family(cx);
        let property = property_color(self, cx).unwrap_or(c.text.accent);
        let id = match part {
            Part::Watch => verbatim!("debug-watch-row"),
            _ => verbatim!("debug-variable-row"),
        };
        range
            .filter_map(|ix| {
                let row = rows.get(ix)?.clone();
                let (_, cursor) = self.ground(part, ix, false, window, cx);
                let depth = row.depth();
                let small = |text: String, color: Hsla| {
                    div()
                        .font_family(code.clone())
                        .text_size(CODE_SMALL.size)
                        .text_color(color)
                        .child(text)
                };
                let mut element = div()
                    .id(ElementId::NamedInteger(id.into(), ix as u64))
                    .flex_none()
                    .h(fe_ui::height::ROW)
                    .w_full()
                    .pl(px(8. + 12. * depth as f32))
                    .pr_2()
                    .flex()
                    .items_center()
                    .gap_1()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .when_some(cursor, |row, border| row.border_1().border_color(border))
                    .hover(move |row| row.bg(ui.row_hover))
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        panel.select_row(part, ix, window, cx);
                        panel.click_value_row(part, ix, cx);
                    }));
                let expandable = row.expandable().then(|| row.expanded());
                let value_parts =
                    |element: gpui::Stateful<gpui::Div>, name: &str, value: &Value| {
                        element
                            .child(small(name.to_owned(), property))
                            .child(small(verbatim!(" = ").to_owned(), c.text.muted))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(small(value.text.clone(), c.text.primary)),
                            )
                            .when_some(value.r#type.clone(), |row, kind| {
                                row.child(small(kind, c.text.faint).flex_none())
                            })
                            .tooltip(tooltip(value.text.clone()))
                    };
                element = match &row {
                    ValueRow::Watch {
                        index,
                        expression,
                        value,
                        ..
                    } => {
                        let element = element.child(chevron(expandable, ui.chevron));
                        let element = match value {
                            Some(Ok(value)) => value_parts(element, expression, value),
                            Some(Err(error)) => {
                                element.child(small(expression.clone(), property)).child(
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
                                )
                            }
                            None => element
                                .child(small(expression.clone(), property))
                                .child(div().flex_1()),
                        };
                        let index = *index;
                        let remove = tr!(
                            "workbench-debug-panel-remove-watch",
                            expression = expression.as_str()
                        );
                        element.child(
                            fe_ui::Button::icon_only(
                                ElementId::NamedInteger(
                                    verbatim!("debug-watch-remove").into(),
                                    index as u64,
                                ),
                                Icon::X,
                            )
                            .name(remove)
                            .small()
                            .ghost()
                            .on_click(
                                cx.listener(move |panel, _, _, cx| panel.remove_watch(index, cx)),
                            ),
                        )
                    }
                    ValueRow::Scope { name, .. } => element
                        .child(chevron(expandable, ui.chevron))
                        .child(div().text_color(c.text.muted).child(name.clone())),
                    ValueRow::Variable { name, value, .. } => {
                        value_parts(element.child(chevron(expandable, ui.chevron)), name, value)
                    }
                    ValueRow::More { .. } => element
                        .child(chevron(None, ui.chevron))
                        .child(div().text_color(c.text.accent).child(more_label())),
                    ValueRow::Message { text, error, .. } => {
                        element.child(chevron(None, ui.chevron)).child(
                            div()
                                .text_color(if *error { c.status.error } else { c.text.muted })
                                .child(text.clone()),
                        )
                    }
                };
                let states = States {
                    expanded: expandable,
                    level: Some(depth + 1),
                    ..Default::default()
                };
                Some(
                    fe_ui::node_with(element, window, cx, Role::TreeItem, row.name(), states)
                        .into_any_element(),
                )
            })
            .collect()
    }
}

const STACK_ID_ELEMENT: &str = super::STACK_ID;

/// The Watch tree's height at most, past which it scrolls.
pub(crate) const WATCH_MAX_HEIGHT: gpui::Pixels = px(160.);
const WATCH_ID_ELEMENT: &str = super::WATCH_ID;
const VARIABLES_ID_ELEMENT: &str = super::VARIABLES_ID;

/// A tree row's chevron: right or down for a parent, room for one else.
pub(super) fn chevron(expanded: Option<bool>, color: Hsla) -> AnyElement {
    let slot = div().w(px(12.)).flex_none();
    match expanded {
        Some(expanded) => {
            let icon = if expanded {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            };
            slot.child(svg().path(icon.path()).size(px(12.)).text_color(color))
                .into_any_element()
        }
        None => slot.into_any_element(),
    }
}

/// The code font, for frames and values.
pub(super) fn code_family(cx: &App) -> SharedString {
    (cx.try_global::<FontFamilies>().cloned().unwrap_or_default()).family(FamilySlot::Code, cx)
}

/// The editor theme's property colour, for variable names.
pub(super) fn property_color(panel: &DebugPanel, cx: &App) -> Option<Hsla> {
    let theme = panel
        .workbench
        .upgrade()?
        .read(cx)
        .pane()
        .read(cx)
        .theme()
        .clone();
    let property = fe_syntax::Highlight::from_capture(verbatim!("property"))?;
    theme.syntax.get(property.index()).copied().flatten()
}

/// A tooltip of `text` alone.
pub(super) fn tooltip(
    text: impl Into<SharedString>,
) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView + 'static {
    let text = text.into();
    move |_, cx| {
        let ui = fe_ui::ui_theme(cx);
        gpui::AppContext::new(cx, |_| fe_ui::Tooltip::new(text.clone(), None::<&str>, &ui)).into()
    }
}
