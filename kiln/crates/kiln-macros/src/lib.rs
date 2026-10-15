// SPDX-License-Identifier: LicenseRef-Proprietary

//! Kiln's attribute macros. Use them through the kiln crate, which
//! documents each: `#[kiln::command]` is [`command`] here. Each expands to
//! plain data and fn pointers that name kiln's items only, through
//! `::kiln`, so an app needs no other dependency for them.

mod command;

use proc_macro::TokenStream;

/// Makes a fn a command: see `kiln::command`, which documents it.
#[proc_macro_attribute]
pub fn command(args: TokenStream, item: TokenStream) -> TokenStream {
    command::expand(args.into(), item.into()).into()
}
