//! The project watcher's batches reach language servers that registered for
//! them as created, changed or deleted, minus VS Code's watcher excludes; a
//! rescan sends nothing.

use std::{fs, time::Duration};

use fe_lsp::fake::{Record, Script};
use fe_project::FsChange;
use gpui::TestAppContext;
use lsp_types::Registration;
use lsp_types::RegistrationParams;
use lsp_types::request::RegisterCapability;
use serde_json::{Value, json};

use crate::tests::lsp::Lsp;

const WATCHED: &str = "workspace/didChangeWatchedFiles";

/// A started project whose rust-analyzer fake registered `**/*.rs` and
/// `**/Cargo.toml`.
fn registered(cx: &mut TestAppContext) -> Lsp {
    let mut s = Lsp::with_scripts(cx, Script::default(), Script::default());
    s.set_trust(true);
    s.open("src/main.rs");
    s.fake.wait_initialized();
    let options = json!({"watchers": [
        {"globPattern": "**/*.rs"},
        {"globPattern": "**/Cargo.toml"},
    ]});
    s.fake
        .request::<RegisterCapability>(RegistrationParams {
            registrations: vec![Registration {
                id: "w".into(),
                method: WATCHED.into(),
                register_options: Some(options),
            }],
        })
        .unwrap();
    s
}

/// Feeds the watcher's batch of `names` (relative to the root) through the
/// project panel.
fn paths(s: &mut Lsp, names: &[&str]) {
    let changes = names.iter().map(|n| FsChange::Path(s.path(n))).collect();
    batch(s, changes);
}

/// Feeds `changes` through the project panel, as its watcher does.
fn batch(s: &mut Lsp, changes: Vec<FsChange>) {
    let panel = s.workbench.read_with(&s.cx, |w, _| w.panel().clone());
    s.cx.update(|_, cx| panel.update(cx, |panel, cx| panel.apply_fs_changes(&changes, cx)));
    s.cx.run_until_parked();
}

fn watched(s: &Lsp) -> Vec<Value> {
    (s.fake.records().into_iter())
        .filter_map(|record| match record {
            Record::Notification { method, params } if method == WATCHED => Some(params),
            _ => None,
        })
        .collect()
}

#[gpui::test]
fn watcher_batches_reach_registered_servers(cx: &mut TestAppContext) {
    let mut s = registered(cx);
    fs::write(s.path("src/old.rs"), "").unwrap();
    fs::create_dir_all(s.path(".git/objects/ab")).unwrap();
    fs::write(s.path("Cargo.toml"), "[package]\n").unwrap();

    // A first batch, after which the manager's mark is its arrival.
    paths(&mut s, &["src/old.rs"]);
    s.fake.wait_for(WATCHED, |records| {
        records.iter().any(|r| r.is_notification(WATCHED))
    });
    let before = watched(&s).len();

    // The filesystem's clock may lag the one the mark is read from.
    std::thread::sleep(Duration::from_millis(50));
    fs::write(s.path("src/new.rs"), "").unwrap();
    fs::write(s.path("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
    fs::remove_file(s.path("src/old.rs")).unwrap();
    fs::write(s.path("README.md"), "").unwrap();
    fs::write(s.path(".git/objects/ab/cd"), "").unwrap();
    let created = match fs::metadata(s.path("src/new.rs")).and_then(|m| m.created()) {
        Ok(_) => 1,
        Err(_) => 2,
    };
    paths(
        &mut s,
        &[
            "src/new.rs",
            "Cargo.toml",
            "src/old.rs",
            "README.md",
            ".git/objects/ab/cd",
        ],
    );
    s.fake.wait_for(WATCHED, move |records| {
        records
            .iter()
            .filter(|r| r.is_notification(WATCHED))
            .count()
            > before
    });
    let sent = watched(&s);
    assert_eq!(sent.len(), before + 1);
    let (new, cargo, old) = (
        s.uri("src/new.rs"),
        s.uri("Cargo.toml"),
        s.uri("src/old.rs"),
    );
    assert_eq!(
        sent[before],
        json!({"changes": [
            {"uri": new.as_str(), "type": created},
            {"uri": cargo.as_str(), "type": 2},
            {"uri": old.as_str(), "type": 3},
        ]})
    );
}

#[gpui::test]
fn a_rescan_sends_nothing(cx: &mut TestAppContext) {
    let mut s = registered(cx);
    batch(&mut s, vec![FsChange::Rescan]);
    // A later batch is the first thing the server hears.
    fs::write(s.path("src/other.rs"), "pub fn h() {}\n").unwrap();
    paths(&mut s, &["src/other.rs"]);
    s.fake.wait_for(WATCHED, |records| {
        records.iter().any(|r| r.is_notification(WATCHED))
    });
    let other = s.uri("src/other.rs");
    let sent = watched(&s);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["changes"].as_array().unwrap().len(), 1);
    assert_eq!(sent[0]["changes"][0]["uri"], other.as_str());
}
