// SPDX-License-Identifier: LicenseRef-Proprietary

//! `#[kiln::command]` on a fn: the command's type, its GPUI action (through
//! `kiln::__command!`, which writes it as `kiln::commands!` does), and an
//! `inventory` record of its ID, title, context, keys, menu and handler,
//! which `kiln::app` or the crate's `kiln::module!` adds to the app's
//! contributions. Its parts here serve the impl form too
//! (`view_commands`).

use heck::ToUpperCamelCase as _;
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use syn::{
    Attribute, Expr, ExprLit, Ident, ItemFn, Lit, LitBool, LitInt, LitStr, Meta, MetaNameValue,
    Token, ext::IdentExt as _, parse::Parser as _, punctuated::Punctuated,
};

/// The arguments: `key = "secondary-n"`, `menu = "File"`, `order = 10`,
/// `id = "workbench.save_all"`, `context = "editor"`.
#[derive(Default)]
pub(crate) struct Args {
    pub(crate) key: Option<LitStr>,
    pub(crate) menu: Option<LitStr>,
    pub(crate) order: Option<LitInt>,
    pub(crate) id: Option<LitStr>,
    pub(crate) context: Option<LitStr>,
    /// `palette = false` keeps the command out of the command palette.
    pub(crate) palette: Option<LitBool>,
}

const ARGUMENTS: &str = "key, menu, order, id, context and palette";

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
    if function.sig.receiver().is_some() {
        return Err(syn::Error::new(
            Span::call_site(),
            "#[kiln::command] makes a method a command only inside \
             `#[kiln::command(context = \"..\")] impl View { .. }`: put that on the impl, \
             and mark the method `#[command]`",
        ));
    }
    let args = parse_args(args, "#[kiln::command]")?;
    let span = function.sig.ident.span();
    let docs = doc_lines(&function.attrs);
    let title = title(&docs, span, "a #[kiln::command] fn")?;
    check_order(&args)?;
    if !function.sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &function.sig.generics,
            "a #[kiln::command] fn takes no generics: it's \
             `fn(&mut Workspace, &mut Cx<Workspace>)`",
        ));
    }

    let Names {
        ty,
        id,
        action_name,
    } = names(&function.sig.ident, &args, "fn")?;
    let (context, keys, menu, order, palette) = (
        option(&args.context),
        option(&args.key),
        option(&args.menu),
        order(&args),
        palette(&args),
    );
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
                    palette: #palette,
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

/// The command's title: the first doc line that isn't blank, else an
/// error spanned on `span`, `what` being what needs one.
pub(crate) fn title(docs: &[String], span: Span, what: &str) -> syn::Result<String> {
    match docs
        .iter()
        .map(|line| line.trim())
        .find(|line| !line.is_empty())
    {
        Some(title) => Ok(title.to_owned()),
        None => Err(syn::Error::new(
            span,
            format!(
                "{what} needs a doc comment: its first line is the command's title, as in \
                 `/// New Note`"
            ),
        )),
    }
}

/// An error if `order` is given without `menu`.
pub(crate) fn check_order(args: &Args) -> syn::Result<()> {
    match &args.order {
        Some(order) if args.menu.is_none() => Err(syn::Error::new(
            order.span(),
            "`order` places the command's menu item: give it a `menu` too",
        )),
        _ => Ok(()),
    }
}

/// `Some(value)` or `None`, as an expression.
pub(crate) fn option(value: &Option<LitStr>) -> TokenStream {
    match value {
        Some(value) => quote!(::std::option::Option::Some(#value)),
        None => quote!(::std::option::Option::None),
    }
}

/// `order`, `0` unless given.
pub(crate) fn order(args: &Args) -> TokenStream {
    match &args.order {
        Some(order) => quote!(#order),
        None => quote!(0),
    }
}

/// `palette`, `true` unless `palette = false`.
pub(crate) fn palette(args: &Args) -> TokenStream {
    match &args.palette {
        Some(palette) => quote!(#palette),
        None => quote!(true),
    }
}

/// What a command named after the fn or method `ident` is called.
pub(crate) struct Names {
    /// Its type: `ident` in PascalCase, spanned on `ident`.
    pub(crate) ty: Ident,
    /// Its ID, as a `&'static str` expression.
    pub(crate) id: TokenStream,
    /// GPUI's name for its action, whose namespace and type in snake case
    /// give the ID back.
    pub(crate) action_name: TokenStream,
}

/// The type, ID and action name of the command named after `ident`, a
/// `kind` (`fn` or `method`): its ID is `id = ".."`'s if given, else
/// `<crate>.<ident>`.
pub(crate) fn names(ident: &Ident, args: &Args, kind: &str) -> syn::Result<Names> {
    let name = ident.unraw();
    let span = ident.span();
    let type_name = name.to_string().to_upper_camel_case();
    let ty = format_ident!("{}", type_name, span = span);
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
                         {kind} or give the ID with `id = \"..\"`"
                    ),
                ));
            }
            (
                quote!(::std::concat!(::std::env!("CARGO_CRATE_NAME"), ".", #name)),
                quote!(::std::concat!(::std::env!("CARGO_CRATE_NAME"), "::", #type_name)),
            )
        }
    };
    Ok(Names {
        ty,
        id,
        action_name,
    })
}

/// The arguments, each a string but `order`, each once, an unknown one
/// spanned on its name; `attribute` is the attribute they're given to, as
/// errors name it.
pub(crate) fn parse_args(args: TokenStream, attribute: &str) -> syn::Result<Args> {
    let pairs = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(args)?;
    let mut parsed = Args::default();
    for pair in pairs {
        let path = pair.path().clone();
        let Some(key) = path.get_ident().map(|ident| ident.to_string()) else {
            return Err(syn::Error::new_spanned(
                &path,
                format!("unknown argument: {attribute} takes {ARGUMENTS}"),
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
            "palette" => {
                if parsed.palette.is_some() {
                    return Err(syn::Error::new_spanned(&path, "`palette` is given twice"));
                }
                parsed.palette = Some(match value {
                    Expr::Lit(ExprLit {
                        lit: Lit::Bool(palette),
                        ..
                    }) => palette,
                    other => {
                        return Err(syn::Error::new_spanned(
                            other,
                            "`palette` takes a bool: `palette = false`",
                        ));
                    }
                });
                continue;
            }
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
                    format!("unknown argument `{key}`: {attribute} takes {ARGUMENTS}"),
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

/// The doc lines among `attrs`, as written: `#[doc = ".."]` with a string.
pub(crate) fn doc_lines(attrs: &[Attribute]) -> Vec<String> {
    (attrs.iter())
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
pub(crate) fn check_id(id: &str) -> Result<Vec<&str>, String> {
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
