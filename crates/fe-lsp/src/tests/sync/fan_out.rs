//! The subscription hub (docs/decisions/q5j03ax2af-lsp-add-on-servers.md
//! §4): one buffer subscription fanned out to a tap per server, each
//! draining the same events in order, a tap joining later starting at the
//! latest text drained, and `detach` stopping every tap.

use fe_buffer::{Buffer, BufferEvent, Origin, Version};

use crate::document::{Attached, Subscribed};

/// What a tap took, as (version, text) for an edit and the version for a
/// save.
#[derive(Debug, PartialEq, Eq)]
enum Seen {
    Edit(Version, String),
    Save(Version),
}

fn take(tap: &Attached) -> Vec<Seen> {
    tap.take_events()
        .into_iter()
        .map(|event| match event {
            BufferEvent::Edited { to, text, .. } => Seen::Edit(to, text.to_string()),
            BufferEvent::Saved { version } => Seen::Save(version),
        })
        .collect()
}

fn hub(buffer: &mut Buffer) -> Subscribed {
    let subscription = buffer.subscribe();
    Subscribed::new(subscription, buffer.text().clone(), buffer.version())
}

fn edit(buffer: &mut Buffer, at: usize, text: &str) -> Version {
    buffer
        .edit([(at..at, text)], Origin::User)
        .expect("the edit changed something");
    buffer.version()
}

#[test]
fn two_taps_drain_the_same_events_in_order() {
    let mut buffer = Buffer::new("one\n");
    let hub = hub(&mut buffer);
    let (a, a_text, a_version) = hub.tap();
    let (b, b_text, b_version) = hub.tap();
    assert_eq!(
        (a_text.to_string(), a_version),
        ("one\n".into(), buffer.version())
    );
    assert_eq!(
        (b_text.to_string(), b_version),
        ("one\n".into(), buffer.version())
    );

    let v1 = edit(&mut buffer, 0, "1");
    buffer.did_save();
    let v2 = edit(&mut buffer, 0, "2");
    // `a` drains first, and so drains the subscription for `b` too.
    assert_eq!(
        take(&a),
        [
            Seen::Edit(v1, "1one\n".into()),
            Seen::Save(v1),
            Seen::Edit(v2, "21one\n".into()),
        ]
    );
    let v3 = edit(&mut buffer, 0, "3");
    assert_eq!(
        take(&b),
        [
            Seen::Edit(v1, "1one\n".into()),
            Seen::Save(v1),
            Seen::Edit(v2, "21one\n".into()),
            Seen::Edit(v3, "321one\n".into()),
        ]
    );
    assert_eq!(take(&a), [Seen::Edit(v3, "321one\n".into())]);
    assert_eq!(take(&a), []);
    assert_eq!(take(&b), []);

    // Dropping a tap drops only that tap.
    let v4 = edit(&mut buffer, 0, "4");
    drop(a);
    assert_eq!(hub.taps(), 1);
    assert!(hub.is_attached());
    let v5 = edit(&mut buffer, 0, "5");
    assert_eq!(
        take(&b),
        [
            Seen::Edit(v4, "4321one\n".into()),
            Seen::Edit(v5, "54321one\n".into()),
        ]
    );
    // The last tap's drop drops the subscription: nothing drains it now.
    drop(b);
    assert_eq!(hub.taps(), 0);
    assert!(!hub.is_attached());
}

#[test]
fn late_tap_starts_at_the_latest_text() {
    let mut buffer = Buffer::new("one\n");
    let hub = hub(&mut buffer);
    let (a, _, _) = hub.tap();
    let v1 = edit(&mut buffer, 0, "1");
    buffer.did_save();
    let v2 = edit(&mut buffer, 0, "2");
    assert_eq!(take(&a).len(), 3);

    // Joins at the text `a` drained, with none of the events before it.
    let (b, text, version) = hub.tap();
    assert_eq!((text.to_string(), version), ("21one\n".into(), v2));
    assert!(v1 < version);
    assert_eq!(take(&b), []);

    // An edit still in the subscription when it joins is newer than that
    // text, and reaches it.
    let v3 = edit(&mut buffer, 0, "3");
    let (c, text, version) = hub.tap();
    assert_eq!((text.to_string(), version), ("21one\n".into(), v2));
    let v4 = edit(&mut buffer, 0, "4");
    let after = [
        Seen::Edit(v3, "321one\n".into()),
        Seen::Edit(v4, "4321one\n".into()),
    ];
    assert_eq!(take(&c), after);
    assert_eq!(take(&b), after);
    assert_eq!(take(&a), after);

    // Once drained, a tap joins at the newest text.
    let (_d, text, version) = hub.tap();
    assert_eq!((text.to_string(), version), ("4321one\n".into(), v4));
}

#[test]
fn detach_stops_both_taps() {
    let mut buffer = Buffer::new("one\n");
    let hub = hub(&mut buffer);
    let (a, _, _) = hub.tap();
    let (b, _, _) = hub.tap();
    let v1 = edit(&mut buffer, 0, "1");
    buffer.did_save();
    hub.detach();
    assert!(!hub.is_attached());

    // Edits after the detach reach neither tap; those before stay for
    // each tap's drains, in order.
    edit(&mut buffer, 0, "2");
    buffer.did_save();
    let before = [Seen::Edit(v1, "1one\n".into()), Seen::Save(v1)];
    assert_eq!(take(&b), before);
    assert_eq!(take(&a), before);
    edit(&mut buffer, 0, "3");
    assert_eq!(take(&a), []);
    assert_eq!(take(&b), []);

    // A tap joining after the detach starts at the text drained before
    // it, and never hears of later edits.
    let (c, text, version) = hub.tap();
    assert_eq!((text.to_string(), version), ("1one\n".into(), v1));
    edit(&mut buffer, 0, "4");
    assert_eq!(take(&c), []);
}
