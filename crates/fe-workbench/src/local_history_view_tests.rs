//! Local history: `history.show_local` lists a file's past texts, one opens
//! read-only against its buffer, and `history.restore_local` makes it the
//! buffer's text as one undoable edit.

use std::{
    collections::VecDeque,
    fs,
    path::PathBuf,
    time::{Duration, SystemTime},
};

use fe_buffer::{
    LocalHistory, Origin,
    local_history::{EntryId, Reason},
};
use fe_command::{CommandId, ContextPredicate};
use fe_gpui::{DiffView, EditorView};
use gpui::{Entity, TestAppContext, VisualTestContext};
use kiln::platform::command_actions::typed_command;
use kiln_shell::{Picker, command_palette::CommandPalette};

use super::local_history_view::{ListingHolds, restore_edits, stamp};
use crate::{
    Workbench,
    tests::find_bar::{draw, editor, editor_focused, opened, pane, run},
};

const SHOW: &str = "history.show_local";
const RESTORE: &str = "history.restore_local";

/// `a.txt` open with `text`, and a store installed, with the entries given.
struct Fixture<'a> {
    workbench: Entity<Workbench>,
    cx: &'a mut VisualTestContext,
    store: LocalHistory,
    path: PathBuf,
    tab: Entity<EditorView>,
    _dir: tempfile::TempDir,
    _store_dir: tempfile::TempDir,
}

fn fixture<'a>(cx: &'a mut TestAppContext, text: &str) -> Fixture<'a> {
    let (dir, workbench, cx) = opened(cx, &[("a.txt", text)]);
    let store_dir = tempfile::tempdir().unwrap();
    let store = LocalHistory::new(store_dir.path().join("history"));
    let installed = store.clone();
    cx.update(|_, cx| fe_gpui::pane_item::set_local_history(cx, Some(installed)));
    let tab = editor(&pane(&workbench, cx), cx);
    let path = tab.read_with(cx, |tab, _| tab.model().path().unwrap().to_owned());
    Fixture {
        workbench,
        cx,
        store,
        path,
        tab,
        _dir: dir,
        _store_dir: store_dir,
    }
}

impl Fixture<'_> {
    fn record(&self, text: &str, reason: Reason, ago: Duration) -> (EntryId, SystemTime) {
        let at = SystemTime::now() - ago;
        let id = self.store.record(&self.path, text.as_bytes(), reason, at);
        (id.unwrap().expect("the entry is recorded"), at)
    }

    /// The labels of the picker showing, if one is.
    fn listed(&mut self) -> Option<Vec<String>> {
        let picker = self
            .workbench
            .read_with(self.cx, |workbench, cx| workbench.local_history_picker(cx))?;
        Some(picker.read_with(self.cx, |picker, _| picker.delegate.labels()))
    }

    /// Lists the entries and picks the row `down` below the first.
    fn pick(&mut self, down: usize) {
        run(SHOW, self.cx);
        for _ in 0..down {
            self.cx.simulate_keystrokes("down");
        }
        draw(self.cx);
        self.cx.simulate_keystrokes("enter");
        draw(self.cx);
    }

    fn titles(&mut self) -> Vec<String> {
        self.workbench.read_with(self.cx, |workbench, cx| {
            (workbench.pane().read(cx).tabs().iter())
                .map(|tab| tab.item().tab_title(cx).to_string())
                .collect()
        })
    }

    fn diff(&mut self) -> Entity<DiffView> {
        self.workbench.read_with(self.cx, |workbench, cx| {
            (workbench.pane().read(cx).active_item())
                .and_then(|item| item.downcast_ref::<DiffView>().cloned())
                .expect("a diff view is shown")
        })
    }

    fn sides(&mut self) -> [Entity<EditorView>; 2] {
        let diff = self.diff();
        diff.read_with(self.cx, |diff, _| {
            [diff.left().clone(), diff.right().clone()]
        })
    }

    fn toasts(&mut self) -> Vec<String> {
        let layer = self
            .workbench
            .read_with(self.cx, |workbench, _| workbench.toasts().clone());
        layer.read_with(self.cx, |layer, _| {
            (layer.queue().shown().iter())
                .map(|entry| entry.toast.message.clone())
                .collect()
        })
    }

    /// The command IDs the palette lists where focus is now.
    fn palette(&mut self) -> Vec<String> {
        self.cx.simulate_keystrokes("cmd-shift-p");
        draw(self.cx);
        let picker: Entity<Picker<CommandPalette>> =
            self.workbench.read_with(self.cx, |workbench, cx| {
                let layer = workbench.shell().read(cx).modals().read(cx);
                layer.modal_view().unwrap().clone().downcast().unwrap()
            });
        let ids = picker.read_with(self.cx, |picker, _| {
            (picker.delegate.matches().iter())
                .map(|entry| entry.id.to_string())
                .collect()
        });
        self.cx.simulate_keystrokes("escape");
        draw(self.cx);
        ids
    }

    fn buffer_text(&mut self) -> String {
        self.tab
            .read_with(self.cx, |tab, _| tab.model().buffer().text().to_string())
    }
}

