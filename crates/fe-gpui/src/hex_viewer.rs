//! [`HexViewer`]: a read-only tab showing a file's bytes as rows of
//! `offset | 16 hex bytes | ASCII`, in the editor theme's mono font
//! (`model.rs` has the row text).
//!
//! It reuses `fe_fileview`: a [`FileView`] reads the bytes, through its
//! chunk cache, with [`FileView::bytes`], and [`fe_fileview::find_bytes`]
//! scans past the cache for a find (`find.rs`, the file viewer's background
//! search copied). It builds no `LineIndex` and uses no `ViewerModel`: row
//! `r` starts at byte `16 * r`, so only the rows on screen are ever read,
//! and the scroll position is a `u64` top row and a pixel fraction of it,
//! never one pixel offset a multi-GiB file would overflow `f32` with.
//!
//! The view is read-only: no edit, no dirty state, nothing to save, and
//! typed text does nothing. It has no minimap, and no `[editor.minimap]`
//! setting gives it one: the pane applies those to editors only.
//!
//! A selection is a run of whole bytes, from an anchor to a head byte, and
//! shows in both columns: the focused column (the one last clicked) in the
//! theme's `selection`, the other in its weaker `identifier_highlight`;
//! the current find match is outlined in `find_match`. Arrows move a byte or
//! a row, Page Up and Down a screen, Home and End to the row's ends, and
//! secondary-up and -down to the file's; shift extends.
//!
//! Its commands ([`registry`], in [`KEY_CONTEXT`]): Go to Offset and Find
//! open an input in the header strip, whose `find_bar` key context gives it
//! the find bar's keys (Enter, Shift-Enter, Escape and the text input's);
//! Copy copies the selection as hex from the hex column and as text from
//! the ASCII one. Like the file viewer it re-stats the file every
//! [`CHECK_INTERVAL`] and on focus, and a changed or unreadable file turns
//! the header into a "changed on disk" banner with Reload. It's a pane
//! item: see `pane_item.rs`.

use std::{ops::Range, path::Path, sync::Arc, time::Duration};

use fe_command::{
    Actor, CommandTarget, ContextFrame, ContextPredicate, ContextStack, Dispatched, Effect,
    Invocation, Registry, Source,
};
use fe_fileview::{Direction, FileView, Freshness};
use fe_ui::{ActiveTheme as _, TextInput, TextInputEvent, ToggleButton};
use gpui::{
    App, AppContext as _, BorderStyle, Bounds, ClipboardItem, Context, CursorStyle, Entity,
    EventEmitter, FocusHandle, Focusable, Hsla, InteractiveElement as _, IntoElement, KeyContext,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _,
    Pixels, Point, Render, ScrollWheelEvent, ShapedLine, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, TextAlign, TextRun, Window,
    WindowId, canvas, div, fill, outline, point, px, size,
};
use kiln_l10n::{tr, verbatim};

use crate::{EditorTheme, commands::RunCommand, pane_item::file_name};

mod find;
pub(crate) mod model;
mod pane_item;

pub use model::Column as HexColumn;
use model::{Column, Columns, ROW_BYTES};

/// The key context of a hex viewer, which its commands run in.
pub const KEY_CONTEXT: &str = verbatim!("HexViewer");

/// Opens the Go to Offset input.
pub(crate) const GO_TO_OFFSET: &str = "hex.go_to_offset";
/// Opens the Find input.
pub(crate) const FIND: &str = "hex.find";
/// Selects the next match.
pub(crate) const FIND_NEXT: &str = "hex.find_next";
/// Selects the previous match.
pub(crate) const FIND_PREVIOUS: &str = "hex.find_previous";
/// Copies the selection as the focused column shows it.
pub(crate) const COPY: &str = "hex.copy";
/// Copies the selection as hex pairs.
pub(crate) const COPY_AS_HEX: &str = "hex.copy_as_hex";
/// Copies the selection as text.
pub(crate) const COPY_AS_TEXT: &str = "hex.copy_as_text";
/// `hex.open_as_text`: the workbench's, which the header's Text button
/// runs.
pub(crate) const OPEN_AS_TEXT: &str = "hex.open_as_text";

/// How often a view checks its file on disk.
const CHECK_INTERVAL: Duration = Duration::from_secs(2);

/// The key context of the header's inputs: the find bar's, so its keys
/// (`find.next` on Enter, `find.previous` on Shift-Enter, `find.close` on
/// Escape, and the text input's editing keys) apply in them.
const INPUT_CONTEXT: &str = "find_bar";

/// The space left of the offsets.
const PAD: Pixels = px(8.);

/// The [`CommandTarget`] of the viewer's commands, which its [`RunCommand`]
/// handler runs: registered so keymaps and the palette know them.
pub(crate) struct HexCommands;

impl CommandTarget for HexCommands {
    type Tx = ();
    type Target<'a> = ();
}

/// The viewer's commands, in the [`KEY_CONTEXT`] context. Their handlers
/// do nothing: the view runs them.
pub(crate) fn registry() -> Registry<HexCommands> {
    let commands = [
        GO_TO_OFFSET,
        FIND,
        FIND_NEXT,
        FIND_PREVIOUS,
        COPY,
        COPY_AS_HEX,
        COPY_AS_TEXT,
    ];
    let mut registry = Registry::new();
    for id in commands {
        let title = crate::l10n::command_title(id);
        registry
            .register(id, &title, |_: &mut (), _: &Actor| {
                Ok(Dispatched::new(Effect::Nothing))
            })
            .expect("hex command IDs are valid and distinct")
            .context(ContextPredicate::identifier(KEY_CONTEXT));
    }
    registry
}

/// What a [`HexViewer`] tells its host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HexViewerEvent {
    /// A copy of `len` bytes was refused: more than
    /// [`fe_editor::COPY_LIMIT`].
    CopyTooLarge { len: u64 },
    /// The file changed on disk, or couldn't be read, since it was opened.
    /// Sent once per change: once more only after a
    /// [`HexViewerEvent::Reloaded`].
    Changed,
    /// [`HexViewer::reload`] reopened the file and shows it now.
    Reloaded,
    /// A search selected a match; `wrapped` if it went round the file.
    Found { wrapped: bool },
    /// A search found no match; the selection stays.
    NotFound,
}

