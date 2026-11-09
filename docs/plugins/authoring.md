# Writing a Tenmoku plugin

## Status: a preview

Code plugins are a **preview**. They are off by default, behind the setting
`extensions.plugins_preview`; with it off no plugin is compiled, loaded or
prompted for. The supported way to extend Tenmoku is a declarative extension:
themes, keymaps, menus, language servers, grammars and snippets.

The WIT is **unstable**. `kiln:extension@1.0.0` and `fastedit:editor@1.0.0` may
change incompatibly in any release until they are declared stable, so rebuild
your plugin against each release. The editor package keeps its pre-rename name,
`fastedit:editor`, during the preview.

## Quick start

1. Copy `crates/tenmoku-plugin/template/` somewhere outside the Tenmoku tree.
2. In the copy's `Cargo.toml`, point the SDK at your Tenmoku checkout:
   `tenmoku-plugin = { path = "/path/to/tenmoku/crates/tenmoku-plugin" }`.
3. Rename the plugin. Pick an extension id (`<you>.<name>`, each part
   `[a-z][a-z0-9_]*`, such as `acme.my_plugin`) and a crate name (`my-plugin`;
   cargo builds it as `my_plugin.wasm`). The template spells its name in these
   places, and each needs your name:
   - `extension.toml`: `id` (the extension id) and `wasm` (`my_plugin.wasm`).
   - `Cargo.toml`: `name` (`my-plugin`).
   - `build.zsh`: both `word_count.wasm` paths, the one it reads from
     `target/wasm32-wasip2/release/` and the one it copies to, and the comment.
   - `src/lib.rs`: the command id constant `COUNT`
     (`"example.word_count.count"`). A command id is the extension id, then
     your own segment, so it becomes `acme.my_plugin.count`. The host refuses a
     command outside your id's namespace, and `run_command` matches on the same
     constant, so keep them together. The palette title
     (`"Word Count: Count Words"`) and the doc comments are yours to reword.
4. Run `./build.zsh` (needs `rustup target add wasm32-wasip2`).
5. In Tenmoku, turn on `extensions.plugins_preview`, then run
   **Extensions: Install Folder for Development…** and pick the folder. The
   plugin reloads whenever its `.wasm` is rebuilt.

## The manifest

```toml
id = "example.word_count"
name = "Word Count"
version = "0.1.0"
license = "MIT"

[engine]
tenmoku = "1.1"

[contributes.plugin]
wasm = "word_count.wasm"
capabilities = ["buffer.read", "status", "commands"]
# signature = "…"   # reserved; nothing verifies it yet
```

A plugin installs in the user extensions directory only. A workspace's
`.tenmoku/extensions` can never supply one.

## Capabilities and prompts

A plugin has no ambient authority. Each capability it lists is shown to the user
in a prompt, and nothing is granted until they accept:

| Capability | Lets the plugin |
|---|---|
| `buffer.read` | read the text of open buffers |
| `buffer.write` | edit or propose edits to open buffers |
| `commands` | register commands and invoke the commands plugins may call |
| `decorations` | draw decorations and gutter marks in editors |
| `status` | show items in the status bar |
| `fs.read:workspace` | read files in the open workspace |
| `fs.write:workspace` | write files in the open workspace |
| `process.spawn:<program>` | run the named program |
| `net:<host>` | make requests to the named host |

Some combinations carry an extra warning, such as `buffer.read` with `net:`
(your text could leave the machine). `process.spawn:*` and `net:*` are
full-access wildcards: they show as full access and need a second acceptance.

An update that asks for new or wider capabilities stops and asks again; the old
version stays loaded until you answer. You can revoke a grant, or uninstall the
plugin, from the extensions list; uninstalling deletes the plugin's secrets.

## Trust

For the preview, trust is a local, unsigned install plus the grant prompt. The
prompt is the gate. There is no registry, no signing and no auto-update yet.
There is also no Lua or other scripting layer: plugins are WASM components.

## Limits

A call into your plugin is held to these.

- Overrunning the **call deadline** or the **memory** limit interrupts the call
  and unloads the plugin, with a diagnostic.
- The **read limit** is different: a read past it is refused, and the host
  does not read a component larger than it. That refuses the one read (or the
  load), not the running plugin.

- **Call deadline:** 100 ms per call (time spent in a program you spawn, or in
  `net.fetch`, counts against its own 10 s share instead).
- **Memory:** 64 MiB per plugin.
- **Event queue:** 256 events wait for you; past that the host drops them and
  tells you with a `lagged` event.
- **Read limit:** 16 MiB for one buffer read.

## API tour

The SDK is the `tenmoku-plugin` crate. Implement `Plugin` (every method has a
default; override what you use) and export it with `export_plugin!`:

```rust
use tenmoku_plugin::{Error, Plugin, export_plugin, info};

#[derive(Default)]
struct Hello;

impl Plugin for Hello {
    fn init(&mut self) -> Result<(), Error> {
        info!("hello");
        Ok(())
    }
}

export_plugin!(Hello);
```

The interfaces, by module:

- `commands`: register commands and invoke allowed ones; `run_command` is
  called when yours runs.
