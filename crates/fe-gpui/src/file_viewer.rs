//! [`FileViewer`]: a read-only view of a file too large for a buffer, a
//! [`ViewerModel`] over a [`FileText`] painted under a one-line banner.
//!
//! It keeps out of [`crate::EditorView`]'s way: the editor's keys reach it
//! through its key context, and it runs them on the viewer model, which
//! refuses every edit. While the file's [`LineIndex`] is still building it
//! polls for the rows found since, so the view grows as the scan goes.
//!
//! Once the index is complete it re-stats the file every
//! [`CHECK_INTERVAL`] while it's in a window, and when it gets focus. A file
//! that changed, or couldn't be read, turns the banner into "changed on
//! disk." with a Reload button, which reopens it ([`FileViewer::reload`]).
//! It's a pane item: see `pane_item.rs`.

use std::{io, ops::Range, path::Path, sync::Arc, time::Duration};

use fe_editor::{CopyError, Outcome, ViewerModel, commands::CONTEXT};
use fe_fileview::{FileText, FileView, Freshness, IndexState, LineIndex};
use fe_text::TextSource as _;
use gpui::{
    App, ClipboardItem, Context, CursorStyle, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyContext, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, Point, Render, ScrollWheelEvent,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window,
    WindowId, div, px,
};

use crate::{EditorSettings, EditorTheme, commands::RunCommand, pane_item::file_name};

mod element;
mod find;
mod pane_item;

use element::{FileViewerElement, ViewerLayout};

/// How often a view polls an index that is still building.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// The status bar counts the chars in at most this many bytes, and bytes
/// past it.
const COUNT_LIMIT: usize = 1024 * 1024;

/// How often a view whose index is complete checks its file on disk.
const CHECK_INTERVAL: Duration = Duration::from_secs(2);

/// What a [`FileViewer`] tells its host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileViewerEvent {
    /// A key or command tried to edit the text, which is read-only. Nothing
    /// changed.
    ReadOnly,
    /// A copy of `len` bytes was refused: more than
    /// [`fe_editor::COPY_LIMIT`].
    CopyTooLarge { len: usize },
    /// The file changed on disk, or couldn't be read, since it was opened:
    /// the index stopped short of `Complete`, a read failed, or a check of
    /// the file found it changed. Sent once per change: once more only
    /// after a [`FileViewerEvent::Reloaded`].
    Changed,
    /// [`FileViewer::reload`] reopened the file and shows it now.
    Reloaded,
    /// A search selected a match; `wrapped` if it went round the file.
    Found { wrapped: bool },
    /// A search found no match; the selection stays.
    NotFound,
}

/// A read-only view of a [`FileView`], with its rows from `index`.
pub struct FileViewer {
    view: Arc<FileView>,
    index: Arc<LineIndex>,
    /// The snapshot the model holds, whose read errors it records.
    text: FileText,
    pub(crate) model: ViewerModel,
    /// What the banner shows: the host's, or why to reload.
    banner: SharedString,
    /// The host's banner, which a reload puts back.
    host_banner: SharedString,
    /// The banner has a Reload button.
    offers_reload: bool,
    pub(crate) theme: EditorTheme,
    pub(crate) focus: FocusHandle,
    /// How far the text is scrolled, in pixels (both ≥ 0).
    pub(crate) scroll: Point<Pixels>,
    /// Scroll the head into view on the next frame.
    pub(crate) autoscroll: bool,
    /// What the last frame laid out, for hit testing.
    pub(crate) layout: Option<ViewerLayout>,
    /// Where a mouse drag selects from.
    drag: Option<usize>,
    /// Checks the index once the host can hear of it, then polls it while
    /// it builds.
    poll: Option<Task<()>>,
    /// [`FileViewerEvent::Changed`] was sent.
    changed_sent: bool,
    /// Checks the file on disk once the index is complete.
    watch: Option<Watch>,
    /// Reopens the file for [`FileViewer::reload`].
    reload: Option<Task<()>>,
    /// Where a reload or a restore puts the selection and scroll back.
    restore: Option<Restore>,
    /// Counts the text snapshots the model has had, for [`CursorKey`].
    generation: u64,
    /// The status bar's cursor text, and the selection and text it's of.
    cursor: Option<(CursorKey, SharedString)>,
    /// Works out the cursor text for another selection or text, off the
    /// main thread: finding a row or a column can read much of the file.
    cursor_task: Option<(CursorKey, Task<()>)>,
    /// The cursor texts worked out.
    #[cfg(test)]
    pub(crate) cursors_worked_out: usize,
    /// The query, and the search running.
    find: find::FindState,
    /// The banner the last frame drew.
    #[cfg(test)]
    pub(crate) drawn_banner: SharedString,
    /// The rows the last frame painted: buffer row, text and window
    /// position.
    #[cfg(test)]
    pub(crate) painted_rows: Vec<(u32, String, Point<Pixels>)>,
}

