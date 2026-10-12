//! Pulling typescript-language-server's diagnostics through tsserver
//! ([`PullDiagnostics::TsserverRequest`](crate::PullDiagnostics::TsserverRequest),
//! docs/decisions/lsp-client.md §4.1, and
//! docs/decisions/rq81bj4nvm-tsserver-diagnostics.md).
//!
//! - **The requests.** A pull is three `workspace/executeCommand`
//!   requests for `typescript.tsserverRequest`, with the arguments
//!   `[<command>, { "file": <uri> }]`, for [`COMMANDS`] in order. The
//!   actor sends them back to back: the first is queued like any pull,
//!   and hands the other two over as it's sent ([`Lead`]), for the actor to
//!   send right behind it, so no notification gets between them.
//! - **The replies.** Each is tsserver's response object, whose `body` is
//!   the diagnostic array. Each diagnostic's `start` and `end` are
//!   `{ line, offset }`, both 1-based, `offset` counting UTF-16 code units
//!   whatever position encoding the session negotiated. They become LSP
//!   diagnostics with 0-based UTF-16 positions, decoded against the ring's
//!   text like any pulled list (pull.rs), but always as utf-16. A reply
//!   that isn't a successful response drops the pull; an entry that isn't
//!   a diagnostic is skipped.

use std::sync::{Arc, Mutex, PoisonError};

use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::BoxFuture;
use lsp_types::request::{ExecuteCommand, Request};
use lsp_types::{DiagnosticSeverity, DiagnosticTag, NumberOrString, Position, Range, Url};
use serde_json::{Value, json};

use crate::event::LspError;
use crate::requests::{Issue, Issuer, Out, Reply, Settled, call};

/// The command typescript-language-server runs tsserver commands through.
pub(crate) const COMMAND: &str = "typescript.tsserverRequest";

/// The tsserver commands of a pull, in the order they're sent.
pub(crate) const COMMANDS: [&str; 3] = [
    "syntacticDiagnosticsSync",
    "semanticDiagnosticsSync",
    "suggestionDiagnosticsSync",
];

/// `workspace/executeCommand` with its result as JSON: lsp-types 0.95.1
/// has it as `Option<Value>`, which reads a `null` result as `None`.
enum Execute {}

impl Request for Execute {
    type Params = Value;
    type Result = Value;
    const METHOD: &'static str = ExecuteCommand::METHOD;
}

/// Requests sent as their lead is, for the actor to send right behind it.
pub(crate) type Released = Arc<Mutex<Vec<Issue>>>;

/// The first request of a pull, carrying the others: as it's sent, it
/// moves them to `released`.
struct Lead {
    first: Issue,
    rest: Vec<Issue>,
    released: Released,
}

impl Issuer for Lead {
    fn gone(&self) -> bool {
        self.first.gone()
    }

    #[cfg(test)]
    fn method(&self) -> &'static str {
        self.first.method()
    }

    fn issue(self: Box<Self>, out: Out<'_>) -> BoxFuture<'static, Settled> {
        let Lead {
            first,
            rest,
            released,
        } = *self;
        (released.lock().unwrap_or_else(PoisonError::into_inner)).extend(rest);
        first.issue(out)
    }
}

/// A pull of `uri`'s diagnostics: the request to queue, and its replies,
/// once all three have come. The first error among them answers at once,
/// cancelling the others.
pub(crate) fn pull(
    uri: &Url,
    released: &Released,
) -> (Issue, BoxFuture<'static, Result<[Value; 3], LspError>>) {
    let mut issues = Vec::new();
    let mut replies = Vec::new();
    for command in COMMANDS {
        let (answer, answered) = oneshot::channel();
        let params = json!({ "command": COMMAND, "arguments": [command, { "file": uri }] });
        issues.push(call::<Execute, _>(answer, params, |_, reply| {
            reply.map(|Reply { result, .. }| result)
        }));
        replies.push(answered.map(|answer| answer.unwrap_or(Err(LspError::ServerDown))));
    }
    let [a, b, c]: [_; 3] = replies.try_into().ok().expect("one reply per command");
    let replies = futures::future::try_join3(a, b, c).map(|replies| {
        let (a, b, c) = replies?;
        Ok([a, b, c])
    });
    let mut issues = issues.into_iter();
    let first = issues.next().expect("a first command");
    let lead = Lead {
        first,
        rest: issues.collect(),
        released: released.clone(),
    };
    (Box::new(lead), replies.boxed())
}

/// The diagnostics in a pull's three replies, in order, with 0-based
/// UTF-16 positions; why the pull is dropped if one isn't a successful
/// tsserver response.
pub(crate) fn diagnostics(
    replies: [Value; 3],
    uri: &Url,
) -> Result<Vec<lsp_types::Diagnostic>, String> {
    let mut items = Vec::new();
    for (command, reply) in COMMANDS.into_iter().zip(replies) {
        let body = (reply["type"] == "response" && reply["success"] == true)
            .then(|| reply["body"].as_array())
            .flatten()
            .ok_or_else(|| format!("{command} answered {}", brief(&reply)))?;
        for entry in body {
            match diagnostic(entry) {
                Some(item) => items.push(item),
                None => log::debug!(
                    "{uri}: skipped {command}'s malformed diagnostic {}",
                    brief(entry)
                ),
            }
        }
    }
    Ok(items)
}

/// One tsserver diagnostic as an LSP one, or `None` if it's malformed.
fn diagnostic(entry: &Value) -> Option<lsp_types::Diagnostic> {
    let severity = match entry["category"].as_str()? {
        "error" => DiagnosticSeverity::ERROR,
        "warning" => DiagnosticSeverity::WARNING,
        "suggestion" => DiagnosticSeverity::HINT,
        "message" => DiagnosticSeverity::INFORMATION,
        _ => return None,
    };
    let code = match &entry["code"] {
        Value::Null => None,
        code => Some(NumberOrString::Number(
            code.as_i64().and_then(|n| i32::try_from(n).ok())?,
        )),
    };
    // Set as `true`, or any value but `false` (tsserver's type is `{}`).
    let flagged = |key: &str| !matches!(entry[key], Value::Null | Value::Bool(false));
    let tags = [
        (flagged("reportsUnnecessary")).then_some(DiagnosticTag::UNNECESSARY),
        (flagged("reportsDeprecated")).then_some(DiagnosticTag::DEPRECATED),
    ];
    let tags: Vec<_> = tags.into_iter().flatten().collect();
    Some(lsp_types::Diagnostic {
        range: Range::new(position(&entry["start"])?, position(&entry["end"])?),
        severity: Some(severity),
        code,
        source: Some("typescript".into()),
        message: entry["text"].as_str()?.into(),
        tags: (!tags.is_empty()).then_some(tags),
        ..Default::default()
    })
}

/// A 1-based `{ line, offset }` as a 0-based position.
fn position(location: &Value) -> Option<Position> {
    let one_based = |key: &str| {
        let n = location[key].as_u64()?.checked_sub(1)?;
        u32::try_from(n).ok()
    };
    Some(Position::new(one_based("line")?, one_based("offset")?))
}

/// `value` for a log line, cut short.
fn brief(value: &Value) -> String {
    let mut text = value.to_string();
    if text.len() > 200 {
        let mut end = 200;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push('…');
    }
    text
}
