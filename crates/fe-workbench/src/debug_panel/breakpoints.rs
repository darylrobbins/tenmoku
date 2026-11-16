//! The Breakpoints tab: the adapter's exception filters as checkboxes,
//! then every breakpoint grouped by file as Problems groups them (file
//! icon, path, count), each a checkbox that enables it, its glyph, `line
//! N` and the line's code (`docs/design/design-system.md`, "The Debug tool
//! window").

use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_buffer::BufferId;
use fe_ui::{
    ActiveTheme as _, CODE_SMALL, Checkbox, Role, States, UI_SMALL, file_icon_ref, radius,
};
use gpui::{
    AnyElement, App, AppContext as _, Context, ElementId, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window, div, prelude::FluentBuilder as _, px, uniform_list,
};
use kiln_l10n::{tr, verbatim};

use super::{BREAKPOINTS_ID, DebugPanel, Part, rows::code_family, rows::tooltip};

/// The breakpoint lines' code of files not open, read from disk, until the
/// store next changes.
#[derive(Default)]
pub(crate) struct CodeCache {
    files: HashMap<PathBuf, Option<Rc<Vec<String>>>>,
    reading: HashSet<PathBuf>,
    /// Counts the clears: a read started before the last one lands nothing.
    generation: u64,
}

impl CodeCache {
    pub(crate) fn clear(&mut self) {
        self.files.clear();
        self.reading.clear();
        self.generation += 1;
    }
}

/// Holds each read of a file's code until the test sends.
#[cfg(test)]
pub(crate) struct CodeReadGate(pub(crate) async_channel::Receiver<()>);

#[cfg(test)]
impl gpui::Global for CodeReadGate {}

/// A row of the Breakpoints tab.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum BreakpointRow {
    /// "Exception filters", over them.
    FiltersHeading,
    Filter {
        filter: String,
        label: String,
        description: Option<String>,
        on: bool,
    },
    /// No session has said which filters it offers.
    NoFilters,
    File {
        path: PathBuf,
        /// As shown: relative to the root.
        shown: String,
        count: usize,
    },
    Breakpoint {
        path: PathBuf,
        /// The file's first editor's buffer in this window, if it's open.
        buffer: Option<BufferId>,
        /// In that buffer, else on disk, from 0.
        row: u32,
        enabled: bool,
        /// What the session said of it: `Some(false)` until bound.
        verified: Option<bool>,
        message: Option<String>,
        code: Option<String>,
    },
    /// No breakpoints.
    Empty,
}

impl BreakpointRow {
    /// What a screen reader names it.
    pub(crate) fn name(&self) -> SharedString {
        match self {
            BreakpointRow::FiltersHeading => tr!("workbench-debug-panel-exception-filters"),
            BreakpointRow::Filter { label, .. } => label.clone().into(),
            BreakpointRow::NoFilters => tr!("workbench-debug-panel-no-filters"),
            BreakpointRow::File { shown, count, .. } => tr!(
                "workbench-debug-panel-file-name",
                path = shown.as_str(),
                count = *count
            ),
            BreakpointRow::Breakpoint { path, row, .. } => breakpoint_name(path, *row),
            BreakpointRow::Empty => tr!("workbench-debug-panel-no-breakpoints"),
        }
    }
}

/// A breakpoint's checkbox's name: "Breakpoint, minimap.rs line 42".
fn breakpoint_name(path: &Path, row: u32) -> SharedString {
    let file =
        (path.file_name()).map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    tr!(
        "workbench-debug-panel-breakpoint-name",
        file = file,
        line = row + 1
    )
}

/// The open editors' buffers in this window, by canonical file, with their
/// text: the first of each file's.
fn open_buffers(panel: &DebugPanel, cx: &App) -> HashMap<PathBuf, (BufferId, fe_text::Text)> {
    let mut open = HashMap::new();
    let Some(workbench) = panel.workbench.upgrade() else {
        return open;
    };
    for editor in workbench.read(cx).file_editors(cx) {
        let model = editor.read(cx).model();
        let Some(path) = model.path() else {
            continue;
        };
        let path = dunce::canonicalize(path).unwrap_or_else(|_| path.to_owned());
        let snapshot = model.snapshot();
        open.entry(path)
            .or_insert_with(|| (snapshot.id(), snapshot.text().clone()));
    }
    open
}

