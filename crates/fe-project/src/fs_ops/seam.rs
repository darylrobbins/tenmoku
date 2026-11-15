//! Where tests of this crate and of the crates above it (`cfg(test)` or the
//! `test-support` feature) change the disk at a boundary of an operation, as
//! another process of the same user racing it there could: never by timing.

use std::{cell::RefCell, io};

use super::Point;

/// A boundary of an operation where [`at`] runs a test's change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Boundary {
    /// A folder or symlink was just made (a create's folder, a copy's
    /// folder or symlink, a restore's staging folder) and is about to be
    /// recorded as made.
    Made,
    /// A folder just made (a create's or a copy's folder, a restore's
    /// staging folder) was looked up by name, and is about to be opened and
    /// confirmed to be the one made.
    Confirming,
    /// `restore` staged the entry and checked it, and is about to publish
    /// it by name.
    Staged,
    /// `restore` published the entry by name, and is about to check what
    /// landed there.
    Published,
}

impl Boundary {
    fn of(point: Point) -> Option<Self> {
        match point {
            Point::Recording => Some(Self::Made),
            Point::Opening => Some(Self::Confirming),
            Point::Staged => Some(Self::Staged),
            Point::Published => Some(Self::Published),
            _ => None,
        }
    }
}

type Hook = Box<dyn FnOnce() -> io::Result<()>>;

thread_local! {
    // What `reach` runs, once, when this thread reaches its boundary.
    static HOOK: RefCell<Option<(Boundary, Hook)>> = const { RefCell::new(None) };
}

/// Sets `hook` to run the next time this thread reaches `boundary`, in
/// place of any hook set before. It may set another.
pub fn at(boundary: Boundary, hook: impl FnOnce() + 'static) {
    HOOK.set(Some((
        boundary,
        Box::new(|| {
            hook();
            Ok(())
        }),
    )));
}

/// Makes the step after `boundary` fail the next time this thread reaches
/// it, as an open that ran out of descriptors (`EMFILE`), in place of any
/// hook set before.
pub fn fail_at(boundary: Boundary) {
    HOOK.set(Some((
        boundary,
        Box::new(|| Err(io::Error::from_raw_os_error(libc::EMFILE))),
    )));
}

/// Whether the hook last set has run.
pub fn reached() -> bool {
    HOOK.with_borrow(Option::is_none)
}

/// Runs the hook set for `point`'s boundary, if any, which may fail in
/// place of the step that follows.
pub(super) fn reach(point: Point) -> io::Result<()> {
    let Some(boundary) = Boundary::of(point) else {
        return Ok(());
    };
    let hook = HOOK.with_borrow_mut(|slot| slot.take_if(|(at, _)| *at == boundary));
    match hook {
        Some((_, hook)) => hook(),
        None => Ok(()),
    }
}
