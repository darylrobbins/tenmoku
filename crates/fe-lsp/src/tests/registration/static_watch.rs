//! Root-scoped static watches (docs/decisions/q5j03ax2af-lsp-add-on-servers.md
//! §2.4, §7 "Watches"): a spec's `watch_files` and `watch_excludes`, or the
//! lists its `_from` pointers find in its settings, are a permanent
//! watched-files registration of each session, matching only paths under
//! that session's root.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use fe_buffer::Buffer;
use fe_config::settings::{LspServerSettings, LspSettings};
use lsp_types::{FileChangeType, Url};
use serde_json::{Value, json};

use super::WATCHED;
use crate::fake::{FakeServer, Record, Script, fake_path};
use crate::tests::{Events, ROOT, manager};
use crate::{LspEvent, LspManager, ManualClock, Notice, ServerId, ServerSpec, ServerStatus};

const MARKER: &str = "workspace/didCreateFiles";

/// css-variables-language-server's shape: stylesheets, minus build and
/// package folders.
fn vars_spec() -> ServerSpec {
    ServerSpec {
        watch_files: vec!["**/*.css".into(), "**/*.scss".into()],
        watch_excludes: vec!["**/node_modules".into(), "**/dist".into()],
        ..ServerSpec::new("vars", "fake-vars")
    }
}

fn uri(path: &Path) -> String {
    Url::from_file_path(path).unwrap().to_string()
}

/// The watched-files notifications launch `launch` has been sent, once a
/// marker sent to every session after them has reached it.
fn sent_to(fake: &FakeServer, manager: &LspManager, launches: &[usize]) -> Vec<Vec<Value>> {
    let markers = |records: &[Record]| {
        records
            .iter()
            .filter(|record| record.is_notification(MARKER))
            .count()
    };
    let before: Vec<usize> = (launches.iter())
        .map(|&launch| markers(&fake.records_of(launch)))
        .collect();
    manager.notify_sessions(MARKER, |_, _| Some(json!({ "files": [] })));
    (launches.iter().zip(before))
        .map(|(&launch, before)| {
            fake.wait_for_at(launch, "the marker", |records| markers(records) > before);
            (fake.records_of(launch).into_iter())
                .filter_map(|record| match record {
                    Record::Notification { method, params } if method == WATCHED => Some(params),
                    _ => None,
                })
                .collect()
        })
        .collect()
}

/// `vars_spec` running at two roots, `a` and `b`, launches 1 and 2.
struct TwoRoots {
    fake: FakeServer,
    manager: LspManager,
    _events: Events,
    first: ServerId,
    a: PathBuf,
    b: PathBuf,
}

impl TwoRoots {
    fn new() -> Self {
        let fake = FakeServer::new(Script::default());
        let (mut manager, events) = manager(fake.launcher(), Some(Arc::new(ManualClock::new())));
        let (a, b) = (fake_path("a"), fake_path("b"));
        let first: ServerId = manager.start(vars_spec(), &a).unwrap();
        assert_eq!(events.status(first), ServerStatus::Starting);
        fake.wait_initialized_at(1);
        let second = manager.start(vars_spec(), &b).unwrap();
        assert_eq!(events.status(second), ServerStatus::Starting);
        fake.wait_initialized_at(2);
        assert_eq!(
            (fake.launched().into_iter())
                .map(|(_, root)| root)
                .collect::<Vec<_>>(),
            [a.clone(), b.clone()]
        );
        Self {
            fake,
            manager,
            _events: events,
            first,
            a,
            b,
        }
    }

    /// What each session has been sent: (root `a`'s, root `b`'s).
    fn sent(&self) -> (Vec<Value>, Vec<Value>) {
        let mut sent = sent_to(&self.fake, &self.manager, &[1, 2]).into_iter();
        (sent.next().unwrap(), sent.next().unwrap())
    }
}

#[test]
fn change_under_root_b_reaches_only_root_b_session() {
    let s = TwoRoots::new();
    // Both roots hold `vars.css`, each defining the same variables its own
    // way: B's change must never reach A's index, which keeps one value
    // per name.
    s.manager
        .files_changed(vec![(s.b.join("vars.css"), FileChangeType::CHANGED)]);
    let (to_a, to_b) = s.sent();
    assert_eq!(to_a, [] as [Value; 0]);
    assert_eq!(
        to_b,
        [json!({ "changes": [{ "uri": uri(&s.b.join("vars.css")), "type": 2 }] })]
    );

    // In one batch, each session gets its own root's paths only, in order.
    // A sibling folder whose name starts with B's matches neither.
    s.manager.files_changed(vec![
        (s.a.join("theme/vars.scss"), FileChangeType::CREATED),
        (s.b.join("vars.css"), FileChangeType::DELETED),
        (fake_path("b2/vars.css"), FileChangeType::CHANGED),
        (s.a.join("vars.css"), FileChangeType::CHANGED),
    ]);
    let (to_a, to_b) = s.sent();
    assert_eq!(
        to_a,
        [json!({ "changes": [
            { "uri": uri(&s.a.join("theme/vars.scss")), "type": 1 },
            { "uri": uri(&s.a.join("vars.css")), "type": 2 },
        ] })]
    );
    assert_eq!(
        to_b[1..],
        [json!({ "changes": [{ "uri": uri(&s.b.join("vars.css")), "type": 3 }] })]
    );

    // The static watch is no registration the server made: none is listed,
    // and unregistering never removes it.
    assert!(s.manager.registrations(s.first).is_empty());
}

