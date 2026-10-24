//! A disabled extension in the extensions list (extensions record §10);
//! the list following the extensions and the consents while it shows; its
//! servers' status by extension, with what each runs in full; and Enter,
//! which asks about each waiting or declined server in turn through the
//! consent prompt, or shows a row's details.

use std::{fs, os::unix::fs::PermissionsExt, path::Path};

use fe_lsp::ServerSpec;
use gpui::{Focusable as _, TestAppContext, VisualTestContext};
use kiln::{
    Config,
    core::{ApiVersion, extension::MANIFEST_FILE},
    platform::config::{BatchSender, batch_channel},
};
use serde_json::json;
use tempfile::TempDir;

use super::{
    ExtensionState,
    extensions_picker_tests::{dispatch, draw, picker, rows, workbench},
};
use crate::{
    ExtensionServer, decline_extension_server, extension_servers, grant_extension_server,
    pending_extension_consents, picker::PickerDelegate as _, set_extension_servers,
    tests::open_root,
};

/// Zig, with the language servers `servers` names, and Wasm, with one
/// named `wasm_server` if given, installed; kiln started over them.
fn host(
    cx: &mut TestAppContext,
    servers: &[&str],
    wasm_server: Option<&str>,
) -> (TempDir, BatchSender) {
    let dir = tempfile::tempdir().unwrap();
    let install = |name: &str, id: &str, servers: &[&str]| {
        let path = dir.path().join("extensions").join(name);
        fs::create_dir_all(&path).unwrap();
        let mut manifest = format!(
            "id = \"{id}\"\nname = \"{}\"\nversion = \"1.2.3\"\nlicense = \"MIT\"\n\n\
             [engine]\nfastedit = \"1.0\"\n",
            name[..1].to_uppercase() + &name[1..]
        );
        for server in servers {
            manifest.push_str(&format!(
                "\n[contributes.lsp.{server}]\ncommand = \"{server}\"\n"
            ));
        }
        fs::write(path.join(MANIFEST_FILE), manifest).unwrap();
    };
    install("zig", "acme.zig", servers);
    install("wasm", "acme.wasm", wasm_server.as_slice());
    let (sender, batches) = batch_channel();
    let config = Config::new("dev.fastedit.test", "fastedit")
        .extensions(
            dir.path().join("extensions"),
            "fastedit",
            ApiVersion { major: 1, minor: 0 },
        )
        .extension_kind("lsp", json!({ "type": "object" }))
        .watch_with(batches);
    cx.update(|cx| {
        assert_eq!(kiln::init(cx, config), []);
        cx.reset_prompt_builder();
    });
    cx.run_until_parked();
    (dir, sender)
}

/// Extension `name`'s (`zig`, `wasm`) server `spec`, as the host hands it.
fn server(dir: &Path, name: &str, spec: ServerSpec) -> ExtensionServer {
    ExtensionServer {
        extension_id: format!("acme.{name}"),
        extension_name: name[..1].to_uppercase() + &name[1..],
        extension_version: "1.2.3".into(),
        extension_license: "MIT".into(),
        extension_dir: dir.join("extensions").join(name).canonicalize().unwrap(),
        spec: ServerSpec {
            languages: vec!["zig".into()],
            ..spec
        },
    }
}

fn spec(name: &str, args: &[&str]) -> ServerSpec {
    ServerSpec {
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        ..ServerSpec::new(name, format!("/opt/{name}/bin/{name}"))
    }
}

/// The servers, handed with the window inactive, so it doesn't ask by
/// itself.
fn give(cx: &mut VisualTestContext, servers: Vec<ExtensionServer>) {
    cx.deactivate_window();
    cx.update(|_, cx| set_extension_servers(cx, servers));
    cx.run_until_parked();
}

/// Decides each pending server `names` has: grant or decline.
fn decide(cx: &mut VisualTestContext, names: &[&str], grant: bool) {
    for pending in cx.update(|_, cx| pending_extension_consents(cx)) {
        if names.contains(&pending.server.as_str()) {
            cx.update(|_, cx| {
                if grant {
                    grant_extension_server(cx, &pending).unwrap();
                } else {
                    decline_extension_server(cx, &pending).unwrap();
                }
            });
        }
    }
    cx.run_until_parked();
}

