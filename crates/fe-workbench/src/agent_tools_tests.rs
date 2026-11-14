//! The API chat's workbench tools ([`super`]): their caps and truncation
//! lines, the deny list, refusals outside the root, the pending-proposal
//! view `read_file` returns, overlapping edits refused, and supersession
//! across turns (the agents record's § 12).
//!
//! [`ChatProposals`] is tested over plain buffers; the executors over a
//! workbench on a temp folder of `.txt` and `.go` files, so no language
//! server starts.

#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, atomic::AtomicBool},
};

use fe_buffer::{Buffer, BufferId, DiskStamp, HunkStatus, Origin, ProposedChange};
use fe_chat::{ListFilesInput, ProposeEditInput, ProposedEdit, ReadFileInput, SearchInput};
use fe_editor::{BufferSet, Changeset};
use fe_gpui::EditorView;
use gpui::{Entity, TestAppContext, VisualTestContext};

use super::*;
use crate::{Launch, Workbench, tests::open};

fn edit(old_text: &str, new_text: &str) -> ProposedEdit {
    ProposedEdit {
        old_text: old_text.to_owned(),
        new_text: new_text.to_owned(),
    }
}

fn ai() -> Origin {
    Origin::Ai {
        agent: Arc::from("fastedit.chat"),
        session: Arc::from("chat"),
    }
}

/// A folder under the real temp folder.
fn folder() -> tempfile::TempDir {
    let temp = dunce::canonicalize(std::env::temp_dir()).unwrap();
    tempfile::tempdir_in(temp).unwrap()
}

fn never() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

fn list(root: &Path, dir: Option<&str>, glob: Option<&str>) -> ToolOutcome {
    let input = ListFilesInput {
        dir: dir.map(str::to_owned),
        glob: glob.map(str::to_owned),
    };
    list_files(root, &input, never())
}

fn find(root: &Path, pattern: &str) -> ToolOutcome {
    let input = SearchInput {
        pattern: pattern.to_owned(),
        regex: false,
        case_sensitive: true,
        glob: None,
    };
    search(root, &input, never())
}

/// The buffers a test's changesets reach.
struct Buffers(Vec<Buffer>);

impl BufferSet for Buffers {
    fn buffer_mut(&mut self, id: BufferId) -> Option<&mut Buffer> {
        self.0.iter_mut().find(|buffer| buffer.id() == id)
    }
}

// ---------------------------------------------------------------------------
// Caps and truncation lines

#[test]
fn list_output_caps_at_a_thousand_paths() {
    let paths: Vec<String> = (0..1003).map(|ix| format!("f{ix:04}.txt")).collect();
    let out = list_output(&paths);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), MAX_PATHS + 1);
    assert_eq!(lines[MAX_PATHS - 1], "f0999.txt");
    assert_eq!(lines[MAX_PATHS], "[truncated: 3 more]");
    let few = list_output(&paths[..2]);
    assert_eq!(few, "f0000.txt\nf0001.txt");
}

#[test]
fn list_files_of_a_big_folder_is_truncated() {
    let dir = folder();
    for ix in 0..1005 {
        fs::write(dir.path().join(format!("f{ix:04}.txt")), "x").unwrap();
    }
    let out = list(dir.path(), None, None).unwrap();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), MAX_PATHS + 1);
    assert_eq!(lines[0], "f0000.txt");
    assert_eq!(lines[MAX_PATHS], "[truncated: 5 more]");
}

#[test]
fn list_files_under_a_folder_and_by_glob() {
    let dir = folder();
    fs::create_dir_all(dir.path().join("src/deep")).unwrap();
    fs::write(dir.path().join("top.txt"), "").unwrap();
    fs::write(dir.path().join("src/main.go"), "").unwrap();
    fs::write(dir.path().join("src/notes.txt"), "").unwrap();
    fs::write(dir.path().join("src/deep/lib.go"), "").unwrap();
    assert_eq!(
        list(dir.path(), Some("src"), None).unwrap(),
        "src/deep/lib.go\nsrc/main.go\nsrc/notes.txt"
    );
    assert_eq!(
        list(dir.path(), None, Some("*.go")).unwrap(),
        "src/deep/lib.go\nsrc/main.go"
    );
    assert_eq!(
        list(dir.path(), None, Some("**/*.txt")).unwrap(),
        "src/notes.txt\ntop.txt"
    );
    assert_eq!(
        list(dir.path(), None, Some("src/*.go")).unwrap(),
        "src/main.go"
    );
    assert_eq!(
        list(dir.path(), None, Some("deep/")).unwrap(),
        "src/deep/lib.go"
    );
    assert_eq!(list(dir.path(), None, Some("*.md")).unwrap(), "[no files]");
    assert!(list(dir.path(), None, Some("[ab")).is_err());
    assert!(list(dir.path(), Some("top.txt"), None).is_err());
}

#[test]
fn read_output_caps_at_two_thousand_lines() {
    let text: String = (1..=2500).map(|row| format!("line {row}\n")).collect();
    let out = read_output(&text, None, None, &View::default()).unwrap();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), MAX_READ_LINES + 1);
    assert_eq!(lines[MAX_READ_LINES - 1], "line 2000");
    assert_eq!(
        lines[MAX_READ_LINES],
        "[truncated: 500 more lines; read from line 2001]"
    );
    // Reading on from there gets the rest, untruncated.
    let rest = read_output(&text, Some(2001), None, &View::default()).unwrap();
    assert_eq!(rest.lines().count(), 500);
    assert!(rest.starts_with("line 2001\n") && rest.ends_with("line 2500\n"));
}

#[test]
fn read_output_caps_at_64_kib() {
    // 100 lines of 1 KiB each, newline included: 64 fit.
    let line = format!("{}\n", "x".repeat(1023));
    let text = line.repeat(100);
    let out = read_output(&text, None, None, &View::default()).unwrap();
    let (body, last) = out.rsplit_once('\n').unwrap();
    assert_eq!(body.len() + 1, MAX_READ_BYTES);
    assert_eq!(last, "[truncated: 36 more lines; read from line 65]");
    // A range is cut at its end, not the file's.
    let some = read_output(&text, Some(10), Some(80), &View::default()).unwrap();
    assert!(some.ends_with("[truncated: 7 more lines; read from line 74]"));
}

#[test]
fn read_output_ranges_and_headers() {
    let text = "a\nb\nc\nd";
    let none = View::default();
    assert_eq!(
        read_output(text, Some(2), Some(3), &none).unwrap(),
        "b\nc\n"
    );
    assert_eq!(read_output(text, Some(4), None, &none).unwrap(), "d");
    assert_eq!(read_output(text, Some(3), Some(99), &none).unwrap(), "c\nd");
    assert!(read_output(text, Some(0), None, &none).is_err());
    assert!(read_output(text, Some(5), None, &none).is_err());
    assert!(read_output(text, Some(3), Some(2), &none).is_err());
    assert_eq!(read_output("", None, None, &none).unwrap(), "");
    let view = View {
        text: None,
        pending: 2,
        stale: 1,
    };
    assert_eq!(
        read_output("a\n", None, None, &view).unwrap(),
        "[includes 2 pending changes you proposed]\n\
         [1 of your earlier changes no longer apply]\na\n"
    );
}

#[test]
fn search_caps_at_two_hundred_lines() {
    let dir = folder();
    let text: String = (1..=250).map(|row| format!("hit {row}\n")).collect();
    fs::write(dir.path().join("many.txt"), text).unwrap();
    let out = find(dir.path(), "hit").unwrap();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), MAX_SEARCH_LINES + 1);
    assert_eq!(lines[0], "many.txt:1: hit 1");
    assert_eq!(lines[MAX_SEARCH_LINES - 1], "many.txt:200: hit 200");
    assert_eq!(lines[MAX_SEARCH_LINES], "[truncated]");
    assert_eq!(find(dir.path(), "absent").unwrap(), "[no matches]");
    assert_eq!(
        find(dir.path(), "hit 25\n").ok(),
        None,
        "a line break is invalid"
    );
}

#[test]
fn search_output_caps_at_64_kib_and_cuts_previews() {
    let dir = folder();
    // 150 lines over 512 bytes: each preview is cut to 512.
    let long = format!("hit {}\n", "y".repeat(1000));
    fs::write(dir.path().join("long.txt"), long.repeat(150)).unwrap();
    let out = find(dir.path(), "hit").unwrap();
    let (body, last) = out.rsplit_once('\n').unwrap();
    assert_eq!(last, "[truncated]");
    assert!(body.len() <= MAX_SEARCH_BYTES);
    let first = body.lines().next().unwrap();
    assert_eq!(first.len(), "long.txt:1: ".len() + 512);
    assert!(body.lines().count() < MAX_SEARCH_LINES);
}

// ---------------------------------------------------------------------------
// The deny list

/// A folder holding a text file and the names the deny list keeps out.
fn with_secrets() -> tempfile::TempDir {
    let dir = folder();
    for name in [
        "notes.txt",
        ".env",
        ".env.local",
        "id_rsa",
        "server.pem",
        "tls.key",
    ] {
        fs::write(dir.path().join(name), "secret here\n").unwrap();
    }
    fs::create_dir_all(dir.path().join(".git")).unwrap();
    fs::write(dir.path().join(".git/config"), "secret here\n").unwrap();
    fs::create_dir_all(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("sub/id_ed25519"), "secret here\n").unwrap();
    dir
}

#[test]
fn deny_list_keeps_secrets_out_of_listings_and_searches() {
    let dir = with_secrets();
    assert_eq!(list(dir.path(), None, None).unwrap(), "notes.txt");
    assert_eq!(list(dir.path(), None, Some("*")).unwrap(), "notes.txt");
    assert_eq!(
        find(dir.path(), "secret").unwrap(),
        "notes.txt:1: secret here"
    );
    let globbed = SearchInput {
        pattern: "secret".to_owned(),
        regex: false,
        case_sensitive: false,
        glob: Some("id_*".to_owned()),
    };
    assert_eq!(
        search(dir.path(), &globbed, never()).unwrap(),
        "[no matches]"
    );
    assert!(list(dir.path(), Some(".git"), None).is_err());
}

#[test]
fn deny_list_refuses_paths_as_written_and_as_they_really_are() {
    let dir = with_secrets();
    for name in [
        ".env",
        ".env.local",
        "id_rsa",
        "server.pem",
        "tls.key",
        ".git/config",
        "sub/id_ed25519",
    ] {
        assert!(resolve(dir.path(), name).is_err(), "{name} is refused");
    }
    assert!(resolve(dir.path(), "notes.txt").is_ok());
    #[cfg(unix)]
    {
        // A link to a secret, and a secret's name on a link to text.
        symlink(dir.path().join(".env"), dir.path().join("plain.txt")).unwrap();
        symlink(dir.path().join("notes.txt"), dir.path().join("deploy.pem")).unwrap();
        assert!(resolve(dir.path(), "plain.txt").is_err());
        assert!(resolve(dir.path(), "deploy.pem").is_err());
    }
}

// ---------------------------------------------------------------------------
// Outside the root

#[test]
fn paths_outside_the_root_are_refused() {
    let outer = folder();
    let root = outer.path().join("root");
    fs::create_dir_all(&root).unwrap();
    fs::write(outer.path().join("beside.txt"), "x\n").unwrap();
    fs::write(root.join("in.txt"), "x\n").unwrap();
    for rel in ["../beside.txt", "/etc/passwd", "a/../../beside.txt", ""] {
        assert!(resolve(&root, rel).is_err(), "{rel:?} is refused");
    }
    assert!(list(&root, Some(".."), None).is_err());
    #[cfg(unix)]
    {
        symlink(outer.path().join("beside.txt"), root.join("link.txt")).unwrap();
        symlink(outer.path(), root.join("up")).unwrap();
        assert!(resolve(&root, "link.txt").is_err());
        assert!(resolve(&root, "up/beside.txt").is_err());
        assert!(list(&root, Some("up"), None).is_err());
    }
    assert_eq!(resolve(&root, "in.txt").unwrap(), root.join("in.txt"));
}

