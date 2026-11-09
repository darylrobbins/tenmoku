---
task_uid: "01m6z4nkghkvq2c19ssr3bee0k"
task: "T1644"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "tests::registry::css::css_scss_and_less_get_the_css_server_by_default: with default settings, css, scss and less documents are routed to the built-in server named vscode-css-language-server whose command is vscode-css-language-server with args [--stdio], languages [css, scss, less], root markers [package.json], initialization_options {\"provideFormatter\": true} and an empty settings object; go still gets gopls and rust still gets rust-analyzer"
    required: true
    manual: false
  - id: "C2"
    text: "tests::registry::css::css_language_ids_pass_through: language_id gives css for a.css, scss for a.scss and less for a.less, and a scss document opened through the routed fake server arrives in didOpen with languageId scss"
    required: true
    manual: false
  - id: "C3"
    text: "tests::registry::css::settings_can_disable_or_override_the_css_server: [lsp.servers.vscode-css-language-server] enabled = false leaves css and scss documents with no server, and a command override replaces the built-in command while keeping args, languages, root markers and initialization_options"
    required: true
    manual: false
  - id: "C4"
    text: "tests::registry::css::css_server_root_is_the_package_json_folder: for a stylesheet outside the window root, the server root is the nearest ancestor holding package.json, and the file's own folder with none"
    required: true
    manual: false
  - id: "C5"
    text: "tests::registry::css::css_settings_sections_come_from_user_settings: with the built-in spec, workspace/configuration for section css answers null; with [lsp.servers.vscode-css-language-server.settings.css] validate = false it answers {\"validate\": false} and section scss still answers null"
    required: true
    manual: false
  - id: "C6"
    text: "tests::registry::builtin_specs lists vscode-css-language-server after gopls; tests::registry::gopls, tests::registry::root_markers and tests::registry::language_ids pass unchanged; fe-config's tests::settings::schema_matches_golden passes with only the LspSettings servers description changed in settings.schema.json; cargo nextest run -p fe-lsp -p fe-config and cargo clippy -p fe-lsp -p fe-config --all-targets -- -D warnings pass"
    required: true
    manual: false
  - id: "C7"
    text: "git diff --stat against the base touches no file under crates/fe-workbench, THIRD_PARTY.md or NOTICE"
    required: true
    manual: false
---

# Make vscode-css-language-server a built-in language server for CSS, SCSS and Less: colour swatches, completion, hover and diagnostics with no config when it's on PATH

The user (2026-11-08) approved making vscode-css-language-server (npm `vscode-langservers-extracted`, MIT, Microsoft) a built-in language server, used whenever `vscode-css-language-server` is on the login shell's PATH, exactly as pyright and gopls are. Today CSS and SCSS files get no server unless the user writes `[lsp.servers.<name>]` by hand. With it, `.css`/`.scss` files get completion, hover, diagnostics, document symbols, folding, selection ranges, rename and colour swatches (documentColor, which crates/fe-workbench/src/lsp_colors.rs already draws, T1113) with no config. Precedent: gopls (T1367, commit 3dce44f79, spec docs/specs/1dbnegnewe-make-gopls-a-built-in-language-server.md).

Not shipped or vendored: fastedit only launches it, like pyright and typescript-language-server, which THIRD_PARTY.md and NOTICE don't list. Don't add it there.

## Do

- Add a ServerSpec to `builtin_specs` (crates/fe-lsp/src/registry.rs), after gopls: name "vscode-css-language-server", command "vscode-css-language-server", args ["--stdio"], languages ["css", "scss", "less"], root_markers ["package.json"], `initialization_options` `{"provideFormatter": true}`, empty `settings` object, default readiness and diagnostics modes. One short comment says what each non-default is for.
  - Languages: fe-syntax's built-in CSS language is "css" (crates/fe-syntax/src/language.rs); SCSS is "scss", from the first-party wasm grammar extensions/grammars/scss. There is no Less or Sass grammar today; "less" is listed so a Less grammar (an extension's `language = "less"`) gets the server with no further change. `language_id` (crates/fe-lsp/src/document.rs) passes names through, so the languageIds are "css", "scss" and "less", which are the ones the server expects. document.rs needs no change.
  - provideFormatter: the server advertises document and range formatting only when `initializationOptions.provideFormatter === true` (VS Code passes it when `css.format.enable`, default on). `editor.format_on_save` is off by default, so nothing reformats unasked.
  - Settings: the server asks `workspace/configuration` for the section named by the document's languageId (`css`, `scss`, `less`). An empty settings object answers `null`, and the server then uses its defaults (validation and lint on). A user tunes it with `[lsp.servers.vscode-css-language-server.settings.css]` (e.g. `lint.unknownProperties = "error"`). Pass no default sections.
  - Capabilities it relies on that fastedit already sends: `completionItem.snippetSupport: true` (crates/fe-lsp/src/server.rs; without it the server advertises no completionProvider), `textDocument.diagnostic` (so the server uses pull diagnostics, which `PullDiagnostics::Advertised` handles), and colorProvider is static. Check these against the server source (microsoft/vscode extensions/css-language-features/server/src/cssServer.ts, via Context7 or the web) and say in the commit message if anything differs.
  - Root markers: `package.json`, so a stylesheet opened outside the window folder gets its package's folder (where node_modules `~` imports resolve), else its own folder.
- Update the lists of built-in servers: the doc comment on `LspSettings::servers` in crates/fe-config/src/settings.rs (only that comment; no new setting) and, regenerated from it, crates/fe-config/settings.schema.json (`UPDATE_GOLDEN=1 cargo nextest run -p fe-config schema_matches_golden`, then review the diff: only that description changes).
- Update `tests::registry::builtin_specs` (crates/fe-lsp/src/tests/registry.rs) for the new entry and default spec order, and declare the new test module there.
- A missing `vscode-css-language-server` behaves like any missing built-in server today (existing not-found handling, no crash); no new UI.
- One server per language is chosen (docs/decisions/lsp-client.md §3.2, `server_for`), so this is CSS's only built-in server. css-variables-language-server, which needs to run beside it, is a separate card that depends on this one.

## Tests

New file crates/fe-lsp/src/tests/registry/css.rs, declared from crates/fe-lsp/src/tests/registry.rs, using that module's helpers (`routed`, `open`, `root`, `server`, `strings`, `uri`) as gopls.rs does. Fake server only; no real server, no network.

Headless; Linux-OK. Don't touch crates/fe-workbench (no workbench test opens a .css or .scss file; checked with rg), lsp_colors.rs, or any colour module: a parallel card adds built-in colour detection there.
