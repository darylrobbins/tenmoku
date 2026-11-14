// SPDX-License-Identifier: Apache-2.0

//! Unit tests, one module per area. The fake adapter they drive is
//! `crate::fake`.

mod fake;
mod launcher;
mod protocol;
mod registry;
mod session_fairness;
mod session_harness;
mod session_log;
mod session_requests;
mod session_restart;
mod session_startup;
mod session_terminate;
mod transport;
