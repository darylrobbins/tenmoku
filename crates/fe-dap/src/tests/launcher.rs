// SPDX-License-Identifier: Apache-2.0

//! `LocalLauncher`'s connect loop and startup-output drain (§2), on the
//! injected clock and on in-memory pipes. The launches of real processes
//! (the `fe-dap-fake` binary) are the integration tests in
//! `tests/launcher.rs`, where cargo hands over the binary's path.

use std::io;
use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use crate::clock::ManualClock;
use crate::launcher::{AdapterSpec, CONNECT_RETRY, LaunchCancel};

/// `connect_with` on a thread, on `clock`, with `attempt`.
fn connect_on_thread<F>(
    clock: Arc<ManualClock>,
    attempt: impl FnMut() -> F + Send + 'static,
) -> std::thread::JoinHandle<io::Result<()>>
where
    F: std::future::Future<Output = io::Result<()>> + Send,
{
    std::thread::spawn(move || {
        let spec = AdapterSpec::new("pending", "unused");
        async_io::block_on(crate::launcher::connect_with(
            &*clock,
            &LaunchCancel::new(),
            &spec,
            4711,
            || false,
            attempt,
        ))
    })
}

#[test]
fn a_handshake_that_never_finishes_times_out_at_5_s() {
    let clock = Arc::new(ManualClock::new());
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = attempts.clone();
    let connect = connect_on_thread(clock.clone(), move || {
        counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        futures::future::pending::<io::Result<()>>()
    });
    clock.wait_for_sleep(crate::launcher::CONNECT_TIMEOUT);
    clock.advance(Duration::from_millis(4900));
    std::thread::sleep(Duration::from_millis(100));
    assert!(!connect.is_finished());
    clock.advance(Duration::from_millis(100));
    let err = connect.join().unwrap().unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::TimedOut, "{err}");
    assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn no_attempt_starts_once_5_s_have_passed() {
    let clock = Arc::new(ManualClock::new());
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = attempts.clone();
    // The first attempt is refused; any later one would connect.
    let connect = connect_on_thread(clock.clone(), move || {
        let first = counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0;
        async move {
            if first {
                Err(io::ErrorKind::ConnectionRefused.into())
            } else {
                Ok(())
            }
        }
    });
    clock.wait_for_sleep(CONNECT_RETRY);
    // The clock passes 5 s during the retry's sleep.
    clock.advance(Duration::from_secs(6));
    let err = connect.join().unwrap().unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::TimedOut, "{err}");
    assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn a_connection_made_as_the_deadline_passes_is_refused() {
    use std::future::Future as _;
    use std::task::{Context, Poll};

    let spec = AdapterSpec::new("late", "unused");
    let waker = futures::task::noop_waker();
    let mut cx = Context::from_waker(&waker);
    // One attempt, which completes when `connect` is sent to.
    let attempt_once = |receiver: futures::channel::oneshot::Receiver<()>| {
        let mut receiver = Some(receiver);
        move || {
            let receiver = receiver.take().expect("a second attempt");
            async move { receiver.await.map_err(|_| io::Error::other("dropped")) }
        }
    };

    // Pending, then the connection and the deadline in the same poll.
    let clock = ManualClock::new();
    let cancel = LaunchCancel::new();
    let (connect, connected) = futures::channel::oneshot::channel();
    let mut loop_ = pin!(crate::launcher::connect_with(
        &clock,
        &cancel,
        &spec,
        4711,
        || false,
        attempt_once(connected),
    ));
    assert!(loop_.as_mut().poll(&mut cx).is_pending());
    clock.advance(Duration::from_secs(6));
    connect.send(()).unwrap();
    match loop_.as_mut().poll(&mut cx) {
        Poll::Ready(Err(err)) => assert_eq!(err.kind(), io::ErrorKind::TimedOut, "{err}"),
        other => panic!("not timed out: {other:?}"),
    }

    // The same with no time passed connects.
    let clock = ManualClock::new();
    let (connect, connected) = futures::channel::oneshot::channel();
    let mut loop_ = pin!(crate::launcher::connect_with(
        &clock,
        &cancel,
        &spec,
        4711,
        || false,
        attempt_once(connected),
    ));
    assert!(loop_.as_mut().poll(&mut cx).is_pending());
    connect.send(()).unwrap();
    assert!(matches!(loop_.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));

    // And with a cancel in the same poll, it is cancelled.
    let clock = ManualClock::new();
    let cancel = LaunchCancel::new();
    let (connect, connected) = futures::channel::oneshot::channel();
    let mut loop_ = pin!(crate::launcher::connect_with(
        &clock,
        &cancel,
        &spec,
        4711,
        || false,
        attempt_once(connected),
    ));
    assert!(loop_.as_mut().poll(&mut cx).is_pending());
    cancel.cancel();
    connect.send(()).unwrap();
    match loop_.as_mut().poll(&mut cx) {
        Poll::Ready(Err(err)) => assert_eq!(err.kind(), io::ErrorKind::Interrupted, "{err}"),
        other => panic!("not cancelled: {other:?}"),
    }
}

/// A pipe that is always readable, forever.
struct AlwaysReady;

impl futures::AsyncRead for AlwaysReady {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<io::Result<usize>> {
        buf.fill(b'x');
        std::task::Poll::Ready(Ok(buf.len()))
    }
}

/// A connection loop that never connects, run on a thread while draining
/// two always-readable pipes; its result and what was kept of each.
fn draining_forever(
    clock: Arc<ManualClock>,
    cancel: LaunchCancel,
) -> std::sync::mpsc::Receiver<(io::Result<()>, usize, usize)> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let spec = AdapterSpec::new("chatty", "unused");
        let connect = crate::launcher::connect_with(
            &*clock,
            &cancel,
            &spec,
            4711,
            || false,
            futures::future::pending::<io::Result<()>>,
        );
        let (result, out, err) = async_io::block_on(crate::launcher::while_draining(
            connect,
            Some(&mut AlwaysReady),
            Some(&mut AlwaysReady),
            "chatty",
        ));
        let _ = sender.send((result, out.len(), err.len()));
    });
    receiver
}

#[test]
fn draining_an_always_readable_pipe_lets_the_deadline_and_cancel_through() {
    use crate::launcher::{CONNECT_TIMEOUT, STARTUP_OUTPUT_LIMIT};

    let limit = Duration::from_secs(10);
    // A cancel gets through, and both pipes were drained, within the limit.
    let cancel = LaunchCancel::new();
    let done = draining_forever(Arc::new(ManualClock::new()), cancel.clone());
    std::thread::sleep(Duration::from_millis(50));
    cancel.cancel();
    let (result, out, err) = done
        .recv_timeout(limit)
        .expect("the cancel never got through");
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
    assert!(out > 0 && out <= STARTUP_OUTPUT_LIMIT, "{out}");
    assert!(err > 0 && err <= STARTUP_OUTPUT_LIMIT, "{err}");

    // So does the deadline.
    let clock = Arc::new(ManualClock::new());
    let done = draining_forever(clock.clone(), LaunchCancel::new());
    clock.wait_for_sleep(CONNECT_TIMEOUT);
    clock.advance(CONNECT_TIMEOUT);
    let (result, _, _) = done
        .recv_timeout(limit)
        .expect("the deadline never got through");
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
}
