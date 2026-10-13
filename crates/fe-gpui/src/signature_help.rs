//! Signature help (docs/decisions/lsp-client.md §7): the signatures of the
//! call the caret is in, from the view's [`SignatureHelpProvider`] (a
//! language server's, say), in a popover above the caret's row with the
//! active parameter in bold.
//!
//! - **Asking.** Typing one of the provider's trigger characters asks
//!   ([`SignatureTrigger::Character`]). While help shows, or is awaited,
//!   typing a retrigger or trigger character asks again with it, and any
//!   other edit or caret move with [`SignatureTrigger::ContentChange`]:
//!   both as a retrigger while it shows, with the help shown as `active`.
//!   `editor.show_signature_help` asks with [`SignatureTrigger::Invoked`].
//!   Asking again replaces the request. With vim on, only insert mode
//!   asks.
//! - **Answers.** One for a buffer version or caret the view has moved on
//!   from, or landing with a selection or outside insert mode, is dropped.
//!   `None` hides the help.
//! - **Hiding.** `escape`, losing focus, a selection, leaving insert mode,
//!   or the caret moving off the rows it was asked on hides it, and drops
//!   the request.
//! - **Popover.** In the hover popover's style, over the caret's row, or
//!   below it with no room for it above in the editor. An open completion
//!   menu on that side pushes it further out, its laid-out bounds (its
//!   documentation's too) measured against the popover's in the same
//!   frame; with room for it on neither side, it goes where there's more,
//!   cut to fit, so it's never over the menu. It shows the active
//!   signature's label in the editor's font, its active parameter in bold,
//!   `n/m` before it with several signatures, then the active parameter's
//!   documentation and the signature's, as text, [`MAX_DOC_ROWS`] rows at
//!   most. It takes neither focus nor keys.

use std::{future::Future, ops::Range, pin::Pin, rc::Rc};

use fe_buffer::{BufferId, Version};
use gpui::{
    AnyElement, AvailableSpace, Bounds, Context, FontWeight, HighlightStyle, IntoElement,
    ParentElement as _, Pixels, SharedString, Styled as _, StyledText, Subscription, Task, Window,
    canvas, deferred, div, point, px, size,
};

use crate::{
    EditorView,
    hover::{Block, UI_FONT, markdown_blocks},
};

/// Asks for signature help at the caret.
pub const SHOW: &str = "editor.show_signature_help";

/// The most rows of documentation the popover shows.
pub(crate) const MAX_DOC_ROWS: usize = 10;

/// Why signature help is asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignatureTrigger {
    /// By `editor.show_signature_help`.
    Invoked,
    /// By typing one of the provider's trigger or retrigger characters.
    Character(char),
    /// By the text or the caret changing while help shows.
    ContentChange,
}

/// What a request says about why it's made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureContext {
    pub trigger: SignatureTrigger,
    /// Whether help was showing.
    pub is_retrigger: bool,
    /// The help showing, if any.
    pub active: Option<SignatureHelp>,
}

/// The signatures a call could match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureHelp {
    pub signatures: Vec<Signature>,
    /// Which of `signatures` is active; always one of them.
    pub active_signature: usize,
    /// The active parameter of signatures with none of their own.
    pub active_parameter: Option<usize>,
}

/// One signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    pub label: String,
    /// Markdown or plain text.
    pub documentation: Option<String>,
    pub parameters: Vec<Parameter>,
    /// The active parameter, if this signature says which.
    pub active_parameter: Option<usize>,
}

/// One of a signature's parameters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parameter {
    /// Its text, in bytes of the signature's label.
    pub range: Range<usize>,
    /// Markdown or plain text.
    pub documentation: Option<String>,
}

impl SignatureHelp {
    /// The active signature, and the range of its active parameter's text
    /// in its label, if it has one inside the label.
    fn active(&self) -> Option<(&Signature, Option<&Parameter>)> {
        let signature = self.signatures.get(self.active_signature)?;
        let parameter = (signature.active_parameter.or(self.active_parameter))
            .and_then(|ix| signature.parameters.get(ix));
        Some((signature, parameter))
    }
}

