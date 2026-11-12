//! `FakeLaunchers`: one manager, two fake servers.

use std::path::Path;
use std::sync::Arc;

use super::{ROOT, manager};
use crate::fake::{FakeLaunchers, FakeServer, Script};
use crate::{ServerSpec, ServerStatus};

fn two() -> (FakeServer, FakeServer, Arc<FakeLaunchers>) {
    let (css, vars) = (
        FakeServer::new(Script::default()),
        FakeServer::new(Script::default()),
    );
    let launchers = Arc::new(FakeLaunchers::new([
        ("css", css.launcher()),
        ("vars", vars.launcher()),
    ]));
    (css, vars, launchers)
}

#[test]
fn routes_each_spec_name_to_its_own_launcher() {
    let (css, vars, launchers) = two();
    let (mut manager, events) = manager(launchers, None);
    let id = manager
        .start(ServerSpec::new("vars", "fake-server"), Path::new(ROOT))
        .unwrap();
    assert_eq!(events.status(id), ServerStatus::Starting);
    vars.wait_initialized();
    assert_eq!(vars.launches(), 1);
    assert_eq!(css.launches(), 0);
}

#[test]
fn each_server_keeps_its_own_records() {
    let (css, vars, launchers) = two();
    let (mut manager, events) = manager(launchers, None);
    for name in ["css", "vars"] {
        let id = manager
            .start(ServerSpec::new(name, "fake-server"), Path::new(ROOT))
            .unwrap();
        assert_eq!(events.status(id), ServerStatus::Starting);
    }
    css.wait_initialized();
    vars.wait_initialized();
    for fake in [&css, &vars] {
        let methods: Vec<_> = fake
            .records()
            .iter()
            .map(|r| r.method().to_owned())
            .collect();
        assert!(methods.iter().any(|m| m == "initialize"));
        assert_eq!(fake.launches(), 1);
    }
}
