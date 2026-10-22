//! The consent prompt: its text, who it asks, and what the answers do.

use std::{fs, path::Path};

use fe_lsp::ServerSpec;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{
    ExtensionServer, Invocation, Launch, PendingConsent, Workbench, extension_servers,
    lsp_consent_prompt::consent_prompt_text, pending_extension_consents, set_extension_servers,
};
use fe_gpui::{EditorSettings, EditorTheme};

fn pending(invocation: Invocation, previous: Option<Invocation>) -> PendingConsent {
    PendingConsent {
        extension_id: "acme.zig".into(),
        extension_name: "Zig".into(),
        extension_version: "1.2.3".into(),
        extension_license: "MIT".into(),
        extension_dir: "/ext/zig".into(),
        server: "zls".into(),
        languages: vec!["zig".into(), "zir".into()],
        invocation,
        previous,
        declined: false,
    }
}

fn invocation(command: &str, args: &[&str]) -> Invocation {
    Invocation {
        command: command.into(),
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        env: [("ZLS_LOG".to_owned(), "info".to_owned())].into(),
        script_hashes: Default::default(),
    }
}

#[test]
fn consent_prompt_text_shows_the_effective_invocation() {
    let plain = invocation("/opt/zls/bin/zls", &["--stdio", "--log"]);
    let (question, detail) = consent_prompt_text(&pending(plain.clone(), None));
    assert_eq!(question, "Allow Zig to run a language server?");
    for expected in [
        "acme.zig",
        "1.2.3",
        "MIT",
        "/ext/zig",
        "zls",
        "zig, zir",
        "/opt/zls/bin/zls",
        "--stdio",
        "--log",
        "ZLS_LOG=info",
    ] {
        assert!(detail.contains(expected), "{expected} in {detail}");
    }
    assert!(!detail.contains("can run any code"));
    assert!(!detail.contains("changed what it runs"));

    // An interpreter can run anything its arguments say, and its script is named.
    let mut node = invocation("/usr/bin/node", &["server.js"]);
    node.script_hashes
        .insert("server.js".into(), "abc123 /ext/zig/server.js".into());
    let (_, detail) = consent_prompt_text(&pending(node, None));
    assert!(detail.contains("node can run any code in its arguments."));
    assert!(detail.contains("/ext/zig/server.js"));

    // A change shows what was granted before.
    let old = invocation("/opt/zls/old/zls", &["--stdio"]);
    let (_, detail) = consent_prompt_text(&pending(plain, Some(old)));
    assert!(detail.contains("This extension changed what it runs."));
    assert!(detail.contains("/opt/zls/old/zls"));
    assert!(detail.contains("/opt/zls/bin/zls"));
}

/// An extension directory, canonical.
fn extension(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("zig");
    fs::create_dir_all(&path).unwrap();
    path.canonicalize().unwrap()
}

fn server(extension: &Path, args: &[&str]) -> ExtensionServer {
    ExtensionServer {
        extension_id: "acme.zig".into(),
        extension_name: "Zig".into(),
        extension_version: "1.2.3".into(),
        extension_license: "MIT".into(),
        extension_dir: extension.to_owned(),
        spec: ServerSpec {
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            languages: vec!["zig".into()],
            ..ServerSpec::new("zls", "/opt/zls/bin/zls")
        },
    }
}

/// A headless workbench in a new window.
fn window(cx: &mut TestAppContext) -> (Entity<Workbench>, VisualTestContext) {
    let launch = Launch {
        root: None,
        file: None,
    };
    let (workbench, vcx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let mut vcx = vcx.clone();
    vcx.update(|window, cx| {
        window.focus(&workbench.focus_handle(cx), cx);
        window.activate_window();
    });
    vcx.run_until_parked();
    (workbench, vcx)
}

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
}

fn give(cx: &mut TestAppContext, servers: Vec<ExtensionServer>) {
    cx.update(|cx| set_extension_servers(cx, servers));
    cx.run_until_parked();
}