/// The find bar's commands, as the header's inputs take them: see
/// [`HexViewer::run_input_command`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputCommand {
    /// `find.show`: the Find input, its text selected.
    Show,
    /// `find.next` (Enter): go to the offset, or the next match.
    Next,
    /// `find.previous` (Shift-Enter): the previous match.
    Previous,
    /// `find.close` (Escape): closes the input, focusing the rows.
    Close,
    /// `find.toggle_case`: Match Case, for a text query.
    ToggleCase,
}

impl InputCommand {
    /// The command of the find bar's command `id`, if it's one the inputs
    /// take.
    pub fn of(id: &str) -> Option<Self> {
        Some(match id {
            "find.show" => Self::Show,
            "find.next" => Self::Next,
            "find.previous" => Self::Previous,
            "find.close" => Self::Close,
            "find.toggle_case" => Self::ToggleCase,
            _ => return None,
        })
    }
}

/// Which input the header shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Opened {
    GoTo,
    Find,
}

/// How the find input's text reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FindMode {
    /// Pairs of hex digits.
    Hex,
    /// Text, as UTF-8 bytes.
    Text,
}

/// What the last search said, beside the find input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FindSaid {
    Wrapped,
    NotFound,
}

/// The header's inputs, made when one first opens.
struct Inputs {
    go_to: Entity<TextInput>,
    find: Entity<TextInput>,
    _changes: Subscription,
}

/// The file checks of a view in `window`: every [`CHECK_INTERVAL`] and on
/// focus.
struct Watch {
    window: WindowId,
    _checks: Task<()>,
    _focus: Subscription,
}

/// What the last frame laid out, for hit testing and scrolling.
#[derive(Clone, Copy, Debug)]
pub(crate) struct HexLayout {
    /// The rows' area, left of the scrollbar.
    pub(crate) rows: Bounds<Pixels>,
    /// The scrollbar's strip.
    pub(crate) scrollbar: Bounds<Pixels>,
    /// Its thumb, if the rows don't all fit.
    thumb: Option<Bounds<Pixels>>,
    /// A mono cell's width.
    pub(crate) char_width: Pixels,
    pub(crate) row_height: Pixels,
    /// The rows wholly on screen.
    page_rows: u64,
    /// How far the rows are scrolled left.
    scroll_x: Pixels,
}

impl HexLayout {
    /// The x of char `chars` of a row.
    fn x(&self, chars: usize) -> Pixels {
        self.rows.left() + PAD - self.scroll_x + self.char_width * chars as f32
    }
}

/// A row the last frame painted, for tests: its index and its text.
#[cfg(test)]
#[derive(Clone, Debug)]
pub(crate) struct PaintedRow {
    pub(crate) row: u64,
    pub(crate) text: String,
}

/// A read-only view of a [`FileView`]'s bytes.
pub struct HexViewer {
    view: Arc<FileView>,
    pub(crate) theme: EditorTheme,
    focus: FocusHandle,
    /// The selection's ends: whole bytes, both inclusive. Both 0 in an
    /// empty file, which has nothing to select.
    anchor: u64,
    head: u64,
    /// The column that was clicked last: it shows the selection in the
    /// theme's `selection`, and decides what Copy copies.
    column: Column,
    /// The row at the top of the rows' area, and how far it's scrolled up
    /// out of it, less than a row.
    top_row: u64,
    top_px: Pixels,
    /// How far the rows are scrolled left, in a pane narrower than them.
    scroll_x: Pixels,
    /// Scroll the head into view on the next frame.
    autoscroll: bool,
    pub(crate) layout: Option<HexLayout>,
    /// Where a drag selects from, and in which column.
    drag: Option<(u64, Column)>,
    /// While the scrollbar's thumb is dragged: how far below its top it was
    /// pressed.
    grab: Option<Pixels>,
    /// Why to reload, while the file changed or couldn't be read.
    banner: Option<SharedString>,
    /// [`HexViewerEvent::Changed`] was sent.
    changed_sent: bool,
    watch: Option<Watch>,
    reload: Option<Task<()>>,
    inputs: Option<Inputs>,
    opened: Option<Opened>,
    /// Why the Go to Offset input's text isn't an offset.
    go_to_error: Option<SharedString>,
    find_mode: FindMode,
    match_case: bool,
    /// Why the find input's text isn't a query.
    find_error: Option<SharedString>,
    find_said: Option<FindSaid>,
    find: find::FindState,
    /// The rows the last frame painted.
    #[cfg(test)]
    pub(crate) painted: Vec<PaintedRow>,
    /// The byte cells the last frame filled: column, offset and colour.
    #[cfg(test)]
    pub(crate) highlighted: Vec<(Column, u64, Hsla)>,
    /// The font family the last frame painted in.
    #[cfg(test)]
    pub(crate) painted_font: SharedString,
    /// The current find match the last frame outlined.
    #[cfg(test)]
    pub(crate) painted_match: Option<Range<u64>>,
}

impl EventEmitter<HexViewerEvent> for HexViewer {}

impl HexViewer {
    /// A viewer of `view`, the first byte selected.
    pub fn new(view: Arc<FileView>, theme: EditorTheme, cx: &mut Context<Self>) -> Self {
        Self {
            view,
            theme,
            focus: cx.focus_handle(),
            anchor: 0,
            head: 0,
            column: Column::Hex,
            top_row: 0,
            top_px: px(0.),
            scroll_x: px(0.),
            autoscroll: false,
            layout: None,
            drag: None,
            grab: None,
            banner: None,
            changed_sent: false,
            watch: None,
            reload: None,
            inputs: None,
            opened: None,
            go_to_error: None,
            find_mode: FindMode::Hex,
            match_case: false,
            find_error: None,
            find_said: None,
            find: Default::default(),
            #[cfg(test)]
            painted: Vec::new(),
            #[cfg(test)]
            highlighted: Vec::new(),
            #[cfg(test)]
            painted_font: SharedString::default(),
            #[cfg(test)]
            painted_match: None,
        }
    }

    /// The file the viewer shows.
    pub fn view(&self) -> &Arc<FileView> {
        &self.view
    }

    pub fn path(&self) -> &Path {
        self.view.path()
    }

