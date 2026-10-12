//! `textDocument/signatureHelp`: the signatures of the call at an offset,
//! with the active parameter.
//!
//! - **Parameters.** Each is the byte range of its text in its signature's
//!   label: from `labelOffsets` (UTF-16 offsets into the label, whatever
//!   the session's position encoding), or by finding a string label in the
//!   label, left to right, each after the one before. One a label doesn't
//!   hold is an empty range at the label's end.
//! - **Active ones.** An active signature or parameter past the end is
//!   clamped to the last; a signature with no parameters has no active
//!   parameter.
//! - **Failures.** A timeout (`lsp.timeouts.hover`'s) is no help. Other
//!   failures are errors.

use std::ops::Range;

use fe_buffer::{BufferId, Version};
use fe_text::Bias;
use futures::FutureExt;
use lsp_types::request::SignatureHelpRequest;
use lsp_types::{
    Documentation, MarkupContent, ParameterInformation, ParameterLabel, SignatureHelpContext,
    SignatureHelpParams, SignatureHelpTriggerKind, SignatureInformation, TextDocumentIdentifier,
    TextDocumentPositionParams,
};

use crate::document::Admitted;
use crate::encoding::to_lsp;
use crate::event::LspError;
use crate::manager::LspManager;

/// Why signature help is asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignatureTrigger {
    /// By the user.
    Invoked,
    /// By typing one of the server's trigger or retrigger characters.
    Character(char),
    /// By the text changing while help is showing.
    ContentChange,
}

/// What a signature help request says about why it's made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureContext {
    pub trigger: SignatureTrigger,
    /// Whether help was already showing.
    pub is_retrigger: bool,
    /// The help showing, if any.
    pub active: Option<SignatureHelp>,
}

impl SignatureContext {
    pub fn new(trigger: SignatureTrigger) -> Self {
        Self {
            trigger,
            is_retrigger: false,
            active: None,
        }
    }
}

/// The signatures a call could match.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SignatureHelp {
    pub signatures: Vec<Signature>,
    /// Which of `signatures` is active; always one of them.
    pub active_signature: usize,
    /// The active parameter of signatures with none of their own.
    pub active_parameter: Option<usize>,
}

impl SignatureHelp {
    pub fn new(
        signatures: Vec<Signature>,
        active_signature: usize,
        active_parameter: Option<usize>,
    ) -> Self {
        Self {
            signatures,
            active_signature,
            active_parameter,
        }
    }
}

/// One signature.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Signature {
    pub label: String,
    /// Markdown or plain text.
    pub documentation: Option<String>,
    /// Each parameter's text, in bytes of `label`.
    pub parameters: Vec<Range<usize>>,
    /// The active parameter, if this signature says which.
    pub active_parameter: Option<usize>,
}

impl Signature {
    pub fn new(
        label: impl Into<String>,
        documentation: Option<String>,
        parameters: Vec<Range<usize>>,
        active_parameter: Option<usize>,
    ) -> Self {
        Self {
            label: label.into(),
            documentation,
            parameters,
            active_parameter,
        }
    }
}

impl LspManager {
    /// The signature help for byte `offset` of open buffer `buffer`, at
    /// its `version`: sent only if the server has that version, as any
    /// [`document_request`](Self::document_request) is.
    ///
    /// `Ok(None)` if the server has no signatures, or doesn't say within
    /// the timeout (which also cancels the request). Dropping the future
    /// cancels it too. It doesn't borrow the manager.
    pub fn signature_help(
        &self,
        buffer: BufferId,
        version: Version,
        offset: usize,
        context: SignatureContext,
    ) -> impl Future<Output = Result<Option<SignatureHelp>, LspError>> + Send + 'static + use<>
    {
        let reply = self.document_request::<SignatureHelpRequest>(buffer, version, move |at| {
            SignatureHelpParams {
                text_document_position_params: position(at, offset),
                work_done_progress_params: Default::default(),
                context: Some(request_context(&context)),
            }
        });
        reply.boxed().map(|answer| match answer {
            Ok((reply, _)) => Ok(reply.result.and_then(convert)),
            Err(LspError::Timeout) => Ok(None),
            Err(err) => Err(err),
        })
    }

    /// The characters that ask the server of open buffer `buffer` for
    /// signature help when typed, and those that ask again while help is
    /// showing, once it has initialized.
    pub fn signature_help_triggers(&self, buffer: BufferId) -> Option<(Vec<char>, Vec<char>)> {
        let server = self.server_of(buffer)?;
        let options = self.capabilities(server)?.signature_help_provider?;
        let chars = |strings: Option<Vec<String>>| {
            (strings.unwrap_or_default().iter())
                .filter_map(|s| s.chars().next())
                .collect::<Vec<_>>()
        };
        Some((
            chars(options.trigger_characters),
            chars(options.retrigger_characters),
        ))
    }
}

