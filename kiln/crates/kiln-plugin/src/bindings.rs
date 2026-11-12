// SPDX-License-Identifier: Apache-2.0

//! The `kiln:extension` worlds the host and plugins share, and their generated bindings: one world
//! per supported API minor, which [`compat::world_for`](crate::compat::world_for) picks.
//!
//! The current minor's WIT lives in `wit/`, one file per interface, so a card that implements an
//! interface edits only its own file; its bindings are this module's. Each released minor is
//! frozen in `wit/released/<minor>/`, and its world generated in a module of its own ([`v1_0`])
//! whose interfaces are mapped onto the current ones: a minor only adds, so the types and `Host`
//! impls are shared, and one linker serves every minor's imports (wasmtime matches `@1.0.0`
//! against `@1.1.0`). What a world adds is its exports, which a component is checked against. [`HostState`] is the store data a plugin's imports run against: it holds one
//! `State` per slice (`commands::State` and so on), each defined in that slice's module beside its
//! `Host` impl, so a slice adds fields without touching this file. A slice whose card has not
//! landed answers every call with [`unimplemented`], an error value rather than a trap.

use std::time::Instant;

use wasmtime::component::{HasSelf, Linker};

use crate::{commands, events, fs, net, process, secrets};

wasmtime::component::bindgen!({
    world: "extension",
    path: "wit",
    additional_derives: [PartialEq, Eq],
});

/// The frozen `kiln:extension@1.0.0` world, from `wit/released/1.0`, over the current
/// interfaces' types.
pub mod v1_0 {
    wasmtime::component::bindgen!({
        world: "extension",
        path: "wit/released/1.0",
        with: {
            "kiln:extension": crate::bindings::kiln::extension,
        },
    });
}

pub use kiln::extension::log::Level;

/// Receives the plugin's `log` calls.
pub type LogSink = Box<dyn FnMut(Level, String) + Send>;

/// The data a plugin's store holds: what its host imports read and write.
pub struct HostState {
    pub log: LogSink,
    /// When the call into the plugin running now must end, as the runner sets it: none outside
    /// a call, or with no deadline. An import that waits stops waiting then.
    pub deadline: Option<Instant>,
    /// What the call's program runs have taken of their share, which the call's deadline doesn't
    /// count: the runner arms it at each call's start, and `process.spawn` charges it.
    pub program_runs: process::ProgramBudget,
    pub commands: commands::State,
    pub events: events::State,
    pub fs: fs::State,
    pub process: process::State,
    pub net: net::State,
    pub secrets: secrets::State,
}

impl HostState {
    /// A state whose plugin logs go to `log`.
    pub fn new(log: LogSink) -> Self {
        Self {
            log,
            deadline: None,
            program_runs: process::ProgramBudget::default(),
            commands: commands::State::default(),
            events: events::State,
            fs: fs::State,
            process: process::State::default(),
            net: net::State::default(),
            secrets: secrets::State::default(),
        }
    }
}

impl Default for HostState {
    /// Plugin logs go to the `log` crate, under the `kiln_plugin::guest` target.
    fn default() -> Self {
        Self::new(Box::new(|level, message| {
            let level = match level {
                Level::Trace => log::Level::Trace,
                Level::Debug => log::Level::Debug,
                Level::Info => log::Level::Info,
                Level::Warn => log::Level::Warn,
                Level::Error => log::Level::Error,
            };
            log::log!(target: "kiln_plugin::guest", level, "{message}");
        }))
    }
}

/// Adds every `kiln:extension` import to `linker`, run against the [`HostState`] `get` returns.
pub fn add_to_linker<T: 'static>(
    linker: &mut Linker<T>,
    get: fn(&mut T) -> &mut HostState,
) -> wasmtime::Result<()> {
    Extension::add_to_linker::<T, HasSelf<HostState>>(linker, get)
}

/// The error an import returns until its card implements it. `import` names it as
/// `interface.function`, such as `kiln:extension/process.spawn`.
pub fn unimplemented<T>(import: &str) -> Result<T, Error> {
    Err(Error::Unimplemented(import.to_owned()))
}

impl kiln::extension::types::Host for HostState {}

impl kiln::extension::log::Host for HostState {
    fn log(&mut self, level: Level, message: String) {
        (self.log)(level, message);
    }
}
