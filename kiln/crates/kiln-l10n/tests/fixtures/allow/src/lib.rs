// SPDX-License-Identifier: Apache-2.0
// A fixture crate for the literal lint's allow-list tests; never compiled.

mod a;
mod b;
#[path = "renamed.rs"]
mod c;
#[cfg(test)]
mod tests;
