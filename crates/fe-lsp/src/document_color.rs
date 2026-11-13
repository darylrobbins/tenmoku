//! `textDocument/documentColor` and `textDocument/colorPresentation`: the
//! colour literals a server finds in a document (CSS, SCSS, Tailwind…),
//! and the ways it can write a colour back, for a picker.
//!
//! - **Servers.** Merged (docs/decisions/q5j03ax2af-lsp-add-on-servers.md
//!   §5): every server of the document with `colorProvider` is asked at
//!   once, at the one version. With none, nothing is sent: `Ok(None)`. A
//!   range shows one swatch, so where two servers' colours have the same
//!   range the earlier server's (in server order, the lead first) wins;
//!   colours at other ranges are all kept. A server that fails or times
//!   out contributes nothing; the answer fails only if every server did.
//! - **Owner.** Each colour carries the server that found it, as its
//!   session had the document open ([`ColorInformation::owner`]). Its
//!   presentations are asked of that session alone, never another server
//!   nor one the server restarted into: then nothing is sent, and the
//!   answer is `Unavailable("the language server restarted")`. A server
//!   without `colorProvider` (or with it `false`) gets no presentation
//!   request: `Unavailable("colorPresentation")`.
//! - **Ranges.** Decoded against the text the server had at the request's
//!   version, as bytes of that version, sorted by start.
//! - **Colours.** Red, green, blue and alpha, each clamped to `0..=1`.
//! - **Failures.** A `null` reply is none. A request past
//!   `lsp.timeouts.hover` (3 s) is none too, and cancels the request.

use std::ops::Range;

use fe_buffer::{BufferId, Version};
use futures::FutureExt;
use lsp_types::request::Request;
use lsp_types::{
    ColorPresentationParams, ColorProviderCapability, DocumentColorParams, ServerCapabilities,
    TextDocumentIdentifier,
};

use crate::document::Admitted;
use crate::encoding::to_lsp;
use crate::event::LspError;
use crate::manager::{LspManager, Owner};
use crate::requests::{Gate, by_precedence, merged};

/// A colour a server found in a document.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct ColorInformation {
    /// What spells it, in bytes of the version the request was made on.
    pub range: Range<usize>,
    /// Red, green, blue and alpha, each in `0..=1`.
    pub color: [f32; 4],
    /// The server that found it, and the document as its session had it
    /// open: presentations are asked of that session alone.
    owner: Option<Owner>,
}

impl ColorInformation {
    /// A colour no server owns: its presentations are asked of the lead
    /// of the buffer they're for.
    pub fn new(range: Range<usize>, color: [f32; 4]) -> Self {
        Self {
            range,
            color,
            owner: None,
        }
    }

    /// The server that found the colour, with the opening of the document
    /// it was found in.
    pub fn owner(&self) -> Option<Owner> {
        self.owner
    }
}

/// One way a server can write a colour: `rgb(…)`, `#rrggbb`…
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ColorPresentation {
    /// What a picker shows; also what's written when there's no
    /// `text_edit`.
    pub label: String,
    /// What writes it, in bytes of the version asked about.
    pub text_edit: Option<(Range<usize>, String)>,
    /// Other edits that go with it, such as an import, in bytes of the
    /// version asked about.
    pub additional_edits: Vec<(Range<usize>, String)>,
}

/// What `Unavailable` says when the server doesn't provide colours.
const FEATURE: &str = "documentColor";

/// What `Unavailable` says when the server can't present them.
const PRESENTATION: &str = "colorPresentation";

/// `textDocument/documentColor`, whose reply may be `null`, which
/// lsp-types 0.95.1 can't decode as its list.
enum DocumentColor {}

impl Request for DocumentColor {
    type Params = DocumentColorParams;
    type Result = Option<Vec<lsp_types::ColorInformation>>;
    const METHOD: &'static str = lsp_types::request::DocumentColor::METHOD;
}

/// `textDocument/colorPresentation`, as [`DocumentColor`] is.
enum ColorPresentationRequest {}

impl Request for ColorPresentationRequest {
    type Params = ColorPresentationParams;
    type Result = Option<Vec<lsp_types::ColorPresentation>>;
    const METHOD: &'static str = lsp_types::request::ColorPresentationRequest::METHOD;
}

