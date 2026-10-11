//! Trust grants and settings reloads move documents on the manager by
//! themselves; the workbench's owners and reservations follow, so no path
//! alias opens a document a server already has (T23).

use std::sync::Arc;

use fe_config::{Settings, SettingsEvent, settings::LspServerSettings};
use fe_lsp::fake::{FakeServer, Record};
use gpui::TestAppContext;
use lsp_types::Url;

use super::lsp::Lsp;

const ALIAS: &str = "src/../src/main.rs";

fn preload() {
    for name in ["rust", "python"] {
        assert!(fe_syntax::language_by_name(name).is_some());
    }
}

/// Reloads `s`'s settings with each of `servers` serving its languages
/// (`None`: disabled).
fn reload(s: &mut Lsp, servers: &[(&str, Option<&[&str]>)]) {
    let mut settings = Settings::default();
    for &(name, languages) in servers {
        settings.lsp.servers.insert(
            name.to_owned(),
            LspServerSettings {
                enabled: languages.is_some(),
                languages: languages.map(|l| l.iter().map(|&l| l.to_owned()).collect()),
                ..LspServerSettings::default()
            },
        );
    }
    let event = SettingsEvent {
        settings: Arc::new(settings),
        previous: Arc::new(Settings::default()),
        diagnostics: Vec::new(),
    };
    let workbench = s.workbench.clone();
    workbench.update(&mut s.cx, |w, cx| w.reload_settings(&event, cx));
    s.draw();
}

/// How many documents of `uri` `records` leave open, and the most open at
/// once.
fn open_counts(records: &[Record], uri: &Url) -> (usize, usize) {
    let (mut open, mut most) = (0usize, 0usize);
    for record in records {
        if let Record::Notification { method, params } = record
            && params["textDocument"]["uri"] == uri.as_str()
        {
            match method.as_str() {
                "textDocument/didOpen" => open += 1,
                "textDocument/didClose" => open = open.saturating_sub(1),
                _ => {}
            }
            most = most.max(open);
        }
    }
    (open, most)
}

/// Asserts no launch of `server` ever had `uri` open twice at once, and
/// that its last launch has it open once.
fn assert_one_document(server: &FakeServer, uri: &Url) {
    // The fake records on its own thread: wait up to ten seconds for the
    // last launch's `didOpen`, and assert on what it has if none comes.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while open_counts(&server.records_of(server.launches()), uri).0 != 1
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let launches = server.launches();
    assert!(launches > 0, "the server was launched");
    for launch in 1..=launches {
        let (_, most) = open_counts(&server.records_of(launch), uri);
        assert!(most <= 1, "launch {launch} had {uri} open twice at once");
    }
    let (open, _) = open_counts(&server.records_of(launches), uri);
    assert_eq!(open, 1, "the last launch has {uri} open once");
}

/// Draws some frames more, for anything a first leaves to the next.
fn settle(s: &mut Lsp) {
    for _ in 0..3 {
        s.draw();
    }
}

#[gpui::test]
fn a_trust_grant_does_not_open_the_waiting_alias_too(cx: &mut TestAppContext) {
    preload();
    let mut s = Lsp::untrusted(cx);
    s.open("src/main.rs");
    s.open(ALIAS);
    s.open("src/main.rs");
    s.draw();
    let main = s.uri("src/main.rs");
    assert_eq!(s.sent(1, "textDocument/didOpen", &main), 0);

    s.set_trust(true);
    s.fake
        .wait_for_document(&main, "didOpen", |r| r.is_some_and(|r| r.open));
    settle(&mut s);
    // Sentinel: what the server was sent before it, it was sent by now.
    s.open("src/lib.rs");
    s.fake
        .wait_for_document(&s.uri("src/lib.rs"), "didOpen", |r| r.is_some());
    assert_eq!(s.sent(1, "textDocument/didOpen", &main), 1);
    assert_one_document(&s.fake, &main);
    let (held, actual) = s.workbench.read_with(&s.cx, |w, _| {
        (w.lsp.owner_servers(), w.lsp.actual_owner_servers())
    });
    assert_eq!(held, actual);
}

#[gpui::test]
fn a_settings_change_that_moves_the_owner_does_not_open_the_alias_too(cx: &mut TestAppContext) {
    preload();
    let mut s = Lsp::new(cx);
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());
    s.open(ALIAS);
    s.open("src/main.rs");

    // pyright takes Rust over from rust-analyzer: the owner moves to it,
    // where the alias would be routed too.
    reload(
        &mut s,
        &[
            ("rust-analyzer", Some(&["toml"])),
            ("pyright", Some(&["rust", "python"])),
        ],
    );
    s.python
        .wait_for_document(&main, "didOpen", |r| r.is_some_and(|r| r.open));
    settle(&mut s);
    s.open("src/lib.rs");
    s.python
        .wait_for_document(&s.uri("src/lib.rs"), "didOpen", |r| r.is_some());
    assert_one_document(&s.python, &main);
    let (held, actual) = s.workbench.read_with(&s.cx, |w, _| {
        (w.lsp.owner_servers(), w.lsp.actual_owner_servers())
    });
    assert_eq!(held, actual);
    assert!(
        held.iter()
            .all(|server| server.as_deref() == Some("pyright"))
    );
}

