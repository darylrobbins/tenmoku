# jdtls bundles and a host's workspace/executeCommand (T644, debugger D21)

Part of `docs/decisions/r1b9dw3g0g-debugger.md` §13 (Java joins the MVP):
java-debug runs inside jdtls as a bundle, and the debugger asks jdtls over
LSP for the adapter's port. This card is the fe-lsp half; D21b finds the
jar and launches sessions.

## Bundles: `LspManager::set_jdtls_bundles(Vec<PathBuf>) -> JdtlsBundles`

- **Merged, never replacing.** The host's paths are appended to
  `initializationOptions.bundles` after the ones the user's
  `[lsp.servers.jdtls] initialization_options` list, skipping any already
  listed (the user's position wins). No options, or null ones, become
  `{ "bundles": [...] }`; the user's other keys are kept. Options that
  aren't a table, or a `bundles` that isn't a list, are left as written,
  with a warning in the log: merging would mean replacing them. The host
  hears why (`JdtlsBundles::refused`: `BundlesRefused::OptionsNotATable`
  or `BundlesNotAList`, an enum so the host words it in its catalogue).
- **Which jdtls.** The one `open` routes documents to, whatever tier its
  spec came from (built-in, settings, or a host spec named `jdtls`). A spec
  the host passes to `start` launches exactly as given: the host named it.
- **When they apply: the next jdtls start at a root.** jdtls loads bundles
  only as it initializes, and a restart re-imports the project, which can
  take minutes on a large Gradle build. So new bundles do *not* restart a
  running jdtls. The merge happens as an instance launches, outside the
  specs that `update_settings` compares, so a bundles change never counts
  as a spec change. They reach: the first document at a new root,
  `restart("jdtls", root)`, and a restart for any other change to jdtls's
  spec. A crash restart relaunches the instance as it was. One that gave
  up after too many crashes stays stopped, as any server does, through
  another document, unchanged settings or new bundles: its crash budget
  is compared with the spec it launched with, bundles included, and the
  manager keeps the bundles that reached the last launch at each
  (name, root) for that comparison. Only `restart` (or a real spec change)
  brings it back, with the bundles as they are then.
- **What the host is told.** The call returns a `JdtlsBundles`, which
  `LspManager::jdtls_bundles()` also gives at any time (after a settings
  change, say). It has `stale`, the roots of the routed jdtls instances
  running without exactly these bundles, oldest first, and `refused`, why
  the current options can't take them at all. An instance counts as having
  the bundles only if they were actually merged into its launch: one
  launched under refused options is stale. D21b uses `stale` to say
  debugging needs jdtls restarted, or to restart it, before asking for a
  debug session at that root. It uses `refused` to say the user's
  `initialization_options` need fixing, since a restart won't help then.
  Setting the same bundles again changes nothing and reports the same.
- Paths that aren't UTF-8 are skipped with a warning: bundles travel as
  JSON strings. Repeats are dropped.

## `LspManager::execute_command_at(name, root, command, arguments)`

- **Addressed by (server name, root)**, the newest instance running there,
  routed or host-started: the debugger knows the project root and the
  server, not a `ServerId`. None running there answers `ServerDown`.
- **Only the server's own commands.** A command missing from
  `executeCommandProvider.commands` is refused with `Unavailable`
  ("<command>: not a command of the server", the existing
  `lsp-not-a-server-command` message), with no request sent. If the server
  has initialized, the refusal is immediate; while it starts, the request
  waits as other requests do and is checked on the server's thread, just
  before it would be sent, against that session's capabilities.
- fe-lsp declares no `workspace.executeCommand.dynamicRegistration`, so
  jdtls lists every command, its bundles' included, statically in its
  `initialize` reply (it registers them dynamically only for clients that
  ask). The static list is therefore the whole truth.
- **The reply** is the result as raw JSON (`null` stays `null`:
  lsp-types' own `ExecuteCommand` reads it as `None`). A server error comes
  back as `LspError::Server`. `workspace/executeCommand` has no
  `lsp.timeouts` entry, and `vscode.java.resolveClasspath` can wait on a
  build, so there's no timeout: dropping the future cancels the request.
- The crate-private `execute_command` that code actions, lenses and
  completion items use is unchanged: it is gated on the document's opening,
  which a host command has no use for.

## Fake server

`Script::commands` answers `workspace/executeCommand` by the command it
names, ahead of `replies` (`FakeServer::set_command_reply` changes one).
