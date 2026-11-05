// SPDX-License-Identifier: Apache-2.0

//! A `fetch` counts against the call's program share, not its 100 ms deadline.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kiln_command::capability::{Capability, Grants};
use kiln_plugin::bindings::kiln::extension::net::{Host, Method, Request};
use kiln_plugin::bindings::{Error, HostState};
use kiln_plugin::limits::Limits;
use kiln_plugin::net::{NetBackend, Outgoing, Received, State};

struct Slow {
    delay: Duration,
    sent: Arc<Mutex<Vec<Outgoing>>>,
}

impl NetBackend for Slow {
    fn send(&mut self, request: &Outgoing) -> Result<Received, Error> {
        self.sent.lock().unwrap().push(request.clone());
        std::thread::sleep(self.delay);
        Ok(Received {
            status: 200,
            headers: Vec::new(),
            body: Vec::new(),
            peer: request.host.clone(),
        })
    }
}

fn host(delay: Duration) -> (HostState, Arc<Mutex<Vec<Outgoing>>>) {
    let grants: Grants = [Capability::parse("net:example.com").unwrap()]
        .into_iter()
        .collect();
    let sent = Arc::new(Mutex::new(Vec::new()));
    let fake = Slow {
        delay,
        sent: Arc::clone(&sent),
    };
    let mut host = HostState::new(Box::new(|_, _| {}));
    host.net = State::new(&grants, &Limits::default(), Box::new(fake));
    (host, sent)
}

fn request(timeout_ms: Option<u32>) -> Request {
    Request {
        method: Method::Get,
        url: "https://example.com/".into(),
        headers: Vec::new(),
        body: None,
        timeout_ms,
    }
}

#[test]
fn a_fetch_slower_than_the_call_deadline_is_charged_to_the_program_share_not_the_call() {
    let (mut host, _) = host(Duration::from_millis(300));
    let set = Instant::now() + Duration::from_millis(100);
    host.deadline = Some(set);
    host.program_runs.arm(Duration::from_secs(10));
    let reply = host.fetch(request(None));
    assert!(reply.is_ok());
    assert!(host.program_runs.spent() >= Duration::from_millis(300));
    assert!(host.deadline.unwrap() >= set + Duration::from_millis(300));
}

#[test]
fn a_fetch_is_capped_by_what_is_left_of_the_program_share() {
    let (mut host, sent) = host(Duration::ZERO);
    host.program_runs.arm(Duration::from_millis(50));
    assert!(host.fetch(request(Some(5000))).is_ok());
    assert!(sent.lock().unwrap()[0].timeout <= Duration::from_millis(50));

    let (mut host, sent) = host_unarmed();
    let set = Instant::now() + Duration::from_millis(100);
    host.deadline = Some(set);
    assert!(host.fetch(request(Some(5000))).is_ok());
    assert_eq!(host.program_runs.spent(), Duration::ZERO);
    assert_eq!(host.deadline, Some(set));
    let timeout = sent.lock().unwrap()[0].timeout;
    assert!(timeout > Duration::from_millis(4000) && timeout <= Duration::from_millis(5000));
}

fn host_unarmed() -> (HostState, Arc<Mutex<Vec<Outgoing>>>) {
    host(Duration::ZERO)
}
