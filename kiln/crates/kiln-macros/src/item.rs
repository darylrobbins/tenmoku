// SPDX-License-Identifier: LicenseRef-Proprietary

//! `#[kiln::item]` on a struct that implements `kiln::Item` and
//! `kiln::View`: the struct stays as it is, and gets `Render` (through
//! `View`, with its `ViewCommands` attached), `Focusable`, `PaneItem` and
//! `ItemState` through `Item`; and an `inventory` record of its kind,
//! which `kiln::app` or the crate's `kiln::module!` contributes, so that
//! its saved items restore.

use heck::ToSnakeCase as _;
use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};
use syn::ItemStruct;

use crate::{
    command::check_id,
    panel::{Values, site},
};

const ARGUMENTS: &str = "kind";

/// The struct, then what makes it an item; the struct and the errors,
/// spanned on the user's tokens, if it can't.
pub(crate) fn expand(args: TokenStream, item: TokenStream) -> TokenStream {
    let item = match syn::parse2::<ItemStruct>(item.clone()) {
        Ok(item) => item,
        Err(_) => {
            let error = syn::Error::new(
                Span::call_site(),
                "#[kiln::item] goes on a struct that implements kiln::Item and kiln::View",
            )
            .into_compile_error();
            return quote!(#item #error);
        }
    };
    match pane_item(args, &item) {
        Ok(expanded) => quote!(#item #expanded),
        Err(error) => {
            let error = error.into_compile_error();
            quote!(#item #error)
        }
    }
}

fn pane_item(args: TokenStream, item: &ItemStruct) -> syn::Result<TokenStream> {
    let args = Values::parse(args, "#[kiln::item]", &["kind"], ARGUMENTS)?;
    if !item.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &item.generics,
            "a #[kiln::item] struct takes no generics: its kind is one in saved layouts",
        ));
    }
    let ident = &item.ident;
    let span = ident.span();
    let kind = match args.string("kind")? {
        Some(kind) => {
            check_id(&kind.value()).map_err(|why| {
                syn::Error::new(kind.span(), why.replace("a command ID", "an item kind"))
            })?;
            quote!(#kind)
        }
        None => {
            let name = ident.to_string().to_snake_case();
            if check_id(&format!("app.{name}")).is_err() {
                return Err(syn::Error::new_spanned(
                    ident,
                    format!(
                        "the item's kind would be `<crate>.{name}`, which isn't one: \
                         rename it or give the kind with `kind = \"..\"`"
                    ),
                ));
            }
            let name = format!(".{name}");
            quote!(::std::concat!(::std::env!("CARGO_CRATE_NAME"), #name))
        }
    };
    // Spanned on the struct's name, so a struct that isn't an `Item` or a
    // `View` is reported there.
    let as_item = quote_spanned!(span=> <#ident as ::kiln::Item>);
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
                    let focus = #as_item::focus(self, cx);
                    let element = #view_root(self, &focus, window, cx);
                    ::kiln::__private::attach!(#ident, element, cx)
                }
            }

            impl ::kiln::gpui::Focusable for #ident {
                fn focus_handle(&self, cx: &::kiln::gpui::App) -> ::kiln::gpui::FocusHandle {
                    #as_item::focus(self, cx)
                }
            }

            impl ::kiln::shell::PaneItem for #ident {
                const KIND: &'static str = #kind;

                fn tab_title(&self, cx: &::kiln::gpui::App) -> ::kiln::gpui::SharedString {
                    #as_item::title(self, cx)
                }

                fn tab_icon(
                    &self,
                    cx: &::kiln::gpui::App,
                ) -> ::std::option::Option<::kiln::ui::Icon> {
                    #as_item::icon(self, cx)
                }

                fn is_dirty(&self, cx: &::kiln::gpui::App) -> bool {
                    #as_item::is_dirty(self, cx)
                }

                fn status_items(
                    &self,
                    cx: &::kiln::gpui::App,
                ) -> ::std::vec::Vec<::kiln::shell::StatusItem> {
                    #as_item::status_items(self, cx)
                }

                fn may_close(
                    &mut self,
                    window: &mut ::kiln::gpui::Window,
                    cx: &mut ::kiln::gpui::Context<Self>,
                ) -> ::kiln::shell::CloseAnswer {
                    #as_item::may_close(self, window, cx)
                }

                fn close_revision(&self, cx: &::kiln::gpui::App) -> u64 {
                    #as_item::close_revision(self, cx)
                }

                fn split(
                    &mut self,
                    window: &mut ::kiln::gpui::Window,
                    cx: &mut ::kiln::gpui::Context<Self>,
                ) -> ::std::option::Option<::kiln::gpui::Entity<Self>> {
                    let split = #as_item::split(self, window, cx)?;
                    ::std::option::Option::Some(::kiln::gpui::AppContext::new(cx, |_| split))
                }

                fn serialize(
                    &self,
                    cx: &::kiln::gpui::App,
                ) -> ::std::option::Option<::kiln::command::Value> {
                    ::kiln::shell::item::state::serialize(self, cx)
                }

                fn restore(
                    state: ::kiln::command::Value,
                    window: &mut ::kiln::gpui::Window,
                    cx: &mut ::kiln::gpui::App,
                ) -> ::kiln::gpui::Task<::std::option::Option<::kiln::gpui::Entity<Self>>> {
                    ::kiln::shell::item::state::restore::<Self>(state, window, cx)
                }

                fn apply_state(
                    &mut self,
                    state: &::kiln::command::Value,
                    window: &mut ::kiln::gpui::Window,
                    cx: &mut ::kiln::gpui::Context<Self>,
                ) {
                    ::kiln::shell::item::state::apply(self, state, window, cx)
                }
            }

            impl ::kiln::shell::item::ItemState for #ident {
                type State = #as_item::State;

                fn state(
                    &self,
                    _: &::kiln::gpui::App,
                ) -> ::std::option::Option<Self::State> {
                    ::std::option::Option::Some(#as_item::state(self))
                }

                fn from_state(
                    state: Self::State,
                    window: &mut ::kiln::gpui::Window,
                    cx: &mut ::kiln::gpui::App,
                ) -> ::kiln::gpui::Task<::std::option::Option<::kiln::gpui::Entity<Self>>> {
                    ::kiln::__private::restore_item::<Self>(state, window, cx)
                }
            }

            ::kiln::__private::inventory::submit! {
                ::kiln::__private::ItemRecord {
                    kind: #kind,
                    register: ::kiln::shell::layout::register_item::<#ident>,
                    #site
                }
            }
        };
    })
}
