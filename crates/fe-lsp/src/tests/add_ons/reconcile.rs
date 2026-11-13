//! Reconciling per server (§4 joins, removals and reroots, §6.2, §7 Sync):
//! settings, trust and specs adding or removing one server of a document,
//! and `restart(name, root)` of one, leave its other servers alone.

use std::path::Path;

use fe_buffer::Buffer;
use fe_config::settings::LspSettings;
use lsp_types::Url;

use super::lead_unavailable::{tree, vars_spec};
use super::{CSS, Fixture, ROOT, defined, settle, spec};

/// Where a server comes from: each adds or removes one as [`update_settings`],
/// [`register_spec`] and [`set_extension_specs`] do.
///
/// [`update_settings`]: crate::LspManager::update_settings
/// [`register_spec`]: crate::LspManager::register_spec
/// [`set_extension_specs`]: crate::LspManager::set_extension_specs
#[derive(Clone, Copy, Debug)]
enum Source {
    Settings,
    Host,
    Extension,
}

/// Settings defining primary `css` and, if `vars`, add-on `vars`, both for
/// [`CSS`].
fn settings(vars: bool) -> LspSettings {
    if vars {
        defined(&[("css", &[CSS], false, true), ("vars", &[CSS], true, true)])
    } else {
        defined(&[("css", &[CSS], false, true)])
    }
}

/// Gives `f` primary `css` and, if `vars`, add-on `vars` from `source`.
/// (A host spec can't be taken back.)
fn configure(f: &mut Fixture, source: Source, vars: bool) {
    match source {
        Source::Settings => f.manager.update_settings(&settings(vars)),
        Source::Host => {
            f.manager.register_spec(spec("css", &[CSS], false));
            if vars {
                f.manager.register_spec(spec("vars", &[CSS], true));
            }
        }
        Source::Extension => {
            f.manager.register_spec(spec("css", &[CSS], false));
            let specs = if vars {
                vec![spec("vars", &[CSS], true)]
            } else {
                Vec::new()
            };
            f.manager.set_extension_specs(specs);
        }
    }
}

/// The document notifications fake `name` got for `uri`, methods only.
fn methods(f: &Fixture, name: &str, uri: &Url) -> Vec<String> {
    (f.notes(name, uri).into_iter())
        .map(|(method, _)| method)
        .collect()
}

/// Asserts fake `name` opened `uri` once and never closed it: its edits
/// may run together, so they aren't counted.
fn untouched(f: &Fixture, name: &str, uri: &Url, what: &str) {
    let methods = methods(f, name, uri);
    assert_eq!(
        methods.first().map(String::as_str),
        Some("didOpen"),
        "{what}"
    );
    assert!(
        methods[1..].iter().all(|method| method == "didChange"),
        "{what}: {name} got {methods:?}"
    );
    assert_eq!(f.fake(name).launches(), 1, "{what}");
}

#[test]
fn add_on_joining_on_reload_opens_at_current_text() {
    for source in [Source::Settings, Source::Host, Source::Extension] {
        let mut f = Fixture::new(&["css", "vars"]);
        configure(&mut f, source, false);
        let mut buffer = Buffer::new("a {}\n");
        let uri = f.open(&mut buffer, "a.css", CSS);
        f.wait_synced("css", &uri, &buffer);
        f.edit(&mut buffer, "x");
        f.edit(&mut buffer, "y");
        f.wait_synced("css", &uri, &buffer);
        let css = f.placed(&buffer, "css").unwrap();
        assert_eq!(f.fake("vars").launches(), 0, "{source:?}");

        // The add-on opens the document at the text it has now, at its
        // first version; the primary isn't touched.
        configure(&mut f, source, true);
        assert_eq!(
            f.wait_synced("vars", &uri, &buffer).version,
            0,
            "{source:?}"
        );
        let vars = f.placed(&buffer, "vars").unwrap();
        assert_eq!(f.manager.servers_of(buffer.id()), [css, vars], "{source:?}");
        assert_eq!(f.manager.lead_of(buffer.id()), Some(css), "{source:?}");
        f.barrier(vars);
        assert_eq!(f.notes("vars", &uri), [("didOpen".into(), Some(0))]);

        // Both sync on.
        f.edit(&mut buffer, "z");
        f.wait_synced("vars", &uri, &buffer);
        f.wait_synced("css", &uri, &buffer);
        untouched(&f, "css", &uri, &format!("{source:?}"));
    }
}