#[gpui::test]
fn a_settings_change_that_merges_routes_leaves_one_document(cx: &mut TestAppContext) {
    preload();
    let mut s = Lsp::new(cx);
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());
    s.open(ALIAS);
    s.cx.simulate_input("x");
    s.draw();
    s.open("src/main.rs");
    let owner = s
        .workbench
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("a tab is shown");
    // The owner goes to Python, the alias holds the document on Rust.
    s.cx.update(|_, cx| owner.update(cx, |view, cx| view.set_language(Some("python"), cx)));
    s.draw();
    s.python
        .wait_for_document(&main, "didOpen", |r| r.is_some_and(|r| r.open));
    s.fake.wait_for_document(&main, "didOpen", |r| {
        r.is_some_and(|r| r.open && r.text.starts_with('x'))
    });
    assert_eq!(
        s.workbench.read_with(&s.cx, |w, _| w.lsp.owner_servers()),
        [Some("pyright".into()), Some("rust-analyzer".into())]
    );

    // Both routes become pyright's.
    reload(
        &mut s,
        &[
            ("rust-analyzer", Some(&["toml"])),
            ("pyright", Some(&["rust", "python"])),
        ],
    );
    settle(&mut s);
    s.open("src/lib.rs");
    s.python
        .wait_for_document(&s.uri("src/lib.rs"), "didOpen", |r| r.is_some());
    assert_one_document(&s.python, &main);
    let (held, actual) = s.workbench.read_with(&s.cx, |w, _| {
        (w.lsp.owner_servers(), w.lsp.actual_owner_servers())
    });
    assert_eq!(held, actual);
    assert_eq!(held, vec![Some("pyright".to_owned()); 2]); // main.rs and lib.rs
}

/// Opens `src/main.rs` and an edited alias of it, then sends the plain
/// path's buffer to Python: pyright holds it, and rust-analyzer the alias.
fn split_across_servers(s: &mut Lsp) -> Url {
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());
    s.open(ALIAS);
    s.cx.simulate_input("x");
    s.draw();
    s.open("src/main.rs");
    let owner = s
        .workbench
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("a tab is shown");
    s.cx.update(|_, cx| owner.update(cx, |view, cx| view.set_language(Some("python"), cx)));
    s.draw();
    s.python
        .wait_for_document(&main, "didOpen", |r| r.is_some_and(|r| r.open));
    s.fake.wait_for_document(&main, "didOpen", |r| {
        r.is_some_and(|r| r.open && r.text.starts_with('x'))
    });
    main
}

#[gpui::test]
fn a_document_kept_on_a_running_server_survives_a_preferred_route_change(cx: &mut TestAppContext) {
    preload();
    let mut s = Lsp::new(cx);
    let main = split_across_servers(&mut s);

    // rust-analyzer now lists Python first; pyright is unchanged and keeps
    // running, and its document stays on it beside the Rust alias.
    reload(
        &mut s,
        &[("rust-analyzer", Some(&["rust", "python", "toml"]))],
    );
    settle(&mut s);
    s.open("src/lib.rs");
    s.fake
        .wait_for_document(&s.uri("src/lib.rs"), "didOpen", |r| r.is_some());
    assert_eq!(s.python.launches(), 1, "pyright wasn't restarted");
    assert_eq!(open_counts(&s.python.records_of(1), &main), (1, 1));
    assert_one_document(&s.fake, &main);
}

#[gpui::test]
fn trust_revoked_and_granted_with_converging_routes_opens_one_document(cx: &mut TestAppContext) {
    preload();
    let mut s = Lsp::new(cx);
    let main = split_across_servers(&mut s);
    reload(
        &mut s,
        &[("rust-analyzer", Some(&["rust", "python", "toml"]))],
    );
    settle(&mut s);

    // Without trust both documents wait for rust-analyzer; granted, they
    // go to it together, and it has the one.
    s.set_trust(false);
    settle(&mut s);
    s.set_trust(true);
    settle(&mut s);
    s.open("src/lib.rs");
    let lib = s.uri("src/lib.rs");
    s.fake
        .wait_for_document(&lib, "didOpen", |r| r.is_some_and(|r| r.open));
    settle(&mut s);
    for server in [&s.fake, &s.python] {
        for launch in 1..=server.launches() {
            let (_, most) = open_counts(&server.records_of(launch), &main);
            assert!(most <= 1, "launch {launch} had {main} open twice at once");
        }
    }
}

