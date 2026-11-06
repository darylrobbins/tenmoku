// SPDX-License-Identifier: Apache-2.0

//! Plugin host: runs plugins as WebAssembly components on the workspace's single wasmtime.
//! Headless; must not depend on GPUI or any application crate.

pub mod bindings;
pub mod cache;
pub mod commands;
pub mod compat;
pub mod engine;
pub mod events;
pub mod fs;
pub mod grants;
pub mod limits;
pub mod manifest;
pub mod net;
#[cfg(test)]
mod prelaunch_tests;
pub mod process;
#[cfg(test)]
mod program_budget_tests;
pub mod runner;
pub mod secrets;
pub mod warnings;
