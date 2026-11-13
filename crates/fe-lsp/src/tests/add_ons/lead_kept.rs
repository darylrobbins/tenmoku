//! A lead that keeps running stays the lead (§4), even once another
//! primary would come first; restarted, it gives way to that one.

use fe_buffer::Buffer;
use fe_config::settings::LspSettings;
use lsp_types::Url;
use std::path::Path;

use super::super::ROOT;
use super::{CSS, Fixture, defined, settle};

/// `a` (when `a`) and `b` both primaries for CSS, `b` with `args`. `a`
/// sorts first, so it comes first.
fn settings(a: bool, args: &[&str]) -> LspSettings {
    let mut settings = defined(&[("a", &[CSS], false, a), ("b", &[CSS], false, true)]);
    settings.servers.get_mut("b").unwrap().args =
        Some(args.iter().map(|&arg| arg.to_owned()).collect());
    settings
}

/// A document on `b`, kept there once `a` comes first.
fn kept() -> (Fixture, Buffer, Url) {
    let mut f = Fixture::new(&["a", "b"]);
    f.manager.update_settings(&settings(false, &[]));
    let mut buffer = Buffer::new("a {}\n");
    let uri = f.open(&mut buffer, "a.css", CSS);
    f.wait_synced("b", &uri, &buffer);
    let b = f.manager.lead_of(buffer.id()).unwrap();

    f.manager.update_settings(&settings(true, &[]));
    settle();
    assert_eq!(f.manager.route_for(CSS).as_deref(), Some("a"));
    assert_eq!(f.manager.lead_of(buffer.id()), Some(b));
    assert_eq!(f.manager.route_of(buffer.id()).as_deref(), Some("b"));
    assert_eq!(f.fake("a").launches(), 0);
    assert_eq!(f.fake("b").launches(), 1);
    (f, buffer, uri)
}

/// The document left `b` for `a`, which has it at its current text.
fn moved(f: &Fixture, buffer: &Buffer, uri: &Url) {
    f.wait_synced("a", uri, buffer);
    let a = f.manager.lead_of(buffer.id()).unwrap();
    assert_eq!(f.manager.servers_of(buffer.id()), [a]);
    assert_eq!(f.manager.route_of(buffer.id()).as_deref(), Some("a"));
    let replica = f.fake("b").document(uri).unwrap();
    assert!(replica.launch == 1, "not opened on the new b: {replica:?}");
}

#[test]
fn a_kept_lead_restarted_by_a_spec_change_gives_way() {
    let (mut f, mut buffer, uri) = kept();
    f.edit(&mut buffer, "x");
    f.manager.update_settings(&settings(true, &["--again"]));
    moved(&f, &buffer, &uri);
}

#[test]
fn a_kept_lead_restarted_by_hand_gives_way() {
    let (mut f, mut buffer, uri) = kept();
    f.edit(&mut buffer, "x");
    f.manager.restart("b", Path::new(ROOT)).unwrap().unwrap();
    moved(&f, &buffer, &uri);
    super::super::wait_until("b to start again", || f.fake("b").launches() == 2);
}