/// Opens the list filtered to `query` and presses Enter on its first row.
fn enter_on(query: &str, cx: &mut VisualTestContext) {
    dispatch("workbench.show_extensions", cx);
    cx.simulate_input(query);
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
}

/// The server the prompt up now asks about.
fn asked(cx: &mut VisualTestContext) -> String {
    let (_, detail) = cx.pending_prompt().expect("a prompt");
    let line = detail
        .lines()
        .find_map(|line| line.strip_prefix("Language server: "))
        .expect("names a server");
    line.split(' ').next().unwrap().to_owned()
}

fn answer(cx: &mut VisualTestContext, button: &str) {
    cx.simulate_prompt_answer(button);
    draw(cx);
}

#[gpui::test]
fn the_list_shows_disabled(cx: &mut TestAppContext) {
    let (host, _sender) = host(cx, &["zls"], None);
    let (workbench, mut cx) = workbench(&host, cx);
    let picker = picker(&workbench, &mut cx);
    cx.update(|_, cx| kiln::extensions::set_disabled(cx, ["acme.zig"]));
    dispatch("workbench.show_extensions", &mut cx);
    let states: Vec<_> = (rows(&picker, &mut cx).into_iter())
        .map(|row| (row.id, row.state))
        .collect();
    assert_eq!(
        states,
        [
            ("acme.wasm".to_owned(), ExtensionState::Active),
            ("acme.zig".to_owned(), ExtensionState::Disabled),
        ]
    );
    // Enter shows its details, its state among them.
    enter_on("zig", &mut cx);
    let (_, detail) = cx.pending_prompt().expect("a prompt");
    assert!(detail.contains("State: Disabled"), "{detail}");
}

#[gpui::test]
fn the_list_follows_extensions_and_consents_while_it_shows(cx: &mut TestAppContext) {
    let (host, _sender) = host(cx, &["zls"], None);
    let (workbench, mut cx) = workbench(&host, cx);
    let picker = picker(&workbench, &mut cx);
    cx.deactivate_window();
    dispatch("workbench.show_extensions", &mut cx);
    // Zig selected, below Wasm.
    picker.update(&mut cx, |picker, cx| picker.select(1, cx));
    let selected = |cx: &mut VisualTestContext| {
        picker.read_with(cx, |picker, _| picker.delegate.match_id(picker.selected()))
    };
    let zig = selected(&mut cx);
    let zig_state = |cx: &mut VisualTestContext| {
        let rows = rows(&picker, cx);
        rows.into_iter()
            .find(|row| row.id == "acme.zig")
            .unwrap()
            .state
    };
    assert_eq!(zig_state(&mut cx), ExtensionState::Active);

    // Its server's consent is evaluated while the list shows.
    give(&mut cx, vec![server(host.path(), "zig", spec("zls", &[]))]);
    assert_eq!(
        zig_state(&mut cx),
        ExtensionState::NeedsConsent {
            servers: vec!["zls".into()]
        }
    );
    assert_eq!(selected(&mut cx), zig);

    // Decided elsewhere: the row follows.
    decide(&mut cx, &["zls"], true);
    assert_eq!(zig_state(&mut cx), ExtensionState::Active);

    // Disabled, then enabled, while it shows.
    cx.update(|_, cx| kiln::extensions::set_disabled(cx, ["acme.zig"]));
    draw(&mut cx);
    assert_eq!(zig_state(&mut cx), ExtensionState::Disabled);
    assert_eq!(selected(&mut cx), zig);
    cx.update(|_, cx| kiln::extensions::set_disabled(cx, Vec::<String>::new()));
    draw(&mut cx);
    assert_eq!(zig_state(&mut cx), ExtensionState::Active);

    // The query stays.
    cx.simulate_input("wasm");
    draw(&mut cx);
    cx.update(|_, cx| kiln::extensions::set_disabled(cx, ["acme.wasm"]));
    draw(&mut cx);
    let shown: Vec<_> = (rows(&picker, &mut cx).into_iter())
        .map(|row| (row.id, row.state))
        .collect();
    assert_eq!(shown, [("acme.wasm".to_owned(), ExtensionState::Disabled)]);
}

