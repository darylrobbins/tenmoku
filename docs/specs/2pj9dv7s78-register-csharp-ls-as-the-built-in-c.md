---
task_uid: "01m79cxekrg7n7vs2pj9dv7s78"
task: "T598"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "`tests::registry::csharp::csharp_gets_csharp_ls_by_default` and `tests::registry::csharp::csharp_language_id_passes_through_and_the_fake_server_gets_didopen` pass"
    required: true
    manual: false
  - id: "C2"
    text: "`tests::registry::csharp::settings_can_disable_or_override_csharp_ls` and `tests::registry::csharp::server_root_matches_extension_globs` pass"
    required: true
    manual: false
  - id: "C3"
    text: "README.md, the [lsp.servers] doc text (settings.rs, settings.schema.json, settings.ftl) and BUILT_IN_SERVERS name csharp-ls; the decision file records the install command, the .NET 10 SDK requirement, the glob marker rule and the Razor limit"
    required: true
    manual: false
---

# Register csharp-ls as the built-in C# language server, rooted at the folder with the solution (Mac T1770)

# Register csharp-ls as the built-in C# language server, rooted at the folder with the solution

Add `csharp-ls` (razzmatazz/csharp-language-server, MIT, `dotnet tool install --global csharp-ls`) to `builtin_specs` in
crates/fe-lsp/src/registry.rs, the way jdtls and kotlin-lsp are (docs/decisions/cydayg0n57-java-kotlin-servers.md). Nothing is
shipped or installed by fastedit, so THIRD_PARTY.md and NOTICE do not change.

## Server facts (checked 2026-11-12 against the csharp-ls README and source)

- Binary `csharp-ls`, LSP on stdio with no args. Needs the .NET 10 SDK or later on the machine (it analyses older target
  frameworks fine). Install: `dotnet tool install --global csharp-ls` (binary lands in `~/.dotnet/tools`; the launcher resolves bare
  names through the login shell PATH, so a user with that dir on PATH is covered; say so in the docs line).
- Solution discovery (src/CSharpLanguageServer/Roslyn/Solution.fs): it does NOT want a `.sln` as rootUri; it walks the workspace
  folder (`rootUri`) recursively for `*.sln`/`*.slnx`, picks a preferred one, else falls back to every `*.csproj`/`*.fsproj` below it.
  `--solution <file>` (relative to CWD) or setting `csharp.solutionPathOverride` overrides it. So the right root is the folder at or
  above the solution/projects.
- Settings arrive via `workspace/configuration` section `csharp` or `initializationOptions` of the same shape (`{"csharp": {...}}`);
  we send none, defaults apply (razor support is off by default, `csharp.razorSupport`).
- Language id: the grammar's language is `csharp` (extensions/grammars/csharp/extension.toml) and `csharp` is the LSP id, so
  `document::language_id` passes it through (no table change).