#[test]
fn blacklisted_folder_change_reaches_neither() {
    let s = TwoRoots::new();
    s.manager.files_changed(vec![
        (
            s.a.join("node_modules/pkg/vars.css"),
            FileChangeType::CHANGED,
        ),
        (s.b.join("dist/vars.css"), FileChangeType::CREATED),
        (s.b.join("src/dist/deep/vars.scss"), FileChangeType::CHANGED),
        (s.a.join("node_modules"), FileChangeType::DELETED),
        // Not a stylesheet.
        (s.b.join("src/app.ts"), FileChangeType::CHANGED),
        // Kept: `dist` names a file here, not a folder above it, and
        // `node_modules.css` isn't `node_modules`.
        (s.b.join("src/ok.css"), FileChangeType::CHANGED),
        (s.a.join("node_modules.css"), FileChangeType::CHANGED),
    ]);
    let (to_a, to_b) = s.sent();
    assert_eq!(
        to_a,
        [json!({ "changes": [{ "uri": uri(&s.a.join("node_modules.css")), "type": 2 }] })]
    );
    assert_eq!(
        to_b,
        [json!({ "changes": [{ "uri": uri(&s.b.join("src/ok.css")), "type": 2 }] })]
    );

    // The globs see `/`-joined parts. On Windows that's what leaves out
    // `src\dist\deep\vars.scss` above; here a backslash is part of a
    // name, so this is one file in B's root, not under `dist`.
    #[cfg(unix)]
    {
        let literal = s.b.join("src\\dist\\x.scss");
        s.manager
            .files_changed(vec![(literal.clone(), FileChangeType::CHANGED)]);
        let (to_a, to_b) = s.sent();
        assert_eq!(to_a.len(), 1);
        assert_eq!(
            to_b[1..],
            [json!({ "changes": [{ "uri": uri(&literal), "type": 2 }] })]
        );
    }

    // Only excluded paths: neither session is sent anything.
    s.manager.files_changed(vec![
        (s.a.join("node_modules/x.css"), FileChangeType::CHANGED),
        (s.b.join("dist/x.css"), FileChangeType::CHANGED),
    ]);
    let (to_a, to_b) = s.sent();
    let unix = usize::from(cfg!(unix));
    assert_eq!((to_a.len(), to_b.len()), (1, 1 + unix));
}

/// A language no built-in server lists, so the settings' server is its
/// only one.
const LANGUAGE: &str = "css-vars";

/// The server's settings with `lookupFiles` as given.
fn css_variables(lookup: &[&str]) -> LspServerSettings {
    let settings = json!({ "cssVariables": {
        "lookupFiles": lookup,
        "blacklistFolders": ["**/node_modules"],
    } });
    LspServerSettings {
        command: Some("fake-vars".into()),
        languages: Some(vec![LANGUAGE.into()]),
        watch_files_from: Some("/cssVariables/lookupFiles".into()),
        watch_excludes_from: Some("/cssVariables/blacklistFolders".into()),
        settings: settings.as_object().cloned(),
        ..LspServerSettings::default()
    }
}

#[test]
fn lookup_files_change_changes_what_is_sent() {
    let fake = FakeServer::new(Script::default());
    let (mut manager, events) = manager(fake.launcher(), Some(Arc::new(ManualClock::new())));
    let mut settings = LspSettings::default();
    settings
        .servers
        .insert("vars".into(), css_variables(&["**/*.css"]));
    manager.update_settings(&settings);
    let mut buffer = Buffer::new(":root { --x: red; }\n");
    let path = fake_path("site.css");
    let server = manager
        .open(
            Some(Path::new(ROOT)),
            buffer.id(),
            buffer.text().clone(),
            buffer.version(),
            buffer.subscribe(),
            &path,
            LANGUAGE,
        )
        .unwrap()
        .unwrap();
    assert_eq!(events.status(server), ServerStatus::Starting);
    fake.wait_initialized();
    let changes = || {
        vec![
            (fake_path("a.css"), FileChangeType::CHANGED),
            (fake_path("b.scss"), FileChangeType::CHANGED),
            (fake_path("node_modules/c.scss"), FileChangeType::CHANGED),
        ]
    };

    manager.files_changed(changes());
    let sent = |manager: &LspManager| sent_to(&fake, manager, &[1]).remove(0);
    assert_eq!(
        sent(&manager),
        [json!({ "changes": [{ "uri": uri(&fake_path("a.css")), "type": 2 }] })]
    );

    // The user moves the scan to SCSS: the server is configured, not
    // restarted, and the watch follows its settings from then on.
    settings
        .servers
        .insert("vars".into(), css_variables(&["**/*.scss"]));
    manager.update_settings(&settings);
    events.until("the configuration's Notified", |event| match event {
        LspEvent::Notified {
            server: from,
            what: Notice::Configuration,
        } if from == server => Some(()),
        _ => None,
    });
    manager.files_changed(changes());
    assert_eq!(
        sent(&manager)[1..],
        [json!({ "changes": [{ "uri": uri(&fake_path("b.scss")), "type": 2 }] })]
    );
    assert_eq!(fake.launches(), 1);

    // A pointer at something that isn't a list of globs watches nothing.
    let mut broken = css_variables(&[]);
    broken.settings = json!({ "cssVariables": { "lookupFiles": "**/*.css" } })
        .as_object()
        .cloned();
    settings.servers.insert("vars".into(), broken);
    manager.update_settings(&settings);
    manager.files_changed(changes());
    assert_eq!(sent(&manager).len(), 2);
    assert_eq!(fake.launches(), 1);
}
