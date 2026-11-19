//! An agent edit under a root opened by another spelling of the same folder
//! (T695): an 8.3 short name or another letter case on Windows, another
//! letter case on case-insensitive APFS. The edit is located inside the
//! root, in the file system's spelling, and proposed there; and the file it
//! opened is the chat's to read, as automatic context and an @file, its
//! secrets still refused.
//!
//! Linux makes neither spelling, so there those cases are skipped (a root
//! reached through a link runs everywhere unix); see
//! `docs/decisions/mej5xavm0k-chat-edit-root-spelling.md`.

use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use fe_buffer::Origin;
use fe_chat::{ProposeEditInput, ProposedEdit};
use gpui::{Entity, TestAppContext, VisualTestContext};

use crate::{
    Launch, Workbench,
    agent_tools::{self, ChatProposals, ToolOutcome},
    chat_panel::{chat_apply, chat_context},
    tests::open,
};

/// The folder edits go under, in its canonical spelling: a long name with
/// letters, so it has a short name and a case to change.
fn canonical_root(temp: &tempfile::TempDir) -> PathBuf {
    let root = temp.path().join("Spelled Root Folder");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("notes.txt"), "one\ntwo\n").unwrap();
    dunce::canonicalize(root).unwrap()
}

/// `root` with the letter case of its last part swapped, if that names the
/// same folder here.
fn other_case(root: &Path) -> Option<PathBuf> {
    let name = root.file_name()?.to_str()?;
    let swapped: String = (name.chars())
        .map(|c| {
            if c.is_uppercase() {
                c.to_ascii_lowercase()
            } else {
                c.to_ascii_uppercase()
            }
        })
        .collect();
    let spelled = root.with_file_name(swapped);
    same_folder(root, spelled)
}

/// `root` by its 8.3 short name, if the volume makes them.
#[cfg(windows)]
fn short_name(root: &Path) -> Option<PathBuf> {
    use std::os::windows::process::CommandExt as _;
    let out = std::process::Command::new("cmd")
        .raw_arg(format!(
            "/D /C for %I in (\"{}\") do @echo %~sI",
            root.display()
        ))
        .output()
        .ok()?;
    let spelled = PathBuf::from(String::from_utf8(out.stdout).ok()?.trim());
    (spelled != root).then_some(())?;
    same_folder(root, spelled)
}

#[cfg(not(windows))]
fn short_name(_: &Path) -> Option<PathBuf> {
    None
}

/// `spelled`, if it's another spelling of `root` that names it.
fn same_folder(root: &Path, spelled: PathBuf) -> Option<PathBuf> {
    let differs = spelled.as_os_str() != root.as_os_str();
    let same = dunce::canonicalize(&spelled).is_ok_and(|real| real == root);
    (differs && same).then_some(spelled)
}

fn ai() -> Origin {
    Origin::Ai {
        agent: Arc::from("fastedit.chat"),
        session: Arc::from("chat"),
    }
}

fn propose(
    workbench: &Entity<Workbench>,
    proposals: &Rc<RefCell<ChatProposals>>,
    path: &str,
    cx: &mut VisualTestContext,
) -> ToolOutcome {
    let input = ProposeEditInput {
        path: path.to_owned(),
        edits: vec![ProposedEdit {
            old_text: "two".to_owned(),
            new_text: "TWO".to_owned(),
        }],
    };
    let slot: Rc<RefCell<Option<ToolOutcome>>> = Rc::default();
    cx.update(|window, cx| {
        let task = agent_tools::propose_edit(workbench, proposals, input, window, cx);
        let slot = slot.clone();
        cx.spawn(async move |_| *slot.borrow_mut() = Some(task.await))
            .detach();
    });
    cx.run_until_parked();
    slot.borrow_mut().take().expect("the tool finished")
}

/// The edit under `spelled`, another spelling of `root`: located inside
/// it, by its canonical path, and proposed to the file opened there.
fn edits_under(spelled: &Path, root: &Path, cx: &mut TestAppContext) {
    let notes = root.join("notes.txt");
    let located = chat_apply::locate(spelled, "notes.txt", &[]).expect("inside the root");
    assert_eq!(located.path, notes, "under {}", spelled.display());
    let new = chat_apply::locate(spelled, "src/new.txt", &[]).expect("inside the root");
    assert_eq!(new.path, root.join("src/new.txt"), "a file not there yet");
    assert_eq!(
        agent_tools::resolve(spelled, "notes.txt"),
        Ok(notes.clone())
    );

    let launch = Launch {
        root: Some(spelled.to_owned()),
        file: None,
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    let proposals = Rc::new(RefCell::new(ChatProposals::new()));
    proposals.borrow_mut().begin_turn("chat:0", ai());
    assert_eq!(
        propose(&workbench, &proposals, "notes.txt", cx),
        Ok("Proposed 1 changes to notes.txt; the user will review them.".to_owned()),
        "under {}",
        spelled.display()
    );
    assert!(proposals.borrow().proposal(&notes).is_some());
    let stamp = workbench.read_with(cx, |workbench, cx| {
        (workbench.file_editors(cx).into_iter())
            .find(|editor| editor.read(cx).model().path() == Some(notes.as_path()))
            .map(|editor| editor.read(cx).model().buffer().disk_stamp())
    });
    let stamp = stamp.expect("the file is open by its canonical path");

    // The file the edit opened is the chat's to read: automatic context
    // vets the active editor's path, an @file finds its buffer, against
    // the root as it was opened.
    assert_eq!(
        chat_context::vet_text(spelled, &notes, stamp.as_ref(), || {}),
        Ok(()),
        "automatic context under {}",
        spelled.display()
    );
    let open = [(notes.clone(), stamp)];
    let found = chat_context::find_source_with(spelled, &notes, &open, || {});
    assert!(
        matches!(found, Ok(chat_context::Source::Open(0))),
        "an @file under {}",
        spelled.display()
    );
    let joined = spelled.join("notes.txt");
    assert_eq!(
        chat_context::vet_text(spelled, &joined, stamp.as_ref(), || {}),
        Ok(()),
        "joined to the root as opened"
    );
    // Its secrets are still refused, by either spelling.
    fs::write(root.join(".env"), "KEY=1\n").unwrap();
    for secret in [root.join(".env"), spelled.join(".env")] {
        let refused = chat_context::open_vetted(spelled, &secret).map(|(real, _)| real);
        assert!(refused.is_err(), "{} refused", secret.display());
    }
}

#[gpui::test]
fn an_edit_under_a_root_in_another_letter_case_is_inside_it(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = canonical_root(&temp);
    let Some(spelled) = other_case(&root) else {
        eprintln!("skipped: {} has no other letter case here", root.display());
        return;
    };
    edits_under(&spelled, &root, cx);
}

#[gpui::test]
fn an_edit_under_a_root_by_its_short_name_is_inside_it(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = canonical_root(&temp);
    let Some(spelled) = short_name(&root) else {
        eprintln!("skipped: {} has no short name here", root.display());
        return;
    };
    edits_under(&spelled, &root, cx);
}

/// A spelling Linux makes too, which `real_path` resolved before: through a
/// link to the root.
#[cfg(unix)]
#[gpui::test]
fn an_edit_under_a_root_through_a_link_is_inside_it(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = canonical_root(&temp);
    let link = temp.path().join("link");
    std::os::unix::fs::symlink(&root, &link).unwrap();
    let spelled = same_folder(&root, link).expect("the link names the root");
    edits_under(&spelled, &root, cx);
}
