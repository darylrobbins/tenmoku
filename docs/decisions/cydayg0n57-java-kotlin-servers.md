# Java and Kotlin built-in language servers (T313)

## Decision

- **Java**: `jdtls` (Eclipse JDT LS), the Homebrew/Mason wrapper script. It
  speaks LSP on stdio with no args; a `-data <dir>` arg only moves its
  workspace cache.
- **Kotlin**: JetBrains `kotlin-lsp` (`--stdio`) is the default. It is the
  maintained server; fwcd's `kotlin-language-server` is the alternative, kept
  as a manual override rather than a second built-in (one server per language,
  `server_for` takes the first spec): set `[lsp.servers.kotlin-lsp]
  enabled = false` and add `[lsp.servers.kotlin-language-server]` with
  `command = "kotlin-language-server"` and `languages = ["kotlin"]`.
- Groovy and Gradle scripts get no server.

fastedit only launches the server from the login shell's PATH, like gopls;
nothing is shipped, so THIRD_PARTY.md and NOTICE don't change.

## Missing server

A server not on the PATH is handled like any other built-in: the document
simply has no language features and the usual server-missing hint shows. No
install is attempted.

## Root

`server_root` tries the markers in order, and the first marker found in any
ancestor folder wins, nearest folder first. jdtls markers: `pom.xml`,
`build.gradle`, `build.gradle.kts`, `settings.gradle`, `settings.gradle.kts`,
`.git`; kotlin-lsp the same without `pom.xml`. So:

- a Maven module roots at its own `pom.xml`, not the parent pom's;
- a Gradle module with a build file roots there; one without roots at the
  folder with `settings.gradle(.kts)`.

Rooting a multi-module repo at its topmost marker needs an outermost-ancestor
rule the registry doesn't have, and none was added. A file inside the window
root still roots at the window root.