/// A [`SignatureHelpProvider`]'s answer to come: `None` for no help,
/// whatever the reason. Dropping it should cancel the work.
pub type SignatureHelpFuture = Pin<Box<dyn Future<Output = Option<SignatureHelp>>>>;

/// Where a view's signature help comes from, set by its host
/// ([`EditorView::set_signature_help_provider`]): fe-gpui knows nothing of
/// language servers.
pub trait SignatureHelpProvider {
    /// The characters that ask for help when typed in `buffer`, and those
    /// that ask again while it shows; `None` for no help there.
    fn triggers(&self, buffer: BufferId) -> Option<(Vec<char>, Vec<char>)>;

    /// The help for byte `offset` of `buffer`, at its `version`.
    fn signature_help(
        &self,
        buffer: BufferId,
        version: Version,
        offset: usize,
        context: SignatureContext,
    ) -> SignatureHelpFuture;
}

/// The view's signature help: the help shown, the request in flight, and
/// where the frame being drawn puts it.
#[derive(Default)]
pub(crate) struct SignaturePopover {
    provider: Option<Rc<dyn SignatureHelpProvider>>,
    shown: Option<SignatureHelp>,
    /// The version and caret last asked at, and the buffer rows asked on
    /// since help was first asked for: while there's help shown or awaited.
    asked: Option<Asked>,
    /// The request in flight: dropping it cancels it.
    request: Option<Task<()>>,
    /// The caret's bounds and the editor's in the frame being drawn, which
    /// the popover is placed by.
    anchor: Option<(Bounds<Pixels>, Bounds<Pixels>)>,
    /// Hide it on `escape` and on losing focus.
    subscriptions: Vec<Subscription>,
    /// What the last frame showed.
    #[cfg(test)]
    pub(crate) painted: Option<Painted>,
}

#[derive(Clone, Debug, PartialEq)]
struct Asked {
    version: Version,
    caret: usize,
    rows: Range<u32>,
}

/// What the last frame's popover showed.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Painted {
    /// `n/m`, with several signatures.
    pub(crate) counter: Option<String>,
    pub(crate) label: String,
    /// The text set in bold.
    pub(crate) bold: Option<String>,
    pub(crate) documentation: Vec<String>,
    /// Whether it's above the caret.
    pub(crate) above: bool,
    /// Its bounds as laid out, in window coordinates.
    pub(crate) bounds: Bounds<Pixels>,
}

impl EditorView {
    /// Sets where signature help comes from, or none. Help shown or awaited
    /// from the last one goes.
    pub fn set_signature_help_provider(&mut self, provider: Option<Rc<dyn SignatureHelpProvider>>) {
        self.clear_signature_help();
        self.signature_help.provider = provider;
    }

    pub fn signature_help_provider(&self) -> Option<&Rc<dyn SignatureHelpProvider>> {
        self.signature_help.provider.as_ref()
    }

    /// The help shown, if any.
    pub fn signature_help_shown(&self) -> Option<&SignatureHelp> {
        self.signature_help.shown.as_ref()
    }

    /// Whether help may be asked for, or shown: with no selection, and with
    /// vim on, in insert mode.
    fn signature_help_allowed(&self) -> bool {
        let insert = self.vim.is_none() || self.typing_mode() == Some(false);
        insert && self.model.primary_selection().range.is_empty()
    }

