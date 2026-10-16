// SPDX-License-Identifier: LicenseRef-Proprietary

//! `#[kiln::settings(section = "..")]` on a struct of named fields: the
//! struct gets serde's `Serialize` and `Deserialize` (every key optional),
//! schemars' `JsonSchema`, `PartialEq`, a `Default` from its fields'
//! `#[setting(default = ..)]`s, and `kiln::SettingsSection`; and an
//! `inventory` record of its section, which `kiln::app` or the crate's
//! `kiln::module!` contributes, so that it reads its table of the app's
//! `settings.toml`.

use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};
use syn::{
    Attribute, Expr, ExprLit, Fields, ItemStruct, Lit, Meta, MetaNameValue, Token,
    parse::Parser as _, punctuated::Punctuated, spanned::Spanned as _,
};

use crate::panel::{Values, site};

const ARGUMENTS: &str = "section";

/// The struct, made settings; the struct and the errors, spanned on the
/// user's tokens, if it can't be.
pub(crate) fn expand(args: TokenStream, item: TokenStream) -> TokenStream {
    let mut item = match syn::parse2::<ItemStruct>(item.clone()) {
        Ok(item) => item,
        Err(_) => {
            let error = syn::Error::new(
                Span::call_site(),
                "#[kiln::settings] goes on a struct of named fields",
            )
            .into_compile_error();
            return quote!(#item #error);
        }
    };
    let original = item.clone();
    match settings(args, &mut item) {
        Ok(expanded) => expanded,
        Err(error) => {
            // Without its `#[setting]`s, which no derive would take.
            let mut original = original;
            strip_settings(&mut original);
            let error = error.into_compile_error();
            quote!(#original #error)
        }
    }
}

fn settings(args: TokenStream, item: &mut ItemStruct) -> syn::Result<TokenStream> {
    let args = Values::parse(args, "#[kiln::settings]", &["section"], ARGUMENTS)?;
    let Some(section) = args.string("section")? else {
        return Err(syn::Error::new_spanned(
            &item.ident,
            "#[kiln::settings] needs its table of settings.toml: `section = \"..\"`",
        ));
    };
    check_section(&section.value()).map_err(|why| syn::Error::new(section.span(), why))?;
    if !item.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &item.generics,
            "a #[kiln::settings] struct takes no generics: it's one table of settings.toml",
        ));
    }
    let Fields::Named(fields) = &mut item.fields else {
        return Err(syn::Error::new_spanned(
            &item.ident,
            "#[kiln::settings] goes on a struct of named fields: each one a key of its table",
        ));
    };
    let mut defaults = Vec::new();
    for field in &mut fields.named {
        let ident = field.ident.clone().expect("a named field");
        let mut default = None;
        let mut error = None;
        field.attrs.retain(|attr| {
            if !attr.path().is_ident("setting") {
                return true;
            }
            match setting_default(attr, default.is_some()) {
                Ok(value) => default = Some(value),
                Err(found) => {
                    error.get_or_insert(found);
                }
            }
            false
        });
        if let Some(error) = error {
            return Err(error);
        }
        let value = match default {
            // A string literal for a `String`, say.
            Some(Expr::Lit(ExprLit {
                lit: Lit::Str(text),
                ..
            })) => quote_spanned!(text.span()=> ::std::convert::Into::into(#text)),
            Some(value) => quote_spanned!(value.span()=> #value),
            None => quote!(::std::default::Default::default()),
        };
        // There exactly when the field is.
        let conditions = conditions(&field.attrs);
        defaults.push(quote!(#(#conditions)* #ident: #value));
    }

    let ident = &item.ident;
    let span = ident.span();
    let site = site(span);
    let add = quote_spanned!(span=> ::kiln::__private::add_section::<#ident>);
    let attrs = std::mem::take(&mut item.attrs);
    Ok(quote! {
        #[derive(
            ::kiln::serde::Serialize,
            ::kiln::serde::Deserialize,
            ::kiln::schemars::JsonSchema,
            ::std::cmp::PartialEq
        )]
        #[serde(crate = "::kiln::serde", default)]
        #[schemars(crate = "::kiln::schemars")]
        #(#attrs)*
        #item

        const _: () = {
            impl ::std::default::Default for #ident {
                fn default() -> Self {
                    Self { #(#defaults,)* }
                }
            }

            impl ::kiln::SettingsSection for #ident {
                const SECTION: &'static str = #section;
            }

            ::kiln::__private::inventory::submit! {
                ::kiln::__private::SettingsRecord {
                    section: #section,
                    add: #add,
                    #site
                }
            }
        };
    })
}

/// The value of a `#[setting(default = ..)]`, the only argument it takes;
/// `given` if the field had one already.
fn setting_default(attr: &syn::Attribute, given: bool) -> syn::Result<Expr> {
    let Meta::List(list) = &attr.meta else {
        return Err(syn::Error::new_spanned(
            attr,
            "#[setting] takes the key's default: `#[setting(default = ..)]`",
        ));
    };
    let metas = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(list.tokens.clone())?;
    let mut value = None;
    for meta in metas {
        match meta {
            Meta::NameValue(MetaNameValue {
                path, value: given, ..
            }) if path.is_ident("default") => {
                if value.is_some() {
                    return Err(syn::Error::new_spanned(path, "`default` is given twice"));
                }
                value = Some(given);
            }
            other => {
                return Err(syn::Error::new_spanned(
                    other.path(),
                    "unknown argument: #[setting] takes `default = ..`",
                ));
            }
        }
    }
    match value {
        Some(_) if given => Err(syn::Error::new_spanned(
            attr,
            "the field has a #[setting(default = ..)] already",
        )),
        Some(value) => Ok(value),
        None => Err(syn::Error::new_spanned(
            attr,
            "#[setting] takes the key's default: `#[setting(default = ..)]`",
        )),
    }
}

/// A field's conditions, as attributes for its initializer in `Default`:
/// each `#[cfg(..)]`, and each `#[cfg_attr(..)]` that holds one, kept to
/// its `cfg`s, as the initializer may have no other attribute.
fn conditions(attrs: &[Attribute]) -> Vec<TokenStream> {
    (attrs.iter())
        .filter_map(|attr| condition(&attr.meta))
        .map(|meta| quote!(#[#meta]))
        .collect()
}

/// `meta` if it's a `cfg(..)`; a `cfg_attr(..)` kept to the conditions it
/// holds, if it holds any; else `None`.
fn condition(meta: &Meta) -> Option<TokenStream> {
    if meta.path().is_ident("cfg") {
        return Some(quote!(#meta));
    }
    let Meta::List(list) = meta else {
        return None;
    };
    if !list.path.is_ident("cfg_attr") {
        return None;
    }
    let metas = Punctuated::<Meta, Token![,]>::parse_terminated
        .parse2(list.tokens.clone())
        .ok()?;
    let mut metas = metas.into_iter();
    let predicate = metas.next()?;
    let held: Vec<TokenStream> = metas.filter_map(|meta| condition(&meta)).collect();
    (!held.is_empty()).then(|| quote!(cfg_attr(#predicate, #(#held),*)))
}

/// Removes the fields' `#[setting]`s.
fn strip_settings(item: &mut ItemStruct) {
    for field in &mut item.fields {
        field.attrs.retain(|attr| !attr.path().is_ident("setting"));
    }
}

/// Why `section` isn't a bare TOML key, the table's name, if it isn't.
fn check_section(section: &str) -> Result<(), String> {
    if section.is_empty() {
        return Err("a section is a table's name: `section = \"notes\"`".into());
    }
    match section
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || *c == '_' || *c == '-'))
    {
        Some(c) => Err(format!(
            "`{section}` isn't a table's bare name: it has `{c}`, and a section's name \
             is letters, digits, `_` and `-`"
        )),
        None => Ok(()),
    }
}