const MINUTE: Duration = Duration::from_secs(60);
const HOUR: Duration = Duration::from_secs(60 * 60);
const DAY: Duration = Duration::from_secs(24 * 60 * 60);

#[gpui::test]
fn show_history_lists_entries_newest_first(cx: &mut TestAppContext) {
    let mut f = fixture(cx, "now\n");
    let (_, oldest) = f.record("oldest text\n", Reason::Saved, 3 * DAY);
    f.record("an hour\n", Reason::External, HOUR + MINUTE);
    f.record("minutes\n", Reason::Saved, 5 * MINUTE);
    f.record("new\n", Reason::External, Duration::ZERO);

    run(SHOW, f.cx);
    assert_eq!(
        f.listed().expect("the picker shows"),
        [
            "just now · Changed on disk · 4 bytes".to_owned(),
            "5 minutes ago · Saved · 8 bytes".to_owned(),
            "1 hour ago · Changed on disk · 8 bytes".to_owned(),
            format!("{} · Saved · 12 bytes", stamp(oldest)),
        ]
    );

    // Typing filters; Escape closes it, back in the editor.
    f.cx.simulate_input("disk");
    draw(f.cx);
    assert_eq!(f.listed().unwrap().len(), 2);
    f.cx.simulate_keystrokes("escape");
    draw(f.cx);
    assert_eq!(f.listed(), None);
    assert!(editor_focused(&f.tab, f.cx));
    assert_eq!(f.titles(), ["a.txt"]);
}

#[gpui::test]
fn choosing_an_entry_opens_its_diff(cx: &mut TestAppContext) {
    let mut f = fixture(cx, "alpha\nbeta\n");
    let (_, old) = f.record("older\nbeta\n", Reason::Saved, 2 * DAY);
    f.record("newer\nbeta\n", Reason::Saved, MINUTE);

    f.pick(1);
    assert_eq!(f.listed(), None);
    let title = format!("a.txt (Local History {} ↔ Working Tree)", stamp(old));
    assert_eq!(f.titles(), ["a.txt".to_owned(), title]);
    let [left, right] = f.sides();
    left.read_with(f.cx, |left, _| {
        assert_eq!(left.model().buffer().text().to_string(), "older\nbeta\n");
        assert!(left.is_read_only());
    });
    let tab = f.tab.clone();
    right.read_with(f.cx, |right, cx| {
        assert_eq!(right.model().buffer().text().to_string(), "alpha\nbeta\n");
        assert!(!right.is_read_only());
        assert!(right.model().shares_buffer_with(tab.read(cx).model()));
    });
    let diff = f.diff();
    assert_eq!(diff.read_with(f.cx, |diff, _| diff.diff().hunks().len()), 1);
    assert!(editor_focused(&right, f.cx));

    // Picking it again shows that tab, focused, even from the tab's own side.
    f.pick(1);
    assert_eq!(f.titles().len(), 2);
    assert_eq!(f.diff(), diff);
    assert!(editor_focused(&right, f.cx));
}

