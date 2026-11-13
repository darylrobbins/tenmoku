// SPDX-License-Identifier: Apache-2.0

//! Layer 1 of the catalogue checks (`docs/decisions/xte5e7hqsw-i18n-design.md`
//! § 8): no string converts into a [`Text`], so `Picker::new(d, "files", …)`
//! and `Button::new(id, "Save")`, which take `impl Into<Text>`, don't compile.
//! A label reaches them only through `text!` or `Text::verbatim`.
//!
//! This binary is the check: it stops compiling if a `From` impl for a string
//! type comes back. `kiln-ui`'s `Button::new` and `kiln-shell`'s
//! `Picker::new` carry `compile_fail` doc tests on the constructors
//! themselves.

use std::borrow::Cow;

use gpui_shared_string::SharedString;
use kiln_l10n::{Text, text};

/// `assert_not_into_text!(T)` compiles only when `T: Into<Text>` doesn't
/// hold: if it did, both impls of `AmbiguousIfInto` would apply to `T` and
/// naming `some_item` through an inferred parameter would be ambiguous.
macro_rules! assert_not_into_text {
    ($($ty:ty),+ $(,)?) => {{
        trait AmbiguousIfInto<A> {
            fn some_item() {}
        }
        impl<T: ?Sized> AmbiguousIfInto<()> for T {}
        struct Converts;
        impl<T: ?Sized + Into<Text>> AmbiguousIfInto<Converts> for T {}
        $(let _ = <$ty as AmbiguousIfInto<_>>::some_item;)+
    }};
}

/// What `Picker::new` and `Button::new` take for their label.
fn label(text: impl Into<Text>) -> Text {
    text.into()
}

#[test]
fn no_string_converts_into_text() {
    assert_not_into_text!(
        &'static str,
        &String,
        String,
        SharedString,
        &SharedString,
        Cow<'static, str>,
        Box<str>,
    );
}

#[test]
fn a_label_is_keyed_or_explicitly_verbatim() {
    assert_eq!(label(text!("probe-key")).key(), Some("probe-key"));
    assert_eq!(label(Text::verbatim("Cargo.toml")).resolve(), "Cargo.toml");
    assert_eq!(label(Text::verbatim(String::from("a.rs"))).key(), None);
}