// ---------------------------------------------------------------------------
// Overlapping edits

#[test]
fn overlapping_edits_are_refused() {
    let view = Some("one two three\nfour\n");
    let err = apply_edits(view, &[edit("two three", "2 3"), edit("one two", "1 2")]).unwrap_err();
    assert_eq!(err, "Edits 1 and 2 overlap");
    let err = apply_edits(view, &[edit("four", "4"), edit("four", "IV")]).unwrap_err();
    assert_eq!(err, "Edits 1 and 2 overlap");
    // Touching isn't overlapping.
    assert_eq!(
        apply_edits(view, &[edit("three\n", "3\n"), edit("one two ", "")]).unwrap(),
        "3\nfour\n"
    );
    // Found twice, not at all, or a whole-file edit beside another.
    assert!(apply_edits(Some("aa\naa\n"), &[edit("aa", "b")]).is_err());
    assert!(
        apply_edits(Some("aaa"), &[edit("aa", "b")]).is_err(),
        "overlapping matches"
    );
    assert!(apply_edits(view, &[edit("five", "5")]).is_err());
    assert!(apply_edits(view, &[edit("", "all"), edit("four", "4")]).is_err());
    assert!(apply_edits(None, &[edit("four", "4")]).is_err());
    assert_eq!(apply_edits(None, &[edit("", "new\r\n")]).unwrap(), "new\n");
}

#[test]
fn an_overlapping_edit_set_leaves_the_earlier_proposal_as_it_was() {
    let buffer = Buffer::new("alpha\nbeta\ngamma\n");
    let snapshot = buffer.snapshot();
    let key = Path::new("/root/a.txt");
    let mut proposals = ChatProposals::new();
    proposals.begin_turn("chat:0", ai());
    let first = (proposals.propose(
        key,
        key,
        Base::Buffer(&snapshot),
        &[edit("beta", "BETA")],
        None,
    ))
    .unwrap();
    let earlier = first.proposal.unwrap();
    let refused = proposals.propose(
        key,
        key,
        Base::Buffer(&snapshot),
        &[edit("alpha\nBETA", "x"), edit("BETA\ngamma", "y")],
        None,
    );
    assert_eq!(refused.unwrap_err(), "Edits 1 and 2 overlap");
    assert!(Rc::ptr_eq(&proposals.proposal(key).unwrap(), &earlier));
    assert_eq!(earlier.borrow().status_count(HunkStatus::Pending), 1);
    let view = proposals.view(key, Base::Buffer(&snapshot), None).unwrap();
    assert_eq!(view.text.as_deref(), Some("alpha\nBETA\ngamma\n"));
    assert_eq!(view.pending, 1);
}

// ---------------------------------------------------------------------------
// Supersession across turns

#[test]
fn a_to_b_then_b_to_c_leaves_one_pending_a_to_c() {
    let mut buffers = Buffers(vec![Buffer::new("alpha\nbeta\ngamma\n")]);
    let snapshot = buffers.0[0].snapshot();
    let key = Path::new("/root/a.txt");
    let mut proposals = ChatProposals::new();

    proposals.begin_turn("chat:0", ai());
    let one = (proposals.propose(
        key,
        key,
        Base::Buffer(&snapshot),
        &[edit("alpha", "ALPHA")],
        None,
    ))
    .unwrap();
    assert_eq!(
        proposed_text("a.txt", &one),
        "Proposed 1 changes to a.txt; the user will review them."
    );
    let mut turn_one = proposals.end_turn(None, |_| None).expect("a file proposed");
    let first = one.proposal.unwrap();

    proposals.begin_turn("chat:1", ai());
    let view = proposals
        .view(key, Base::Buffer(&snapshot), Some(&mut turn_one))
        .unwrap();
    assert_eq!(view.text.as_deref(), Some("ALPHA\nbeta\ngamma\n"));
    let two = (proposals.propose(
        key,
        key,
        Base::Buffer(&snapshot),
        &[edit("ALPHA", "Alpha!")],
        Some(&mut turn_one),
    ))
    .unwrap();
    assert!(Rc::ptr_eq(two.replaced.as_ref().unwrap(), &first));
    assert!(
        two.shown_changed,
        "turn 1's changeset, shown, has rows to redraw"
    );
    let second = two.proposal.unwrap();

    // One pending a→c, in turn 2.
    let hunks = second.borrow().hunks(&snapshot);
    assert_eq!(hunks.len(), 1);
    assert_eq!(&*hunks[0].old_text, "alpha\n");
    assert_eq!(&*hunks[0].new_text, "Alpha!\n");
    assert_eq!(hunks[0].status, HunkStatus::Pending);
    // Turn 1's hunk reads as superseded, and its changeset counts it decided.
    let old = first.borrow().hunks(&snapshot);
    assert_eq!(old[0].status, HunkStatus::Rejected);
    assert!(proposals.is_superseded(old[0].id));
    assert!(!proposals.is_superseded(hunks[0].id));
    let counts = turn_one.summary(&mut buffers).counts;
    assert_eq!((counts.pending, counts.rejected), (0, 1));
    assert!(turn_one.is_resolved());

    let mut turn_two = proposals.end_turn(None, |_| None).expect("a file proposed");
    assert_eq!(turn_two.turn_id(), "chat:1");
    let (_, held) = turn_two.proposal(key).unwrap();
    assert!(Rc::ptr_eq(&held, &second));
    assert_eq!(turn_two.summary(&mut buffers).counts.pending, 1);
    // Nothing touched the buffer.
    assert_eq!(buffers.0[0].text().to_string(), "alpha\nbeta\ngamma\n");
}

#[test]
fn edits_in_one_turn_make_one_entry() {
    let buffer = Buffer::new("alpha\nbeta\n");
    let snapshot = buffer.snapshot();
    let key = Path::new("/root/a.txt");
    let mut proposals = ChatProposals::new();
    proposals.begin_turn("chat:0", ai());
    proposals
        .propose(
            key,
            key,
            Base::Buffer(&snapshot),
            &[edit("alpha", "A")],
            None,
        )
        .unwrap();
    let two = proposals
        .propose(
            key,
            key,
            Base::Buffer(&snapshot),
            &[edit("beta", "B")],
            None,
        )
        .unwrap();
    assert_eq!(two.changes, 1, "adjacent lines diff as one hunk");
    let changeset = proposals.end_turn(None, |_| None).unwrap();
    let mut buffers = Buffers(vec![buffer]);
    let mut changeset = changeset;
    let files = changeset.files(&mut buffers);
    assert_eq!(files.len(), 1);
    let (_, proposal) = changeset.proposal(key).unwrap();
    let dry_run = proposal.borrow().dry_run(&snapshot);
    assert_eq!(dry_run.text().to_string(), "A\nB\n");
    // Undoing its own change leaves nothing to propose, nor to hand in.
    proposals.begin_turn("chat:1", ai());
    let back = (proposals.propose(
        key,
        key,
        Base::Buffer(&snapshot),
        &[edit("A\nB", "alpha\nbeta")],
        None,
    ))
    .unwrap();
    assert_eq!(back.changes, 0);
    assert!(proposals.proposal(key).is_none());
    assert!(proposals.end_turn(None, |_| None).is_none());
}

#[test]
fn a_partly_decided_proposal_carries_only_its_pending_hunks() {
    let mut buffer = Buffer::new("alpha\nbeta\ngamma\ndelta\nepsilon\n");
    let key = Path::new("/root/a.txt");
    let mut proposals = ChatProposals::new();
    proposals.begin_turn("chat:0", ai());
    let snapshot = buffer.snapshot();
    let one = (proposals.propose(
        key,
        key,
        Base::Buffer(&snapshot),
        &[
            edit("alpha", "ALPHA"),
            edit("gamma", "GAMMA"),
            edit("epsilon", "EPSILON"),
        ],
        None,
    ))
    .unwrap();
    assert_eq!(one.changes, 3);
    let first = one.proposal.unwrap();
    let mut turn_one = proposals.end_turn(None, |_| None).unwrap();
    // The user accepts the first hunk and rejects the second.
    let ids: Vec<_> = (first.borrow().hunks(&snapshot).iter())
        .map(|hunk| hunk.id)
        .collect();
    (first
        .borrow_mut()
        .accept_as(&mut buffer, ids[0], Origin::User))
    .unwrap();
    first.borrow_mut().reject(ids[1]).unwrap();

    proposals.begin_turn("chat:1", ai());
    let snapshot = buffer.snapshot();
    let view = proposals
        .view(key, Base::Buffer(&snapshot), Some(&mut turn_one))
        .unwrap();
    assert_eq!(
        view.text.as_deref(),
        Some("ALPHA\nbeta\ngamma\ndelta\nEPSILON\n"),
        "accepted in the buffer, rejected left out, pending applied"
    );
    assert_eq!(view.pending, 1);
    let two = (proposals.propose(
        key,
        key,
        Base::Buffer(&snapshot),
        &[edit("beta", "BETA")],
        Some(&mut turn_one),
    ))
    .unwrap();
    let second = two.proposal.unwrap();
    let dry_run = second.borrow().dry_run(&snapshot);
    assert_eq!(
        dry_run.text().to_string(),
        "ALPHA\nBETA\ngamma\ndelta\nEPSILON\n"
    );
    assert_eq!(second.borrow().hunk_count(), 2);
    // Only the pending hunk was superseded.
    let old = first.borrow().hunks(&snapshot);
    let states: Vec<_> = old
        .iter()
        .map(|hunk| (hunk.status, proposals.is_superseded(hunk.id)))
        .collect();
    assert_eq!(
        states,
        [
            (HunkStatus::Accepted, false),
            (HunkStatus::Rejected, false),
            (HunkStatus::Rejected, true),
        ]
    );
    let mut buffers = Buffers(vec![buffer]);
    let counts = turn_one.summary(&mut buffers).counts;
    assert_eq!(
        (counts.accepted, counts.rejected, counts.pending),
        (1, 2, 0)
    );
}

#[test]
fn a_stale_hunk_is_left_out_and_said_so() {
    let mut buffer = Buffer::new("alpha\nbeta\ngamma\n");
    let key = Path::new("/root/a.txt");
    let mut proposals = ChatProposals::new();
    proposals.begin_turn("chat:0", ai());
    let snapshot = buffer.snapshot();
    let first = (proposals.propose(
        key,
        key,
        Base::Buffer(&snapshot),
        &[edit("alpha", "ALPHA")],
        None,
    ))
    .unwrap()
    .proposal
    .unwrap();
    let mut turn_one = proposals.end_turn(None, |_| None).unwrap();
    // The user changes the text under the hunk.
    buffer.edit([(0..5, "omega")], Origin::User);

    proposals.begin_turn("chat:1", ai());
    let snapshot = buffer.snapshot();
    let view = proposals
        .view(key, Base::Buffer(&snapshot), Some(&mut turn_one))
        .unwrap();
    assert_eq!(view.text.as_deref(), Some("omega\nbeta\ngamma\n"));
    assert_eq!((view.pending, view.stale), (0, 1));
    assert_eq!(
        read_output(view.text.as_deref().unwrap(), None, None, &view).unwrap(),
        "[1 of your earlier changes no longer apply]\nomega\nbeta\ngamma\n"
    );
    // The lost change isn't built on: `alpha` is gone from the view.
    assert!(
        proposals
            .propose(
                key,
                key,
                Base::Buffer(&snapshot),
                &[edit("ALPHA", "x")],
                Some(&mut turn_one)
            )
            .is_err()
    );
    let two = (proposals.propose(
        key,
        key,
        Base::Buffer(&snapshot),
        &[edit("gamma", "GAMMA")],
        Some(&mut turn_one),
    ))
    .unwrap();
    let hunks = two.proposal.unwrap().borrow().hunks(&snapshot);
    assert_eq!(hunks.len(), 1);
    assert_eq!(&*hunks[0].new_text, "GAMMA\n");
    // The stale hunk stays stale, for the user to reject: not superseded.
    let old = first.borrow().hunks(&snapshot);
    assert_eq!(old[0].status, HunkStatus::Stale);
    assert!(!proposals.is_superseded(old[0].id));
    let after = proposals.view(key, Base::Buffer(&snapshot), None).unwrap();
    assert_eq!((after.pending, after.stale), (1, 0));
}