impl DebugPanel {
    /// The Breakpoints tab's rows.
    pub(crate) fn breakpoint_rows(&self, cx: &App) -> Vec<BreakpointRow> {
        let mut rows = vec![BreakpointRow::FiltersHeading];
        let (filters, verification_of) = match self.debugger() {
            Some(debugger) => {
                let debugger = debugger.read(cx);
                (
                    debugger.exception_filters(),
                    debugger.is_active().then_some(debugger),
                )
            }
            None => (Vec::new(), None),
        };
        if filters.is_empty() {
            rows.push(BreakpointRow::NoFilters);
        }
        for (filter, on) in filters {
            rows.push(BreakpointRow::Filter {
                filter: filter.filter,
                label: filter.label,
                description: filter.description.filter(|text| !text.is_empty()),
                on,
            });
        }
        let Some(store) = &self.store else {
            rows.push(BreakpointRow::Empty);
            return rows;
        };
        let store = store.read(cx);
        let open = open_buffers(self, cx);
        let mut any = false;
        for path in store.files() {
            let (buffer, text) = match open.get(&path) {
                Some((buffer, text)) => (Some(*buffer), Some(text)),
                None => (None, None),
            };
            let placements: Vec<(u32, u32, bool)> = match buffer {
                Some(buffer) => (store.placements_in(&path, buffer).into_iter())
                    .map(|placement| (placement.row, placement.line, placement.enabled))
                    .collect(),
                None => (store.on_disk(&path).into_iter())
                    .map(|breakpoint| (breakpoint.line - 1, breakpoint.line, breakpoint.enabled))
                    .collect(),
            };
            if placements.is_empty() {
                continue;
            }
            any = true;
            let shown = path.strip_prefix(store.root()).unwrap_or(&path);
            rows.push(BreakpointRow::File {
                path: path.clone(),
                shown: shown.to_string_lossy().into_owned(),
                count: placements.len(),
            });
            let said = verification_of.map_or(&[][..], |debugger| debugger.verification(&path));
            let disk = self.code.files.get(&path).cloned().flatten();
            for (row, line, enabled) in placements {
                let said = said.iter().find(|said| enabled && said.line == line);
                let code = match (text, &disk) {
                    (Some(text), _) if row < text.row_count() => {
                        let start = text.row_start(row);
                        let end = start + text.row_len(row) as usize;
                        Some(text.slice_to_string(start..end))
                    }
                    (None, Some(lines)) => lines.get(row as usize).cloned(),
                    _ => None,
                };
                rows.push(BreakpointRow::Breakpoint {
                    path: path.clone(),
                    buffer,
                    row,
                    enabled,
                    verified: said.map(|said| said.verified),
                    message: said.and_then(|said| said.message.clone()),
                    code: code.map(|code| code.trim().to_owned()),
                });
            }
        }
        if !any {
            rows.push(BreakpointRow::Empty);
        }
        rows
    }

    /// Reads the code of the files with breakpoints that aren't open, off
    /// the main thread, once until the store changes.
    fn read_code(&mut self, cx: &mut Context<Self>) {
        let Some(store) = &self.store else {
            return;
        };
        let open = open_buffers(self, cx);
        let wanted: Vec<PathBuf> = (store.read(cx).files().into_iter())
            .filter(|path| !open.contains_key(path))
            .filter(|path| !self.code.files.contains_key(path) && !self.code.reading.contains(path))
            .collect();
        let generation = self.code.generation;
        for path in wanted {
            self.code.reading.insert(path.clone());
            #[cfg(test)]
            let gate = cx.try_global::<CodeReadGate>().map(|gate| gate.0.clone());
            let read = cx.background_spawn({
                let path = path.clone();
                async move {
                    #[cfg(test)]
                    if let Some(gate) = gate {
                        gate.recv().await.ok();
                    }
                    let text = std::fs::read_to_string(&path).ok()?;
                    Some(text.lines().map(str::to_owned).collect::<Vec<_>>())
                }
            });
            cx.spawn(async move |panel, cx| {
                let lines = read.await.map(Rc::new);
                panel
                    .update(cx, |panel, cx| {
                        // Read before the store last changed: the file may
                        // have since.
                        if panel.code.generation == generation && panel.code.reading.remove(&path) {
                            panel.code.files.insert(path, lines);
                            cx.notify();
                        }
                    })
                    .ok();
            })
            .detach();
        }
    }