#[test]
fn removed_add_on_gets_did_close_alone() {
    // A host spec can't be taken back.
    for source in [Source::Settings, Source::Extension] {
        let mut f = Fixture::new(&["css", "vars"]);
        configure(&mut f, source, true);
        let mut buffer = Buffer::new("a {}\n");
        let uri = f.open(&mut buffer, "a.css", CSS);
        f.wait_synced("css", &uri, &buffer);
        f.wait_synced("vars", &uri, &buffer);
        let css = f.placed(&buffer, "css").unwrap();

        // The add-on is told the document closed, then shuts down; the
        // primary keeps it.
        configure(&mut f, source, false);
        f.fake("vars").wait_for("vars to shut down", |records| {
            records.iter().any(|r| r.method() == "shutdown")
        });
        assert_eq!(
            methods(&f, "vars", &uri),
            ["didOpen", "didClose"],
            "{source:?}"
        );
        assert_eq!(f.manager.servers_of(buffer.id()), [css], "{source:?}");
        assert_eq!(f.manager.route_servers(buffer.id()).len(), 1, "{source:?}");

        f.edit(&mut buffer, "x");
        f.wait_synced("css", &uri, &buffer);
        untouched(&f, "css", &uri, &format!("{source:?}"));
        settle();
        assert_eq!(
            methods(&f, "vars", &uri),
            ["didOpen", "didClose"],
            "{source:?}"
        );
        assert_eq!(f.fake("vars").launches(), 1, "{source:?}");
    }
}

#[test]
fn untrusted_add_on_root_starts_only_the_primary() {
    // The add-on runs at `proj` (its marker), the primary at `proj/src`.
    let (_dir, proj, path) = tree();
    let uri = Url::from_file_path(&path).unwrap();
    let mut f = Fixture::css_and_vars();
    f.manager.register_spec(vars_spec());
    let untrusted = proj.clone();
    let trust_all_but_proj = move |root: &Path| root != untrusted.as_path();
    f.manager.set_trust(trust_all_but_proj.clone());

    let mut buffer = Buffer::new("a {}\n");
    let css = f.open_at(None, &mut buffer, &path, CSS).unwrap();
    f.wait_synced("css", &uri, &buffer);
    assert_eq!(f.manager.servers_of(buffer.id()), [css]);
    assert_eq!(f.manager.lead_of(buffer.id()), Some(css));
    assert_eq!(f.manager.status_at("vars", &proj), None);
    f.edit(&mut buffer, "x");
    f.wait_synced("css", &uri, &buffer);
    settle();
    assert_eq!(f.fake("vars").launches(), 0);

    // Trusting it starts the add-on at the current text.
    f.manager.set_trust(|_| true);
    f.wait_synced("vars", &uri, &buffer);
    let vars = f.placed(&buffer, "vars").unwrap();
    assert_eq!(f.manager.servers_of(buffer.id()), [css, vars]);

    // Rejecting it again stops the add-on alone: the document waits there,
    // and the primary syncs on.
    f.manager.set_trust(trust_all_but_proj);
    f.fake("vars").wait_for("vars to shut down", |records| {
        records.iter().any(|r| r.method() == "shutdown")
    });
    assert_eq!(f.manager.servers_of(buffer.id()), [css]);
    assert_eq!(f.manager.route_servers(buffer.id())[1].placed, None);
    f.edit(&mut buffer, "y");
    f.wait_synced("css", &uri, &buffer);
    untouched(&f, "css", &uri, "trust");
    settle();
    assert_eq!(f.fake("vars").launches(), 1);
}

/// Settings defining primary `css` and add-on `vars` for [`CSS`], with
/// `off` turned off.
fn turned_off(off: Option<&str>) -> LspSettings {
    defined(&[
        ("css", &[CSS], false, off != Some("css")),
        ("vars", &[CSS], true, off != Some("vars")),
    ])
}