#[test]
fn a_pending_new_file_edited_again_replaces_its_entry() {
    let key = Path::new("/root/new.txt");
    let mut proposals = ChatProposals::new();
    proposals.begin_turn("chat:0", ai());
    let one = proposals
        .propose(key, key, Base::Missing, &[edit("", "hello\nworld\n")], None)
        .unwrap();
    assert_eq!(one.changes, 1);
    assert!(one.proposal.is_none());
    // Edited again in the same turn: one entry still.
    proposals
        .propose(key, key, Base::Missing, &[edit("hello", "hi")], None)
        .unwrap();
    let mut turn_one = proposals.end_turn(None, |_| None).unwrap();
    assert_eq!(turn_one.new_file_text(key), Some("hi\nworld\n"));
    assert_eq!(turn_one.files(&mut Buffers(Vec::new())).len(), 1);

    proposals.begin_turn("chat:1", ai());
    let view = proposals
        .view(key, Base::Missing, Some(&mut turn_one))
        .unwrap();
    assert_eq!(view.text.as_deref(), Some("hi\nworld\n"));
    assert_eq!(view.pending, 1);
    let two = (proposals.propose(
        key,
        key,
        Base::Missing,
        &[edit("world", "there")],
        Some(&mut turn_one),
    ))
    .unwrap();
    assert!(two.shown_changed);
    // Turn 1's entry is decided and reads as superseded; turn 2 has the text.
    let entry = turn_one.files(&mut Buffers(Vec::new())).remove(0);
    assert_eq!((entry.counts.pending, entry.counts.rejected), (0, 1));
    assert!(proposals.is_new_file_superseded("chat:0", key));
    assert!(!proposals.is_new_file_superseded("chat:1", key));
    let turn_two = proposals.end_turn(None, |_| None).unwrap();
    assert_eq!(turn_two.new_file_text(key), Some("hi\nthere\n"));

    // While another turn's changeset is shown, it stays as last seen:
    // pending, and superseded again in a later turn.
    let mut other = Changeset::new("chat:9", ai());
    proposals.begin_turn("chat:2", ai());
    let view = proposals
        .view(key, Base::Missing, Some(&mut other))
        .unwrap();
    assert_eq!(view.text.as_deref(), Some("hi\nthere\n"));
    let three = (proposals.propose(
        key,
        key,
        Base::Missing,
        &[edit("hi", "yo")],
        Some(&mut other),
    ))
    .unwrap();
    assert!(!three.shown_changed);
    assert!(proposals.is_new_file_superseded("chat:1", key));
    assert_eq!(
        proposals
            .end_turn(None, |_| None)
            .unwrap()
            .new_file_text(key),
        Some("yo\nthere\n")
    );
}

#[test]
fn a_new_file_rejected_by_the_user_is_no_longer_its_view() {
    let key = Path::new("/root/new.txt");
    let mut proposals = ChatProposals::new();
    proposals.begin_turn("chat:0", ai());
    proposals
        .propose(key, key, Base::Missing, &[edit("", "draft\n")], None)
        .unwrap();
    let mut turn_one = proposals.end_turn(None, |_| None).unwrap();
    turn_one.reject_file(&mut Buffers(Vec::new()), key).unwrap();
    proposals.begin_turn("chat:1", ai());
    assert_eq!(
        proposals
            .view(key, Base::Missing, Some(&mut turn_one))
            .unwrap()
            .text,
        None
    );
    let again = (proposals.propose(
        key,
        key,
        Base::Missing,
        &[edit("", "fresh\n")],
        Some(&mut turn_one),
    ))
    .unwrap();
    assert!(!again.shown_changed);
    assert!(!proposals.is_new_file_superseded("chat:0", key));
}

#[test]
fn read_output_cuts_an_oversized_line_and_says_so() {
    let none = View::default();
    // One line of 70 KiB: what fits, and the rest said to be left out.
    let long = "x".repeat(70 << 10);
    let out = read_output(&long, None, None, &none).unwrap();
    let (body, last) = out.rsplit_once('\n').unwrap();
    assert_eq!(body.len(), MAX_READ_BYTES);
    assert_eq!(
        last,
        "[truncated: the rest of line 1, which is over 64 KiB]"
    );
    // Cut on a char boundary: a 3-byte char across the cap isn't split.
    let wide = format!("{}€€€\nnext\n", "x".repeat(MAX_READ_BYTES - 1));
    let out = read_output(&wide, None, None, &none).unwrap();
    let (body, last) = out.rsplit_once('\n').unwrap();
    assert_eq!(body, "x".repeat(MAX_READ_BYTES - 1));
    assert_eq!(
        last,
        "[truncated: the rest of line 1, which is over 64 KiB, and 1 more lines; \
         read from line 2]"
    );
    // A range ending on the oversized line.
    let text = format!("a\n{long}\nb\n");
    let out = read_output(&text, Some(2), Some(2), &none).unwrap();
    assert!(out.ends_with("\n[truncated: the rest of line 2, which is over 64 KiB]"));
    // After whole lines, the oversized one is left whole for the next read.
    let out = read_output(&text, None, None, &none).unwrap();
    assert_eq!(out, "a\n[truncated: 2 more lines; read from line 2]");
}

#[test]
fn an_oversized_or_binary_view_is_refused() {
    let key = Path::new("/root/a.txt");
    let mut proposals = ChatProposals::new();
    let big = Buffer::new(&"x\n".repeat(MAX_FILE / 2 + 1));
    assert!(
        proposals
            .view(key, Base::Buffer(&big.snapshot()), None)
            .is_err()
    );
    let binary = Buffer::new("a\0b\n");
    assert!(
        proposals
            .view(key, Base::Buffer(&binary.snapshot()), None)
            .is_err()
    );
    // A proposal that would make the view over 1 MiB isn't made.
    let small = Buffer::new("x\n");
    proposals.begin_turn("chat:0", ai());
    let huge = "y".repeat(MAX_FILE);
    let refused = proposals.propose(
        key,
        key,
        Base::Buffer(&small.snapshot()),
        &[edit("x", &huge)],
        None,
    );
    assert!(refused.is_err());
    assert!(proposals.proposal(key).is_none());
}

#[test]
fn a_decided_proposal_is_kept_for_revert() {
    let buffer = Buffer::new("alpha\nbeta\n");
    let snapshot = buffer.snapshot();
    let key = Path::new("/root/a.txt");
    let mut proposals = ChatProposals::new();
    proposals.begin_turn("chat:0", ai());
    let first = (proposals.propose(
        key,
        key,
        Base::Buffer(&snapshot),
        &[edit("alpha", "ALPHA")],
        None,
    ))
    .unwrap()
    .proposal
    .unwrap();
    proposals.end_turn(None, |_| None).unwrap();
    let id = first.borrow().hunks(&snapshot)[0].id;
    first.borrow_mut().reject(id).unwrap();
    let view = proposals.view(key, Base::Buffer(&snapshot), None).unwrap();
    assert_eq!(
        (view.text.as_deref(), view.pending),
        (Some("alpha\nbeta\n"), 0)
    );
    assert!(proposals.proposal(key).is_some(), "decided, not forgotten");
    // Revert Turn makes it pending again: it's the chat's view again.
    first.borrow_mut().reset_to_pending(id).unwrap();
    let view = proposals.view(key, Base::Buffer(&snapshot), None).unwrap();
    assert_eq!(
        (view.text.as_deref(), view.pending),
        (Some("ALPHA\nbeta\n"), 1)
    );
}

#[test]
fn the_running_turns_proposal_follows_a_reopened_buffer() {
    let first = Buffer::new("alpha\nbeta\n");
    let key = Path::new("/root/a.txt");
    let mut proposals = ChatProposals::new();
    proposals.begin_turn("chat:0", ai());
    let proposal = (proposals.propose(
        key,
        key,
        Base::Buffer(&first.snapshot()),
        &[edit("alpha", "ALPHA")],
        None,
    ))
    .unwrap()
    .proposal
    .unwrap();
    // Its tab closed; the file opened again in a new buffer of the same text.
    drop(first);
    let again = Buffer::new("alpha\nbeta\n");
    let view = proposals
        .view(key, Base::Buffer(&again.snapshot()), None)
        .unwrap();
    assert_eq!(
        (view.text.as_deref(), view.pending),
        (Some("ALPHA\nbeta\n"), 1)
    );
    assert_eq!(proposal.borrow().buffer(), again.id());
    assert!(Rc::ptr_eq(&proposals.proposal(key).unwrap(), &proposal));
    // Opened on other text, it can't be followed: its change no longer
    // applies, and it stays the file's until a later one replaces it.
    let other = Buffer::new("gamma\n");
    let view = proposals
        .view(key, Base::Buffer(&other.snapshot()), None)
        .unwrap();
    assert_eq!(
        (view.text.as_deref(), view.pending, view.stale),
        (Some("gamma\n"), 0, 1)
    );
    assert!(proposals.proposal(key).is_some());
    let replaced = (proposals.propose(
        key,
        key,
        Base::Buffer(&other.snapshot()),
        &[edit("gamma", "GAMMA")],
        None,
    ))
    .unwrap();
    assert!(Rc::ptr_eq(replaced.replaced.as_ref().unwrap(), &proposal));
    let mut changeset = proposals.end_turn(None, |_| None).unwrap();
    assert_eq!(changeset.files(&mut Buffers(vec![other])).len(), 1);
}

#[test]
fn only_the_file_the_walk_opened_is_reached() {
    let dir = folder();
    let path = dir.path().join("a.txt");
    fs::write(&path, "one\n").unwrap();
    let stamp = |path: &Path| DiskStamp::from_metadata(&fs::metadata(path).unwrap()).ok();
    let read = vet(dir.path(), "a.txt", &[(path.clone(), stamp(&path))]).unwrap();
    assert_eq!(read.open, Some(0));
    assert_eq!(read.real, path);
    // A buffer read from the file before it was replaced isn't its text.
    let before = stamp(&path);
    fs::write(dir.path().join("swap.txt"), "two\n").unwrap();
    fs::rename(dir.path().join("swap.txt"), &path).unwrap();
    assert!(vet(dir.path(), "a.txt", &[(path.clone(), before)]).is_err());
    // Nor is a buffer of a file now gone, nor one with no file now there.
    assert!(
        vet(
            dir.path(),
            "gone.txt",
            &[(dir.path().join("gone.txt"), stamp(&path))],
        )
        .is_err()
    );
    assert!(vet(dir.path(), "a.txt", &[(path.clone(), None)]).is_err());
    // Read through the handle the walk opened, unless a buffer has it.
    let read = vet(dir.path(), "a.txt", &[]).unwrap();
    let loaded = read.loaded.expect("loaded");
    assert_eq!(loaded.text().to_string(), "two\n");
    assert!(same_file(
        loaded.disk_stamp().as_ref(),
        stamp(&path).as_ref()
    ));
    // Over 1 MiB, a folder, or (on unix) a FIFO, a link to a secret.
    fs::write(dir.path().join("big.txt"), "x".repeat(MAX_FILE + 1)).unwrap();
    assert!(vet(dir.path(), "big.txt", &[]).is_err());
    fs::create_dir(dir.path().join("sub")).unwrap();
    assert!(vet(dir.path(), "sub", &[]).is_err());
    #[cfg(unix)]
    {
        let fifo = std::ffi::CString::new(dir.path().join("pipe").to_str().unwrap()).unwrap();
        // SAFETY: a NUL-terminated path that outlives the call.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert!(vet(dir.path(), "pipe", &[]).is_err());
        assert!(vet(dir.path(), "pipe", &[]).is_err());
        fs::write(dir.path().join(".env"), "SECRET=1\n").unwrap();
        fs::remove_file(&path).unwrap();
        symlink(dir.path().join(".env"), &path).unwrap();
        assert!(vet(dir.path(), "a.txt", &[]).is_err());
    }
}