/// Entries of one minute have one title, but each is its own comparison.
#[gpui::test]
fn entries_of_one_minute_open_their_own_diffs(cx: &mut TestAppContext) {
    let mut f = fixture(cx, "now\n");
    let at = SystemTime::now() - 2 * DAY;
    for text in ["first\n", "second\n"] {
        (f.store)
            .record(&f.path, text.as_bytes(), Reason::Saved, at)
            .unwrap()
            .unwrap();
    }

    let mut lefts = Vec::new();
    for down in [0, 1, 0] {
        f.pick(down);
        let [left, _] = f.sides();
        lefts.push(left.read_with(f.cx, |left, _| left.model().buffer().text().to_string()));
    }
    assert_eq!(lefts, ["second\n", "first\n", "second\n"]);
    assert_eq!(f.titles().len(), 3);
}

#[gpui::test]
fn restore_replaces_the_text_as_one_undo(cx: &mut TestAppContext) {
    let mut f = fixture(cx, "one\ntwo\nthree\nfour\n");
    f.record("ONE\ntwo\nthree\nFOUR\n", Reason::Saved, HOUR);
    let on_disk = fs::read_to_string(&f.path).unwrap();
    // Unsaved typing is its own undo step, kept under the restore.
    f.tab.update(f.cx, |tab, cx| {
        (tab.model_mut().buffer_mut()).edit([(4..4, "2")], Origin::User);
        cx.notify();
    });
    let before = "one\n2two\nthree\nfour\n";

    f.pick(0);
    let [_, right] = f.sides();
    // A caret on a row both texts have stays on it.
    let three = before.find("three").unwrap() + 2;
    right.update(f.cx, |right, _| {
        let model = right.model_mut();
        model.sync_display();
        model.set_selections([fe_buffer::ResolvedSelection {
            range: three..three,
            reversed: false,
        }]);
    });

    run(RESTORE, f.cx);
    let restored = "ONE\ntwo\nthree\nFOUR\n";
    assert_eq!(f.buffer_text(), restored);
    assert!(
        f.tab
            .read_with(f.cx, |tab, _| tab.model().buffer().is_dirty())
    );
    assert_eq!(fs::read_to_string(&f.path).unwrap(), on_disk);
    assert!(editor_focused(&right, f.cx));
    let diff = f.diff();
    assert_eq!(diff.read_with(f.cx, |diff, _| diff.diff().hunks().len()), 0);
    let caret = right.update(f.cx, |right, _| {
        let model = right.model_mut();
        model.sync_display();
        model.selections()[0].range.clone()
    });
    let moved = restored.find("three").unwrap() + 2;
    assert_eq!(caret, moved..moved);

    run("editor.undo", f.cx);
    assert_eq!(f.buffer_text(), before);
    run("editor.undo", f.cx);
    assert_eq!(f.buffer_text(), "one\ntwo\nthree\nfour\n");
}

/// An open transaction, a vim insert session's say, doesn't take the restore
/// in: undo takes back the restore alone.
#[gpui::test]
fn restore_is_apart_from_an_open_transaction(cx: &mut TestAppContext) {
    let mut f = fixture(cx, "one\ntwo\n");
    f.record("ONE\ntwo\n", Reason::Saved, HOUR);
    f.pick(0);
    let [_, right] = f.sides();
    right.update(f.cx, |right, cx| {
        let mut buffer = right.model_mut().buffer_mut();
        buffer.start_transaction(Origin::User);
        buffer.edit([(8..8, "typed\n")], Origin::User);
        drop(buffer);
        cx.notify();
    });
    draw(f.cx);

    run(RESTORE, f.cx);
    assert_eq!(f.buffer_text(), "ONE\ntwo\n");
    run("editor.undo", f.cx);
    assert_eq!(f.buffer_text(), "one\ntwo\ntyped\n");
}

