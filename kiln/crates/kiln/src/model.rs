// SPDX-License-Identifier: LicenseRef-Proprietary

//! Models: app-wide state keyed by its type, each an [`Entity`] kiln holds,
//! made from `Default` the first time it's read or [`register`]ed. A view
//! reads one through its [`Cx`](crate::Cx): `cx.watch::<M>()` to redraw
//! when it changes, `cx.model::<M>()` to read it without, and
//! `cx.update_model(..)` to change it. And services: app-wide values a
//! view reads with `cx.service::<T>()`, once [`provide`]d.
//!
//! A view's watches need no [`Subscription`] of its own: kiln holds one
//! per view and model, made the first time the view watches the model and
//! dropped when the view is released, so a view that watches on every
//! render observes once.
//!
//! ```
//! use kiln::{Contributions, gpui::App};
//!
//! #[derive(Default)]
//! struct Recent(Vec<String>);
//!
//! fn module(cx: &mut App) {
//!     Contributions::add_model(cx, kiln::model::register::<Recent>);
//! }
//! # let _: kiln::Module = module;
//! ```

use std::{
    any::{Any, TypeId},
    collections::HashMap,
};

pub mod persist;

use gpui::{AnyEntity, App, AppContext as _, Context, Entity, EntityId, Global, Subscription};

/// Every model, by its type.
#[derive(Default)]
struct Models(HashMap<TypeId, AnyEntity>);

impl Global for Models {}

/// What each view watches: by view, then by the type watched (a model's,
/// or `GlobalSettings<S>`), the subscription that redraws it.
#[derive(Default)]
struct Watches(HashMap<EntityId, HashMap<TypeId, Subscription>>);

impl Global for Watches {}

/// Every service, by its type.
#[derive(Default)]
struct Services(HashMap<TypeId, Box<dyn Any>>);

impl Global for Services {}

/// The model `M`, made from `M::default()` if there's none yet: GPUI's
/// own handle, to observe or update by hand.
pub fn entity<M: Default + 'static>(cx: &mut App) -> Entity<M> {
    let found = (cx.try_global::<Models>()).and_then(|models| models.0.get(&TypeId::of::<M>()));
    if let Some(model) = found {
        return model.clone().downcast().expect("models are keyed by type");
    }
    let model = cx.new(|_| M::default());
    let models = &mut cx.default_global::<Models>().0;
    models.insert(TypeId::of::<M>(), model.clone().into_any());
    model
}

/// The model `M`, if it's made.
pub(crate) fn entity_if<M: 'static>(cx: &App) -> Option<Entity<M>> {
    let models = cx.try_global::<Models>()?;
    Some(
        models
            .0
            .get(&TypeId::of::<M>())?
            .clone()
            .downcast()
            .expect("models are keyed by type"),
    )
}

/// Makes the model `M` from `value`, or sets it to `value` if it's made.
pub(crate) fn insert<M: 'static>(cx: &mut App, value: M) -> Entity<M> {
    let found = (cx.try_global::<Models>()).and_then(|models| models.0.get(&TypeId::of::<M>()));
    if let Some(model) = found {
        let model: Entity<M> = model.clone().downcast().expect("models are keyed by type");
        model.update(cx, |model, cx| {
            *model = value;
            cx.notify();
        });
        return model;
    }
    let model = cx.new(|_| value);
    let models = &mut cx.default_global::<Models>().0;
    models.insert(TypeId::of::<M>(), model.clone().into_any());
    model
}

/// Makes the model `M` from `M::default()`, unless it's made: a
/// [`Contributions::add_model`](crate::Contributions::add_model) call, as
/// `kiln::model::register::<M>`.
pub fn register<M: Default + 'static>(cx: &mut App) {
    entity::<M>(cx);
}

/// The model `M`, made from `M::default()` if there's none yet. Read in a
/// view's render, GPUI redraws its window when `M` changes, but a view
/// drawn cached ([`gpui::AnyView::cached`]) only if it watches `M`
/// ([`Cx::watch`](crate::Cx::watch)).
pub fn read<M: Default + 'static>(cx: &mut App) -> &M {
    let model = entity::<M>(cx);
    model.read(cx)
}

/// Changes the model `M` and notifies its observers, the views watching
/// it among them.
pub fn update<M: Default + 'static, R>(
    cx: &mut App,
    update: impl FnOnce(&mut M, &mut Context<M>) -> R,
) -> R {
    entity::<M>(cx).update(cx, |model, cx| {
        let result = update(model, cx);
        cx.notify();
        result
    })
}

/// How many views watch `T`, a model or `GlobalSettings<S>`: each counts
/// once however often it watches, until it's released.
pub fn watchers<T: 'static>(cx: &App) -> usize {
    let key = TypeId::of::<T>();
    cx.try_global::<Watches>().map_or(0, |watches| {
        (watches.0.values())
            .filter(|keys| keys.contains_key(&key))
            .count()
    })
}

/// Has the view `cx` updates redraw through the subscription `observe`
/// makes, unless it already watches `key`. Kiln holds the subscription
/// until the view is released.
pub(crate) fn watch<V: 'static>(
    key: TypeId,
    cx: &mut Context<V>,
    observe: impl FnOnce(&mut Context<V>) -> Subscription,
) {
    let view = cx.entity_id();
    let watches = &cx.default_global::<Watches>().0;
    let watched = watches.get(&view);
    if watched.is_some_and(|keys| keys.contains_key(&key)) {
        return;
    }
    let first = watched.is_none();
    let subscription = observe(cx);
    if first {
        cx.on_release(move |_, cx| forget(view, cx)).detach();
    }
    let watches = &mut cx.default_global::<Watches>().0;
    watches.entry(view).or_default().insert(key, subscription);
}

/// Drops the released `view`'s subscriptions.
fn forget(view: EntityId, cx: &mut App) {
    if cx.has_global::<Watches>() {
        let dropped = cx.global_mut::<Watches>().0.remove(&view);
        drop(dropped);
    }
}

/// Provides `value` app-wide, in place of any `T` provided before: a view
/// reads it with [`Cx::service`](crate::Cx::service), anything else with
/// [`service`].
pub fn provide<T: 'static>(cx: &mut App, value: T) {
    let services = &mut cx.default_global::<Services>().0;
    services.insert(TypeId::of::<T>(), Box::new(value));
}

/// The `T` [`provide`]d, if one was.
pub fn service<T: 'static>(cx: &App) -> Option<&T> {
    let service = cx.try_global::<Services>()?.0.get(&TypeId::of::<T>())?;
    service.downcast_ref()
}
