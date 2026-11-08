// SPDX-License-Identifier: Apache-2.0

//! A watchdog for a macOS window whose frame requests stop arriving
//! (docs/decisions/h9n3r1vpgs-freeze.md, "Card 2").
//!
//! gpui-pre-macos's `start_display_link` can leave a window unsubscribed
//! from its display link (a stale occlusion state, no screen, or a failed
//! or lost start on the shared per-display link), and nothing retries
//! until the next occlusion change, screen change, `displayLayer:` or
//! activation. The window then stops drawing and taking visible input.
//!
//! The watchdog notices and restarts the link without forking GPUI:
//!
//! - **Heartbeat.** A probe posted with [`Window::on_next_frame`]. Only the
//!   platform frame callback runs next-frame callbacks (`step`,
//!   `displayLayer:` or activation), never a key event's synchronous draw,
//!   so an acknowledged probe means a frame request reached the window.
//! - **Stall.** A probe left unacknowledged for [`STALL`] and across
//!   [`STALL_POLLS`] polls, [`POLL`] apart, while the window is visible or
//!   key. Two polls keep a busy main thread from counting as a stall.
//! - **Nudge.** `[view setNeedsDisplay:YES]`: AppKit calls GPUI's
//!   `displayLayer:`, which draws, presents and subscribes again.
//! - **Recovery.** `displayLayer:` acknowledges the probe pending at the
//!   nudge, so that probe is consumed, not counted. The episode ends after
//!   [`RECOVERY_PROBES`] fresh probes in a row acknowledged with no nudge
//!   in between.
//! - **Log.** One warn record when an episode begins and one when it ends.
//!
//! The state in the begin record is read at the nudge, not at the failed
//! start, so it is context, not attribution.

use std::{
    cell::RefCell,
    rc::{Rc, Weak},
    time::{Duration, Instant},
};

use gpui::{App, Task, Window};

/// How often the watchdog looks at its probe.
pub(crate) const POLL: Duration = Duration::from_millis(250);
/// How long a probe may go unacknowledged before it's a stall.
pub(crate) const STALL: Duration = Duration::from_millis(500);
/// How many polls must see a probe unacknowledged before it's a stall.
pub(crate) const STALL_POLLS: u32 = 2;
/// Nudges in an episode before backing off to one per [`BACKOFF`].
pub(crate) const FAST_NUDGES: u32 = 8;
/// The time between nudges once an episode has backed off.
pub(crate) const BACKOFF: Duration = Duration::from_secs(2);
/// Fresh probes acknowledged in a row, with no nudge, that end an episode.
pub(crate) const RECOVERY_PROBES: u32 = 2;

/// The pure policy: what to do at each poll, given which probes came back.
#[derive(Debug, Default)]
pub(crate) struct Policy {
    next_id: u64,
    /// The probe the watchdog is waiting on.
    probe: Option<Probe>,
    /// The probe the last nudge consumed, until the nudge's frame runs it.
    /// No fresh probe is posted before then, so the nudge's own frame
    /// can't acknowledge one.
    consumed: Option<u64>,
    episode: Option<Episode>,
}

#[derive(Debug)]
struct Probe {
    id: u64,
    posted: Instant,
    /// Polls that have seen it unacknowledged.
    polls: u32,
    acknowledged: bool,
    /// Posted during an episode, after the last nudge's frame ran.
    fresh: bool,
}

#[derive(Debug)]
struct Episode {
    /// When the probe that stalled was posted.
    since: Instant,
    nudges: u32,
    last_nudge: Instant,
    polls_since_nudge: u32,
    /// Fresh probes acknowledged in a row since the last nudge.
    recovered: u32,
}

/// What a poll asks for, in this order: report, nudge, post the probe.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Step {
    pub(crate) begin: Option<Begin>,
    pub(crate) end: Option<End>,
    pub(crate) nudge: bool,
    /// The id of a probe to post with [`Window::on_next_frame`], which
    /// calls [`Policy::acknowledge`] with it.
    pub(crate) probe: Option<u64>,
}

/// An episode beginning: the policy's half of the begin record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Begin {
    pub(crate) unacknowledged: Duration,
    pub(crate) visible: bool,
    pub(crate) key: bool,
}

/// An episode ending in recovery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct End {
    pub(crate) nudges: u32,
    pub(crate) stalled_for: Duration,
}