    /// Asks the provider for help at the caret, `trigger` saying why: as a
    /// retrigger while help shows. Where it's not allowed
    /// ([`Self::signature_help_allowed`]), hides it instead.
    pub(crate) fn ask_signature_help(&mut self, trigger: SignatureTrigger, cx: &mut Context<Self>) {
        self.sync_vim(cx);
        let Some(provider) = self.signature_help.provider.clone() else {
            return;
        };
        if !self.signature_help_allowed() {
            self.hide_signature_help(cx);
            return;
        }
        let snapshot = self.model.snapshot();
        let caret = self.model.primary_selection().head();
        let (buffer, version) = (snapshot.id(), snapshot.version());
        let row = self.model.text().offset_to_point(caret).row;
        let popover = &mut self.signature_help;
        let rows = match &popover.asked {
            Some(asked) => asked.rows.start.min(row)..asked.rows.end.max(row + 1),
            None => row..row + 1,
        };
        popover.asked = Some(Asked {
            version,
            caret,
            rows,
        });
        let context = SignatureContext {
            trigger,
            is_retrigger: popover.shown.is_some(),
            active: popover.shown.clone(),
        };
        let reply = provider.signature_help(buffer, version, caret, context);
        popover.request = Some(cx.spawn(async move |view, cx| {
            let help = reply.await;
            view.update(cx, |view, cx| {
                view.signature_help_landed(version, caret, help, cx)
            })
            .ok();
        }));
    }

    fn signature_help_landed(
        &mut self,
        version: Version,
        caret: usize,
        help: Option<SignatureHelp>,
        cx: &mut Context<Self>,
    ) {
        self.sync_vim(cx);
        if !self.signature_help_allowed() {
            self.hide_signature_help(cx);
            return;
        }
        // For text or a caret the view has moved on from: what asks there
        // is in flight, or has hidden it.
        let now = (
            self.model.snapshot().version(),
            self.model.primary_selection().head(),
        );
        if now != (version, caret) {
            return;
        }
        let popover = &mut self.signature_help;
        popover.request = None;
        match help.filter(|help| help.active().is_some()) {
            Some(help) => {
                popover.shown = Some(help);
                cx.notify();
            }
            None => self.hide_signature_help(cx),
        }
    }

    /// Typing `text` asks for help if it ends with a trigger character, or,
    /// while help shows or is awaited, with a retrigger character.
    pub(crate) fn signature_help_typed(&mut self, text: &str, cx: &mut Context<Self>) {
        let Some(provider) = &self.signature_help.provider else {
            return;
        };
        let Some((triggers, retriggers)) = provider.triggers(self.model.buffer().id()) else {
            return;
        };
        let Some(typed) = text.chars().last() else {
            return;
        };
        let active = self.signature_help.asked.is_some();
        if triggers.contains(&typed) || (active && retriggers.contains(&typed)) {
            self.ask_signature_help(SignatureTrigger::Character(typed), cx);
        }
    }

    /// Before each frame: while help shows or is awaited, hides it where
    /// it's not allowed ([`Self::signature_help_allowed`]) or the caret has
    /// moved off the rows it was asked on, and else asks again where an
    /// edit or a move has taken the caret since it last asked.
    pub(crate) fn sync_signature_help(&mut self, cx: &mut Context<Self>) {
        let Some(asked) = &self.signature_help.asked else {
            return;
        };
        let version = self.model.snapshot().version();
        let caret = self.model.primary_selection().head();
        let row = self.model.text().offset_to_point(caret).row;
        // A move, not an edit, off the rows it was asked on.
        let moved_off = asked.version == version && !asked.rows.contains(&row);
        if moved_off || !self.signature_help_allowed() {
            self.hide_signature_help(cx);
            return;
        }
        if (asked.version, asked.caret) != (version, caret) {
            self.ask_signature_help(SignatureTrigger::ContentChange, cx);
        }
    }

    /// Drops the help and its request; whether help was shown.
    fn clear_signature_help(&mut self) -> bool {
        let popover = &mut self.signature_help;
        popover.asked = None;
        popover.request = None;
        popover.anchor = None;
        popover.shown.take().is_some()
    }

    /// Hides the help, and drops its request.
    pub(crate) fn hide_signature_help(&mut self, cx: &mut Context<Self>) {
        if self.clear_signature_help() {
            cx.notify();
        }
    }

