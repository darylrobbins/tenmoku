// SPDX-License-Identifier: LicenseRef-Proprietary

//! Kiln's attribute macros. Use them through the kiln crate, which
//! documents each: `#[kiln::command]`, on a fn or a view's impl, is
//! [`command`] here, `#[kiln::panel]` [`panel`] and `#[kiln::status]`
//! [`status`]. Each expands to
//! plain data and fn pointers that name kiln's items only, through
//! `::kiln`, so an app needs no other dependency for them.

mod command;
mod panel;
mod status;
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

/// Makes a struct that implements `kiln::View` a dock panel: see
/// `kiln::panel`, which documents it.
#[proc_macro_attribute]
pub fn panel(args: TokenStream, item: TokenStream) -> TokenStream {
    panel::expand(args.into(), item.into()).into()
}

/// Makes a fn a status bar source: see `kiln::status`, which documents it.
#[proc_macro_attribute]
pub fn status(args: TokenStream, item: TokenStream) -> TokenStream {
    status::expand(args.into(), item.into()).into()
}