#[test]
fn a_file_swapped_after_the_walk_is_never_read() {
    let outer = folder();
    let root = outer.path().join("root");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("a.txt");
    fs::write(outer.path().join("outside.txt"), "outside\n").unwrap();
    // Swapped for a link out of the root, between the walk and the read.
    fs::write(&path, "inside\n").unwrap();
    let vetted = vet_with(&root, "a.txt", &[], || {
        fs::remove_file(&path).unwrap();
        #[cfg(unix)]
        symlink(outer.path().join("outside.txt"), &path).unwrap();
        #[cfg(not(unix))]
        fs::write(&path, "outside\n").unwrap();
    })
    .unwrap();
    assert_eq!(vetted.real, path);
    assert_eq!(vetted.loaded.unwrap().text().to_string(), "inside\n");
    // And for a FIFO: the read is of the file opened, so it can't block.
    #[cfg(unix)]
    {
        fs::remove_file(&path).unwrap();
        fs::write(&path, "again\n").unwrap();
        let vetted = vet_with(&root, "a.txt", &[], || {
            fs::remove_file(&path).unwrap();
            let fifo = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
            // SAFETY: a NUL-terminated path that outlives the call.
            assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        })
        .unwrap();
        assert_eq!(vetted.loaded.unwrap().text().to_string(), "again\n");
    }
    // Grown past 1 MiB between the walk's check and the read: refused.
    fs::remove_file(&path).unwrap();
    fs::write(&path, "small\n").unwrap();
    let grown = vet_with(&root, "a.txt", &[], || {
        use std::io::Write as _;
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(&vec![b'x'; MAX_FILE + 1]).unwrap();
    });
    assert!(grown.is_err());
}

#[test]
fn a_pending_view_over_1_mib_is_refused_before_the_dry_run() {
    let mut buffer = Buffer::new("alpha\nbeta\n");
    let key = Path::new("/root/a.txt");
    let mut proposals = ChatProposals::new();
    proposals.begin_turn("chat:0", ai());
    let snapshot = buffer.snapshot();
    (proposals.propose(
        key,
        key,
        Base::Buffer(&snapshot),
        &[edit("alpha", "ALPHA")],
        None,
    ))
    .unwrap();
    // Text pasted inside the pending hunk: a conflict, and over 1 MiB.
    buffer.edit([(2..2, "x".repeat(2 * MAX_FILE))], Origin::User);
    let snapshot = buffer.snapshot();
    assert!(proposals.view(key, Base::Buffer(&snapshot), None).is_err());
    assert!(proposals.proposal(key).is_some());
}

#[test]
fn pending_work_moves_to_the_latest_turns_review() {
    let mut buffers = Buffers(vec![
        Buffer::new("a1\n"),
        Buffer::new("b1\n"),
        Buffer::new("c1\n"),
    ]);
    let snaps: Vec<_> = buffers.0.iter().map(Buffer::snapshot).collect();
    let [a, b, c, n, m] = ["a.txt", "b.txt", "c.txt", "new.txt", "gone.txt"]
        .map(|name| PathBuf::from("/root").join(name));
    let mut proposals = ChatProposals::new();
    let propose = |proposals: &mut ChatProposals,
                   key: &Path,
                   base: Base,
                   edits: &[ProposedEdit],
                   shown: Option<&mut Changeset>| {
        proposals.propose(key, key, base, edits, shown).unwrap()
    };

    proposals.begin_turn("chat:0", ai());
    let first_a = propose(
        &mut proposals,
        &a,
        Base::Buffer(&snaps[0]),
        &[edit("a1", "A1")],
        None,
    )
    .proposal
    .unwrap();
    let first_c = propose(
        &mut proposals,
        &c,
        Base::Buffer(&snaps[2]),
        &[edit("c1", "C1")],
        None,
    )
    .proposal
    .unwrap();
    propose(
        &mut proposals,
        &n,
        Base::Missing,
        &[edit("", "fresh\n")],
        None,
    );
    propose(
        &mut proposals,
        &m,
        Base::Missing,
        &[edit("", "doomed\n")],
        None,
    );
    let mut turn_zero = proposals.end_turn(None, |_| None).unwrap();
    // The user decides c's change and the new file m in turn 0's review.
    let id = first_c.borrow().hunks(&snaps[2])[0].id;
    first_c.borrow_mut().reject(id).unwrap();
    turn_zero.reject_file(&mut Buffers(Vec::new()), &m).unwrap();

    // Turn 1 proposes only b; its review takes a and n, still pending, too.
    proposals.begin_turn("chat:1", ai());
    propose(
        &mut proposals,
        &b,
        Base::Buffer(&snaps[1]),
        &[edit("b1", "B1")],
        None,
    );
    let mut turn_one = proposals.end_turn(Some(&mut turn_zero), |_| None).unwrap();
    let listed: Vec<PathBuf> = (turn_one.files(&mut buffers).into_iter())
        .map(|entry| entry.path)
        .collect();
    assert_eq!(listed, [b.clone(), a.clone(), n.clone()]);
    assert!(Rc::ptr_eq(&turn_one.proposal(&a).unwrap().1, &first_a));
    assert_eq!(turn_one.new_file_text(&n), Some("fresh\n"));
    assert_eq!(turn_one.summary(&mut buffers).counts.pending, 3);

    // Superseding them now marks turn 1's review, the one shown.
    proposals.begin_turn("chat:2", ai());
    let two = propose(
        &mut proposals,
        &a,
        Base::Buffer(&snaps[0]),
        &[edit("A1", "Aa")],
        Some(&mut turn_one),
    );
    assert!(two.shown_changed);
    let two = propose(
        &mut proposals,
        &n,
        Base::Missing,
        &[edit("fresh", "final")],
        Some(&mut turn_one),
    );
    assert!(two.shown_changed);
    assert!(proposals.is_new_file_superseded("chat:1", &n));
    let counts = turn_one.summary(&mut buffers).counts;
    assert_eq!((counts.pending, counts.rejected), (1, 2));
    let mut turn_two = proposals.end_turn(Some(&mut turn_one), |_| None).unwrap();
    let listed: Vec<PathBuf> = (turn_two.files(&mut buffers).into_iter())
        .map(|entry| entry.path)
        .collect();
    assert_eq!(listed, [a.clone(), n.clone(), b.clone()]);

    // A turn that proposes nothing leaves the review shown be, unless
    // another changeset took its place.
    proposals.begin_turn("chat:3", ai());
    assert!(proposals.end_turn(Some(&mut turn_two), |_| None).is_none());
    proposals.begin_turn("chat:4", ai());
    let mut elsewhere = Changeset::new("review", Origin::User);
    let mut back = proposals.end_turn(Some(&mut elsewhere), |_| None).unwrap();
    assert_eq!(back.turn_id(), "chat:4");
    assert_eq!(back.files(&mut buffers).len(), 3);
}

#[test]
fn only_a_proposal_never_handed_in_is_made_again_on_a_reopened_buffer() {
    let key = Path::new("/root/a.txt");
    let text = "alpha\nbeta\ngamma\n";
    // Handed in: its changeset follows it (Changeset::rebind), not the chat,
    // so the chat never rebinds the proposal its review holds.
    let first = Buffer::new(text);
    let snapshot = first.snapshot();
    let mut proposals = ChatProposals::new();
    proposals.begin_turn("chat:0", ai());
    let proposal = (proposals.propose(
        key,
        key,
        Base::Buffer(&snapshot),
        &[edit("alpha", "ALPHA"), edit("gamma", "GAMMA")],
        None,
    ))
    .unwrap()
    .proposal
    .unwrap();
    proposals.end_turn(None, |_| None).unwrap();
    let id = proposal.borrow().hunks(&snapshot)[0].id;
    proposal.borrow_mut().reject(id).unwrap();
    let held = proposal.borrow().buffer();
    drop(first);
    proposals.begin_turn("chat:1", ai());
    let again = Buffer::new(text);
    let view = proposals
        .view(key, Base::Buffer(&again.snapshot()), None)
        .unwrap();
    assert_eq!(
        (view.text.as_deref(), view.pending, view.stale),
        (Some(text), 0, 1)
    );
    assert_eq!(proposal.borrow().buffer(), held);
    assert_eq!(proposal.borrow().status_count(HunkStatus::Rejected), 1);

    // Never handed in, but a hunk was found stale, then the edit undone and
    // the tab closed: refused, as rebind refuses a stale hunk.
    let mut buffer = Buffer::new(text);
    let key = Path::new("/root/b.txt");
    let stale = (proposals.propose(
        key,
        key,
        Base::Buffer(&buffer.snapshot()),
        &[edit("beta", "BETA")],
        None,
    ))
    .unwrap()
    .proposal
    .unwrap();
    buffer.edit([(6..10, "BETA")], Origin::User);
    assert_eq!(
        stale.borrow().hunks(&buffer.snapshot())[0].status,
        HunkStatus::Stale
    );
    buffer.undo(|_| true).unwrap();
    assert_eq!(buffer.text().to_string(), text);
    drop(buffer);
    let reopened = Buffer::new(text);
    let view = proposals
        .view(key, Base::Buffer(&reopened.snapshot()), None)
        .unwrap();
    assert_eq!(
        (view.text.as_deref(), view.pending, view.stale),
        (Some(text), 0, 1)
    );
    assert_ne!(stale.borrow().buffer(), reopened.id());
}