    /// Turns exception filter `filter` on or off, as its checkbox does.
    pub(crate) fn set_exception_filter(&mut self, filter: &str, on: bool, cx: &mut Context<Self>) {
        let Some(debugger) = self.debugger() else {
            return;
        };
        debugger.update(cx, |debugger, cx| {
            debugger.set_exception_filter(filter, on, cx)
        });
        cx.notify();
    }

    /// Space on row `ix`: its checkbox toggles.
    pub(crate) fn toggle_breakpoint_row(&mut self, ix: usize, cx: &mut Context<Self>) -> bool {
        match self.breakpoint_rows(cx).get(ix).cloned() {
            Some(BreakpointRow::Filter { filter, on, .. }) => {
                self.set_exception_filter(&filter, !on, cx);
                true
            }
            Some(BreakpointRow::Breakpoint {
                path,
                buffer,
                row,
                enabled,
                ..
            }) => {
                self.set_breakpoint_enabled(&path, buffer, row, !enabled, cx);
                true
            }
            _ => false,
        }
    }

    fn set_breakpoint_enabled(
        &mut self,
        path: &Path,
        buffer: Option<BufferId>,
        row: u32,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(store) = self.store.clone() else {
            return;
        };
        store.update(cx, |store, cx| match buffer {
            Some(buffer) => store.set_enabled_in(path, buffer, row, enabled, cx),
            None => store.set_enabled(path, row, enabled, cx),
        });
    }

    /// A click on, or enter on, row `ix`: a breakpoint's line opens.
    pub(crate) fn click_breakpoint_row(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(BreakpointRow::Breakpoint { path, row, .. }) =
            self.breakpoint_rows(cx).get(ix).cloned()
        else {
            return;
        };
        self.with_workbench(window, cx, move |workbench, window, cx| {
            workbench
                .open_document_at(path, Some(row + 1), None, window, cx)
                .detach();
        });
    }

