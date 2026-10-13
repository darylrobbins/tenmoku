# proposal_demo example

Status: **decided**. Source card T54 (Mac T260). Code:
`crates/fe-gpui/examples/proposal_demo.rs`, entry in `crates/fe-gpui/Cargo.toml`.

Uses only public API of fe-buffer, fe-editor and fe-gpui (plus `fe_syntax::language_name_for_path`
for the language); no `crates/*/src` file changes.

## C2: bad arguments exit with status 2 before any window opens

No arguments:

```
usage: proposal_demo <file> <proposed-file>
exit status 2
```

Unreadable files:

```
/nonexistent: No such file or directory (os error 2)
/nonexistent: No such file or directory (os error 2)
usage: proposal_demo <file> <proposed-file>
exit status 2
```

## C3: invocations and proposal events

```rust
fe_gpui::commands::observe_invocations(cx, |invocation| {
    let command = invocation.command.as_str();
    if command.starts_with("proposal.") || matches!(command, "editor.undo" | "editor.redo")
    {
        println!("invoked: {command} {}", invocation.args);
    }
});
```

```rust
cx.subscribe(&view, |_, event: &ProposalEvent, _| match event {
    ProposalEvent::Accepted { hunk, transaction } => {
        println!("event: accepted {hunk:?} as {transaction:?}");
    }
    ProposalEvent::Rejected { hunk } => println!("event: rejected {hunk:?}"),
})
```

(`fe_gpui::commands::observe_invocations` is the re-export of kiln-platform's.)

## Dry runs per edit

The view is observed, not polled: each buffer `Edited` event prints `edit: <transaction> <kind> to <version>`
and the dry run. A buffer snapshot of an older version can't be had through public API, so if several edits land
within one notification only the last has its dry run, the others print `dry run: superseded by a later edit`.
Typed at 1 ms between keys under Xvfb, each of three edits got its own notification and dry run.

## Left to the Mac

Checked under Xvfb (build with `-p fe-gpui -p fastedit`, as only fastedit enables gpui's x11 feature): the start
dry run, `invoked: proposal.next_hunk`, per-edit dry runs. Not seen live here: accept/reject events and
`editor.undo`/`editor.redo` invocations (xdotool's ctrl-alt-y and ctrl-z didn't register), the Accept/Reject
buttons, and cmd-z on an accepted hunk on macOS.
