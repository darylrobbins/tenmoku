// SPDX-License-Identifier: Apache-2.0

//! The session's wait is fair: an adapter whose output, stderr and stdout
//! never stop being readable still lets requests go out, timeouts expire,
//! Stop take effect and the adapter's exit be seen.

use std::sync::atomic::Ordering;

use serde_json::json;

use super::session_harness::{
    Endless, TestProcess, advance_past, frame, initialize_response, next_command, options, start_on,
};
use crate::fake::pipe;
use crate::launcher::AdapterIo;
use crate::protocol::{EvaluateArguments, EventMessage, Message, request};
use crate::session::{
    EXIT_DRAIN, EXIT_GRACE, EndReason, REQUEST_TIMEOUT, RequestError, SessionState,
    TERMINATED_GRACE,
};
use crate::transport::FrameReader;

#[test]
fn a_flooding_adapter_starves_nothing() {
    let output = Message::Event(EventMessage {
        seq: 2,
        event: "output".into(),
        body: Some(json!({ "category": "stdout", "output": "spam\n" })),
        ..Default::default()
    });
    let (sent, input) = pipe();
    let (process, killed) = TestProcess::new();
    let io = AdapterIo {
        input: Box::pin(input),
        output: Box::pin(Endless::new(
            frame(&initialize_response(
                json!({ "supportsTerminateRequest": true }),
            )),
            frame(&output),
        )),
        stderr: Some(Box::pin(Endless::new(Vec::new(), b"noise\n".to_vec()))),
        stdout: Some(Box::pin(Endless::new(Vec::new(), b"more noise\n".to_vec()))),
        port: None,
        process,
    };
    let mut harness = start_on(io, options());
    let mut sent = FrameReader::new(sent);

    // Requests still go out.
    assert_eq!(next_command(&mut sent), "initialize");
    assert_eq!(next_command(&mut sent), "launch");
    let pending = harness
        .session
        .request::<request::Evaluate>(&EvaluateArguments {
            expression: "x".into(),
            context: Some("watch".into()),
            ..Default::default()
        });
    assert_eq!(next_command(&mut sent), "evaluate");

    // Timeouts still expire.
    advance_past(&harness.clock, REQUEST_TIMEOUT);
    assert!(matches!(
        async_io::block_on(pending),
        Err(RequestError::Timeout { .. })
    ));

    // Stop still takes effect, through to the kill and the exit.
    harness.session.stop();
    assert_eq!(next_command(&mut sent), "terminate");
    advance_past(&harness.clock, TERMINATED_GRACE);
    assert_eq!(next_command(&mut sent), "disconnect");
    advance_past(&harness.clock, REQUEST_TIMEOUT);
    advance_past(&harness.clock, EXIT_GRACE);
    // Its streams never end: what's left is read for a while, then not.
    advance_past(&harness.clock, EXIT_DRAIN);
    let ended = harness.wait_ended();
    assert_eq!(ended.reason, EndReason::Stopped);
    assert!(killed.load(Ordering::SeqCst));
    assert!(
        harness.states().contains(&SessionState::Terminating),
        "{:?}",
        harness.states()
    );
}
