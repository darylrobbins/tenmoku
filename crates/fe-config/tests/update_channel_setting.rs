//! `update.channel`: stable by default, user-only.

use fe_config::settings::{PROJECT_UPDATE_CHANNEL, ReleaseChannel};
use fe_config::{Settings, SettingsStore, Severity};

fn write(dir: &tempfile::TempDir, name: &str, contents: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, contents).unwrap();
    path
}

#[test]
fn channel_defaults_to_stable() {
    assert_eq!(Settings::default().update.channel, ReleaseChannel::Stable);
    let dir = tempfile::tempdir().unwrap();
    let store = SettingsStore::load(Some(write(&dir, "u.toml", "[update]\n")), None);
    assert_eq!(store.diagnostics(), []);
    assert_eq!(store.settings().update.channel, ReleaseChannel::Stable);
}

#[test]
fn channel_preview_parses() {
    let dir = tempfile::tempdir().unwrap();
    let user = write(&dir, "u.toml", "[update]\nchannel = \"preview\"\n");
    let store = SettingsStore::load(Some(user), None);
    assert_eq!(store.diagnostics(), []);
    assert_eq!(store.settings().update.channel, ReleaseChannel::Preview);
    let user = write(&dir, "s.toml", "[update]\nchannel = \"stable\"\n");
    let store = SettingsStore::load(Some(user), None);
    assert_eq!(store.settings().update.channel, ReleaseChannel::Stable);
}

#[test]
fn channel_unknown_value_is_a_diagnostic_and_stays_stable() {
    let dir = tempfile::tempdir().unwrap();
    let user = write(&dir, "u.toml", "[update]\nchannel = \"nightly\"\n");
    let store = SettingsStore::load(Some(user), None);
    assert!(!store.diagnostics().is_empty());
    assert_eq!(store.settings().update.channel, ReleaseChannel::Stable);
}

#[test]
fn a_project_file_cannot_set_the_channel() {
    let dir = tempfile::tempdir().unwrap();
    let project = write(&dir, "p.toml", "[update]\nchannel = \"preview\"\n");
    let store = SettingsStore::load(None, Some(project));
    assert_eq!(store.settings().update.channel, ReleaseChannel::Stable);
    assert!(store.diagnostics().iter().any(|d| {
        d.severity == Severity::Warning
            && d.key.as_deref() == Some("update.channel")
            && d.message == PROJECT_UPDATE_CHANNEL
    }));
    let user = write(&dir, "u.toml", "[update]\nchannel = \"preview\"\n");
    let project = write(&dir, "p2.toml", "[update]\nchannel = \"stable\"\n");
    let store = SettingsStore::load(Some(user), Some(project));
    assert_eq!(store.settings().update.channel, ReleaseChannel::Preview);
}
