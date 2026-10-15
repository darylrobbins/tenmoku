// SPDX-License-Identifier: LicenseRef-Proprietary

//! `#[kiln::command]` on a fn: the command's type, its GPUI action (through
//! `kiln::__command!`, which writes it as `kiln::commands!` does), and an
//! `inventory` record of its ID, title, context, keys, menu and handler,
//! which `kiln::app` or the crate's `kiln::module!` adds to the app's
//! contributions.

use heck::ToUpperCamelCase as _;
use proc_macro2::TokenStream;
use quote::{format_ident, quote, quote_spanned};
use syn::{
    Expr, ExprLit, ItemFn, Lit, LitInt, LitStr, Meta, MetaNameValue, Token, ext::IdentExt as _,
    parse::Parser as _, punctuated::Punctuated,
};

/// The arguments: `key = "secondary-n"`, `menu = "File"`, `order = 10`,
/// `id = "workbench.save_all"`, `context = "editor"`.
#[derive(Default)]
struct Args {
    key: Option<LitStr>,
    menu: Option<LitStr>,
    order: Option<LitInt>,
    id: Option<LitStr>,
    context: Option<LitStr>,
}

const ARGUMENTS: &str = "key, menu, order, id and context";

/// The fn, then the command it makes; the fn and the errors, spanned on
/// the user's tokens, if it can't.
pub(crate) fn expand(args: TokenStream, item: TokenStream) -> TokenStream {
    let function = match syn::parse2::<ItemFn>(item.clone()) {
        Ok(function) => function,
        Err(error) => {
            let error = error.into_compile_error();
            return quote!(#item #error);
        }
    };
    match command(args, &function) {
        Ok(command) => quote!(#function #command),
        Err(error) => {
            let error = error.into_compile_error();
            quote!(#function #error)
        }
    }
}

fn command(args: TokenStream, function: &ItemFn) -> syn::Result<TokenStream> {
    let args = parse_args(args)?;
    let name = function.sig.ident.unraw();
    let span = function.sig.ident.span();
    let docs = doc_lines(function);
    let Some(title) = docs
        .iter()
        .map(|line| line.trim())
        .find(|line| !line.is_empty())
    else {
        return Err(syn::Error::new(
            span,
            "a #[kiln::command] fn needs a doc comment: its first line is the command's \
             title, as in `/// New Note`",
        ));
    };
    if let Some(order) = &args.order
        && args.menu.is_none()
    {
        return Err(syn::Error::new(
            order.span(),
            "`order` places the command's menu item: give it a `menu` too",
        ));
    }
    if !function.sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &function.sig.generics,
            "a #[kiln::command] fn takes no generics: it's \
             `fn(&mut Workspace, &mut Cx<Workspace>)`",
        ));
    }

    let type_name = name.to_string().to_upper_camel_case();
    let ty = format_ident!("{}", type_name, span = span);
    // The ID, and GPUI's name for the action, whose namespace and type in
    // snake case give that ID back.
    let (id, action_name) = match &args.id {
        Some(id) => {
            let value = id.value();
            let segments = check_id(&value).map_err(|why| syn::Error::new(id.span(), why))?;
            let (last, namespace) = segments.split_last().expect("an ID has two segments");
            let action_type = last.to_upper_camel_case();
            if snake_case(&action_type) != *last {
                return Err(syn::Error::new(
                    id.span(),
                    format!(
                        "the ID's last segment `{last}` must be snake case that comes back \
                         from its type name, `{action_type}`, as `{}`",
                        snake_case(&action_type)
                    ),
                ));
            }
            let action_name = format!("{}::{action_type}", namespace.join("::"));
            (quote!(#id), quote!(#action_name))
        }
        None => {
            let name = name.to_string();
            let fits = check_segment(&name).is_ok() && snake_case(&type_name) == name;
            if !fits {
                return Err(syn::Error::new(
                    span,
                    format!(
                        "the command's ID would be `<crate>.{name}`, but `{name}` isn't a \
                         snake-case ID segment that comes back from `{type_name}`: rename the \
                         fn or give the ID with `id = \"..\"`"
                    ),
                ));
            }
            (
                quote!(::std::concat!(::std::env!("CARGO_CRATE_NAME"), ".", #name)),
                quote!(::std::concat!(::std::env!("CARGO_CRATE_NAME"), "::", #type_name)),
            )
        }
    };
    let option = |value: &Option<LitStr>| match value {
        Some(value) => quote!(::std::option::Option::Some(#value)),
        None => quote!(::std::option::Option::None),
    };
    let (context, keys, menu) = (option(&args.context), option(&args.key), option(&args.menu));
    let order = match &args.order {
        Some(order) => quote!(#order),
        None => quote!(0),
    };
    let vis = &function.vis;
    let ident = &function.sig.ident;
    // Spanned on the fn's name, so a signature other than the handler's
    // is reported there (E0061, E0308), not in this expansion.
    let call = quote_spanned!(span=> #ident(workspace, cx));
    Ok(quote! {
        ::kiln::__command!(#vis #ty, #action_name, [#(#docs),*]);

        const _: () = {
            fn __kiln_register(
                cx: &mut ::kiln::gpui::App,
                info: ::kiln::CommandInfo,
            ) {
                ::kiln::__private::register_command::<#ty>(cx, info, __kiln_run);
            }

            fn __kiln_run(
                workspace: &mut ::kiln::shell::Workspace,
                cx: &mut ::kiln::Cx<'_, '_, ::kiln::shell::Workspace>,
            ) {
                #call
            }

            ::kiln::__private::inventory::submit! {
                ::kiln::__private::CommandRecord {
                    id: #id,
                    title: #title,
                    context: #context,
                    keys: #keys,
                    menu: #menu,
                    order: #order,
                    file: ::std::file!(),
                    line: ::std::line!(),
                    krate: ::std::env!("CARGO_CRATE_NAME"),
                    bin: ::std::option_env!("CARGO_BIN_NAME"),
                    test_dir: ::std::option_env!("CARGO_TARGET_TMPDIR"),
                    register: __kiln_register,
                    run: __kiln_run,
                }
            }
        };
    })
}

/// The arguments, each a string but `order`, each once, an unknown one
/// spanned on its name.
fn parse_args(args: TokenStream) -> syn::Result<Args> {
    let pairs = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(args)?;
    let mut parsed = Args::default();
    for pair in pairs {
        let path = pair.path().clone();
        let Some(key) = path.get_ident().map(|ident| ident.to_string()) else {
            return Err(syn::Error::new_spanned(
                &path,
                format!("unknown argument: #[kiln::command] takes {ARGUMENTS}"),
            ));
        };
        let value = match pair {
            Meta::NameValue(MetaNameValue { value, .. }) => value,
            _ => {
                return Err(syn::Error::new_spanned(
                    &path,
                    format!("`{key}` takes a value: `{key} = ..`"),
                ));
            }
        };
        let slot = match key.as_str() {
            "key" => &mut parsed.key,
            "menu" => &mut parsed.menu,
            "id" => &mut parsed.id,
            "context" => &mut parsed.context,
            "order" => {
                if parsed.order.is_some() {
                    return Err(syn::Error::new_spanned(&path, "`order` is given twice"));
                }
                parsed.order = Some(match value {
                    Expr::Lit(ExprLit {
                        lit: Lit::Int(order),
                        ..
                    }) => order,
                    other => {
                        return Err(syn::Error::new_spanned(
                            other,
                            "`order` takes a number: `order = 10`",
                        ));
                    }
                });
                continue;
            }
            _ => {
                return Err(syn::Error::new_spanned(
                    &path,
                    format!("unknown argument `{key}`: #[kiln::command] takes {ARGUMENTS}"),
                ));
            }
        };
        if slot.is_some() {
            return Err(syn::Error::new_spanned(
                &path,
                format!("`{key}` is given twice"),
            ));
        }
        *slot = Some(match value {
            Expr::Lit(ExprLit {
                lit: Lit::Str(text),
                ..
            }) => text,
            other => {
                return Err(syn::Error::new_spanned(
                    other,
                    format!("`{key}` takes a string: `{key} = \"..\"`"),
                ));
            }
        });
    }
    Ok(parsed)
}

/// The fn's doc lines, as written: `#[doc = ".."]` with a string.
fn doc_lines(function: &ItemFn) -> Vec<String> {
    (function.attrs.iter())
        .filter(|attr| attr.path().is_ident("doc"))
        .filter_map(|attr| match &attr.meta {
            Meta::NameValue(MetaNameValue {
                value:
                    Expr::Lit(ExprLit {
                        lit: Lit::Str(line),
                        ..
                    }),
                ..
            }) => Some(line.value()),
            _ => None,
        })
        .collect()
}

/// `id`'s segments, if it's a command ID as kiln's `CommandId::parse`
/// takes it, else why not.
fn check_id(id: &str) -> Result<Vec<&str>, String> {
    let segments: Vec<&str> = id.split('.').collect();
    for segment in &segments {
        check_segment(segment).map_err(|why| format!("`{id}` isn't a command ID: {why}"))?;
    }
    if segments.len() < 2 {
        return Err(format!(
            "`{id}` isn't a command ID: it needs a namespace, as in `editor.move_left`"
        ));
    }
    Ok(segments)
}

fn check_segment(segment: &str) -> Result<(), &'static str> {
    let mut chars = segment.chars();
    match chars.next() {
        None => return Err("a segment is empty"),
        Some('a'..='z') => {}
        Some(_) => return Err("a segment must start with a-z"),
    }
    if !chars.all(|ch| matches!(ch, 'a'..='z' | '0'..='9' | '_')) {
        return Err("segments may only contain a-z, 0-9 and _");
    }
    Ok(())
}

/// A type name in snake case, as kiln-platform's `command_id` writes it
/// for the command's ID: `NewNote` is `new_note`.
fn snake_case(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::new();
    for (ix, &ch) in chars.iter().enumerate() {
        if ch.is_uppercase() {
            let after_lower =
                ix > 0 && (chars[ix - 1].is_lowercase() || chars[ix - 1].is_ascii_digit());
            let before_lower = ix > 0
                && chars[ix - 1].is_uppercase()
                && chars.get(ix + 1).is_some_and(|next| next.is_lowercase());
            if after_lower || before_lower {
                out.push('_');
            }
            out.extend(ch.to_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}
