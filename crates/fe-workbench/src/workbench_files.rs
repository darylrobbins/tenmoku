//! Opening and saving files: the open panel, new files, Save As, files
//! opened from the project panel, and editors following files the panel
//! renames or moves, with the free helpers that match their paths.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use fe_buffer::Buffer;
use fe_editor::{EditorModel, save_target};
use fe_gpui::EditorView;
use gpui::{
    App, AppContext as _, Context, Entity, EntityId, Focusable, Global, PromptButton, PromptLevel,
    Window,
};

#[cfg(test)]
use super::OPEN_PANEL_FAILS;
use super::toast::Toast;
use super::{Workbench, open_panel_failed, toast, vim_host};
use crate::{
    Pane,
    loader::Opened,
    open::{self, OpenOp},
    project_panel, shortcuts_editor,
};

impl Workbench {
    /// Shows the open panel for `op`, then opens what's picked: a folder
    /// re-roots the workbench, and a file opens in the active pane as a
    /// tree click does, the root staying. Cancelling does nothing, and an
    /// error from the panel is logged and shown in an error toast
    /// ([`open_panel_failed`]). Asked again while the panel shows, it does
    /// nothing.
    pub(crate) fn prompt_open(&mut self, op: OpenOp, window: &mut Window, cx: &mut Context<Self>) {
        if op == OpenOp::Settings {
            return self.open_settings(window, cx);
        }
        if op == OpenOp::NewWindow {
            return open::open_window(self.root.clone(), cx);
        }
        if op == OpenOp::NewFile {
            return self.new_file(window, cx);
        }
        if op == OpenOp::SaveAs {
            return self.prompt_save_as(window, cx);
        }
        if self.prompting {
            return;
        }
        self.prompting = true;
        let answer = cx.prompt_for_paths(op.prompt());
        #[cfg(test)]
        let answer = match OPEN_PANEL_FAILS.take() {
            Some(error) => {
                let (failed, answer) = futures::channel::oneshot::channel();
                failed.send(Err(std::io::Error::other(error).into())).ok();
                answer
            }
            None => answer,
        };
        cx.spawn_in(window, async move |workbench, cx| {
            let mut failed = None;
            let path = match answer.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) => None,
                Ok(Err(error)) => {
                    log::warn!("the open panel failed: {error}");
                    failed = Some(error.to_string());
                    None
                }
                Err(_) => {
                    log::warn!("the open panel went without an answer");
                    None
                }
            };
            workbench
                .update_in(cx, |workbench, window, cx| {
                    workbench.prompting = false;
                    if let Some(error) = failed {
                        let (message, detail) = open_panel_failed(&error);
                        toast::show(cx, Toast::error(message).detail(detail));
                    }
                    match (op, path) {
                        (_, None)
                        | (
                            OpenOp::Settings | OpenOp::NewWindow | OpenOp::NewFile | OpenOp::SaveAs,
                            Some(_),
                        ) => {}
                        (OpenOp::FolderInNewWindow, Some(path)) => {
                            open::open_window(Some(path), cx)
                        }
                        (OpenOp::Folder, Some(path)) => workbench.set_root(path, window, cx),
                        (OpenOp::File, Some(path)) => workbench
                            .active
                            .update(cx, |pane, cx| pane.open(path, window, cx)),
                    }
                })
                .ok();
        })
        .detach();
    }

    /// Opens an empty untitled tab in the active pane, focused.
    fn new_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let model = EditorModel::new(Buffer::new(""), None);
        self.active.update(cx, |pane, cx| {
            pane.add(model, cx);
            pane.activate(pane.tabs().len() - 1, window, cx);
        });
    }

    /// Shows the save panel for the active editor, in its file's folder
    /// (else the root, else home) with its file name (else `untitled`)
    /// suggested, then saves the editor where it answers
    /// ([`Workbench::save_editor_as`]). Cancelling changes nothing, and an
    /// error from the panel is shown as the open panel's is. Asked again
    /// while a panel shows, it does nothing.
    fn prompt_save_as(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.active.read(cx).active_editor().cloned() else {
            return;
        };
        // A copy to review isn't saved anywhere: no panel.
        if editor.read(cx).is_unsavable() {
            return;
        }
        if self.prompting {
            return;
        }
        self.prompting = true;
        let path = editor.read(cx).model().path().map(Path::to_owned);
        let dir = (path.as_deref().and_then(Path::parent).map(Path::to_owned))
            .or_else(|| self.root.clone())
            .or_else(std::env::home_dir)
            .unwrap_or_else(|| PathBuf::from("/"));
        let name = fe_gpui::pane_item::file_name(path.as_deref());
        let answer = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn_in(window, async move |workbench, cx| {
            let mut failed = None;
            let path = match answer.await {
                Ok(Ok(path)) => path,
                Ok(Err(error)) => {
                    log::warn!("the save panel failed: {error}");
                    failed = Some(error.to_string());
                    None
                }
                Err(_) => {
                    log::warn!("the save panel went without an answer");
                    None
                }
            };
            workbench
                .update_in(cx, |workbench, window, cx| {
                    workbench.prompting = false;
                    // As the open panel's (see `prompt_open`).
                    if let Some(error) = failed {
                        let (message, detail) = open_panel_failed(&error);
                        toast::show(cx, Toast::error(message).detail(detail));
                    }
                    if let Some(path) = path {
                        workbench.save_editor_as(&editor, path, window, cx);
                    }
                })
                .ok();
        })
        .detach();
    }

    /// Saves `editor` to `path` ([`EditorView::save_as`]), the tabs and the
    /// editors over its buffer following it there once it's written, or
    /// shows why it couldn't be.
    ///
    /// A file another buffer might also save to is refused: one another
    /// tab shows, a pane is reading, in this window or another, or another
    /// Save As holds, by any path to it ([`open::file_identity`]). The destination is held from here
    /// until the outcome is taken in ([`open::reserve`]), so nothing else
    /// takes it meanwhile, even if the tab closes. The destination's
    /// `.editorconfig` line ending, read now, goes with the save: it's
    /// written with it, and the buffer takes it only if the save goes
    /// through. So does its charset, for an untitled buffer: it saves in
    /// the charset's encoding, and goes back to its own if the save fails.
    pub(crate) fn save_editor_as(
        &mut self,
        editor: &Entity<EditorView>,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.save_editor_as_with(editor, path, true, window, cx);
    }

    /// [`Workbench::save_editor_as`], refusing a file that exists unless
    /// `overwrite` (a vim `:sav` without `!`): that is the vim message
    /// `E13`, not a prompt.
    pub(crate) fn save_editor_as_with(
        &mut self,
        editor: &Entity<EditorView>,
        path: PathBuf,
        overwrite: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A vim map waiting for this save goes on, or ends, when it's done.
        let hold = editor.update(cx, |editor, cx| editor.hold_vim_host(cx));
        let finish = move |editor: &Entity<EditorView>, ok: bool, cx: &mut App| {
            if let Some(hold) = hold {
                editor.update(cx, |editor, cx| editor.finish_vim_host(hold, ok, cx));
            }
        };
        let name = fe_gpui::pane_item::file_name(Some(&path));
        let identity = open::file_identity(&path);
        let same = |other: &Path| {
            other == path || identity.is_some() && open::file_identity(other) == identity
        };
        let model = editor.read(cx).model();
        let buffer = model.buffer().id();
        // This window, which is being updated, then every other one.
        let this_window = window.window_handle().window_id();
        let others = (cx.windows().into_iter())
            .filter(|other| other.window_id() != this_window)
            .filter_map(|other| other.downcast::<Workbench>())
            .filter_map(|other| other.read(cx).ok());
        let in_use = self.uses_file(&same, model, cx)
            || others
                .into_iter()
                .any(|other| other.uses_file(&same, model, cx));
        let held =
            open::reserved(&path, identity.as_deref()).is_some_and(|(holder, _)| holder != buffer);
        if in_use || held {
            drop(window.prompt(
                PromptLevel::Critical,
                &format!("“{name}” is open in another tab."),
                Some("Close it first, or save under another name."),
                &[PromptButton::ok("OK")],
                cx,
            ));
            return finish(editor, false, cx);
        }
        let reservation = open::reserve(buffer, path.clone(), identity);
        // Read here, so Save As requests keep their order.
        let config = fe_config::editorconfig_for(&path);
        let line_ending = crate::pane::config_line_ending(&config);
        let fixes = fe_gpui::SaveFixes {
            trim_trailing_whitespace: config.trim_trailing_whitespace == Some(true),
            insert_final_newline: config.insert_final_newline == Some(true),
        };
        // An untitled buffer saves in the destination's charset, if it's
        // still untitled when the save is prepared (see
        // `EditorView::save_as_in`); one of a file keeps its encoding.
        let charset = (config.charset)
            .filter(|_| model.path().is_none())
            .map(crate::loader::charset_encoding);
        let workbench = cx.weak_entity();
        let release = move |cx: &mut App| {
            (workbench.update(cx, |w, cx| w.lsp_release(buffer, cx))).ok();
        };
        let saved = editor.update(cx, |editor, cx| {
            editor.save_as_in(
                path,
                line_ending,
                charset,
                fixes,
                overwrite,
                release,
                window,
                cx,
            )
        });
        let editor = editor.clone();
        cx.spawn_in(window, async move |workbench, cx| {
            let result = saved.await;
            workbench
                .update_in(cx, |workbench, window, cx| {
                    if result.is_ok() {
                        workbench.editor_moved(&editor, cx);
                    }
                    drop(reservation);
                    let Err(error) = result else {
                        return finish(&editor, true, cx);
                    };
                    let exists = (error.downcast_ref::<std::io::Error>())
                        .and_then(|error| error.get_ref())
                        .is_some_and(|inner| inner.is::<fe_editor::SaveError>());
                    if exists && !overwrite {
                        editor.update(cx, |editor, cx| editor.set_vim_message(vim_host::E13, cx));
                        return finish(&editor, false, cx);
                    }
                    let detail = format!("{error:#}");
                    drop(window.prompt(
                        PromptLevel::Critical,
                        &format!("“{name}” couldn’t be saved."),
                        Some(&detail),
                        &[PromptButton::ok("OK")],
                        cx,
                    ));
                    finish(&editor, false, cx);
                })
                .ok();
        })
        .detach();
    }

    /// Whether a file `same` matches is in use here other than by `model`'s
    /// buffer: shown in a tab (a read-only viewer's too), being read into
    /// one, or the launch file the host is reading.
    fn uses_file(&self, same: &dyn Fn(&Path) -> bool, model: &EditorModel, cx: &App) -> bool {
        let panes = self.pane_group(cx).panes();
        let shown = (panes.iter())
            .flat_map(|pane| pane.read(cx).tabs())
            .any(|tab| {
                let ours = (tab.editor())
                    .is_some_and(|other| other.read(cx).model().shares_buffer_with(model));
                !ours && tab.item().path(cx).is_some_and(|other| same(&other))
            });
        let reading = (panes.iter()).any(|pane| pane.read(cx).reading().any(same));
        shown || reading || self.launch_reading(cx).is_some_and(same)
    }

    /// After a Save As of `editor`'s buffer: the tabs take its path as it
    /// is now, and its other editors detect their language from it.
    fn editor_moved(&mut self, editor: &Entity<EditorView>, cx: &mut Context<Self>) {
        for other in self.open_editors(cx) {
            let shares = other != *editor
                && (other.read(cx).model()).shares_buffer_with(editor.read(cx).model());
            if shares {
                other.update(cx, |other, cx| other.detect_language(cx));
            }
        }
        for pane in self.panes(cx) {
            pane.update(cx, |pane, cx| pane.sync_paths(cx));
        }
        self.track_editors(cx);
        self.sync_lsp(cx);
        cx.notify();
    }

    /// Turns `workbench.preview_tabs` on or off, as a settings reload
    /// does.
    pub fn set_preview_tabs(&mut self, on: bool) {
        self.preview_tabs = on;
    }

    /// Opens `path` from the project panel in the active pane. With vim on,
    /// its editor is in normal mode once it shows, whatever mode an editor
    /// of it already open was left in (a new one starts there): at once, or
    /// once an open waiting (behind a Save As of that file, say) lands, in
    /// each pane. A later open from the panel in the same pane replaces the
    /// one waiting there. With `preview`, if `workbench.preview_tabs` is on,
    /// it opens in the pane's preview tab ([`Pane::open_preview`]).
    pub(super) fn open_from_tree(
        &mut self,
        path: PathBuf,
        preview: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane = self.active.clone();
        pane.update(cx, |pane, cx| {
            if preview && self.preview_tabs {
                pane.open_preview(path.clone(), window, cx);
            } else {
                pane.open(path.clone(), window, cx);
            }
        });
        self.tree_opens.remove(&pane.entity_id());
        if self.tree_opened(&pane, &path, window, cx) {
            return;
        }
        let waiting = cx.observe_in(&pane, window, {
            let path = path.clone();
            move |workbench, pane, window, cx| {
                let current = (workbench.tree_opens.get(&pane.entity_id()))
                    .is_some_and(|(waiting, _)| *waiting == path);
                if current && workbench.tree_opened(&pane, &path, window, cx) {
                    workbench.tree_opens.remove(&pane.entity_id());
                }
            }
        });
        self.tree_opens.insert(pane.entity_id(), (path, waiting));
    }

    /// Whether the open of `path` from the tree is over in `pane`: its
    /// editor shows, which is put in vim's normal mode, or `path` is no
    /// longer being read (it failed, or another file was opened instead).
    fn tree_opened(
        &mut self,
        pane: &Entity<Pane>,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let pane = pane.read(cx);
        let editor = (pane.active_editor().cloned())
            .filter(|editor| editor.read(cx).model().path() == Some(path));
        let Some(editor) = editor else {
            return !pane.reading().any(|reading| reading == path);
        };
        let normal = |editor: &Entity<EditorView>, cx: &App| {
            (editor.read(cx).vim_mode()).is_none_or(|mode| mode == "normal")
        };
        if normal(&editor, cx) {
            return true;
        }
        // Outside this update: the key runs the editor's own.
        window.defer(cx, move |window, cx| {
            window.focus(&editor.focus_handle(cx), cx);
            // One escape leaves insert, a visual mode or a pending command;
            // a second, should the first only drop a count.
            for _ in 0..2 {
                if normal(&editor, cx) {
                    break;
                }
                window.dispatch_keystroke(gpui::Keystroke::parse("escape").unwrap(), cx);
            }
        });
        true
    }

    /// Why a rename of the file or folder at `path` must wait, if it must:
    /// an editor saving to it or under it, by its path or the file a save
    /// writes through a symlink ([`fe_editor::save_target`]), whether its
    /// tab and window are open or closed since ([`DetachedSaves`]); a Save
    /// As holding it as its destination; a pane reading it, in `window`'s
    /// workbench or any other window's; or the project panel creating,
    /// duplicating or trashing where it overlaps, from any window, open or
    /// closed since ([`project_panel::running`]). Paths are matched by the
    /// folders they're in, resolved ([`under`]), so another spelling of the
    /// root matches too.
    pub(crate) fn busy_under(path: &Path, window: &Window, cx: &App) -> Option<Busy> {
        let this_window = window.window_handle().window_id();
        let this = window.root::<Workbench>().flatten();
        let others = (cx.windows().into_iter())
            .filter(|other| other.window_id() != this_window)
            .filter_map(|other| other.downcast::<Workbench>())
            .filter_map(|other| other.entity(cx).ok());
        let workbenches = (this.into_iter().chain(others)).map(|workbench| workbench.read(cx));
        Self::busy_among(path, workbenches, cx)
    }

    /// [`Workbench::busy_under`] from inside this workbench's update, which
    /// has no window to ask: this workbench and `others`
    /// ([`Workbench::other_workbenches`]).
    pub(crate) fn busy_under_here(
        &self,
        path: &Path,
        others: &[Entity<Workbench>],
        cx: &App,
    ) -> Option<Busy> {
        Self::busy_among(path, self.and_others(others, cx), cx)
    }

    /// Every other window's workbench, from inside this one's update. A
    /// window being updated (this one's, as a picker in it applies an
    /// edit) can't be read, and is passed over: reading it panics, where
    /// updating it fails.
    pub(crate) fn other_workbenches(cx: &mut Context<Self>) -> Vec<Entity<Workbench>> {
        let this = cx.entity_id();
        let windows: Vec<_> = (cx.windows().into_iter())
            .filter(|window| window.downcast::<Workbench>().is_some())
            .collect();
        (windows.into_iter())
            .filter_map(|window| cx.update_window(window, |root, _, _| root).ok())
            .filter_map(|root| root.downcast::<Workbench>().ok())
            .filter(|other| other.entity_id() != this)
            .collect()
    }

    /// This workbench, then `others`, read.
    pub(crate) fn and_others<'a>(
        &'a self,
        others: &'a [Entity<Workbench>],
        cx: &'a App,
    ) -> impl Iterator<Item = &'a Workbench> {
        std::iter::once(self).chain(others.iter().map(|other| other.read(cx)))
    }

    /// [`Workbench::busy_under`] over `workbenches`.
    fn busy_among<'a>(
        path: &Path,
        workbenches: impl Iterator<Item = &'a Workbench>,
        cx: &'a App,
    ) -> Option<Busy> {
        let from = resolved(path);
        let under = |other: &Path| under(other, path, &from);
        if open::reserved_paths().iter().any(|held| under(held)) {
            return Some(Busy::Saving);
        }
        let saving_under = |editor: &Entity<EditorView>| {
            let editor = editor.read(cx);
            editor.is_saving()
                && editor.model().path().is_some_and(|saved| {
                    under(saved) || save_target(saved).is_ok_and(|target| under(&target))
                })
        };
        let detached = cx.try_global::<DetachedSaves>();
        if detached.is_some_and(|saves| saves.0.iter().any(saving_under)) {
            return Some(Busy::Saving);
        }
        for workbench in workbenches {
            if workbench.open_editors(cx).iter().any(saving_under) {
                return Some(Busy::Saving);
            }
            let panes = workbench.pane_group(cx).panes();
            let reading = (panes.iter()).any(|pane| pane.read(cx).reading().any(under));
            if reading || workbench.launch_reading(cx).is_some_and(under) {
                return Some(Busy::Opening);
            }
        }
        if project_panel::running()
            .iter()
            .any(|running| overlap(running, path))
        {
            return Some(Busy::Working);
        }
        None
    }

    /// Where every editor of `from`, or of a file under it, in `window`'s
    /// workbench and every other window's, will be once `from` is renamed
    /// `to` ([`moved_to`]). Taken before the rename, while every spelling
    /// of `from` still resolves: a symlink into a renamed folder dangles
    /// after it.
    pub(crate) fn plan_rename(from: &Path, to: &Path, window: &Window, cx: &App) -> Moves {
        let this_window = window.window_handle().window_id();
        let this = window.root::<Workbench>().flatten();
        let others = (cx.windows().into_iter())
            .filter(|other| other.window_id() != this_window)
            .filter_map(|other| other.downcast::<Workbench>())
            .filter_map(|other| other.entity(cx).ok());
        (this.into_iter().chain(others))
            .flat_map(|workbench| workbench.read(cx).moves(from, to, cx))
            .collect()
    }

    /// [`Workbench::plan_rename`] from inside this workbench's update, over
    /// it and `others`.
    pub(crate) fn plan_rename_here(
        &self,
        from: &Path,
        to: &Path,
        others: &[Entity<Workbench>],
        cx: &App,
    ) -> Moves {
        (self.and_others(others, cx))
            .flat_map(|workbench| workbench.moves(from, to, cx))
            .collect()
    }

    /// [`Workbench::follow_moves`] in this workbench and `others`, from
    /// inside this one's update.
    pub(crate) fn follow_moves_everywhere(
        &mut self,
        moves: &Moves,
        others: &[Entity<Workbench>],
        cx: &mut Context<Self>,
    ) {
        self.follow_moves(moves, cx);
        for other in others {
            other.update(cx, |other, cx| other.follow_moves(moves, cx));
        }
    }

    /// [`Workbench::plan_rename`]'s moves of this workbench's editors.
    fn moves(&self, from: &Path, to: &Path, cx: &App) -> Moves {
        let (from_resolved, to_resolved) = (resolved(from), resolved(to));
        (self.open_editors(cx).into_iter())
            .filter_map(|editor| {
                let path = editor.read(cx).model().path()?;
                let path = moved_to(path, from, to, &from_resolved, &to_resolved)?;
                Some((editor, path))
            })
            .collect()
    }

    /// After the project panel renamed a file or folder: the editors of it,
    /// or of the files in it, take the paths `moves` planned for them
    /// ([`Workbench::plan_rename`]), in this window and every other
    /// ([`Workbench::follow_moves`]).
    pub(super) fn panel_renamed(&mut self, moves: &Moves, window: &Window, cx: &mut Context<Self>) {
        self.follow_moves(moves, cx);
        // This window, which is being updated, then every other one.
        let this_window = window.window_handle().window_id();
        let others = (cx.windows().into_iter())
            .filter(|other| other.window_id() != this_window)
            .filter_map(|other| other.downcast::<Workbench>());
        for other in others {
            (other.update(cx, |other, _, cx| other.follow_moves(moves, cx))).ok();
        }
    }

    /// Has the editors of `from`, or of files under it, follow its rename to
    /// `to`, as [`Workbench::follow_moves`] does, their paths resolved now:
    /// for tests that rename behind the panel's back.
    #[cfg(test)]
    pub(crate) fn follow_rename(&mut self, from: &Path, to: &Path, cx: &mut Context<Self>) {
        let moves = self.moves(from, to, cx);
        self.follow_moves(&moves, cx);
    }

    /// Has each of this workbench's editors in `moves` save to its new path
    /// from now on ([`EditorModel::move_path`]: the stamp it knows moves
    /// too, so a save still refuses the file if it changed), once per
    /// buffer, as the path set through one editor is every split's: its
    /// tabs, title and breadcrumbs follow, it detects its language again,
    /// and its language server document is closed and opened again under
    /// the new path. Its text, vim mode and selections stay as they are.
    pub(crate) fn follow_moves(&mut self, moves: &Moves, cx: &mut Context<Self>) {
        let open: HashSet<EntityId> = (self.open_editors(cx).iter())
            .map(Entity::entity_id)
            .collect();
        let moved: Vec<_> = (moves.iter())
            .filter(|(editor, _)| open.contains(&editor.entity_id()))
            .collect();
        if moved.is_empty() {
            return;
        }
        let mut buffers = HashSet::new();
        for (editor, path) in &moved {
            let buffer = editor.read(cx).model().buffer().id();
            if buffers.insert(buffer) {
                self.lsp_release(buffer, cx);
                let path = path.clone();
                editor.update(cx, |editor, _| editor.model_mut().move_path(path));
            }
        }
        for (editor, _) in &moved {
            editor.update(cx, |editor, cx| editor.detect_language(cx));
        }
        for pane in self.panes(cx) {
            pane.update(cx, |pane, cx| pane.sync_paths(cx));
        }
        self.track_editors(cx);
        self.sync_lsp(cx);
        cx.notify();
    }

    /// Opens the user's `settings.toml` in the active pane, creating it from
    /// the template first, in the background, if it's missing.
    fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = open::settings_path(cx) else {
            log::error!("can't open settings: there's no config directory");
            toast::show(
                cx,
                Toast::error("Can't open settings").detail("There's no config directory"),
            );
            return;
        };
        let created = cx.background_spawn({
            let path = path.clone();
            async move { open::create_settings(&path) }
        });
        cx.spawn_in(window, async move |workbench, cx| {
            if let Err(error) = created.await {
                log::error!("can't create {}: {error}", path.display());
                cx.update(|_, cx| {
                    toast::show(
                        cx,
                        Toast::error(format!("Can't create {}", path.display()))
                            .detail(error.to_string()),
                    )
                })
                .ok();
                return;
            }
            workbench
                .update_in(cx, |workbench, window, cx| {
                    workbench
                        .active
                        .update(cx, |pane, cx| pane.open(path, window, cx))
                })
                .ok();
        })
        .detach();
    }

    /// Opens the user's `keymap.toml` in the active pane, creating it with
    /// [`fe_keymap::KEYMAP_HEADER`] first if it's missing: Open Keyboard
    /// Shortcuts (JSON).
    pub(crate) fn open_keymap_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = shortcuts_editor::keymap_path(cx) else {
            log::error!("can't open the keymap: there's no config directory");
            toast::show(
                cx,
                Toast::error("Can't open the keymap").detail("There's no config directory"),
            );
            return;
        };
        let created = cx.background_spawn({
            let path = path.clone();
            async move { shortcuts_editor::create_keymap(&path) }
        });
        cx.spawn_in(window, async move |workbench, cx| {
            if let Err(error) = created.await {
                log::error!("can't create {}: {error}", path.display());
                cx.update(|_, cx| {
                    toast::show(
                        cx,
                        Toast::error(format!("Can't create {}", path.display()))
                            .detail(error.to_string()),
                    )
                })
                .ok();
                return;
            }
            workbench
                .update_in(cx, |workbench, window, cx| {
                    workbench
                        .active
                        .update(cx, |pane, cx| pane.open(path, window, cx))
                })
                .ok();
        })
        .detach();
    }

    /// The launch file the host is reading, until it lands: it opens in a
    /// tab then, whatever the pane has shown since.
    /// Nothing once its pane has closed: [`Workbench::file_read`] drops it
    /// then.
    pub(crate) fn launch_reading(&self, cx: &App) -> Option<&Path> {
        let (path, pane) = self.launch_read.as_ref()?;
        let pane = pane.upgrade()?;
        self.pane_group(cx)
            .panes()
            .contains(&&pane)
            .then_some(path.as_path())
    }

    /// Opens the host's read of the launch file, `Ok(Opened::Text(None))`
    /// being a new file, in the pane it was launched in, whatever that pane has shown
    /// since; see [`Launch::file`]. If that pane has closed, the read is
    /// dropped. Any other read opens in the pane showing it being read, else
    /// the active pane.
    pub fn file_read(
        &mut self,
        path: PathBuf,
        result: std::io::Result<Opened>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane = match self.launch_read.take_if(|(launched, _)| *launched == path) {
            Some((_, pane)) => {
                let open = self.pane_group(cx).panes();
                let pane = pane.upgrade().filter(|pane| open.contains(&pane));
                let Some(pane) = pane else {
                    log::info!("not opening {}: its pane closed", path.display());
                    return;
                };
                pane
            }
            None => self
                .pane_group(cx)
                .panes()
                .into_iter()
                .find(|pane| pane.read(cx).is_opening(&path))
                .unwrap_or(&self.active)
                .clone(),
        };
        pane.update(cx, |pane, cx| pane.file_read(path, result, window, cx));
    }
}

