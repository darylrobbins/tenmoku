// SPDX-License-Identifier: Apache-2.0

//! `#[kiln::status]` on a `fn(&App) -> T`, `T` a `String`, an
//! `Option<String>` or a `StatusItem`: the fn stays as it is, and an
//! `inventory` record of a status source calling it, which `kiln::app` or
//! the crate's `kiln::module!` contributes.

use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};
use syn::{ItemFn, ext::IdentExt as _};

use crate::{
    command::check_id,
    panel::{Values, site},
};

const ARGUMENTS: &str = "side, order and id";

/// The fn, then the source it makes; the fn and the errors, spanned on
/// the user's tokens, if it can't.
pub(crate) fn expand(args: TokenStream, item: TokenStream) -> TokenStream {
    let function = match syn::parse2::<ItemFn>(item.clone()) {
        Ok(function) => function,
        Err(_) => {
            let error = syn::Error::new(
                Span::call_site(),
                "#[kiln::status] goes on a fn: `fn count(cx: &App) -> String`",
            )
            .into_compile_error();
            return quote!(#item #error);
        }
    };
    match status(args, &function) {
        Ok(status) => quote!(#function #status),
        Err(error) => {
            let error = error.into_compile_error();
            quote!(#function #error)
        }
    }
}

fn status(args: TokenStream, function: &ItemFn) -> syn::Result<TokenStream> {
    let args = Values::parse(args, "#[kiln::status]", &["side", "order", "id"], ARGUMENTS)?;
    if function.sig.receiver().is_some() {
        return Err(syn::Error::new_spanned(
            &function.sig,
            "#[kiln::status] goes on a free fn: `fn count(cx: &App) -> String`",
        ));
    }
    if !function.sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &function.sig.generics,
            "a #[kiln::status] fn takes no generics: it's `fn(&App) -> String`",
        ));
    }
    let ident = &function.sig.ident;
    let span = ident.span();
    let id = match args.string("id")? {
        Some(id) => {
            check_id(&id.value()).map_err(|why| syn::Error::new(id.span(), why))?;
            quote!(#id)
        }
        None => {
            let name = format!(".{}", ident.unraw());
            quote!(::std::concat!(::std::env!("CARGO_CRATE_NAME"), #name))
        }
    };
    let side = args.variant("side", quote!(::kiln::shell::StatusSide), "Left");
    let order = args.expr("order", quote!(0));
    // Spanned on the fn's name, so a signature other than `fn(&App) -> T`,
    // or a `T` that isn't a status, is reported there.
    let call = quote_spanned! {span=>
        ::kiln::__private::IntoStatus::into_status(#ident(cx), #id, #side, #order)
    };
    let site = site(span);
    Ok(quote! {
        const _: () = {
            fn __kiln_status(
                cx: &::kiln::gpui::App,
            ) -> ::std::option::Option<::kiln::shell::StatusItem> {
                #call
            }

            ::kiln::__private::inventory::submit! {
                ::kiln::__private::StatusRecord {
                    id: #id,
                    side: #side,
                    order: #order,
                    item: __kiln_status,
                    #site
                }
            }
        };
    })
}