    /// The selected bytes: at least the head's, but none in an empty file.
    pub fn selected(&self) -> Range<u64> {
        if self.view.is_empty() {
            return 0..0;
        }
        self.anchor.min(self.head)..self.anchor.max(self.head) + 1
    }

    /// The selection's ends: the anchor and the head byte.
    pub fn selection_ends(&self) -> (u64, u64) {
        (self.anchor, self.head)
    }

    /// The column that has the selection.
    pub fn column(&self) -> HexColumn {
        self.column
    }

    /// The row at the top of the view.
    pub fn top_row(&self) -> u64 {
        self.top_row
    }

    /// The rows' area in the last frame: left of the scrollbar, below the
    /// header.
    pub fn rows_bounds(&self) -> Option<Bounds<Pixels>> {
        self.layout.map(|layout| layout.rows)
    }

    /// The scrollbar's strip in the last frame.
    pub fn scrollbar_bounds(&self) -> Option<Bounds<Pixels>> {
        self.layout.map(|layout| layout.scrollbar)
    }

    /// What the header's banner says while the file changed or couldn't
    /// be read, which offers a reload.
    pub fn banner(&self) -> Option<&SharedString> {
        self.banner.as_ref()
    }

    /// Whether the banner offers a reload.
    pub fn offers_reload(&self) -> bool {
        self.banner.is_some()
    }

    /// Selects from byte `anchor` to byte `head`, both clipped to the file,
    /// and scrolls the head into view.
    pub fn select(&mut self, anchor: u64, head: u64) {
        let last = self.view.len().saturating_sub(1);
        self.anchor = anchor.min(last);
        self.head = head.min(last);
        self.autoscroll = true;
    }

    /// Puts back a selection and top row, clipped to the file.
    pub(crate) fn put_back(&mut self, anchor: u64, head: u64, top_row: Option<u64>) {
        self.select(anchor, head);
        if let Some(top) = top_row {
            self.top_row = top.min(model::row_count(self.view.len()).saturating_sub(1));
            self.top_px = px(0.);
            self.autoscroll = false;
        }
    }

    /// The status bar's cursor text: `Offset 0x1A2F`, or with more than a
    /// byte selected, its first and last and how many.
    pub(crate) fn cursor_text(&self) -> SharedString {
        let selected = self.selected();
        if selected.end - selected.start <= 1 {
            return tr!(
                "gpui-hex-status-offset",
                offset = model::hex_offset(self.head)
            );
        }
        tr!(
            "gpui-hex-status-range",
            start = model::hex_offset(selected.start),
            end = model::hex_offset(selected.end - 1),
            count = selected.end - selected.start
        )
    }

    fn file_name(&self) -> String {
        file_name(Some(self.view.path()))
    }

    /// Offers a reload in the banner and sends [`HexViewerEvent::Changed`],
    /// once per change.
    pub(crate) fn mark_changed(&mut self, cx: &mut Context<Self>) {
        if self.changed_sent {
            return;
        }
        self.changed_sent = true;
        self.banner = Some(tr!("gpui-hex-changed", name = self.file_name()));
        cx.emit(HexViewerEvent::Changed);
        cx.notify();
    }

    /// Re-stats the file, and says whether to check again: until it's
    /// found changed.
    fn check_disk(&mut self, cx: &mut Context<Self>) -> bool {
        if self.changed_sent {
            return false;
        }
        if self.view.check() == Freshness::Changed {
            self.mark_changed(cx);
            return false;
        }
        true
    }

