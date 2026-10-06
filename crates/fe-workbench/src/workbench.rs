//! The window shell: the editor pane, with the dock (the project panel,
//! search or the outline) on the right.

use std::path::{Path, PathBuf};

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_gpui::{EditorSettings, EditorTheme, EditorView, SettingsEvent, commands::RunCommand};
use fe_ui::UiTheme;
use gpui::{
    App, AppContext as _, Context, DismissEvent, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, Styled as _, Subscription,
    Window, div, prelude::FluentBuilder as _, px,
};

use crate::{
    Dock, DockPanel, KEY_CONTEXT, OutlinePanel, Pane, Picker, ProjectPanel, QuickOpen, SearchPanel,
    dock, outline_panel,
    picker::{self, PaletteOp},
    project_panel::PanelEvent,
};

/// Shown in a tab when fastedit starts with nothing to open.
pub const WELCOME: &str = "\
fastedit

Open a folder or a file with:  cargo run -p fastedit -- path

Keys: arrows, alt-arrows (words), cmd-arrows (line/document),
shift to select, double/triple-click, cmd-c/x/v, cmd-z/shift-cmd-z, cmd-s.
Tabs: cmd-shift-[ and cmd-shift-] to switch, cmd-w to close.
";

/// What the workbench opens with.
pub struct Launch {
    /// The project panel's folder.
    pub root: Option<PathBuf>,
    /// A file to open in a tab: its path, and its buffer if it's read by
    /// now. Without the buffer, the tab shows it loading until the host
    /// hands the read to [`Workbench::file_read`].
    pub file: Option<(PathBuf, Option<Buffer>)>,
}

/// The launch for the command-line argument `arg`, made absolute: a
/// directory is the root, with no file; a file, or a missing path (a new
/// file, which saving creates), opens in a tab with its parent directory as
/// the root. No argument is no root, and the welcome text in a tab.
pub fn resolve_launch(arg: Option<PathBuf>) -> Launch {
    let Some(arg) = arg else {
        return Launch {
            root: None,
            file: None,
        };
    };
    let path = std::path::absolute(&arg).unwrap_or(arg);
    if path.is_dir() {
        return Launch {
            root: Some(path),
            file: None,
        };
    }
    Launch {
        root: path.parent().map(Path::to_owned),
        file: Some((path, None)),
    }
}

pub struct Workbench {
    panel: Entity<ProjectPanel>,
    dock: Entity<Dock>,
    pane: Entity<Pane>,
    quick_open: Entity<Picker<QuickOpen>>,
    /// Whether the quick-open palette shows.
    palette: bool,
    ui: UiTheme,
    _subscriptions: [Subscription; 4],
}

impl Workbench {
    /// Opens `launch`. `settings` configure the editors it opens (tab size,
    /// soft wrap); `window` is where file rows open their tabs. Nothing
    /// touches the file system on this thread: the project panel lists
    /// in the background.
    pub fn new(
        launch: Launch,
        theme: EditorTheme,
        settings: EditorSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let ui = UiTheme::default_dark();
        let welcome = launch.root.is_none() && launch.file.is_none();
        let root = launch.root.clone();
        let panel = cx.new(|cx| ProjectPanel::new(launch.root, ui, cx));
        let pane = cx.new(|cx| {
            let mut pane = Pane::new(theme, settings, ui, cx);
            match launch.file {
                None if welcome => pane.add(EditorModel::new(Buffer::new(WELCOME), None), cx),
                None => {}
                Some((path, Some(buffer))) => pane.add(EditorModel::new(buffer, Some(path)), cx),
                Some((path, None)) => pane.opening(path, cx),
            }
            pane
        });
        let dock = cx.new(|cx| {
            let search = cx.new(|cx| SearchPanel::new(root.clone(), pane.downgrade(), ui, cx));
            let outline = cx.new(|_| OutlinePanel::new(ui));
            Dock::new(panel.clone(), search, outline, ui)
        });
        let quick_open = cx.new(|cx| {
            let delegate = QuickOpen::new(root, pane.downgrade(), ui);
            Picker::new(delegate, "Go to file…", ui, cx)
        });
        let open = cx.subscribe_in(
            &panel,
            window,
            |workbench, _, event, window, cx| match event {
                PanelEvent::OpenFile(path) => workbench
                    .pane
                    .update(cx, |pane, cx| pane.open(path.clone(), window, cx)),
                PanelEvent::FsChanged(changes) => workbench
                    .quick_open
                    .update(cx, |picker, cx| picker.delegate.fs_changed(changes, cx)),
            },
        );
        // Hidden once picked from, dismissed or left: focus is elsewhere by
        // then, unless nothing was focused before it opened.
        let dismissed = cx.subscribe_in(
            &quick_open,
            window,
            |workbench, picker, _: &DismissEvent, window, cx| {
                if picker.focus_handle(cx).is_focused(window) {
                    window.focus(&workbench.focus_handle(cx), cx);
                }
                workbench.palette = false;
                cx.notify();
            },
        );
        // Left for another element, or the window deactivated: GPUI blurs
        // then too, focus staying on the input. That's dismissed as Escape
        // would be, so focus doesn't stay on an input no longer drawn.
        let left = cx.on_blur(
            &quick_open.focus_handle(cx),
            window,
            |workbench, window, cx| {
                if !workbench.palette {
                    return;
                }
                let picker = workbench.quick_open.clone();
                if picker.focus_handle(cx).is_focused(window) {
                    picker.update(cx, |picker, cx| picker.dismiss(window, cx));
                }
                workbench.palette = false;
                cx.notify();
            },
        );
        // Retitles the window whenever a tab changes.
        let retitle = cx.observe(&pane, |_, _, cx| cx.notify());
        Self {
            panel,
            dock,
            pane,
            quick_open,
            palette: false,
            ui,
            _subscriptions: [open, retitle, dismissed, left],
        }
    }

