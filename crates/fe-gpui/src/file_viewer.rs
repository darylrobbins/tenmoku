//! [`FileViewer`]: a read-only view of a file too large for a buffer, a
//! [`ViewerModel`] over a [`FileText`] painted under a one-line banner.
//!
//! It keeps out of [`crate::EditorView`]'s way: the editor's keys reach it
//! through its key context, and it runs them on the viewer model, which
//! refuses every edit. While the file's [`LineIndex`] is still building it
//! polls for the rows found since, so the view grows as the scan goes.

use std::{sync::Arc, time::Duration};

use fe_editor::{CopyError, Outcome, ViewerModel, commands::CONTEXT};
use fe_fileview::{FileText, FileView, IndexState, LineIndex};
use gpui::{
    App, ClipboardItem, Context, CursorStyle, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyContext, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, Point, Render, ScrollWheelEvent,
    SharedString, Styled as _, Task, Window, div, px,
};

use crate::{EditorSettings, EditorTheme, commands::RunCommand};

mod element;

use element::{FileViewerElement, ViewerLayout};

/// How often a view polls an index that is still building.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

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
    /// the index stopped short of `Complete`, or a read failed. Sent once.
    Changed,
}

/// A read-only view of a [`FileView`], with its rows from `index`.
pub struct FileViewer {
    view: Arc<FileView>,
    index: Arc<LineIndex>,
    /// The snapshot the model holds, whose read errors it records.
    text: FileText,
    pub(crate) model: ViewerModel,
    banner: SharedString,
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
    /// The rows the last frame painted: buffer row, text and window
    /// position.
    #[cfg(test)]
    pub(crate) painted_rows: Vec<(u32, String, Point<Pixels>)>,
}

impl EventEmitter<FileViewerEvent> for FileViewer {}

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
            banner,
            theme,
            focus: cx.focus_handle(),
            scroll: Point::default(),
            autoscroll: false,
            layout: None,
            drag: None,
            poll: None,
            changed_sent: false,
            #[cfg(test)]
            painted_rows: Vec::new(),
        };
        // An index that has already failed is told of on the foreground
        // executor's next turn, once whoever is constructing the view has
        // subscribed to it: an event sent now would reach no one.
        viewer.poll = Some(cx.spawn(async move |this, cx| {
            let mut polling = this.update(cx, |viewer, cx| {
                viewer.check_index(cx);
                if !building {
                    viewer.poll = None;
                }
                building
            });
            while matches!(polling, Ok(true)) {
                cx.background_executor().timer(POLL_INTERVAL).await;
                polling = this.update(cx, |viewer, cx| viewer.refresh(cx));
            }
        }));
        viewer
    }

    pub fn model(&self) -> &ViewerModel {
        &self.model
    }

    pub fn banner(&self) -> &SharedString {
        &self.banner
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
        self.text = FileText::new(self.view.clone(), self.index.clone());
        self.model.refresh(Arc::new(self.text.clone()));
        self.check_index(cx);
        if !building {
            self.poll = None;
        }
        cx.notify();
        building
    }

    /// Sends [`FileViewerEvent::Changed`], once, if the index stopped short
    /// or a read of the text failed. Call after anything that reads it.
    pub(crate) fn check_index(&mut self, cx: &mut Context<Self>) {
        let state = self.index.progress().state;
        let failed = !matches!(state, IndexState::Building | IndexState::Complete);
        if self.changed_sent || !failed && self.text.error().is_none() {
            return;
        }
        self.changed_sent = true;
        cx.emit(FileViewerEvent::Changed);
    }

    /// Runs an editor command on the model. Commands the viewer doesn't
    /// know that aren't the editor's go on to the app.
    fn on_run_command(&mut self, run: &RunCommand, _: &mut Window, cx: &mut Context<Self>) {
        let invocation = &run.0;
        let id = invocation.command.as_str();
        self.model.set_page_rows(self.page_rows());
        match self.model.run(id, &invocation.args) {
            Outcome::Changed => {
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
        cx.notify();
    }
}

impl Render for FileViewer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = &self.theme;
        let mut key_context = KeyContext::default();
        key_context.add(CONTEXT);
        let banner = div()
            .flex_none()
            .px(px(8.))
            .py(px(2.))
            .bg(theme.popover_background)
            .border_b_1()
            .border_color(theme.popover_border)
            .text_color(theme.gutter_active_foreground)
            .text_size(theme.font_size)
            .font_family(theme.font_family.clone())
            .whitespace_nowrap()
            .overflow_hidden()
            .child(self.banner.clone());
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
