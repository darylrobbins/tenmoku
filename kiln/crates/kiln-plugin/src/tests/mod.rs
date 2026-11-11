// SPDX-License-Identifier: Apache-2.0

//! The crate's unit tests, and the seams the code under test calls. The pre-launch and budget
//! tests run scripts, `#!/bin/sh` on Unix and `.cmd` on Windows, written by the helper the
//! integration tests share.

use std::sync::{PoisonError, RwLock, RwLockReadGuard};

pub(crate) mod hash_probe;
pub(crate) mod seams;

#[path = "../../tests/support/script.rs"]
mod support;

mod prelaunch;
mod program_budget;

/// Held by every test that resolves or checks a program, against one that changes what makes a
/// file a program while they run: on Windows that is `PATHEXT`, which is the process's.
static PROGRAMS: RwLock<()> = RwLock::new(());

/// For a test that resolves or checks programs, beside others that do.
pub(crate) fn programs() -> RwLockReadGuard<'static, ()> {
    PROGRAMS.read().unwrap_or_else(PoisonError::into_inner)
}

/// For a test that changes what makes a file a program, while no other test resolves or checks
/// one.
#[cfg(windows)]
pub(crate) fn programs_alone() -> std::sync::RwLockWriteGuard<'static, ()> {
    PROGRAMS.write().unwrap_or_else(PoisonError::into_inner)
}