    pub fn panel(&self) -> &Entity<ProjectPanel> {
        &self.panel
    }

    /// The right dock, which shows the project panel, search or the outline.
    pub fn dock(&self) -> &Entity<Dock> {
        &self.dock
    }

    pub fn pane(&self) -> &Entity<Pane> {
        &self.pane
    }

    /// The quick-open palette, shown while [`Workbench::palette_open`].
    pub fn quick_open(&self) -> &Entity<Picker<QuickOpen>> {
        &self.quick_open
    }

    pub fn palette_open(&self) -> bool {
        self.palette
    }

    /// Shows the quick-open palette, focused, or dismisses it.
    fn toggle_quick_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let show = !self.palette;
        let previous = window.focused(cx);
        self.quick_open.update(cx, |picker, cx| {
            if show {
                picker.delegate.opened(previous, cx);
                picker.show(window, cx);
            } else {
                picker.dismiss(window, cx);
            }
        });
        self.palette = show;
        cx.notify();
    }

    /// The active tab's editor, unless a file being read or a read error
    /// shows in its place.
    pub fn active_editor<'a>(&self, cx: &'a App) -> Option<&'a Entity<EditorView>> {
        self.pane.read(cx).active_editor()
    }

    /// Tells the outline which editor is active, if that changed. Every tab
    /// change redraws, though not every one notifies (focusing an open tab
    /// only focuses it), so the workbench calls it as it renders. With
    /// splits, it's the active pane's active editor.
    fn follow_active_editor(&mut self, window: &Window, cx: &mut Context<Self>) {
        let editor = self.active_editor(cx).cloned();
        let outline = self.dock.read(cx).outline().clone();
        outline.update(cx, |outline, cx| outline.set_editor(editor.as_ref(), cx));
        // The editor reparses as it renders, which notifies no one.
        cx.defer_in(window, move |_, _, cx| {
            outline.update(cx, OutlinePanel::frame_drawn)
        });
    }

    /// Opens the host's read of the launch file, `Ok(None)` being a new
    /// file; see [`Launch::file`].
    pub fn file_read(
        &mut self,
        path: PathBuf,
        result: std::io::Result<Option<Buffer>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pane
            .update(cx, |pane, cx| pane.file_read(path, result, window, cx));
    }

    /// Applies reloaded settings to the open editors and to editors yet to
    /// open.
    pub fn reload_settings(&mut self, event: &SettingsEvent, cx: &mut Context<Self>) {
        self.pane
            .update(cx, |pane, cx| pane.reload_settings(event, cx));
    }

    /// Runs a `workbench.*` command, a palette toggle or a dock command;
    /// others go on to the app.
    fn on_run_command(&mut self, run: &RunCommand, window: &mut Window, cx: &mut Context<Self>) {
        let Some(registry) =
            crate::registry(cx).filter(|registry| registry.get(&run.0.command).is_some())
        else {
            if picker::op(run, cx) == Some(PaletteOp::ToggleQuickOpen) {
                return self.toggle_quick_open(window, cx);
            }
            let fallback = self.focus_handle(cx);
            if outline_panel::shows(run, cx) {
                // Focus stays where it is, unless on search's hidden input.
                return self.dock.update(cx, |dock, cx| {
                    dock.show(DockPanel::Outline, cx);
                    dock.leave_search(&fallback, window, cx);
                });
            }
            let done = dock::op(run, cx).is_some_and(|op| {
                self.dock
                    .update(cx, |dock, cx| dock.run(op, &fallback, window, cx))
            });
            if !done {
                cx.propagate();
            }
            return;
        };
        let result = self.pane.update(cx, |pane, cx| {
            let result = registry.dispatch(&mut (pane, window, &mut **cx), &run.0);
            cx.notify();
            result
        });
        if let Err(error) = result {
            log::warn!("{error}");
        }
    }
}

impl Focusable for Workbench {
    /// The active editor's, or else the pane's, so the workbench's commands
    /// apply.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self.active_editor(cx) {
            Some(editor) => editor.focus_handle(cx),
            None => self.pane.focus_handle(cx),
        }
    }
}

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_window_title(&self.pane.read(cx).title(cx));
        self.follow_active_editor(window, cx);
        div()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::on_run_command))
            .relative()
            .size_full()
            .flex()
            .bg(self.ui.background)
            .text_color(self.ui.text)
            .text_size(px(12.))
            .child(self.pane.clone())
            .child(self.dock.clone())
            .when(self.palette, |root| root.child(self.quick_open.clone()))
    }
}
