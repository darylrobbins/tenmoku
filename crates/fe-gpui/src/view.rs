use std::{
    ops::Range,
    path::{Path, PathBuf},
    time::Duration,
};

use fe_buffer::{Bias, BufferSnapshot, Version};
use fe_command::{ContextFrame, ContextStack, Invocation, Source};
use fe_config::settings::LineNumbersSetting;
use fe_display::{LineNumbers, layout::RowLayout};
use fe_editor::EditorModel;
use fe_keymap::{Key, Keystroke};
use fe_syntax::{Language, ParseJob, Spawner, Syntax, SyntaxDriver};
use fe_vim::VisualKind;
use gpui::{
    AnyElement, AnyView, App, AppContext as _, AvailableSpace, Bounds, Context, CursorStyle,
    FocusHandle, Focusable, Hsla, InteractiveElement as _, IntoElement, KeyContext, Modifiers,
    ModifiersChangedEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ParentElement as _, Pixels, Point, Render, ScrollWheelEvent, Styled as _, Subscription, Task,
    Window, canvas, deferred, div, point, px, size,
};

use crate::{
    commands::RunCommand,
    element::{EditorElement, GlyphWidths},
    keymap,
    timing::FrameTimings,
    *,
};

/// The gutter's numbers `setting` (`editor.line_numbers`) shows: `on`
/// is vim's `nu`, `relative` `nonu rnu` and `hybrid` `nu rnu`.
pub fn line_numbers(setting: LineNumbersSetting) -> LineNumbers {
    let (number, relative) = match setting {
        LineNumbersSetting::On => (true, false),
        LineNumbersSetting::Off => (false, false),
        LineNumbersSetting::Relative => (false, true),
        LineNumbersSetting::Hybrid => (true, true),
    };
    LineNumbers { number, relative }
}

/// How long a frame may spend reparsing before the parse moves to a
/// background thread. Small edits to files of 10k lines reparse within it.
const SYNC_PARSE_BUDGET: Duration = Duration::from_millis(1);

/// How much of the buffer's start is read for a shebang, when the path names
/// no language.
pub(crate) const DETECT_BYTES: usize = 512;

/// The most text written to the primary selection, in bytes.
const PRIMARY_SELECTION_MAX: usize = 1 << 20;

#[cfg(test)]
pub(crate) type SyntaxHook = Box<dyn FnMut(&Syntax)>;

