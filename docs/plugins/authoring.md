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
- `diagnostics`, `formatting`: in the WIT, not yet wired in the app.
- `fs`, `process`, `net`: files, programs and HTTP, each behind its capability.
- `secrets`: per-plugin storage held by the host. It is not a capability and
  needs no grant; only your plugin can read what it stores.
- `log`: `info!` and friends. Read a plugin's log with **Extensions: Show
  Plugin Log**.

## Installing from an archive

**Extensions: Install from File or Folder…** installs a `.zip`, a `.tar.gz` or
a folder, checked and unpacked into the user extensions directory.