impl EventEmitter<FileViewerEvent> for FileViewer {}

/// The file checks of a view in `window`: every [`CHECK_INTERVAL`] and
/// on focus.
struct Watch {
    window: WindowId,
    _checks: Task<()>,
    _focus: Subscription,
}

/// A selection, and maybe a scroll offset, to put back as a new index
/// reaches them: until it's done, a snapshot of it clamps them to the rows
/// found so far.
/// What a cursor text is of: a snapshot of the text, the anchor and the
/// head.
type CursorKey = (u64, usize, usize);

struct Restore {
    anchor: usize,
    head: usize,
    /// `None` scrolls the head into view instead.
    scroll_y: Option<Pixels>,
}

/// Opens `path` and starts indexing it, as a host does a large file.
pub(crate) fn open(path: &Path) -> io::Result<(Arc<FileView>, Arc<LineIndex>)> {
    let view = Arc::new(FileView::open(path)?);
    let index = Arc::new(LineIndex::build(view.clone()));
    Ok((view, index))
}

impl FileViewer {
    /// A viewer of `view`, whose rows `index` finds, under a strip saying
    /// `banner`.
    pub fn new(
        view: Arc<FileView>,
        index: Arc<LineIndex>,
        banner: SharedString,
        theme: EditorTheme,
        cx: &mut Context<Self>,
    ) -> Self {
        // Read before the snapshot: an index that completes in between is
        // still polled once more, for the rest of its rows.
        let building = index.progress().state == IndexState::Building;
        let text = FileText::new(view.clone(), index.clone());
        let model = ViewerModel::new(Arc::new(text.clone()), EditorSettings::default().tab_size);
        let mut viewer = Self {
            view,
            index,
            text,
            model,
            banner: banner.clone(),
            host_banner: banner,
            offers_reload: false,
            theme,
            focus: cx.focus_handle(),
            scroll: Point::default(),
            autoscroll: false,
            layout: None,
            drag: None,
            poll: None,
            changed_sent: false,
            watch: None,
            reload: None,
            restore: None,
            generation: 0,
            cursor: None,
            cursor_task: None,
            find: Default::default(),
            #[cfg(test)]
            cursors_worked_out: 0,
            #[cfg(test)]
            drawn_banner: SharedString::default(),
            #[cfg(test)]
            painted_rows: Vec::new(),
        };
        viewer.start_polling(building, cx);
        viewer
    }

    /// Checks the index on the foreground executor's next turn, and then,
    /// if it was `building` before the text's snapshot, polls it.
    fn start_polling(&mut self, building: bool, cx: &mut Context<Self>) {
        // An index that has already failed is told of on the foreground
        // executor's next turn, once whoever is constructing the view has
        // subscribed to it: an event sent now would reach no one.
        self.poll = Some(cx.spawn(async move |this, cx| {
            let mut polling = this.update(cx, |viewer, cx| {
                viewer.check_index(cx);
                if !building {
                    viewer.put_back(true);
                    viewer.poll = None;
                }
                building
            });
            while matches!(polling, Ok(true)) {
                cx.background_executor().timer(POLL_INTERVAL).await;
                polling = this.update(cx, |viewer, cx| viewer.refresh(cx));
            }
        }));
    }

    pub fn model(&self) -> &ViewerModel {
        &self.model
    }

    /// What the banner shows: the host's, until the file changes on disk
    /// or a reload fails.
    pub fn banner(&self) -> &SharedString {
        &self.banner
    }

    /// Whether the banner has a Reload button.
    pub fn offers_reload(&self) -> bool {
        self.offers_reload
    }

    /// Whether a [`FileViewer::reload`] is reopening the file.
    pub fn is_reloading(&self) -> bool {
        self.reload.is_some()
    }

    /// The file's name, for the banner.
    fn file_name(&self) -> String {
        file_name(Some(self.view.path()))
    }

    /// Whether the view is still polling its index for more rows.
    pub fn is_polling(&self) -> bool {
        self.poll.is_some()
    }

