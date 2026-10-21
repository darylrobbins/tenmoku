// SPDX-License-Identifier: LicenseRef-Proprietary

//! The `kiln:extension@1.0.0` world the host and plugins share, and its generated bindings.
//!
//! The WIT lives in `wit/`, one file per interface, so a card that implements an interface edits
//! only its own file. [`HostState`] is the store data a plugin's imports run against: it holds one
//! `State` per slice (`commands::State` and so on), each defined in that slice's module beside its
//! `Host` impl, so a slice adds fields without touching this file. A slice whose card has not
//! landed answers every call with [`unimplemented`], an error value rather than a trap.

use wasmtime::component::{HasSelf, Linker};

use crate::{commands, events, fs, net, process, secrets};

wasmtime::component::bindgen!({
    world: "extension",
    path: "wit",
    additional_derives: [PartialEq, Eq],
});

pub use kiln::extension::log::Level;

/// Receives the plugin's `log` calls.
pub type LogSink = Box<dyn FnMut(Level, String) + Send>;

/// The data a plugin's store holds: what its host imports read and write.
pub struct HostState {
    pub log: LogSink,
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
            commands: commands::State,
            events: events::State,
            fs: fs::State,
            process: process::State,
            net: net::State,
            secrets: secrets::State,
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
