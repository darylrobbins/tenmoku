// SPDX-License-Identifier: Apache-2.0

//! Commands: stable namespaced IDs, typed args, the contexts they apply in,
//! serializable invocation records, and a registry that dispatches them
//! headlessly. Knows nothing about any particular app: an app, or a
//! component such as a text editor, registers its commands on it. Must never
//! depend on GPUI.
//!
//! [`capability`] names what a plugin may do, in the plugin record's
//! strings (`buffer.read`, `net:<host>`), and the [`Grants`] it holds. A
//! plugin's invocation runs only if its grants cover the command's
//! [`PluginAccess`], checked against a [`PluginPolicy`].
//!
//! The design is recorded in `docs/command-model.md` in kiln.

mod authorize;
pub mod capability;
mod id;
mod invocation;
mod predicate;
mod registry;

pub use authorize::{PluginAccess, PluginDenied, PluginPolicy};
pub use capability::{Capability, CapabilityError, FsScope, Grants, NetHosts};
pub use id::{CommandId, InvalidCommandId};
pub use invocation::{Actor, Invocation, Source, Target};
pub use predicate::{ContextFrame, ContextPredicate, ContextStack, PredicateError};
pub use registry::{
    CommandBuilder, CommandError, CommandMeta, CommandTarget, DispatchError, Dispatched, Effect,
    RegisterError, Registry, UnknownCommand,
};
pub use serde_json::Value;

#[cfg(test)]
mod tests;
