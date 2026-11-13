//! Dynamic registration (§3.6): only `workspace/didChangeConfiguration`,
//! `workspace/didChangeWatchedFiles` and `textDocument/onTypeFormatting`
//! are accepted (the last in on_type_formatting_tests.rs), and the
//! configuration notification never depends on its registration. A
//! spec's static watch is a registration of its own (static_watch.rs), and
//! each notification sent is reported (index_events.rs).

mod globs;
mod index_events;
mod static_watch;
mod watched_files;

use std::path::Path;
use std::sync::Arc;

use fe_buffer::Buffer;
use fe_config::settings::{LspServerSettings, LspSettings};
use lsp_types::request::{RegisterCapability, UnregisterCapability};
use lsp_types::{Registration, RegistrationParams, Unregistration, UnregistrationParams};
use serde_json::{Value, json};

use super::{ROOT, manager};
use crate::fake::{FakeError, FakeServer, Record, Script};
use crate::{LspManager, ManualClock, ServerId, ServerStatus};

const CONFIGURATION: &str = "workspace/didChangeConfiguration";
const WATCHED: &str = "workspace/didChangeWatchedFiles";
const RANGE_FORMATTING: &str = "textDocument/rangeFormatting";

fn register(fake: &FakeServer, registrations: &[(&str, &str)]) -> Result<Value, FakeError> {
    fake.request::<RegisterCapability>(RegistrationParams {
        registrations: registrations
            .iter()
            .map(|&(id, method)| Registration {
                id: id.into(),
                method: method.into(),
                register_options: None,
            })
            .collect(),
    })
}

fn unregister(fake: &FakeServer, id: &str) -> Result<Value, FakeError> {
    fake.request::<UnregisterCapability>(UnregistrationParams {
        unregisterations: vec![Unregistration {
            id: id.into(),
            method: CONFIGURATION.into(),
        }],
    })
}

/// Changes rust-analyzer's settings, and waits for the fake to be sent
/// them.
struct Settings {
    settings: LspSettings,
    generation: u64,
}

impl Settings {
    fn change(&mut self, fake: &FakeServer, manager: &mut LspManager) {
        self.generation += 1;
        let settings = json!({ "generation": self.generation });
        self.settings.servers.insert(
            "rust-analyzer".into(),
            LspServerSettings {
                settings: settings.as_object().cloned(),
                ..LspServerSettings::default()
            },
        );
        manager.update_settings(&self.settings);
        fake.wait_for(&format!("settings {}", self.generation), |records| {
            records.iter().any(|record| {
                matches!(record, Record::Notification { method, params }
                    if method == CONFIGURATION && params["settings"] == settings)
            })
        });
    }
}

#[test]
fn dynamic_registration_policy() {
    let fake = FakeServer::new(Script::default());
    let (mut manager, events) = manager(fake.launcher(), Some(Arc::new(ManualClock::new())));
    let mut settings = Settings {
        settings: LspSettings::default(),
        generation: 0,
    };
    let mut buffer = Buffer::new("fn main() {}\n");
    let path = format!("{ROOT}/main.rs");
    let server: ServerId = manager
        .open(
            Some(Path::new(ROOT)),
            buffer.id(),
            buffer.text().clone(),
            buffer.version(),
            buffer.subscribe(),
            Path::new(&path),
            "rust",
        )
        .unwrap()
        .unwrap();
    assert_eq!(events.status(server), ServerStatus::Starting);
    fake.wait_initialized();

    // With no registration at all, a settings change is sent.
    settings.change(&fake, &mut manager);

    // Two registrations are accepted, and recorded.
    assert_eq!(
        register(&fake, &[("a", CONFIGURATION), ("b", CONFIGURATION)]),
        Ok(Value::Null)
    );
    assert_eq!(manager.registrations(server), ["a", "b"]);
    settings.change(&fake, &mut manager);
    assert_eq!(unregister(&fake, "a"), Ok(Value::Null));
    assert_eq!(manager.registrations(server), ["b"]);
    settings.change(&fake, &mut manager);
    // An unknown id: a success that changes nothing.
    assert_eq!(unregister(&fake, "unknown"), Ok(Value::Null));
    assert_eq!(manager.registrations(server), ["b"]);
    assert_eq!(unregister(&fake, "b"), Ok(Value::Null));
    assert!(manager.registrations(server).is_empty());
    settings.change(&fake, &mut manager);

    // An unsupported method, alone or with a supported one, is refused as
    // a whole, naming it, and nothing is recorded.
    for registrations in [
        vec![("w", RANGE_FORMATTING)],
        vec![("c", CONFIGURATION), ("w", RANGE_FORMATTING)],
    ] {
        let refused = register(&fake, &registrations).unwrap_err();
        assert_eq!(refused.code, Some(-32803), "{refused:?}");
        assert!(refused.message.contains(RANGE_FORMATTING), "{refused:?}");
        assert!(!refused.message.contains(CONFIGURATION), "{refused:?}");
        assert!(manager.registrations(server).is_empty());
    }
}
