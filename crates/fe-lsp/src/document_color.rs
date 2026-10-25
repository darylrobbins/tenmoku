//! `textDocument/documentColor` and `textDocument/colorPresentation`: the
//! colour literals a server finds in a document (CSS, SCSS, Tailwind…),
//! and the ways it can write a colour back, for a picker.
//!
//! - **Provider.** A server without `colorProvider` (or with it `false`)
//!   gets no request: colours are `Ok(None)`, presentations
//!   `Unavailable("colorPresentation")`.
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
use crate::manager::LspManager;

/// A colour a server found in a document.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct ColorInformation {
    /// What spells it, in bytes of the version the request was made on.
    pub range: Range<usize>,
    /// Red, green, blue and alpha, each in `0..=1`.
    pub color: [f32; 4],
}

impl ColorInformation {
    pub fn new(range: Range<usize>, color: [f32; 4]) -> Self {
        Self { range, color }
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
    /// The colours in open buffer `buffer` at its `version`: sent only if
    /// the server has that version, as any
    /// [`document_request`](Self::document_request) is.
    ///
    /// `Ok(None)` if the server doesn't provide colours: nothing is sent.
    /// `Ok(Some(vec![]))` if it has none, or doesn't say within
    /// `lsp.timeouts.hover` (which also cancels the request). Dropping the
    /// future cancels it too. It doesn't borrow the manager.
    pub fn document_colors(
        &self,
        buffer: BufferId,
        version: Version,
    ) -> impl Future<Output = Result<Option<Vec<ColorInformation>>, LspError>> + Send + 'static + use<>
    {
        let reply = self.document_request_if_provided::<DocumentColor>(
            buffer,
            version,
            FEATURE,
            provides_colors,
            |at| DocumentColorParams {
                text_document: TextDocumentIdentifier {
                    uri: at.document.uri.clone(),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            },
        );
        // Boxed, as its type names the manager's borrow.
        reply
            .boxed()
            .map(|answer| match answer {
                Ok((reply, admitted)) => {
                    let mut colors: Vec<_> = (reply.result.unwrap_or_default().into_iter())
                        .map(|info| ColorInformation {
                            range: admitted.offsets(info.range),
                            color: components(info.color),
                        })
                        .collect();
                    colors.sort_by_key(|info| (info.range.start, info.range.end));
                    Ok(Some(colors))
                }
                Err(LspError::Timeout) => Ok(Some(Vec::new())),
                Err(LspError::Unavailable(why)) if why == FEATURE => Ok(None),
                Err(err) => Err(err),
            })
            .boxed()
    }

    /// The ways the server can write `info`'s colour over `info`'s range,
    /// found in open buffer `buffer` at its `version`, with the edits in
    /// bytes of that version. Sent only while the server has that version.
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
        let reply = self.document_request_if_provided::<ColorPresentationRequest>(
            buffer,
            version,
            PRESENTATION,
            provides_colors,
            move |at| {
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
            },
        );
        reply
            .boxed()
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
