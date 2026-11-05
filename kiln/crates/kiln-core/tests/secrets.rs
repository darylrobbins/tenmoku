// SPDX-License-Identifier: Apache-2.0

use kiln_core::{KeychainStore, MemoryStore, Secret, SecretStore};

const SERVICE: &str = "dev.kiln.test";

#[test]
fn memory_store_gets_sets_and_deletes() {
    let store = MemoryStore::new();
    assert_eq!(store.get(SERVICE, "chat.anthropic").unwrap(), None);
    store.set(SERVICE, "chat.anthropic", "sk-ant-123").unwrap();
    let secret = store.get(SERVICE, "chat.anthropic").unwrap().unwrap();
    assert_eq!(secret.expose(), "sk-ant-123");
    assert_eq!(store.get(SERVICE, "ext.other.key").unwrap(), None);
    assert_eq!(store.get("dev.kiln.other", "chat.anthropic").unwrap(), None);

    store.set(SERVICE, "chat.anthropic", "sk-ant-456").unwrap();
    let secret = store.get(SERVICE, "chat.anthropic").unwrap().unwrap();
    assert_eq!(secret.expose(), "sk-ant-456");

    store.delete(SERVICE, "chat.anthropic").unwrap();
    assert_eq!(store.get(SERVICE, "chat.anthropic").unwrap(), None);
    store.delete(SERVICE, "chat.anthropic").unwrap();
    store.delete(SERVICE, "never.set").unwrap();

    let debug = format!("{secret:?}");
    assert_eq!(debug, "Secret(<redacted>)");
    assert!(!debug.contains("sk-ant"));
    assert!(!format!("{:?}", Some(Secret::new("hunter2"))).contains("hunter2"));
}

/// Runs itself again with the session bus pointed nowhere, as on a headless
/// box with no Secret Service: the store says it's unavailable, and doesn't
/// panic.
#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn keychain_without_secret_service_is_unavailable() {
    use kiln_core::SecretError;

    const CHILD: &str = "KILN_SECRETS_TEST_NO_SECRET_SERVICE";
    if std::env::var_os(CHILD).is_some() {
        match KeychainStore::new() {
            Err(SecretError::Unavailable(_)) => {}
            other => panic!("expected Unavailable, got {other:?}"),
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "keychain_without_secret_service_is_unavailable",
            "--test-threads=1",
        ])
        .env(CHILD, "1")
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            "unix:path=/nonexistent/kiln-bus",
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}

/// The real keychain: run it by hand (`cargo test -p kiln-core --test
/// secrets -- --ignored`). On macOS it may raise a Keychain prompt.
#[test]
#[ignore = "touches the real keychain, which may prompt"]
fn keychain_round_trip_live() {
    let store = KeychainStore::new().unwrap();
    let account = format!("test.round_trip.{}", std::process::id());
    store.delete(SERVICE, &account).unwrap();
    assert_eq!(store.get(SERVICE, &account).unwrap(), None);
    store.set(SERVICE, &account, "kiln secret").unwrap();
    let secret = store.get(SERVICE, &account).unwrap().unwrap();
    assert_eq!(secret.expose(), "kiln secret");
    store.set(SERVICE, &account, "kiln secret 2").unwrap();
    let secret = store.get(SERVICE, &account).unwrap().unwrap();
    assert_eq!(secret.expose(), "kiln secret 2");
    store.delete(SERVICE, &account).unwrap();
    assert_eq!(store.get(SERVICE, &account).unwrap(), None);
    store.delete(SERVICE, &account).unwrap();
}