#[gpui::test]
fn enter_asks_about_the_undecided_then_each_declined(cx: &mut TestAppContext) {
    let (host, _sender) = host(cx, &["zls", "zls_two"], None);
    let (_workbench, mut cx) = workbench(&host, cx);
    let servers = |two: &[&str]| {
        vec![
            server(host.path(), "zig", spec("zls", &[])),
            server(host.path(), "zig", spec("zls_two", two)),
        ]
    };
    let running = |cx: &mut VisualTestContext| -> Vec<String> {
        (cx.update(|_, cx| extension_servers(cx)).into_iter())
            .map(|spec| spec.name)
            .collect()
    };
    give(&mut cx, servers(&[]));

    // Both declined: Enter goes through each in turn.
    decide(&mut cx, &["zls", "zls_two"], false);
    enter_on("zig", &mut cx);
    assert_eq!(asked(&mut cx), "zls");
    answer(&mut cx, "Don't Allow");
    assert_eq!(asked(&mut cx), "zls_two");
    answer(&mut cx, "Allow");
    assert!(!cx.has_pending_prompt());
    assert_eq!(running(&mut cx), ["zls_two"]);

    // Mixed: the first declined, the second changed since its grant, so
    // waiting. Enter asks about the waiting one first, then the declined.
    give(&mut cx, servers(&["--stdio"]));
    let pending = cx.update(|_, cx| pending_extension_consents(cx));
    let declined: Vec<_> = pending
        .iter()
        .map(|p| (p.server.as_str(), p.declined))
        .collect();
    assert_eq!(declined, [("zls", true), ("zls_two", false)]);
    enter_on("zig", &mut cx);
    assert_eq!(asked(&mut cx), "zls_two");
    answer(&mut cx, "Don't Allow");
    assert_eq!(asked(&mut cx), "zls");
    answer(&mut cx, "Allow");
    assert!(!cx.has_pending_prompt());
    assert_eq!(running(&mut cx), ["zls"]);
}

#[gpui::test]
fn a_picker_prompt_is_the_only_one_for_its_server(cx: &mut TestAppContext) {
    let (host, _sender) = host(cx, &["zls"], None);
    let (_workbench, mut first) = workbench(&host, cx);
    let other = host.path().join("other");
    fs::create_dir_all(&other).unwrap();
    let (_second, second) = open_root(cx, &other);
    let mut second = second.clone();
    second.deactivate_window();
    let servers = vec![server(host.path(), "zig", spec("zls", &[]))];
    give(&mut first, servers.clone());
    assert!(!first.has_pending_prompt());

    // The list asks about it.
    enter_on("zig", &mut first);
    assert_eq!(asked(&mut first), "zls");

    // Meanwhile it's evaluated again, and the other window becomes active:
    // neither asks about it a second time.
    first.update(|_, cx| set_extension_servers(cx, servers));
    first.run_until_parked();
    second.update(|window, _| window.activate_window());
    second.run_until_parked();
    answer(&mut first, "Allow");
    cx.run_until_parked();
    assert!(!first.has_pending_prompt());
    assert!(!second.has_pending_prompt());
    assert_eq!(cx.update(|cx| extension_servers(cx)).len(), 1);
}

#[gpui::test]
fn servers_show_by_extension_with_what_they_run(cx: &mut TestAppContext) {
    let (host, _sender) = host(cx, &["zls"], Some("zls"));
    let (workbench, mut cx) = workbench(&host, cx);
    let picker = picker(&workbench, &mut cx);
    // Zig's runs from the PATH its environment sets, with an argument
    // that has a space.
    let bin = host.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let zls = bin.join("zls");
    fs::write(&zls, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&zls, fs::Permissions::from_mode(0o755)).unwrap();
    let path = bin.to_string_lossy().into_owned();
    let zig = ServerSpec {
        args: vec!["--config".into(), "my config.json".into()],
        env: vec![
            ("PATH".into(), path.clone()),
            ("ZLS_LOG".into(), "debug".into()),
        ],
        ..ServerSpec::new("zls", "zls")
    };
    let wasm = spec("zls", &["--wasm"]);
    give(
        &mut cx,
        vec![
            server(host.path(), "zig", zig),
            server(host.path(), "wasm", wasm),
        ],
    );
    let line = |status: &str| {
        let command = zls.display();
        format!("zls: PATH={path} ZLS_LOG=debug {command} --config 'my config.json' ({status})")
    };
    dispatch("workbench.show_extensions", &mut cx);
    let detail = |cx: &mut VisualTestContext, id: &str| {
        let rows = rows(&picker, cx);
        rows.into_iter().find(|row| row.id == id).unwrap().servers
    };
    assert_eq!(detail(&mut cx, "acme.zig"), [line("pending")]);

    // Both granted: only the first of the name runs, and the other's row
    // says so, not that it's running.
    decide(&mut cx, &["zls"], true);
    assert_eq!(detail(&mut cx, "acme.zig"), [line("granted")]);
    assert_eq!(
        detail(&mut cx, "acme.wasm"),
        [
            "zls: /opt/zls/bin/zls --wasm (granted; not running: an earlier extension's \
          server of this name runs)"
        ]
    );
}