/// An editor for one buffer.
pub struct EditorView {
    pub(crate) model: EditorModel,
    pub(crate) theme: EditorTheme,
    pub(crate) focus: FocusHandle,
    /// The colour of this editor's tab title, if set: its file's git status.
    pub(crate) tab_tint: Option<Hsla>,
    /// The tab's title and the file name its icon is picked by, in place of
    /// its file's name ([`EditorView::set_tab_title`]).
    pub(crate) tab_title: Option<(gpui::SharedString, gpui::SharedString)>,
    /// How many ems soft wrap leaves free at the right, beyond its usual
    /// margin, for text after a line ([`EditorView::set_wrap_reserve`]).
    pub(crate) wrap_reserve: f32,
    /// `editor.wrap_column` and `editor.wrap_mode`: where soft wrap wraps,
    /// when on ([`EditorView::wrap_limit`]).
    pub(crate) wrap_column: Option<u32>,
    pub(crate) wrap_mode: fe_config::settings::WrapMode,
    /// The file's `.editorconfig` `max_line_length`, if it has one.
    pub(crate) max_line_length: Option<usize>,
    /// Whether the gutter shows ([`EditorView::set_gutter`]).
    pub(crate) gutter: bool,
    /// The minimap's options, if it's on ([`EditorView::set_minimap`]).
    pub(crate) minimap: Option<fe_display::minimap::MinimapOptions>,
    /// Whether the pointer is over the minimap and its slider.
    pub(crate) minimap_hover: crate::element::minimap::Hover,
    /// The minimap's rows' runs and its tiles, kept across frames.
    pub(crate) minimap_cache: std::cell::RefCell<crate::element::minimap::Cache>,
    /// Counts the parses and language loads that landed, which change
    /// highlights beyond the rows edited.
    pub(crate) highlight_epoch: u64,
    /// A sticky scroll row's header clicked, whose row the next frame whose
    /// syntax matches the text scrolls to, under its parent's band
    /// ([`EditorView::reveal_sticky_header`]).
    pub(crate) sticky_reveal: Option<fe_buffer::Anchor>,
    /// The scopes sticky scroll's band was last found from
    /// ([`crate::element::sticky_scroll::scopes`]).
    pub(crate) sticky_scopes: Option<crate::element::sticky_scroll::Scopes>,
    /// The host's column left of the line numbers
    /// ([`EditorView::set_gutter_annotations`]).
    pub(crate) annotations: Option<crate::gutter_annotations::Annotated>,
    /// The host's links ([`EditorView::set_link_ranges`]).
    pub(crate) links: crate::link_ranges::LinkRanges,
    /// How far the content is scrolled, in pixels (both ≥ 0).
    pub(crate) scroll: Point<Pixels>,
    /// Scroll the primary cursor into view on the next frame.
    pub(crate) autoscroll: bool,
    /// A vim scroll (`zt`, `<C-e>`, `H`…) placed the view, which keeps
    /// nvim's top rows, or the view is too short to show the caret's row
    /// below sticky scroll's band: the band leaves the caret's row out
    /// instead, until the caret moves or the view scrolls otherwise
    /// ([`crate::element::sticky_scroll::keep_above`]).
    pub(crate) band_spares_caret: bool,
    /// The caret moved or the text changed since the last frame, which then
    /// scrolls the caret's row out from under sticky scroll's band, even
    /// where vim keeps the view where it is (`k` within it).
    pub(crate) clear_band: bool,
    /// The hunk whose block the next frame scrolls into view, after the
    /// caret: where next or previous hunk went.
    pub(crate) reveal_hunk: Option<fe_buffer::HunkId>,
    /// The buffer row a vim scroll put first since the last frame, which
    /// the next dispatch sees as the top until a frame is laid out.
    pub(crate) scrolled_top: Option<u32>,
    /// The display row such a scroll put first, until a frame is laid out:
    /// where the view starts, which [`EditorView::pin_top_row`] leaves be,
    /// as it leaves the pixel offset, even partway down a wrapped row.
    /// Without one, the last frame's first row.
    pub(crate) scrolled_display_top: Option<u32>,
    /// What the last frame laid out, for hit testing and IME.
    pub(crate) layout: Option<crate::element::FrameLayout>,
    drag: Option<Drag>,
    /// Text being composed by an input method ([`ime`]).
    composition: Option<ime::Composition>,
    /// Whether selections go to the Linux primary selection, which a middle
    /// click pastes.
    primary_selection: bool,
    /// The text this view last wrote to the primary selection.
    last_primary: Option<String>,
    /// The buffer's syntax tree, once its language is loaded.
    pub(crate) syntax: Option<SyntaxDriver<Task<()>>>,
    /// The language loading in the background.
    language_task: Option<Task<()>>,
    /// Whether the language was set by hand ([`EditorView::set_language`])
    /// rather than detected from the path and first lines.
    pub(crate) language_chosen: bool,
    /// The language [`EditorView::set_language`] last set, `None` for
    /// plain text: what a split of the view is set to while it's chosen.
    pub(crate) chosen_language: Option<&'static str>,
    /// The path a model with none takes its language from, as a file at a
    /// revision does ([`EditorView::detect_language_from`]).
    pub(crate) language_source: Option<PathBuf>,
    /// Injected languages loading in the background.
    injected_task: Option<Task<()>>,
    /// Runs just before the view takes the injected languages a frame
    /// lacked, so a test can compile one in between.
    #[cfg(test)]
    pub(crate) before_taking_missing: Option<SyntaxHook>,
    pub(crate) frame_timings: FrameTimings,
    /// Vim's state, while vim is on ([`crate::vim`]).
    pub(crate) vim: Option<fe_vim::VimState>,
    /// The serial of the last yank flashed, and the flash under way.
    pub(crate) yank_seen: u64,
    pub(crate) yank_flash: Option<crate::vim::YankFlash>,
    /// An open vim search line's preview, and the view to put back
    /// ([`EditorView::preview_search`]).
    pub(crate) search_preview: Option<crate::vim::SearchPreview>,
    /// The which-key popup, or its wait ([`crate::vim::which_key`]).
    pub(crate) which_key: crate::vim::which_key::WhichKeyPopup,
    /// The vim switch's generation this view last synced with.
    pub(crate) vim_generation: u64,
    /// Vim stays off here whatever the switch says
    /// ([`EditorView::set_vim_off`]).
    pub(crate) vim_off: bool,
    /// A macro's keys are being fed ([`EditorView::replay_vim`]): the
    /// typing paths they go through don't record them again.
    pub(crate) vim_replaying: bool,
    /// The last [`EditorView::run_command`] failed: a remap's `<Cmd>fe:…<CR>`
    /// the keymap dispatched reads it, there being no result to return.
    pub(crate) command_failed: bool,
    /// A press that waited for a held prefix runs again, which then goes on
    /// without waiting ([`EditorView::settle_press`]).
    press_settled: bool,
    /// An editor command a recording can't hold was logged since the
    /// recording began ([`EditorView::record_typing`]).
    pub(crate) vim_unrecorded_logged: bool,
    /// The command line took keys through the `c` maps
    /// ([`EditorView::remap_line_key`]), which a recording took as typed:
    /// the line they build, so only the rest of its text is recorded.
    pub(crate) vim_line_echo: Option<String>,
    /// The key that ended the line came from a map's right side, so a
    /// recording has it in the map's keys already.
    pub(crate) vim_line_end_expanded: bool,
    /// A replay read the literal-next marker on the command line: the next character is
    /// literal text, not matched against the `c` maps.
    pub(crate) vim_line_literal: bool,
    /// The pointer is over the fold column, so the chevrons of folds that
    /// are open show too.
    pub(crate) fold_column_hovered: bool,
    /// The host's glyph in the gutter's mark slot, its row anchored at the
    /// row's start so it follows edits.
    gutter_glyph: Option<(fe_buffer::Anchor, GlyphKind)>,
    /// The host's overlay under its anchor's last row.
    overlay: Option<Overlay>,
    /// What a click on the glyph calls.
    glyph_click: Option<GlyphClick>,
    /// Where the last frame painted the glyph, in window coordinates.
    pub(crate) glyph_hit: Option<Bounds<Pixels>>,
    /// The glyph the last frame painted: buffer row and bounds.
    #[cfg(test)]
    pub(crate) painted_glyph: Option<(u32, Bounds<Pixels>)>,
    /// The word under the pointer while the held modifiers' click is bound
    /// to a command, such as `cmd-click`: underlined, with a pointing hand.
    hover: Option<Hover>,
    /// Clears the hover range when the view loses focus, even if it's no
    /// longer painted then, as when navigating to another tab, and drops a
    /// composition.
    hover_blur: Option<Subscription>,
    /// The popover for what's under the pointer ([`crate::hover`]).
    pub(crate) popover: crate::hover::HoverPopover,
    /// Where the popover's hover text comes from, if anywhere.
    pub(crate) hover_provider: Option<std::rc::Rc<dyn crate::HoverProvider>>,
    /// What draws hover and completion documentation Markdown, if anything.
    pub(crate) markdown_renderer: Option<std::rc::Rc<dyn crate::MarkdownRenderer>>,
    /// The completion menu, open or awaiting its first answer
    /// ([`crate::completion_menu`]).
    pub(crate) completion: Option<crate::completion_menu::Menu>,
    /// Where completions come from, if anywhere.
    pub(crate) completion_provider: Option<std::rc::Rc<dyn crate::CompletionProvider>>,
    /// What runs before each save ([`crate::pane_item::SaveHook`]).
    pub(crate) save_hook: Option<std::rc::Rc<dyn crate::pane_item::SaveHook>>,
    /// What saving fixes in the text ([`crate::pane_item::SaveFixes`]).
    pub(crate) save_fixes: crate::pane_item::SaveFixes,
    /// Close the menu on `escape` and on losing focus.
    pub(crate) completion_subscriptions: Vec<Subscription>,
    /// Signature help, and where it comes from ([`crate::signature_help`]).
    pub(crate) signature_help: crate::signature_help::SignaturePopover,
    /// The frames around the view's own in its context stack, as last seen
    /// while it was focused: see [`EditorView::context_stack`].
    ancestors: Option<ContextStack>,
    /// The find pattern and its count ([`find`]).
    pub(crate) find: find::Find,
    /// Edits are refused ([`host`]).
    pub(crate) read_only: bool,
    /// Read-only, its proposals' hunks can still be accepted by their
    /// buttons ([`EditorView::set_accepts_when_read_only`]).
    pub(crate) accepts_when_read_only: bool,
    /// Never saved, to its path or any other
    /// ([`EditorView::set_unsavable`]).
    pub(crate) unsavable: bool,
    /// The open command line is a search an operator other than a yank
    /// waits on ([`EditorView::note_command_line`]).
    pub(crate) line_operates: bool,
    /// The host's blocks between the rows ([`host::HostBlock`]).
    pub(crate) blocks: Vec<HostBlock>,
    /// Where the frame laid out the reserve blocks among them.
    pub(crate) reserved: Vec<host::ReservedBlock>,
    /// The host's flags in the key context
    /// ([`EditorView::set_key_context_flag`]).
    pub(crate) key_flags: Vec<&'static str>,
    /// The proposed changes shown as diff blocks ([`proposal`]).
    pub(crate) proposals: Vec<std::rc::Rc<std::cell::RefCell<fe_buffer::ProposedChange>>>,
    /// The buttons in the proposals' blocks the last frame painted: the
    /// hunk, the button, and the part of its box shown, in window
    /// coordinates. A click there presses it.
    pub(crate) proposal_buttons: Vec<(fe_buffer::HunkId, ProposalButton, gpui::Bounds<Pixels>)>,
    /// Where lens rows come from ([`crate::lens`]).
    lens_providers: Vec<std::rc::Rc<std::cell::RefCell<dyn crate::LensProvider>>>,
    /// The lens rows the last frame painted: the part of each in view, and
    /// of each of its items, in window coordinates. A click on an item runs
    /// its command; elsewhere on the row, nothing.
    pub(crate) lens_rows: Vec<crate::lens::PaintedRow>,
    /// The suggestion shown at the caret ([`ghost_text`]).
    ghost_text: Option<fe_editor::ghost_text::GhostText>,
    /// The host's fold ranges, if it gave any ([`crate::server_folds`]).
    pub(crate) server_folds: Option<crate::server_folds::ServerFolds>,
    /// The host's inlay hints ([`crate::inlay_hints`]).
    pub(crate) inlay_hints: crate::inlay_hints::Hints,
    /// Resolves a hint's tooltip and edits.
    pub(crate) inlay_hint_host: Option<std::rc::Rc<dyn crate::inlay_hints::InlayHintHost>>,
    /// The hints the last frame painted: their index and window bounds.
    pub(crate) inlay_hints_bounds: Vec<(usize, Bounds<Pixels>)>,
    /// The host's colour swatches ([`crate::color_swatches`]).
    pub(crate) color_swatches: crate::color_swatches::Swatches,
    /// The suggestion losing focus dropped, with the buffer's version then,
    /// for a palette pick that accepts it if the buffer hasn't changed since
    /// ([`ghost_text`]).
    blurred_ghost_text: Option<(fe_editor::ghost_text::GhostText, fe_buffer::Version)>,
    /// Drop it on `escape` and on losing focus.
    ghost_text_subscriptions: Vec<Subscription>,
    /// The top, in screen rows, the last frame reported or the host set
    /// ([`EditorView::report_scroll_top`]).
    pub(crate) reported_top: f32,
    /// Where the host set the top since the last frame, which doesn't
    /// report it unless something else scrolled since.
    pub(crate) top_set: Option<Pixels>,
    /// The carets the last frame painted, in window coordinates.
    #[cfg(test)]
    pub(crate) painted_carets: Vec<Bounds<Pixels>>,
    /// The selection rects the last frame painted, in window coordinates.
    #[cfg(test)]
    pub(crate) painted_selections: Vec<Bounds<Pixels>>,
    /// The search-match rects the last frame painted, in window coordinates.
    #[cfg(test)]
    pub(crate) painted_matches: Vec<Bounds<Pixels>>,
    /// The chevrons the last frame painted: buffer row, folded, and box in
    /// window coordinates.
    #[cfg(test)]
    pub(crate) painted_chevrons: Vec<(u32, bool, Bounds<Pixels>)>,
    /// The bracket outlines the last frame painted, in window coordinates.
    #[cfg(test)]
    pub(crate) painted_brackets: Vec<Bounds<Pixels>>,
    /// The previewed search match painted in the last frame
    /// ([`EditorView::search_preview`]).
    #[cfg(test)]
    pub(crate) painted_current_match: Vec<Bounds<Pixels>>,
    /// The hover range's underlines the last frame painted, in window
    /// coordinates, and whether it set the pointing hand.
    #[cfg(test)]
    pub(crate) painted_hover: (Vec<Bounds<Pixels>>, bool),
    /// The diagnostics' squiggles the last frame painted, in window
    /// coordinates, and their colours.
    #[cfg(test)]
    pub(crate) painted_squiggles: Vec<(Bounds<Pixels>, gpui::UnderlineStyle)>,
    /// The strikes through deprecated text the last frame painted, in
    /// window coordinates.
    #[cfg(test)]
    pub(crate) painted_strikes: Vec<Bounds<Pixels>>,
    /// The underline under composed text the last frame painted, in window
    /// coordinates.
    #[cfg(test)]
    pub(crate) painted_composition: Option<Bounds<Pixels>>,
    /// The gutter marks the last frame painted: buffer row and colour.
    #[cfg(test)]
    pub(crate) painted_marks: Vec<(u32, gpui::Hsla)>,
    /// The hover popover's text, as the last render showed it.
    #[cfg(test)]
    pub(crate) painted_popover: Option<String>,
    /// Its parts, and the font each was set in.
    #[cfg(test)]
    pub(crate) painted_popover_blocks: Vec<(crate::hover::Block, gpui::SharedString)>,
    /// The annotation column's display rows the last frame painted: the
    /// buffer row, its text and its tint.
    #[cfg(test)]
    pub(crate) painted_annotations: Vec<(u32, Option<gpui::SharedString>, Option<Hsla>)>,
    /// The completion menu, as the last frame showed it.
    #[cfg(test)]
    pub(crate) painted_completion: Option<crate::completion_menu::Painted>,
    /// The host's blocks the last frame painted.
    #[cfg(test)]
    pub(crate) painted_blocks: Vec<crate::element::PaintedBlock>,
    /// The proposals' blocks the last frame painted: the hunk, its status,
    /// the header's text and the block's box in window coordinates.
    #[cfg(test)]
    pub(crate) painted_proposals: Vec<(
        fe_buffer::HunkId,
        fe_buffer::HunkStatus,
        String,
        Bounds<Pixels>,
    )>,
    /// The ghost text the last frame painted: its inline part, its block's
    /// rows, and the inline part's box in window coordinates.
    #[cfg(test)]
    pub(crate) painted_ghost: Option<(String, Vec<String>, Bounds<Pixels>)>,
    /// The inlay hints the last frame painted: their text and boxes.
    #[cfg(test)]
    pub(crate) painted_inlay_hints: Vec<(String, Bounds<Pixels>)>,
    /// The colour swatches the last frame painted.
    #[cfg(test)]
    pub(crate) painted_swatches: Vec<crate::color_swatches::Painted>,
    /// The diagnostic messages the last frame painted after their rows.
    #[cfg(test)]
    pub(crate) painted_inline_diagnostics: crate::inline_diagnostics::Painted,
    /// The minimap the last frame painted.
    #[cfg(test)]
    pub(crate) painted_minimap: Option<crate::element::minimap::Painted>,
    /// Sticky scroll's band the last frame painted.
    #[cfg(test)]
    pub(crate) painted_sticky_scroll: Option<crate::element::sticky_scroll::Painted>,
}

/// A word a bound click would run its command on: see [`EditorView::hover`].
#[derive(Clone, Debug, PartialEq)]
struct Hover {
    range: Range<usize>,
    /// The buffer version and scroll position it was found at: an edit or
    /// a scroll makes it stale.
    version: Version,
    scroll: Point<Pixels>,
}

/// What a mouse drag extends: characters, or whole words or rows starting
/// from the range first clicked, or an alt-shift-drag's box from the
/// (buffer row, display column) it started at; or the minimap's slider
/// ([`minimap`]).
#[derive(Clone)]
enum Drag {
    Chars,
    Words(Range<usize>),
    Rows(Range<usize>),
    Box((u32, usize)),
    Minimap(Box<minimap::MinimapDrag>),
}

/// Which mouse button's press waits for a held prefix
/// ([`EditorView::settle_press`]).
#[derive(Clone, Copy)]
enum Press {
    Left,
    Middle,
}