#[test]
fn carrying_a_partly_accepted_proposal_keeps_each_turns_revert() {
    let mut buffers = Buffers(vec![
        Buffer::new("alpha\nbeta\ngamma\n"),
        Buffer::new("b\n"),
    ]);
    let (sa, sb) = (buffers.0[0].snapshot(), buffers.0[1].snapshot());
    let (a, b) = (PathBuf::from("/root/a.txt"), PathBuf::from("/root/b.txt"));
    let mut proposals = ChatProposals::new();
    proposals.begin_turn("chat:0", ai());
    let first = (proposals.propose(
        &a,
        &a,
        Base::Buffer(&sa),
        &[edit("alpha", "ALPHA"), edit("gamma", "GAMMA")],
        None,
    ))
    .unwrap()
    .proposal
    .unwrap();
    let mut turn_zero = proposals.end_turn(None, |_| None).unwrap();
    // The user accepts the first change in turn 0's review.
    let ids: Vec<_> = (first.borrow().hunks(&sa).iter())
        .map(|hunk| hunk.id)
        .collect();
    turn_zero.accept_hunk(&mut buffers, &a, ids[0]).unwrap();
    assert_eq!(buffers.0[0].text().to_string(), "ALPHA\nbeta\ngamma\n");

    // Turn 1's review takes only the pending change, as a proposal of its
    // own: turn 0 keeps its accept, and its checkpoint.
    proposals.begin_turn("chat:1", ai());
    proposals
        .propose(&b, &b, Base::Buffer(&sb), &[edit("b", "B")], None)
        .unwrap();
    let now: Vec<BufferSnapshot> = buffers.0.iter().map(Buffer::snapshot).collect();
    let mut turn_one = proposals
        .end_turn(Some(&mut turn_zero), |id| {
            now.iter().find(|s| s.id() == id).cloned()
        })
        .unwrap();
    let carried = proposals.proposal(&a).unwrap();
    assert!(!Rc::ptr_eq(&carried, &first));
    assert!(Rc::ptr_eq(&turn_one.proposal(&a).unwrap().1, &carried));
    let entry = (turn_one.files(&mut buffers).into_iter())
        .find(|entry| entry.path == a)
        .unwrap();
    assert_eq!(
        (
            entry.counts.hunks,
            entry.counts.pending,
            entry.counts.accepted
        ),
        (1, 1, 0)
    );
    let old = first.borrow().hunks(&buffers.0[0].snapshot());
    assert_eq!(old[1].status, HunkStatus::Rejected);
    assert!(proposals.is_superseded(old[1].id));
    // Accepting the rest, then Revert Turn on each review, undoes each
    // turn's own accept.
    turn_one.accept_file(&mut buffers, &a).unwrap();
    assert_eq!(buffers.0[0].text().to_string(), "ALPHA\nbeta\nGAMMA\n");
    turn_one.revert(&mut buffers);
    assert_eq!(buffers.0[0].text().to_string(), "ALPHA\nbeta\ngamma\n");
    // Turn 0's file was edited since its accept (turn 1's revert), so its
    // Revert Turn asks first, as for any edit, then restores it.
    let asked = turn_zero.revert(&mut buffers);
    assert_eq!(
        asked,
        [fe_editor::RevertOutcome::NeedsConfirm { path: a.clone() }]
    );
    turn_zero.revert_forced(&mut buffers);
    assert_eq!(buffers.0[0].text().to_string(), "alpha\nbeta\ngamma\n");
}

#[test]
fn the_bounded_loader_reads_as_the_pane_does() {
    // Under each .editorconfig charset, and none, the buffer read from the
    // vetted handle is the one `loader::read` gives: text, encoding, BOM,
    // whether it needs saving, and the bytes a save writes.
    let cases: [(Option<&str>, &[u8]); 6] = [
        (None, "caf\u{e9}\n".as_bytes()),
        (Some("latin1"), b"plain ascii\n"),
        (Some("latin1"), b"caf\xe9\n"),
        (Some("utf-8-bom"), b"no bom yet\n"),
        (Some("utf-8"), b"\xef\xbb\xbfwith a bom\n"),
        // Another encoding's BOM wins over the charset, as the loader has it.
        (Some("latin1"), b"\xef\xbb\xbfcaf\xc3\xa9\n"),
    ];
    for (charset, bytes) in cases {
        let dir = folder();
        if let Some(charset) = charset {
            let config = format!("root = true\n[*]\ncharset = {charset}\n");
            fs::write(dir.path().join(".editorconfig"), config).unwrap();
        }
        let path = dir.path().join("a.txt");
        fs::write(&path, bytes).unwrap();
        let ours = vet(dir.path(), "a.txt", &[]).unwrap().loaded.unwrap();
        let theirs = crate::loader::read(path.clone()).unwrap().unwrap();
        let case = format!("{charset:?} {bytes:?}");
        assert_eq!(ours.text().to_string(), theirs.text().to_string(), "{case}");
        assert_eq!(ours.encoding(), theirs.encoding(), "{case}");
        assert_eq!(ours.is_dirty(), theirs.is_dirty(), "{case}");
        let (mut written, mut expected) = (Vec::new(), Vec::new());
        ours.write_to(&mut written).ok();
        theirs.write_to(&mut expected).ok();
        assert_eq!(written, expected, "{case}: saves the same bytes");
    }
}

// ---------------------------------------------------------------------------
// The executors over a workbench

fn opened<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    (workbench, cx)
}

/// Runs `start`'s task to its end.
fn run(
    cx: &mut VisualTestContext,
    start: impl FnOnce(&mut gpui::Window, &mut gpui::App) -> gpui::Task<ToolOutcome>,
) -> ToolOutcome {
    let slot: Rc<RefCell<Option<ToolOutcome>>> = Rc::default();
    cx.update(|window, cx| {
        let task = start(window, cx);
        let slot = slot.clone();
        cx.spawn(async move |_| *slot.borrow_mut() = Some(task.await))
            .detach();
    });
    cx.run_until_parked();
    slot.borrow_mut().take().expect("the tool finished")
}

fn read(
    workbench: &Entity<Workbench>,
    proposals: &Rc<RefCell<ChatProposals>>,
    path: &str,
    cx: &mut VisualTestContext,
) -> ToolOutcome {
    let input = ReadFileInput {
        path: path.to_owned(),
        start_line: None,
        end_line: None,
    };
    run(cx, |window, cx| {
        read_file(workbench, proposals, input, window, cx)
    })
}

fn propose_to(
    workbench: &Entity<Workbench>,
    proposals: &Rc<RefCell<ChatProposals>>,
    path: &str,
    edits: Vec<ProposedEdit>,
    cx: &mut VisualTestContext,
) -> ToolOutcome {
    let input = ProposeEditInput {
        path: path.to_owned(),
        edits,
    };
    run(cx, |window, cx| {
        propose_edit(workbench, proposals, input, window, cx)
    })
}

fn editor_of(
    workbench: &Entity<Workbench>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.file_editors(cx).into_iter())
            .find(|editor| editor.read(cx).model().path() == Some(path))
            .expect("open")
    })
}

/// Ends the turn and hands its changeset to the workbench, as the loop
/// does: whether one was set.
fn hand_in(
    workbench: &Entity<Workbench>,
    proposals: &Rc<RefCell<ChatProposals>>,
    cx: &mut VisualTestContext,
) -> bool {
    let slot: Rc<RefCell<Option<bool>>> = Rc::default();
    cx.update(|window, cx| {
        let task = end_turn(workbench, proposals, window, cx);
        let slot = slot.clone();
        cx.spawn(async move |_| *slot.borrow_mut() = Some(task.await))
            .detach();
    });
    cx.run_until_parked();
    slot.borrow_mut().take().expect("the turn ended")
}

/// The shown changeset's counts for `path`: pending, rejected.
fn shown_counts(
    workbench: &Entity<Workbench>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> (usize, usize) {
    workbench.update(cx, |workbench, _| {
        let changeset = &mut workbench.ai_changes.as_mut().expect("shown").changeset;
        let entry = (changeset.files(&mut Buffers(Vec::new())).into_iter())
            .find(|entry| entry.path == path)
            .expect("listed");
        (entry.counts.pending, entry.counts.rejected)
    })
}

fn is_open(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) -> bool {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.file_editors(cx).iter())
            .any(|editor| editor.read(cx).model().path() == Some(path))
    })
}

/// Closes the tab on `path` without asking, as a close the user agreed to:
/// one on a buffer with unsaved edits would ask first.
fn take_tab(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let ids: Vec<_> = pane.read_with(cx, |pane, cx| {
        (pane.tabs().iter())
            .filter(|tab| (tab.editor()).is_some_and(|e| e.read(cx).model().path() == Some(path)))
            .map(|tab| tab.id())
            .collect()
    });
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.take_ids(&ids, window, cx)));
    cx.run_until_parked();
}

fn close_active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.close_active(window, cx)));
    cx.run_until_parked();
}

#[gpui::test]
fn read_file_shows_the_pending_proposal(cx: &mut TestAppContext) {
    let dir = folder();
    let path: PathBuf = dir.path().join("a.txt");
    fs::write(&path, "one\ntwo\nthree\n").unwrap();
    let (workbench, cx) = opened(cx, dir.path());
    let proposals = Rc::new(RefCell::new(ChatProposals::new()));
    proposals.borrow_mut().begin_turn("chat:0", ai());

    assert_eq!(
        read(&workbench, &proposals, "a.txt", cx).unwrap(),
        "one\ntwo\nthree\n"
    );
    // Not open yet: proposing opens it in the active pane.
    assert_eq!(
        propose_to(
            &workbench,
            &proposals,
            "a.txt",
            vec![edit("two", "TWO")],
            cx
        )
        .unwrap(),
        "Proposed 1 changes to a.txt; the user will review them."
    );
    assert_eq!(
        read(&workbench, &proposals, "a.txt", cx).unwrap(),
        "[includes 1 pending changes you proposed]\none\nTWO\nthree\n"
    );
    // Proposed, never written, and not yet the editor's to decide.
    let editor = editor_of(&workbench, &path, cx);
    let text = editor.read_with(cx, |editor, _| editor.model().text().to_string());
    assert_eq!(text, "one\ntwo\nthree\n");
    assert_eq!(fs::read_to_string(&path).unwrap(), "one\ntwo\nthree\n");
    assert!(editor.read_with(cx, |editor, _| editor.proposals().is_empty()));
    propose_to(
        &workbench,
        &proposals,
        "a.txt",
        vec![edit("TWO\nthree", "TWO\n3")],
        cx,
    )
    .unwrap();
    assert_eq!(
        read(&workbench, &proposals, "a.txt", cx).unwrap(),
        "[includes 1 pending changes you proposed]\none\nTWO\n3\n"
    );

    // The turn's changeset puts its one proposal in the editor.
    assert!(hand_in(&workbench, &proposals, cx));
    let shown = editor.read_with(cx, |editor, _| editor.proposals().to_vec());
    assert_eq!(shown.len(), 1);
    assert!(Rc::ptr_eq(
        &shown[0],
        &proposals.borrow().proposal(&path).unwrap()
    ));

    // Unsaved edits are read; the user's edit under the hunk makes it stale.
    proposals.borrow_mut().begin_turn("chat:1", ai());
    editor.update(cx, |editor, _| {
        let mut buffer = editor.model_mut().buffer_mut();
        buffer.edit([(4..7, "zwei")], Origin::User);
    });
    cx.run_until_parked();
    assert_eq!(
        read(&workbench, &proposals, "a.txt", cx).unwrap(),
        "[1 of your earlier changes no longer apply]\none\nzwei\nthree\n"
    );
}

#[gpui::test]
fn the_executors_refuse_secrets_and_paths_outside_the_root(cx: &mut TestAppContext) {
    let outer = folder();
    let root = outer.path().join("root");
    fs::create_dir_all(&root).unwrap();
    fs::write(outer.path().join("beside.txt"), "x\n").unwrap();
    fs::write(root.join("main.go"), "package main\n").unwrap();
    fs::write(root.join("id_rsa"), "key\n").unwrap();
    fs::write(root.join("server.pem"), "cert\n").unwrap();
    let (workbench, cx) = opened(cx, &root);
    let proposals = Rc::new(RefCell::new(ChatProposals::new()));
    proposals.borrow_mut().begin_turn("chat:0", ai());

    for rel in ["id_rsa", "../beside.txt", "/etc/hostname", ".git/HEAD"] {
        assert!(
            read(&workbench, &proposals, rel, cx).is_err(),
            "{rel} isn't read"
        );
    }
    for rel in ["server.pem", "../beside.txt", ".env"] {
        let proposed = propose_to(&workbench, &proposals, rel, vec![edit("", "x\n")], cx);
        assert!(proposed.is_err(), "{rel} isn't proposed to");
    }
    assert_eq!(
        fs::read_to_string(outer.path().join("beside.txt")).unwrap(),
        "x\n"
    );
    assert!(!root.join(".env").exists());
    assert!(proposals.borrow_mut().end_turn(None, |_| None).is_none());
    // A file that isn't there reads as none.
    assert!(read(&workbench, &proposals, "absent.go", cx).is_err());
    assert_eq!(
        read(&workbench, &proposals, "main.go", cx).unwrap(),
        "package main\n"
    );
}

