// SPDX-License-Identifier: Apache-2.0

//! `#[kiln::model]` and `#[kiln::model(persist)]` on a struct or enum: the
//! type stays as it is, and gets an `inventory` record that `kiln::app` or
//! the crate's `kiln::module!` contributes, which makes the model from its
//! `Default` at start, or, for `persist`, loads it from (and keeps it in)
//! `<state>/<file>.json`.

use heck::ToSnakeCase as _;
use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};
use syn::{
    Expr, ExprLit, Item, Lit, LitStr, Meta, MetaNameValue, Token, parse::Parser as _,
    punctuated::Punctuated,
};

use crate::panel::site;

const ARGUMENTS: &str = "`persist`, and with it `file = \"..\"`";

/// The type, then what registers its model; the type and the errors,
/// spanned on the user's tokens, if it can't.
pub(crate) fn expand(args: TokenStream, item: TokenStream) -> TokenStream {
    let (ident, generics) = match syn::parse2::<Item>(item.clone()) {
        Ok(Item::Struct(item)) => (item.ident, item.generics),
        Ok(Item::Enum(item)) => (item.ident, item.generics),
        _ => {
            let error = syn::Error::new(
                Span::call_site(),
                "#[kiln::model] goes on a struct or an enum",
            )
            .into_compile_error();
            return quote!(#item #error);
        }
    };
    match model(args, &ident, &generics) {
        Ok(expanded) => quote!(#item #expanded),
        Err(error) => {
            let error = error.into_compile_error();
            quote!(#item #error)
        }
    }
}

fn model(
    args: TokenStream,
    ident: &syn::Ident,
    generics: &syn::Generics,
) -> syn::Result<TokenStream> {
    let (persist, file) = arguments(args)?;
    if !generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            generics,
            "a #[kiln::model] type takes no generics: kiln registers one model of it",
        ));
    }
    let span = ident.span();
    let site = site(span);
    let name = ident.to_string();
    let add = if persist {
        let file = match file {
            Some(file) => {
                check_file(&file.value()).map_err(|why| syn::Error::new(file.span(), why))?;
                quote!(#file)
            }
            None => {
                let file = ident.to_string().to_snake_case();
                quote!(#file)
            }
        };
        // Spanned on the type's name, so a type without the bounds is
        // reported there.
        let persist = quote_spanned!(span=> ::kiln::model::persist::persist::<#ident>);
        quote! {
            {
                fn add(cx: &mut ::kiln::gpui::App) {
                    #persist(cx, #file);
                }
                add
            }
        }
    } else {
        quote_spanned!(span=> ::kiln::model::register::<#ident>)
    };

    Ok(quote! {
        const _: () = {
            ::kiln::__private::inventory::submit! {
                ::kiln::__private::ModelRecord {
                    name: #name,
                    add: #add,
                    #site
                }
            }
        };
    })
}

/// Whether `persist` was given, and the `file`, if any.
fn arguments(args: TokenStream) -> syn::Result<(bool, Option<LitStr>)> {
    let metas = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(args)?;
    let mut persist = None;
    let mut file: Option<LitStr> = None;
    for meta in metas {
        let path = meta.path().clone();
        let Some(name) = path.get_ident().cloned() else {
            return Err(syn::Error::new_spanned(
                &path,
                format!("unknown argument: #[kiln::model] takes {ARGUMENTS}"),
            ));
        };
        match (name.to_string().as_str(), meta) {
            ("persist", Meta::Path(_)) => {
                if persist.replace(name.clone()).is_some() {
                    return Err(syn::Error::new_spanned(name, "`persist` is given twice"));
                }
            }
            ("persist", _) => {
                return Err(syn::Error::new_spanned(
                    name,
                    "`persist` takes no value: `#[kiln::model(persist)]`",
                ));
            }
            ("file", Meta::NameValue(MetaNameValue { value, .. })) => {
                let Expr::Lit(ExprLit {
                    lit: Lit::Str(text),
                    ..
                }) = value
                else {
                    return Err(syn::Error::new_spanned(
                        name,
                        "`file` is a string literal: `file = \"recent\"`",
                    ));
                };
                if file.replace(text).is_some() {
                    return Err(syn::Error::new_spanned(name, "`file` is given twice"));
                }
            }
            ("file", _) => {
                return Err(syn::Error::new_spanned(
                    name,
                    "`file` takes a value: `file = \"..\"`",
                ));
            }
            _ => {
                return Err(syn::Error::new_spanned(
                    &name,
                    format!("unknown argument `{name}`: #[kiln::model] takes {ARGUMENTS}"),
                ));
            }
        }
    }
    if let (None, Some(_)) = (&persist, &file) {
        return Err(syn::Error::new(
            file.expect("given").span(),
            "`file` names the persisted model's file: it goes with `persist`",
        ));
    }
    Ok((persist.is_some(), file))
}

/// Why `file` isn't the bare name of a file in the state directory, if it
/// isn't.
fn check_file(file: &str) -> Result<(), String> {
    if file.is_empty() {
        return Err("a file is a name in the state directory: `file = \"recent\"`".into());
    }
    match file
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')))
    {
        Some(c) => Err(format!(
            "`{file}` isn't a bare file name: it has `{c}`, and a name is letters, \
             digits, `_`, `-` and `.`; it gets `.json` added"
        )),
        None if file.starts_with('.') => Err("a file's name doesn't start with `.`".into()),
        None => Ok(()),
    }
}