impl EditorView {
    /// An editor for `model`, highlighted according to its file's language.
    pub fn new(mut model: EditorModel, theme: EditorTheme, cx: &mut Context<Self>) -> Self {
        model.set_char_widths(glyph_widths(&theme, cx));
        let mut view = Self {
            model,
            theme,
            focus: cx.focus_handle(),
            tab_tint: None,
            tab_title: None,
            wrap_reserve: 0.,
            wrap_column: None,
            wrap_mode: Default::default(),
            max_line_length: None,
            gutter: true,
            minimap: None,
            minimap_hover: Default::default(),
            minimap_cache: Default::default(),
            highlight_epoch: 0,
            sticky_reveal: None,
            sticky_scopes: None,
            annotations: None,
            links: Default::default(),
            scroll: Point::default(),
            autoscroll: true,
            band_spares_caret: false,
            clear_band: false,
            reveal_hunk: None,
            scrolled_top: None,
            scrolled_display_top: None,
            layout: None,
            drag: None,
            composition: None,
            primary_selection: cfg!(target_os = "linux"),
            last_primary: None,
            syntax: None,
            language_task: None,
            language_chosen: false,
            chosen_language: None,
            language_source: None,
            injected_task: None,
            #[cfg(test)]
            before_taking_missing: None,
            frame_timings: FrameTimings::default(),
            vim: None,
            yank_seen: 0,
            yank_flash: None,
            search_preview: None,
            which_key: Default::default(),
            vim_generation: 0,
            vim_off: false,
            vim_replaying: false,
            command_failed: false,
            press_settled: false,
            vim_unrecorded_logged: false,
            vim_line_echo: None,
            vim_line_end_expanded: false,
            vim_line_literal: false,
            fold_column_hovered: false,
            gutter_glyph: None,
            overlay: None,
            glyph_click: None,
            glyph_hit: None,
            #[cfg(test)]
            painted_glyph: None,
            hover: None,
            hover_blur: None,
            popover: Default::default(),
            hover_provider: None,
            markdown_renderer: None,
            completion: None,
            completion_provider: None,
            save_hook: None,
            save_fixes: Default::default(),
            completion_subscriptions: Vec::new(),
            signature_help: Default::default(),
            ancestors: None,
            find: Default::default(),
            read_only: false,
            accepts_when_read_only: false,
            unsavable: false,
            line_operates: false,
            blocks: Vec::new(),
            reserved: Vec::new(),
            key_flags: Vec::new(),
            proposals: Vec::new(),
            proposal_buttons: Vec::new(),
            lens_providers: Vec::new(),
            lens_rows: Vec::new(),
            ghost_text: None,
            server_folds: None,
            inlay_hints: Default::default(),
            inlay_hint_host: None,
            inlay_hints_bounds: Vec::new(),
            color_swatches: Default::default(),
            blurred_ghost_text: None,
            ghost_text_subscriptions: Vec::new(),
            reported_top: 0.,
            top_set: None,
            #[cfg(test)]
            painted_carets: Vec::new(),
            #[cfg(test)]
            painted_selections: Vec::new(),
            #[cfg(test)]
            painted_matches: Vec::new(),
            #[cfg(test)]
            painted_chevrons: Vec::new(),
            #[cfg(test)]
            painted_brackets: Vec::new(),
            #[cfg(test)]
            painted_current_match: Vec::new(),
            #[cfg(test)]
            painted_hover: (Vec::new(), false),
            #[cfg(test)]
            painted_squiggles: Vec::new(),
            #[cfg(test)]
            painted_strikes: Vec::new(),
            #[cfg(test)]
            painted_composition: None,
            #[cfg(test)]
            painted_marks: Vec::new(),
            #[cfg(test)]
            painted_popover: None,
            #[cfg(test)]
            painted_popover_blocks: Vec::new(),
            #[cfg(test)]
            painted_annotations: Vec::new(),
            #[cfg(test)]
            painted_completion: None,
            #[cfg(test)]
            painted_blocks: Vec::new(),
            #[cfg(test)]
            painted_proposals: Vec::new(),
            #[cfg(test)]
            painted_ghost: None,
            #[cfg(test)]
            painted_inlay_hints: Vec::new(),
            #[cfg(test)]
            painted_swatches: Vec::new(),
            #[cfg(test)]
            painted_inline_diagnostics: Default::default(),
            #[cfg(test)]
            painted_minimap: None,
            #[cfg(test)]
            painted_sticky_scroll: None,
        };
        crate::vim::register(&view.focus, cx);
        crate::hover::hide_on_escape(&mut view, cx);
        // The minimap's tiles stay in the windows' atlases until dropped.
        cx.on_release(|view, cx| {
            for image in view.minimap_cache.get_mut().take_images() {
                cx.drop_image(image, None);
            }
        })
        .detach();
        view.sync_vim(cx);
        view.detect_language(cx);
        view
    }

    /// Tints the tab title in `tint`, or not; redraws only if that changed.
    pub fn set_tab_tint(&mut self, tint: Option<Hsla>, cx: &mut Context<Self>) {
        if self.tab_tint != tint {
            self.tab_tint = tint;
            cx.notify();
        }
    }

    /// Titles the tab `title`, its icon the one a file named `icon_by` has,
    /// in place of its file's name: a buffer with no file that stands for
    /// one, such as a file's text at a commit.
    pub fn set_tab_title(
        &mut self,
        title: impl Into<gpui::SharedString>,
        icon_by: impl Into<gpui::SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.tab_title = Some((title.into(), icon_by.into()));
        cx.notify();
    }

    /// Has soft wrap leave `ems` more of the text area free at the right,
    /// so text after a line (a decoration's
    /// [`fe_editor::decorations::DecorationKind::AfterLine`]) up to that
    /// wide shows whole after a wrapped row's last piece. None by default.
    pub fn set_wrap_reserve(&mut self, ems: f32, cx: &mut Context<Self>) {
        if self.wrap_reserve != ems {
            self.wrap_reserve = ems;
            cx.notify();
        }
    }

    /// How many ems soft wrap leaves free for text after a line: see
    /// [`EditorView::set_wrap_reserve`].
    pub fn wrap_reserve(&self) -> f32 {
        self.wrap_reserve
    }

    /// Shows or hides the gutter: its line numbers, fold column, change lane
    /// and diagnostics' marks. Without it, the text starts half an em in
    /// from the left and soft wraps over the room it leaves
    /// ([`fe_display::layout::layout_frame_without_gutter`]). Shown by
    /// default.
    pub fn set_gutter(&mut self, shown: bool, cx: &mut Context<Self>) {
        if self.gutter != shown {
            self.gutter = shown;
            // A popover waiting or shown for an annotation goes with the
            // column, so showing it again doesn't revive the old timer.
            if !shown && self.popover_shows_annotation() {
                self.hide_popover(cx);
            }
            cx.notify();
        }
    }

    /// Turns the Linux primary selection on or off: while on, the selection
    /// is written to it and a middle click pastes from it.
    pub fn set_primary_selection(&mut self, on: bool) {
        self.primary_selection = on;
    }

    /// Writes the primary selection's text to the primary selection, unless
    /// it's empty, over [`PRIMARY_SELECTION_MAX`] or already what this view
    /// wrote last.
    pub(crate) fn write_primary(&mut self, cx: &mut Context<Self>) {
        if !self.primary_selection {
            return;
        }
        let range = self.model.primary_selection().range;
        if range.is_empty() || range.len() > PRIMARY_SELECTION_MAX {
            return;
        }
        let text = self.model.text().slice_to_string(range);
        if self.last_primary.as_deref() != Some(text.as_str()) {
            write_primary_text(cx, text.clone());
            self.last_primary = Some(text);
        }
    }