#[gpui::test]
fn a_new_file_is_proposed_without_being_written(cx: &mut TestAppContext) {
    let dir = folder();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    let (workbench, cx) = opened(cx, dir.path());
    let proposals = Rc::new(RefCell::new(ChatProposals::new()));
    proposals.borrow_mut().begin_turn("chat:0", ai());
    assert!(
        propose_to(
            &workbench,
            &proposals,
            "src/new.go",
            vec![edit("a", "b")],
            cx
        )
        .is_err()
    );
    assert!(
        propose_to(
            &workbench,
            &proposals,
            "nowhere/new.go",
            vec![edit("", "x\n")],
            cx
        )
        .is_err()
    );
    let proposed = propose_to(
        &workbench,
        &proposals,
        "src/new.go",
        vec![edit("", "package src\n")],
        cx,
    );
    assert_eq!(
        proposed.unwrap(),
        "Proposed 1 changes to src/new.go; the user will review them."
    );
    assert_eq!(
        read(&workbench, &proposals, "src/new.go", cx).unwrap(),
        "[includes 1 pending changes you proposed]\npackage src\n"
    );
    propose_to(
        &workbench,
        &proposals,
        "src/new.go",
        vec![edit("src", "lib")],
        cx,
    )
    .unwrap();
    assert!(!dir.path().join("src/new.go").exists());
    let changeset = proposals.borrow_mut().end_turn(None, |_| None).unwrap();
    let key = dir.path().join("src/new.go");
    assert_eq!(changeset.new_file_text(&key), Some("package lib\n"));
}

#[gpui::test]
fn a_closed_editors_proposal_is_followed_when_read_again(cx: &mut TestAppContext) {
    let dir = folder();
    let a = dir.path().join("a.txt");
    let b = dir.path().join("b.txt");
    fs::write(&a, "one\ntwo\n").unwrap();
    fs::write(&b, "red\nblue\n").unwrap();
    let (workbench, cx) = opened(cx, dir.path());
    let proposals = Rc::new(RefCell::new(ChatProposals::new()));

    // An earlier turn's proposal, shown, its tab then closed: the AI
    // Changes panel follows it to the reopened buffer.
    proposals.borrow_mut().begin_turn("chat:0", ai());
    propose_to(
        &workbench,
        &proposals,
        "a.txt",
        vec![edit("two", "TWO")],
        cx,
    )
    .unwrap();
    assert!(hand_in(&workbench, &proposals, cx));
    close_active(&workbench, cx);
    assert!(!is_open(&workbench, &a, cx));
    proposals.borrow_mut().begin_turn("chat:1", ai());
    assert_eq!(
        read(&workbench, &proposals, "a.txt", cx).unwrap(),
        "[includes 1 pending changes you proposed]\none\nTWO\n"
    );
    // And it's superseded there, not left beside a second one.
    let earlier = proposals.borrow().proposal(&a).unwrap();
    propose_to(
        &workbench,
        &proposals,
        "a.txt",
        vec![edit("one", "ONE")],
        cx,
    )
    .unwrap();
    assert_eq!(earlier.borrow().status_count(HunkStatus::Rejected), 1);
    assert_eq!(shown_counts(&workbench, &a, cx), (0, 1));

    // The running turn's own proposal, its tab closed, is made again.
    propose_to(
        &workbench,
        &proposals,
        "b.txt",
        vec![edit("blue", "BLUE")],
        cx,
    )
    .unwrap();
    close_active(&workbench, cx);
    assert!(!is_open(&workbench, &b, cx));
    assert_eq!(
        read(&workbench, &proposals, "b.txt", cx).unwrap(),
        "[includes 1 pending changes you proposed]\nred\nBLUE\n"
    );
    let mut changeset = proposals.borrow_mut().end_turn(None, |_| None).unwrap();
    let files = changeset.files(&mut Buffers(Vec::new()));
    let paths: Vec<&Path> = files.iter().map(|entry| entry.path.as_path()).collect();
    assert_eq!(paths, [a.as_path(), b.as_path()]);
}

#[gpui::test]
fn a_previewed_pending_new_file_is_edited_again(cx: &mut TestAppContext) {
    let dir = folder();
    let path = dir.path().join("new.txt");
    let (workbench, cx) = opened(cx, dir.path());
    let proposals = Rc::new(RefCell::new(ChatProposals::new()));
    proposals.borrow_mut().begin_turn("chat:0", ai());
    propose_to(
        &workbench,
        &proposals,
        "new.txt",
        vec![edit("", "draft\n")],
        cx,
    )
    .unwrap();
    assert!(hand_in(&workbench, &proposals, cx));
    // The user looks at it: a read-only preview of its text.
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.open_ai_file(path.clone(), window, cx)
        })
    });
    cx.run_until_parked();
    let preview = editor_of(&workbench, &path, cx);
    assert!(preview.read_with(cx, |editor, _| editor.is_read_only()));

    proposals.borrow_mut().begin_turn("chat:1", ai());
    assert_eq!(
        read(&workbench, &proposals, "new.txt", cx).unwrap(),
        "[includes 1 pending changes you proposed]\ndraft\n"
    );
    propose_to(
        &workbench,
        &proposals,
        "new.txt",
        vec![edit("draft", "final")],
        cx,
    )
    .unwrap();
    assert_eq!(shown_counts(&workbench, &path, cx), (0, 1));
    assert!(proposals.borrow().is_new_file_superseded("chat:0", &path));
    let changeset = proposals.borrow_mut().end_turn(None, |_| None).unwrap();
    assert_eq!(changeset.new_file_text(&path), Some("final\n"));
    assert!(!path.exists());
}

#[gpui::test]
fn an_unopened_svg_is_proposed_to_as_text(cx: &mut TestAppContext) {
    let dir = folder();
    let path = dir.path().join("icon.svg");
    let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\">\n</svg>\n";
    fs::write(&path, svg).unwrap();
    let (workbench, cx) = opened(cx, dir.path());
    let proposals = Rc::new(RefCell::new(ChatProposals::new()));
    proposals.borrow_mut().begin_turn("chat:0", ai());
    let proposed = propose_to(
        &workbench,
        &proposals,
        "icon.svg",
        vec![edit("\n</svg>", "\n<rect/>\n</svg>")],
        cx,
    );
    assert_eq!(
        proposed.unwrap(),
        "Proposed 1 changes to icon.svg; the user will review them."
    );
    assert_eq!(
        read(&workbench, &proposals, "icon.svg", cx).unwrap(),
        "[includes 1 pending changes you proposed]\n\
         <svg xmlns=\"http://www.w3.org/2000/svg\">\n<rect/>\n</svg>\n"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), svg);
}

#[gpui::test]
fn the_executors_never_open_a_fifo(cx: &mut TestAppContext) {
    let dir = folder();
    #[cfg(unix)]
    {
        let fifo = std::ffi::CString::new(dir.path().join("pipe.txt").to_str().unwrap()).unwrap();
        // SAFETY: a NUL-terminated path that outlives the call.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    }
    let (workbench, cx) = opened(cx, dir.path());
    let proposals = Rc::new(RefCell::new(ChatProposals::new()));
    proposals.borrow_mut().begin_turn("chat:0", ai());
    #[cfg(unix)]
    {
        assert!(read(&workbench, &proposals, "pipe.txt", cx).is_err());
        let proposed = propose_to(
            &workbench,
            &proposals,
            "pipe.txt",
            vec![edit("", "x\n")],
            cx,
        );
        assert!(proposed.is_err());
    }
    assert!(proposals.borrow_mut().end_turn(None, |_| None).is_none());
}

#[gpui::test]
fn read_file_finds_unsaved_edits_in_another_window(cx: &mut TestAppContext) {
    let dir = folder();
    let path = dir.path().join("a.txt");
    fs::write(&path, "saved\n").unwrap();
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let window = |cx: &mut TestAppContext| {
        let launch = Launch {
            root: Some(dir.path().to_owned()),
            file: None,
        };
        let window = cx.add_window(move |window, cx| {
            let theme = fe_gpui::EditorTheme::default_dark(cx);
            Workbench::new(
                launch,
                theme,
                fe_gpui::EditorSettings::default(),
                window,
                cx,
            )
        });
        let workbench = window.root(cx).unwrap();
        (workbench, VisualTestContext::from_window(*window, cx))
    };
    let (asking, mut ask_cx) = window(cx);
    let (other, mut other_cx) = window(cx);
    // The other window has the file open, edited and not saved.
    let pane = other.read_with(&other_cx, |workbench, _| workbench.pane().clone());
    other_cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path.clone(), window, cx)));
    other_cx.run_until_parked();
    let editor = editor_of(&other, &path, &mut other_cx);
    editor.update(&mut other_cx, |editor, _| {
        let mut buffer = editor.model_mut().buffer_mut();
        buffer.edit([(0..5, "unsaved")], Origin::User);
    });
    other_cx.run_until_parked();

    let proposals = Rc::new(RefCell::new(ChatProposals::new()));
    ask_cx.update(|window, _| window.activate_window());
    assert_eq!(
        read(&asking, &proposals, "a.txt", &mut ask_cx).unwrap(),
        "unsaved\n"
    );
}

#[gpui::test]
fn a_proposal_to_a_file_open_in_another_window_is_reviewed_here(cx: &mut TestAppContext) {
    let dir = folder();
    let path = dir.path().join("a.txt");
    fs::write(&path, "saved\nline\n").unwrap();
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let window = |cx: &mut TestAppContext| {
        let launch = Launch {
            root: Some(dir.path().to_owned()),
            file: None,
        };
        let window = cx.add_window(move |window, cx| {
            let theme = fe_gpui::EditorTheme::default_dark(cx);
            Workbench::new(
                launch,
                theme,
                fe_gpui::EditorSettings::default(),
                window,
                cx,
            )
        });
        let workbench = window.root(cx).unwrap();
        (workbench, VisualTestContext::from_window(*window, cx))
    };
    let (asking, mut ask_cx) = window(cx);
    let (other, mut other_cx) = window(cx);
    let pane = other.read_with(&other_cx, |workbench, _| workbench.pane().clone());
    other_cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path.clone(), window, cx)));
    other_cx.run_until_parked();
    let remote = editor_of(&other, &path, &mut other_cx);
    remote.update(&mut other_cx, |editor, _| {
        let mut buffer = editor.model_mut().buffer_mut();
        buffer.edit([(0..5, "unsaved")], Origin::User);
    });
    other_cx.run_until_parked();

    let proposals = Rc::new(RefCell::new(ChatProposals::new()));
    ask_cx.update(|window, _| window.activate_window());
    proposals.borrow_mut().begin_turn("chat:0", ai());
    propose_to(
        &asking,
        &proposals,
        "a.txt",
        vec![edit("line", "LINE")],
        &mut ask_cx,
    )
    .unwrap();
    // Its tab here closed before the turn ends, while the other window's
    // stays: the turn's end gives it a tab here again, on the same buffer.
    assert!(is_open(&asking, &path, &mut ask_cx));
    take_tab(&asking, &path, &mut ask_cx);
    assert!(!is_open(&asking, &path, &mut ask_cx));
    assert!(hand_in(&asking, &proposals, &mut ask_cx));
    // This window has a tab on the same buffer, unsaved edits and all,
    // showing the proposal its AI Changes panel reaches.
    let local = editor_of(&asking, &path, &mut ask_cx);
    let proposal = proposals.borrow().proposal(&path).unwrap();
    local.read_with(&ask_cx, |editor, _| {
        assert_eq!(editor.model().text().to_string(), "unsaved\nline\n");
        assert_eq!(editor.model().buffer().id(), proposal.borrow().buffer());
        assert!(
            editor
                .proposals()
                .iter()
                .any(|shown| Rc::ptr_eq(shown, &proposal))
        );
    });
    // Accepted from the changeset, it lands in the buffer both windows show.
    ask_cx.update(|window, cx| {
        asking.update(cx, |workbench, cx| {
            workbench.decide_ai_file(&path, true, window, cx)
        })
    });
    ask_cx.run_until_parked();
    let text = remote.read_with(&other_cx, |editor, _| {
        editor.model().buffer().text().to_string()
    });
    assert_eq!(text, "unsaved\nLINE\n");
    // And Revert Turn takes it back out of that buffer.
    ask_cx.update(|window, cx| {
        asking.update(cx, |workbench, cx| {
            workbench.run_ai_changes_op(crate::ai_changes_panel::AiChangesOp::Revert, window, cx)
        })
    });
    ask_cx.run_until_parked();
    let text = remote.read_with(&other_cx, |editor, _| {
        editor.model().buffer().text().to_string()
    });
    assert_eq!(text, "unsaved\nline\n");
}