- Known limit, to go in the docs and the decision file: Razor/`.cshtml`/`.razor` get no server (there is no `razor` language here
  and csharp-ls's razor support is opt-in and experimental).

## Root markers need a glob

`server_root` (registry.rs ~line 326) tests each marker with a literal `dir.join(marker).exists()`, so `*.sln` does not work today.
Add the smallest glob support: a marker that starts with `*.` matches any entry in the directory with that extension (a `read_dir`
check, case-sensitive, no other glob syntax, a literal marker unchanged). Then csharp-ls gets
`root_markers = ["*.sln", "*.slnx", "*.csproj", ".git"]` (first marker with a match in any ancestor, nearest first, as documented for
jdtls: a solution beats a project, a project beats `.git`). A file inside the window root still roots at the window root, which is the
common case; markers matter for files outside it. Document the `*.ext` form for the `[lsp.servers.<name>].root_markers` setting
where that setting is described (fe-config settings.rs doc comment + settings.schema.json + settings.ftl, generated text: keep the
three in step, as for the server list below).

## Not-installed behaviour

No install hint mechanism exists per server: a missing binary gives the status detail `lsp-server-not-found` ("csharp-ls not
found", crates/fe-lsp/l10n/en-US/fe-lsp.ftl:53, produced by `launch_error` in server.rs:735), same as jdtls/gopls. Do not add a new
string; the install command goes in the docs. (T1741 adds a toast naming the npm package for add-ons only; out of scope here.)

## Changes

1. registry.rs: the `csharp-ls` spec (command `csharp-ls`, no args, languages `["csharp"]`, markers above) after kotlin-lsp; the glob
   rule in `server_root`.
2. Server-name lists that must stay in step: `BUILT_IN_SERVERS` in crates/fe-workbench/src/plugin_prompt.rs (array length 8 -> 9; keep
   plugin_prompt_tests passing), the `[lsp.servers]` doc text in crates/fe-config/src/settings.rs (~1370), crates/fe-config/settings.schema.json
   (~673) and crates/fe-config/l10n/en-US/settings.ftl (~330) (all three name the built-ins; add `csharp-ls`).
3. README.md "Language servers" bullet (line ~72): add csharp-ls and the install command; add the Razor limit.
4. docs/decisions/<new id>-csharp-server.md (a decision file, scope-exempt per the lane's convention): the decision, the facts above,
   why csharp-ls over Microsoft's Roslyn language server (the latter is not a standalone product: NuGet
   `Microsoft.CodeAnalysis.LanguageServer` is MIT but meant for an editor extension; the `roslyn-language-server` dotnet tool is a
   prerelease wrapper), the manual override recipe for it, the glob marker rule, the Razor limit.

## Tests (new files only)

crates/fe-lsp/src/tests/registry/csharp.rs, declared `mod csharp;` in crates/fe-lsp/src/tests/registry.rs (pattern: tests/registry/java.rs):

- `tests::registry::csharp::csharp_gets_csharp_ls_by_default`: `server_for(&specs, "csharp")` is `csharp-ls`, command `csharp-ls`, empty
  args, languages `["csharp"]`, markers `*.sln`, `*.slnx`, `*.csproj`, `.git`; no other built-in claims `csharp`.
- `tests::registry::csharp::csharp_language_id_passes_through_and_the_fake_server_gets_didopen`: `language_id("csharp", a.cs) == "csharp"`
  and a routed fake server sees `languageId` `csharp` (as `java_and_kotlin_language_ids_pass_through`).
- `tests::registry::csharp::settings_can_disable_or_override_csharp_ls`: `enabled = false` leaves `csharp` with no server; a `command`
  override replaces the command (as the java test).
- `tests::registry::csharp::server_root_matches_extension_globs`: in a temp dir, `src/App/App.csproj` with `src/Repo.sln` above it
  roots at `src` for `src/App/Program.cs` (solution beats nearer project); with no `.sln` it roots at `src/App`; a `.git`-only
  tree roots there; a literal marker still matches literally (`server_root` with `["Cargo.toml"]`); a directory named `x.sln` still
  counts (plain existence), a file `sln` or `a.slnx.bak` does not match `*.sln`. Window-root-wins is unchanged.

No real-server test (csharp-ls and the .NET SDK are not on CI); no new ignored test either.

## Notes for the lead

Linux-OK (label linux-ok): pure registry/path logic and strings, no GPUI, no OS cfg. Overlap: touches registry.rs and
tests/registry.rs and tests/registry/**, which T1742 (scope includes both) also edits; make this depend on T1742 or run after it so
the `mod` line and the spec list do not conflict. Also overlaps T1741/T1735 only via registry.rs; check at lane-up.

Do not widen --scope; if a file outside it is needed, stop and say so.

## Implementation notes

- The glob rule lives in `holds_marker` (registry.rs): `*.ext` matches an entry named `<stem>.ext` with a non-empty stem,
  compared on the name's encoded bytes, so `.sln` doesn't match `*.sln` either. `read_dir` failures count as no match.
- README now also names jdtls and kotlin-lsp, which were missing from the server list.
- The decision file is docs/decisions/2pj9dv7s78-csharp-server.md.