impl LspManager {
    /// The colours in open buffer `buffer` at its `version`, merged across
    /// its servers with `colorProvider` (see the [module](self)), each
    /// owned by the server that found it: each sent only if its server has
    /// that version, as any [`document_request`](Self::document_request)
    /// is.
    ///
    /// `Ok(None)` if no server provides colours: nothing is sent.
    /// `Ok(Some(vec![]))` if they have none, or don't say within
    /// `lsp.timeouts.hover` (which also cancels the request). Dropping the
    /// future cancels every request still out. It doesn't borrow the
    /// manager.
    pub fn document_colors(
        &self,
        buffer: BufferId,
        version: Version,
    ) -> impl Future<Output = Result<Option<Vec<ColorInformation>>, LspError>> + Send + 'static + use<>
    {
        let asks = self.ask_each::<DocumentColor, _>(
            buffer,
            version,
            Gate::Advertised,
            FEATURE,
            provides_colors,
            |at| DocumentColorParams {
                text_document: TextDocumentIdentifier {
                    uri: at.document.uri.clone(),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            },
            |server, reply, admitted| {
                let owner = Some(Owner::of(server, &admitted));
                (reply.result.unwrap_or_default().into_iter())
                    .map(|info| ColorInformation {
                        range: admitted.offsets(info.range),
                        color: components(info.color),
                        owner,
                    })
                    .collect::<Vec<_>>()
            },
        );
        merged(asks).map(|answer| match answer {
            Ok(Some(parts)) => {
                let mut colors = by_precedence(parts, |info| &info.range);
                colors.sort_by_key(|info| (info.range.start, info.range.end));
                Ok(Some(colors))
            }
            Ok(None) => Ok(None),
            Err(LspError::Timeout) => Ok(Some(Vec::new())),
            Err(err) => Err(err),
        })
    }

    /// The ways the server can write `info`'s colour over `info`'s range,
    /// found in open buffer `buffer` at its `version`, with the edits in
    /// bytes of that version. Sent only while the server has that version,
    /// and only to `info`'s [`owner`](ColorInformation::owner), while its
    /// session has the document open as it had it: otherwise nothing is
    /// sent, and the answer is `Unavailable("the language server
    /// restarted")`. A colour with no owner goes to `buffer`'s lead.
    ///
    /// A server without `colorProvider` is
    /// `Unavailable("colorPresentation")`, and nothing is sent. One that
    /// answers `null`, or doesn't answer within `lsp.timeouts.hover` (which
    /// also cancels the request), has none.
    pub fn color_presentations(
        &self,
        buffer: BufferId,
        version: Version,
        info: ColorInformation,
    ) -> impl Future<Output = Result<Vec<ColorPresentation>, LspError>> + Send + 'static + use<>
    {
        let owner = info.owner;
        let params = move |at: &Admitted| {
            let text = &at.document.sent.text;
            let len = text.len();
            let (start, end) = (info.range.start.min(len), info.range.end.min(len));
            let [red, green, blue, alpha] = info.color;
            ColorPresentationParams {
                text_document: TextDocumentIdentifier {
                    uri: at.document.uri.clone(),
                },
                color: lsp_types::Color {
                    red,
                    green,
                    blue,
                    alpha,
                },
                range: lsp_types::Range {
                    start: to_lsp(text, start, at.encoding),
                    end: to_lsp(text, end, at.encoding),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            }
        };
        let reply = match owner {
            Some(owner) => self
                .owner_request::<ColorPresentationRequest>(
                    owner,
                    version,
                    PRESENTATION,
                    provides_colors,
                    params,
                )
                .boxed(),
            None => self
                .document_request_if_provided::<ColorPresentationRequest>(
                    buffer,
                    version,
                    PRESENTATION,
                    provides_colors,
                    params,
                )
                .boxed(),
        };
        reply
            .map(|answer| match answer {
                Ok((reply, admitted)) => Ok((reply.result.unwrap_or_default().into_iter())
                    .map(|presentation| convert(presentation, &admitted))
                    .collect()),
                Err(LspError::Timeout) => Ok(Vec::new()),
                Err(err) => Err(err),
            })
            .boxed()
    }
}

/// `color`'s components, each clamped to `0..=1`.
fn components(color: lsp_types::Color) -> [f32; 4] {
    let clamp = |c: f32| if c.is_nan() { 0. } else { c.clamp(0., 1.) };
    [color.red, color.green, color.blue, color.alpha].map(clamp)
}

/// A reply's presentation, its edits in bytes of the version asked about.
fn convert(presentation: lsp_types::ColorPresentation, at: &Admitted) -> ColorPresentation {
    let edit = |edit: lsp_types::TextEdit| (at.offsets(edit.range), edit.new_text);
    ColorPresentation {
        label: presentation.label,
        text_edit: presentation.text_edit.map(edit),
        additional_edits: (presentation
            .additional_text_edits
            .unwrap_or_default()
            .into_iter())
        .map(edit)
        .collect(),
    }
}

/// Whether the server provides document colours.
fn provides_colors(capabilities: &ServerCapabilities) -> bool {
    !matches!(
        capabilities.color_provider,
        None | Some(ColorProviderCapability::Simple(false))
    )
}

#[cfg(test)]
#[path = "document_color_tests.rs"]
mod tests;