    /// Has `escape` hide the help, wherever it's pressed, before any binding
    /// takes it, and losing focus hide it too.
    pub(crate) fn hide_signature_help_on_escape_and_blur(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let this = cx.weak_entity();
        let escape = cx.intercept_keystrokes(move |event, _, cx| {
            if event.keystroke.key == "escape" {
                this.update(cx, |view, cx| view.hide_signature_help(cx))
                    .ok();
            }
        });
        // Blur listeners run at the end of a draw, which asks for no frame
        // after it: the notify waits until it's over.
        let blur = cx.on_blur(&self.focus, window, |view, window, cx| {
            if view.clear_signature_help() {
                cx.defer_in(window, |_, _, cx| cx.notify());
            }
        });
        self.signature_help.subscriptions = vec![escape, blur];
    }

    pub(crate) fn signature_help_subscribed(&self) -> bool {
        !self.signature_help.subscriptions.is_empty()
    }

    /// The caret, for the frame being laid out to place the help by, if it
    /// shows.
    pub(crate) fn signature_help_caret(&self) -> Option<usize> {
        self.signature_help.shown.as_ref()?;
        Some(self.model.primary_selection().head())
    }

    /// Sets what the help is placed by in the frame being drawn: `caret`,
    /// the bounds of the caret, if the frame shows it, in an editor of
    /// `editor` bounds. Without a caret it isn't drawn.
    pub(crate) fn anchor_signature_help(
        &mut self,
        caret: Option<Bounds<Pixels>>,
        editor: Bounds<Pixels>,
    ) {
        self.signature_help.anchor = caret.map(|caret| (caret, editor));
    }

    /// The popover laid out and placed, if help shows and the frame being
    /// drawn shows the caret: over the caret's row, or under it with no
    /// room there in the editor, clear of the completion menu this frame
    /// has laid out, if one shows. With room for it on neither side, it's
    /// cut to the side with more; with less than a row, it isn't drawn.
    pub(crate) fn layout_signature_help(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        #[cfg(test)]
        {
            self.signature_help.painted = None;
        }
        let (caret, editor) = self.signature_help.anchor?;
        let help = self.signature_help.shown.clone()?;
        let menu = self.completion_bounds();
        // The space above the caret and the menu, unless it's all below the
        // caret, and below them, unless it's all above: a menu by a word
        // that soft wrap has begun on a row above the caret's spans it, and
        // bounds both.
        let menu_reaches_over = menu.filter(|menu| menu.top() < caret.bottom() - px(1.));
        let menu_reaches_under = menu.filter(|menu| menu.bottom() > caret.top() + px(1.));
        let over = menu_reaches_over.map_or(caret.top(), |menu| menu.top().min(caret.top()));
        let under =
            menu_reaches_under.map_or(caret.bottom(), |menu| menu.bottom().max(caret.bottom()));
        let (room_above, room_below) = (over - editor.top(), editor.bottom() - under);
        let viewport = window.viewport_size();
        let space = size(
            AvailableSpace::Definite(viewport.width),
            AvailableSpace::MaxContent,
        );
        let mut popover = self.signature_help_element(&help, None)?;
        let mut laid = popover.layout_as_root(space, window, cx);
        let above =
            laid.height <= room_above || (laid.height > room_below && room_above >= room_below);
        let room = if above { room_above } else { room_below };
        if laid.height > room {
            let line_height = self.theme.font_size * self.theme.line_height;
            if room < line_height {
                return None;
            }
            popover = self.signature_help_element(&help, Some(room))?;
            laid = popover.layout_as_root(space, window, cx);
        }
        let y = if above { over - laid.height } else { under };
        let x = caret.left().min(viewport.width - laid.width).max(px(0.));
        let origin = point(x, y);
        popover.prepaint_at(origin, window, cx);
        #[cfg(test)]
        {
            let (signature, parameter) = help.active()?;
            let bold = (parameter.map(|parameter| parameter.range.clone()))
                .and_then(|range| Some(signature.label.get(range)?.to_owned()))
                .filter(|bold| !bold.is_empty());
            let docs = documentation(signature, parameter);
            self.signature_help.painted = Some(Painted {
                counter: counter(&help),
                label: signature.label.clone(),
                bold,
                documentation: docs.iter().map(|block| block.text().to_owned()).collect(),
                above,
                bounds: Bounds::new(origin, laid),
            });
        }
        Some(popover)
    }

