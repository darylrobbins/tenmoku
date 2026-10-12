use std::time::{Duration, Instant};

use fe_editor::auto_save::{AutoSaveMode, AutoSavePolicy, Facts, SaveReason};

const FILE: Facts = Facts {
    has_path: true,
    read_only: false,
    vim_insert: false,
};
const INSERT: Facts = Facts {
    vim_insert: true,
    ..FILE
};

fn secs(base: Instant, ms: u64) -> Instant {
    base + Duration::from_millis(ms)
}

#[test]
fn after_delay() {
    let b = Instant::now();
    let mut p = AutoSavePolicy::new(AutoSaveMode::AfterDelay(Duration::from_secs(1)));
    p.edited(1u32, secs(b, 0), FILE);
    p.edited(1, secs(b, 500), FILE);
    assert_eq!(p.next_deadline(), Some(secs(b, 1500)));
    assert!(p.due(secs(b, 1499)).is_empty());
    assert_eq!(p.due(secs(b, 1500)), vec![1]);
    assert!(p.due(secs(b, 9000)).is_empty());
    assert_eq!(p.next_deadline(), None);

    p.edited(2, secs(b, 2000), INSERT);
    assert_eq!(p.next_deadline(), None);
    assert!(p.due(secs(b, 4000)).is_empty());
    p.insert_left(2, secs(b, 5000));
    assert!(p.due(secs(b, 5999)).is_empty());
    assert_eq!(p.due(secs(b, 6000)), vec![2]);
}

#[test]
fn focus_and_window() {
    let b = Instant::now();
    let t = secs(b, 0);
    let mut p = AutoSavePolicy::new(AutoSaveMode::OnFocusChange);
    p.edited(1u32, t, FILE);
    p.edited(2, t, FILE);
    assert!(p.due(t).is_empty());
    p.focus_lost(1);
    assert_eq!(p.due(t), vec![1]);
    p.window_blurred();
    assert_eq!(p.due(t), vec![2]);

    let mut p = AutoSavePolicy::new(AutoSaveMode::OnWindowChange);
    p.edited(1u32, t, FILE);
    p.focus_lost(1);
    assert!(p.due(t).is_empty());
    p.window_blurred();
    assert_eq!(p.due(t), vec![1]);

    let mut p = AutoSavePolicy::new(AutoSaveMode::OnInsertLeave);
    p.edited(1u32, t, INSERT);
    assert!(p.due(t).is_empty());
    p.insert_left(1, t);
    assert_eq!(p.due(t), vec![1]);
    p.edited(1, t, FILE);
    assert_eq!(p.due(t), vec![1]);
    // Vim never seen for buffer 2: behaves as OnFocusChange.
    p.edited(2, t, FILE);
    assert!(p.due(t).is_empty());
    p.focus_lost(2);
    assert_eq!(p.due(t), vec![2]);

    let mut p = AutoSavePolicy::new(AutoSaveMode::Off);
    p.edited(1u32, t, FILE);
    p.focus_lost(1);
    p.window_blurred();
    p.insert_left(1, t);
    assert!(p.due(secs(b, 100_000)).is_empty());
}

#[test]
fn exclusions() {
    let b = Instant::now();
    let t = secs(b, 0);
    let d = Duration::from_secs(1);
    let mut p = AutoSavePolicy::new(AutoSaveMode::AfterDelay(d));
    p.edited(
        1u32,
        t,
        Facts {
            has_path: false,
            ..FILE
        },
    );
    p.edited(
        2,
        t,
        Facts {
            read_only: true,
            ..FILE
        },
    );
    assert!(p.due(secs(b, 5000)).is_empty());
    assert_eq!(p.next_deadline(), None);

    p.edited(3, t, FILE);
    p.save_failed(3, true);
    p.edited(3, secs(b, 100), FILE);
    assert!(p.due(secs(b, 5000)).is_empty());
    p.reloaded(3);
    p.edited(3, secs(b, 6000), FILE);
    assert_eq!(p.due(secs(b, 7000)), vec![3]);

    p.edited(4, t, FILE);
    p.save_failed(4, false);
    assert!(p.due(secs(b, 5000)).is_empty());
    p.edited(4, secs(b, 8000), FILE);
    assert_eq!(p.due(secs(b, 9000)), vec![4]);

    p.edited(5, secs(b, 10_000), FILE);
    p.set_mode(AutoSaveMode::Off);
    assert_eq!(p.next_deadline(), None);
    assert!(p.due(secs(b, 20_000)).is_empty());
}

#[test]
fn edits_allowed() {
    for v in [false, true] {
        assert!(!SaveReason::AutoDelay.edits_allowed(v));
    }
    assert!(SaveReason::Explicit.edits_allowed(false));
    for r in [
        SaveReason::AutoFocus,
        SaveReason::AutoWindow,
        SaveReason::AutoInsertLeave,
    ] {
        assert!(r.edits_allowed(false));
        assert!(!r.edits_allowed(true));
    }
}

#[test]
fn conflict_hold_survives_other_failures() {
    let b = Instant::now();
    let mut p = AutoSavePolicy::new(AutoSaveMode::AfterDelay(Duration::from_secs(1)));
    p.edited(1u32, secs(b, 0), FILE);
    p.save_failed(1, true);
    p.save_failed(1, false);
    p.edited(1, secs(b, 100), FILE);
    assert!(p.due(secs(b, 5000)).is_empty());
}

#[test]
fn insert_leave_without_vim_follows_window() {
    let t = Instant::now();
    let mut p = AutoSavePolicy::new(AutoSaveMode::OnInsertLeave);
    p.edited(1u32, t, FILE);
    p.edited(2, t, INSERT);
    p.window_blurred();
    assert_eq!(p.due(t), vec![1]);
}

#[test]
fn set_mode_anchors_on_insert_leave() {
    let b = Instant::now();
    let mut p = AutoSavePolicy::new(AutoSaveMode::AfterDelay(Duration::from_secs(1)));
    p.edited(1u32, secs(b, 0), INSERT);
    p.insert_left(1, secs(b, 5000));
    p.set_mode(AutoSaveMode::AfterDelay(Duration::from_secs(2)));
    assert_eq!(p.next_deadline(), Some(secs(b, 7000)));
}
