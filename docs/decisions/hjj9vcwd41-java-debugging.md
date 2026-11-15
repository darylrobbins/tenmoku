# Java debugging through jdtls: design calls (debugger D21b)

Card T660 (Mac T1858), under `docs/decisions/r1b9dw3g0g-debugger.md` §13
("Java joins the MVP"), on D21's fe-lsp half
(`docs/decisions/aet7gz5k46-jdtls-debug-bundles.md`) and D3's `Port`
launch kind (`docs/decisions/bpkhtxssvc-debug-adapter-registry.md`). The
code is `crates/fe-workbench/src/debug_java.rs`.

## The adapter

fe-dap's built-ins gain `java`: `transport = Port(0)` (`JAVA_PORT`, a
placeholder no adapter listens on), `languages = ["java"]`,
`types = ["java"]`, its command `jdtls` for the record only. The registry
treats a Port spec as found, so it is always listed as found and is Java's
default. It answers to `java` by its name, which beats any other adapter's
`type` alias (fe-dap's `registry_host` test, whose host adapter
`java-debug` had `types = ["java"]`, now finds that one by its name; the
respec added the test to the card's scope). At each start the workbench puts the port jdtls answered in place
of the placeholder. A configuration of the `java` adapter always goes
through jdtls's steps below; if the user's settings changed its transport
to stdio or tcp, the spec is used as they made it, with no port put in.

## The jar

java-debug is `com.microsoft.java.debug.plugin-<version>.jar` (EPL-1.0).
Tenmoku never ships or downloads it. It is looked for in this order, the
first found winning:

1. Each entry of `[debug.adapters.java] path`, in order. The record leaves
   `path` as "folders a bare command is looked in", and java has no
   command to look up, so here an entry is the jar itself or a folder
   holding one. `~/` is the home. An entry that gives no jar is passed over
   with a warning in the log; the next entry, then the well-known installs,
   are still tried.
2. The well-known installs (`debug_java::well_known`), in order:
   - `~/.vscode/extensions/vscjava.vscode-java-debug-*/server/` (VS Code's
     Debugger for Java),
   - `~/.vscode-insiders/extensions/vscjava.vscode-java-debug-*/server/`,
   - `~/.vscode-oss/extensions/vscjava.vscode-java-debug-*/server/`
     (VSCodium),
   - `$XDG_DATA_HOME/nvim/mason/packages/java-debug-adapter/extension/server/`
     (Mason's `java-debug-adapter`; `$XDG_DATA_HOME` defaults to
     `~/.local/share`, on macOS too, as Neovim has it).

   Within an extensions folder, the newest extension version is tried
   first; within a folder, the newest jar wins. Versions compare part by
   part, numbers as numbers (`0.10.0` after `0.9.1`).
3. Else "not found", whose hint (`workbench-debug-java-no-jar`) names the
   VS Code extension, Mason's package and `debug.adapters.java.path`.

**When.** Each window's debugger looks, off the main thread, as it is made
and at every `reload_settings` (the path may have changed); only the latest
look counts (a generation number). What it finds goes to the window's
`LspManager::set_jdtls_bundles` at once and to every manager made after
(`start_lsp_manager`, before any server starts there). Not found is an
empty list, so a jar removed from the settings leaves jdtls's next start
without it. A rooted window's manager is made after its root resolves off
the main thread, and jdtls only after a Java file opens and the folder's
trusted settings are read, so the look has in practice finished by then;
if it hasn't, jdtls starts without it and a start explains (below).

The well-known places come from `$HOME` and `$XDG_DATA_HOME`
(`JavaDebugHome`, a global the tests set).

## Detection

Reading the configurations (the picker, a rerun) asks the root's jdtls for
`vscode.java.resolveMainClass` with no arguments (jdtls's whole workspace,
which is the root's), for at most 10 s (`MAIN_CLASS_WAIT`): a project still
importing may be slow, and the picker shows "Reading…" meanwhile. Only in a
trusted folder, and only of a jdtls already running there: reading never
starts one. No answer in time, no jdtls or an error lists no Java
configurations.

Each main class is "Debug <class>" (`workbench-debug-launch-detected-java`):
the simple name, or the full one where two classes share the simple name;
where two projects have the same class, each also names its project
(`…-detected-java-in`, "Debug com.example.Server (tools)"). They come after
Cargo's and before Python's current file, under the `java` adapter, with
`mainClass` and `projectName` as adapter arguments, no program, the root as
`cwd`, and no pre-launch task: jdtls builds the classes itself.

## launch.json and the launch file

`type: "java"` names the built-in through the alias table. A java
configuration's keys besides the typed fields pass through, as for any
adapter: `mainClass`, `projectName`, `classPaths`, `modulePaths`,
`vmArgs`, `encoding` and so on. vscode-java-debug lets `args` be one
string; for a `java` configuration a string `args` is passed through as
written instead of being refused for not being an array. In the launch
file the java keys go under `[launch.adapter_args]`.

## Starting

After the pre-launch task, if any (so a Gradle or Maven build, T1660's
detected tasks among them, runs first), and on the start's own numbering,
so Stop, untrusting or a newer start drop it, off the main thread:

1. With no `mainClass`: `vscode.java.resolveMainClass`; exactly one main
   class is used, its project too; none or several refuse the start,
   naming them and `mainClass`.
2. Unless the configuration gives `classPaths` or `modulePaths`:
   `vscode.java.resolveClasspath` with `[mainClass, projectName or null]`,
   whose answer is `[modulePaths, classPaths]`, as vscode-java-debug reads
   it.
3. `vscode.java.startDebugSession`: java-debug's port, a number 1-65535.

`mainClass` and `projectName` are the configuration's expanded strings.
The launch arguments are built as for any adapter (D7), then jdtls's
(`mainClass`, `projectName`, `classPaths`, `modulePaths`), then the
configuration's adapter arguments over both. `args`, and `vmArgs` given as
a list, go as java-debug reads them, one string each, joined as
vscode-java-debug's `concatArgs` does: by spaces, an argument holding a
quote or white space in double quotes with quotes and backslashes escaped.
`console` maps to java-debug's `integratedTerminal` and `internalConsole`.

None of the three has a timeout: `resolveClasspath` can wait on a build,
and Stop gives the start up.

Every command goes to the jdtls running for the window's root, by
`execute_command_at("jdtls", root, …)`.

## Why it can't start

Each is the detail of the failed start's toast ("The debug session <name>
couldn't start"), and no session starts:

- **No jdtls for the root** (no manager for the root, or `ServerDown`):
  open one of its Java files and let jdtls start.
- **jdtls without java-debug's command** (`Unavailable`, fe-lsp's static
  check against `executeCommandProvider.commands`), explained by what is
  known, first match: the jar wasn't found (the hint); the user's
  `initialization_options` refuse the bundles (`BundlesRefused`, naming
  which); jdtls started before the jar was found (the root is in
  `JdtlsBundles::stale`: restart it with `lsp.restart` in one of its Java
  files); else jdtls didn't load it from the jar named. The workbench asks
  first and explains only on `Unavailable`, rather than refusing up front,
  so a user who lists the jar in their own `initialization_options` still
  debugs.
- **jdtls's own errors**, per step, with its message.
- **An answer that isn't what was asked for** (no port, a malformed
  classpath).