/// The request's document and offset, as the server has them.
fn position(at: &Admitted, offset: usize) -> TextDocumentPositionParams {
    let text = &at.document.sent.text;
    TextDocumentPositionParams {
        text_document: TextDocumentIdentifier {
            uri: at.document.uri.clone(),
        },
        position: to_lsp(text, text.clip_offset(offset, Bias::Left), at.encoding),
    }
}

fn request_context(context: &SignatureContext) -> SignatureHelpContext {
    let (trigger_kind, trigger_character) = match context.trigger {
        SignatureTrigger::Invoked => (SignatureHelpTriggerKind::INVOKED, None),
        SignatureTrigger::Character(c) => (
            SignatureHelpTriggerKind::TRIGGER_CHARACTER,
            Some(c.to_string()),
        ),
        SignatureTrigger::ContentChange => (SignatureHelpTriggerKind::CONTENT_CHANGE, None),
    };
    SignatureHelpContext {
        trigger_kind,
        trigger_character,
        is_retrigger: context.is_retrigger,
        active_signature_help: (context.active.as_ref()).map(active_help),
    }
}

/// `help` as the protocol has it, its parameters as label offsets.
fn active_help(help: &SignatureHelp) -> lsp_types::SignatureHelp {
    let signatures = (help.signatures.iter())
        .map(|signature| {
            let utf16 = |offset: usize| {
                let end = offset.min(signature.label.len());
                (signature.label.get(..end)).map_or(0, |head| head.encode_utf16().count()) as u32
            };
            SignatureInformation {
                label: signature.label.clone(),
                documentation: (signature.documentation.clone()).map(Documentation::String),
                parameters: Some(
                    (signature.parameters.iter())
                        .map(|range| ParameterInformation {
                            label: ParameterLabel::LabelOffsets([
                                utf16(range.start),
                                utf16(range.end),
                            ]),
                            documentation: None,
                        })
                        .collect(),
                ),
                active_parameter: signature.active_parameter.map(|n| n as u32),
            }
        })
        .collect();
    lsp_types::SignatureHelp {
        signatures,
        active_signature: Some(help.active_signature as u32),
        active_parameter: help.active_parameter.map(|n| n as u32),
    }
}

/// A reply as [`SignatureHelp`], if it has signatures.
fn convert(help: lsp_types::SignatureHelp) -> Option<SignatureHelp> {
    if help.signatures.is_empty() {
        return None;
    }
    let last = help.signatures.len() - 1;
    let active_signature = (help.active_signature.unwrap_or(0) as usize).min(last);
    let signatures: Vec<Signature> = help.signatures.into_iter().map(signature).collect();
    // The help's own is clamped against the active signature's parameters.
    let count = signatures[active_signature].parameters.len();
    let active_parameter = clamp(help.active_parameter, count);
    Some(SignatureHelp {
        signatures,
        active_signature,
        active_parameter,
    })
}

fn signature(info: SignatureInformation) -> Signature {
    let label = info.label;
    let mut from = 0;
    let parameters: Vec<Range<usize>> = (info.parameters.unwrap_or_default().into_iter())
        .map(|parameter| {
            let range = match parameter.label {
                ParameterLabel::LabelOffsets([start, end]) => {
                    let start = byte_of(&label, start);
                    start..byte_of(&label, end).max(start)
                }
                ParameterLabel::Simple(text) => (label[from..].find(&text))
                    .map_or(label.len()..label.len(), |at| {
                        from + at..from + at + text.len()
                    }),
            };
            from = range.end;
            range
        })
        .collect();
    let active_parameter = clamp(info.active_parameter, parameters.len());
    Signature {
        label,
        documentation: info.documentation.map(|documentation| match documentation {
            Documentation::String(text)
            | Documentation::MarkupContent(MarkupContent { value: text, .. }) => text,
        }),
        parameters,
        active_parameter,
    }
}

/// `active` as an index of `count` things: the last if it's past the end.
fn clamp(active: Option<u32>, count: usize) -> Option<usize> {
    Some((active? as usize).min(count.checked_sub(1)?))
}

/// The byte of `label` at UTF-16 offset `units`: the label's end if it's
/// past it, the start of the character it's inside.
fn byte_of(label: &str, units: u32) -> usize {
    let mut seen = 0;
    for (byte, c) in label.char_indices() {
        if seen + c.len_utf16() > units as usize {
            return byte;
        }
        seen += c.len_utf16();
    }
    label.len()
}

#[cfg(test)]
#[path = "signature_help_tests.rs"]
mod tests;
