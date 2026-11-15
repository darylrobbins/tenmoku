//! The operation in progress on the status bar (T1586): "Indexing" with its
//! 72 px bar half full, a `3/12` count and `+2` for the other two (the
//! surface `operations`), and the list a click opens above it, one row
//! each, open (`operations_list`).
//!
//! The steps compile on every platform (`main.rs` includes this file on
//! Linux too, without the surfaces), so they are checked there; the
//! surfaces are captured on the Mac (Metal only) with the batch, and until
//! then they are unrecorded (`docs/decisions/25w38k498b-ui-polish-audit.md`).
//!
//! The status bar is the window's bottom 24 px, y = 576–600 at the audit's
//! 1000 × 600; the item follows the bar's 8 px of padding, its spinner at
//! x = 16–28.

use fe_workbench::Workbench;
use gpui::{Context, Window};

/// A step, as `audit_scenes::Step` is.
type Step = Box<dyn FnOnce(&mut Workbench, &mut Window, &mut Context<Workbench>)>;

/// Three operations, the first with a fraction and a count in its message.
pub(crate) fn operations() -> Step {
    Box::new(|workbench, _, cx| {
        (workbench.operations().clone()).update(cx, |operations, cx| {
            let indexing = operations.begin("Indexing", cx);
            operations.update(&indexing, Some(0.5), Some("3/12 crates".into()), cx);
            operations.begin("Checking", cx);
            operations.begin("Building", cx);
        });
    })
}

/// Opens the list above the item.
pub(crate) fn open_list() -> Step {
    Box::new(|workbench, window, cx| workbench.show_operations(window, cx))
}

#[cfg(target_os = "macos")]
mod surfaces {
    use crate::{
        audit_scenes::{DEFAULT, Surface, idle},
        token_probe::{Probe, probe},
    };

    /// The item with the list closed.
    pub(crate) const CLOSED: Surface = Surface {
        name: "operations",
        steps: |_| vec![super::operations(), idle()],
        probes: CLOSED_PROBES,
        ..DEFAULT
    };

    /// The item with the list open.
    pub(crate) const LIST: Surface = Surface {
        name: "operations_list",
        steps: |_| vec![super::operations(), super::open_list(), idle()],
        probes: LIST_PROBES,
        ..DEFAULT
    };

    const CLOSED_PROBES: &[Probe] = &[
        // The status bar, right of the item.
        probe(900., 588., "surface.chrome"),
        // The editor, above the bar.
        probe(640., 400., "surface.background"),
    ];

    const LIST_PROBES: &[Probe] = &[
        // The status bar, right of the item.
        probe(900., 588., "surface.chrome"),
        // The list's card, 3 px inside its left edge, above the item (360
        // wide, three 28 px rows and 4 px padding top and bottom, 4 px over
        // the bar): its raised surface.
        probe(11., 520., "surface.raised"),
        // The editor, right of the list.
        probe(640., 400., "surface.background"),
    ];
}

#[cfg(target_os = "macos")]
pub(crate) use surfaces::{CLOSED, LIST};