    /// A middle click: puts the caret where it was pressed and inserts the
    /// primary selection there.
    fn on_middle_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.primary_selection {
            return;
        }
        let over_text = self
            .layout
            .as_ref()
            .is_some_and(|layout| layout.over_text(event.position, window));
        let Some(text) = read_primary_text(cx) else {
            return;
        };
        if !over_text || text.is_empty() {
            return;
        }
        if self.read_only {
            self.refuse_edit(cx);
            return;
        }
        if !self.settle_press(event, Press::Middle, window, cx) {
            return;
        }
        window.focus(&self.focus, cx);
        // A middle click ends a composition, typing nothing, as a click does.
        self.composition = None;
        self.drag = None;
        let vim = self.vim_owns_mouse(cx);
        let offset = if vim {
            self.vim_offset_for_position(event.position)
        } else {
            self.offset_for_position(event.position)
        };
        // Ending an insert session can drop unused autoindent before the
        // click, so the offset follows the text, not its number.
        let at = self.model.snapshot().anchor_before(offset);
        // Like nvim (mouse=a), a middle-click paste leaves Visual mode for
        // normal, so vim's visual extent doesn't restore the old selection
        // over the caret.
        if vim {
            self.cancel_vim(cx);
        }
        self.split_session(
            fe_vim::Splits::Edits,
            |model| {
                let offset = model.snapshot().resolve(&at);
                model.set_cursor(offset);
                model.insert(&text);
            },
            cx,
        );
        self.sync_completion(cx);
        self.changed(cx);
    }

    /// Highlights the buffer as the language called `name`, or as plain text
    /// for `None` or a name that isn't compiled in. Overrides whatever was
    /// detected when the view opened, and sticks until the next call. A
    /// language not loaded yet loads in the background, with the buffer
    /// plain until it's ready.
    pub fn set_language(&mut self, name: Option<&str>, cx: &mut Context<Self>) {
        self.language_chosen = true;
        // Dropping the driver and the load cancels them, so neither can
        // install anything for the old language.
        self.syntax = None;
        self.language_task = None;
        self.highlight_epoch += 1;
        cx.notify();
        let name = name.and_then(|name| fe_syntax::language_names().find(|&n| n == name));
        self.chosen_language = name;
        let Some(name) = name else {
            return;
        };
        if let Some(language) = fe_syntax::loaded_language(name) {
            self.syntax = Some(syntax_driver(language, self.model.snapshot()));
            return;
        }
        // Compiling a highlight query can take a few frames.
        self.language_task = Some(cx.spawn(async move |this, cx| {
            let language = cx
                .background_spawn(async move { fe_syntax::language_by_name(name) })
                .await;
            this.update(cx, |view, cx| {
                view.language_task = None;
                view.syntax =
                    language.map(|language| syntax_driver(language, view.model.snapshot()));
                view.highlight_epoch += 1;
                cx.notify();
            })
            .ok();
        }));
    }

    /// The buffer's syntax, once its language has loaded; `None` for plain
    /// text. Its [`Syntax::tree`] has been edited to match
    /// [`Syntax::version`], which may trail the model's snapshot until the
    /// next frame, but the tree may not have been reparsed since the latest
    /// edits: [`Syntax::needs_parse`] says whether it has. Node positions are
    /// right for the text at that version either way; only the structure of
    /// the edited text may be stale.
    pub fn syntax(&self) -> Option<&Syntax> {
        self.syntax.as_ref().map(SyntaxDriver::syntax)
    }

    /// Whether the language the view was opened or set with is still loading
    /// in the background, the buffer plain text until it's ready.
    pub fn is_loading_language(&self) -> bool {
        self.language_task.is_some()
    }

    /// Whether the language, plain text included, was set by
    /// [`EditorView::set_language`] since it was last detected (as the view
    /// opened, or after a Save As): a language picked by hand.
    pub fn is_language_chosen(&self) -> bool {
        self.language_chosen
    }

    /// The path its language is detected from: its model's, else the one it
    /// was told ([`EditorView::detect_language_from`]).
    pub fn language_path(&self) -> Option<&Path> {
        self.model.path().or(self.language_source.as_deref())
    }

    pub fn model(&self) -> &EditorModel {
        &self.model
    }

    /// Takes the font family, size and line height, the tab size and the
    /// soft-wrap widths from `settings`, and redraws. Soft wrap itself is
    /// left alone: see [`EditorView::reload_settings`].
    pub fn apply_settings(&mut self, settings: &EditorSettings, cx: &mut Context<Self>) {
        self.theme.set_metrics(settings, cx);
        self.model.set_char_widths(glyph_widths(&self.theme, cx));
        if self.model.tab_size() != settings.tab_size {
            self.model.set_tab_size(settings.tab_size);
        }
        self.set_wrap_settings(settings);
        cx.notify();
    }

    /// Applies a reload's settings. Soft wrap and line numbers change only
    /// if the reload changed them, so a reload of anything else keeps the
    /// editor's `alt-z` toggle and `:set nu rnu`.
    pub fn reload_settings(&mut self, event: &SettingsEvent, cx: &mut Context<Self>) {
        let (settings, previous) = (&event.settings.editor, &event.previous.editor);
        if settings == previous {
            return;
        }
        self.apply_settings(settings, cx);
        if settings.soft_wrap != previous.soft_wrap {
            self.model.set_soft_wrap(settings.soft_wrap);
        }
        if settings.line_numbers != previous.line_numbers {
            self.model
                .set_line_numbers(line_numbers(settings.line_numbers));
        }
        if settings.cursor_surrounding_lines != previous.cursor_surrounding_lines {
            self.model.set_scroll_off(settings.cursor_surrounding_lines);
        }
        if settings.cursor_line != previous.cursor_line {
            self.model.set_cursor_line(settings.cursor_line);
        }
    }

    /// Takes `editor.wrap_column` and `editor.wrap_mode` from `settings`: for
    /// an editor made from them rather than applied to ([`EditorView::apply_settings`]
    /// does too). The frame loop wraps at the new width when it next draws.
    pub fn set_wrap_settings(&mut self, settings: &EditorSettings) {
        self.wrap_column = settings.effective_wrap_column();
        self.wrap_mode = settings.wrap_mode;
    }

    /// Sets the file's `.editorconfig` `max_line_length` (`None` for none, or
    /// `off`). It is the wrap column, clamped to 20..=500, replacing
    /// `editor.wrap_column`, where `editor.wrap_mode` is `column` or
    /// `bounded` (not in `viewport`), and draws a ruler at the column, in any
    /// wrap mode. Settings changes leave it alone.
    pub fn set_max_line_length(&mut self, columns: Option<usize>, cx: &mut Context<Self>) {
        if self.max_line_length != columns {
            self.max_line_length = columns;
            cx.notify();
        }
    }

    /// The width soft wrap wraps at, given the `viewport` width it would
    /// fill and the `em` of a monospace cell: `editor.wrap_column` columns
    /// for `column`, the smaller of the two for `bounded`.
    pub(crate) fn wrap_limit(&self, viewport: f32, em: f32) -> f32 {
        use fe_config::settings::WrapMode;
        let column = (self.max_line_length)
            .map(|n| n.clamp(20, 500) as u32)
            .or(self.wrap_column);
        match (self.wrap_mode, column) {
            (WrapMode::Column, Some(n)) => n as f32 * em,
            (WrapMode::Bounded, Some(n)) => viewport.min(n as f32 * em),
            _ => viewport,
        }
    }

    /// Whether the caret's row's band is painted: the editor's 'cursorline'.
    pub fn paints_cursor_line(&self) -> bool {
        self.model.cursor_line()
    }

    pub fn model_mut(&mut self) -> &mut EditorModel {
        &mut self.model
    }

    /// How long the editor took to lay out and paint its recent frames.
    pub fn frame_timings(&self) -> &FrameTimings {
        &self.frame_timings
    }

    pub fn frame_timings_mut(&mut self) -> &mut FrameTimings {
        &mut self.frame_timings
    }

    /// Call after anything that moves the cursor or changes the text.
    pub(crate) fn changed(&mut self, cx: &mut Context<Self>) {
        self.autoscroll = true;
        self.band_spares_caret = false;
        self.clear_band = true;
        self.sticky_reveal = None;
        self.blurred_ghost_text = None;
        // An edit stops the find count running and starts another now, not
        // at the next frame.
        self.sync_find(cx);
        cx.notify();
    }

    /// The host API for "go to" jumps (search results, outline, later
    /// go-to-def): selects `range`, head at its end, and scrolls it into view
    /// on the next frame as caret movement does. Offsets that are stale, past
    /// the buffer or inside a char are clamped to the char boundary on their
    /// left, a reversed range is normalised, and a fold hiding the start is
    /// opened. It doesn't focus the view. It ends vim's visual mode and any
    /// pending vim command.
    pub fn select_and_reveal(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        // As a click does, a jump drops a pending vim command and leaves
        // vim's visual mode, and ends a composition, typing nothing.
        self.cancel_vim(cx);
        self.composition = None;
        let text = self.model.text();
        let start = text.clip_offset(range.start, Bias::Left);
        let end = text.clip_offset(range.end, Bias::Left);
        let range = start.min(end)..start.max(end);
        let row = text.offset_to_point(range.start).row;
        self.model.unfold_rows(row..=row);
        self.model.set_selections([fe_buffer::ResolvedSelection {
            range,
            reversed: false,
        }]);
        self.changed(cx);
    }

    /// Edits the text as the user would, with a host's replacements: `plan`
    /// gives them from the model once vim's insert or replace session has
    /// ended (and so any autoindent it dropped), so offsets are never stale,
    /// and the session resumes after them. Typing before and after them
    /// undoes apart from them. They are one user transaction, and the caret
    /// goes to the start of the first, scrolled into view as
    /// [`EditorView::select_and_reveal`] does, with a fold hiding it opened.
    /// `plan` gives replacements in order, not overlapping. Returns whether
    /// it gave any.
    pub fn edit_and_reveal(
        &mut self,
        plan: impl FnOnce(&EditorModel) -> Vec<(Range<usize>, String)>,
        cx: &mut Context<Self>,
    ) -> bool {
        self.cancel_vim(cx);
        self.composition = None;
        let edited = self.split_session(
            fe_vim::Splits::Edits,
            |model| {
                let edits = plan(model);
                let at = edits.first()?.0.start;
                let mut buffer = model.buffer_mut();
                let version = buffer.version();
                buffer.edit(edits, fe_buffer::Origin::User);
                buffer.note_edits_since(version);
                drop(buffer);
                let text = model.text();
                let at = text.clip_offset(at, Bias::Left);
                model.unfold_rows(text.offset_to_point(at).row..=text.offset_to_point(at).row);
                model.set_selections([fe_buffer::ResolvedSelection {
                    range: at..at,
                    reversed: false,
                }]);
                Some(())
            },
            cx,
        );
        self.changed(cx);
        edited.is_some()
    }

    /// The buffer row first in view, and how many display rows the view
    /// shows: `None` before the first frame.
    pub fn viewport_rows(&self) -> Option<(u32, u32)> {
        let frame = &self.layout.as_ref()?.frame;
        Some((self.top_buffer_row()?, frame.visible_rows()))
    }

    /// The buffer row first in view: where the last frame began, or a vim
    /// scroll since.
    pub(crate) fn top_buffer_row(&self) -> Option<u32> {
        self.scrolled_top.or_else(|| {
            let frame = &self.layout.as_ref()?.frame;
            Some(self.model.display().row(frame.first_row).buffer_row)
        })
    }

    /// The display row first in view: where a vim scroll put it since the
    /// last frame, or where that frame began, partway down a wrapped row as
    /// it may be.
    pub(crate) fn top_display_row(&self) -> Option<u32> {
        let frame = &self.layout.as_ref()?.frame;
        Some(self.scrolled_display_top.unwrap_or(frame.first_row))
    }

    /// What the view shows, in the text's rows as the vim engine counts
    /// them; the rows are the frame's whole display rows.
    pub(crate) fn viewport(&self) -> Option<fe_editor::commands::Viewport> {
        let frame = &self.layout.as_ref()?.frame;
        Some(fe_editor::commands::Viewport {
            top_row: self.top_buffer_row()?,
            rows: frame.visible_rows(),
        })
    }

    /// The display columns of an unwrapped row the last frame showed: from
    /// the horizontal scroll to the text's right edge, in the width of a
    /// space; `None` before the first frame.
    pub(crate) fn view_columns(&self) -> Option<std::ops::Range<u32>> {
        let layout = self.layout.as_ref()?;
        layout.view_columns()
    }

    /// Whether the last frame laid out a display row of buffer row `row`,
    /// or `None` before one was drawn.
    pub fn shows_buffer_row(&self, row: u32) -> Option<bool> {
        let frame = &self.layout.as_ref()?.frame;
        Some(frame.rows.iter().any(|laid| laid.buffer_row == row))
    }

    /// Shows buffer row `top_row` first from the next frame, which leaves
    /// the caret where it is even inside the autoscroll margin.
    pub(crate) fn scroll_to_top_row(&mut self, top_row: u32) {
        let Some(layout) = &self.layout else { return };
        let frame = &layout.frame;
        let display_row = self.model.display().row_for_buffer_row(top_row);
        self.scroll.y = px(frame.line_height * frame.block_map.screen_row(display_row) as f32);
        self.scrolled_top = Some(top_row);
        self.scrolled_display_top = Some(display_row);
        self.autoscroll = false;
        self.band_spares_caret = true;
        self.clear_band = false;
    }

    /// Keeps the view where it is through a vim scroll that left the top
    /// row alone, as the caret moves: its pixel offset, even partway down
    /// a wrapped row, stays as it is.
    pub(crate) fn pin_top_row(&mut self, scrolled: bool) {
        self.scrolled_top = self.top_buffer_row();
        self.autoscroll = false;
        self.band_spares_caret = scrolled;
        self.clear_band &= !scrolled;
    }

    /// The rows a page scrolls. Called as each dispatch starts, it also
    /// hands the dispatch this view's viewport.
    pub(crate) fn page_rows(&self) -> u32 {
        crate::commands::VIEWPORT.set(self.viewport());
        crate::commands::TOP_DISPLAY_ROW.set(self.top_display_row());
        crate::commands::VIEW_COLUMNS.set(self.view_columns());
        self.layout.as_ref().map_or(20, |layout| {
            layout.frame.visible_rows().saturating_sub(2).max(1)
        })
    }

    /// The buffer offset under a window position, in the last frame.
    fn offset_for_position(&mut self, position: Point<Pixels>) -> usize {
        let text = self.model.text();
        self.layout
            .as_mut()
            .map_or(0, |layout| layout.offset_at(text, position))
    }

    pub(crate) fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        if !self.settle_press(event, Press::Left, window, cx) {
            return;
        }
        self.press_proceeds(event, window, cx);
    }

    /// Resolves keys held for a longer map before a press acts, whatever it
    /// hits. If that edited the text, or left a replay waiting on a map's
    /// `<Cmd>fe:…<CR>` command, the press waits for the replay to finish and
    /// a frame to lay out the new text, then runs again
    /// ([`EditorView::press_when_settled`]), so its destination is measured
    /// on that text. Returns whether the press goes on now.
    fn settle_press(
        &mut self,
        event: &MouseDownEvent,
        press: Press,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if std::mem::take(&mut self.press_settled) {
            return true;
        }
        let version = self.model.snapshot().version();
        self.resolve_held_prefix(cx);
        if !self.vim_replaying && self.model.snapshot().version() == version {
            return true;
        }
        self.press_when_settled(event.clone(), press, window, cx);
        false
    }

    /// Runs `event` as a `press` once no replay is suspended and the last
    /// frame laid out the current text.
    fn press_when_settled(
        &mut self,
        event: MouseDownEvent,
        press: Press,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();
        window.on_next_frame(move |window, cx| {
            view.update(cx, |view, cx| {
                let version = view.model.snapshot().version();
                let laid_out =
                    (view.layout.as_ref()).is_none_or(|layout| layout.version == version);
                if view.vim_replaying || !laid_out {
                    view.press_when_settled(event, press, window, cx);
                    return;
                }
                view.press_settled = true;
                match press {
                    Press::Left => view.on_mouse_down(&event, window, cx),
                    Press::Middle => view.on_middle_down(&event, window, cx),
                }
                view.press_settled = false;
            })
        });
        window.refresh();
    }

    fn press_proceeds(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The minimap scrolls, and leaves the carets, a composition and vim's
        // mode be, whoever owns the mouse.
        if self.press_minimap(event, window, cx) {
            return;
        }
        // Sticky scroll's band, which takes presses from what's under it.
        let sticky = (self.layout.as_ref())
            .and_then(|layout| element::sticky_scroll::row_at(layout, event.position, window));
        if let Some(header) = sticky {
            self.drag = None;
            self.reveal_sticky_header(header, cx);
            return;
        }
        // A click ends a composition, typing nothing.
        self.composition = None;
        // A proposal's button leaves the carets where they are, as a lens
        // row does.
        if self.press_proposal_button(event.position, cx) || self.press_lens(event, window, cx) {
            self.drag = None;
            return;
        }
        if self.click_glyph(event, window, cx) {
            self.drag = None;
            return;
        }
        // The annotation column: a plain click on an entry is the host's;
        // nothing else happens there.
        if self.click_annotation(event, window, cx) {
            self.drag = None;
            return;
        }
        if self.toggle_fold_at(event.position, cx) {
            self.drag = None;
            self.changed(cx);
            return;
        }
        if self.click_change_bar(event.position, cx) {
            self.drag = None;
            self.changed(cx);
            return;
        }
        if event.click_count == 2 && self.double_click_inlay_hint(event.position, cx) {
            self.drag = None;
            self.changed(cx);
            return;
        }
        let offset = self.offset_for_position(event.position);
        if event.click_count == 1
            && let Some(invocation) = self.click_command(&event.modifiers, window, cx)
        {
            // A bound click places the caret, then runs its command there as
            // its key would, from the view: it neither drags nor extends.
            self.cancel_vim(cx);
            self.note_clicked_link(event.position);
            self.split_session(fe_vim::Splits::Moves, |model| model.set_cursor(offset), cx);
            self.drag = None;
            // The command may take the view away; a move finds it again.
            self.set_hover(None, cx);
            self.changed(cx);
            let action = RunCommand(invocation.source(Source::Mouse));
            let focus = self.focus.clone();
            // Once this listener is done: the view, which handles it first,
            // is out for it until then.
            window.defer(cx, move |window, cx| {
                focus.dispatch_action(&action, window, cx)
            });
            return;
        }
        if self.vim_owns_mouse(cx) {
            self.drag = Some(self.vim_mouse_down(event, cx));
            self.changed(cx);
            return;
        }
        if event.click_count == 1 && alt_only(&event.modifiers) {
            self.drag = if event.modifiers.shift {
                // A box, from here to wherever the pointer goes.
                let anchor = self.box_point_at(event.position);
                let select = |model: &mut EditorModel| model.select_box(anchor, anchor);
                self.split_session(fe_vim::Splits::Moves, select, cx);
                Some(Drag::Box(anchor))
            } else {
                let toggle = |model: &mut EditorModel| model.toggle_caret(offset);
                self.split_session(fe_vim::Splits::Moves, toggle, cx);
                None
            };
            self.sync_completion(cx);
            self.changed(cx);
            return;
        }
        let click = |model: &mut EditorModel| match event.click_count {
            2 => {
                let word = model.word_range_at(offset);
                model.select_range(word.clone(), false);
                Drag::Words(word)
            }
            3.. => {
                let row = model.line_range_at(offset);
                model.select_range(row.clone(), false);
                Drag::Rows(row)
            }
            _ => {
                if event.modifiers.shift {
                    model.extend_primary_to(offset);
                } else {
                    model.set_cursor(offset);
                }
                Drag::Chars
            }
        };
        // The word or row as selected now, which the split may have moved.
        let selected = |model: &EditorModel| model.primary_selection().range;
        self.drag = Some(match self.split_session(fe_vim::Splits::Moves, click, cx) {
            Drag::Words(_) => Drag::Words(selected(&self.model)),
            Drag::Rows(_) => Drag::Rows(selected(&self.model)),
            drag => drag,
        });
        self.sync_completion(cx);
        self.changed(cx);
    }

    /// A click on the sticky scroll row of the header at `buffer_row`: the
    /// caret goes to the header's first non-blank, with no selection, and
    /// the view scrolls so the header shows directly under the band its own
    /// enclosing scopes then form. It's a jump, as go-to-definition is: the
    /// host hears where from ([`EditorViewEvent::Jumped`]), and with vim on
    /// vim's jumplist gets it (`''` returns) as `m'` would, in insert mode
    /// too. As a click in the text does, it leaves vim's visual mode and
    /// ends a composition. A press on the band comes here once held keys
    /// have resolved ([`EditorView::settle_press`]), so the row, the text
    /// and the last frame agree.
    pub(crate) fn reveal_sticky_header(&mut self, buffer_row: u32, cx: &mut Context<Self>) {
        self.cancel_vim(cx);
        self.composition = None;
        // The header, and the jump's origin, through anchors: vim's jump (in
        // insert mode through `<C-o>`) and the move after it can end the
        // insert session, which edits the text: unused autoindent goes, a
        // block insert fills its rows.
        let header = self
            .model
            .snapshot()
            .anchor_before(self.model.text().row_start(buffer_row));
        self.record_vim_jump(cx);
        let from = (self.model.snapshot()).anchor_before(self.model.primary_selection().head());
        let to_header = |model: &mut EditorModel| {
            let row = sticky_header_row(model, &header);
            let text = model.text();
            let start = text.row_start(row);
            let line = text.slice_to_string(start..start + text.row_len(row) as usize);
            model.set_cursor(start + line.len() - line.trim_start().len());
        };
        self.split_session(fe_vim::Splits::Moves, to_header, cx);
        self.sync_completion(cx);
        self.changed(cx);
        // Directly under the band its own scopes form, which a frame finds
        // once its syntax has caught up with any edit.
        self.sticky_reveal = Some(header);
        self.autoscroll = false;
        self.clear_band = false;
        let from = self.model.snapshot().resolve(&from);
        cx.emit(EditorViewEvent::Jumped { from });
    }

    /// A click on a lens row the last frame painted: on an item, runs its
    /// command from the view once this listener is done, as a bound
    /// click's; elsewhere on the row, nothing. False off the lens rows.
    fn press_lens(&self, event: &MouseDownEvent, window: &mut Window, cx: &mut App) -> bool {
        let position = event.position;
        let Some(row) = (self.lens_rows.iter()).find(|row| row.bounds.contains(&position)) else {
            return false;
        };
        let item = (row.items.iter()).find(|(_, bounds)| bounds.contains(&position));
        if let Some((item, _)) = item {
            let action = RunCommand(item.invocation.clone().source(Source::Mouse));
            let focus = self.focus.clone();
            window.defer(cx, move |window, cx| {
                focus.dispatch_action(&action, window, cx)
            });
        }
        true
    }

    /// Adds a source of lens rows, shown from the next frame.
    pub fn add_lens_provider(
        &mut self,
        provider: std::rc::Rc<std::cell::RefCell<dyn crate::LensProvider>>,
    ) {
        self.lens_providers.push(provider);
    }

    /// Removes `provider`, if added, from the next frame.
    pub fn remove_lens_provider(
        &mut self,
        provider: &std::rc::Rc<std::cell::RefCell<dyn crate::LensProvider>>,
    ) {
        (self.lens_providers).retain(|added| !std::rc::Rc::ptr_eq(added, provider));
    }

    pub fn lens_providers(&self) -> &[std::rc::Rc<std::cell::RefCell<dyn crate::LensProvider>>] {
        &self.lens_providers
    }

    /// What a click with `modifiers` held runs here: the command its
    /// keystroke, such as `cmd-click`, is bound to in the view's context
    /// ([`EditorView::context_stack`]).
    fn click_command(
        &mut self,
        modifiers: &Modifiers,
        window: &Window,
        cx: &mut App,
    ) -> Option<Invocation> {
        // A click without a modifier can't be bound.
        if !modifiers.modified() {
            return None;
        }
        let keystroke = Keystroke::new(keymap::modifiers(modifiers), Key::Click);
        let stack = self.context_stack(window, cx)?;
        keymap::lookup(&keystroke, &stack, cx)
    }

    /// The view's context stack now, if known: the frames around it, then
    /// its own frame stamped from its live vim state, as a key's is (see
    /// [`crate::vim::live_stack`]). GPUI gives only the focused element's
    /// stack, so the frames around it are those it had when last focused
    /// and drawn; `None` for a view not focused since it was first drawn.
    fn context_stack(&mut self, window: &Window, cx: &mut App) -> Option<ContextStack> {
        self.note_ancestors(window);
        self.sync_vim(cx);
        let stack = (self.ancestors.clone()?).with(ContextFrame::new(KEY_CONTEXT));
        Some(crate::vim::stamp(stack, self.vim.as_ref()))
    }

    /// While the view is focused and drawn, records the frames around its
    /// own in GPUI's context stack.
    pub(crate) fn note_ancestors(&mut self, window: &Window) {
        if !self.focus.is_focused(window) {
            return;
        }
        let stack = keymap::context_stack(&window.context_stack());
        let frames = stack.frames();
        // Not drawn yet, GPUI gives the root's stack.
        let Some(own) = frames.iter().rposition(|frame| frame.name == KEY_CONTEXT) else {
            return;
        };
        self.ancestors = Some(frames[..own].iter().cloned().collect());
    }

    /// Finds the hover range for the pointer at `position`, `None` if it's
    /// off the text, with `modifiers` held: the link under it, or else the
    /// word, if their click is bound. Not while dragging.
    pub(crate) fn hover_at(
        &mut self,
        position: Option<Point<Pixels>>,
        modifiers: &Modifiers,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let range = position
            .filter(|_| self.drag.is_none())
            .filter(|_| self.click_command(modifiers, window, cx).is_some())
            .and_then(|position| (self.link_under(position)).or_else(|| self.word_under(position)));
        self.set_hover(range, cx);
    }

    /// [`EditorView::hover_at`] the pointer's position now, if it's over
    /// the view's text in the rendered frame.
    pub(crate) fn hover_at_pointer(
        &mut self,
        modifiers: &Modifiers,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let position = self
            .layout
            .as_ref()
            .and_then(|layout| crate::element::pointer_over_text(layout, window, cx));
        self.hover_at(position, modifiers, window, cx);
    }

    /// Once a frame is drawn: drops a hover range an edit or a scroll made
    /// stale, and finds one still standing again under the still pointer,
    /// which a change of layout or keymap may have moved or unbound, or new
    /// link ranges may have made.
    pub(crate) fn reconcile_hover(&mut self, window: &Window, cx: &mut Context<Self>) {
        // New links may lie under a pointer that found nothing.
        let links_changed = self.take_links_changed();
        if self.hover.is_none() && !links_changed {
            return;
        }
        if self.hover.is_some() && self.hover_range().is_none() {
            self.set_hover(None, cx);
            return;
        }
        self.hover_at_pointer(&window.modifiers(), window, cx);
    }

    /// The word around the glyph under a window position in the last frame,
    /// unless its first char is whitespace or there's none, as past a row's
    /// end. None either if the text changed since that frame, which GPUI
    /// doesn't redraw before a mouse event: its offsets may be past the end.
    fn word_under(&mut self, position: Point<Pixels>) -> Option<Range<usize>> {
        let text = self.model.text();
        let layout = self.layout.as_ref()?;
        if layout.version != self.model.snapshot().version() {
            return None;
        }
        let start = layout
            .char_at(text, position)
            .filter(|&start| start < text.len())?;
        let end = text.clip_offset(start + 1, Bias::Right);
        if text.slice_to_string(start..end).trim().is_empty() {
            return None;
        }
        Some(self.model.word_range_at(start))
    }

    /// Sets the hover range, or clears it with `None`.
    pub(crate) fn set_hover(&mut self, range: Option<Range<usize>>, cx: &mut Context<Self>) {
        let hover = range.map(|range| Hover {
            range,
            version: self.model.snapshot().version(),
            scroll: self.scroll,
        });
        if self.hover != hover {
            self.hover = hover;
            cx.notify();
        }
    }

    /// The word a bound click under the pointer would run its command on,
    /// unless an edit or a scroll has happened since it was found.
    pub fn hover_range(&self) -> Option<Range<usize>> {
        self.hover
            .as_ref()
            .filter(|hover| {
                hover.version == self.model.snapshot().version() && hover.scroll == self.scroll
            })
            .map(|hover| hover.range.clone())
    }

    /// For a host's root element's `on_modifiers_changed`: GPUI tells a
    /// modifier pressed or released only along the focused element's path,
    /// and an editor's own listener hears it only while it has focus. On
    /// the root, which is on every path, the editors in the window find
    /// their hover ranges with the new modifiers whatever has focus, such
    /// as a panel.
    pub fn modifiers_changed(event: &ModifiersChangedEvent, window: &mut Window, cx: &mut App) {
        crate::element::modifiers_changed(event, window, cx);
    }

    /// Clears a hover range made stale, or one that isn't `shown`, as in an
    /// inactive window, so it doesn't come back with a scroll back or
    /// reactivation. For paint, which redraws anyway.
    pub(crate) fn drop_stale_hover(&mut self, shown: bool) {
        if !shown || self.hover_range().is_none() {
            self.hover = None;
        }
    }

    /// Runs `select` on the model: a mouse click or drag, which moves carets
    /// without editing, or a fold click. In vim's insert mode it splits the
    /// session as an editor command does, `splits` saying when
    /// ([`fe_vim::VimState::split_session`]).
    fn split_session<R>(
        &mut self,
        splits: fe_vim::Splits,
        select: impl FnOnce(&mut EditorModel) -> R,
        cx: &mut Context<Self>,
    ) -> R {
        self.sync_vim(cx);
        match &mut self.vim {
            Some(state) => state.split_session(&mut self.model, splits, select),
            None => select(&mut self.model),
        }
    }

    /// Of a fold toggle or marker under a window position in the last frame,
    /// unfolds its row if it's folded or folds its candidate if not. Carets
    /// the fold hides move to its header, as with the keyboard. Returns
    /// whether there was one there.
    fn toggle_fold_at(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        let Some(layout) = &self.layout else {
            return false;
        };
        let at = layout.to_frame(position);
        let Some(area) = layout
            .frame
            .fold_click_areas()
            .find(|area| area.rect.contains(at))
        else {
            return false;
        };
        let row = area.buffer_row;
        let folded = area.folded;
        let candidate = if folded {
            None
        } else {
            match self.server_fold_ranges() {
                Some(ranges) => crate::server_folds::range_at(&ranges, row),
                None => synced_syntax(&self.syntax, &self.model)
                    .and_then(|syntax| fe_syntax::fold_range_at(syntax, row)),
            }
            .filter(|candidate| *candidate.start() == row)
        };
        // Like the fold commands, it splits an insert session whatever it
        // does to the carets.
        self.split_session(
            fe_vim::Splits::Other,
            |model| {
                if folded {
                    model.unfold_rows(row..=row);
                } else if let Some(candidate) = candidate {
                    model.fold_rows(candidate);
                }
            },
            cx,
        );
        true
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.clone() else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.drag = None;
            return;
        }
        // Wherever the pointer goes ([`EditorView::drag_minimap`]).
        if let Drag::Minimap(_) = drag {
            return;
        }
        if self.vim_owns_mouse(cx) {
            self.vim_mouse_drag(drag, event.position, cx);
            return;
        }
        if let Drag::Box(anchor) = drag {
            let head = self.box_point_at(event.position);
            self.split_session(
                fe_vim::Splits::Moves,
                |model| model.select_box(anchor, head),
                cx,
            );
            self.changed(cx);
            return;
        }
        let offset = self.offset_for_position(event.position);
        self.split_session(
            fe_vim::Splits::Moves,
            |model| {
                let (origin, under) = match drag {
                    Drag::Chars => {
                        model.extend_primary_to(offset);
                        return;
                    }
                    Drag::Box(_) | Drag::Minimap(_) => return,
                    Drag::Words(origin) => (origin, model.word_range_at(offset)),
                    Drag::Rows(origin) => (origin, model.line_range_at(offset)),
                };
                // Grow from the originally clicked word or row, whole units
                // at a time.
                if under.start < origin.start {
                    model.select_range(under.start..origin.end, true);
                } else {
                    model.select_range(origin.start..under.end.max(origin.end), false);
                }
            },
            cx,
        );
        self.changed(cx);
    }

    /// The buffer row and display column (in chars) under a window position,
    /// in the last frame, for a box selection. Past a row's end the column
    /// goes on, a space's width at a time, so a box can be wider than the
    /// row.
    fn box_point_at(&mut self, position: Point<Pixels>) -> (u32, usize) {
        let offset = self.offset_for_position(position);
        let text = self.model.text();
        // From the shaped row where it can, so a position inside an expanded
        // tab keeps its column; else through the offset it snaps to.
        let direct =
            (self.layout.as_ref()).and_then(|layout| layout.display_column_at(text, position));
        let (row, column) = direct.unwrap_or_else(|| {
            let row = text.offset_to_point(offset).row;
            let expanded = self.model.expanded_row(row);
            let column = expanded.char_column(expanded.to_display(offset - text.row_start(row)));
            (row, column)
        });
        let past =
            (self.layout.as_mut()).map_or(0, |layout| layout.columns_past_row_end(text, position));
        (row, column + past)
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        match self.drag.take() {
            // The slider goes back to its colour at rest.
            Some(Drag::Minimap(_)) => cx.notify(),
            Some(_) => self.write_primary(cx),
            None => {}
        }
    }

    /// Whether vim takes the mouse's clicks and drags: with vim on, outside
    /// insert mode, as nvim's default `mouse=nvi` does.
    fn vim_owns_mouse(&mut self, cx: &App) -> bool {
        self.sync_vim(cx);
        self.vim.is_some() && !self.accepts_text()
    }

    /// Where vim puts the cursor for a window position: on the grapheme
    /// under it, or past a row's end at that end.
    fn vim_offset_for_position(&mut self, position: Point<Pixels>) -> usize {
        let text = self.model.text();
        let under = self
            .layout
            .as_ref()
            .and_then(|layout| layout.char_at(text, position));
        under.unwrap_or_else(|| self.offset_for_position(position))
    }

    /// A click with vim owning the mouse. A plain click drops a pending
    /// command, leaves a visual mode (`vim.visual_exit`, so `gv` brings it
    /// back) and puts the caret at it; a shift-click extends charwise visual mode to it; a double
    /// click selects its word in charwise visual mode, and a triple click
    /// its row in linewise visual mode. Returns what a drag extends.
    fn vim_mouse_down(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) -> Drag {
        let at = self.vim_offset_for_position(event.position);
        let visual = self.vim_visual_kind();
        match event.click_count {
            2 => {
                let word = self.model.word_range_at(at);
                self.select_visual(VisualKind::Char, word.start, last_of(&word));
                Drag::Words(word)
            }
            3.. => {
                self.select_visual(VisualKind::Line, at, at);
                Drag::Rows(self.model.line_range_at(at))
            }
            _ if event.modifiers.shift => {
                let anchor = self.vim_anchor();
                self.select_visual(visual.unwrap_or(VisualKind::Char), anchor, at);
                Drag::Chars
            }
            _ => {
                self.cancel_vim(cx);
                self.model.set_cursor(at);
                Drag::Chars
            }
        }
    }

    /// A drag with vim owning the mouse: from the caret it started at, or
    /// the visual anchor, into charwise visual mode (or the visual mode it's
    /// in) over the grapheme under the pointer; from a double click's word,
    /// whole words at a time; from a triple click's row, linewise.
    fn vim_mouse_drag(&mut self, drag: Drag, position: Point<Pixels>, cx: &mut Context<Self>) {
        let at = self.vim_offset_for_position(position);
        match drag {
            Drag::Chars => {
                let anchor = self.vim_anchor();
                let kind = self.vim_visual_kind();
                // Still on the grapheme it started at: not a drag yet.
                if kind.is_none() && at == anchor {
                    return;
                }
                self.select_visual(kind.unwrap_or(VisualKind::Char), anchor, at);
            }
            Drag::Words(origin) => {
                let under = self.model.word_range_at(at);
                if under.start < origin.start {
                    self.select_visual(VisualKind::Char, last_of(&origin), under.start);
                } else {
                    // An empty row's word is empty: the cursor goes on it.
                    let last = match under.end > origin.end {
                        true => last_of(&under),
                        false => last_of(&origin),
                    };
                    self.select_visual(VisualKind::Char, origin.start, last);
                }
            }
            Drag::Rows(origin) => self.select_visual(VisualKind::Line, origin.start, at),
            // Begun in insert mode, which a key has since left: vim's modes
            // have no box.
            Drag::Box(_) | Drag::Minimap(_) => return,
        }
        self.changed(cx);
    }

    /// The visual mode's kind, if vim is in one.
    fn vim_visual_kind(&self) -> Option<VisualKind> {
        match self.vim.as_ref()?.mode() {
            fe_vim::Mode::Visual(kind) => Some(kind),
            _ => None,
        }
    }

    /// The visual anchor in a visual mode, or else the primary caret.
    fn vim_anchor(&self) -> usize {
        self.vim
            .as_ref()
            .and_then(|vim| vim.visual_anchor(&self.model))
            .unwrap_or_else(|| self.model.primary_selection().head())
    }

    /// Enters the visual mode of `kind` over `anchor` and `cursor`, or moves
    /// the current one there ([`fe_vim::VimState::select_visual`]).
    fn select_visual(&mut self, kind: VisualKind, anchor: usize, cursor: usize) {
        if let Some(vim) = &mut self.vim {
            vim.select_visual(&mut self.model, kind, anchor, cursor);
        }
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let line_height = self
            .layout
            .as_ref()
            .map_or(px(20.), |layout| px(layout.frame.line_height));
        let delta = event.delta.pixel_delta(line_height);
        self.set_hover(None, cx);
        self.hide_popover(cx);
        self.scroll.x = (self.scroll.x - delta.x).max(px(0.));
        self.scroll.y = (self.scroll.y - delta.y).max(px(0.));
        // The element clamps against the content size on the next frame.
        self.autoscroll = false;
        self.band_spares_caret = false;
        self.clear_band = false;
        self.sticky_reveal = None;
        cx.notify();
    }

    /// Brings the syntax tree up to date with the text: within this frame if
    /// the reparse is quick, otherwise on a background thread while the stale
    /// tree keeps highlighting. A file too big to parse whole gets a window
    /// around the text the last frame showed, parsed in the background.
    fn sync_syntax(&mut self, cx: &mut Context<Self>) {
        let visible = self
            .layout
            .as_ref()
            .map_or(0..0, |layout| shown_text(&layout.frame.rows));
        if let Some(syntax) = &mut self.syntax {
            let language = syntax.syntax().language();
            syntax.sync(
                self.model.snapshot(),
                visible,
                &mut ParseSpawner { cx, language },
            );
        }
    }

    /// Compiles the injected languages the last frame's highlights lacked,
    /// such as a Markdown fence's, in the background, then draws again.
    pub(crate) fn load_injected_languages(&mut self, cx: &mut Context<Self>) {
        let Some(syntax) = &self.syntax else {
            return;
        };
        #[cfg(test)]
        if let Some(hook) = &mut self.before_taking_missing {
            hook(syntax.syntax());
        }
        // Taken even while a task runs: its redraw highlights again, and
        // finds any still missing.
        let missing = syntax.syntax().take_missing_languages();
        if missing.is_empty() || self.injected_task.is_some() {
            return;
        }
        self.injected_task = Some(cx.spawn(async move |this, cx| {
            cx.background_spawn(async move {
                for name in missing {
                    fe_syntax::language_by_name(name);
                }
            })
            .await;
            this.update(cx, |view, cx| {
                view.injected_task = None;
                view.highlight_epoch += 1;
                cx.notify();
            })
            .ok();
        }));
    }
}

