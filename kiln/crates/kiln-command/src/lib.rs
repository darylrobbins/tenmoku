// SPDX-License-Identifier: LicenseRef-Proprietary

//! Commands: stable namespaced IDs, typed args, the contexts they apply in,
//! serializable invocation records, and a registry that dispatches them
//! headlessly. Knows nothing about any particular app: an app, or a
//! component such as a text editor, registers its commands on it. Must never
//! depend on GPUI.
//!
//! The design is recorded in `docs/decisions/command-model.md`.

mod id;
mod invocation;
mod predicate;
mod registry;

pub use id::{CommandId, InvalidCommandId};
pub use invocation::{Actor, Invocation, Source, Target};
pub use predicate::{ContextFrame, ContextPredicate, ContextStack, PredicateError};
pub use registry::{
    CommandBuilder, CommandError, CommandMeta, CommandTarget, DispatchError, Dispatched, Effect,
    RegisterError, Registry,
};
pub use serde_json::Value;

#[cfg(test)]
mod tests;