#[gpui::test]
fn a_stale_hunk_undone_and_closed_reads_as_no_longer_applying(cx: &mut TestAppContext) {
    let dir = folder();
    let a = dir.path().join("a.txt");
    fs::write(&a, "one\ntwo\n").unwrap();
    let (workbench, cx) = opened(cx, dir.path());
    let proposals = Rc::new(RefCell::new(ChatProposals::new()));
    proposals.borrow_mut().begin_turn("chat:0", ai());
    propose_to(
        &workbench,
        &proposals,
        "a.txt",
        vec![edit("one", "ONE")],
        cx,
    )
    .unwrap();
    assert!(hand_in(&workbench, &proposals, cx));
    let proposal = proposals.borrow().proposal(&a).unwrap();
    let held = proposal.borrow().buffer();
    // The user edits under the hunk, which the review finds stale, undoes
    // the edit, and closes the tab.
    let editor = editor_of(&workbench, &a, cx);
    editor.update(cx, |editor, _| {
        let mut buffer = editor.model_mut().buffer_mut();
        buffer.edit([(0..3, "uno")], Origin::User);
    });
    workbench.update(cx, |workbench, cx| workbench.sync_ai_changes(cx));
    cx.run_until_parked();
    assert_eq!(proposal.borrow().status_count(HunkStatus::Stale), 1);
    editor.update(cx, |editor, _| {
        let mut buffer = editor.model_mut().buffer_mut();
        buffer.undo(|_| true).unwrap();
    });
    cx.run_until_parked();
    close_active(&workbench, cx);
    assert!(!is_open(&workbench, &a, cx));
    // Read again: the review can't follow a stale hunk, and the chat
    // doesn't make it pending behind its back.
    proposals.borrow_mut().begin_turn("chat:1", ai());
    assert_eq!(
        read(&workbench, &proposals, "a.txt", cx).unwrap(),
        "[1 of your earlier changes no longer apply]\none\ntwo\n"
    );
    assert_eq!(proposal.borrow().buffer(), held);
    assert_eq!(proposal.borrow().status_count(HunkStatus::Pending), 0);
}

#[gpui::test]
fn decisions_before_a_review_is_replaced_are_kept(cx: &mut TestAppContext) {
    let dir = folder();
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(dir.path().join(name), format!("{name}\n")).unwrap();
    }
    let path = |name: &str| dir.path().join(name);
    let (workbench, cx) = opened(cx, dir.path());
    let proposals = Rc::new(RefCell::new(ChatProposals::new()));
    let listed = |workbench: &Entity<Workbench>, cx: &mut VisualTestContext| {
        workbench.update(cx, |workbench, _| {
            let changeset = &mut workbench.ai_changes.as_mut().expect("shown").changeset;
            (changeset.files(&mut Buffers(Vec::new())).into_iter())
                .map(|entry| entry.path)
                .collect::<Vec<_>>()
        })
    };

    // Decided in turn 0's review, then the next turn's replaces it with no
    // tool call between: nothing decided is carried.
    proposals.borrow_mut().begin_turn("chat:0", ai());
    propose_to(&workbench, &proposals, "a.txt", vec![edit("a", "A")], cx).unwrap();
    propose_to(&workbench, &proposals, "n.txt", vec![edit("", "new\n")], cx).unwrap();
    assert!(hand_in(&workbench, &proposals, cx));
    for (name, accept) in [("a.txt", true), ("n.txt", false)] {
        cx.update(|window, cx| {
            workbench.update(cx, |workbench, cx| {
                workbench.decide_ai_file(&path(name), accept, window, cx)
            })
        });
        cx.run_until_parked();
    }
    proposals.borrow_mut().begin_turn("chat:1", ai());
    propose_to(&workbench, &proposals, "b.txt", vec![edit("b", "B")], cx).unwrap();
    assert!(hand_in(&workbench, &proposals, cx));
    assert_eq!(listed(&workbench, cx), [path("b.txt")]);

    // A new file accepted, then another review shown in place of the
    // chat's: the file's buffer says it was decided.
    proposals.borrow_mut().begin_turn("chat:2", ai());
    propose_to(
        &workbench,
        &proposals,
        "m.txt",
        vec![edit("", "made\n")],
        cx,
    )
    .unwrap();
    assert!(hand_in(&workbench, &proposals, cx));
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.decide_ai_file(&path("m.txt"), true, window, cx)
        })
    });
    cx.run_until_parked();
    assert!(is_open(&workbench, &path("m.txt"), cx));
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.set_changeset(Changeset::new("review", Origin::User), window, cx)
        })
    });
    cx.run_until_parked();
    proposals.borrow_mut().begin_turn("chat:3", ai());
    propose_to(&workbench, &proposals, "c.txt", vec![edit("c", "C")], cx).unwrap();
    assert!(hand_in(&workbench, &proposals, cx));
    // b's change, pending in the review replaced, comes back with c's; m,
    // accepted, doesn't.
    assert_eq!(listed(&workbench, cx), [path("c.txt"), path("b.txt")]);
}

#[gpui::test]
fn ending_a_turn_hands_in_closed_files_and_carries_pending_work(cx: &mut TestAppContext) {
    let dir = folder();
    let a = dir.path().join("a.txt");
    let b = dir.path().join("b.txt");
    fs::write(&a, "one\n").unwrap();
    fs::write(&b, "two\n").unwrap();
    let (workbench, cx) = opened(cx, dir.path());
    let proposals = Rc::new(RefCell::new(ChatProposals::new()));

    // Its tab closed before the turn ends: opened again for the review.
    proposals.borrow_mut().begin_turn("chat:0", ai());
    propose_to(
        &workbench,
        &proposals,
        "a.txt",
        vec![edit("one", "ONE")],
        cx,
    )
    .unwrap();
    close_active(&workbench, cx);
    assert!(!is_open(&workbench, &a, cx));
    assert!(hand_in(&workbench, &proposals, cx));
    let proposal = proposals.borrow().proposal(&a).unwrap();
    let editor = editor_of(&workbench, &a, cx);
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.model().buffer().id(), proposal.borrow().buffer());
        assert!(
            editor
                .proposals()
                .iter()
                .any(|shown| Rc::ptr_eq(shown, &proposal))
        );
    });
    assert_eq!(shown_counts(&workbench, &a, cx), (1, 0));

    // The next turn's review takes a's pending change too, and decides it.
    proposals.borrow_mut().begin_turn("chat:1", ai());
    propose_to(
        &workbench,
        &proposals,
        "b.txt",
        vec![edit("two", "TWO")],
        cx,
    )
    .unwrap();
    assert!(hand_in(&workbench, &proposals, cx));
    let turn = workbench.read_with(cx, |workbench, _| {
        (workbench.ai_changes.as_ref()).map(|changes| changes.changeset.turn_id().to_owned())
    });
    assert_eq!(turn.as_deref(), Some("chat:1"));
    assert_eq!(shown_counts(&workbench, &a, cx), (1, 0));
    assert_eq!(shown_counts(&workbench, &b, cx), (1, 0));
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.decide_ai_file(&a, true, window, cx)
        })
    });
    cx.run_until_parked();
    let text = editor.read_with(cx, |editor, _| editor.model().buffer().text().to_string());
    assert_eq!(text, "ONE\n");
    // With nothing left undecided but what the shown review holds, a turn
    // that proposed nothing sets none.
    proposals.borrow_mut().begin_turn("chat:2", ai());
    assert!(!hand_in(&workbench, &proposals, cx));
}

#[test]
fn the_charset_decode_reads_only_the_bytes_vetted() {
    let dir = folder();
    fs::write(
        dir.path().join(".editorconfig"),
        "root = true\n[*]\ncharset = latin1\n",
    )
    .unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, b"caf\xe9\n").unwrap();
    let bytes = fs::read(&path).unwrap();
    let stamp = DiskStamp::from_metadata(&fs::metadata(&path).unwrap()).ok();
    // The file grows and changes after its bytes were taken: the buffer is
    // of those bytes, in the charset's encoding, stamped as they were read.
    fs::write(&path, vec![b'x'; 2 * MAX_FILE]).unwrap();
    let buffer = load(&bytes, &path, stamp).unwrap();
    assert_eq!(buffer.text().to_string(), "caf\u{e9}\n");
    assert_eq!(buffer.disk_stamp(), stamp);
    let same = dir.path().join("b.txt");
    fs::write(&same, b"caf\xe9\n").unwrap();
    let theirs = crate::loader::read(same).unwrap().unwrap();
    assert_eq!(buffer.encoding(), theirs.encoding());
    assert!(!buffer.is_dirty());
}