/// The buffer row of the sticky scroll header `anchor` marks the start
/// of, in `model`'s text now.
pub(crate) fn sticky_header_row(model: &EditorModel, anchor: &fe_buffer::Anchor) -> u32 {
    let at = model.snapshot().resolve(anchor);
    model.text().offset_to_point(at).row
}

/// The offset of `range`'s last byte, which vim's cursor takes at its
/// grapheme, or its start if it's empty.
/// Whether `modifiers` are alt, and maybe shift, alone: an alt-click adds
/// or removes a caret, and an alt-shift-drag selects a box.
fn alt_only(modifiers: &Modifiers) -> bool {
    modifiers.alt && !modifiers.control && !modifiers.platform && !modifiers.function
}

fn last_of(range: &Range<usize>) -> usize {
    range.end.saturating_sub(1).max(range.start)
}

/// Measures chars in `theme`'s font, for soft wrap.
fn glyph_widths(theme: &EditorTheme, cx: &App) -> Box<GlyphWidths> {
    let font = theme.font();
    Box::new(GlyphWidths::new(
        cx.text_system().clone(),
        &font,
        theme.font_size,
    ))
}

/// The text a frame shows, to parse a window around: of its longest run of
/// rows with nothing hidden between them, from the first shaped byte to the
/// last. A fold hides rows, and only part of a long row is shaped.
fn shown_text<L>(rows: &[RowLayout<L>]) -> Range<usize> {
    rows.chunk_by(|above, below| {
        below.buffer_row <= above.buffer_row + 1
            && above.shown().end == above.end
            && below.shown().start == below.start
    })
    .max_by_key(|run| run.len())
    .map_or(0..0, |run| {
        run[0].shown().start..run[run.len() - 1].shown().end
    })
}

