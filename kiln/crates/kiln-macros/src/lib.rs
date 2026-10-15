// SPDX-License-Identifier: LicenseRef-Proprietary

//! Kiln's attribute macros. Use them through the kiln crate, which
//! documents each: `#[kiln::command]`, on a fn or a view's impl, is
//! [`command`] here. Each expands to
//! plain data and fn pointers that name kiln's items only, through
//! `::kiln`, so an app needs no other dependency for them.

mod command;
mod view_commands;

use proc_macro::TokenStream;

/// Makes a fn a command, or an impl's marked methods its view's commands:
/// see `kiln::command`, which documents it.
#[proc_macro_attribute]
pub fn command(args: TokenStream, item: TokenStream) -> TokenStream {
    match syn::parse::<syn::ItemImpl>(item.clone()) {
        Ok(item) => view_commands::expand(args.into(), item).into(),
        Err(_) => command::expand(args.into(), item.into()).into(),
    }
}