    /// Checks the file every [`CHECK_INTERVAL`] while the view is in
    /// `window`, and when it gets focus there.
    fn watch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = window.window_handle().window_id();
        if self.changed_sent || self.watch.as_ref().is_some_and(|w| w.window == id) {
            return;
        }
        let checks = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(CHECK_INTERVAL).await;
                // Through the window, so it fails once the window is gone.
                let checking =
                    cx.update(|_, cx| this.update(cx, |viewer, cx| viewer.check_disk(cx)));
                if !matches!(checking, Ok(Ok(true))) {
                    break;
                }
            }
        });
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

    /// Whether a [`HexViewer::reload`] is reopening the file.
    pub fn is_reloading(&self) -> bool {
        self.reload.is_some()
    }

    /// Reopens the file off the main thread and shows it, keeping the
    /// selection, clipped, and the top row; then sends
    /// [`HexViewerEvent::Reloaded`]. If it can't be opened, the banner says
    /// so and still offers a reload.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.find.stop();
        let path = self.view.path().to_owned();
        let opening = cx.background_spawn(async move { FileView::open(&path) });
        self.reload = Some(cx.spawn(async move |this, cx| {
            let opened = opening.await;
            this.update(cx, |viewer, cx| viewer.reopened(opened, cx))
                .ok();
        }));
    }

    fn reopened(&mut self, opened: std::io::Result<FileView>, cx: &mut Context<Self>) {
        self.reload = None;
        let view = match opened {
            Ok(view) => view,
            Err(error) => {
                self.mark_changed(cx);
                let (name, error) = (self.file_name(), error.to_string());
                self.banner = Some(tr!("gpui-hex-cannot-read", name = name, error = error));
                cx.notify();
                return;
            }
        };
        let (anchor, head, top) = (self.anchor, self.head, self.top_row);
        // A search started since the reload began scans the old file.
        self.find.stop();
        self.view = Arc::new(view);
        self.find.current = None;
        self.put_back(anchor, head, Some(top));
        self.banner = None;
        self.changed_sent = false;
        self.watch = None;
        self.drag = None;
        cx.emit(HexViewerEvent::Reloaded);
        cx.notify();
    }

    /// Rows wholly on screen in the last frame, or 20 before one.
    fn page_rows(&self) -> u64 {
        self.layout.map_or(20, |layout| layout.page_rows.max(1))
    }

    /// The top row that shows the file's last row at the bottom.
    fn max_top_row(&self) -> u64 {
        model::row_count(self.view.len()).saturating_sub(self.page_rows())
    }

    /// Scrolls by `delta` pixels, down for a positive one, through the top
    /// row.
    fn scroll_by(&mut self, delta: Pixels) {
        let row_height = self.layout.map_or(px(20.), |layout| layout.row_height);
        let at = self.top_px + delta;
        let rows = (at / row_height).floor();
        let mut top = self.top_row as i128 + rows as i128;
        let mut top_px = at - row_height * rows;
        let max = self.max_top_row() as i128;
        if top < 0 {
            (top, top_px) = (0, px(0.));
        }
        if top >= max {
            (top, top_px) = (max, px(0.));
        }
        self.top_row = top as u64;
        self.top_px = top_px;
    }

    /// Moves the head to `head`, clipped to the file; `extend` keeps the
    /// anchor, else it follows.
    fn move_to(&mut self, head: u64, extend: bool, cx: &mut Context<Self>) {
        if self.view.is_empty() {
            return;
        }
        let last = self.view.len() - 1;
        self.head = head.min(last);
        if !extend {
            self.anchor = self.head;
        }
        self.autoscroll = true;
        self.find_said = None;
        cx.notify();
    }

    /// The navigation keys, and typed text, which does nothing: see the
    /// module docs. Only while the rows have focus, not an input.
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.focus.is_focused(window) {
            return;
        }
        let keystroke = &event.keystroke;
        let held = keystroke.modifiers;
        let (extend, secondary) = (held.shift, held.secondary());
        let head = self.head;
        let row_start = head - head % ROW_BYTES;
        let page = self.page_rows() * ROW_BYTES;
        let target = match (keystroke.key.as_str(), secondary) {
            ("left", false) => Some(head.saturating_sub(1)),
            ("right", false) => Some(head + 1),
            ("up", false) => Some(head.checked_sub(ROW_BYTES).unwrap_or(head)),
            ("down", false) => {
                let below = head + ROW_BYTES;
                // From the row above a partial last row, to its last byte.
                let last_row = model::row_count(self.view.len()).saturating_sub(1);
                (below / ROW_BYTES <= last_row)
                    .then_some(below)
                    .or(Some(head))
            }
            ("pageup", false) => {
                self.scroll_by(
                    -(self.layout.map_or(px(20.), |l| l.row_height)) * self.page_rows() as f32,
                );
                Some(head.saturating_sub(page))
            }
            ("pagedown", false) => {
                self.scroll_by(
                    self.layout.map_or(px(20.), |l| l.row_height) * self.page_rows() as f32,
                );
                Some(head.saturating_add(page))
            }
            ("home", false) => Some(row_start),
            ("end", false) => Some(row_start + ROW_BYTES - 1),
            ("up", true) | ("home", true) => Some(0),
            ("down", true) | ("end", true) => Some(u64::MAX),
            _ => None,
        };
        if let Some(target) = target {
            self.move_to(target, extend, cx);
            cx.stop_propagation();
            return;
        }
        // A typed char, which no binding took, would edit: nothing does.
        if keystroke.key_char.is_some() && !held.platform && !held.control {
            cx.stop_propagation();
        }
    }

    /// The byte under a window position, and its column, in the last
    /// frame: clamped to the rows shown and the file.
    fn byte_at(&self, position: Point<Pixels>) -> Option<(u64, Column)> {
        let layout = self.layout?;
        if self.view.is_empty() {
            return None;
        }
        let y = (position.y - layout.rows.top() + self.top_px).max(px(0.));
        let row = self.top_row + (y / layout.row_height).floor() as u64;
        let columns = Columns::for_len(self.view.len());
        let x = (position.x - layout.x(0)) / layout.char_width;
        let (column, ix) = columns.hit(x);
        let last = self.view.len() - 1;
        Some(((row * ROW_BYTES + ix as u64).min(last), column))
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        let Some(layout) = self.layout else {
            return;
        };
        if layout.scrollbar.contains(&event.position) {
            self.press_scrollbar(event.position, layout);
            cx.notify();
            return;
        }
        let Some((offset, column)) = self.byte_at(event.position) else {
            return;
        };
        self.find_said = None;
        let anchor = if event.modifiers.shift {
            self.anchor
        } else {
            offset
        };
        self.column = column;
        self.anchor = anchor;
        self.head = offset;
        self.drag = Some((anchor, column));
        self.autoscroll = true;
        cx.notify();
    }

    /// A press in the scrollbar: on the thumb it grabs it, elsewhere the
    /// thumb jumps to centre there, and is grabbed.
    fn press_scrollbar(&mut self, position: Point<Pixels>, layout: HexLayout) {
        let Some(thumb) = layout.thumb else {
            return;
        };
        if thumb.contains(&position) {
            self.grab = Some(position.y - thumb.top());
            return;
        }
        let grab = thumb.size.height / 2.;
        self.grab = Some(grab);
        self.drag_thumb(position.y - grab, layout);
    }

    /// Scrolls so the thumb's top is at `top`.
    fn drag_thumb(&mut self, top: Pixels, layout: HexLayout) {
        let Some(thumb) = layout.thumb else {
            return;
        };
        let room = layout.scrollbar.size.height - thumb.size.height;
        let max = self.max_top_row();
        if room <= px(0.) || max == 0 {
            return;
        }
        let at = f64::from(((top - layout.scrollbar.top()) / room).clamp(0., 1.));
        self.top_row = ((max as f64) * at).round() as u64;
        self.top_px = px(0.);
        self.autoscroll = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let held = event.pressed_button == Some(MouseButton::Left);
        if let (Some(grab), Some(layout)) = (self.grab, self.layout) {
            if !held {
                self.grab = None;
                return;
            }
            self.drag_thumb(event.position.y - grab, layout);
            cx.notify();
            return;
        }
        let Some((anchor, column)) = self.drag else {
            return;
        };
        if !held {
            self.drag = None;
            return;
        }
        if let Some((offset, _)) = self.byte_at(event.position) {
            self.column = column;
            self.anchor = anchor;
            self.head = offset;
            self.autoscroll = true;
            cx.notify();
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.drag = None;
        if self.grab.take().is_some() {
            cx.notify();
        }
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let row_height = self.layout.map_or(px(20.), |layout| layout.row_height);
        let delta = event.delta.pixel_delta(row_height);
        self.scroll_by(-delta.y);
        // The frame clamps it to the row's width.
        self.scroll_x = (self.scroll_x - delta.x).max(px(0.));
        self.autoscroll = false;
        cx.notify();
    }

    /// Copies the selection: as hex pairs (`hex`), or as text, decoded
    /// lossily. More than [`fe_editor::COPY_LIMIT`] bytes is refused.
    fn copy(&mut self, hex: bool, cx: &mut Context<Self>) {
        let selected = self.selected();
        let len = selected.end - selected.start;
        if len == 0 {
            return;
        }
        if len > fe_editor::COPY_LIMIT as u64 {
            cx.emit(HexViewerEvent::CopyTooLarge { len });
            return;
        }
        let bytes = match self.view.bytes(selected) {
            Ok(bytes) => bytes,
            Err(_) => return self.mark_changed(cx),
        };
        let text = match hex {
            true => model::hex_pairs(&bytes),
            false => String::from_utf8_lossy(&bytes).into_owned(),
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    /// Runs the viewer's commands, the find bar's in its inputs, and the
    /// inputs' editing commands; any other goes on to the app.
    fn on_run_command(&mut self, run: &RunCommand, window: &mut Window, cx: &mut Context<Self>) {
        match run.0.command.as_str() {
            GO_TO_OFFSET => self.open_input(Opened::GoTo, window, cx),
            FIND => self.open_input(Opened::Find, window, cx),
            FIND_NEXT => self.find_next(cx),
            FIND_PREVIOUS => self.find_prev(cx),
            // An input's own text isn't the rows' bytes: nothing to copy
            // there.
            COPY | COPY_AS_HEX | COPY_AS_TEXT if self.focused_input(window, cx).is_some() => {}
            COPY => self.copy(self.column == Column::Hex, cx),
            COPY_AS_HEX => self.copy(true, cx),
            COPY_AS_TEXT => self.copy(false, cx),
            id if InputCommand::of(id)
                .is_some_and(|command| self.run_input_command(command, window, cx)) => {}
            _ => {
                let edit = kiln_shell::picker::op(run, cx).and_then(|op| op.edit());
                match (edit, self.focused_input(window, cx)) {
                    (Some(edit), Some(input)) => input.update(cx, edit),
                    _ => cx.propagate(),
                }
            }
        }
    }

    /// Runs `command`, one of the find bar's keys, in the header input that
    /// has focus. The workbench dispatches those keys as its own typed
    /// commands, so its pane hands them here; a host without them sends
    /// them as [`RunCommand`]s. Returns whether an input had focus: if
    /// not, the command isn't the viewer's.
    pub fn run_input_command(
        &mut self,
        command: InputCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.focused_input(window, cx).is_none() {
            return false;
        }
        match (command, self.opened) {
            (InputCommand::Next, Some(Opened::GoTo)) => self.go_to(window, cx),
            (InputCommand::Next, _) => self.find_next(cx),
            (InputCommand::Previous, Some(Opened::Find)) => self.find_prev(cx),
            (InputCommand::Close, _) => self.close_input(window, cx),
            (InputCommand::ToggleCase, Some(Opened::Find)) => self.toggle_match_case(cx),
            (InputCommand::Show, _) => self.open_input(Opened::Find, window, cx),
            _ => {}
        }
        true
    }

    /// Whether one of the header's inputs has focus.
    pub fn input_focused(&self, window: &Window, cx: &App) -> bool {
        self.focused_input(window, cx).is_some()
    }

    /// The input that has focus, if one does.
    fn focused_input(&self, window: &Window, cx: &App) -> Option<Entity<TextInput>> {
        let inputs = self.inputs.as_ref()?;
        [&inputs.go_to, &inputs.find]
            .into_iter()
            .find(|input| input.focus_handle(cx).is_focused(window))
            .cloned()
    }

    /// The header's inputs, made the first time.
    fn inputs(&mut self, cx: &mut Context<Self>) -> &Inputs {
        if self.inputs.is_none() {
            let ui = fe_ui::ui_theme(cx);
            let name = tr!("gpui-hex-go-to-offset");
            let go_to = cx.new(|cx| {
                TextInput::new(kiln_l10n::text!("gpui-hex-go-to-placeholder"), &ui, cx).name(name)
            });
            let name = tr!("gpui-hex-find");
            let find = cx.new(|cx| {
                TextInput::new(kiln_l10n::text!("gpui-hex-find-placeholder"), &ui, cx)
                    .search()
                    .name(name)
            });
            let changes = cx.subscribe(&find, |viewer, _, event, cx| {
                if *event == TextInputEvent::Changed {
                    viewer.query_changed(cx);
                }
            });
            self.inputs = Some(Inputs {
                go_to,
                find,
                _changes: changes,
            });
        }
        self.inputs.as_ref().expect("made above")
    }

    /// Opens `which` input in the header, its text selected, with focus.
    pub(crate) fn open_input(
        &mut self,
        which: Opened,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let inputs = self.inputs(cx);
        let input = match which {
            Opened::GoTo => inputs.go_to.clone(),
            Opened::Find => inputs.find.clone(),
        };
        if which == Opened::GoTo {
            self.go_to_error = None;
            // The find input hides: its search stops, as on closing it.
            self.find.stop();
        }
        self.opened = Some(which);
        input.update(cx, |input, cx| input.select_all(cx));
        window.focus(&input.focus_handle(cx), cx);
        cx.notify();
    }

    /// Closes the header's input and focuses the rows. A search running
    /// stops.
    pub(crate) fn close_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.opened = None;
        self.go_to_error = None;
        self.find.stop();
        window.focus(&self.focus, cx);
        cx.notify();
    }

    #[cfg(test)]
    /// Which input the header shows.
    pub(crate) fn opened(&self) -> Option<Opened> {
        self.opened
    }

    /// Goes to the offset the Go to Offset input names: selects its byte,
    /// shows it, closes the input and focuses the rows. One past the end or
    /// that doesn't parse shows why, and keeps the selection.
    fn go_to(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(inputs) = &self.inputs else {
            return;
        };
        let text = inputs.go_to.read(cx).text().to_owned();
        match model::parse_offset(&text, self.view.len()) {
            Ok(offset) => {
                self.select(offset, offset);
                self.close_input(window, cx);
            }
            Err(model::OffsetError::Unparsed) => {
                self.go_to_error = Some(tr!("gpui-hex-go-to-invalid", text = text.trim()));
                cx.notify();
            }
            Err(model::OffsetError::PastEnd { len }) => {
                self.go_to_error = Some(tr!(
                    "gpui-hex-go-to-past-end",
                    offset = text.trim(),
                    last = model::hex_offset(len.saturating_sub(1))
                ));
                cx.notify();
            }
        }
    }

    #[cfg(test)]
    /// Why Go to Offset's text isn't an offset, while it says.
    pub(crate) fn go_to_error(&self) -> Option<&SharedString> {
        self.go_to_error.as_ref()
    }

    #[cfg(test)]
    /// Why the find input's text isn't a query, while it says.
    pub(crate) fn find_error(&self) -> Option<&SharedString> {
        self.find_error.as_ref()
    }

    #[cfg(test)]
    /// What the last search said: wrapped, or nothing found.
    pub(crate) fn find_said(&self) -> Option<FindSaid> {
        self.find_said
    }

    /// Reads the find input as a query and searches for it from the
    /// selection. A query that doesn't parse clears it and says why.
    fn query_changed(&mut self, cx: &mut Context<Self>) {
        let Some(inputs) = &self.inputs else {
            return;
        };
        let text = inputs.find.read(cx).text().to_owned();
        self.find_error = None;
        self.find_said = None;
        let query = match self.find_mode {
            FindMode::Hex => match model::parse_hex_query(&text) {
                Ok(needle) => Some(find::ByteQuery {
                    needle,
                    case_sensitive: true,
                }),
                Err(model::HexQueryError::OddDigits) => {
                    self.find_error = Some(tr!("gpui-hex-find-odd"));
                    None
                }
                Err(model::HexQueryError::NotHex(ch)) => {
                    self.find_error = Some(tr!("gpui-hex-find-not-hex", digit = ch.to_string()));
                    None
                }
            },
            FindMode::Text => Some(find::ByteQuery {
                needle: text.into_bytes(),
                case_sensitive: self.match_case,
            }),
        };
        let query = query.filter(|query| !query.needle.is_empty());
        let searching = query.is_some();
        self.set_find(query, cx);
        if searching {
            self.search(Direction::Forward, true, cx);
        }
    }

    /// Reads the find input as hex pairs, or as text.
    pub(crate) fn set_find_mode(&mut self, mode: FindMode, cx: &mut Context<Self>) {
        if self.find_mode != mode {
            self.find_mode = mode;
            self.query_changed(cx);
        }
    }

    /// Turns Match Case, for a text query, on or off.
    pub(crate) fn toggle_match_case(&mut self, cx: &mut Context<Self>) {
        self.match_case = !self.match_case;
        if self.find_mode == FindMode::Text {
            self.query_changed(cx);
        }
        cx.notify();
    }

    /// The header strip: the banner or the file's size, the open input and
    /// what it says, and the Text button.
    fn header(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = &self.theme;
        let ui = fe_ui::ui_theme(cx);
        let line_height = (theme.font_size * theme.line_height).round();
        // What the last search said: beside the Find input while it's open,
        // else in the header's info, so a search from the rows' keys says it
        // too.
        let found_said = self.find_said.map(|said| match said {
            FindSaid::Wrapped => (tr!("gpui-hex-find-wrapped"), false),
            FindSaid::NotFound => (tr!("gpui-hex-find-not-found"), true),
        });
        let said_element = |text: SharedString, error: bool| {
            div()
                .debug_selector(|| verbatim!("hex-viewer-said").into())
                .min_w_0()
                .overflow_hidden()
                .text_color(if error {
                    theme.error
                } else {
                    theme.gutter_foreground
                })
                .child(text)
        };
        let info = match (&self.banner, self.opened, found_said.clone()) {
            (Some(banner), ..) => div().child(banner.clone()),
            (None, None, Some((text, error))) => said_element(text, error),
            _ => div().child(tr!("gpui-hex-size", bytes = self.view.len())),
        };
        let reload = self.banner.is_some().then(|| {
            button(
                theme,
                verbatim!("hex-viewer-reload"),
                tr!("gpui-hex-reload"),
            )
            .on_click(cx.listener(|viewer, _, _, cx| viewer.reload(cx)))
        });
        let input = self.opened.and_then(|opened| {
            let inputs = self.inputs.as_ref()?;
            let (input, said) = match opened {
                Opened::GoTo => (
                    inputs.go_to.clone(),
                    self.go_to_error.clone().map(|e| (e, true)),
                ),
                Opened::Find => {
                    let said = match &self.find_error {
                        Some(error) => Some((error.clone(), true)),
                        None => found_said.clone(),
                    };
                    (inputs.find.clone(), said)
                }
            };
            let said = said.map(|(text, error)| said_element(text, error).flex_shrink(1.));
            let toggles = (opened == Opened::Find).then(|| {
                let mode = self.find_mode;
                let options =
                    [
                        (FindMode::Hex, tr!("gpui-hex-find-hex")),
                        (FindMode::Text, tr!("gpui-hex-find-text")),
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(ix, (which, label))| {
                        let selector = match which {
                            FindMode::Hex => verbatim!("hex-find-mode-hex"),
                            FindMode::Text => verbatim!("hex-find-mode-text"),
                        };
                        div().debug_selector(move || selector.into()).child(
                            ToggleButton::new(("hex-find-mode", ix), label.clone(), label, &ui)
                                .on(mode == which)
                                .on_click(cx.listener(move |viewer, _, _, cx| {
                                    viewer.set_find_mode(which, cx)
                                })),
                        )
                    });
                let case = ToggleButton::new(
                    "hex-find-case",
                    verbatim!("Aa"),
                    tr!("gpui-hex-match-case"),
                    &ui,
                )
                .on(self.match_case)
                .disabled(mode == FindMode::Hex)
                .on_click(cx.listener(|viewer, _, _, cx| viewer.toggle_match_case(cx)));
                div()
                    .flex()
                    .flex_none()
                    .gap_0p5()
                    // A toggle keeps focus in the input.
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                    .children(options)
                    .child(case)
            });
            // Shrinks, its message first, so the buttons stay in a narrow
            // pane.
            Some(
                div()
                    .key_context(INPUT_CONTEXT)
                    .flex()
                    .flex_shrink(1.)
                    .min_w_0()
                    .overflow_hidden()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex_shrink(1.)
                            .min_w(px(80.))
                            .w(px(220.))
                            .border_1()
                            .border_color(theme.popover_border)
                            .rounded(px(3.))
                            .child(input),
                    )
                    .children(toggles)
                    .children(said),
            )
        });
        let focus = self.focus.clone();
        let text = button(
            theme,
            verbatim!("hex-viewer-open-as-text"),
            tr!("gpui-hex-open-as-text"),
        )
        .on_click(move |_, window, cx| {
            window.focus(&focus, cx);
            // Invoked in the viewer, which the command applies in.
            let context = ContextStack::new().with(ContextFrame::new(KEY_CONTEXT));
            let invocation = Invocation::of(OPEN_AS_TEXT)
                .expect("a valid command ID")
                .source(Source::Mouse)
                .context(context);
            window.dispatch_action(Box::new(RunCommand(invocation)), cx);
        });
        div()
            .id("hex-viewer-header")
            .debug_selector(|| verbatim!("hex-viewer-header").into())
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .px(PAD)
            .py(px(2.))
            .min_h(line_height + px(8.))
            .bg(theme.popover_background)
            .border_b_1()
            .border_color(theme.popover_border)
            .text_color(theme.gutter_active_foreground)
            .text_size(theme.font_size)
            .font_family(theme.font_family.clone())
            .whitespace_nowrap()
            .overflow_hidden()
            .children(input)
            .child(
                div()
                    .debug_selector(|| verbatim!("hex-viewer-banner").into())
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .child(info),
            )
            .children(reload)
            .child(text)
    }

    /// Lays out the rows in `bounds`, the rows' area and the scrollbar,
    /// reading the bytes on screen, and what to paint.
    fn prepaint(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Frame {
        let theme = self.theme.clone();
        let font = gpui::Font {
            features: theme.font_features.clone(),
            ..gpui::font(theme.font_family.clone())
        };
        let font_size = theme.font_size;
        let row_height = (font_size * theme.line_height).round().max(px(1.));
        let text_system = window.text_system().clone();
        let font_id = text_system.resolve_font(&font);
        let char_width = text_system
            .advance(font_id, font_size, '0')
            .map(|advance| advance.width)
            .or_else(|_| text_system.em_advance(font_id, font_size))
            .unwrap_or(font_size * 0.6);
        let scrollbar_width = fe_ui::SCROLLBAR_WIDTH;
        let rows = Bounds::new(
            bounds.origin,
            size(
                (bounds.size.width - scrollbar_width).max(px(0.)),
                bounds.size.height,
            ),
        );
        let scrollbar = Bounds::new(
            point(rows.right(), bounds.top()),
            size(scrollbar_width, bounds.size.height),
        );
        let page_rows = ((rows.size.height / row_height).floor() as u64).max(1);
        let len = self.view.len();
        let total = model::row_count(len);
        let mut layout = HexLayout {
            rows,
            scrollbar,
            thumb: None,
            char_width,
            row_height,
            page_rows,
            scroll_x: self.scroll_x,
        };
        self.layout = Some(layout);
        let columns = Columns::for_len(len);
        // Sideways, in a pane narrower than a row: the head's cell in view,
        // and no further than the row's end.
        let row_width = PAD * 2. + char_width * columns.row_chars() as f32;
        let max_x = (row_width - rows.size.width).max(px(0.));
        if self.autoscroll && len > 0 {
            let cell = columns.cell(self.column, (self.head % ROW_BYTES) as usize);
            let left = PAD + char_width * cell.start as f32;
            let right = PAD + char_width * cell.end as f32;
            if left < self.scroll_x + PAD {
                self.scroll_x = left - PAD;
            } else if right + PAD > self.scroll_x + rows.size.width {
                self.scroll_x = right + PAD - rows.size.width;
            }
        }
        self.scroll_x = self.scroll_x.clamp(px(0.), max_x);
        layout.scroll_x = self.scroll_x;
        let max_top = total.saturating_sub(page_rows);
        if self.autoscroll && len > 0 {
            let head_row = self.head / ROW_BYTES;
            if head_row < self.top_row || (head_row == self.top_row && self.top_px > px(0.)) {
                self.top_row = head_row;
                self.top_px = px(0.);
            } else {
                // Its bottom edge, below the rows' area's top: past the
                // area's height, it's cut off.
                let bottom = row_height * (head_row - self.top_row + 1) as f32 - self.top_px;
                if bottom > rows.size.height {
                    self.top_row = (head_row + 1).saturating_sub(page_rows);
                    self.top_px = px(0.);
                }
            }
        }
        self.autoscroll = false;
        if self.top_row >= max_top {
            self.top_row = max_top;
            self.top_px = px(0.);
        }
        layout.thumb = thumb_bounds(scrollbar, self.top_row, max_top, page_rows, total);
        self.layout = Some(layout);

        let shown = (((rows.size.height + self.top_px) / row_height).ceil() as u64).max(1);
        let first = self.top_row;
        let end_row = (first + shown).min(total);
        let start = first * ROW_BYTES;
        let bytes = match self.view.bytes(start..end_row * ROW_BYTES) {
            Ok(bytes) => bytes,
            Err(_) => {
                self.mark_changed(cx);
                Vec::new()
            }
        };
        let run = |len: usize, color: Hsla| TextRun {
            len,
            font: font.clone(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let focused = self.focus.is_focused(window);
        let selected = self.selected();
        let (strong, weak) = (theme.selection, theme.identifier_highlight);
        let found = self.find.current.clone().filter(|found| *found == selected);
        let mut quads = Vec::new();
        let mut lines = Vec::new();
        #[cfg(test)]
        {
            self.painted.clear();
            self.highlighted.clear();
            self.painted_match = None;
            self.painted_font = theme.font_family.clone();
        }
        if len == 0 {
            let text: SharedString = tr!("gpui-hex-empty");
            let line = text_system.shape_line(
                text.clone(),
                font_size,
                &[run(text.len(), theme.gutter_foreground)],
                None,
            );
            let origin = point(layout.x(0), rows.top());
            #[cfg(test)]
            self.painted.push(PaintedRow {
                row: 0,
                text: text.to_string(),
            });
            lines.push((origin, line));
        }
        for (ix, chunk) in bytes.chunks(ROW_BYTES as usize).enumerate() {
            let row = first + ix as u64;
            let offset = row * ROW_BYTES;
            let top = rows.top() - self.top_px + row_height * ix as f32;
            let text = model::row_text(columns, offset, chunk);
            // Each selected byte's cells, run on to the next selected one in
            // the hex column so a run of them reads as one.
            for (byte_ix, _) in chunk.iter().enumerate() {
                let at = offset + byte_ix as u64;
                if !selected.contains(&at) {
                    continue;
                }
                for column in [Column::Hex, Column::Ascii] {
                    let color = if column == self.column { strong } else { weak };
                    let cell = columns.cell(column, byte_ix);
                    let joined = column == Column::Hex
                        && byte_ix < 15
                        && selected.contains(&(at + 1))
                        && byte_ix + 1 < chunk.len();
                    let end = match joined {
                        true => columns.cell(column, byte_ix + 1).start,
                        false => cell.end,
                    };
                    let cell_bounds = Bounds::from_corners(
                        point(layout.x(cell.start), top),
                        point(layout.x(end), top + row_height),
                    );
                    quads.push(fill(cell_bounds, color));
                    #[cfg(test)]
                    self.highlighted.push((column, at, color));
                }
            }
            if let Some(found) = &found {
                #[cfg(test)]
                {
                    self.painted_match = Some(found.clone());
                }
                for column in [Column::Hex, Column::Ascii] {
                    let from = found.start.max(offset);
                    let to = found.end.min(offset + chunk.len() as u64);
                    if from >= to {
                        continue;
                    }
                    let first_cell = columns.cell(column, (from - offset) as usize);
                    let last_cell = columns.cell(column, (to - 1 - offset) as usize);
                    quads.push(outline(
                        Bounds::from_corners(
                            point(layout.x(first_cell.start), top),
                            point(layout.x(last_cell.end), top + row_height),
                        ),
                        theme.find_match,
                        BorderStyle::Solid,
                    ));
                }
            }
            if focused && (offset..offset + chunk.len() as u64).contains(&self.head) {
                let cell = columns.cell(self.column, (self.head - offset) as usize);
                quads.push(fill(
                    Bounds::from_corners(
                        point(layout.x(cell.start), top + row_height - px(2.)),
                        point(layout.x(cell.end), top + row_height),
                    ),
                    theme.cursor,
                ));
            }
            let hex_len = columns.ascii_start();
            let line = text_system.shape_line(
                text.clone().into(),
                font_size,
                &[
                    run(columns.offset_digits, theme.gutter_foreground),
                    run(hex_len - columns.offset_digits, theme.foreground),
                    run(text.len() - hex_len, theme.foreground),
                ],
                None,
            );
            let origin = point(layout.x(0), top);
            #[cfg(test)]
            self.painted.push(PaintedRow { row, text });
            lines.push((origin, line));
        }
        let thumb = layout.thumb.map(|thumb| {
            let colors = cx.theme().colors.scrollbar;
            let color = if self.grab.is_some() {
                colors.thumb_hover
            } else {
                colors.thumb
            };
            fill(thumb, color).corner_radii(px(3.))
        });
        Frame {
            rows,
            lines,
            quads,
            thumb,
            row_height,
        }
    }
}

/// What a frame paints, from [`HexViewer::prepaint`].
struct Frame {
    rows: Bounds<Pixels>,
    lines: Vec<(Point<Pixels>, ShapedLine)>,
    quads: Vec<gpui::PaintQuad>,
    thumb: Option<gpui::PaintQuad>,
    row_height: Pixels,
}

/// The scrollbar's thumb in `track` for rows from `top` of `max` (the top
/// row showing the last), `page` of `total` on screen; `None` if they all
/// fit.
fn thumb_bounds(
    track: Bounds<Pixels>,
    top: u64,
    max: u64,
    page: u64,
    total: u64,
) -> Option<Bounds<Pixels>> {
    if max == 0 || track.size.height <= px(0.) {
        return None;
    }
    let viewport = track.size.height;
    let share = (page as f64 / total as f64) as f32;
    let height = (viewport * share).max(px(20.)).min(viewport);
    let at = (top as f64 / max as f64).clamp(0., 1.) as f32;
    let y = track.top() + (viewport - height) * at;
    Some(Bounds::new(
        point(track.left() + px(1.), y),
        size(track.size.width - px(2.), height),
    ))
}

/// A header button, as the image viewer's Text button.
fn button(theme: &EditorTheme, id: &'static str, label: SharedString) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .debug_selector(move || id.into())
        .flex_none()
        .px(px(8.))
        .rounded(px(4.))
        .cursor(CursorStyle::PointingHand)
        .bg(theme.background)
        .border_1()
        .border_color(theme.gutter_foreground)
        .text_color(theme.gutter_foreground)
        .child(label)
}

impl Focusable for HexViewer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for HexViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.watch(window, cx);
        let theme = self.theme.clone();
        let mut key_context = KeyContext::default();
        key_context.add(KEY_CONTEXT);
        let header = self.header(cx);
        let viewer = cx.entity();
        let rows = div()
            .id("hex-viewer-rows")
            .flex_1()
            .min_h_0()
            .relative()
            .overflow_hidden()
            .cursor(CursorStyle::IBeam)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .child(
                canvas(
                    move |bounds, window, cx| {
                        viewer.update(cx, |viewer, cx| viewer.prepaint(bounds, window, cx))
                    },
                    move |_, frame, window, cx| {
                        window.with_content_mask(
                            Some(gpui::ContentMask { bounds: frame.rows }),
                            |window| {
                                for quad in frame.quads {
                                    window.paint_quad(quad);
                                }
                                for (origin, line) in &frame.lines {
                                    line.paint(
                                        *origin,
                                        frame.row_height,
                                        TextAlign::Left,
                                        None,
                                        window,
                                        cx,
                                    )
                                    .ok();
                                }
                            },
                        );
                        if let Some(thumb) = frame.thumb {
                            window.paint_quad(thumb);
                        }
                    },
                )
                .size_full(),
            );
        div()
            .id("hex-viewer")
            .key_context(key_context)
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.background)
            .on_action(cx.listener(Self::on_run_command))
            .on_key_down(cx.listener(Self::on_key_down))
            .child(header)
            .child(rows)
    }
}

#[cfg(test)]
mod model_tests;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod find_tests;

#[cfg(test)]
mod pane_item_tests;