#[gpui::test]
fn a_replaced_review_still_decides_for_the_chat(cx: &mut TestAppContext) {
    let dir = folder();
    let path = |name: &str| dir.path().join(name);
    fs::write(path("a.txt"), "one\n").unwrap();
    fs::write(path("b.txt"), "two\n").unwrap();
    let (workbench, cx) = opened(cx, dir.path());
    let proposals = Rc::new(RefCell::new(ChatProposals::new()));
    proposals.borrow_mut().begin_turn("chat:0", ai());
    propose_to(
        &workbench,
        &proposals,
        "a.txt",
        vec![edit("one", "ONE")],
        cx,
    )
    .unwrap();
    propose_to(
        &workbench,
        &proposals,
        "n.txt",
        vec![edit("", "rejected\n")],
        cx,
    )
    .unwrap();
    propose_to(
        &workbench,
        &proposals,
        "m.txt",
        vec![edit("", "accepted\n")],
        cx,
    )
    .unwrap();
    assert!(hand_in(&workbench, &proposals, cx));
    let proposal = proposals.borrow().proposal(&path("a.txt")).unwrap();

    // n rejected and m accepted, then another review replaces the chat's
    // at once, with no tool call between; then m's tab and a's close.
    for (name, accept) in [("n.txt", false), ("m.txt", true)] {
        cx.update(|window, cx| {
            workbench.update(cx, |workbench, cx| {
                workbench.decide_ai_file(&path(name), accept, window, cx)
            })
        });
        cx.run_until_parked();
    }
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.set_changeset(Changeset::new("review", Origin::User), window, cx)
        })
    });
    cx.run_until_parked();
    take_tab(&workbench, &path("m.txt"), cx);
    take_tab(&workbench, &path("a.txt"), cx);
    assert!(!is_open(&workbench, &path("m.txt"), cx));
    assert!(!is_open(&workbench, &path("a.txt"), cx));

    // The review given back still says what was decided: neither new file
    // is pending to the chat.
    proposals.borrow_mut().begin_turn("chat:1", ai());
    for name in ["n.txt", "m.txt"] {
        assert!(
            !proposals.borrow_mut().pending_new_file(&path(name), None),
            "{name}"
        );
        assert!(read(&workbench, &proposals, name, cx).is_err(), "{name}");
    }
    // a's pending change follows its reopened buffer through that review.
    assert_eq!(
        read(&workbench, &proposals, "a.txt", cx).unwrap(),
        "[includes 1 pending changes you proposed]\nONE\n"
    );
    let editor = editor_of(&workbench, &path("a.txt"), cx);
    let buffer = editor.read_with(cx, |editor, _| editor.model().buffer().id());
    assert_eq!(proposal.borrow().buffer(), buffer);
    // The next review carries a, and nothing decided.
    propose_to(
        &workbench,
        &proposals,
        "b.txt",
        vec![edit("two", "TWO")],
        cx,
    )
    .unwrap();
    assert!(hand_in(&workbench, &proposals, cx));
    let listed = workbench.update(cx, |workbench, _| {
        let changeset = &mut workbench.ai_changes.as_mut().expect("shown").changeset;
        (changeset.files(&mut Buffers(Vec::new())).into_iter())
            .map(|entry| (entry.path, entry.counts.pending))
            .collect::<Vec<_>>()
    });
    assert_eq!(listed, [(path("b.txt"), 1), (path("a.txt"), 1)]);
    editor.read_with(cx, |editor, _| {
        assert!(
            editor
                .proposals()
                .iter()
                .any(|shown| Rc::ptr_eq(shown, &proposal))
        );
    });
}

#[gpui::test]
fn decisions_made_after_a_review_is_replaced_survive_a_reopen(cx: &mut TestAppContext) {
    let dir = folder();
    let path = |name: &str| dir.path().join(name);
    fs::write(path("a.txt"), "one\ntwo\nthree\nfour\n").unwrap();
    fs::write(path("b.txt"), "red\nblue\n").unwrap();
    fs::write(path("c.txt"), "x\ny\nz\nw\n").unwrap();
    fs::write(path("d.txt"), "dee\n").unwrap();
    let (workbench, cx) = opened(cx, dir.path());
    let proposals = Rc::new(RefCell::new(ChatProposals::new()));
    proposals.borrow_mut().begin_turn("chat:0", ai());
    let two = |first: &str, second: &str| {
        vec![
            edit(first, &first.to_uppercase()),
            edit(second, &second.to_uppercase()),
        ]
    };
    propose_to(&workbench, &proposals, "a.txt", two("one", "four"), cx).unwrap();
    propose_to(
        &workbench,
        &proposals,
        "b.txt",
        vec![edit("red", "RED")],
        cx,
    )
    .unwrap();
    propose_to(&workbench, &proposals, "c.txt", two("x", "w"), cx).unwrap();
    assert!(hand_in(&workbench, &proposals, cx));
    let proposal = |name: &str| proposals.borrow().proposal(&path(name)).unwrap();
    let (a, b, c) = (proposal("a.txt"), proposal("b.txt"), proposal("c.txt"));
    assert_eq!((a.borrow().hunk_count(), c.borrow().hunk_count()), (2, 2));
    // Another review replaces the chat's.
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.set_changeset(Changeset::new("review", Origin::User), window, cx)
        })
    });
    cx.run_until_parked();
    // Then, in the editors: a's first hunk rejected, c's first accepted,
    // and b's hunk found stale under an edit.
    let first_hunk =
        |name: &str, proposal: &Rc<RefCell<ProposedChange>>, cx: &mut VisualTestContext| {
            let editor = editor_of(&workbench, &path(name), cx);
            let snapshot = editor.read_with(cx, |e, _| e.model().buffer().snapshot());
            (editor, proposal.borrow().hunks(&snapshot)[0].id)
        };
    let (a_editor, id) = first_hunk("a.txt", &a, cx);
    a_editor
        .update(cx, |editor, cx| editor.reject_hunk(id, cx))
        .unwrap();
    let (c_editor, id) = first_hunk("c.txt", &c, cx);
    c_editor
        .update(cx, |editor, cx| editor.accept_hunk(id, cx))
        .unwrap();
    cx.run_until_parked();
    let b_editor = editor_of(&workbench, &path("b.txt"), cx);
    b_editor.update(cx, |editor, _| {
        let mut buffer = editor.model_mut().buffer_mut();
        buffer.edit([(0..3, "rot")], Origin::User);
    });
    let b_now = b_editor.read_with(cx, |e, _| e.model().buffer().snapshot());
    assert_eq!(b.borrow().hunks(&b_now)[0].status, HunkStatus::Stale);
    // The accept is under turn 0's review's checkpoint, which the chat kept.
    let kept_counts = |cx: &mut VisualTestContext| {
        let editors = workbench.read_with(cx, |workbench, cx| workbench.open_editors(cx));
        let mut made = crate::ai_changes_buffers::Made::default();
        cx.update(|_, cx| {
            crate::ai_changes_buffers::with_editors(&editors, &mut made, cx, |set| {
                let mut proposals = proposals.borrow_mut();
                let kept = proposals.kept.get_mut("chat:0").expect("kept");
                (kept.files(set).into_iter())
                    .find(|entry| entry.path == path("c.txt"))
                    .map(|entry| entry.counts.accepted)
            })
        })
    };
    assert_eq!(kept_counts(cx), Some(1));
    // Revert Turn on that review undoes it, as only a checkpointed accept
    // can be; c's proposal is then pending again, whole.
    let editors = workbench.read_with(cx, |workbench, cx| workbench.open_editors(cx));
    let mut made = crate::ai_changes_buffers::Made::default();
    let reverted = cx.update(|_, cx| {
        crate::ai_changes_buffers::with_editors(&editors, &mut made, cx, |set| {
            let mut proposals = proposals.borrow_mut();
            proposals.kept.get_mut("chat:0").expect("kept").revert(set)
        })
    });
    assert!(reverted.contains(&fe_editor::RevertOutcome::Reverted {
        path: path("c.txt"),
        reapplied: true
    }));
    cx.run_until_parked();
    let c_text = c_editor.read_with(cx, |e, _| e.model().buffer().text().to_string());
    assert_eq!(c_text, "x\ny\nz\nw\n");

    // a's and b's tabs close, unsaved edits dropped, and are read again.
    take_tab(&workbench, &path("a.txt"), cx);
    take_tab(&workbench, &path("b.txt"), cx);
    proposals.borrow_mut().begin_turn("chat:1", ai());
    // a: the rejection stands, the untouched hunk is still pending.
    assert_eq!(
        read(&workbench, &proposals, "a.txt", cx).unwrap(),
        "[includes 1 pending changes you proposed]\none\ntwo\nthree\nFOUR\n"
    );
    let a_now = editor_of(&workbench, &path("a.txt"), cx);
    let a_buffer = a_now.read_with(cx, |e, _| e.model().buffer().id());
    assert_eq!(a.borrow().buffer(), a_buffer);
    assert_eq!(
        (
            a.borrow().status_count(HunkStatus::Rejected),
            a.borrow().status_count(HunkStatus::Pending)
        ),
        (1, 1)
    );
    // b: a hunk found stale isn't made pending again; it no longer applies.
    assert_eq!(
        read(&workbench, &proposals, "b.txt", cx).unwrap(),
        "[1 of your earlier changes no longer apply]\nred\nblue\n"
    );
    assert_eq!(b.borrow().status_count(HunkStatus::Pending), 0);

    // The next review takes a's and c's pending changes, and they accept.
    propose_to(
        &workbench,
        &proposals,
        "d.txt",
        vec![edit("dee", "DEE")],
        cx,
    )
    .unwrap();
    assert!(hand_in(&workbench, &proposals, cx));
    let pending =
        |name: &str, cx: &mut VisualTestContext| shown_counts(&workbench, &path(name), cx).0;
    assert_eq!((pending("a.txt", cx), pending("c.txt", cx)), (1, 2));
    for name in ["a.txt", "c.txt"] {
        cx.update(|window, cx| {
            workbench.update(cx, |workbench, cx| {
                workbench.decide_ai_file(&path(name), true, window, cx)
            })
        });
        cx.run_until_parked();
    }
    let text = |editor: &Entity<EditorView>, cx: &mut VisualTestContext| {
        editor.read_with(cx, |e, _| e.model().buffer().text().to_string())
    };
    assert_eq!(text(&a_now, cx), "one\ntwo\nthree\nFOUR\n");
    assert_eq!(text(&c_editor, cx), "X\ny\nz\nW\n");
}

#[gpui::test]
fn a_file_only_a_diff_shows_is_proposed_on_a_tab_the_review_reaches(cx: &mut TestAppContext) {
    let dir = folder();
    let root = dir.path();
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap()
            .status;
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    fs::write(root.join("a.txt"), "one\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "init"]);
    fs::write(root.join("a.txt"), "one\ntwo\n").unwrap();
    let path = root.join("a.txt");
    let (workbench, cx) = opened(cx, root);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path.clone(), window, cx)));
    cx.run_until_parked();
    let focus = editor_of(&workbench, &path, cx).read_with(cx, gpui::Focusable::focus_handle);
    cx.update(|window, cx| window.focus(&focus, cx));
    let run = fe_gpui::commands::RunCommand(
        fe_command::Invocation::of("git.diff_head")
            .unwrap()
            .context(
                fe_command::ContextStack::new().with(fe_command::ContextFrame::new("workbench")),
            ),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    cx.run_until_parked();
    // Only the diff's working side shows the file now.
    take_tab(&workbench, &path, cx);
    let tabs = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |workbench, cx| {
            (workbench.open_editors(cx).iter())
                .filter(|editor| editor.read(cx).model().path() == Some(path.as_path()))
                .count()
        })
    };
    assert_eq!(tabs(cx), 0);
    assert!(is_open(&workbench, &path, cx), "the diff's working side");

    let proposals = Rc::new(RefCell::new(ChatProposals::new()));
    proposals.borrow_mut().begin_turn("chat:0", ai());
    propose_to(
        &workbench,
        &proposals,
        "a.txt",
        vec![edit("two", "TWO")],
        cx,
    )
    .unwrap();
    assert_eq!(tabs(cx), 1, "a tab sharing the diff's buffer");
    assert!(hand_in(&workbench, &proposals, cx));
    let proposal = proposals.borrow().proposal(&path).unwrap();
    let diff_side = workbench.read_with(cx, |workbench, cx| {
        (workbench.file_editors(cx).into_iter())
            .find(|editor| {
                editor.read(cx).model().path() == Some(path.as_path())
                    && !workbench.open_editors(cx).contains(editor)
            })
            .expect("the diff's working side")
    });
    let text = |cx: &mut VisualTestContext| {
        diff_side.read_with(cx, |editor, _| editor.model().buffer().text().to_string())
    };
    assert_eq!(
        diff_side.read_with(cx, |editor, _| editor.model().buffer().id()),
        proposal.borrow().buffer()
    );
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.decide_ai_file(&path, true, window, cx)
        })
    });
    cx.run_until_parked();
    assert_eq!(text(cx), "one\nTWO\n");
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.run_ai_changes_op(crate::ai_changes_panel::AiChangesOp::Revert, window, cx)
        })
    });
    cx.run_until_parked();
    assert_eq!(text(cx), "one\ntwo\n");
}