- `events`: subscribe to editor events, delivered to `handle_events`.
- `buffer`: read, edit and propose changes to buffers.
- `decorations`: inline decorations and gutter entries.
- `status`: status bar items; clicking one runs its command as your plugin.
- `diagnostics`: problems on a buffer, shown as a language server's (see
  [Diagnostics](#diagnostics)).
- `formatting`: a formatter for Format Document and format on save (see
  [Formatting](#formatting)).
- `fs`, `process`, `net`: files, programs and HTTP, each behind its capability.
- `secrets`: per-plugin storage held by the host. It is not a capability and
  needs no grant; only your plugin can read what it stores.
- `log`: `info!` and friends. Read a plugin's log with **Extensions: Show
  Plugin Log**.

### Commands you can invoke

`commands.invoke` runs a command as your plugin, with `commands` granted. Only
the commands below are open to plugins; each needs the capabilities after it as
well, and is refused (`denied`, naming what is missing) without them. Every
other command is refused, and so is a plugin's command, another plugin's or
your own: one plugin's grants can't be used through another's.

- `editor.insert` (`text`), `editor.newline`: `buffer.write`
- `editor.move` (`motion`), `editor.move_line_start`, `editor.move_line_end`,
  `editor.move_document_start`, `editor.move_document_end`, `editor.select_all`,
  `editor.fold`, `editor.unfold`: nothing more
- `editor.save`: `buffer.write` and `fs.write:workspace`
- `workbench.open_path` (`path`, optional `row` and `column`): `fs.read:workspace`
- `nav.back`, `nav.forward`, `diagnostics.next`, `diagnostics.previous`,
  `project_panel.reveal_active`, `problems.show`: nothing more

The editor commands act on the active editor of the active window.

- `editor.save` saves the active buffer to its own file as it is: format on
  save and code actions on save don't run for a plugin's save, so the file
  written is the one checked. It is refused for an untitled buffer, for a file
  outside the workspace's folders (where it leads, symlinks followed, one to a
  file not there yet included), and while a save of the buffer is under way.
- `workbench.open_path` opens a file with the caret at its 0-based `row` and
  `column` (in characters, clamped to the text), or where it was without a
  `row`. `path` must be absolute and inside a workspace folder once symlinks
  and `..` are followed: a path outside, a `..` escape or a symlink out is
  refused.
- `nav.back` and `nav.forward` move through the places the user went;
  `diagnostics.next` and `diagnostics.previous` move the caret to the next or
  previous problem; `project_panel.reveal_active` shows the active file in the
  project tree; `problems.show` opens the Problems panel.

### Diagnostics

A linter reports what it finds through `diagnostics`, under the `decorations`
capability. Publish a named set on a buffer against the version you read; a
set replaces the one of that name, and the editor moves it with later edits
until you publish it again:

```rust
use tenmoku_plugin::{diagnostics, types::{Diagnostic, Range, Severity}};

let items = vec![Diagnostic {
    range: Range { start: 31, end: 32 },
    severity: Severity::Error,
    message: "cannot find `y`".into(),
    source: None,
    code: Some("E0425".into()),
}];
diagnostics::publish(buffer, version, "lint", &items)?;
// …and when the problems are fixed:
diagnostics::clear(buffer, "lint")?;
```

They show as a language server's do, beside any server's for the same buffer:
squiggles, inline diagnostics, **Go to Next Problem** and **Go to Previous
Problem**, and rows in the Problems panel under the file. A diagnostic without
a `source` is labelled with your plugin's name. Your sets are merged: each
set is yours to replace or clear, and clearing one leaves the rest.

- Ranges are byte offsets into the text at `version`. One off a character
  boundary or past the end fails with `invalid-argument`; a version the editor
  can no longer map from fails with `conflict`, so read the buffer and publish
  again against its version now.
- At most 10,000 diagnostics per buffer, over all your sets.
- Diagnostics are for open buffers only. Closing the buffer drops them;
  unloading or reloading your plugin drops all of them, so publish again from
  `init` or your next event.

### Formatting

A formatter registers one of your own commands for languages, under the
`buffer.read` and `buffer.write` capabilities, usually from `init`:

```rust
use tenmoku_plugin::formatting;

formatting::register(&["toml".into()], "acme.toml.format")?;
```

**Format Document**, and format on save when `editor.format_on_save` is on,
then run that command with JSON arguments
`{"request": n, "buffer": id, "version": v, "language": "toml"}`. Read the
buffer, reply once through `formatting::answer(request, &edits)` and return;
returning without answering leaves the text as it is. The edits are as for
`buffer.edit`: byte ranges into the text at `version`, in order, apart and on
character boundaries. Anything else fails the request.

- **Yours comes last.** The language's `[formatters.<language>]` command and
  its language server format first: yours runs only when neither does (there
  is none, its program isn't there, or it fails or times out). Selections are
  never a plugin's to format.
- **The language** is the one the language servers route by: the editor's
  language name, as `rust`, or for a file with none, its extension, as `toml`.
  Registering a language again replaces the formatter for it.
- **Time.** The command runs under your call deadline, and the editor waits no
  longer for it; overrunning it unloads your plugin. A save then writes the
  text unformatted, without telling the user.
- **Typing meanwhile.** If the buffer changes while you're asked, your answer
  is dropped, whatever it is.
- **Failures.** A Format Document the user asked for shows one line naming
  your plugin when your formatter fails; a save only logs it.
- Unloading or reloading your plugin drops your formatters: register them
  again from `init`.

## Installing from an archive

**Extensions: Install from File or Folder…** installs a `.zip`, a `.tar.gz` or
a folder, checked and unpacked into the user extensions directory.