/// The syntax tree, if it's been edited to match `model`'s text.
pub(crate) fn synced_syntax<'a>(
    syntax: &'a Option<SyntaxDriver<Task<()>>>,
    model: &EditorModel,
) -> Option<&'a Syntax> {
    let syntax = syntax.as_ref()?.syntax();
    (syntax.version() == model.snapshot().version()).then_some(syntax)
}

fn syntax_driver(language: &'static Language, snapshot: &BufferSnapshot) -> SyntaxDriver<Task<()>> {
    SyntaxDriver::new(Syntax::new(language, snapshot), SYNC_PARSE_BUDGET)
}

/// Runs a view's parses on GPUI's background executor and hands the results
/// back to it.
struct ParseSpawner<'a, 'b> {
    cx: &'a mut Context<'b, EditorView>,
    /// The language being parsed.
    language: &'static Language,
}

impl Spawner for ParseSpawner<'_, '_> {
    type Task = Task<()>;

    fn spawn(&mut self, job: ParseJob) -> Task<()> {
        let language = self.language;
        self.cx.spawn(async move |this, cx| {
            let parsed = cx.background_spawn(async move { job.run() }).await;
            this.update(cx, |view, cx| {
                // `set_language` drops the driver, cancelling this, but a
                // tree of another language must never be installed: the
                // driver only checks the buffer and version.
                if let Some(syntax) = &mut view.syntax
                    && std::ptr::eq(syntax.syntax().language(), language)
                {
                    syntax.finish(parsed, &mut ParseSpawner { cx, language });
                    view.highlight_epoch += 1;
                }
                // Repaint, and reparse whatever changed or scrolled into
                // view meanwhile.
                cx.notify();
            })
            .ok();
        })
    }
}

