//! The editor pane: a tab per open file, the active one's editor below.

use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
};

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_gpui::{EditorSettings, EditorTheme, EditorView, SettingsEvent};
use fe_ui::{TabBar, UiTheme};
use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, ScrollHandle, Styled as _, Task, Window, div,
};

use crate::loader;

/// An open file.
pub struct Tab {
    /// `None` for a buffer that was never saved, such as the welcome text.
    pub path: Option<PathBuf>,
    pub editor: Entity<EditorView>,
}

pub struct Pane {
    tabs: Vec<Tab>,
    active: usize,
    /// The file opened last, while it's read: shown in the active tab's
    /// place until it lands.
    opening: Option<PathBuf>,
    /// Reads running, so a second click on a file being read doesn't read it
    /// again. Dropping one cancels it.
    reads: HashMap<PathBuf, Task<()>>,
    /// Why the file opened last couldn't be read, shown in the active tab's
    /// place until another tab is shown.
    error: Option<String>,
    theme: EditorTheme,
    /// For editors yet to open.
    settings: EditorSettings,
    ui: UiTheme,
    /// Focused when there's no editor, so the pane's commands still apply.
    focus: FocusHandle,
    tab_scroll: ScrollHandle,
}

impl Pane {
    pub fn new(
        theme: EditorTheme,
        settings: EditorSettings,
        ui: UiTheme,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            tabs: Vec::new(),
            active: 0,
            opening: None,
            reads: HashMap::new(),
            error: None,
            theme,
            settings,
            ui,
            focus: cx.focus_handle(),
            tab_scroll: ScrollHandle::new(),
        }
    }

    /// The theme editors open with.
    pub fn theme(&self) -> &EditorTheme {
        &self.theme
    }

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// The active tab's index; `None` with no tabs.
    pub fn active(&self) -> Option<usize> {
        (self.active < self.tabs.len()).then_some(self.active)
    }

    /// The active tab's editor, unless a file being opened or a read error
    /// shows in its place.
    pub fn active_editor(&self) -> Option<&Entity<EditorView>> {
        if self.opening.is_some() || self.error.is_some() {
            return None;
        }
        self.tabs.get(self.active).map(|tab| &tab.editor)
    }

    /// The window title: the active tab's file name, marked if it has
    /// unsaved changes.
    pub fn title(&self, cx: &App) -> String {
        if let Some(path) = &self.opening {
            return format!("{} — loading", file_name(Some(path)));
        }
        match self.tabs.get(self.active) {
            Some(tab) => {
                let model = tab.editor.read(cx).model();
                let dirty = if model.is_dirty() { " ●" } else { "" };
                format!("{}{dirty} — fastedit", file_name(model.path()))
            }
            None => "fastedit".into(),
        }
    }

    /// Opens `model` in a new tab and shows it, without focusing it.
    pub fn add(&mut self, model: EditorModel, cx: &mut Context<Self>) {
        let editor = self.new_editor(model, cx);
        let path = editor.read(cx).model().path().map(Path::to_owned);
        self.tabs.push(Tab { path, editor });
        self.active = self.tabs.len() - 1;
        self.error = None;
        cx.notify();
    }

    /// Shows the tab for `path` if it's open; otherwise reads the file in the
    /// background and opens it in a new tab, focused, once read. Meanwhile
    /// the pane is focused, as the editor it showed is hidden.
    pub fn open(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.position(&path) {
            self.activate(ix, window, cx);
            return;
        }
        self.opening = Some(path.clone());
        self.error = None;
        window.focus(&self.focus, cx);
        cx.notify();
        if self.reads.contains_key(&path) {
            return;
        }
        let read = cx.background_spawn({
            let path = path.clone();
            async move { loader::read(path) }
        });
        let task = cx.spawn_in(window, {
            let path = path.clone();
            async move |pane, cx| {
                let result = read.await;
                pane.update_in(cx, |pane, window, cx| {
                    pane.file_read(path, result, window, cx)
                })
                .ok();
            }
        });
        self.reads.insert(path, task);
    }

    /// Shows `path` as being read, by the host, which hands the result to
    /// [`Pane::file_read`].
    pub fn opening(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.opening = Some(path);
        cx.notify();
    }

    /// Opens a finished read of `path` (`Ok(None)` is a new file) in a new
    /// tab, unless it's open already. The tab is shown and focused if it's
    /// the file opened last, or if the pane shows nothing else (no tab,
    /// read or error); an error is shown in the tab's place, with the pane
    /// focused, only for the file opened last. So an older read never hides
    /// a newer one or its error, and never shows without focus.
    pub fn file_read(
        &mut self,
        path: PathBuf,
        result: io::Result<Option<Buffer>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reads.remove(&path);
        let last = self.opening.as_ref() == Some(&path);
        if last {
            self.opening = None;
        }
        let empty = self.active().is_none() && self.opening.is_none() && self.error.is_none();
        match result {
            Ok(buffer) => {
                let open = self.position(&path);
                let ix = open.unwrap_or_else(|| {
                    let model =
                        EditorModel::new(buffer.unwrap_or_else(|| Buffer::new("")), Some(path));
                    let editor = self.new_editor(model, cx);
                    self.tabs.push(Tab {
                        path: editor.read(cx).model().path().map(Path::to_owned),
                        editor,
                    });
                    self.tabs.len() - 1
                });
                if last || empty {
                    self.activate(ix, window, cx);
                }
            }
            Err(error) => {
                log::error!("cannot open {}: {error}", path.display());
                if last {
                    self.error = Some(format!("Cannot open {}: {error}", path.display()));
                    window.focus(&self.focus, cx);
                }
            }
        }
        cx.notify();
    }

    /// Shows tab `ix` and focuses its editor. A file still being read then
    /// opens in a tab without being shown.
    pub fn activate(&mut self, ix: usize, window: &mut Window, cx: &mut App) {
        let Some(tab) = self.tabs.get(ix) else {
            return;
        };
        self.active = ix;
        self.opening = None;
        self.error = None;
        window.focus(&tab.editor.focus_handle(cx), cx);
    }

    /// Activates the next tab, or with `back` the previous one, wrapping.
    pub fn cycle(&mut self, back: bool, window: &mut Window, cx: &mut App) {
        let count = self.tabs.len();
        if count > 1 {
            let ix = if back {
                self.active + count - 1
            } else {
                self.active + 1
            } % count;
            self.activate(ix, window, cx);
        }
    }

    /// Closes the active tab and activates its right neighbour, or the left
    /// one if it was last. A tab with unsaved changes stays open (until
    /// there are save prompts).
    pub fn close_active(&mut self, window: &mut Window, cx: &mut App) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let model = tab.editor.read(cx).model();
        if model.is_dirty() {
            log::warn!(
                "not closing {}: it has unsaved changes",
                file_name(model.path())
            );
            return;
        }
        self.tabs.remove(self.active);
        self.error = None;
        if self.tabs.is_empty() {
            self.active = 0;
            window.focus(&self.focus, cx);
        } else {
            self.activate(self.active.min(self.tabs.len() - 1), window, cx);
        }
    }

    /// Applies reloaded settings to the open editors and to editors yet to
    /// open.
    pub fn reload_settings(&mut self, event: &SettingsEvent, cx: &mut Context<Self>) {
        self.settings = event.settings.editor.clone();
        self.theme.set_metrics(&self.settings, cx);
        for tab in &self.tabs {
            tab.editor
                .update(cx, |editor, cx| editor.reload_settings(event, cx));
        }
        cx.notify();
    }

    fn position(&self, path: &Path) -> Option<usize> {
        self.tabs
            .iter()
            .position(|tab| tab.path.as_deref() == Some(path))
    }

    fn new_editor(&self, mut model: EditorModel, cx: &mut Context<Self>) -> Entity<EditorView> {
        if model.tab_size() != self.settings.tab_size {
            model.set_tab_size(self.settings.tab_size);
        }
        if model.soft_wrap() != self.settings.soft_wrap {
            model.set_soft_wrap(self.settings.soft_wrap);
        }
        let theme = self.theme.clone();
        let editor = cx.new(|cx| EditorView::new(model, theme, cx));
        // Re-render (and retitle) whenever the editor changes.
        cx.observe(&editor, |_, _, cx| cx.notify()).detach();
        editor
    }

    fn message(&self, text: String) -> gpui::AnyElement {
        div()
            .size_full()
            .px_3()
            .py_2()
            .text_color(self.ui.muted_text)
            .child(text)
            .into_any_element()
    }
}