#[gpui::test]
fn the_palette_lists_them_where_they_apply(cx: &mut TestAppContext) {
    let mut f = fixture(cx, "now\n");
    f.record("then\n", Reason::Saved, HOUR);
    let (show, restore) = (SHOW.to_owned(), RESTORE.to_owned());

    let listed = f.palette();
    assert!(listed.contains(&show));
    assert!(!listed.contains(&restore));

    // Run from the palette, it lists the entries once that's closed.
    f.cx.simulate_keystrokes("cmd-shift-p");
    draw(f.cx);
    f.cx.simulate_input("Local History: Show History");
    draw(f.cx);
    f.cx.simulate_keystrokes("enter");
    draw(f.cx);
    assert_eq!(f.listed().expect("the picker shows").len(), 1);
    f.cx.simulate_keystrokes("enter");
    draw(f.cx);

    let listed = f.palette();
    assert!(listed.contains(&show));
    assert!(listed.contains(&restore));

    // From the palette too, in an entry's diff.
    f.cx.simulate_keystrokes("cmd-shift-p");
    draw(f.cx);
    f.cx.simulate_input("Local History: Restore");
    draw(f.cx);
    f.cx.simulate_keystrokes("enter");
    draw(f.cx);
    assert_eq!(f.buffer_text(), "then\n");
}

#[gpui::test]
fn restore_outside_an_entrys_diff_changes_nothing(cx: &mut TestAppContext) {
    let mut f = fixture(cx, "now\n");
    f.record("then\n", Reason::Saved, HOUR);
    run(RESTORE, f.cx);
    assert_eq!(f.buffer_text(), "now\n");
    assert_eq!(f.toasts(), ["Open a Local History version to restore it"]);

    // Nor in a comparison with the saved text.
    run("diff.compare_saved", f.cx);
    run(RESTORE, f.cx);
    assert_eq!(f.buffer_text(), "now\n");
}

#[gpui::test]
fn no_entries_shows_a_toast(cx: &mut TestAppContext) {
    let mut f = fixture(cx, "now\n");

    // A store with nothing of the file.
    run(SHOW, f.cx);
    assert_eq!(f.listed(), None);
    assert_eq!(f.toasts(), ["No local history for a.txt"]);

    // No store at all.
    f.cx.update(|_, cx| fe_gpui::pane_item::set_local_history(cx, None));
    run(SHOW, f.cx);
    assert_eq!(f.listed(), None);
    assert_eq!(f.toasts().last().unwrap(), "No local history for a.txt");

    // An untitled buffer has no file.
    run("workbench.new_file", f.cx);
    run(SHOW, f.cx);
    assert_eq!(f.listed(), None);
    assert_eq!(f.toasts().last().unwrap(), "E32: No file name");
    assert_eq!(f.titles().len(), 2);
}

/// Holds the next listings' landing, in order, until each sender drops.
fn hold_listings(n: usize, cx: &mut VisualTestContext) -> Vec<async_channel::Sender<()>> {
    let (senders, receivers): (Vec<_>, VecDeque<_>) =
        (0..n).map(|_| async_channel::bounded(1)).unzip();
    cx.update(|_, cx| cx.set_global(ListingHolds(receivers)));
    senders
}

/// A listing landing after quick open opened over the editor shows no
/// picker: it would close quick open and take its focus.
#[gpui::test]
fn a_listing_landing_under_a_newer_modal_shows_nothing(cx: &mut TestAppContext) {
    let mut f = fixture(cx, "now\n");
    f.record("then\n", Reason::Saved, HOUR);
    let mut held = hold_listings(1, f.cx);
    run(SHOW, f.cx);
    assert_eq!(f.listed(), None);
    run("quick_open.toggle", f.cx);
    let quick_open = f.workbench.read_with(f.cx, |workbench, cx| {
        workbench.shell().read(cx).modals().read(cx).modal()
    });
    assert!(quick_open.is_some());

    drop(held.pop());
    draw(f.cx);
    assert_eq!(f.listed(), None);
    let shown = f.workbench.read_with(f.cx, |workbench, cx| {
        workbench.shell().read(cx).modals().read(cx).modal()
    });
    assert_eq!(shown, quick_open);
    assert!(!editor_focused(&f.tab, f.cx));
}

