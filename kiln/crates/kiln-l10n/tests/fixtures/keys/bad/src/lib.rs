// SPDX-License-Identifier: Apache-2.0
// A fixture crate for the key check; never compiled.

pub fn labels() {
    let _ = tr!("app-own");
    let _ = tr!("lib-private");
    let _ = text!("nowhere-key", name = "x");
}
