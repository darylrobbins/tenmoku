use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

/// How many recent frames [`FrameTimings`] keeps: two seconds at 120 fps.
const KEPT_FRAMES: usize = 240;

/// The editor element's cost in one frame.
#[derive(Copy, Clone, Debug)]
pub struct FrameTiming {
    /// When prepaint started. The window may prepaint other elements
    /// between this element's prepaint and paint.
    pub prepaint_started: Instant,
    /// Layout: scrolling, highlighting and shaping the visible rows.
    pub prepaint: Duration,
    pub paint: Duration,
    /// When painting finished.
    pub painted_at: Instant,
}

/// Timings of the editor's most recent frames. See
/// [`EditorView::frame_timings`](crate::EditorView::frame_timings).
#[derive(Default)]
pub struct FrameTimings {
    frames: VecDeque<FrameTiming>,
}

impl FrameTimings {
    pub(crate) fn record(&mut self, frame: FrameTiming) {
        if self.frames.len() == KEPT_FRAMES {
            self.frames.pop_front();
        }
        self.frames.push_back(frame);
    }

    /// Oldest first.
    pub fn frames(&self) -> impl ExactSizeIterator<Item = &FrameTiming> {
        self.frames.iter()
    }

    pub fn clear(&mut self) {
        self.frames.clear();
    }
}