jdtls is never restarted on the user's behalf: a restart re-imports the
project, which can take minutes.

## Tests

`src/tests/debug_java.rs`: jdtls is fe-lsp's fake server, launched by
opening a Java file in a trusted Maven or Gradle root. java-debug is fe-dap's
fake adapter (`fe_dap::fake::serve`) listening on a real port on
127.0.0.1, the session connecting there through `LocalLauncher`'s Port
kind; a unit test can't reach the `fe-dap-fake` binary itself
(`CARGO_BIN_EXE_*` names only a crate's own binaries), so the test serves
the same fake in process behind a socket.

## Carried findings (T658)

- **F19** and the read-failure half of **F17**: a stop's file held by a
  Save As is opened only after the Save As lets go, by the stop's own
  navigation (dropped with the stop, so an obsolete stop opens nothing),
  for a file already in a tab too; no read result is injected over the
  reservation; and a read that fails opens nothing and starts no read
  outside the stop's guard. Regression:
  `tests::debug_java::a_stop_in_a_file_a_save_as_holds_waits_for_it`.
- **F16** (a notification after the stop epoch advances without a later
  event) needs fe-dap's session actor to tell of the retirement; fe-dap's
  session isn't in this card's scope, so it stays open.

## Known limits

- Only a jdtls rooted at the window's root is asked. A multi-module repo
  whose Java files root jdtls at a module's build file (fe-lsp's nearest
  marker) has no jdtls "for the root"; fe-lsp has no lookup by name of the
  instances under a folder.
- The server must be named `jdtls`; a user's own Java server under another
  name isn't asked.
- Java test debugging, hot code replace and attaching to a JVM are other
  cards' (§13).