#[gpui::test]
fn enter_on_any_other_row_shows_all_of_it(cx: &mut TestAppContext) {
    let (host, _sender) = host(cx, &["zls", "zls_two"], None);
    let (_workbench, mut cx) = workbench(&host, cx);
    let long: Vec<String> = (0..40)
        .map(|ix| format!("--option-{ix}=value-{ix}"))
        .collect();
    let long: Vec<&str> = long.iter().map(String::as_str).collect();
    give(
        &mut cx,
        vec![
            server(host.path(), "zig", spec("zls", &long)),
            server(host.path(), "zig", spec("zls_two", &["--stdio"])),
        ],
    );
    decide(&mut cx, &["zls", "zls_two"], true);
    enter_on("zig", &mut cx);
    let (question, detail) = cx.pending_prompt().expect("a prompt");
    assert_eq!(question, "Zig 1.2.3");
    let lines: Vec<&str> = detail.lines().collect();
    assert!(lines.contains(&"State: Active"), "{detail}");
    assert!(lines.contains(&"Contributes: 2 lsp servers"), "{detail}");
    let first = format!("  zls: /opt/zls/bin/zls {} (granted)", long.join(" "));
    assert!(lines.contains(&first.as_str()), "{detail}");
    assert!(
        lines.contains(&"  zls_two: /opt/zls_two/bin/zls_two --stdio (granted)"),
        "{detail}"
    );
    answer(&mut cx, "OK");
    assert!(!cx.has_pending_prompt());
}

/// The prompts Enter shows take focus from the editor that was focused
/// before the list opened, and give it back there once answered: with the
/// prompt drawn in the window (Linux's), keys go to it, not the editor.
#[gpui::test]
fn its_prompts_take_focus_and_give_it_back(cx: &mut TestAppContext) {
    let (host, _sender) = host(cx, &["zls"], None);
    let (workbench, mut cx) = workbench(&host, cx);
    let file = host.path().join("project/a.txt");
    fs::write(&file, "alpha\n").unwrap();
    let pane = workbench.read_with(&cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(file, window, cx)));
    draw(&mut cx);
    let editor = workbench.read_with(&cx, |workbench, cx| {
        workbench.active_editor(cx).cloned().expect("a.txt shows")
    });
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    give(&mut cx, vec![server(host.path(), "zig", spec("zls", &[]))]);
    cx.update(|_, cx| cx.set_prompt_builder(fe_ui::prompt_renderer));
    let editor_focused = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
    };
    let prompt_shows = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            window.has_active_prompt()
        })
    };
    let text = |cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, _| editor.model().text().to_string())
    };
    assert!(editor_focused(&mut cx));

    // Wasm's details, then Zig's consent: Enter answers each.
    for query in ["wasm", "zig"] {
        enter_on(query, &mut cx);
        assert!(prompt_shows(&mut cx), "{query}");
        assert!(!editor_focused(&mut cx), "{query}: the prompt has the keys");
        cx.simulate_keystrokes("enter");
        draw(&mut cx);
        assert!(!prompt_shows(&mut cx), "{query}");
        assert!(editor_focused(&mut cx), "{query}: back in the editor");
        assert_eq!(text(&mut cx), "alpha\n", "{query}: the editor took no key");
    }
    // Enter on the consent prompt was Allow.
    assert_eq!(cx.update(|_, cx| extension_servers(cx)).len(), 1);
}
