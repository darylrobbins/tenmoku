//! The window shell: the editor pane, with the project panel docked right.

use std::path::{Path, PathBuf};

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_gpui::{EditorSettings, EditorTheme, EditorView, SettingsEvent, commands::RunCommand};
use fe_ui::UiTheme;
use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, Styled as _, Subscription, Window, div, px,
};

use crate::{KEY_CONTEXT, Pane, ProjectPanel, project_panel::PanelEvent};

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
    pane: Entity<Pane>,
    ui: UiTheme,
    _subscriptions: [Subscription; 2],
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
        let open = cx.subscribe_in(&panel, window, |workbench, _, event, window, cx| {
            let PanelEvent::OpenFile(path) = event;
            let path = path.clone();
            workbench
                .pane
                .update(cx, |pane, cx| pane.open(path, window, cx));
        });
        // Retitles the window whenever a tab changes.
        let retitle = cx.observe(&pane, |_, _, cx| cx.notify());
        Self {
            panel,
            pane,
            ui,
            _subscriptions: [open, retitle],
        }
    }

    pub fn panel(&self) -> &Entity<ProjectPanel> {
        &self.panel
    }

    pub fn pane(&self) -> &Entity<Pane> {
        &self.pane
    }

    /// The active tab's editor, unless a file being read or a read error
    /// shows in its place.
    pub fn active_editor<'a>(&self, cx: &'a App) -> Option<&'a Entity<EditorView>> {
        self.pane.read(cx).active_editor()
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

    /// Runs a `workbench.*` command; others go on to the app.
    fn on_run_command(&mut self, run: &RunCommand, window: &mut Window, cx: &mut Context<Self>) {
        let Some(registry) =
            crate::registry(cx).filter(|registry| registry.get(&run.0.command).is_some())
        else {
            cx.propagate();
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
        div()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::on_run_command))
            .size_full()
            .flex()
            .bg(self.ui.background)
            .text_color(self.ui.text)
            .text_size(px(12.))
            .child(self.pane.clone())
            .child(self.panel.clone())
    }
}
