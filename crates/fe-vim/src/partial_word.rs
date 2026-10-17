//! `g*` and `g#` (`:help gstar`), a feature module; see [`crate::feature`].
//!
//! `*` and `#` without the `\<` `\>` word boundaries: they search for the
//! keyword (or other run) under or after the caret wherever its text
//! appears. Everything else is as for `*` and `#`; `cmdline::resolve` turns
//! the motions into [`Motion::SearchWord`](crate::Motion::SearchWord) with
//! [`unbounded`].

use crate::{Pending, Step, VimKey, args::Motion, feature::Feature};

pub(crate) const FEATURE: Feature = Feature { g, ..Feature::NONE };

/// `*` and `#` after `g`.
fn g(pending: &Pending, key: VimKey) -> Option<Step> {
    let motion = match key {
        VimKey::Char('*') => Motion::SearchPartialWordForward,
        VimKey::Char('#') => Motion::SearchPartialWordBackward,
        _ => return None,
    };
    Some(pending.clone().motion(motion))
}

/// `pattern` from `search::word_at` without its `\<` prefix and `\>`
/// suffix, both present exactly when the run is a keyword run.
pub(crate) fn unbounded(pattern: String) -> String {
    match pattern.strip_prefix("\\<") {
        Some(rest) => rest.strip_suffix("\\>").unwrap_or(rest).to_string(),
        None => pattern,
    }
}