/// Editors and the paths they're to take: [`Workbench::plan_rename`]'s.
pub(crate) type Moves = Vec<(Entity<EditorView>, PathBuf)>;

/// Editors still saving after their tab or window closed, until the save is
/// over, held so [`Workbench::busy_under`] sees the write coming.
#[derive(Default)]
pub(super) struct DetachedSaves(Vec<Entity<EditorView>>);

impl Global for DetachedSaves {}

impl DetachedSaves {
    /// Keeps those of `editors` saving, and lets go of the saves over.
    pub(super) fn keep(editors: impl Iterator<Item = Entity<EditorView>>, cx: &mut App) {
        let mut kept = (cx.try_global::<DetachedSaves>())
            .map(|saves| saves.0.clone())
            .unwrap_or_default();
        kept.extend(editors);
        kept.retain(|editor| editor.read(cx).is_saving());
        cx.set_global(DetachedSaves(kept));
    }
}

/// Whether `a` and `b` overlap: one is the other, or under it, as [`under`]
/// matches.
pub(crate) fn overlap(a: &Path, b: &Path) -> bool {
    under(a, b, &resolved(b)) || under(b, a, &resolved(a))
}

/// Why a rename waits ([`Workbench::busy_under`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Busy {
    /// A save or a Save As would write there.
    Saving,
    /// A file there is being read into a tab.
    Opening,
    /// The project panel is creating, duplicating or trashing there.
    Working,
}

