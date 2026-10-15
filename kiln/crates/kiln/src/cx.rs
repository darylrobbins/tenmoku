// SPDX-License-Identifier: LicenseRef-Proprietary

//! [`Cx`]: a view's window and context together, with kiln's reads of
//! models, settings, the workspace and services, so a view keeps no
//! [`Subscription`](gpui::Subscription) to redraw when they change.

use std::{
    any::{TypeId, type_name},
    collections::HashMap,
    ops::{Deref, DerefMut},
};

use gpui::{App, Context, Entity, WeakEntity, Window, WindowId};
use kiln_platform::config::GlobalSettings;
use kiln_shell::{ItemPane, Workspace};

use crate::model;

/// A view's window and its context together: it derefs to GPUI's
/// [`Context<V>`], so every call of GPUI's stays one field away, `window`
/// and `cx` both public. [`with_cx`] makes one in a `Render::render`.
///
/// ```
/// use kiln::{
///     Cx,
///     gpui::{Context, IntoElement, Render, Window, div, prelude::*},
/// };
///
/// #[derive(Default)]
/// struct Clicks(usize);
///
/// struct Counter;
///
/// impl Render for Counter {
///     fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
///         kiln::with_cx(window, cx, |cx: &mut Cx<Self>| {
///             // A Context call, through Deref:
///             let id = cx.entity_id();
///             // a Window call, on its field:
///             let scale = cx.window.scale_factor();
///             // and kiln's: redraws when `Clicks` changes.
///             let clicks = cx.watch::<Clicks>().0;
///             div()
///                 .id("counter")
///                 .child(format!("{id} at {scale}x: {clicks}"))
///                 .on_click(cx.handler(|_: &mut Counter, cx| {
///                     cx.update_model(|clicks: &mut Clicks, _| clicks.0 += 1);
///                 }))
///         })
///     }
/// }
/// ```
pub struct Cx<'a, 'b, V> {
    pub window: &'a mut Window,
    pub cx: &'a mut Context<'b, V>,
}

impl<'b, V> Deref for Cx<'_, 'b, V> {
    type Target = Context<'b, V>;

    fn deref(&self) -> &Self::Target {
        self.cx
    }
}

impl<V> DerefMut for Cx<'_, '_, V> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.cx
    }
}

/// Calls `f` with a [`Cx`] of `window` and `cx`: in a handwritten
/// `Render::render`, `kiln::with_cx(window, cx, |cx| div()..)`.
pub fn with_cx<V, R>(
    window: &mut Window,
    cx: &mut Context<V>,
    f: impl FnOnce(&mut Cx<'_, '_, V>) -> R,
) -> R {
    f(&mut Cx { window, cx })
}

impl<V: 'static> Cx<'_, '_, V> {
    /// The model `M`, read without watching it: this view isn't told when
    /// it changes ([`model::read`]).
    pub fn model<M: Default + 'static>(&mut self) -> &M {
        model::read(self.cx)
    }

    /// Changes the model `M` and notifies its observers, the views
    /// watching it among them ([`model::update`]).
    pub fn update_model<M: Default + 'static, R>(
        &mut self,
        update: impl FnOnce(&mut M, &mut Context<M>) -> R,
    ) -> R {
        model::update(self.cx, update)
    }

    /// The model `M`, this view redrawing each time it changes from now
    /// until the view is released. Kiln observes `M` once for the view,
    /// however often it watches.
    pub fn watch<M: Default + 'static>(&mut self) -> &M {
        let entity = model::entity::<M>(self.cx);
        model::watch(TypeId::of::<M>(), self.cx, |cx| {
            cx.observe(&entity, |_, _, cx| cx.notify())
        });
        entity.read(self.cx)
    }

    /// The app's settings of type `S`, watched as [`Cx::watch`] watches a
    /// model, so this view redraws after a reload changes them:
    /// `S::default()` if the app has none ([`crate::prelude::AppExt::settings`]).
    pub fn setting<S: Default + 'static>(&mut self) -> &S {
        model::watch(TypeId::of::<GlobalSettings<S>>(), self.cx, |cx| {
            cx.observe_global::<GlobalSettings<S>>(|_, cx| cx.notify())
        });
        if self.cx.has_global::<GlobalSettings<S>>() {
            return self.cx.global::<GlobalSettings<S>>().settings();
        }
        &self.cx.default_global::<Unset<S>>().0
    }

    /// A listener for `on_click` and the like that updates this view with
    /// a `Cx`, as GPUI's [`Context::listener`] does with a `Context`,
    /// leaving the event out: that one is still at hand for it.
    pub fn handler<E: ?Sized, F>(&self, f: F) -> impl Fn(&E, &mut Window, &mut App) + use<V, E, F>
    where
        F: Fn(&mut V, &mut Cx<'_, '_, V>) + 'static,
    {
        let view = self.cx.weak_entity();
        move |_: &E, window: &mut Window, cx: &mut App| {
            view.update(cx, |view, cx| f(view, &mut Cx { window, cx }))
                .ok();
        }
    }

    /// The workspace of this view's window, which [`crate::app()`] opened.
    ///
    /// # Panics
    ///
    /// If the window isn't a workspace window: [`crate::AppBuilder::root`]'s,
    /// say.
    pub fn workspace(&self) -> Entity<Workspace<ItemPane>> {
        let id = self.window.window_handle().window_id();
        let recorded = (self.cx.try_global::<Workspaces>())
            .and_then(|workspaces| workspaces.0.get(&id))
            .and_then(WeakEntity::upgrade);
        recorded
            .or_else(|| self.window.root::<Workspace<ItemPane>>().flatten())
            .expect("cx.workspace() in a window with no workspace")
    }

    /// The `T` [`crate::provide`] stored.
    ///
    /// # Panics
    ///
    /// Naming `T`, if none was.
    pub fn service<T: 'static>(&self) -> &T {
        model::service::<T>(self.cx).unwrap_or_else(|| {
            panic!(
                "no service {} was provided: kiln::provide(cx, value) provides one",
                type_name::<T>()
            )
        })
    }
}

/// The settings `S` when the app has no store of them.
struct Unset<S>(S);

impl<S: Default> Default for Unset<S> {
    fn default() -> Self {
        Self(S::default())
    }
}

impl<S: 'static> gpui::Global for Unset<S> {}

/// Each window's workspace, as [`crate::app()`] recorded it.
#[derive(Default)]
struct Workspaces(HashMap<WindowId, WeakEntity<Workspace<ItemPane>>>);

impl gpui::Global for Workspaces {}

/// Records `workspace` as `window`'s, for [`Cx::workspace`], forgetting
/// any of a window since closed.
pub(crate) fn record_workspace(
    window: &Window,
    workspace: &Entity<Workspace<ItemPane>>,
    cx: &mut App,
) {
    let workspaces = &mut cx.default_global::<Workspaces>().0;
    workspaces.retain(|_, workspace| workspace.upgrade().is_some());
    workspaces.insert(window.window_handle().window_id(), workspace.downgrade());
}