pub(crate) fn file_name(path: Option<&Path>) -> String {
    path.and_then(Path::file_name)
        .map_or("untitled".into(), |name| {
            name.to_string_lossy().into_owned()
        })
}

#[cfg(test)]
impl Pane {
    /// Where tab `ix` was laid out in the last frame.
    pub(crate) fn tab_bounds(&self, ix: usize) -> Option<gpui::Bounds<gpui::Pixels>> {
        self.tab_scroll.bounds_for_item(ix)
    }
}

impl Focusable for Pane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Pane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs = self.tabs.iter().enumerate().map(|(ix, tab)| {
            let dirty = tab.editor.read(cx).model().is_dirty();
            fe_ui::Tab::new(("tab", ix), file_name(tab.path.as_deref()), &self.ui)
                .dirty(dirty)
                .active(ix == self.active)
                .on_click(cx.listener(move |pane, _, window, cx| {
                    pane.activate(ix, window, cx);
                    cx.notify();
                }))
                .on_close(cx.listener(move |pane, _, window, cx| {
                    pane.activate(ix, window, cx);
                    pane.close_active(window, cx);
                    cx.notify();
                }))
        });
        let tab_bar = TabBar::new("tabs", &self.ui)
            .track_scroll(&self.tab_scroll)
            .children(tabs);
        let body = if let Some(path) = &self.opening {
            self.message(format!("Loading {}…", file_name(Some(path))))
        } else if let Some(error) = &self.error {
            self.message(error.clone())
        } else if let Some(tab) = self.tabs.get(self.active) {
            tab.editor.clone().into_any_element()
        } else {
            self.message("No open files".into())
        };
        div()
            .track_focus(&self.focus)
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(self.ui.background)
            .child(tab_bar)
            .child(div().flex_1().min_h_0().child(body))
    }
}
