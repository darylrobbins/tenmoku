// SPDX-License-Identifier: LicenseRef-Proprietary

//! Plugin API version compatibility checks.
//!
//! Stub: a later card of the plugin series links shims for older API versions here.

use wasmtime::component::Linker;

/// Adds the imports older API versions need to `linker`; none yet.
pub fn link<T: 'static>(_linker: &mut Linker<T>) -> wasmtime::Result<()> {
    Ok(())
}
