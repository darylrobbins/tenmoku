// SPDX-License-Identifier: Apache-2.0

//! `#[kiln::command(context = "..")]` on an `impl`: a view's own commands,
//! its methods marked `#[command(..)]`. Each gets a command type and an
//! `inventory` record as the fn form's, with no handler of its own; the
//! view's `kiln::ViewCommands` impl attaches the key context and a
//! handler per method to the view's element.

use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};
use syn::{Attribute, ImplItem, ImplItemFn, ItemImpl, LitStr, Meta};

use crate::command::{
    Args, Names, check_order, doc_lines, names, option, order, palette, parse_args, title,
};

/// The impl, its `#[command]` marks taken off, then the commands it
/// declares; the impl and the errors, spanned on the user's tokens, if it
/// can't.
pub(crate) fn expand(args: TokenStream, mut item: ItemImpl) -> TokenStream {
    let marked = take_marks(&mut item);
    match commands(args, &item, marked) {
        Ok(commands) => quote!(#item #commands),
        Err(error) => {
            let error = error.into_compile_error();
            quote!(#item #error)
        }
    }
}

/// A method marked `#[command(..)]`: its arguments, and the method as it
/// stays in the impl.
struct Marked {
    args: TokenStream,
    method: ImplItemFn,
}

/// The methods marked `#[command]` (or `#[kiln::command]`), each mark
/// taken off the impl's method.
fn take_marks(item: &mut ItemImpl) -> Vec<Result<Marked, syn::Error>> {
    let mut marked = Vec::new();
    for impl_item in &mut item.items {
        let ImplItem::Fn(method) = impl_item else {
            continue;
        };
        let Some(ix) = method.attrs.iter().position(is_mark) else {
            continue;
        };
        let mark = method.attrs.remove(ix);
        if let Some(again) = method.attrs.iter().find(|attr| is_mark(attr)) {
            marked.push(Err(syn::Error::new_spanned(
                again,
                "a method is marked #[command] once",
            )));
            continue;
        }
        let args = match &mark.meta {
            Meta::Path(_) => Ok(TokenStream::new()),
            Meta::List(list) => Ok(list.tokens.clone()),
            Meta::NameValue(_) => Err(syn::Error::new_spanned(
                &mark,
                "#[command] takes its arguments in parentheses: `#[command(key = \"f2\")]`",
            )),
        };
        marked.push(args.map(|args| Marked {
            args,
            method: method.clone(),
        }));
    }
    marked
}

fn is_mark(attr: &Attribute) -> bool {
    let path = attr.path();
    path.is_ident("command")
        || (path.segments.len() == 2
            && path.segments[0].ident == "kiln"
            && path.segments[1].ident == "command")
}

fn commands(
    args: TokenStream,
    item: &ItemImpl,
    marked: Vec<Result<Marked, syn::Error>>,
) -> syn::Result<TokenStream> {
    if let Some((_, path, _)) = &item.trait_ {
        return Err(syn::Error::new_spanned(
            path,
            "#[kiln::command] goes on a view's own impl, not a trait's: \
             `#[kiln::command(context = \"..\")] impl View { .. }`",
        ));
    }
    let context = impl_context(args)?;
    let self_ty = &item.self_ty;
    let (impl_generics, _, where_clause) = item.generics.split_for_impl();

    let mut declared = Vec::new();
    let mut handlers = Vec::new();
    for marked in marked {
        let Marked { args, method } = marked?;
        let args = parse_args(args, "#[command]")?;
        let (command, handler) = command(&args, &method, &context)?;
        declared.push(command);
        handlers.push(handler);
    }

    Ok(quote! {
        #(#declared)*

        impl #impl_generics ::kiln::ViewCommands for #self_ty #where_clause {
            const CONTEXT: &'static str = #context;

            fn attach<E: ::kiln::gpui::InteractiveElement>(
                element: E,
                cx: &::kiln::gpui::Context<Self>,
            ) -> E {
                let element = ::kiln::gpui::InteractiveElement::key_context(element, #context);
                #(
                    let element = ::kiln::gpui::InteractiveElement::on_action(element, #handlers);
                )*
                element
            }
        }
    })
}

/// The impl's `context`, which it must give, and nothing else.
fn impl_context(args: TokenStream) -> syn::Result<LitStr> {
    let Args {
        key,
        menu,
        order,
        id,
        context,
        palette,
    } = parse_args(args, "#[kiln::command] on an impl")?;
    let method_only = (key.map(|key| ("key", key.span())))
        .or(menu.map(|menu| ("menu", menu.span())))
        .or(order.map(|order| ("order", order.span())))
        .or(id.map(|id| ("id", id.span())))
        .or(palette.map(|palette| ("palette", palette.span())));
    if let Some((name, span)) = method_only {
        return Err(syn::Error::new(
            span,
            format!(
                "`{name}` is a command's: give it on the method's `#[command({name} = ..)]`; \
                 an impl takes only `context`"
            ),
        ));
    }
    context.ok_or_else(|| {
        syn::Error::new(
            Span::call_site(),
            "#[kiln::command] on an impl needs the view's key context: \
             `#[kiln::command(context = \"note\")]`",
        )
    })
}

/// The command the method declares, and the view's handler of it.
fn command(
    args: &Args,
    method: &ImplItemFn,
    impl_context: &LitStr,
) -> syn::Result<(TokenStream, TokenStream)> {
    let span = method.sig.ident.span();
    let docs = doc_lines(&method.attrs);
    let title = title(&docs, span, "a #[command] method")?;
    check_order(args)?;
    if !method.sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &method.sig.generics,
            "a #[command] method takes no generics: it's \
             `fn(&mut self, &mut Cx<Self>)`",
        ));
    }
    let Names {
        ty,
        id,
        action_name,
    } = names(&method.sig.ident, args, "method")?;
    let context = args.context.as_ref().unwrap_or(impl_context);
    let context = quote!(::std::option::Option::Some(#context));
    let (keys, menu, order, palette) = (
        option(&args.key),
        option(&args.menu),
        order(args),
        palette(args),
    );
    let vis = &method.vis;
    let ident = &method.sig.ident;
    // Spanned on the method's name, so a signature other than
    // `fn(&mut self, &mut Cx<Self>)` is reported there (E0061, E0308);
    // `file!` and `line!` too, so its menu item goes by where the method
    // is, not the impl.
    let run = quote_spanned! {span=>
        |view: &mut Self, cx: &mut ::kiln::Cx<'_, '_, Self>| Self::#ident(view, cx)
    };
    let handler = quote!(::kiln::__private::view_handler::<#ty, Self>(cx, #run));
    let file = quote_spanned!(span=> ::std::file!());
    let line = quote_spanned!(span=> ::std::line!());
    let command = quote! {
        ::kiln::__command!(#vis #ty, #action_name, [#(#docs),*]);

        ::kiln::__private::inventory::submit! {
            ::kiln::__private::CommandRecord {
                id: #id,
                title: #title,
                context: #context,
                keys: #keys,
                menu: #menu,
                order: #order,
                palette: #palette,
                file: #file,
                line: #line,
                krate: ::std::env!("CARGO_CRATE_NAME"),
                bin: ::std::option_env!("CARGO_BIN_NAME"),
                test_dir: ::std::option_env!("CARGO_TARGET_TMPDIR"),
                register: ::kiln::register_command::<#ty>,
                run: ::kiln::__private::run_in_view,
            }
        }
    };
    Ok((command, handler))
}