    /// Takes the rows the index has found since the last snapshot, and
    /// says whether to poll again: only while it's still building.
    fn refresh(&mut self, cx: &mut Context<Self>) -> bool {
        let building = self.index.progress().state == IndexState::Building;
        // The old snapshot's reads may have failed since the last check.
        self.check_index(cx);
        let text = FileText::new(self.view.clone(), self.index.clone());
        // A read that failed while it was made, or a snapshot with fewer
        // rows than the one shown, would drop rows from the model: keep
        // the last snapshot and offer a reload.
        if text.error().is_some() || text.row_count() < self.text.row_count() {
            self.poll = None;
            self.mark_changed(cx);
            cx.notify();
            return false;
        }
        self.text = text;
        self.model.refresh(Arc::new(self.text.clone()));
        self.generation += 1;
        self.put_back(!building);
        self.check_index(cx);
        if !building {
            self.poll = None;
        }
        cx.notify();
        building
    }

    /// Puts back what [`Restore`] holds, as far as the text reaches, and
    /// forgets it once the text is `last`: its index is done.
    fn put_back(&mut self, last: bool) {
        let Some(restore) = &self.restore else {
            return;
        };
        self.model.set_selection(restore.anchor, restore.head);
        match restore.scroll_y {
            Some(y) => {
                self.scroll.y = y;
                self.autoscroll = false;
            }
            None => self.autoscroll = true,
        }
        if last {
            self.restore = None;
        }
    }

    /// Selects from `anchor` to `head`, clipped to the text: now, and as
    /// the index finds more of it, until it's done or the user moves.
    /// Keeps the scroll at `scroll_y`, or with `None` shows the head.
    fn restore_selection(
        &mut self,
        anchor: usize,
        head: usize,
        scroll_y: Option<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.restore = Some(Restore {
            anchor,
            head,
            scroll_y,
        });
        // While it polls, the poll puts it back once the index is done.
        self.put_back(self.poll.is_none());
        cx.notify();
    }

    /// Sends [`FileViewerEvent::Changed`], once, if the index stopped short
    /// or a read of the text failed. Call after anything that reads it.
    pub(crate) fn check_index(&mut self, cx: &mut Context<Self>) {
        let state = self.index.progress().state;
        let failed = !matches!(state, IndexState::Building | IndexState::Complete);
        if failed || self.text.error().is_some() {
            self.mark_changed(cx);
        }
    }

    /// Re-stats the file once the index is complete, and says whether to
    /// check again: until it's found changed.
    fn check_disk(&mut self, cx: &mut Context<Self>) -> bool {
        if self.changed_sent {
            return false;
        }
        if self.index.progress().state == IndexState::Complete
            && self.view.check() == Freshness::Changed
        {
            self.mark_changed(cx);
            return false;
        }
        true
    }

    /// Offers a reload in the banner and sends
    /// [`FileViewerEvent::Changed`], once per change.
    fn mark_changed(&mut self, cx: &mut Context<Self>) {
        if self.changed_sent {
            return;
        }
        self.changed_sent = true;
        self.banner = format!("{} changed on disk.", self.file_name()).into();
        self.offers_reload = true;
        cx.emit(FileViewerEvent::Changed);
        cx.notify();
    }