fn handed(cx: &mut TestAppContext) -> usize {
    cx.update(|cx| extension_servers(cx).len())
}

#[gpui::test]
fn a_pending_consent_prompts_the_active_window_once(cx: &mut TestAppContext) {
    init(cx);
    let dir = tempfile::tempdir().unwrap();
    let ext = extension(dir.path());
    let (_first, mut first_cx) = window(cx);
    let (_second, second_cx) = window(cx);
    first_cx.deactivate_window();
    give(cx, vec![server(&ext, &["--stdio"])]);

    // The second window is the active one: it asks.
    let (question, detail) = second_cx.pending_prompt().expect("a prompt");
    assert_eq!(question, "Allow Zig to run a language server?");
    assert!(detail.contains("--stdio"), "{detail}");

    // The first window becoming active doesn't ask for the same server: once
    // it's answered, nothing else is asked (the platform shows one prompt
    // for all windows).
    first_cx.update(|window, _| window.activate_window());
    first_cx.run_until_parked();
    second_cx.simulate_prompt_answer("Don't Allow");
    cx.run_until_parked();
    assert!(!first_cx.has_pending_prompt());
    assert!(!second_cx.has_pending_prompt());
}

#[gpui::test]
fn allow_grants_and_dont_allow_declines(cx: &mut TestAppContext) {
    init(cx);
    let dir = tempfile::tempdir().unwrap();
    let ext = extension(dir.path());
    let (_workbench, mut vcx) = window(cx);

    give(cx, vec![server(&ext, &["--stdio"])]);
    assert_eq!(handed(cx), 0);
    vcx.simulate_prompt_answer("Allow");
    cx.run_until_parked();
    assert_eq!(handed(cx), 1);
    assert!(!vcx.has_pending_prompt());

    // A changed granted server asks again; Don't Allow leaves what ran, out.
    give(cx, vec![server(&ext, &["--stdio", "--verbose"])]);
    assert!(vcx.has_pending_prompt());
    vcx.simulate_prompt_answer("Don't Allow");
    cx.run_until_parked();
    assert_eq!(handed(cx), 0);
    assert!(!vcx.has_pending_prompt());

    // Not asked again for the same invocation, even when the window comes back.
    vcx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    assert!(!vcx.has_pending_prompt());
    assert!(cx.update(|cx| pending_extension_consents(cx))[0].declined);

    // Until it changes.
    give(cx, vec![server(&ext, &["--stdio", "--other"])]);
    assert!(vcx.has_pending_prompt());
}

#[gpui::test]
fn closing_the_window_leaves_it_pending(cx: &mut TestAppContext) {
    init(cx);
    let dir = tempfile::tempdir().unwrap();
    let ext = extension(dir.path());
    let (_first, mut first_cx) = window(cx);
    give(cx, vec![server(&ext, &["--stdio"])]);
    assert!(first_cx.has_pending_prompt());

    // The window closes with the prompt up: nothing was decided.
    first_cx.update(|window, _| window.remove_window());
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| pending_extension_consents(cx)).len(), 1);
    assert!(!cx.update(|cx| pending_extension_consents(cx))[0].declined);
    assert_eq!(handed(cx), 0);

    // The platform still queues the closed window's prompt: answering it
    // decides nothing.
    let (_second, second_cx) = window(cx);
    second_cx.simulate_prompt_answer("Don't Allow");
    cx.run_until_parked();
    let asked = cx.update(|cx| pending_extension_consents(cx));
    assert_eq!(asked.len(), 1);
    assert!(!asked[0].declined);

    // The next window to be active asks afresh, and its Allow grants.
    assert!(second_cx.has_pending_prompt());
    second_cx.simulate_prompt_answer("Allow");
    cx.run_until_parked();
    assert_eq!(handed(cx), 1);
    assert!(!second_cx.has_pending_prompt());
}