/// Whether `path` is `from`, or under it, spelled as it is or as another
/// spelling of the same folders: `from_resolved` is `from`'s
/// [`resolved`].
pub(crate) fn under(path: &Path, from: &Path, from_resolved: &Path) -> bool {
    path.starts_with(from) || resolved(path).starts_with(from_resolved)
}

/// Where the file at `path` is once `from` is renamed `to`, if it's `from`
/// or under it: under `to` as `path` spells it, or, for a path spelled
/// through other folders (the root opened through a symlink, say), under
/// `to`'s resolved spelling. `from_resolved` and `to_resolved` are their
/// [`resolved`]s, taken after the rename. The entry itself is never
/// dereferenced, so a symlinked file keeps its link.
fn moved_to(
    path: &Path,
    from: &Path,
    to: &Path,
    from_resolved: &Path,
    to_resolved: &Path,
) -> Option<PathBuf> {
    let join = |base: &Path, rest: &Path| {
        if rest.as_os_str().is_empty() {
            base.to_owned()
        } else {
            base.join(rest)
        }
    };
    if let Ok(rest) = path.strip_prefix(from) {
        return Some(join(to, rest));
    }
    let resolved = resolved(path);
    let rest = resolved.strip_prefix(from_resolved).ok()?;
    Some(join(to_resolved, rest))
}

/// One identity for the spellings of `path`: its folder with the longest
/// existing ancestor resolved through symlinks and `..` (the rest, which
/// may be deleted, kept), then its own name as is. The entry itself is
/// never dereferenced, so a symlink keeps its own identity apart from its
/// target's.
pub(crate) fn resolved(path: &Path) -> PathBuf {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return path.to_owned();
    };
    let mut rest = Vec::new();
    let mut ancestor = parent;
    let base = loop {
        if let Ok(real) = dunce::canonicalize(ancestor) {
            break real;
        }
        match (ancestor.file_name(), ancestor.parent()) {
            (Some(name), Some(up)) => {
                rest.push(name);
                ancestor = up;
            }
            _ => break ancestor.to_owned(),
        }
    };
    rest.iter()
        .rev()
        .fold(base, |base, part| base.join(part))
        .join(name)
}