    /// Checks the file every [`CHECK_INTERVAL`] while the view is in
    /// `window`, and when it gets focus there, once the index is complete.
    fn watch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = window.window_handle().window_id();
        let complete = self.index.progress().state == IndexState::Complete;
        if !complete || self.changed_sent || self.watch.as_ref().is_some_and(|w| w.window == id) {
            return;
        }
        let checks = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(CHECK_INTERVAL).await;
                // Through the window, so it fails once the window is gone,
                // even while someone still holds the view.
                let checking =
                    cx.update(|_, cx| this.update(cx, |viewer, cx| viewer.check_disk(cx)));
                if !matches!(checking, Ok(Ok(true))) {
                    break;
                }
            }
        });
        // The listener starts listening after this frame, whose focus
        // change it would miss: a view focused already is checked now.
        let focus = cx.on_focus(&self.focus, window, |viewer, _, cx| {
            viewer.check_disk(cx);
        });
        if self.focus.is_focused(window) {
            cx.defer_in(window, |viewer, _, cx| {
                viewer.check_disk(cx);
            });
        }
        self.watch = Some(Watch {
            window: id,
            _checks: checks,
            _focus: focus,
        });
    }

    /// The status bar's cursor text, `Ln 12, Col 5` and maybe
    /// ` (17 selected)` as the editor's, the last worked out, which may be of an earlier
    /// selection while the current one's is worked out.
    pub(crate) fn cursor_text(&self) -> Option<&SharedString> {
        self.cursor.as_ref().map(|(_, text)| text)
    }

    /// Works out the cursor text off the main thread, unless it's of the
    /// current selection and text or being worked out for them.
    fn work_out_cursor(&mut self, cx: &mut Context<Self>) {
        use gpui::AppContext as _;
        let key = (self.generation, self.anchor(), self.model.head());
        let current = |done: Option<CursorKey>| done == Some(key);
        if current(self.cursor.as_ref().map(|(key, _)| *key))
            || current(self.cursor_task.as_ref().map(|(key, _)| *key))
        {
            return;
        }
        let text = self.model.text().share();
        let (anchor, head) = (key.1, key.2);
        // Finding the row and its start reads up to 63 rows of the file.
        let working = cx.background_spawn(async move {
            // Its bytes past the limit, rather than read them all.
            let chars_in = |range: Range<usize>| {
                if range.len() > COUNT_LIMIT {
                    return range.len();
                }
                text.chunks_in_range(range)
                    .map(|chunk| chunk.chars().count())
                    .sum()
            };
            let row = text.offset_to_point(head).row;
            let column = chars_in(text.row_start(row)..head) + 1;
            let mut cursor = format!("Ln {}, Col {column}", row + 1);
            let selection = anchor.min(head)..anchor.max(head);
            if !selection.is_empty() {
                cursor += &format!(" ({} selected)", chars_in(selection));
            }
            cursor
        });
        let task = cx.spawn(async move |this, cx| {
            let cursor = working.await;
            this.update(cx, |viewer, cx| {
                viewer.cursor = Some((key, cursor.into()));
                viewer.cursor_task = None;
                #[cfg(test)]
                {
                    viewer.cursors_worked_out += 1;
                }
                // Reading the rows may have found the file changed.
                viewer.check_index(cx);
                cx.notify();
            })
            .ok();
        });
        self.cursor_task = Some((key, task));
    }

    /// Reopens the file off the main thread and shows it, keeping the
    /// selection, clipped, and the scroll row, under the host's banner
    /// again; then sends [`FileViewerEvent::Reloaded`]. If it can't be
    /// opened, the banner says so and still offers a reload.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        use gpui::AppContext as _;
        self.find.stop();
        let path = self.view.path().to_owned();
        let opening = cx.background_spawn(async move { open(&path) });
        self.reload = Some(cx.spawn(async move |this, cx| {
            let opened = opening.await;
            this.update(cx, |viewer, cx| viewer.reopened(opened, cx))
                .ok();
        }));
    }

    fn reopened(
        &mut self,
        opened: io::Result<(Arc<FileView>, Arc<LineIndex>)>,
        cx: &mut Context<Self>,
    ) {
        self.reload = None;
        self.find.stop();
        let (view, index) = match opened {
            Ok(opened) => opened,
            Err(error) => {
                // It changed, as far as checks go, so they leave this banner
                // be.
                self.mark_changed(cx);
                self.banner = format!("Cannot read {}: {error}", self.file_name()).into();
                cx.notify();
                return;
            }
        };
        let building = index.progress().state == IndexState::Building;
        let (anchor, head) = (self.anchor(), self.model.head());
        self.view = view;
        self.index = index;
        self.text = FileText::new(self.view.clone(), self.index.clone());
        // A new model rather than `ViewerModel::refresh`, which takes a
        // later snapshot of the same text, never fewer rows: the long rows
        // laid out for this one go with it.
        self.model = ViewerModel::new(Arc::new(self.text.clone()), self.model.tab_size());
        self.generation += 1;
        self.restore = Some(Restore {
            anchor,
            head,
            scroll_y: Some(self.scroll.y),
        });
        self.put_back(false);
        self.banner = self.host_banner.clone();
        self.offers_reload = false;
        self.changed_sent = false;
        self.watch = None;
        self.drag = None;
        self.start_polling(building, cx);
        cx.emit(FileViewerEvent::Reloaded);
        cx.notify();
    }

    /// Runs an editor command on the model. Commands the viewer doesn't
    /// know that aren't the editor's go on to the app.
    fn on_run_command(&mut self, run: &RunCommand, _: &mut Window, cx: &mut Context<Self>) {
        let invocation = &run.0;
        let id = invocation.command.as_str();
        self.model.set_page_rows(self.page_rows());
        match self.model.run(id, &invocation.args) {
            Outcome::Changed => {
                self.restore = None;
                self.autoscroll = true;
                cx.notify();
            }
            Outcome::Nothing => {}
            Outcome::Copy(Ok(text)) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
            Outcome::Copy(Err(CopyError::TooLarge { len })) => {
                cx.emit(FileViewerEvent::CopyTooLarge { len })
            }
            Outcome::BadArgs(error) => log::error!("{id}: {error}"),
            Outcome::ReadOnly => cx.emit(FileViewerEvent::ReadOnly),
            Outcome::Unsupported if id.starts_with("editor.") => {}
            Outcome::Unsupported => cx.propagate(),
        }
        // A copy or a move may have read rows no frame had.
        self.check_index(cx);
    }

    /// A typed char, which no binding took, would edit.
    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let held = &event.keystroke.modifiers;
        if event.keystroke.key_char.is_some() && !held.platform && !held.control {
            cx.emit(FileViewerEvent::ReadOnly);
            cx.stop_propagation();
        }
    }

    fn page_rows(&self) -> u32 {
        self.layout.as_ref().map_or(20, |layout| {
            layout.frame.visible_rows().saturating_sub(2).max(1)
        })
    }

    /// The offset under a window position, in the last frame.
    fn offset_for_position(&mut self, position: Point<Pixels>) -> usize {
        let model = &self.model;
        self.layout
            .as_mut()
            .map_or(0, |layout| layout.offset_at(model, position))
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        let offset = self.offset_for_position(event.position);
        let anchor = if event.modifiers.shift {
            self.anchor()
        } else {
            offset
        };
        self.model.set_selection(anchor, offset);
        self.restore = None;
        self.drag = Some(anchor);
        self.autoscroll = true;
        self.check_index(cx);
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(anchor) = self.drag else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.drag = None;
            return;
        }
        let offset = self.offset_for_position(event.position);
        self.model.set_selection(anchor, offset);
        self.autoscroll = true;
        self.check_index(cx);
        cx.notify();
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.drag = None;
    }

    /// The selection's end that isn't the head.
    fn anchor(&self) -> usize {
        let selection = self.model.selection();
        if self.model.head() == selection.start {
            selection.end
        } else {
            selection.start
        }
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let line_height = self
            .layout
            .as_ref()
            .map_or(px(20.), |layout| px(layout.frame.line_height));
        let delta = event.delta.pixel_delta(line_height);
        self.scroll.x = (self.scroll.x - delta.x).max(px(0.));
        self.scroll.y = (self.scroll.y - delta.y).max(px(0.));
        // The element clamps against the content size on the next frame.
        self.autoscroll = false;
        self.restore = None;
        cx.notify();
    }
}