    /// `help`'s popover, `max_height` tall at most.
    fn signature_help_element(
        &self,
        help: &SignatureHelp,
        max_height: Option<Pixels>,
    ) -> Option<AnyElement> {
        let (signature, parameter) = help.active()?;
        let theme = &self.theme;
        let bold = (parameter.map(|parameter| parameter.range.clone()))
            .filter(|range| !range.is_empty() && signature.label.get(range.clone()).is_some());
        let highlights = bold.map(|range| {
            let style = HighlightStyle {
                font_weight: Some(FontWeight::BOLD),
                ..HighlightStyle::default()
            };
            (range, style)
        });
        let label = StyledText::new(signature.label.clone()).with_highlights(highlights);
        let header = div()
            .flex()
            .gap_2()
            .font_family(theme.font_family.clone())
            .children(counter(help).map(|counter| {
                (div().font_family(UI_FONT))
                    .text_color(theme.gutter_foreground)
                    .child(counter)
            }))
            .child(label);
        let line_height = theme.font_size * theme.line_height;
        let docs = documentation(signature, parameter)
            .into_iter()
            .map(|block| {
                let font: SharedString = match &block {
                    Block::Text(_) => UI_FONT.into(),
                    Block::Code(_) => theme.font_family.clone(),
                };
                div()
                    .font_family(font)
                    .child(SharedString::from(block.text().to_owned()))
            });
        let mut popover = div()
            .max_w(px(560.))
            .px_2()
            .py_1()
            .flex()
            .flex_col()
            .gap_1()
            .overflow_hidden()
            .bg(theme.popover_background)
            .border_1()
            .border_color(theme.popover_border)
            .rounded_sm()
            .text_color(theme.foreground)
            .text_size(theme.font_size)
            .child(header)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .max_h(line_height * MAX_DOC_ROWS as f32)
                    .overflow_hidden()
                    .children(docs),
            );
        if let Some(max_height) = max_height {
            popover = popover.max_h(max_height);
        }
        Some(popover.into_any_element())
    }
}

/// `n/m`, for help with several signatures.
fn counter(help: &SignatureHelp) -> Option<String> {
    (help.signatures.len() > 1)
        .then(|| format!("{}/{}", help.active_signature + 1, help.signatures.len()))
}

/// The active parameter's documentation, then `signature`'s, as text and
/// code blocks: [`MAX_DOC_ROWS`] lines of them at most.
fn documentation(signature: &Signature, parameter: Option<&Parameter>) -> Vec<Block> {
    let texts = (parameter.and_then(|parameter| parameter.documentation.as_deref()))
        .into_iter()
        .chain(signature.documentation.as_deref());
    let mut rows = MAX_DOC_ROWS;
    let mut blocks = Vec::new();
    for block in texts.flat_map(markdown_blocks) {
        if rows == 0 {
            break;
        }
        let lines: Vec<&str> = block.text().lines().take(rows).collect();
        rows -= lines.len();
        let text = lines.join("\n");
        blocks.push(match block {
            Block::Text(_) => Block::Text(text),
            Block::Code(_) => Block::Code(text),
        });
    }
    blocks
}

/// Where the view's signature help is drawn: over everything else in the
/// window, laid out after the completion menu's layer
/// ([`crate::completion_menu::menu_layer`]), whose bounds it keeps clear of,
/// so it must come after it among the view's children.
pub(crate) fn popover_layer(cx: &mut Context<EditorView>) -> impl IntoElement {
    let view = cx.entity().downgrade();
    let layer = canvas(
        move |_, window, cx| {
            view.update(cx, |view, cx| view.layout_signature_help(window, cx))
                .ok()?
        },
        |_, popover, window, cx| {
            if let Some(mut popover) = popover {
                popover.paint(window, cx);
            }
        },
    );
    deferred(layer.absolute())
}

#[cfg(test)]
#[path = "signature_help_tests.rs"]
mod tests;
