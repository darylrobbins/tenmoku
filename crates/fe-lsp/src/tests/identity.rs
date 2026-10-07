//! Document identity (§4.1): sessions, incarnations, LSP versions across
//! reopens, and the active-identity table against stale opens and closes.

use std::collections::HashSet;
use std::path::Path;

use fe_buffer::Buffer;

use super::{ROOT, Started, wait_until};
use crate::fake::{Script, Writes};
use crate::identity::{Identities, Point};
use crate::{LspEvent, ServerId, ServerSpec, ServerStatus};

#[test]
fn versions_and_incarnations() {
    let mut s = Started::new(Script::default());
    let mut seen = HashSet::new();
    let mut a = Buffer::new("a\n");

    // Active before the fake sees didOpen: its write is held up.
    s.fake.set_writes(Writes::Stepped);
    let a_uri = s.open(&mut a, "a.txt");
    wait_until("the didOpen write", || s.fake.write_waiting());
    let first = s.manager.active_identity(a.id(), s.server);
    assert!(first.is_some());
    assert_eq!(s.fake.document(&a_uri), None);
    s.fake.set_writes(Writes::Open);
    let replica = s
        .fake
        .wait_for_document(&a_uri, "didOpen", |r| r.is_some_and(|r| r.open))
        .unwrap();
    assert_eq!(replica.version, 0);
    assert!(seen.insert(first.unwrap()));
    s.edit(&mut a, &a_uri, (0..0, "1"), 1);
    s.edit(&mut a, &a_uri, (0..0, "2"), 2);

    // Gone as soon as close returns, with didClose not yet sent.
    s.fake.set_writes(Writes::Held);
    s.manager.close(a.id());
    assert_eq!(s.manager.active_identity(a.id(), s.server), None);
    s.fake.set_writes(Writes::Open);
    s.fake
        .wait_for_document(&a_uri, "didClose", |r| r.is_some_and(|r| !r.open));
    assert_eq!(s.manager.active_identity(a.id(), s.server), None);

    // A reopen goes on from the URI's last version, as a new incarnation
    // of the same session; another URI starts at 0.
    let session = first.unwrap().0;
    for expected in [3, 4] {
        s.open(&mut a, "a.txt");
        s.fake
            .wait_for_document(&a_uri, "the reopen", |r| r.is_some_and(|r| r.open));
        assert_eq!(s.fake.document(&a_uri).unwrap().version, expected);
        let identity = s.manager.active_identity(a.id(), s.server).unwrap();
        assert_eq!(identity.0, session);
        assert!(seen.insert(identity), "{identity:?} reused");
        s.manager.close(a.id());
        s.fake
            .wait_for_document(&a_uri, "didClose", |r| r.is_some_and(|r| !r.open));
    }
    let mut b = Buffer::new("b\n");
    let b_uri = s.open_synced(&mut b, "b.txt");
    assert_eq!(s.fake.document(&b_uri).unwrap().version, 0);
    assert!(seen.insert(s.manager.active_identity(b.id(), s.server).unwrap()));

    // A new session of the same server: new ids, and versions from 0.
    s.manager.stop(s.server);
    assert_eq!(s.manager.active_identity(b.id(), s.server), None);
    assert_eq!(s.events.status(s.server), ServerStatus::Stopped(None));
    let again = s
        .manager
        .start(ServerSpec::new("fake", "fake-server"), Path::new(ROOT))
        .unwrap();
    assert_ne!(again, s.server);
    assert_eq!(s.events.status(again), ServerStatus::Starting);
    s.fake.wait_initialized_at(2);
    s.server = again;
    let mut c = Buffer::new("c\n");
    let c_uri = s.open(&mut c, "a.txt");
    s.fake
        .wait_for_document(&c_uri, "the new session's didOpen", |r| {
            r.is_some_and(|r| r.open)
        });
    assert_eq!(s.fake.document(&c_uri).unwrap().version, 0);
    let identity = s.manager.active_identity(c.id(), again).unwrap();
    assert_ne!(identity.0, session);
    assert!(seen.insert(identity), "{identity:?} reused");

    // Ending a session (a crash, here) retires that session's identities
    // and no others, before its end is reported.
    let other = s
        .manager
        .start(ServerSpec::new("other", "fake-server"), Path::new(ROOT))
        .unwrap();
    s.fake.wait_initialized_at(3);
    let mut d = Buffer::new("d\n");
    let d_uri = s.open_on(other, &mut d, "d.txt");
    s.fake
        .wait_for_document(&d_uri, "didOpen", |r| r.is_some_and(|r| r.open));
    let on_other = s.manager.active_identity(d.id(), other).unwrap();
    assert!(seen.insert(on_other), "{on_other:?} reused");
    let (identities, buffers) = (s.manager.identities().clone(), (c.id(), d.id()));
    let (seen_at_end, at_end) = std::sync::mpsc::channel();
    s.events.probe(move |event| {
        if let LspEvent::Status {
            server,
            status: ServerStatus::Restarting(_),
            ..
        } = event
            && *server == again
        {
            let active = (
                identities.active(buffers.0, again),
                identities.active(buffers.1, other),
            );
            let _ = seen_at_end.send(active);
        }
    });
    s.fake.crash_at(2);
    assert_eq!(s.events.status(again), ServerStatus::Restarting(1));
    assert_eq!(at_end.recv().unwrap(), (None, Some(on_other)));
    assert_eq!(s.manager.active_identity(c.id(), again), None);
    assert_eq!(s.manager.active_identity(d.id(), other), Some(on_other));
}