impl Render for FileViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.watch(window, cx);
        self.work_out_cursor(cx);
        #[cfg(test)]
        {
            self.drawn_banner = self.banner.clone();
        }
        let theme = &self.theme;
        let mut key_context = KeyContext::default();
        key_context.add(CONTEXT);
        let reload = self.offers_reload.then(|| {
            div()
                .id("file-viewer-reload")
                .debug_selector(|| "file-viewer-reload".into())
                .flex_none()
                .px(px(6.))
                .border_1()
                .border_color(theme.popover_border)
                .rounded(px(3.))
                .text_color(theme.foreground)
                .cursor_pointer()
                .child("Reload")
                .on_click(cx.listener(|viewer, _, _, cx| viewer.reload(cx)))
        });
        let banner = div()
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .px(px(8.))
            .py(px(2.))
            .bg(theme.popover_background)
            .border_b_1()
            .border_color(theme.popover_border)
            .text_color(theme.gutter_active_foreground)
            .text_size(theme.font_size)
            .font_family(theme.font_family.clone())
            .whitespace_nowrap()
            .child(
                div()
                    .debug_selector(|| "file-viewer-banner".into())
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .child(self.banner.clone()),
            )
            .children(reload);
        let text = div()
            .id("file-viewer-text")
            .flex_1()
            .min_h_0()
            .cursor(CursorStyle::IBeam)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .child(FileViewerElement::new(cx.entity()));
        div()
            .id("file-viewer")
            .key_context(key_context)
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.background)
            .on_action(cx.listener(Self::on_run_command))
            .on_key_down(cx.listener(Self::on_key_down))
            .child(banner)
            .child(text)
    }
}

impl Focusable for FileViewer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod pane_item_tests;

#[cfg(test)]
mod find_tests;
