//! The git and diff commands routed to their features: listed in the
//! palette, the stubs answering `Not supported yet`; typed as fugitive's and
//! gitsigns' ex commands; host commands from the editors a feature shows
//! outside the tabs handed to it; and a diff view's focused side taken for
//! the active editor.

use std::path::Path;

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_gpui::{DiffSide, DiffView, EditorSettings, EditorTheme, EditorView, set_vim_enabled};
use fe_vim::HostCommand;
use gpui::{
    AppContext as _, Entity, Focusable as _, SharedString, TestAppContext, VisualTestContext,
};
use kiln_shell::{
    ItemId, PaneView as _, Picker,
    command_palette::{COMMAND_PALETTE, CommandPalette},
};

use super::{Feature, GitCommand, GitOp, Owner, Routed};
use crate::{Launch, StatusItem, StatusSide, Workbench};

const TEXT: &str = "one\ntwo\nthree\nfour\n";

/// A window of a workbench rooted at `root`, showing `a.txt` from it,
/// focused, with vim on.
fn window(cx: &mut TestAppContext, root: &Path) -> (Entity<Workbench>, VisualTestContext) {
    std::fs::write(root.join("a.txt"), TEXT).unwrap();
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let window = cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let workbench = window.root(cx).unwrap();
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    let cx = &mut vcx;
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = root.join("a.txt");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    (workbench, vcx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Types `:{line}` and Enter.
fn ex(line: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(":");
    cx.simulate_input(line);
    cx.simulate_keystrokes("enter");
    draw(cx);
}

fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("an editor is active")
}

/// Where the git ops and owned editors' host commands went, emptied.
fn routed(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Routed> {
    workbench.update(cx, |workbench, _| std::mem::take(&mut workbench.git_routed))
}

/// `editor`'s vim message, emptied.
fn message(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| {
        editor.update(cx, |editor, cx| {
            let message = editor.vim_state()?.message().map(str::to_owned);
            editor.set_vim_message("", cx);
            message.filter(|message| !message.is_empty())
        })
    })
}

/// What the window's shown toasts say, top first.
fn toasts(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    })
}

fn palette(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<Entity<Picker<CommandPalette>>> {
    workbench.read_with(cx, |workbench, cx| {
        let layer = workbench.shell().read(cx).modals().read(cx);
        (layer.modal() == Some(COMMAND_PALETTE))
            .then(|| layer.modal_view().unwrap().clone().downcast().unwrap())
    })
}

/// The card's commands: id and palette title.
const CARD: [(&str, &str); 27] = [
    ("git.diff_index", "Git: Compare with Index"),
    ("git.diff_head", "Git: Compare with HEAD"),
    ("diff.compare_file", "Compare Active File With…"),
    ("git.next_change", "Go to Next Change"),
    ("git.previous_change", "Go to Previous Change"),
    ("git.stage_file", "Git: Stage File"),
    ("git.unstage_file", "Git: Unstage File"),
    ("git.discard_file", "Git: Discard File Changes"),
    ("git.reset_buffer", "Git: Revert Editor to Index"),
    ("git.stage_hunk", "Git: Stage Change"),
    ("git.unstage_hunk", "Git: Undo Stage Change"),
    ("git.revert_hunk", "Git: Revert Change"),
    ("git.commit", "Git: Commit Staged"),
    ("git.commit_amend", "Git: Commit Staged (Amend)"),
    ("git.toggle_line_blame", "Git: Toggle Line Blame"),
    ("git.blame_line", "Git: Show Line Blame"),
    ("git.open_line_commit", "Git: Open Line's Commit Change"),
    ("git.switch_branch", "Git: Switch Branch…"),
    ("git.create_branch", "Git: Create Branch…"),
    ("git.stash", "Git: Stash"),
    (
        "git.stash_include_untracked",
        "Git: Stash (Include Untracked)",
    ),
    ("git.stash_pop", "Git: Pop Stash…"),
    ("git.stash_apply", "Git: Apply Stash…"),
    ("git.fetch", "Git: Fetch"),
    ("git.pull", "Git: Pull"),
    ("git.push", "Git: Push"),
    ("git.push_force_with_lease", "Git: Push (Force with Lease)"),
];

#[gpui::test]
fn palette_lists(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, mut cx) = window(cx, dir.path());
    let cx = &mut cx;
    let editor = active(&workbench, cx);
    cx.simulate_keystrokes("cmd-shift-p");
    draw(cx);
    let picker = palette(&workbench, cx).expect("the palette shows");
    let listed: Vec<(String, String)> = picker.read_with(cx, |picker, _| {
        (picker.delegate.matches().iter())
            .map(|entry| (entry.id.as_str().to_owned(), entry.title.to_string()))
            .collect()
    });
    for (id, title) in CARD {
        let entry = (id.to_owned(), title.to_owned());
        assert!(listed.contains(&entry), "{entry:?} in {listed:?}");
    }

    cx.simulate_input("git: stage file");
    draw(cx);
    let first = picker.read_with(cx, |picker, _| picker.delegate.matches()[0].title.clone());
    assert_eq!(first.as_ref(), "Git: Stage File");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(palette(&workbench, cx).is_none());
    let op = GitOp {
        command: GitCommand::StageFile,
        args: String::new(),
        range: None,
        editor: Some(editor.clone()),
        typed: false,
    };
    assert_eq!(routed(&workbench, cx), [Routed::Op(op)]);
    // Saved, then staged where there's no repository.
    assert_eq!(toasts(&workbench, cx), ["Not in a git repository"]);
    // Not on the vim line: the palette isn't vim's.
    assert_eq!(message(&editor, cx), None);
}

