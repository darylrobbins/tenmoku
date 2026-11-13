//! Lead unavailable (§3, §7): with the primary untrusted, missing or
//! crashed out, a healthy add-on is still placed and synced, the primary
//! stays the lead (never replaced), and the holder keeps its path.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fe_buffer::Buffer;
use lsp_types::Url;
use tempfile::TempDir;

use super::{CSS, Fixture, route, settle, spec};
use crate::fake::{FakeServer, Script};
use crate::{LaunchCancel, ServerId, ServerIo, ServerLauncher, ServerSpec, ServerStatus};

/// A project at `t/proj`, whose `vars.json` marks the add-on's root, with
/// a stylesheet at `t/proj/src/a.css`: the primary, with no markers, runs
/// at `t/proj/src`.
fn tree() -> (TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let proj = dir.path().join("t/proj");
    std::fs::create_dir_all(proj.join("src")).unwrap();
    std::fs::write(proj.join("vars.json"), "{}").unwrap();
    (dir, proj.clone(), proj.join("src/a.css"))
}

fn vars_spec() -> ServerSpec {
    ServerSpec {
        root_markers: vec!["vars.json".into()],
        ..spec("vars", &[CSS], true)
    }
}

/// Hands each launch to the launcher named for its spec; a name without
/// one fails as a missing command does.
struct Named(Vec<(&'static str, Arc<dyn ServerLauncher>)>);

impl ServerLauncher for Named {
    fn launch(
        &self,
        spec: &ServerSpec,
        root: &Path,
        cancel: &LaunchCancel,
    ) -> io::Result<ServerIo> {
        match self.0.iter().find(|(name, _)| *name == spec.name) {
            Some((_, launcher)) => launcher.launch(spec, root, cancel),
            None => Err(io::Error::new(io::ErrorKind::NotFound, "no such command")),
        }
    }
}

/// The routed `buffer`'s placement once `vars` is placed and `css` isn't:
/// the primary first, with no id.
fn assert_add_on_only(f: &Fixture, buffer: &Buffer, css_root: &Path, proj: &Path) -> ServerId {
    let vars = f.placed(buffer, "vars").expect("the add-on is placed");
    assert_eq!(f.manager.lead_of(buffer.id()), None);
    assert_eq!(f.manager.server_of(buffer.id()), None);
    assert_eq!(f.manager.servers_of(buffer.id()), [vars]);
    assert!(f.manager.is_served(buffer.id()));
    assert_eq!(f.manager.route_of(buffer.id()).as_deref(), Some("css"));
    assert_eq!(
        route(f.manager.route_servers(buffer.id())),
        [
            ("css".to_owned(), css_root.to_owned(), false, None),
            ("vars".to_owned(), proj.to_owned(), true, Some(vars)),
        ]
    );
    vars
}

#[test]
fn untrusted_primary_still_places_trusted_add_on() {
    let (_dir, proj, path) = tree();
    let uri = Url::from_file_path(&path).unwrap();
    let mut f = Fixture::css_and_vars();
    f.manager.register_spec(vars_spec());
    let trusted = proj.clone();
    f.manager.set_trust(move |root| root == trusted.as_path());

    let mut buffer = Buffer::new("a {}\n");
    assert_eq!(f.open_at(None, &mut buffer, &path, CSS), None);
    f.wait_synced("vars", &uri, &buffer);
    assert_add_on_only(&f, &buffer, path.parent().unwrap(), &proj);
    assert_eq!(f.fake("css").launches(), 0);
    assert_eq!(f.manager.status_at("css", path.parent().unwrap()), None);
    assert_eq!(
        f.manager.language_server(&proj, CSS).map(|(name, _)| name),
        Some("css".to_owned())
    );
    assert_eq!(
        (f.manager.language_servers(&proj, CSS).into_iter())
            .map(|(name, add_on, _)| (name, add_on))
            .collect::<Vec<_>>(),
        [("css".to_owned(), false), ("vars".to_owned(), true)]
    );

    // Synced as ever.
    f.edit(&mut buffer, "x");
    f.wait_synced("vars", &uri, &buffer);

    // The holder keeps its path: an alias waits, even though only the
    // add-on is placed.
    let mut alias = Buffer::new("alias {}\n");
    f.open_at(None, &mut alias, &path, CSS);
    assert!(!f.manager.is_served(alias.id()));
    f.barrier(f.placed(&buffer, "vars").unwrap());
    assert_eq!(
        f.fake("vars").document(&uri).unwrap().text,
        buffer.text().to_string()
    );

    // Trusting the primary's root places the holder there too.
    f.manager.set_trust(|_| true);
    f.wait_synced("css", &uri, &buffer);
    let css = f.placed(&buffer, "css").unwrap();
    assert_eq!(f.manager.lead_of(buffer.id()), Some(css));
    assert!(f.manager.servers_of(alias.id()).is_empty());
}

#[test]
fn missing_primary_still_places_add_on() {
    let (_dir, proj, path) = tree();
    let uri = Url::from_file_path(&path).unwrap();
    let vars = FakeServer::new(Script::default());
    let launcher = Named(vec![("vars", vars.launcher())]);
    let mut f = Fixture::with_launcher(vec![("vars", vars)], Arc::new(launcher));
    f.manager.register_spec(spec("css", &[CSS], false));
    f.manager.register_spec(vars_spec());

    let mut buffer = Buffer::new("a {}\n");
    f.open_at(None, &mut buffer, &path, CSS);
    f.wait_synced("vars", &uri, &buffer);
    let css_root = path.parent().unwrap();
    super::super::wait_until("the primary to stop", || {
        matches!(
            f.manager.status_at("css", css_root),
            Some(ServerStatus::Stopped(Some(_)))
        )
    });
    super::super::wait_until("the primary to hand the document back", || {
        f.manager.route_servers(buffer.id())[0].placed.is_none()
    });
    assert_add_on_only(&f, &buffer, css_root, &proj);
    f.edit(&mut buffer, "x");
    f.wait_synced("vars", &uri, &buffer);
}

#[test]
fn crashed_out_primary_still_places_add_on() {
    let (_dir, proj, path) = tree();
    let uri = Url::from_file_path(&path).unwrap();
    let mut f = Fixture::css_and_vars();
    f.manager.register_spec(vars_spec());

    let mut buffer = Buffer::new("a {}\n");
    let css = f.open_at(None, &mut buffer, &path, CSS).unwrap();
    f.wait_synced("css", &uri, &buffer);
    f.wait_synced("vars", &uri, &buffer);

    // Crashes at 0, 1, 3, 7, 15 and 31 s: the sixth spends its budget.
    let status = |f: &Fixture| loop {
        match f.events.status(css) {
            ServerStatus::Starting | ServerStatus::Ready | ServerStatus::Indexing { .. } => {}
            status => return status,
        }
    };
    for n in 1..=5u32 {
        f.fake("css").crash();
        assert_eq!(status(&f), ServerStatus::Restarting(n));
        let backoff = Duration::from_secs(1 << (n - 1));
        f.clock.wait_for_sleep(backoff);
        f.clock.advance(backoff);
        f.fake("css").wait_initialized_at(n as usize + 1);
    }
    f.fake("css").crash();
    assert!(matches!(status(&f), ServerStatus::Stopped(Some(_))));
    super::super::wait_until("the primary to hand the document back", || {
        f.manager.lead_of(buffer.id()).is_none()
    });
    assert_add_on_only(&f, &buffer, path.parent().unwrap(), &proj);

    // Nothing starts it again, and the add-on syncs on.
    f.edit(&mut buffer, "x");
    f.wait_synced("vars", &uri, &buffer);
    settle();
    assert_eq!(f.fake("css").launches(), 6);
    assert_eq!(f.fake("vars").launches(), 1);
}
