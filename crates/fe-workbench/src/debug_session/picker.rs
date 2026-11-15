//! `debug.start`'s picker over a root's launch configurations
//! ([`crate::launch_defs`]): the launch file's, then `launch.json`'s, then
//! the detected ones, each with where it's from and, for one that can't
//! start, why; below them what was wrong with the launch files. It lists
//! them whether or not the folder is trusted: picking one is where trust
//! is asked for ([`Workbench::start_debug_configuration`]).

use std::{ops::Range, path::PathBuf};

use fe_ui::{ActiveTheme as _, HighlightedLabel, char_ranges};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, IntoElement as _,
    ParentElement as _, SharedString, Styled as _, WeakEntity, Window, div,
};
use kiln_l10n::{text, tr};
use kiln_shell::{ModalId, picker::joined_label};

use super::Listing;
use crate::{
    Workbench,
    launch_defs::{LAUNCH_FILE, LAUNCH_JSON, LaunchConfig, Source},
    picker::{Picker, PickerDelegate},
    toolbar_picker::subsequence,
};

/// The picker's name in the modal layer.
pub(crate) const DEBUG_PICKER: ModalId = ModalId("debug");

/// A row of the picker. Only a configuration that can start is picked.
enum Row {
    Config {
        config: Box<LaunchConfig>,
        detail: String,
    },
    /// There's nothing to debug: always shown, whatever the query.
    Empty(String),
    /// A problem with a launch file.
    Warning(String),
}

impl Row {
    /// The byte ranges of the label that `query`, in lowercase chars,
    /// matches: `None` if the row doesn't match.
    fn matched(&self, query: &[char]) -> Option<Vec<Range<usize>>> {
        match self {
            Self::Config { config, detail } => match subsequence(query, &config.name) {
                Some(indices) => Some(char_ranges(&config.name, &indices)),
                None => subsequence(query, detail).map(|_| Vec::new()),
            },
            Self::Empty(_) => Some(Vec::new()),
            Self::Warning(text) => subsequence(query, text).map(|_| Vec::new()),
        }
    }
}

/// Where a configuration came from, in its row.
fn source_name(source: Source) -> String {
    match source {
        Source::File => LAUNCH_FILE.to_owned(),
        Source::LaunchJson => LAUNCH_JSON.to_owned(),
        Source::Detected => tr!("workbench-debug-picker-detected").into(),
    }
}

/// What a configuration's row shows after its name: its source, and why it
/// can't start if it can't.
fn detail(config: &LaunchConfig) -> String {
    let source = source_name(config.source);
    match config.blockers.first() {
        None => source,
        Some(_) => tr!(
            "workbench-debug-picker-blocked",
            source = source,
            why = config.blockers.join("; ")
        )
        .into(),
    }
}

/// The picker over a root's launch configurations.
pub(crate) struct ConfigPicker {
    /// The root it opened on, whose configurations it lists.
    root: Option<PathBuf>,
    workbench: WeakEntity<Workbench>,
    /// What was focused when it opened.
    previous_focus: Option<FocusHandle>,
    rows: Vec<Row>,
    /// The project's tasks, which a pre-launch task names.
    tasks: Vec<crate::task_defs::Task>,
    matches: Vec<(usize, Vec<Range<usize>>)>,
    query: String,
    loaded: bool,
    _reading: Option<gpui::Task<()>>,
}

impl ConfigPicker {
    /// Shows the list once `reading` has it.
    fn read(&mut self, reading: gpui::Task<Listing>, cx: &mut Context<Picker<Self>>) {
        self._reading = Some(cx.spawn(async move |picker, cx| {
            let listing = reading.await;
            let shown = picker.update(cx, |picker, cx| picker.delegate.set_listing(listing, cx));
            shown.ok();
        }));
    }

    fn set_listing(&mut self, listing: Listing, cx: &mut Context<Picker<Self>>) {
        let Listing {
            configs,
            diagnostics,
            tasks,
        } = listing;
        let mut rows: Vec<Row> = (configs.into_iter())
            .map(|config| Row::Config {
                detail: detail(&config),
                config: Box::new(config),
            })
            .collect();
        if rows.is_empty() {
            rows.push(Row::Empty(
                tr!("workbench-debug-picker-none", file = LAUNCH_FILE).into(),
            ));
        }
        rows.extend(
            (diagnostics.iter()).map(|diagnostic| Row::Warning(one_line(&diagnostic.message))),
        );
        self.rows = rows;
        self.tasks = tasks;
        self.loaded = true;
        self.refilter(cx);
    }

    fn refilter(&mut self, cx: &mut Context<Picker<Self>>) {
        let query: Vec<char> = self.query.chars().flat_map(char::to_lowercase).collect();
        self.matches = (self.rows.iter().enumerate())
            .filter_map(|(ix, row)| Some((ix, row.matched(&query)?)))
            .collect();
        cx.notify();
        let picker = cx.entity();
        // `Picker::matches_updated` borrows the picker, which is being
        // updated now.
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }

    fn restore_focus(&mut self, window: &mut Window, cx: &mut App) {
        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }
    }
}

/// `text` on one line.
fn one_line(text: &str) -> String {
    let lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    lines.collect::<Vec<_>>().join(" ")
}

/// A row as the tests read it.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ShownRow {
    Config(String, String),
    Empty(String),
    Warning(String),
}

#[cfg(test)]
impl ConfigPicker {
    /// The rows matching the query.
    pub(crate) fn shown(&self) -> Vec<ShownRow> {
        (self.matches.iter())
            .map(|(row, _)| match &self.rows[*row] {
                Row::Config { config, detail } => {
                    ShownRow::Config(config.name.clone(), detail.clone())
                }
                Row::Empty(text) => ShownRow::Empty(text.clone()),
                Row::Warning(text) => ShownRow::Warning(text.clone()),
            })
            .collect()
    }

    pub(crate) fn loaded(&self) -> bool {
        self.loaded
    }
}

impl PickerDelegate for ConfigPicker {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        self.query = query;
        self.refilter(cx);
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn match_id(&self, ix: usize) -> SharedString {
        format!("debug-row-{}", self.matches[ix].0).into()
    }

    fn render_match(&self, ix: usize, _: bool, cx: &App) -> AnyElement {
        let ui = fe_ui::ui_theme(cx);
        let (row, highlights) = &self.matches[ix];
        let line = div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap_2()
            .overflow_hidden()
            .whitespace_nowrap();
        match &self.rows[*row] {
            Row::Config { config, detail } => {
                let label = HighlightedLabel::new(config.name.clone(), highlights.clone(), &ui);
                let line = if config.can_start() {
                    line
                } else {
                    line.text_color(ui.muted_text)
                };
                line.child(div().flex_none().child(label)).child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(ui.muted_text)
                        .child(detail.clone()),
                )
            }
            Row::Empty(text) => line.text_color(ui.muted_text).child(text.clone()),
            Row::Warning(text) => (line.text_color(cx.theme().colors.status.warning))
                .child(div().min_w_0().truncate().child(text.clone())),
        }
        .into_any_element()
    }

    fn match_label(&self, ix: usize, _: &App) -> SharedString {
        match &self.rows[self.matches[ix].0] {
            Row::Config { config, detail } => joined_label([config.name.as_str(), detail.as_str()]),
            Row::Empty(text) | Row::Warning(text) => text.clone().into(),
        }
    }

    /// Starts a configuration that can start, focus going back where it
    /// was first, unless the window has changed roots since. Any other row
    /// leaves the picker open.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let config = match &self.rows[self.matches[ix].0] {
            Row::Config { config, .. } if config.can_start() => (**config).clone(),
            _ => return false,
        };
        self.restore_focus(window, cx);
        if let Some(workbench) = self.workbench.upgrade()
            && workbench.read(cx).root() == self.root.as_deref()
        {
            let tasks = self.tasks.clone();
            workbench.update(cx, |workbench, cx| {
                workbench.start_debug_configuration(config, tasks, window, cx)
            });
        }
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.restore_focus(window, cx);
    }

    fn rows_fill_width(&self) -> bool {
        true
    }

    fn status(&self) -> Option<SharedString> {
        (!self.loaded).then(|| tr!("workbench-debug-picker-reading"))
    }
}

impl Workbench {
    /// Shows the configuration picker, focused, in place of any other modal;
    /// its list follows once `reading` has it.
    pub(crate) fn show_debug_picker(
        &mut self,
        reading: gpui::Task<Listing>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Picker<ConfigPicker>> {
        let modals = self.shell().read(cx).modals().clone();
        modals.update(cx, |layer, cx| layer.dismiss_modal(window, cx));
        let delegate = ConfigPicker {
            root: self.root().map(ToOwned::to_owned),
            workbench: cx.entity().downgrade(),
            previous_focus: window.focused(cx),
            rows: Vec::new(),
            tasks: Vec::new(),
            matches: Vec::new(),
            query: String::new(),
            loaded: false,
            _reading: None,
        };
        let placeholder = text!("workbench-debug-picker-placeholder");
        let picker = cx.new(|cx| Picker::new(delegate, placeholder, fe_ui::ui_theme(cx), cx));
        picker.update(cx, |picker, cx| {
            picker.show(window, cx);
            picker.delegate.read(reading, cx);
        });
        (self.shell().clone()).update(cx, |shell, cx| {
            shell.show_modal(DEBUG_PICKER, picker.clone(), window, cx);
        });
        picker
    }
}

#[cfg(test)]
impl Workbench {
    /// The configuration picker in the modal layer, if it's the modal
    /// showing.
    pub(crate) fn debug_picker(&self, cx: &App) -> Option<Entity<Picker<ConfigPicker>>> {
        let layer = self.shell().read(cx).modals().read(cx);
        layer.modal().filter(|id| *id == DEBUG_PICKER)?;
        layer.modal_view()?.clone().downcast().ok()
    }
}