    pub(super) fn render_breakpoints_tab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.read_code(cx);
        let count = self.breakpoint_rows(cx).len();
        let list = div()
            .id(BREAKPOINTS_ID)
            .size_full()
            .flex()
            .flex_col()
            .child(
                uniform_list(
                    verbatim!("debug-breakpoint-rows"),
                    count,
                    cx.processor(|panel, range, window, cx| {
                        panel.render_breakpoint_rows(range, window, cx)
                    }),
                )
                .track_scroll(&self.breakpoints_scroll)
                .flex_1(),
            );
        fe_ui::node(
            list,
            window,
            cx,
            Role::Tree,
            tr!("workbench-debug-panel-breakpoints"),
        )
        .into_any_element()
    }

    fn render_breakpoint_rows(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let rows = self.breakpoint_rows(cx);
        let c = cx.theme().colors;
        let ui = fe_ui::ui_theme(cx);
        let code = code_family(cx);
        let theme = (self.workbench.upgrade())
            .map(|workbench| workbench.read(cx).pane().read(cx).theme().clone());
        let (red, grey) = match &theme {
            Some(theme) => (theme.debug_breakpoint, theme.debug_breakpoint_disabled),
            None => (c.status.error, c.text.faint),
        };
        let focused = self.focus.contains_focused(window, cx);
        range
            .filter_map(|ix| {
                let row = rows.get(ix)?.clone();
                let cursor = focused
                    && self.part == Part::Breakpoints
                    && self.selected_in(Part::Breakpoints) == Some(ix);
                let name = row.name();
                let element = div()
                    .id(ElementId::NamedInteger(
                        verbatim!("debug-breakpoint-row").into(),
                        ix as u64,
                    ))
                    .h(fe_ui::height::ROW)
                    .w_full()
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .when(cursor, |row| {
                        row.border_1().border_color(ui.row_focus_border)
                    });
                let element = match &row {
                    BreakpointRow::FiltersHeading => element
                        .text_size(UI_SMALL.size)
                        .text_color(c.text.muted)
                        .child(name.clone()),
                    BreakpointRow::NoFilters | BreakpointRow::Empty => {
                        element.pl_5().text_color(c.text.muted).child(name.clone())
                    }
                    BreakpointRow::Filter {
                        filter,
                        label,
                        description,
                        on,
                    } => {
                        let filter = filter.clone();
                        let check = Checkbox::new(ElementId::NamedInteger(
                            verbatim!("debug-exception-filter").into(),
                            ix as u64,
                        ))
                        .checked(*on)
                        .label(label.clone())
                        .on_change({
                            let panel = cx.weak_entity();
                            move |on, _, cx| {
                                let filter = filter.clone();
                                panel
                                    .update(cx, |panel, cx| {
                                        panel.set_exception_filter(&filter, *on, cx)
                                    })
                                    .ok();
                            }
                        });
                        element
                            .pl_5()
                            .on_click(cx.listener(move |panel, _, window, cx| {
                                panel.select_row(Part::Breakpoints, ix, window, cx)
                            }))
                            .child(check)
                            .when_some(description.clone(), |row, text| row.tooltip(tooltip(text)))
                    }
                    BreakpointRow::File { path, shown, count } => {
                        let file_name = (path.file_name())
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let icon = file_icon_ref(&file_name);
                        element
                            .child(
                                div()
                                    .size(px(14.))
                                    .flex_none()
                                    .child(icon.element(px(14.), ui.icon)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .text_color(c.text.muted)
                                    .child(shown.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .px(px(6.))
                                    .rounded(radius::CONTROL)
                                    .bg(ui.row_hover)
                                    .text_color(c.text.muted)
                                    .text_size(UI_SMALL.size)
                                    .line_height(UI_SMALL.line_height)
                                    .child(count.to_string()),
                            )
                    }
                    BreakpointRow::Breakpoint {
                        path,
                        buffer,
                        row,
                        enabled,
                        verified,
                        message,
                        code: line_code,
                    } => {
                        let (path, buffer, row_ix, enabled) =
                            (path.clone(), *buffer, *row, *enabled);
                        let check = Checkbox::new(ElementId::NamedInteger(
                            verbatim!("debug-breakpoint-check").into(),
                            ix as u64,
                        ))
                        .checked(enabled)
                        .name(breakpoint_name(&path, row_ix))
                        .on_change({
                            let panel = cx.weak_entity();
                            let path = path.clone();
                            move |on, _, cx| {
                                let path = path.clone();
                                panel
                                    .update(cx, |panel, cx| {
                                        panel.set_breakpoint_enabled(&path, buffer, row_ix, *on, cx)
                                    })
                                    .ok();
                            }
                        });
                        let glyph = glyph(enabled, *verified == Some(false), red, grey);
                        element
                            .pl_5()
                            .hover(move |row| row.bg(ui.row_hover))
                            .on_click(cx.listener(move |panel, _, window, cx| {
                                panel.select_row(Part::Breakpoints, ix, window, cx);
                                panel.click_breakpoint_row(ix, window, cx);
                            }))
                            .child(check)
                            .child(glyph)
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(c.text.primary)
                                    .child(tr!("workbench-debug-panel-line", line = row_ix + 1)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .font_family(code.clone())
                                    .text_size(CODE_SMALL.size)
                                    .text_color(c.text.faint)
                                    .when_some(line_code.clone(), |code, text| code.child(text)),
                            )
                            .when_some(message.clone(), |row, message| {
                                row.tooltip(tooltip(message))
                            })
                    }
                };
                let states = States {
                    level: Some(match row {
                        BreakpointRow::Breakpoint { .. } | BreakpointRow::Filter { .. } => 2,
                        _ => 1,
                    }),
                    ..Default::default()
                };
                Some(
                    fe_ui::node_with(element, window, cx, Role::TreeItem, name, states)
                        .into_any_element(),
                )
            })
            .collect()
    }
}

/// A breakpoint's glyph in its row: a filled dot, a ring until the
/// session binds it, a grey dot while disabled, at `glyph::BREAKPOINT`.
fn glyph(enabled: bool, unbound: bool, red: Hsla, grey: Hsla) -> AnyElement {
    let size = fe_ui::glyph::BREAKPOINT;
    let dot = div().flex_none().size(size).rounded_full();
    match (enabled, unbound) {
        (false, _) => dot.bg(grey),
        (true, true) => dot.border(fe_ui::glyph::STROKE).border_color(red),
        (true, false) => dot.bg(red),
    }
    .into_any_element()
}