#[gpui::test]
fn ex_routes(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, mut cx) = window(cx, dir.path());
    let cx = &mut cx;
    let editor = active(&workbench, cx);
    let op = |command, args: &str, range| GitOp {
        command,
        args: args.to_owned(),
        range,
        editor: Some(editor.clone()),
        typed: true,
    };
    use GitCommand::*;
    let lines = [
        ("Gdiffsplit", op(DiffIndex, "", None)),
        ("Gvdiffsplit", op(DiffIndex, "", None)),
        ("Gdiffsplit HEAD", op(DiffHead, "", None)),
        ("Gwrite", op(StageFile, "", None)),
        ("Gread", op(ResetBuffer, "", None)),
        ("Git commit", op(Commit, "", None)),
        ("Git commit --amend", op(CommitAmend, "", None)),
        ("G blame", op(ToggleBlameAnnotations, "", None)),
        ("Git switch", op(SwitchBranch, "", None)),
        ("Git checkout main", op(SwitchBranch, "main", None)),
        ("Git switch -c x", op(CreateBranch, "x", None)),
        ("Git checkout -b x", op(CreateBranch, "x", None)),
        (
            "Gitsigns toggle_current_line_blame",
            op(ToggleLineBlame, "", None),
        ),
        ("Gitsigns next_hunk", op(NextChange, "", None)),
        ("2Gitsigns reset_hunk", op(RevertHunk, "", Some(1..2))),
        ("diffsplit b.txt", op(CompareFile, "b.txt", None)),
    ];
    for (line, op) in lines {
        ex(line, cx);
        // The branch, commit, diff and staging commands are implemented;
        // the root isn't a repository.
        let answer = match op.command {
            DiffIndex | DiffHead | StageFile | ResetBuffer | Commit | CommitAmend => {
                "Not in a git repository".to_owned()
            }
            // `:G blame` annotates.
            ToggleBlameAnnotations => "Not in a git repository".to_owned(),
            NextChange => "No changes".to_owned(),
            RevertHunk => "No change to revert at this line".to_owned(),
            CompareFile => {
                let path = dir.path().join("b.txt");
                format!("Can't open {}: No such file", path.display())
            }
            _ if op.command.feature() == Feature::Branch => "Not in a git repository".to_owned(),
            _ => format!("Not supported yet: {}", op.command.title()),
        };
        // Line blame's toggle answers nothing.
        let answer = (op.command != ToggleLineBlame).then_some(answer);
        assert_eq!(routed(&workbench, cx), [Routed::Op(op)], ":{line}");
        assert_eq!(message(&editor, cx), answer, ":{line}");
    }

    // A visual selection's range.
    cx.simulate_keystrokes("j shift-v j :");
    cx.simulate_input("Gitsigns stage_hunk");
    cx.simulate_keystrokes("enter");
    draw(cx);
    let staged = op(StageHunk, "", Some(1..3));
    assert_eq!(routed(&workbench, cx), [Routed::Op(staged)]);
    let answer = "Not in a git repository";
    assert_eq!(message(&editor, cx).as_deref(), Some(answer));

    let answers = [
        ("Foo", "E492: Not an editor command: Foo"),
        ("Foo! bar", "E492: Not an editor command: Foo! bar"),
        ("Git frobnicate", "Not supported: :Git frobnicate"),
        ("Git", "Not supported: :Git"),
        ("Git switch -c", "Not supported: :Git switch -c"),
        ("Gdiffsplit main", "Not supported: :Gdiffsplit main"),
        ("Gitsigns frob", "Not supported: :Gitsigns frob"),
        ("Gwrite!", "Not supported: :Gwrite!"),
        ("2Gwrite", "E481: No range allowed"),
    ];
    for (line, answer) in answers {
        ex(line, cx);
        assert_eq!(routed(&workbench, cx), [], ":{line}");
        assert_eq!(message(&editor, cx).as_deref(), Some(answer), ":{line}");
    }
}

