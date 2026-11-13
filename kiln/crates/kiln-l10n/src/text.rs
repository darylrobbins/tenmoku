// SPDX-License-Identifier: Apache-2.0

//! [`Text`]: a label stored now and resolved when it is shown.

use std::borrow::Cow;
use std::fmt;

use gpui_shared_string::SharedString;

use crate::Args;

/// A label that is stored and shown later, such as a picker placeholder or a
/// button: either a catalogue key with its arguments, resolved in the locale
/// active when it is drawn, or verbatim text from a user or a plugin.
///
/// Make one with [`text!`](crate::text) or [`Text::verbatim`]. There is no
/// conversion from a string, so a label reaches a widget only through the
/// catalogue or an explicit verbatim.
#[derive(Clone, Debug, PartialEq)]
pub enum Text {
    Key(Cow<'static, str>, Args),
    Verbatim(SharedString),
}

impl Text {
    /// A key and its arguments; what [`text!`](crate::text) expands to.
    pub fn keyed(key: impl Into<Cow<'static, str>>, args: Args) -> Self {
        Text::Key(key.into(), args)
    }

    /// Text shown as it is, never looked up: a user's or a plugin's own
    /// words, a file name.
    pub fn verbatim(text: impl Into<SharedString>) -> Self {
        Text::Verbatim(text.into())
    }

    /// The text to show now, in the active locale.
    pub fn resolve(&self) -> SharedString {
        match self {
            Text::Key(key, args) if args.is_empty() => crate::message(key),
            Text::Key(key, args) => crate::format(key, args),
            Text::Verbatim(text) => text.clone(),
        }
    }

    /// The key, for a keyed text.
    pub fn key(&self) -> Option<&str> {
        match self {
            Text::Key(key, _) => Some(key),
            Text::Verbatim(_) => None,
        }
    }
}

impl Default for Text {
    fn default() -> Self {
        Text::Verbatim(SharedString::default())
    }
}

/// Resolves the text in the active locale.
impl fmt::Display for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.resolve())
    }
}
