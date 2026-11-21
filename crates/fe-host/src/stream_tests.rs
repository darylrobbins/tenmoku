use std::{
    sync::{atomic::Ordering, mpsc},
    thread,
};

use super::*;

#[test]
fn items_then_the_end_value() {
    let (tx, stream) = Stream::<u32, &str>::channel(4);
    assert!(tx.send(1));
    assert!(tx.send(2));
    tx.end("done");
    assert_eq!(stream.recv(), Some(1));
    assert_eq!(stream.recv(), Some(2));
    assert_eq!(stream.recv(), None);
    assert_eq!(stream.finish(), Some("done"));
}

#[test]
fn a_sender_dropped_without_an_end_ends_with_none() {
    let (tx, stream) = Stream::<u32, ()>::channel(1);
    drop(tx);
    assert_eq!(stream.recv(), None);
    assert_eq!(stream.finish(), None);
}

#[test]
fn dropping_the_stream_sets_the_cancel_flag() {
    let (tx, stream) = Stream::<u32, ()>::channel(1);
    let flag = tx.cancel_flag();
    assert!(!tx.is_cancelled());
    drop(stream);
    assert!(flag.load(Ordering::Relaxed));
    assert!(tx.is_cancelled());
    assert!(!tx.send(1));
}

#[test]
fn dropping_the_stream_wakes_a_blocked_sender() {
    let (tx, stream) = Stream::<u32, bool>::channel(0);
    let ended = stream.ended();
    let (sending, sent) = mpsc::channel();
    let producer = thread::spawn(move || {
        sending.send(()).unwrap();
        // Blocks: nothing receives.
        let delivered = tx.send(1);
        tx.end(delivered);
    });
    sent.recv().unwrap();
    drop(stream);
    assert_eq!(ended.wait(), Some(false));
    producer.join().unwrap();
}

#[test]
fn finish_reads_the_rest_without_cancelling() {
    let (tx, stream) = Stream::<u32, u32>::channel(0);
    let producer = thread::spawn(move || {
        let mut sent = 0;
        for item in 0..10 {
            if tx.send(item) {
                sent += 1;
            }
        }
        tx.end(sent);
    });
    assert_eq!(stream.finish(), Some(10));
    producer.join().unwrap();
}

#[test]
fn cancel_waits_for_the_end() {
    let (tx, stream) = Stream::<u32, u32>::channel(0);
    let producer = thread::spawn(move || {
        let mut sent = 0;
        while tx.send(sent) {
            sent += 1;
        }
        tx.end(sent);
    });
    assert_eq!(stream.recv(), Some(0));
    let sent = stream.cancel().unwrap();
    assert!(sent >= 1);
    producer.join().unwrap();
}

#[test]
fn ended_with_has_no_items() {
    let stream = Stream::<u32, &str>::ended_with("refused");
    assert!(stream.ended().is_done());
    assert_eq!(stream.recv(), None);
    assert_eq!(stream.finish(), Some("refused"));
}

#[test]
fn a_subscription_drops_its_source_when_dropped() {
    struct Source(mpsc::Sender<()>);
    impl Drop for Source {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }
    let (tx, rx) = mpsc::channel();
    tx.send(7).unwrap();
    let (dropped_tx, dropped) = mpsc::channel();
    let subscription = Subscription::new(rx, Source(dropped_tx));
    assert_eq!(subscription.recv(), Some(7));
    assert!(dropped.try_recv().is_err());
    drop(subscription);
    dropped.recv().unwrap();
    assert!(tx.send(8).is_err());
}