/// Asserts the workbench's owners are the documents the manager serves.
fn assert_owners_match(s: &mut Lsp) {
    assert!(
        s.workbench
            .read_with(&s.cx, |w, _| w.lsp.owners_match_manager()),
        "owners differ from the manager's placement"
    );
}

/// The Rust and Python aliases converged on pyright, one served and one
/// parked there; returns the URI and the two editors, owner's first.
fn converged(s: &mut Lsp) -> (Url, [gpui::Entity<fe_gpui::EditorView>; 2]) {
    let main = split_across_servers(s);
    let owner = s
        .workbench
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("a tab is shown");
    let alias = s
        .workbench
        .read_with(&s.cx, |w, cx| w.open_editors(cx))
        .into_iter()
        .find(|e| e.entity_id() != owner.entity_id())
        .expect("the alias");
    reload(
        s,
        &[
            ("rust-analyzer", Some(&["toml"])),
            ("pyright", Some(&["rust", "python"])),
        ],
    );
    settle(s);
    assert_owners_match(s);
    (main, [owner, alias])
}

/// What pyright's last launch has open of `uri`, once it has recorded the
/// `didOpen` expected: waited for up to ten seconds, as the fake server
/// records on its own thread, and its last count if none comes.
fn pyright_open(s: &mut Lsp, uri: &Url) -> usize {
    settle(s);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let launch = s.python.launches();
        let open = open_counts(&s.python.records_of(launch), uri).0;
        if open == 1 || std::time::Instant::now() >= deadline {
            return open;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[gpui::test]
fn separating_converged_routes_serves_both(cx: &mut TestAppContext) {
    preload();
    let mut s = Lsp::new(cx);
    let (main, _) = converged(&mut s);
    assert_eq!(pyright_open(&mut s, &main), 1);

    // Back to the built-in routes: the parked document goes to its server.
    reload(&mut s, &[]);
    settle(&mut s);
    assert_owners_match(&mut s);
    let rust = s
        .fake
        .wait_for_document(&main, "didOpen", |r| r.is_some_and(|r| r.open));
    assert!(rust.is_some());
    assert_eq!(pyright_open(&mut s, &main), 1);
    assert_eq!(
        s.workbench.read_with(&s.cx, |w, _| w.lsp.owner_servers()),
        [Some("pyright".into()), Some("rust-analyzer".into())]
    );
}

/// Closes the converged alias whose editor `pick` takes, and checks the
/// other is served and owns the path.
fn close_converged(cx: &mut TestAppContext, close_owner: bool) {
    preload();
    let mut s = Lsp::new(cx);
    let (main, [owner, alias]) = converged(&mut s);
    let closing = if close_owner { owner } else { alias };
    let pane = s.workbench.read_with(&s.cx, |w, _| w.pane().clone());
    s.cx.update(|window, cx| {
        let ix = pane
            .read(cx)
            .tabs()
            .iter()
            .position(|t| t.editor == closing);
        pane.update(cx, |pane, cx| pane.activate(ix.expect("a tab"), window, cx));
    });
    s.draw();
    s.cx.simulate_keystrokes("cmd-w");
    settle(&mut s);
    assert_owners_match(&mut s);
    assert_eq!(pyright_open(&mut s, &main), 1);
    assert_eq!(
        s.workbench.read_with(&s.cx, |w, _| w.lsp.owner_servers()),
        [Some("pyright".to_owned())]
    );
}

#[gpui::test]
fn closing_the_converged_owner_leaves_the_parked_alias_served(cx: &mut TestAppContext) {
    close_converged(cx, true);
}

#[gpui::test]
fn closing_the_parked_converged_alias_leaves_the_owner_served(cx: &mut TestAppContext) {
    close_converged(cx, false);
}

/// Sets the converged alias's language to none, and checks one document is
/// still served and owned.
fn unset_converged(cx: &mut TestAppContext, change_owner: bool) {
    preload();
    let mut s = Lsp::new(cx);
    let (main, [owner, alias]) = converged(&mut s);
    let changing = if change_owner { owner } else { alias };
    s.cx.update(|_, cx| changing.update(cx, |view, cx| view.set_language(None, cx)));
    settle(&mut s);
    assert_owners_match(&mut s);
    assert_eq!(pyright_open(&mut s, &main), 1);
}

#[gpui::test]
fn changing_the_converged_owners_language_leaves_the_parked_alias_served(cx: &mut TestAppContext) {
    unset_converged(cx, true);
}

#[gpui::test]
fn changing_the_parked_converged_alias_language_leaves_the_owner_served(cx: &mut TestAppContext) {
    unset_converged(cx, false);
}
