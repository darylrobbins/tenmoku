// SPDX-License-Identifier: Apache-2.0

//! The one `wasmtime::Engine` every plugin runs on, with epoch interruption, and the ticker
//! thread advancing its epoch while any call into a plugin is running.
//!
//! Modelled on the editor's Wasm grammar runtime: the ticker sleeps while no call runs, so an idle
//! app pays nothing for it. Each tick also wakes the calls waiting on a host import
//! ([`wake_on_tick`]), so they see their deadline pass.

use std::{
    mem,
    sync::{Condvar, Mutex, OnceLock, PoisonError},
    task::Waker,
    thread,
    time::Duration,
};

use kiln_l10n::tr;
use wasmtime::{Config, Engine};

/// How often the epoch advances while a call runs, so how often running guest code checks
/// its deadline.
const EPOCH: Duration = Duration::from_millis(2);

/// The engine plugins are compiled and run on.
#[derive(Clone)]
pub struct PluginEngine {
    engine: Engine,
}

/// Why there is no [`PluginEngine`].
#[derive(Clone, Debug, thiserror::Error)]
pub struct EngineError(String);

/// In the active locale.
impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&tr!("kiln-plugin-no-engine", reason = &self.0))
    }
}

impl PluginEngine {
    /// The process-wide engine, its ticker started on first use. Without a ticker, an error:
    /// a plugin's code would run with no deadline.
    pub fn shared() -> Result<&'static Self, EngineError> {
        static SHARED: OnceLock<Result<PluginEngine, EngineError>> = OnceLock::new();
        SHARED
            .get_or_init(Self::start)
            .as_ref()
            .map_err(Clone::clone)
    }

    fn start() -> Result<Self, EngineError> {
        let mut config = Config::new();
        config.wasm_component_model(true).epoch_interruption(true);
        let engine = Engine::new(&config).map_err(|error| EngineError(error.to_string()))?;
        let ticking = engine.clone();
        thread::Builder::new()
            .name("wasm-plugin-epoch".into())
            .spawn(move || tick(&ticking))
            .map_err(|error| {
                EngineError(
                    tr!("kiln-plugin-no-deadline-thread", reason = error.to_string()).to_string(),
                )
            })?;
        Ok(Self { engine })
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Keeps the epoch advancing until the guard drops: held while guest code runs.
    pub(crate) fn ticking(&self) -> Ticking {
        *RUNNING.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        STARTED.notify_all();
        Ticking
    }
}

/// How many [`Ticking`] guards are held, which the ticker waits for.
static RUNNING: Mutex<usize> = Mutex::new(0);
static STARTED: Condvar = Condvar::new();

/// The calls to wake on the next tick.
static WAITING: Mutex<Vec<Waker>> = Mutex::new(Vec::new());

/// Wakes `waker` on the next tick. Only while a [`Ticking`] guard is held is there one.
pub(crate) fn wake_on_tick(waker: &Waker) {
    let mut waiting = WAITING.lock().unwrap_or_else(PoisonError::into_inner);
    if !waiting.iter().any(|waiting| waiting.will_wake(waker)) {
        waiting.push(waker.clone());
    }
}

/// A call running on the engine: [`PluginEngine::ticking`].
pub(crate) struct Ticking;

impl Drop for Ticking {
    fn drop(&mut self) {
        *RUNNING.lock().unwrap_or_else(PoisonError::into_inner) -= 1;
    }
}

/// The ticker: advances `engine`'s epoch and wakes the waiting calls every [`EPOCH`] while a
/// call is running, and sleeps while none is.
fn tick(engine: &Engine) {
    loop {
        let running = RUNNING.lock().unwrap_or_else(PoisonError::into_inner);
        drop(
            STARTED
                .wait_while(running, |running| *running == 0)
                .unwrap_or_else(PoisonError::into_inner),
        );
        thread::sleep(EPOCH);
        engine.increment_epoch();
        let waiting = mem::take(&mut *WAITING.lock().unwrap_or_else(PoisonError::into_inner));
        waiting.into_iter().for_each(Waker::wake);
    }
}
