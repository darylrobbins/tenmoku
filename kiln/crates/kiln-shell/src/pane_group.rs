// SPDX-License-Identifier: LicenseRef-Proprietary

//! A group of panes, split side by side or one above another: where a
//! split, a focus move or a tab goes ([`Direction`], along an [`Axis`]),
//! and where a tab moves to ([`Destination`]).

use gpui::Entity;

/// How an axis lays its members out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// Side by side, left to right.
    Horizontal,
    /// One above another, top to bottom.
    Vertical,
}

/// A way to go from a pane: where a split, focus or move goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl Direction {
    /// The axis this direction runs along.
    pub fn axis(self) -> Axis {
        match self {
            Direction::Left | Direction::Right => Axis::Horizontal,
            Direction::Up | Direction::Down => Axis::Vertical,
        }
    }

    /// Whether it goes towards the axis's end (right or down).
    pub fn forward(self) -> bool {
        matches!(self, Direction::Right | Direction::Down)
    }
}

/// Where a tab moves to.
pub enum Destination<V> {
    /// Into `pane`, at `index` among its tabs, or at the end; in the tab's
    /// own pane, the tab moves there.
    Pane {
        pane: Entity<V>,
        index: Option<usize>,
    },
    /// Into a new pane, split from `pane` in `direction`.
    Split {
        pane: Entity<V>,
        direction: Direction,
    },
}

impl<V> Clone for Destination<V> {
    fn clone(&self) -> Self {
        match self {
            Destination::Pane { pane, index } => Destination::Pane {
                pane: pane.clone(),
                index: *index,
            },
            Destination::Split { pane, direction } => Destination::Split {
                pane: pane.clone(),
                direction: *direction,
            },
        }
    }
}

impl<V: 'static> PartialEq for Destination<V> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Destination::Pane { pane, index },
                Destination::Pane {
                    pane: other,
                    index: at,
                },
            ) => pane == other && index == at,
            (
                Destination::Split { pane, direction },
                Destination::Split {
                    pane: other,
                    direction: way,
                },
            ) => pane == other && direction == way,
            _ => false,
        }
    }
}

impl<V: 'static> std::fmt::Debug for Destination<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Destination::Pane { pane, index } => f
                .debug_struct("Pane")
                .field("pane", &pane.entity_id())
                .field("index", index)
                .finish(),
            Destination::Split { pane, direction } => f
                .debug_struct("Split")
                .field("pane", &pane.entity_id())
                .field("direction", direction)
                .finish(),
        }
    }
}