/// `:Git fetch`, `:Git pull` and `:Git push` are the palette's commands, typed;
/// force-pushing has no ex form, and nothing else follows those words.
#[gpui::test]
fn ex_forms_fetch_pull_push(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, mut cx) = window(cx, dir.path());
    let cx = &mut cx;
    let editor = active(&workbench, cx);
    let forms = [
        ("Git fetch", GitCommand::Fetch),
        ("G fetch", GitCommand::Fetch),
        ("Git pull", GitCommand::Pull),
        ("Git push", GitCommand::Push),
    ];
    for (line, command) in forms {
        ex(line, cx);
        let op = GitOp {
            command,
            args: String::new(),
            range: None,
            editor: Some(editor.clone()),
            typed: true,
        };
        assert_eq!(routed(&workbench, cx), [Routed::Op(op)], ":{line}");
        let answer = "Not in a git repository";
        assert_eq!(message(&editor, cx).as_deref(), Some(answer), ":{line}");
    }
    for line in [
        "Git push!",
        "Git! push",
        "Git push --force",
        "Git push --force-with-lease",
        "Git pull --rebase",
        "Git fetch origin",
    ] {
        ex(line, cx);
        assert_eq!(routed(&workbench, cx), [], ":{line}");
        let answer = format!("Not supported: :{line}");
        assert_eq!(message(&editor, cx), Some(answer), ":{line}");
    }
}

/// A diff view of a fixed text against `a.txt`'s, made the active pane's
/// active item. The pane can't draw it yet: the diff view card has it
/// shown in a tab.
fn diff_view(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<DiffView> {
    let diff = cx.update(|_, cx| {
        cx.new(|cx| {
            let left = EditorModel::new(Buffer::new("one\n"), None);
            let right = EditorModel::new(Buffer::new(TEXT), None);
            let left = DiffSide::new("Index", left).read_only();
            DiffView::new("a.txt", left, DiffSide::new("Working Tree", right), cx)
        })
    });
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|_, cx| {
        pane.update(cx, |pane, cx| {
            let ix = pane.pane_mut().push(Box::new(diff.clone()), cx);
            pane.pane_mut().set_active(ix);
            cx.notify();
        })
    });
    draw(cx);
    diff
}

/// Runs `command` as if typed in `editor`: what the status bar's line and
/// vim's keys hand the workbench. An editor the pane can't draw yet takes
/// no keys.
fn host_command(
    workbench: &Entity<Workbench>,
    editor: &Entity<EditorView>,
    command: HostCommand,
    cx: &mut VisualTestContext,
) {
    let editor = editor.downgrade();
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.run_vim_host_command(&editor, command, window, cx)
        })
    });
    draw(cx);
}