/// Of two listings, the older landing last doesn't replace the newer's.
#[gpui::test]
fn an_older_listing_landing_late_is_dropped(cx: &mut TestAppContext) {
    let mut f = fixture(cx, "now\n");
    f.record("then\n", Reason::Saved, HOUR);
    let mut held = hold_listings(2, f.cx);
    run(SHOW, f.cx);
    f.record("newer\n", Reason::External, 5 * MINUTE);
    run(SHOW, f.cx);
    assert_eq!(f.listed(), None);

    // The newer lands first, with both entries.
    drop(held.pop());
    draw(f.cx);
    assert_eq!(f.listed().expect("the newer picker shows").len(), 2);

    // The older, with one, is dropped.
    drop(held.pop());
    draw(f.cx);
    assert_eq!(f.listed().expect("the newer picker still shows").len(), 2);
}

/// A version deleted while the picker was open answers, opening no tab.
#[gpui::test]
fn an_entry_gone_when_picked_answers(cx: &mut TestAppContext) {
    let mut f = fixture(cx, "now\n");
    f.record("then\n", Reason::Saved, HOUR);
    run(SHOW, f.cx);
    assert!(f.listed().is_some());
    let store_dir = f.store.dir().to_owned();
    fs::remove_dir_all(store_dir).unwrap();
    f.cx.simulate_keystrokes("enter");
    draw(f.cx);
    assert_eq!(f.titles(), ["a.txt"]);
    assert_eq!(f.toasts(), ["Couldn't read that version of a.txt"]);
}

#[gpui::test]
fn both_commands_are_typed_commands(cx: &mut TestAppContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    cx.read(|cx| {
        for (id, title, context) in [
            (SHOW, "Local History: Show History", "editor"),
            (RESTORE, "Local History: Restore This Version", "DiffView"),
        ] {
            let command = typed_command(&CommandId::parse(id).unwrap(), cx);
            let command = command.unwrap_or_else(|| panic!("{id} is a typed command"));
            assert_eq!(command.title, title);
            assert_eq!(
                command.context,
                Some(ContextPredicate::parse(context).unwrap())
            );
        }
    });
}

/// Applies `restore_edits(old, new)` as a buffer's edit does.
fn applied(old: &str, new: &str) -> String {
    let mut text = old.to_owned();
    for (range, with) in restore_edits(old, new).into_iter().rev() {
        text.replace_range(range, &with);
    }
    text
}

#[test]
fn restore_edits_turn_the_old_text_into_the_new() {
    for (old, new) in [
        ("", ""),
        ("", "a\n"),
        ("a\n", ""),
        ("a", "a\n"),
        ("a\n", "a"),
        ("a\nb", "a\nb\nc"),
        ("a\nb\nc\n", "a\nc\n"),
        ("a\nb\nc\nd\n", "A\nb\nc\nD\n"),
        ("é\nñandú\n", "é\nñandù\n"),
        ("日本語\n", "日本人\n"),
        ("x\n\n\ny\n", "x\n\ny\n"),
        ("same\n", "same\n"),
    ] {
        assert_eq!(applied(old, new), new, "{old:?} to {new:?}");
    }
}

#[test]
fn restore_edits_leave_what_both_texts_have() {
    assert_eq!(restore_edits("same\n", "same\n"), []);
    // Two hunks, each cut to the bytes that differ.
    assert_eq!(
        restore_edits("one\ntwo\nthree\n", "oNe\ntwo\nthrEE\n"),
        [(1..2, "N".to_owned()), (11..13, "EE".to_owned())]
    );
    // A shared lead byte of two different chars isn't cut through.
    assert_eq!(restore_edits("é\n", "è\n"), [(0..2, "è".to_owned())]);
}