#[test]
fn enabled_false_drops_each_server() {
    for (off, on) in [("css", "vars"), ("vars", "css")] {
        let mut f = Fixture::new(&["css", "vars"]);
        f.manager.update_settings(&turned_off(None));
        let mut buffer = Buffer::new("a {}\n");
        let uri = f.open(&mut buffer, "a.css", CSS);
        f.wait_synced("css", &uri, &buffer);
        f.wait_synced("vars", &uri, &buffer);
        let kept = f.placed(&buffer, on).unwrap();

        // `off` closes the document and shuts down; `on` keeps it, and
        // leads it if `off` led it.
        f.manager.update_settings(&turned_off(Some(off)));
        f.fake(off).wait_for("it to shut down", |records| {
            records.iter().any(|r| r.method() == "shutdown")
        });
        assert_eq!(methods(&f, off, &uri), ["didOpen", "didClose"], "{off} off");
        assert_eq!(f.manager.servers_of(buffer.id()), [kept], "{off} off");
        assert_eq!(f.manager.route_of(buffer.id()).as_deref(), Some(on));
        f.edit(&mut buffer, "x");
        f.wait_synced(on, &uri, &buffer);
        untouched(&f, on, &uri, &format!("{off} off"));

        // Turned on again, it opens the document at the current text, and
        // the primary leads.
        f.manager.update_settings(&turned_off(None));
        let text = buffer.text().to_string();
        f.fake(off)
            .wait_for_document(&uri, "it to open it again", |r| {
                r.is_some_and(|r| r.open && r.launch == 2 && r.text == text)
            });
        assert_eq!(f.manager.route_of(buffer.id()).as_deref(), Some("css"));
        assert_eq!(f.manager.servers_of(buffer.id()).len(), 2, "{off} off");
        untouched(&f, on, &uri, &format!("{off} on again"));
    }
}

#[test]
fn restart_of_one_server_leaves_the_other_running() {
    for (restarted, other) in [("vars", "css"), ("css", "vars")] {
        let mut f = Fixture::css_and_vars();
        let mut buffer = Buffer::new("a {}\n");
        let uri = f.open(&mut buffer, "a.css", CSS);
        f.edit(&mut buffer, "x");
        f.wait_synced("css", &uri, &buffer);
        f.wait_synced("vars", &uri, &buffer);
        let (old, kept) = (
            f.placed(&buffer, restarted).unwrap(),
            f.placed(&buffer, other).unwrap(),
        );

        let new = (f.manager.restart(restarted, Path::new(ROOT)).unwrap()).expect("it restarted");
        assert_ne!(new, old, "{restarted}");
        let text = buffer.text().to_string();
        f.fake(restarted)
            .wait_for_document(&uri, "the new instance to open it", |r| {
                r.is_some_and(|r| r.open && r.launch == 2 && r.text == text)
            });
        assert_eq!(f.placed(&buffer, restarted), Some(new), "{restarted}");
        assert_eq!(f.placed(&buffer, other), Some(kept), "{restarted}");
        assert_eq!(f.manager.lead_of(buffer.id()), f.placed(&buffer, "css"));

        // The other server never noticed.
        f.edit(&mut buffer, "y");
        f.wait_synced(restarted, &uri, &buffer);
        f.wait_synced(other, &uri, &buffer);
        untouched(&f, other, &uri, restarted);
    }
}

#[test]
fn reroot_moves_only_the_server_whose_root_changed() {
    // The add-on runs at `proj` (its marker), the primary at `proj/src`.
    let (_dir, proj, path) = tree();
    let src = path.parent().unwrap().to_owned();
    let uri = Url::from_file_path(&path).unwrap();
    let mut f = Fixture::css_and_vars();
    f.manager.register_spec(vars_spec());
    let mut buffer = Buffer::new("a {}\n");
    let css = f.open_at(None, &mut buffer, &path, CSS).unwrap();
    f.wait_synced("css", &uri, &buffer);
    f.wait_synced("vars", &uri, &buffer);
    let old = f.placed(&buffer, "vars").unwrap();

    // Without its marker, the add-on's root is the primary's: the next
    // reconcile closes the document on the old instance, which runs on,
    // and opens it on one at the new root at the current text.
    std::fs::remove_file(proj.join("vars.json")).unwrap();
    f.edit(&mut buffer, "x");
    f.manager.update_settings(&LspSettings::default());
    let text = buffer.text().to_string();
    f.fake("vars")
        .wait_for_document(&uri, "the new root's instance to open it", |r| {
            r.is_some_and(|r| r.open && r.launch == 2 && r.text == text)
        });
    let new = f.placed(&buffer, "vars").unwrap();
    assert_ne!(new, old);
    assert_eq!(f.manager.servers_of(buffer.id()), [css, new]);
    assert_eq!(f.manager.route_servers(buffer.id())[1].root, src);
    let old_records = f
        .fake("vars")
        .wait_for_at(1, "the old instance to close it", |records| {
            records
                .iter()
                .any(|r| r.is_notification("textDocument/didClose"))
        });
    assert!(!old_records.iter().any(|r| r.method() == "shutdown"));
    // It still answers.
    f.barrier(old);
    untouched(&f, "css", &uri, "reroot");
}