impl Policy {
    /// The poll at `now`, for a window that is `visible` (GPUI's
    /// visibility) or `key`.
    pub(crate) fn poll(&mut self, now: Instant, visible: bool, key: bool) -> Step {
        let mut step = Step::default();
        // A key window is watched even when GPUI says it's hidden: GPUI's
        // visibility reads the occlusion bit a stale state leaves wrong. A
        // window that is neither has its link stopped by design; its
        // pending probe is forgotten, and acknowledges nothing later. A
        // probe a nudge consumed is kept until its frame runs, however
        // late: until then no fresh probe is posted, so that frame can't
        // pass for recovery once the window is shown again.
        if !visible && !key {
            self.probe = None;
            return step;
        }
        if let Some(episode) = &mut self.episode {
            episode.polls_since_nudge += 1;
        }

        let waiting = match self.probe.take() {
            Some(probe) if probe.acknowledged => {
                if let Some(episode) = &mut self.episode
                    && probe.fresh
                {
                    episode.recovered += 1;
                    if episode.recovered >= RECOVERY_PROBES {
                        step.end = Some(End {
                            nudges: episode.nudges,
                            stalled_for: now.saturating_duration_since(episode.since),
                        });
                        self.episode = None;
                    }
                }
                false
            }
            Some(mut probe) => {
                probe.polls += 1;
                if self.episode.is_none()
                    && probe.polls >= STALL_POLLS
                    && now.saturating_duration_since(probe.posted) >= STALL
                {
                    step.begin = Some(Begin {
                        unacknowledged: now.saturating_duration_since(probe.posted),
                        visible,
                        key,
                    });
                    self.episode = Some(Episode {
                        since: probe.posted,
                        nudges: 0,
                        last_nudge: now,
                        polls_since_nudge: 0,
                        recovered: 0,
                    });
                }
                self.probe = Some(probe);
                true
            }
            // The last nudge's frame hasn't run yet.
            None => self.consumed.is_some(),
        };

        if let Some(episode) = &mut self.episode
            && waiting
            && (step.begin.is_some() || episode.nudge_due(now))
        {
            episode.nudges += 1;
            episode.last_nudge = now;
            episode.polls_since_nudge = 0;
            episode.recovered = 0;
            if let Some(probe) = self.probe.take() {
                self.consumed = Some(probe.id);
            }
            step.nudge = true;
            return step;
        }

        if self.probe.is_none() && self.consumed.is_none() {
            let id = self.next_id;
            self.next_id += 1;
            self.probe = Some(Probe {
                id,
                posted: now,
                polls: 0,
                acknowledged: false,
                fresh: self.episode.is_some(),
            });
            step.probe = Some(id);
        }
        step
    }

    /// A frame ran the probe `id`.
    pub(crate) fn acknowledge(&mut self, id: u64) {
        if let Some(probe) = &mut self.probe
            && probe.id == id
        {
            probe.acknowledged = true;
        }
        if self.consumed == Some(id) {
            self.consumed = None;
        }
    }

    /// The nudges in the current episode, if there is one.
    #[cfg(test)]
    pub(crate) fn nudges(&self) -> Option<u32> {
        self.episode.as_ref().map(|episode| episode.nudges)
    }
}

impl Episode {
    /// Nudges alternate with probe-only polls, so at most one every two
    /// polls, until [`FAST_NUDGES`]; then one per [`BACKOFF`].
    fn nudge_due(&self, now: Instant) -> bool {
        if self.nudges < FAST_NUDGES {
            self.polls_since_nudge >= 2
        } else {
            now.saturating_duration_since(self.last_nudge) >= BACKOFF
        }
    }
}

/// The window's native state, for the begin record. `None` where it
/// couldn't be read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Native {
    /// `occlusionState` has `NSWindowOcclusionStateVisible`.
    pub(crate) occlusion_visible: Option<bool>,
    /// `screen` isn't nil.
    pub(crate) has_screen: Option<bool>,
    /// The screen's `NSScreenNumber`, the display id the link runs on.
    pub(crate) display_id: Option<u32>,
}

/// A record the watchdog reports: logged at warn by [`watch`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Record {
    Begin(Begin, Native),
    End(End),
}

/// Asks the window for a frame: [`appkit::nudge`], or a test's stand-in.
pub(crate) type Nudge = Box<dyn Fn(&mut Window, &mut App)>;

/// What the watchdog does to the window, and with its records.
pub(crate) struct Hooks {
    pub(crate) nudge: Nudge,
    pub(crate) native: Box<dyn Fn(&Window) -> Native>,
    pub(crate) report: Box<dyn Fn(Record)>,
}

/// [`super::watch_frame_requests`]: watches `window` until the returned
/// task is dropped or the window closes. `None` where the window has no
/// AppKit view (the test platform).
pub(crate) fn watch(window: &mut Window, cx: &mut App) -> Option<Task<()>> {
    appkit::view(window)?;
    Some(watch_with(
        window,
        cx,
        Hooks {
            nudge: Box::new(|window, _| appkit::nudge(window)),
            native: Box::new(appkit::native),
            report: Box::new(log_record),
        },
    ))
}

/// [`watch`], with the effects in `hooks`: it probes at once, then polls
/// every [`POLL`] on the main thread.
pub(crate) fn watch_with(window: &mut Window, cx: &mut App, hooks: Hooks) -> Task<()> {
    let policy = Rc::new(RefCell::new(Policy::default()));
    let hooks = Rc::new(hooks);
    poll(&policy, &hooks, window, cx);
    window.spawn(cx, async move |cx| {
        loop {
            cx.background_executor().timer(POLL).await;
            if cx
                .update(|window, cx| poll(&policy, &hooks, window, cx))
                .is_err()
            {
                break;
            }
        }
    })
}

