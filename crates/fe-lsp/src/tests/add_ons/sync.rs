//! Sync (§4, §7 Sync): each server of a document has its own replica, at
//! its own LSP versions, and the size limit and aliases apply to all.

use fe_buffer::{Buffer, Origin};

use super::{CSS, Fixture, echo_on, settle, spec};
use crate::event::too_large;
use crate::{LspError, ServerStatus};

#[test]
fn both_servers_get_did_open_and_each_edit_at_its_own_version() {
    // The primary had the URI open before in its session, so its versions
    // run ahead of the add-on's, which joins later.
    let mut f = Fixture::new(&["css", "vars"]);
    f.manager.register_spec(spec("css", &[CSS], false));
    let mut before = Buffer::new("a {}\n");
    let uri = f.open(&mut before, "a.css", CSS);
    f.wait_synced("css", &uri, &before);
    f.manager.close(before.id());
    f.wait_closed("css", &uri);
    f.manager.register_spec(spec("vars", &[CSS], true));
    settle();
    assert_eq!(f.fake("vars").launches(), 0, "nothing to open on it yet");

    let mut buffer = Buffer::new("b {}\n");
    assert_eq!(f.open(&mut buffer, "a.css", CSS), uri);
    let (css, vars) = (
        f.placed(&buffer, "css").unwrap(),
        f.placed(&buffer, "vars").unwrap(),
    );
    assert_eq!(f.events.status(vars), ServerStatus::Starting);
    assert_eq!(f.manager.lead_of(buffer.id()), Some(css));
    assert_eq!(f.manager.server_of(buffer.id()), Some(css));
    assert_eq!(f.manager.servers_of(buffer.id()), [css, vars]);
    assert!(f.manager.is_served(buffer.id()));
    assert_eq!(f.manager.route_of(buffer.id()).as_deref(), Some("css"));

    // didOpen at the same text on both, at each one's next version.
    assert_eq!(f.wait_synced("css", &uri, &buffer).version, 1);
    assert_eq!(f.wait_synced("vars", &uri, &buffer).version, 0);
    let served = f.manager.served_identities(buffer.id());
    assert_eq!(
        served
            .iter()
            .map(|&(server, ..)| server)
            .collect::<Vec<_>>(),
        [css, vars]
    );
    for &(server, session, incarnation) in &served {
        assert_eq!(
            f.manager.active_identity(buffer.id(), server),
            Some((session, incarnation))
        );
    }

    // Each edit is a didChange on each, at its own next version.
    for n in 1..=3 {
        f.edit(&mut buffer, "x");
        assert_eq!(f.wait_synced("css", &uri, &buffer).version, 1 + n);
        assert_eq!(f.wait_synced("vars", &uri, &buffer).version, n);
    }
    // The primary's first two are the earlier buffer's open and close.
    let css_notes: Vec<_> = f.notes("css", &uri).into_iter().skip(2).collect();
    let vars_notes = f.notes("vars", &uri);
    let expected = |first: i64| {
        (0..4)
            .map(|n| {
                let method = if n == 0 { "didOpen" } else { "didChange" };
                (method.to_owned(), Some(first + n))
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(css_notes, expected(1));
    assert_eq!(vars_notes, expected(0));
    let ring = |server| super::answer(f.manager.ring_on(buffer.id(), Some(server)));
    let last = |ring: Vec<(i32, fe_buffer::Version)>| *ring.last().unwrap();
    assert_eq!(last(ring(css)), (4, buffer.version()));
    assert_eq!(last(ring(vars)), (3, buffer.version()));

    // A burst of edits: both replicas end at the buffer's text.
    for n in 0..20 {
        buffer
            .edit(
                [(buffer.text().len()..buffer.text().len(), "y")],
                Origin::User,
            )
            .unwrap();
        if n % 3 == 0 {
            f.manager.changed(buffer.id());
        }
    }
    f.manager.changed(buffer.id());
    f.wait_synced("css", &uri, &buffer);
    f.wait_synced("vars", &uri, &buffer);
    assert!(f.fake("css").sync_errors().is_empty());
    assert!(f.fake("vars").sync_errors().is_empty());

    // Closing closes it on both.
    f.manager.close(buffer.id());
    f.wait_closed("css", &uri);
    f.wait_closed("vars", &uri);
    assert!(f.manager.servers_of(buffer.id()).is_empty());
}

#[test]
fn max_file_bytes_closes_the_document_on_both() {
    let mut f = Fixture::css_and_vars();
    f.manager.set_max_file_bytes(16);
    let mut buffer = Buffer::new("small\n");
    let uri = f.open(&mut buffer, "a.css", CSS);
    f.wait_synced("css", &uri, &buffer);
    f.wait_synced("vars", &uri, &buffer);
    let (css, vars) = (
        f.placed(&buffer, "css").unwrap(),
        f.placed(&buffer, "vars").unwrap(),
    );

    // Grown past it: closed on both, and requests are refused.
    f.edit(&mut buffer, "seventeen bytes!\n");
    f.wait_closed("css", &uri);
    f.wait_closed("vars", &uri);
    assert_eq!(f.manager.active_identity(buffer.id(), css), None);
    assert_eq!(f.manager.active_identity(buffer.id(), vars), None);
    assert!(f.manager.served_identities(buffer.id()).is_empty());
    assert_eq!(
        echo_on(&f.manager, &buffer),
        Err(LspError::Unavailable(too_large()))
    );
    // Still placed on both, waiting to be under it again.
    assert_eq!(f.manager.servers_of(buffer.id()), [css, vars]);

    // Back under: open on both again, at the current text.
    buffer
        .edit([(0.."seventeen bytes!\n".len(), "")], Origin::User)
        .unwrap();
    f.manager.changed(buffer.id());
    f.wait_synced("css", &uri, &buffer);
    f.wait_synced("vars", &uri, &buffer);
    assert_eq!(f.manager.served_identities(buffer.id()).len(), 2);
}

#[test]
fn opened_over_the_limit_opens_on_neither() {
    let mut f = Fixture::css_and_vars();
    f.manager.set_max_file_bytes(16);
    let mut buffer = Buffer::new("seventeen bytes!\n");
    let uri = f.open(&mut buffer, "a.css", CSS);
    let (css, vars) = (
        f.placed(&buffer, "css").unwrap(),
        f.placed(&buffer, "vars").unwrap(),
    );
    f.barrier(css);
    f.barrier(vars);
    assert!(f.notes("css", &uri).is_empty());
    assert!(f.notes("vars", &uri).is_empty());
    assert_eq!(
        echo_on(&f.manager, &buffer),
        Err(LspError::Unavailable(too_large()))
    );
}

#[test]
fn aliases_are_placed_on_both_servers_by_one_holder() {
    let mut f = Fixture::css_and_vars();
    let mut a = Buffer::new("a {}\n");
    let mut b = Buffer::new("b {}\n");
    let uri = f.open(&mut a, "a.css", CSS);
    f.wait_synced("css", &uri, &a);
    f.wait_synced("vars", &uri, &a);
    let (css, vars) = (f.placed(&a, "css").unwrap(), f.placed(&a, "vars").unwrap());

    // The alias waits on both, whatever it does meanwhile.
    f.open(&mut b, "a.css", CSS);
    assert!(f.manager.servers_of(b.id()).is_empty());
    assert!(!f.manager.is_served(b.id()));
    assert_eq!(f.manager.route_of(b.id()).as_deref(), Some("css"));
    f.edit(&mut b, "edited ");
    f.barrier(css);
    f.barrier(vars);
    assert_eq!(f.fake("css").document(&uri).unwrap().text, "a {}\n");
    assert_eq!(f.fake("vars").document(&uri).unwrap().text, "a {}\n");

    // Closing the holder moves both servers to the alias, at its text.
    f.manager.close(a.id());
    f.wait_synced("css", &uri, &b);
    f.wait_synced("vars", &uri, &b);
    assert_eq!(f.manager.servers_of(b.id()), [css, vars]);
}
