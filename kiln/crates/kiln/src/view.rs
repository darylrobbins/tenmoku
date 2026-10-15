// SPDX-License-Identifier: LicenseRef-Proprietary

//! [`View`]: the `Render` of a view kiln hosts, which `#[kiln::panel]`
//! puts in a dock; and [`ViewCommands`]: a view's own commands, which run
//! where focus is, on the view focus is in. `#[kiln::command(context =
//! "..")]` on the view's impl implements it.

use std::{cell::OnceCell, fmt, marker::PhantomData};

use gpui::{
    Action, App, AppContext as _, Context, Div, FocusHandle, InteractiveElement, IntoElement,
    ParentElement as _, SharedString, Styled as _, Window, div,
};
use kiln_shell::{DockItem, StatusItem, StatusSide, Workspace};

use crate::{AnyDockPanel, Cx};

/// What a view kiln hosts draws: GPUI's `Render`, with a [`Cx`].
/// `#[kiln::panel]` ([`crate::panel`]) on the view's struct implements
/// `Render` through it, around the view's element: the view's focus handle
/// tracked, its [`ViewCommands`] attached, and the dock's full size.
///
/// ```
/// use kiln::{Cx, View, gpui::{IntoElement, ParentElement as _, div}};
///
/// #[derive(Default)]
/// struct Count(usize);
///
/// #[kiln::panel(title = "Notes", toggle_key = "secondary-b")]
/// #[derive(Default)]
/// struct Notes;
///
/// impl View for Notes {
///     fn render(&mut self, cx: &mut Cx<Self>) -> impl IntoElement {
///         // Redraws each time `Count` changes, no Subscription stored.
///         let count = cx.watch::<Count>().0;
///         div().child(format!("{count} notes"))
///     }
/// }
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a kiln::View",
    label = "#[kiln::panel] needs `impl kiln::View for {Self}`",
    note = "`impl View for {Self} {{ fn render(&mut self, cx: &mut Cx<Self>) -> impl IntoElement {{ .. }} }}`"
)]
pub trait View: Sized + 'static {
    /// The view's element, drawn each time it's notified or a model it
    /// watches changes ([`Cx::watch`]).
    fn render(&mut self, cx: &mut Cx<'_, '_, Self>) -> impl IntoElement;
}

/// What a view kiln's macros host keeps of kiln's: its focus handle, made
/// the first time it's asked for, which kiln does as it builds the view.
/// `#[kiln::panel]` adds it to the struct as the hidden field `__kiln`;
/// a `new` fn fills that with `Default::default()`.
#[derive(Default)]
pub struct ViewState {
    focus: OnceCell<FocusHandle>,
}

impl ViewState {
    /// The view's focus handle, made now if it has none yet.
    pub fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.focus.get_or_init(|| cx.focus_handle()).clone()
    }
}

/// A copy has a focus handle of its own: two views never share one.
impl Clone for ViewState {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl fmt::Debug for ViewState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ViewState").finish_non_exhaustive()
    }
}

/// Each equal to every other, so a view deriving `PartialEq` compares by
/// its own fields.
impl PartialEq for ViewState {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for ViewState {}

/// `view`'s [`View::render`] in a full-size element tracking `focus`: the
/// root the view macros give a hosted view, before attaching its
/// [`ViewCommands`].
#[doc(hidden)]
pub fn view_root<V: View>(
    view: &mut V,
    focus: &FocusHandle,
    window: &mut Window,
    cx: &mut Context<V>,
) -> Div {
    // Made an element at once: the view's element may borrow the `Cx`.
    let content = view.render(&mut Cx { window, cx }).into_any_element();
    div().track_focus(focus).size_full().child(content)
}

/// The panel `new` builds, its focus handle made: `#[kiln::panel]`'s
/// [`crate::MakePanel`].
#[doc(hidden)]
pub fn make_panel<V: DockItem>(
    window: &mut Window,
    cx: &mut App,
    new: impl FnOnce(&mut Cx<'_, '_, V>) -> V,
) -> AnyDockPanel {
    let panel = cx.new(|cx| {
        let view = new(&mut Cx { window, cx });
        view.focus_handle(cx);
        view
    });
    panel.into()
}

/// What a `#[kiln::status]` fn returns: its text, shown as the item `id`
/// on `side` by `priority`, or the item as it is; `None` shows nothing.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "a #[kiln::status] fn returns a String, an Option<String> or a StatusItem, not `{Self}`",
    label = "returns `{Self}`"
)]
pub trait IntoStatus {
    fn into_status(self, id: &'static str, side: StatusSide, priority: i32) -> Option<StatusItem>;
}

impl IntoStatus for StatusItem {
    fn into_status(self, _: &'static str, _: StatusSide, _: i32) -> Option<StatusItem> {
        Some(self)
    }
}

impl IntoStatus for String {
    fn into_status(self, id: &'static str, side: StatusSide, priority: i32) -> Option<StatusItem> {
        Some(StatusItem::new(id, side, priority, self))
    }
}

impl IntoStatus for &'static str {
    fn into_status(self, id: &'static str, side: StatusSide, priority: i32) -> Option<StatusItem> {
        Some(StatusItem::new(id, side, priority, self))
    }
}

impl IntoStatus for SharedString {
    fn into_status(self, id: &'static str, side: StatusSide, priority: i32) -> Option<StatusItem> {
        Some(StatusItem::new(id, side, priority, self))
    }
}

impl<T: IntoStatus> IntoStatus for Option<T> {
    fn into_status(self, id: &'static str, side: StatusSide, priority: i32) -> Option<StatusItem> {
        self?.into_status(id, side, priority)
    }
}

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
