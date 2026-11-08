// SPDX-License-Identifier: Apache-2.0

//! The crate's unit tests, and the seams the code under test calls. The pre-launch and budget
//! tests run shell scripts made executable by mode, so they are Unix's alone; the seams are
//! built everywhere, so the code that calls them is the same on every OS.

// Their only readers are the Unix-only tests below.
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) mod hash_probe;
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) mod seams;

// Their programs are shell scripts made executable by mode.
#[cfg(unix)]
mod prelaunch;
#[cfg(unix)]
mod program_budget;
