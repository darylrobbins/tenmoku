// SPDX-License-Identifier: LicenseRef-Proprietary

//! [`ViewCommands`]: a view's own commands, which run where focus is,
//! on the view focus is in. `#[kiln::command(context = "..")]` on the
//! view's impl implements it.

use std::marker::PhantomData;

use gpui::{Action, App, Context, InteractiveElement, Window};
use kiln_shell::Workspace;

use crate::Cx;

/// A view's own commands: [`ViewCommands::attach`] sets the view's key
/// context, [`ViewCommands::CONTEXT`], on its element, with a handler of
/// each command that runs on the view. The commands' keys and menu items
/// hold where that context does, and the palette lists them there: while
/// focus is inside the view.
///
/// `#[kiln::command(context = "..")]` on the view's impl implements it,
/// from the methods marked `#[command(..)]` ([`crate::command`]); a
/// handwritten `Render` attaches it to its root element:
///
/// ```
/// use kiln::{
///     Cx, ViewCommands as _,
///     gpui::{Context, FocusHandle, IntoElement, Render, Window, div, prelude::*},
/// };
///
/// struct NoteView {
///     focus: FocusHandle,
///     title: String,
/// }
///
/// #[kiln::command(context = "note")]
/// impl NoteView {
///     /// Rename Note
///     #[command(key = "f2", menu = "Edit")]
///     fn rename(&mut self, cx: &mut Cx<Self>) {
///         self.title = "Renamed".into();
///         cx.notify();
///     }
/// }
///
/// impl Render for NoteView {
///     fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
///         Self::attach(div(), cx)
///             .track_focus(&self.focus)
///             .child(self.title.clone())
///     }
/// }
/// ```
///
/// By hand, `notes::Rename` declared with [`crate::commands!`] and added
/// as a [`crate::CommandSpec`] with `.context("note")`:
///
/// ```ignore
/// impl ViewCommands for NoteView {
///     const CONTEXT: &'static str = "note";
///
///     fn attach<E: InteractiveElement>(element: E, cx: &Context<Self>) -> E {
///         element
///             .key_context("note")
///             .on_action(cx.listener(|view, _: &notes::Rename, window, cx| {
///                 view.rename(&mut Cx { window, cx })
///             }))
///     }
/// }
/// ```
pub trait ViewCommands: Sized + 'static {
    /// The view's key context, which its commands' contexts default to.
    const CONTEXT: &'static str;

    /// `element` in the key context [`Self::CONTEXT`], handling each
    /// command by running it on this view.
    fn attach<E: InteractiveElement>(element: E, cx: &Context<Self>) -> E;
}

/// A handler of the command `A` that runs `run` on the view of `cx` with
/// a [`Cx`], for `.on_action(..)`.
#[doc(hidden)]
pub fn view_handler<A: Action, V: 'static>(
    cx: &Context<V>,
    run: impl Fn(&mut V, &mut Cx<'_, '_, V>) + 'static,
) -> impl Fn(&A, &mut Window, &mut App) + 'static {
    let view = cx.weak_entity();
    move |_: &A, window: &mut Window, cx: &mut App| {
        view.update(cx, |view, cx| run(view, &mut Cx { window, cx }))
            .ok();
    }
}

/// A view command's record's handler, which does nothing: the view's
/// [`ViewCommands::attach`] handles the command.
#[doc(hidden)]
pub fn run_in_view(_: &mut Workspace, _: &mut Cx<'_, '_, Workspace>) {}

/// `element` with `$view`'s [`ViewCommands`] attached, `cx` being the
/// view's context, or `element` as it is if `$view` has none: the view
/// macros attach a hosted view's commands through it, no impl required.
///
/// `kiln::__private::attach!(NoteView, div(), cx)`.
#[doc(hidden)]
#[macro_export]
macro_rules! __attach {
    ($view:ty, $element:expr, $cx:expr $(,)?) => {{
        #[allow(unused_imports)]
        use $crate::__private::{AttachNothing as _, AttachViewCommands as _};
        (&$crate::__private::Attach::<$view>::new()).attach($element, $cx)
    }};
}

/// What [`__attach!`] dispatches on, by autoref: [`AttachViewCommands`]
/// takes `&Attach<V>` as it is, so it applies first where `V` has
/// [`ViewCommands`]; [`AttachNothing`] takes it by a further reference,
/// so it applies to any other `V`.
#[doc(hidden)]
pub struct Attach<V>(PhantomData<V>);

impl<V> Attach<V> {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self(PhantomData)
    }
}

#[doc(hidden)]
pub trait AttachViewCommands<V> {
    fn attach<E: InteractiveElement>(&self, element: E, cx: &Context<V>) -> E;
}

impl<V: ViewCommands> AttachViewCommands<V> for Attach<V> {
    fn attach<E: InteractiveElement>(&self, element: E, cx: &Context<V>) -> E {
        V::attach(element, cx)
    }
}

#[doc(hidden)]
pub trait AttachNothing<V> {
    fn attach<E: InteractiveElement>(&self, element: E, cx: &Context<V>) -> E;
}

impl<V> AttachNothing<V> for &Attach<V> {
    fn attach<E: InteractiveElement>(&self, element: E, _: &Context<V>) -> E {
        element
    }
}
