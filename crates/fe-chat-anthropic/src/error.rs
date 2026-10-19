//! A failed response as a [`ProviderError`] (the record's § 2 and § 14).

use std::time::Duration;

use fe_chat::{ErrorKind, ProviderError};
use http::HeaderMap;
use serde_json::Value;

/// The error a non-2xx response stands for: its kind by status (by the
/// body's `error.type` for a status the record doesn't name), the API's
/// `error.message`, and `retry-after` when the server sent one in seconds.
pub fn error_from_response(status: u16, headers: &HeaderMap, body: &[u8]) -> ProviderError {
    let json: Option<Value> = serde_json::from_slice(body).ok();
    let error = json.as_ref().map(|json| &json["error"]);
    let error_type = error.and_then(|error| error["type"].as_str());
    let message = error
        .and_then(|error| error["message"].as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            let text = String::from_utf8_lossy(body);
            let text = text.trim();
            if text.is_empty() {
                format!("HTTP {status}")
            } else {
                format!("HTTP {status}: {}", truncate(text, 200))
            }
        });
    let retry_after = headers
        .get(http::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|seconds| *seconds >= 0.0)
        // Too long a wait to represent is as good as forever.
        .map(|seconds| Duration::try_from_secs_f64(seconds).unwrap_or(Duration::MAX));
    ProviderError {
        kind: kind(status, error_type),
        message,
        retry_after,
    }
}

fn kind(status: u16, error_type: Option<&str>) -> ErrorKind {
    match status {
        400 => ErrorKind::InvalidRequest,
        401 => ErrorKind::Auth,
        402 => ErrorKind::Billing,
        403 => ErrorKind::Permission,
        404 => ErrorKind::NotFound,
        413 => ErrorKind::TooLarge,
        429 => ErrorKind::RateLimited,
        500 => ErrorKind::Server,
        529 => ErrorKind::Overloaded,
        _ => match error_type {
            Some("invalid_request_error") => ErrorKind::InvalidRequest,
            Some("authentication_error") => ErrorKind::Auth,
            Some("billing_error") => ErrorKind::Billing,
            Some("permission_error") => ErrorKind::Permission,
            Some("not_found_error") => ErrorKind::NotFound,
            Some("request_too_large") => ErrorKind::TooLarge,
            Some("rate_limit_error") => ErrorKind::RateLimited,
            Some("api_error") => ErrorKind::Server,
            Some("overloaded_error") => ErrorKind::Overloaded,
            _ if status >= 500 => ErrorKind::Server,
            _ => ErrorKind::InvalidRequest,
        },
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_owned(),
    }
}