impl Render for EditorView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_syntax(cx);
        // With no syntax there's no window to ask for.
        let windowed = self.syntax.as_ref().is_some_and(|driver| {
            self.model.snapshot().len() > driver.syntax().language().parse_limit()
        });
        if windowed {
            // This frame may scroll; once it's painted, ask for a window
            // around what it shows.
            cx.defer_in(window, |view, _, cx| view.sync_syntax(cx));
        }
        self.sync_vim(cx);
        self.settle_which_key();
        // Vim left insert mode, or came on or went off, while composing.
        self.drop_stale_composition();
        self.sync_find(cx);
        if self.hover_blur.is_none() {
            // Blur listeners run at the end of a draw, which asks for no
            // frame after it: the notify waits until it's over. Losing focus
            // drops a composition too.
            let blur = cx.on_blur(&self.focus, window, |view, window, cx| {
                let composing = view.composition.take().is_some();
                view.autoscroll |= composing;
                if view.hover.take().is_some() || composing {
                    cx.defer_in(window, |_, _, cx| cx.notify());
                }
            });
            self.hover_blur = Some(blur);
        }
        let mut key_context = KeyContext::default();
        key_context.add(KEY_CONTEXT);
        if let Some(state) = &self.vim {
            crate::vim::add_to_key_context(&mut key_context, state);
        }
        if self.shows_open_hunk() {
            key_context.add("proposal");
        }
        for &flag in &self.key_flags {
            key_context.add(flag);
        }
        if self.completion_subscriptions.is_empty() {
            Self::close_completion_on_escape_and_blur(self, window, cx);
        }
        if self.ghost_text_subscriptions.is_empty() {
            Self::dismiss_ghost_text_on_blur(self, window, cx);
        }
        if !self.signature_help_subscribed() {
            self.hide_signature_help_on_escape_and_blur(window, cx);
        }
        self.drop_stale_completion();
        self.drop_stale_ghost_text(cx);
        self.sync_signature_help(cx);
        self.forget_blurred_ghost_text_if_focused(window);
        if self.completion_shown() {
            key_context.add(crate::completion_menu::KEY_CONTEXT);
        }
        if self.ghost_text_shown() {
            key_context.add(ghost_text::KEY_CONTEXT);
        }
        div()
            .id("editor")
            .key_context(key_context)
            .track_focus(&self.focus)
            .size_full()
            // Hover, completion and signature help inherit it for their code.
            .font_features(self.theme.font_features.clone())
            .cursor(CursorStyle::IBeam)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_middle_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .on_modifiers_changed(crate::element::modifiers_changed)
            .on_action(cx.listener(|view, run: &RunCommand, window, cx| {
                view.end_composition_for(&run.0, cx);
                view.on_run_command(run, window, cx)
            }))
            .on_key_down(cx.listener(Self::on_vim_key))
            .child(EditorElement::new(cx.entity()))
            .child(crate::hover::popover_layer(cx))
            .child(crate::completion_menu::menu_layer(cx))
            .child(crate::signature_help::popover_layer(cx))
            .child(overlay_layer(cx))
            .children(crate::vim::which_key::popup_layer(self, cx))
    }
}

impl Focusable for EditorView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// Writes `text` to the primary selection. GPUI has one on Linux and FreeBSD
/// only; elsewhere this does nothing.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn write_primary_text(cx: &mut App, text: String) {
    cx.write_to_primary(gpui::ClipboardItem::new_string(text));
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
fn write_primary_text(_: &mut App, _: String) {}

/// The primary selection's text, where GPUI has one (Linux and FreeBSD).
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn read_primary_text(cx: &App) -> Option<String> {
    cx.read_from_primary().and_then(|item| item.text())
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
fn read_primary_text(_: &App) -> Option<String> {
    None
}

mod change_bar;
mod find;
pub(crate) mod ghost_text;
mod host;
mod ime;
mod minimap;
mod proposal;
mod proposal_commands;

/// What a click on the gutter glyph calls.
type GlyphClick = std::rc::Rc<dyn Fn(&mut Window, &mut App)>;

/// A glyph the host has the gutter draw in its mark slot, in place of the
/// row's diagnostic dot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GutterGlyph {
    /// The buffer row it's on.
    pub row: u32,
    pub kind: GlyphKind,
}

/// Which glyph: VS Code's two lightbulbs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlyphKind {
    /// Code actions are available.
    Lightbulb,
    /// A preferred quick fix is available.
    LightbulbAutofix,
}

/// A view the host draws under a range of the buffer.
struct Overlay {
    view: AnyView,
    /// The anchored range, which follows edits.
    anchor: Range<fe_buffer::Anchor>,
    /// Where the last frame puts its top left, in window coordinates, and
    /// the editor's bounds; `None` while the anchor's row isn't shown.
    placed: Option<(Point<Pixels>, Bounds<Pixels>)>,
}

