# C# built-in language server (T598)

## Decision

- **C#**: `csharp-ls` (razzmatazz/csharp-language-server, MIT) is the
  built-in server for the `csharp` language. It speaks LSP on stdio with no
  args. Install it with `dotnet tool install --global csharp-ls`; it needs the
  **.NET 10 SDK or later** on the machine (it analyses projects that target
  older frameworks fine). The binary lands in `~/.dotnet/tools`, and bare
  commands are resolved through the login shell's `PATH`, so that folder must
  be on it.
- `csharp` is both Tenmoku's language name (the C# grammar,
  extensions/grammars/csharp) and the LSP language id, so `languageId`
  passes through unchanged. A `.cs` file gets the server only once the C#
  grammar is installed, since the grammar is what names its language.
- No settings are sent (no `initializationOptions`, empty
  `workspace/configuration` sections), so csharp-ls's defaults apply; they
  can be set under `[lsp.servers.csharp-ls.settings.csharp]`.

fastedit only launches the server from the login shell's PATH, like gopls
and jdtls; nothing is shipped, so THIRD_PARTY.md and NOTICE don't change.

## Why not Microsoft's Roslyn language server

It isn't a standalone product. The NuGet package
`Microsoft.CodeAnalysis.LanguageServer` (MIT) is built to be driven by an
editor extension: it learns which solution to load from a client-specific
`solution/open` (or `project/open`) notification after `initialize`, which
fastedit doesn't send. The `roslyn-language-server` dotnet tool that wraps it
is prerelease only. csharp-ls finds the solution itself from its root.

Manual override, to try it anyway (falls back to its own project loading,
which is slower and less reliable than `solution/open`):

```toml
[lsp.servers.csharp-ls]
enabled = false

[lsp.servers.roslyn-language-server]
# dotnet tool install --global roslyn-language-server --prerelease
command = "roslyn-language-server"
args = ["--stdio", "--autoLoadProjects"]
languages = ["csharp"]
root_markers = ["*.sln", "*.slnx", "*.csproj", ".git"]
```

## Missing server

A missing binary is handled like any other built-in: the status detail is
`lsp-server-not-found` ("csharp-ls not found") and the document has no
language features. No install is attempted and there is no per-server
install hint; the install command is in the README and here.

## Root

csharp-ls doesn't want a `.sln` as its `rootUri`: it walks the workspace
folder recursively for `*.sln`/`*.slnx`, picks a preferred one, and otherwise
loads every `*.csproj`/`*.fsproj` below it (`--solution <file>` or the
`csharp.solutionPathOverride` setting override that). So the right root is
the folder at or above the solution.

Markers: `*.sln`, `*.slnx`, `*.csproj`, `.git`. `server_root` tries them in
order and the first marker with a match in any ancestor wins, nearest folder
first (as for jdtls, docs/decisions/cydayg0n57-java-kotlin-servers.md): a
solution above beats a nearer project, a project beats `.git`. A file inside
the window root still roots at the window root, the common case; the
markers matter for files outside it.

### Glob markers

Root markers used to be literal names only (`dir.join(marker).exists()`).
The smallest glob form was added, for every server and for the
`[lsp.servers.<name>].root_markers` setting: a marker that starts with `*.`
matches any entry in the folder whose name ends in `.<ext>` after a
non-empty stem. It is case-sensitive, files and folders alike count (as a
literal marker only checks existence), and there is no other glob syntax:
`*.sln` matches `Repo.sln` and a folder `x.sln`, not `sln`, `.sln`,
`A.SLN` or `a.slnx.bak`. A marker without the `*.` prefix is unchanged.

## Known limit: Razor

Razor files (`.cshtml`, `.razor`) get no server: there is no `razor`
language here, and csharp-ls's Razor support is opt-in
(`csharp.razorSupport`) and experimental.