fn poll(policy: &Rc<RefCell<Policy>>, hooks: &Hooks, window: &mut Window, cx: &mut App) {
    let now = cx.background_executor().now();
    let step = policy
        .borrow_mut()
        .poll(now, window.is_visible(), window.is_window_active());
    // The policy isn't borrowed from here: a nudge can run the frame
    // callback, and with it a probe's acknowledgement, at once.
    if let Some(begin) = step.begin {
        (hooks.report)(Record::Begin(begin, (hooks.native)(window)));
    }
    if let Some(end) = step.end {
        (hooks.report)(Record::End(end));
    }
    if step.nudge {
        (hooks.nudge)(window, cx);
    }
    if let Some(id) = step.probe {
        let policy: Weak<RefCell<Policy>> = Rc::downgrade(policy);
        window.on_next_frame(move |_, _| {
            if let Some(policy) = policy.upgrade() {
                policy.borrow_mut().acknowledge(id);
            }
        });
    }
}

fn log_record(record: Record) {
    match record {
        Record::Begin(begin, native) => log::warn!(
            "window frame requests stalled: probe unacknowledged for {} ms (visible {}, key {}, \
             occlusionState Visible {}, screen {}, display id {}); nudging displayLayer:",
            begin.unacknowledged.as_millis(),
            begin.visible,
            begin.key,
            or_unknown(native.occlusion_visible),
            or_unknown(native.has_screen),
            or_unknown(native.display_id),
        ),
        Record::End(end) => log::warn!(
            "window frame requests resumed after {} ms and {} nudge(s)",
            end.stalled_for.as_millis(),
            end.nudges,
        ),
    }
}

fn or_unknown(value: Option<impl ToString>) -> String {
    value.map_or_else(|| "unknown".into(), |value| value.to_string())
}

/// The window's AppKit side, through its raw handle (an `NSView`).
mod appkit {
    use std::{ffi::c_void, ptr::NonNull};

    use gpui::Window;
    use objc2::{class, msg_send, rc::autoreleasepool, runtime::AnyObject};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    use super::Native;

    /// `NSWindowOcclusionStateVisible`.
    const OCCLUSION_VISIBLE: usize = 1 << 1;

    pub(super) fn view(window: &Window) -> Option<NonNull<c_void>> {
        // Window's own `window_handle` is GPUI's handle; this is the raw one.
        match HasWindowHandle::window_handle(window).ok()?.as_raw() {
            RawWindowHandle::AppKit(handle) => Some(handle.ns_view),
            _ => None,
        }
    }

    /// `[view setNeedsDisplay:YES]`, which has AppKit call GPUI's
    /// `displayLayer:`.
    pub(super) fn nudge(window: &Window) {
        let Some(view) = view(window) else { return };
        // SAFETY: the handle is the window's live NSView, on the main
        // thread; setNeedsDisplay: takes a BOOL and returns nothing.
        unsafe {
            let view = view.cast::<AnyObject>().as_ref();
            let () = msg_send![view, setNeedsDisplay: true];
        }
    }

    pub(super) fn native(window: &Window) -> Native {
        let Some(view) = view(window) else {
            return Native::default();
        };
        // SAFETY: the handle is the window's live NSView, on the main
        // thread. Each message is sent to an object checked non-nil, with
        // the argument and return types AppKit declares for it.
        autoreleasepool(|_| unsafe {
            let view = view.cast::<AnyObject>().as_ref();
            let ns_window: *mut AnyObject = msg_send![view, window];
            let Some(ns_window) = ns_window.as_ref() else {
                return Native::default();
            };
            let occlusion: usize = msg_send![ns_window, occlusionState];
            let screen: *mut AnyObject = msg_send![ns_window, screen];
            let screen = screen.as_ref();
            Native {
                occlusion_visible: Some(occlusion & OCCLUSION_VISIBLE != 0),
                has_screen: Some(screen.is_some()),
                display_id: screen.and_then(|screen| screen_number(screen)),
            }
        })
    }

    /// `screen.deviceDescription[@"NSScreenNumber"]`.
    ///
    /// # Safety
    ///
    /// `screen` is an `NSScreen`, on the main thread, in an autorelease pool.
    unsafe fn screen_number(screen: &AnyObject) -> Option<u32> {
        // SAFETY: as the caller promises; each object is checked non-nil.
        unsafe {
            let key: *mut AnyObject =
                msg_send![class!(NSString), stringWithUTF8String: c"NSScreenNumber".as_ptr()];
            let key = key.as_ref()?;
            let description: *mut AnyObject = msg_send![screen, deviceDescription];
            let description = description.as_ref()?;
            let number: *mut AnyObject = msg_send![description, objectForKey: key];
            let number = number.as_ref()?;
            let id: u32 = msg_send![number, unsignedIntValue];
            Some(id)
        }
    }
}