impl EditorView {
    /// Draws `view` at the text area's left edge, just under the last row
    /// of `anchor`, a range of bytes that follows edits, or removes the
    /// overlay with `None`. There is one: a second replaces the first. It
    /// is hidden while the row is out of view, and keeps focus and the
    /// pointer's events to itself.
    pub fn set_overlay(&mut self, view: Option<AnyView>, anchor: Range<usize>) {
        let text = self.model.text();
        let (start, end) = (anchor.start.min(text.len()), anchor.end.min(text.len()));
        let snapshot = self.model.snapshot();
        self.overlay = view.map(|view| Overlay {
            view,
            anchor: snapshot.anchor_before(start)..snapshot.anchor_after(end.max(start)),
            placed: None,
        });
    }

    /// Removes the overlay.
    pub fn clear_overlay(&mut self) {
        self.overlay = None;
    }

    /// The buffer row the overlay goes under, as edits have moved it: the
    /// anchor's last row, or the one before it for a range that ends at the
    /// start of a row.
    pub fn overlay_anchor_row(&self) -> Option<usize> {
        let overlay = self.overlay.as_ref()?;
        let snapshot = self.model.snapshot();
        let range = snapshot.resolve_range(&overlay.anchor);
        let end = if range.end > range.start {
            range.end - 1
        } else {
            range.end
        };
        let text = self.model.text();
        Some(text.offset_to_point(end).row as usize)
    }

    /// Sets where the frame being drawn puts the overlay.
    pub(crate) fn place_overlay(&mut self, at: Option<(Point<Pixels>, Bounds<Pixels>)>) {
        if let Some(overlay) = &mut self.overlay {
            overlay.placed = at;
        }
    }

    /// The overlay laid out and placed, if the frame shows its row: its top
    /// left at the text area's left, clamped into the editor.
    fn layout_overlay(&mut self, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
        let overlay = self.overlay.as_ref()?;
        let (at, editor) = overlay.placed?;
        // Its content may be larger than the editor, which cuts it short, so
        // it never reaches (or takes the input of) what's beside the editor.
        let mut element = div()
            .occlude()
            .max_w(editor.size.width)
            .max_h(editor.size.height)
            .overflow_hidden()
            .debug_selector(|| "editor-overlay".into())
            .child(overlay.view.clone())
            .into_any_element();
        let space = size(AvailableSpace::MaxContent, AvailableSpace::MaxContent);
        let laid = element.layout_as_root(space, window, cx);
        let x = at.x.min(editor.right() - laid.width).max(editor.left());
        let y = at.y.min(editor.bottom() - laid.height).max(editor.top());
        element.prepaint_at(point(x, y), window, cx);
        Some(element)
    }

    /// Shows `glyph` in the gutter's mark slot, or clears it. The row
    /// follows edits. A left click on it calls `on_click`, and does nothing
    /// else.
    pub fn set_gutter_glyph(&mut self, glyph: Option<GutterGlyph>, on_click: Option<GlyphClick>) {
        let text = self.model.text();
        self.gutter_glyph = glyph.map(|glyph| {
            let row = glyph.row.min(text.row_count().saturating_sub(1));
            let at = self.model.snapshot().anchor_after(text.row_start(row));
            (at, glyph.kind)
        });
        self.glyph_click = on_click;
    }

    /// The glyph, its row as edits have moved it.
    pub fn gutter_glyph(&self) -> Option<GutterGlyph> {
        let (at, kind) = self.gutter_glyph.as_ref()?;
        let offset = self.model.snapshot().resolve(at);
        let row = self.model.text().offset_to_point(offset).row;
        Some(GutterGlyph { row, kind: *kind })
    }

    /// Calls the glyph's callback for a left click on it. Returns whether
    /// the click hit it.
    fn click_glyph(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let hit = self
            .glyph_hit
            .filter(|bounds| bounds.contains(&event.position));
        if hit.is_none() || event.button != MouseButton::Left {
            return false;
        }
        if let Some(on_click) = self.glyph_click.clone() {
            // The host may update this view, which is busy until then.
            window.defer(cx, move |window, cx| on_click(window, cx));
        }
        true
    }
}

pub use ghost_text::GhostTextEvent;
pub use host::{EditorViewEvent, HostBlock, HostBlockKind, ReservedBlock};
pub use minimap::minimap_options;
pub(crate) use proposal::ProposalButton;
pub use proposal::ProposalEvent;
pub(crate) use proposal_commands::{ProposalCommands, registry as proposal_registry};

#[cfg(test)]
mod ime_tests;

#[cfg(test)]
mod lens_bounds_tests;

#[cfg(all(test, any(target_os = "linux", target_os = "freebsd")))]
#[path = "view/overlay_tests.rs"]
mod overlay_tests;

#[cfg(all(test, any(target_os = "linux", target_os = "freebsd")))]
mod primary_selection_tests;

#[cfg(test)]
mod language_tests;

#[cfg(test)]
mod window_tests;

#[cfg(test)]
mod reveal_tests;

#[cfg(test)]
mod click_prefix_tests;

#[cfg(test)]
mod find_tests;

#[cfg(test)]
mod change_bar_click_tests;

#[cfg(test)]
mod fold_hover_tests;

#[cfg(test)]
mod mouse_binding_tests;

#[cfg(test)]
mod diagnostics_tests;

#[cfg(test)]
mod hover_tests;

#[cfg(test)]
mod sticky_scroll_click_tests;

#[cfg(test)]
mod sticky_scroll_caret_tests;

#[cfg(test)]
mod completion_tests;

#[cfg(test)]
mod multi_cursor_tests;

#[cfg(test)]
mod primary_reveal_tests;

#[cfg(test)]
mod diff_seams_tests;

#[cfg(test)]
mod proposal_tests;

#[cfg(test)]
mod proposal_accept_tests;

#[cfg(test)]
mod proposal_command_tests;

#[cfg(test)]
mod ghost_text_tests;

#[cfg(test)]
mod ghost_text_key_tests;

/// Soft wrap leaving room for text after a line.
#[cfg(test)]
mod wrap_reserve_tests {
    use std::{cell::RefCell, ops::Range, rc::Rc};

    use fe_buffer::{Buffer, BufferSnapshot};
    use fe_editor::{
        EditorModel,
        decorations::{Decoration, DecorationKind, DecorationProvider, Tint},
    };
    use gpui::{Entity, TestAppContext, VisualTestContext};

    use crate::{EditorTheme, EditorView, init};

    /// A count after the first row, as a commit box shows a subject's length.
    struct Count;

    impl DecorationProvider for Count {
        fn decorations(
            &self,
            snapshot: &BufferSnapshot,
            _: Range<usize>,
            out: &mut Vec<Decoration>,
        ) {
            let text = snapshot.text();
            let row = 0..text.row_len(0) as usize;
            out.push(Decoration::new(
                row,
                DecorationKind::AfterLine("120/72".into()),
                Tint::Warning,
            ));
        }
    }

    /// A soft-wrapped editor with a [`Count`], drawn.
    fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
        cx.update(init);
        let (view, cx) = cx.add_window_view(|_, cx| {
            let mut model = EditorModel::new(Buffer::new("x"), None);
            model.set_soft_wrap(true);
            model.add_decoration_provider(Rc::new(RefCell::new(Count)));
            EditorView::new(model, EditorTheme::default_dark(cx), cx)
        });
        draw(cx);
        (view, cx)
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    /// Replaces `view`'s text with `text` and draws it.
    fn set_text(view: &Entity<EditorView>, text: &str, cx: &mut VisualTestContext) {
        view.update(cx, |view, cx| {
            let len = view.model.text().len();
            (view.model.buffer_mut()).edit([(0..len, text)], fe_buffer::Origin::User);
            cx.notify();
        });
        draw(cx);
    }

    /// With `ems` reserved, fills the first row's pieces to the wrap width
    /// and draws it: how far the count's right edge is inside the text
    /// area's, in pixels (negative where it's cut off).
    fn room(view: &Entity<EditorView>, ems: f32, cx: &mut VisualTestContext) -> f32 {
        view.update(cx, |view, cx| view.set_wrap_reserve(ems, cx));
        set_text(view, &"x".repeat(1000), cx);
        // As many as a piece holds, twice.
        let piece = view.read_with(cx, |view, _| {
            let row = &view.layout.as_ref().expect("drawn").frame.rows[0];
            row.end - row.start
        });
        set_text(view, &"x".repeat(piece * 2), cx);
        view.read_with(cx, |view, _| {
            let layout = view.layout.as_ref().expect("drawn");
            assert_eq!(layout.frame.rows.len(), 2, "two full pieces");
            let text = layout.frame.text_bounds();
            let right = f32::from(layout.origin.x) + text.x + text.width;
            let count = (layout.decorations.placed.iter())
                .find(|placed| matches!(placed.kind, DecorationKind::AfterLine(_)))
                .expect("the count is placed");
            right - f32::from(count.bounds.origin.x + count.bounds.size.width)
        })
    }

    #[gpui::test]
    fn a_reserve_leaves_room_after_a_wrapped_row(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        // By default a full row's count runs past the edge.
        assert!(room(&view, 0., cx) < 0.);
        // Six ems more fit its gap and six characters.
        let room = room(&view, 6., cx);
        assert!(room >= 0., "{room}");
    }
}

/// An editor with its gutter hidden.
#[cfg(test)]
mod gutter_tests {
    use fe_buffer::Buffer;
    use fe_editor::EditorModel;
    use gpui::{TestAppContext, VisualTestContext};

    use crate::{EditorTheme, EditorView, init};

    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    #[gpui::test]
    fn without_a_gutter_the_text_starts_at_the_left_and_wraps_wider(cx: &mut TestAppContext) {
        cx.update(init);
        let (view, cx) = cx.add_window_view(|_, cx| {
            let mut model = EditorModel::new(Buffer::new("a\nb"), None);
            model.set_soft_wrap(true);
            EditorView::new(model, EditorTheme::default_dark(cx), cx)
        });
        draw(cx);
        let shown = |cx: &mut VisualTestContext| {
            view.read_with(cx, |view, _| {
                let frame = &view.layout.as_ref().expect("drawn").frame;
                let wrap = view.model.display().wrap_width().expect("soft wrap is on");
                (frame.text_left, frame.line_numbers.len(), wrap)
            })
        };
        let (gutter_left, numbers, gutter_wrap) = shown(cx);
        assert_eq!(numbers, 2);

        view.update(cx, |view, cx| view.set_gutter(false, cx));
        draw(cx);
        let (left, numbers, wrap) = shown(cx);
        assert_eq!(numbers, 0);
        assert!(left < gutter_left / 4., "{left} vs {gutter_left}");
        // The gutter's room goes to the text.
        assert!(
            wrap >= gutter_wrap + (gutter_left - left),
            "{wrap} vs {gutter_wrap}"
        );
    }
}

/// Where the view's overlay is drawn: over everything else in the window.
fn overlay_layer(cx: &mut Context<EditorView>) -> impl IntoElement {
    let view = cx.entity().downgrade();
    let layer = canvas(
        move |_, window, cx| {
            view.update(cx, |view, cx| view.layout_overlay(window, cx))
                .ok()?
        },
        |_, overlay, window, cx| {
            if let Some(mut overlay) = overlay {
                overlay.paint(window, cx);
            }
        },
    );
    deferred(layer.absolute())
}