#[test]
fn stale_opens_and_closes() {
    let mut s = Started::new(Script::default());
    let identities = s.manager.identities().clone();
    let pauses = identities.pauses();

    // Paused before activating, then closed: nothing activates or goes.
    let mut a = Buffer::new("a\n");
    pauses.arm(Point::Activate);
    let a_uri = s.open(&mut a, "a.txt");
    pauses.wait_held(Point::Activate);
    s.manager.close(a.id());
    pauses.resume();
    s.barrier();
    assert_eq!(s.manager.active_identity(a.id(), s.server), None);
    assert_eq!(s.methods(&a_uri), Vec::<String>::new());

    // Closed and reopened before the paused open resumes: only the new
    // open activates.
    let mut b = Buffer::new("b\n");
    pauses.arm(Point::Activate);
    let b_uri = s.open(&mut b, "b.txt");
    pauses.wait_held(Point::Activate);
    s.manager.close(b.id());
    s.open(&mut b, "b.txt");
    let generation = identities.generation(b.id());
    pauses.resume();
    s.barrier();
    assert_eq!(s.methods(&b_uri), ["didOpen"]);
    assert!(s.manager.active_identity(b.id(), s.server).is_some());
    assert_eq!(identities.generation(b.id()), generation);

    // A size-limit close paused before retiring, with the buffer closed
    // and reopened (back under the limit) meanwhile: the new open's
    // generation and entry are untouched, and it activates.
    s.manager.set_max_file_bytes(8);
    let mut c = Buffer::new("c\n");
    let c_uri = s.open_synced(&mut c, "c.txt");
    let first = s.manager.active_identity(c.id(), s.server).unwrap();
    pauses.arm(Point::Retire);
    c.edit([(0..0, "too long now")], fe_buffer::Origin::User)
        .unwrap();
    s.manager.changed(c.id());
    pauses.wait_held(Point::Retire);
    s.manager.close(c.id());
    c.edit([(0..12, "")], fe_buffer::Origin::User).unwrap();
    s.open(&mut c, "c.txt");
    let generation = identities.generation(c.id());
    pauses.resume();
    s.barrier();
    assert_eq!(s.methods(&c_uri), ["didOpen", "didClose", "didOpen"]);
    let replica = s.fake.document(&c_uri).unwrap();
    assert_eq!((replica.version, replica.text.as_str()), (1, "c\n"));
    let second = s.manager.active_identity(c.id(), s.server).unwrap();
    assert_ne!(second, first);
    assert_eq!(identities.generation(c.id()), generation);

    // The same retirement against the table alone, with the new open
    // already active when it runs.
    let table = Identities::default();
    let (buffer, server, session) = (c.id(), ServerId(0), table.new_session());
    let old = table.open(buffer);
    let first = (
        session,
        table.activate(buffer, old, server, session).unwrap(),
    );
    table.close(buffer);
    let new = table.open(buffer);
    let second = (
        session,
        table.activate(buffer, new, server, session).unwrap(),
    );
    table.retire(buffer, old, server, first);
    table.retire(buffer, new, server, first);
    assert_eq!(table.active(buffer, server), Some(second));
    assert_eq!(table.generation(buffer), Some(new));
    // Its own, still current, it does retire, without a new generation.
    table.retire(buffer, new, server, second);
    assert_eq!(table.active(buffer, server), None);
    assert_eq!(table.generation(buffer), Some(new));
}
