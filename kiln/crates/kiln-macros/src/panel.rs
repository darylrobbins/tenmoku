// SPDX-License-Identifier: Apache-2.0

//! `#[kiln::panel]` on a struct that implements `kiln::View`: the struct
//! gets a hidden `__kiln: kiln::__private::ViewState` field, and `Render`
//! (through `View`, with its `ViewCommands` attached), `Focusable` and
//! `DockItem`; and an `inventory` record of the panel, which `kiln::app`
//! or the crate's `kiln::module!` contributes with its toggle command.

use heck::ToSnakeCase as _;
use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};
use syn::{
    Expr, ExprLit, ExprPath, Field, Fields, FieldsNamed, Ident, ItemStruct, Lit, LitStr, Meta,
    MetaNameValue, Path, Token, parse::Parser as _, parse_quote, punctuated::Punctuated,
};

use crate::command::{check_id, doc_lines};

/// The struct, its field added, then what makes it a panel; the struct
/// as it was and the errors, spanned on the user's tokens, if it can't.
pub(crate) fn expand(args: TokenStream, item: TokenStream) -> TokenStream {
    let mut item = match syn::parse2::<ItemStruct>(item.clone()) {
        Ok(item) => item,
        Err(_) => {
            let error = syn::Error::new(
                Span::call_site(),
                "#[kiln::panel] goes on a struct that implements kiln::View",
            )
            .into_compile_error();
            return quote!(#item #error);
        }
    };
    let original = item.clone();
    match panel(args, &mut item) {
        Ok(panel) => quote!(#item #panel),
        Err(error) => {
            let error = error.into_compile_error();
            quote!(#original #error)
        }
    }
}

/// An attribute's arguments, `name = value` each, by name in the order
/// given.
pub(crate) struct Values {
    pairs: Vec<(Ident, Expr)>,
    attribute: &'static str,
    known: &'static str,
}

impl Values {
    /// The arguments of `attribute`, each one of `names` (listed in `known`
    /// for errors) and given once.
    pub(crate) fn parse(
        args: TokenStream,
        attribute: &'static str,
        names: &[&str],
        known: &'static str,
    ) -> syn::Result<Self> {
        let metas = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(args)?;
        let mut pairs: Vec<(Ident, Expr)> = Vec::new();
        for meta in metas {
            let path = meta.path().clone();
            let Some(name) = path.get_ident().cloned() else {
                return Err(syn::Error::new_spanned(
                    &path,
                    format!("unknown argument: {attribute} takes {known}"),
                ));
            };
            if !names.contains(&name.to_string().as_str()) {
                return Err(syn::Error::new_spanned(
                    &name,
                    format!("unknown argument `{name}`: {attribute} takes {known}"),
                ));
            }
            let Meta::NameValue(MetaNameValue { value, .. }) = meta else {
                return Err(syn::Error::new_spanned(
                    &name,
                    format!("`{name}` takes a value: `{name} = ..`"),
                ));
            };
            if pairs.iter().any(|(given, _)| *given == name) {
                return Err(syn::Error::new_spanned(
                    &name,
                    format!("`{name}` is given twice"),
                ));
            }
            pairs.push((name, value));
        }
        Ok(Self {
            pairs,
            attribute,
            known,
        })
    }

    fn get(&self, name: &str) -> Option<&Expr> {
        let found = self.pairs.iter().find(|(given, _)| given == name);
        found.map(|(_, value)| value)
    }

    /// `name`'s string, if given.
    pub(crate) fn string(&self, name: &str) -> syn::Result<Option<LitStr>> {
        match self.get(name) {
            None => Ok(None),
            Some(Expr::Lit(ExprLit {
                lit: Lit::Str(text),
                ..
            })) => Ok(Some(text.clone())),
            Some(other) => Err(syn::Error::new_spanned(
                other,
                format!("`{name}` takes a string: `{name} = \"..\"`"),
            )),
        }
    }

    /// `name`'s value as an expression of the type at `ty` (`::kiln::shell::
    /// DockPosition`, say): a bare variant, as in `dock = Right`, is that
    /// type's; any other expression stays as written. `default` if not
    /// given.
    pub(crate) fn variant(&self, name: &str, ty: TokenStream, default: &str) -> TokenStream {
        match self.get(name) {
            Some(Expr::Path(ExprPath {
                path, qself: None, ..
            })) if path.get_ident().is_some() => {
                let variant = path.get_ident().expect("an ident");
                quote_spanned!(variant.span()=> #ty::#variant)
            }
            Some(other) => quote!(#other),
            None => {
                let variant = Ident::new(default, Span::call_site());
                quote!(#ty::#variant)
            }
        }
    }

    /// `name`'s value as written, `default` if not given: an `i32`
    /// expression such as `order = -1`.
    pub(crate) fn expr(&self, name: &str, default: TokenStream) -> TokenStream {
        match self.get(name) {
            Some(value) => quote!(#value),
            None => default,
        }
    }

    /// `name`'s path, as in `new = Notes::new`, if given.
    fn path(&self, name: &str) -> syn::Result<Option<&Path>> {
        match self.get(name) {
            None => Ok(None),
            Some(Expr::Path(ExprPath { path, .. })) => Ok(Some(path)),
            Some(other) => Err(syn::Error::new_spanned(
                other,
                format!(
                    "`{name}` takes a fn's path: `{name} = Notes::new` (from {}, which takes {})",
                    self.attribute, self.known
                ),
            )),
        }
    }
}

const ARGUMENTS: &str = "title, icon, dock, toggle_key, order, id and new";

/// The panel's ID: `id = ".."`'s, checked, else `<crate>.<snake name>`;
/// and its toggle command's, `<id>.toggle`; each a `&'static str`
/// expression.
pub(crate) fn ids(
    ident: &Ident,
    id: Option<&LitStr>,
    attribute: &str,
) -> syn::Result<(TokenStream, TokenStream)> {
    Ok(match id {
        Some(id) => {
            check_id(&id.value()).map_err(|why| syn::Error::new(id.span(), why))?;
            let toggle = format!("{}.toggle", id.value());
            (quote!(#id), quote!(#toggle))
        }
        None => {
            let name = ident.to_string().to_snake_case();
            let fits = check_id(&format!("app.{name}")).is_ok();
            if !fits {
                return Err(syn::Error::new_spanned(
                    ident,
                    format!(
                        "{attribute}'s ID would be `<crate>.{name}`, which isn't an ID: \
                         rename it or give the ID with `id = \"..\"`"
                    ),
                ));
            }
            let toggle = format!(".{name}.toggle");
            let name = format!(".{name}");
            (
                quote!(::std::concat!(::std::env!("CARGO_CRATE_NAME"), #name)),
                quote!(::std::concat!(::std::env!("CARGO_CRATE_NAME"), #toggle)),
            )
        }
    })
}

/// `CARGO_BIN_NAME` and the rest, as the records have them, then the
/// record's `file` and `line`, spanned on `span`, where it was declared.
pub(crate) fn site(span: Span) -> TokenStream {
    let file = quote_spanned!(span=> ::std::file!());
    let line = quote_spanned!(span=> ::std::line!());
    quote! {
        file: #file,
        line: #line,
        krate: ::std::env!("CARGO_CRATE_NAME"),
        bin: ::std::option_env!("CARGO_BIN_NAME"),
        test_dir: ::std::option_env!("CARGO_TARGET_TMPDIR"),
    }
}

fn panel(args: TokenStream, item: &mut ItemStruct) -> syn::Result<TokenStream> {
    let args = Values::parse(
        args,
        "#[kiln::panel]",
        &["title", "icon", "dock", "toggle_key", "order", "id", "new"],
        ARGUMENTS,
    )?;
    if !item.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &item.generics,
            "a #[kiln::panel] struct takes no generics: kiln builds one of it for each window",
        ));
    }
    let ident = item.ident.clone();
    let span = ident.span();
    let title = match args.string("title")? {
        Some(title) => title.value(),
        None => {
            let docs = doc_lines(&item.attrs);
            let first = docs
                .iter()
                .map(|line| line.trim())
                .find(|line| !line.is_empty());
            match first {
                Some(title) => title.to_owned(),
                None => {
                    return Err(syn::Error::new(
                        Span::call_site(),
                        "#[kiln::panel] needs a title: `title = \"Notes\"`, or a doc comment \
                         whose first line is the title",
                    ));
                }
            }
        }
    };
    let (id, toggle) = ids(&ident, args.string("id")?.as_ref(), "the panel")?;
    let icon = args.variant("icon", quote!(::kiln::ui::Icon), "Files");
    let position = args.variant("dock", quote!(::kiln::shell::DockPosition), "Left");
    let order = args.expr("order", quote!(0));
    let toggle_key = match args.string("toggle_key")? {
        Some(key) => quote!(::std::option::Option::Some(#key)),
        None => quote!(::std::option::Option::None),
    };
    // Spanned on the user's tokens, so a `new` of another signature, or a
    // struct without `Default`, is reported there.
    let new = match args.path("new")? {
        Some(path) => quote_spanned! {path_span(path)=>
            |cx: &mut ::kiln::Cx<'_, '_, #ident>| #path(cx)
        },
        None => quote_spanned! {span=>
            |_: &mut ::kiln::Cx<'_, '_, #ident>| <#ident as ::std::default::Default>::default()
        },
    };
    let state = add_state(item)?;
    let view_root = quote_spanned!(span=> ::kiln::__private::view_root);
    let site = site(span);

    Ok(quote! {
        const _: () = {
            impl ::kiln::gpui::Render for #ident {
                fn render(
                    &mut self,
                    window: &mut ::kiln::gpui::Window,
                    cx: &mut ::kiln::gpui::Context<Self>,
                ) -> impl ::kiln::gpui::IntoElement {
                    let focus = self.#state.focus_handle(cx);
                    let element = #view_root(self, &focus, window, cx);
                    ::kiln::__private::attach!(#ident, element, cx)
                }
            }

            impl ::kiln::gpui::Focusable for #ident {
                fn focus_handle(&self, cx: &::kiln::gpui::App) -> ::kiln::gpui::FocusHandle {
                    self.#state.focus_handle(cx)
                }
            }

            impl ::kiln::shell::DockItem for #ident {
                fn id(&self) -> ::kiln::shell::PanelId {
                    ::kiln::shell::PanelId(#id)
                }

                fn title(&self, _: &::kiln::gpui::App) -> ::kiln::gpui::SharedString {
                    ::kiln::gpui::SharedString::from(#title)
                }

                fn icon(&self, _: &::kiln::gpui::App) -> ::kiln::ui::Icon {
                    #icon
                }

                fn show_command(&self) -> ::std::option::Option<&'static str> {
                    ::std::option::Option::Some(#toggle)
                }
            }

            fn __kiln_make(
                window: &mut ::kiln::gpui::Window,
                cx: &mut ::kiln::gpui::App,
            ) -> ::kiln::AnyDockPanel {
                ::kiln::__private::make_panel::<#ident>(window, cx, #new)
            }

            ::kiln::__private::inventory::submit! {
                ::kiln::__private::PanelRecord {
                    id: #id,
                    title: #title,
                    icon: #icon,
                    position: #position,
                    order: #order,
                    toggle_key: #toggle_key,
                    make: __kiln_make,
                    #site
                }
            }
        };
    })
}

fn path_span(path: &Path) -> Span {
    path.segments
        .last()
        .map_or_else(Span::call_site, |segment| segment.ident.span())
}

/// Adds the hidden `__kiln` field to `item`, a unit struct becoming one
/// with named fields; the member it is, `__kiln` or the tuple's last
/// index.
fn add_state(item: &mut ItemStruct) -> syn::Result<TokenStream> {
    let ty: syn::Type = parse_quote!(::kiln::__private::ViewState);
    match &mut item.fields {
        Fields::Named(fields) => {
            if let Some(field) = (fields.named.iter())
                .find(|field| field.ident.as_ref().is_some_and(|ident| ident == "__kiln"))
            {
                return Err(syn::Error::new_spanned(
                    field,
                    "#[kiln::panel] adds the field `__kiln` itself",
                ));
            }
            fields.named.push(named_state(ty));
            Ok(quote!(__kiln))
        }
        Fields::Unnamed(fields) => {
            let index = syn::Index::from(fields.unnamed.len());
            fields.unnamed.push(Field {
                attrs: vec![parse_quote!(#[doc(hidden)])],
                vis: syn::Visibility::Inherited,
                mutability: syn::FieldMutability::None,
                ident: None,
                colon_token: None,
                ty,
            });
            Ok(quote!(#index))
        }
        Fields::Unit => {
            let mut named = Punctuated::new();
            named.push(named_state(ty));
            item.fields = Fields::Named(FieldsNamed {
                brace_token: Default::default(),
                named,
            });
            // `struct Notes;` is `struct Notes { .. }` now: no semicolon.
            item.semi_token = None;
            Ok(quote!(__kiln))
        }
    }
}

fn named_state(ty: syn::Type) -> Field {
    Field {
        attrs: vec![parse_quote!(#[doc(hidden)])],
        vis: syn::Visibility::Inherited,
        mutability: syn::FieldMutability::None,
        ident: Some(Ident::new("__kiln", Span::call_site())),
        colon_token: Some(Default::default()),
        ty,
    }
}