#[gpui::test]
fn owned_editors(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, mut cx) = window(cx, dir.path());
    let cx = &mut cx;
    let tab = active(&workbench, cx);

    // `do`, typed in a tab: not a diff.
    cx.simulate_keystrokes("d o");
    draw(cx);
    let e99 = "E99: Current buffer is not in diff mode";
    assert_eq!(message(&tab, cx).as_deref(), Some(e99));
    assert_eq!(routed(&workbench, cx), []);

    let diff = diff_view(&workbench, cx);
    let (left, right) = diff.read_with(cx, |diff, _| (diff.left().clone(), diff.right().clone()));
    // Standing in for the commit box: any editor outside the tabs.
    let commit_box = cx.update(|_, cx| {
        cx.new(|cx| {
            let model = EditorModel::new(Buffer::new(""), None);
            EditorView::new(model, EditorTheme::default_dark(cx), cx)
        })
    });
    workbench.update(cx, |workbench, _| {
        workbench.own_editor(&left, Owner::DiffView(diff.downgrade()));
        workbench.own_editor(&commit_box, Owner::CommitBox);
    });

    let write = HostCommand::Write {
        force: false,
        path: None,
    };
    let get = HostCommand::DiffGet { rows: 1..2 };
    for command in [write.clone(), get.clone()] {
        host_command(&workbench, &left, command.clone(), cx);
        assert_eq!(routed(&workbench, cx), [Routed::DiffView(command)]);
        // The left side is read-only.
        let answer = "E21: Cannot make changes, 'modifiable' is off";
        assert_eq!(message(&left, cx).as_deref(), Some(answer));
    }
    host_command(&workbench, &commit_box, write.clone(), cx);
    assert_eq!(routed(&workbench, cx), [Routed::CommitBox(write.clone())]);
    let answer = "Message kept; :wq commits";
    assert_eq!(message(&commit_box, cx).as_deref(), Some(answer));

    // The right side is no one's: ignored, as before.
    for command in [write, get] {
        host_command(&workbench, &right, command, cx);
        assert_eq!(routed(&workbench, cx), []);
        assert_eq!(message(&right, cx), None);
    }
    let dirty = right.read_with(cx, |right, _| right.model().is_dirty());
    assert!(!dirty);
}

#[gpui::test]
fn active_editor_in_diff_view(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, mut cx) = window(cx, dir.path());
    let cx = &mut cx;
    let tab = active(&workbench, cx);
    let diff = diff_view(&workbench, cx);
    let (left, right) = diff.read_with(cx, |diff, _| (diff.left().clone(), diff.right().clone()));
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    let followed = |cx: &mut VisualTestContext| bar.read_with(cx, |bar, _| bar.item_id());
    let vim_mode = |cx: &mut VisualTestContext| {
        bar.read_with(cx, |bar, _| bar.drawn(StatusSide::Left))
            .into_iter()
            .find(|(id, _)| id == StatusItem::VIM_MODE)
            .map(|(_, text)| text)
    };

    // The right side, the one to edit, until the left has focus.
    assert_eq!(active(&workbench, cx), right);
    assert_eq!(followed(cx), Some(ItemId(right.entity_id())));

    cx.update(|window, cx| window.focus(&left.focus_handle(cx), cx));
    draw(cx);
    assert_eq!(active(&workbench, cx), left);
    assert_eq!(followed(cx), Some(ItemId(left.entity_id())));
    assert_eq!(vim_mode(cx), Some(SharedString::from("NORMAL")));
    // Focus leaving the diff view, as for the vim line, keeps the side.
    cx.update(|window, cx| window.blur(cx));
    draw(cx);
    assert_eq!(active(&workbench, cx), left);

    cx.update(|window, cx| window.focus(&right.focus_handle(cx), cx));
    draw(cx);
    assert_eq!(active(&workbench, cx), right);
    assert_eq!(followed(cx), Some(ItemId(right.entity_id())));
    assert_ne!(tab, right);
}
