# Per-language styling: token model, selector cascade, roles, the `semantic` contribution point

**Status: proposed.** Waiting for the user's sign-off (§9). Task T199 (Mac T993).
Builds on `vmtnmkay62-extensions-and-plugins.md` (T839) §2 and §4; neither that
file nor any code is edited here.

## Context

The user approved (2026-10-18, "fastedit: VS Code Parity Notes", section
"Generic per-language styling") making per-language styling generic: fastedit
knows token types, modifiers and rules that style them, not Rust. A language
extension contributes the meaning (rust-analyzer's `mutable`, drawn underlined).
Three open cards would otherwise settle half of this by accident. At the base
(`a9db90d`):

- **Colour only, by dotted name.** `crates/fe-syntax/src/highlight.rs`:
  `Highlight(u8)` indexes the vocabulary `NAMES` (35 names, `function.method`,
  `markup.heading`, `variable.parameter`, …); `ALIASES` renames other grammars'
  captures; `Highlight::from_capture` strips trailing segments until a name is
  known; `resolve_theme` gives each name the style of its longest listed prefix.
  `crates/fe-config/src/theme.rs`: `EXTENSIONS = ["editor", "syntax"]` (:32) are
  the tables fastedit asks kiln to keep in a theme file;
  `EditorPalette::syntax_color(name)` (:195) finds a colour by exact name in
  `EditorPalette::syntax: Vec<(String, Color)>`; `syntax_colors` (:225) reads the
  `[syntax]` table. `fe-gpui`'s `styled_highlights` (`element.rs:2352`) maps
  `Syntax::highlights` (`fe-syntax/src/syntax.rs:384`) through
  `EditorTheme::syntax_color(Highlight)` (`fe-gpui/src/lib.rs:173`) to
  `StyledSpan<Hsla>`. No bold, italic, underline, strike or background per
  token; no modifiers; no language in a selector.
- **Semantic tokens are ticketed with the mapping baked in.** On argon's board:
  T174 (Mac T945; fe-lsp decodes `SemanticToken { range, token_type, modifiers }`
  as legend names), T173 (Mac T944; `DecorationKind::Foreground(Arc<str>)`, a
  highlight name resolved by `Highlight::from_capture` and `syntax_color`), T175
  (Mac T946; `fe-workbench/src/lsp_semantic_tokens.rs` `highlight_name`: a fixed
  table, modifiers only choosing `.builtin`/`constant`). None is built.
  `DecorationKind` today (`fe-editor/src/decorations.rs:53`) is `RowBackground`,
  `Gutter`, `Highlight` (background), `Underline { wavy }`, `AfterLine`; hosts
  add them with `EditorModel::add_decoration_provider` (`fe-editor/src/lib.rs:388`).
- **Extensions** (T839): kiln loads manifests with `contributes.<kind>` tables
  and an app registers its own kinds with a schema,
  `kiln::Config::extension_kind(name, schema)` (`kiln/crates/kiln/src/setup.rs:382`,
  usage in the module doc of `kiln/crates/kiln/src/extensions.rs:12`), checked by
  `Layer::check_schema` (`kiln-core/src/layers.rs:232`) and kept as written in
  `Manifest::app`; the app reads them in `Config::on_extensions`. Theme files carry
  the app's tables (`ThemeData::extensions`, `ThemeData::extension(name)`,
  `kiln-core/src/theme/mod.rs:49,122`). Layering: T839 §4 (themes: bundled base,
  selected theme, user `theme.toml`).
- **Other styling already ticketed:** diagnostic tags `Unnecessary`/`Deprecated`
  (decoded already, `fe-lsp/src/diagnostics.rs:82`; the styling card is Mac T917
  and is **not on argon's board at the base**), inlay-hint chips (T169, Mac
  T932), bracket-pair colours (T195, Mac T988), indent guides (landed:
  `indent_guide`, `indent_guide_active` in `theme.rs:103`).
- **Dark Modern:** T200 (Mac T994) ships a theme on the colour-only tables.

Clean room: written from fastedit's and kiln's code and docs and VS Code's
public documentation of semantic highlighting (the LSP 3.17 token legend and
the `editor.semanticTokenColorCustomizations` selector form). No Zed or
rust-analyzer source was read.

## 1. Token model

**Decision.** A styled span carries a **token**: a `type` plus an ordered set
of `modifiers`. **One identifier policy.** Names are case-sensitive, never
folded. A core name is spelled exactly as LSP 3.17 spells it (camelCase,
`typeParameter`, `defaultLibrary`), so a legend entry round-trips with no
translation. A contributed name (§4) matches `[a-z][A-Za-z0-9]*`: no dot, no
underscore, and it may not equal a core type, core modifier or fastedit-tier
type. Only fastedit's own second-tier core types are dotted (`markup.link.url`),
each segment `[a-z][a-z0-9]*`. The legacy `[syntax]` keys keep today's
`[a-z0-9_]` dotted form and are a separate namespace (below). The core
vocabulary:

- types: `namespace type class enum interface struct typeParameter parameter
  variable property enumMember event function method macro keyword modifier
  comment string number regexp operator decorator`, plus (LSP 3.17) `label`;
- modifiers: `declaration definition readonly static deprecated abstract async
  modification documentation defaultLibrary`.

Selectors, theme tables and manifests use these spellings exactly; a wrong-case
name is an unknown name (a warning), not a match. An extension adds types and modifiers (§4); a
type may name a **supertype** (Rust's `lifetime` → `typeParameter`; a type
with none falls back only to `*`). Supertypes chain and may not cycle (a cycle
is a manifest warning and drops the later type).

**fe-syntax emits the same names.** Highlights become a `(type, modifiers)` pair
too, so a language with no server is styled by the same rules. The existing
vocabulary maps by a table (the **bridge**, used for semantic tokens and for typing spans; no rename of any
theme key):

| `Highlight` name | token |
|---|---|
| `function`, `function.method`, `function.macro` | `function`, `method`, `macro` |
| `function.builtin`, `type.builtin` | `function.defaultLibrary`, `type.defaultLibrary` |
| `constant`, `constant.builtin` | `variable.readonly`, `variable.readonly.defaultLibrary` |
| `variable`, `variable.builtin`, `variable.parameter` | `variable`, `variable.defaultLibrary`, `parameter` |
| `type`, `module`, `property`, `attribute`, `constructor` | `type`, `namespace`, `property`, `decorator`, `function` |
| `comment`, `comment.documentation` | `comment`, `comment.documentation` (a `comment` with the `documentation` modifier) |
| `string`, `number`, `operator`, `keyword` | themselves |
| `string.special`, `string.special.key`, `escape` | fastedit types (below), supertype `string` |
| `markup.*`, `punctuation.*`, `tag`, `label`, `embedded` | fastedit types (below) |

`constructor` → `function` and `constant` → `variable.readonly` are the lossy
rows: a tree-sitter `@constructor` or `@constant` is the nearest LSP meaning,
not an exact one.

fastedit adds a **second tier of core types** for what LSP has no word for and
tree-sitter does: `punctuation` (`.bracket`, `.delimiter`, `.special` as
sub-types), `markup` (`.heading`, `.link`, `.link.url`, `.raw`), `tag`,
`embedded`, `escape`. **A type name may be dotted** (`markup.link.url`), and a
dotted type's supertype is its prefix: so the supertype rule *is* today's
longest-prefix rule. LSP types are single-segment, so nothing collides.

**Bridge, migrate, or both?** Keep both with **one resolver**, and the legacy
names stay a first-class input, not a lossy translation. Today `constant` and
`constant.builtin`, or `function.method` and `function.macro`, can have
different colours, and a theme styling only `function` styles all its
descendants. The table above is therefore *not* applied to `[syntax]` keys.
Instead each tree-sitter span keeps its `Highlight` (the dense `u8` index and
name) **and** gains its token; a span's resolution has two sources:

1. `[syntax]` entries match by the span's `Highlight` name with today's
   longest-prefix rule (`resolve_theme`), exactly as now. They are *legacy
   rules*: they set `fg` only and sit below every selector rule **of the same
   layer** in specificity, so a theme's `[syntax]` colours behave as before and
   a `[semantic]` rule in the same file refines them.
2. `[semantic]` selector rules match the span's token.

A semantic token from a server has no `Highlight`; for the legacy rules it
takes one through the table's reverse direction (`method` → `function.method`,
`type` + `defaultLibrary` → `type.builtin`, …), and a type with no row gets no
legacy colour (tree-sitter's stays). Both directions are one table in
`fe-style`. Nothing migrates; `[syntax]` is never deprecated by this record.

**Storage.** `HighlightSpan { range, highlight }` (`highlight.rs`) is hot and
mostly modifier-free, so the span keeps `Highlight(u8)` and gains
`modifiers: ModSet`, a `u32` bitset (empty by default, so a modifier-free
frame costs the same). Bit indices are per language: the first bits are the
core modifiers, the rest are that language's contributed modifiers in manifest
order, held in a `LanguageTokens` registry that `fe-syntax` receives from
the app (empty for languages with no extension). A contributed *type* in a
capture (`@lifetime`) is looked up in the same registry: the span's
`Highlight` is its nearest known supertype's legacy name and a `TokenType` id
rides beside it. `from_capture` therefore reads a capture `type.mod1.mod2` as:
the longest known type prefix, the remaining segments as modifiers, names
unknown to the registry dropped. More than 32 modifiers in a language is a
manifest error.

**Crate.** A new **`fe-style`** crate holds the model: `TokenType`, `Modifier`
(interned names), `Token`, `Selector`, `Rule`, `Style`, `Role`, the resolver and
the cascade. It depends on `serde` only: no GPUI, no tree-sitter, no LSP types.
`fe-config` (GPUI-free, and cannot depend on fe-syntax or fe-lsp) depends on it
to parse `[semantic]`/`[roles]`; `fe-syntax` depends on it for `ModSet`, the registry and the
`Highlight`↔`Token` table; `fe-lsp` depends on it to name decoded tokens; `fe-gpui` consumes the
resolved `Style`. No cycle: fe-style is a leaf. Note the doc's "alongside
fe-syntax and fe-lsp" is rejected below.

**Rejected.** (a) *Put the model in fe-syntax* and have fe-lsp and fe-config
depend on it: fe-config would then link tree-sitter, and a GPUI-free settings
crate stops being light (fe-config's `Cargo.toml` today has no fe-* crates).
(b) *Translate `[syntax]` keys into selectors at once*: it merges distinct
legacy styles and loses prefix fallback (`constant` vs `constant.builtin`),
breaking bundled and user themes for nothing. (c) *Keep only dotted names and encode modifiers as extra
segments* (`variable.mutable`): a modifier set is unordered and combinable
(`variable.mutable.static`), which a prefix chain can't say.

## 2. Selector grammar and cascade

**Grammar.**

```
selector  = type { "." modifier } [ ":" language ]     ; or "*" for any type
type      = "*" | word { "." word }                     ; dotted only for fastedit-tier core types
word      = [a-z][A-Za-z0-9]*                         ; case-sensitive, as §1
language  = word                                         ; fastedit language name
```

A modifier is a single word. Because a dotted type and modifiers share dots, a
selector is parsed **against the known type table**: the longest prefix that is
a known type (core or contributed for that language) is the type, the rest are
modifiers. `markup.link.url` is a type; `variable.mutable` is a type and a
modifier; an unparsable remainder is a warning naming the key and dropping the
rule. `*.mutable` applies to every type; `*:rust` every token in Rust.

**Rule.** A rule is a selector with attributes: `fg` and `bg` (colours), `bold`, `italic`, `strike` (booleans), `underline` (`false`,
`"straight"`, `"dotted"`, `"wavy"`; `true` is `"straight"`). A bare string value
is `fg` shorthand, so `[syntax]`'s `"function" = "#61afef"` is already a rule.

**Matching.** A rule matches a token when the token's type equals the rule's
type or has it as an ancestor (supertype chain, or `*`), every modifier in the
selector is among the token's modifiers, and the language is absent or equals the
buffer's language.

**Specificity** compares, in order: (1) language present beats absent; (2)
more modifiers; (3) type distance: exact type beats a nearer supertype beats a
farther one beats `*`. A tuple compare, no summing.

**Each attribute resolves independently**: for every attribute, the style takes
it from the most specific matching rule that *sets* it. A rule that sets only
`underline` doesn't clear `fg`. Ties in specificity go to the later rule in
source order within a layer; between layers the layer decides first (below).

**Layers.** A rule's layer is a **separate, higher-priority key**, not part of
specificity: fastedit defaults < extension < theme < user. Within one attribute,
the highest layer that has *any matching rule that sets it* wins, and specificity
only orders rules within that layer. So a user's `* = underline` beats a theme's
more specific `variable.mutable:rust` underline, and a user's bare
`variable = "#fff"` beats a theme's `variable.mutable:rust = "#aaa"`. The user
who wants only to tweak a case writes the specific selector. **Order of
layers** differs from T839 §4's Themes row only in naming: "theme" is the
selected theme layered over the bundled base; "extension" (§4) sits below theme
so a theme can recolour what a language extension defaults; both over fastedit's
defaults.

**Rejected.** (a) *CSS-style summed specificity or `!important`*: invites
selector hacks and is slow to explain; a tuple compare is predictable. (b)
*Whole-rule override* (the winning rule replaces the others): then underlining
`mutable` would drop its colour, the exact thing the user asked to avoid. (c)
*Specificity across layers* (a more specific theme rule beats the user's): the
user's file would lose to a theme they didn't write.

## 3. Style roles

A **role** is an abstract style a rule or an extension names instead of a colour
and decoration, so a theme restyles it once. **The closed list** (fastedit owns
it; extensions cannot add roles):

| Role | Meaning | Bundled default |
|---|---|---|
| `emphasis.underline` | a state worth marking without a colour (rust `mutable`) | underline straight |
| `emphasis.strong` | stronger than the surrounding code | bold |
| `emphasis.soft` | gentle emphasis | italic |
| `muted` | de-emphasised | `editor.muted` colour |
| `warning.subtle` | something a reader should notice | underline dotted |
| `deprecated` | superseded | strike |
| `unnecessary` | unused or unreachable | `fg` at ~0.5 alpha |
| `error.subtle` | wrong but not a diagnostic | underline wavy, error colour |

**Theme `[roles]`.** A table of `role = style`, where a style is an inline table
of the §2 attributes: `"emphasis.underline" = { underline = "straight" }`.
A theme that lists none gets fastedit's defaults. **A rule's value** may be a
role (`style = "emphasis.underline"`). **Roles resolve first, rules second.**
The `[roles]` tables of every layer are merged, per role and per attribute,
user over theme over extension over defaults, into one final role table; only
then is a rule's `style = <role>` expanded from that table, into attributes
the rule sets at the *rule's own* layer and specificity, with any attribute
written beside it overriding the role's. So a user's `[roles]` restyles a
theme's or an extension's rule that names the role, and a rule naming a role
never takes its layer's possibly-stale copy. A role's own entry is plain
attributes: roles don't reference roles. An `fg` or `bg` that names a role
is not allowed (a colour is a colour).
**Extension mapping:** in `[[semantic.modifiers]]` and `[[semantic.types]]` an
entry's default `role` names one (§4); an unknown role is a warning and the
entry is kept without styling.

**Rejected.** (a) *Open-ended roles an extension can add*: a theme then can't
know what to style, and every extension invents its own `mutable` role. (b)
*Extensions shipping literal colours and decorations*: dark/light clashes and
the user can't restyle a language without touching an extension. (c) *No roles,
only per-selector rules in themes*: every theme needs a rule per language
modifier, so Dark Modern knows nothing of an extension installed later.

## 4. The `semantic` contribution point

**Manifest.** A fastedit kind `semantic` in `extension.toml`, registered with
`extension_kind("semantic", schema)` (schemars, like `lsp`); `Layer::check_schema` gives positioned
diagnostics for the table's own keys and, after the kiln prerequisite below,
for array entries; kiln keeps the table
as written. Because T839's kinds are `[contributes.<kind>]`, it is
`[contributes.semantic]`:

```toml
[contributes.semantic]
language = "rust"                      # the fastedit language name its tokens belong to

[[contributes.semantic.types]]
name = "lifetime"
super = "typeParameter"                # supertype for fallback; omit for none
role = "emphasis.soft"                 # optional default role

[[contributes.semantic.modifiers]]
name = "mutable"
role = "emphasis.underline"
description = "A binding or reference that can be written through"

[[contributes.semantic.rules]]         # default rules (the extension layer)
selector = "variable.mutable"
underline = "straight"
```

A selector in `rules` is implicitly `:language`; one naming another language is
a warning. `name`s are `[a-z][a-zA-Z0-9]*`, may not shadow a core type or
modifier, and are scoped to the language (two languages may each define
`unsafe`). The legend maps by name: a server's legend entry that's a core or
contributed name is used, anything else is dropped (that token keeps tree-sitter's
colour, as T175 intends for an unknown type).

**Theme files.** fastedit adds `semantic` and `roles` to `EXTENSIONS`
(`theme.rs:32`: `["editor", "syntax", "semantic", "roles"]`). `[semantic]` is a
table of selector → rule (`"variable.mutable:rust" = { underline = "straight" }`).
`[syntax]` stays as in §1 and is read as the lowest rules **within the theme layer**.

**User settings.** The user's `settings.toml` gets `[semantic]` (rules, same
form) and `[roles]`; this is the top layer. The user's `theme.toml` is a theme
file and carries the same tables as a theme layer over the selected theme
(unchanged from T839 §4); `settings.toml`'s tables sit **above even that**, so a
quick personal tweak needs no theme file. *Open question 2.*

**Layer order**, lowest first: fastedit defaults (bundled `[semantic]`/`[roles]`
in `fastedit-dark|light.toml` plus the bridge), extension `rules` (by extension
id, then file order), the selected theme with its bundled base, the user's
`theme.toml`, the user's `settings.toml` tables. §2 says how they cascade.

**What lives where.** *kiln*: nothing beyond the existing generic kind plumbing
(`extension_kind`, `check_schema`, `on_extensions`, `theme_extensions`),
**plus one generic prerequisite**: at the base `Layer::check_schema`
(`kiln-core/src/layers.rs:232`) recurses into tables only and checks an array's
outer type, not its entries, so `[[contributes.semantic.types]]` fields would
get no positioned diagnostics. A kiln card makes it follow `items` for arrays
of tables (positions from the layer's spans, which that card verifies cover
array elements); it knows nothing of tokens. Until it lands, fastedit's
`on_extensions` hook validates the entries itself (serde with
`deny_unknown_fields`) and reports them against the manifest file and the key
path with an index, without a line. *fastedit*: `fe-style` (model, resolver), `fe-config` (parsing the
tables, schema, layering, `Settings`), the app's `extension_kind("semantic", …)`
and `on_extensions` hook that merges contributed types, modifiers and rules per
language, and the Rust manifest.

**Rejected.** (a) *Make "semantic" a kiln-native kind like `themes`*: it is
fastedit's notion (editor tokens); kiln must stay token-free. (b) *Semantic
rules inside `contributes.themes`*: a theme is for a look, a language extension
must bring its meaning without owning a theme. (c) *Defaults only in the theme*:
Rust's `mutable` would vanish under any theme that doesn't know it.

## 5. Rendering

**What a decoration carries.** `Foreground(Arc<str>)` (T173) is colour-only and
names a highlight. It's replaced before it lands by `DecorationKind::Token(Token)`
in fe-editor: the decoration carries the **token** (type + modifiers + the buffer
language is the editor's), not a resolved style. fe-gpui resolves it at paint
with the theme's cascade (§2), so a theme or settings change restyles the open
overlay without asking the server again. fe-editor depends on `fe-style` (a leaf,
no GPUI) but still not on fe-syntax or LSP. The tree-sitter spans (`HighlightSpan`)
resolve to tokens the same way (§1) through a per-`Highlight` table cached per
theme, so the hot path stays a table lookup per span.

**The overlay.** `styled_highlights` (`element.rs:2352`) yields `StyledSpan<Style>`
(fg, bg, bold, italic, underline, strike) instead of `StyledSpan<Hsla>`: tree-sitter's
token per span, then the frame's `Token` decorations merged over it, per attribute
(§2's resolver applied to the semantic token alone, then each attribute the
semantic style *sets* overrides tree-sitter's; one it doesn't set keeps it). So
semantic `mutable` underlining changes nothing else, and a semantic `fg` only
replaces where a rule gives one.

**No flicker, no shift.** Tree-sitter paints at open; the semantic answer lands
later and only changes paint attributes: colour, background, underline, strike,
bold, italic. The editor font is monospace, so none moves a glyph; the card
that paints bold and italic pins that with a test that a frame with and without
`Token` decorations lays out identically. Answers for an older version are
dropped; the decorations shown follow edits (anchored) until the next, as T175
specifies. The first answer is not debounced; later ones are.

**A modifier the syntax layer can't tag.** Later uses of `let mut x` aren't
`mutable` in tree-sitter: it sees an identifier. The decision: **fe-syntax
doesn't guess it.** The modifier appears only from a language server, or from
a language's query with a scope-aware predicate that a later card may add. With
no server, uses of `x` are plain `variable`; the declaration `mut x` may get
`variable.declaration` + `modification` only if a bundled query tags it. The
extension's value for such a language is whatever its server provides.

**Rejected.** (a) *Resolve at decoration-build time (the decoration carries a
`Style`)*: stale on a theme change, and the workbench would need the theme. (b)
*Keep `Foreground(name)` and add `Underline`, `Bold`… decorations per
attribute*: the workbench would build a decoration per attribute per token and
re-implement the cascade. (c) *Guess `mutable` from tree-sitter text patterns*:
wrong at shadowing, closures and macros, and it's the server's job.

## 6. Impact on open cards

(Argon numbers; Mac numbers in brackets.) Each reads the model through
`fe-style`, the one resolver.

| Card | Verdict | What changes |
|---|---|---|
| **T174** (Mac T945) fe-lsp decodes tokens | **Stands as written.** It already returns `token_type: Arc<str>`, `modifiers: Vec<Arc<str>>` by legend name; fe-lsp need not depend on fe-style yet (§8 card 1 types them later). |
| **T173** (Mac T944) `Foreground(Arc<str>)` | **Respecced.** Replace the kind with `Token(Token)` carrying type+modifiers; `Foreground` as a name loses what `mutable` needs and would be removed again. Its test and the merge in `element.rs` change to resolve through the cascade; scope adds `fe-style`. The no-decoration path and the per-frame cost claim stand. |
| **T175** (Mac T946) `lsp_semantic_tokens.rs` | **Respecced.** Delete `highlight_name` and its table; build a `Token` from the decoded type and modifiers, dropping legend names that are neither core nor contributed for the buffer's language. The 250 ms, refresh and setting behaviour stand. Depends on the new T173 shape. |
| **Mac T917** (diagnostic tags Unnecessary/Deprecated) | **Respecced** (the verdict is on the Mac card whether or not argon has a counterpart; none is on argon's board at the base, so it is respecced when filed, not edited here). Its styling reads the roles `unnecessary` and `deprecated` (§3) through the §2 cascade as a diagnostic-sourced token (type `*`, modifier `deprecated`/`unnecessary`) rather than fixed fade alphas or strike colours, so one theme restyles tags and semantic `deprecated` together. Its decoding of the tags (`fe-lsp/src/diagnostics.rs:82`) stands. |
| **T169** (Mac T932) inlay-hint chips | **Stands as written.** A chip is a background chip, not a token; it reads `editorInlayHint` colours from `[editor]` keys. It may later read a `muted` role; not worth a respec. |
| **T195** (Mac T988) bracket-pair colours | **Stands**, with one note: `bracket_1..3` are `[editor]` keys and brackets are `punctuation.bracket` tokens; it keeps its own keys (an editor-level feature that needs depth, which a token has no notion of). A later card may express them as `punctuation.bracket.depth1`. |

## 7. Dark Modern

T200 (Mac T994) **ships now** as specified: `[editor]` colours and a `[syntax]`
table of colours by `Highlight` name, read as the theme layer's lowest rules
(§1), which is a strict subset of this model. **Waits** for the follow-ups:
`[semantic]` rules (`*.mutable`/`*.readonly` styling), `[roles]`, bracket-pair
colours (T195's keys), inlay-hint colours (T169's keys), and
`deprecated`/`unnecessary` styling. A later card adds them to the same file; no
decision here blocks T200.

## 8. Cards

Drafted after sign-off. Order is dependency order; the size is the Regatta
`s/m/l`.

0. **kiln: `check_schema` follows array entries** (s). `kiln/crates/kiln-core/src/layers.rs`
   (`check_table`, `wrong_type`) and its tests: arrays of tables are checked against
   `items`, with positions. Generic; lands in kiln (Mac side owns it) before card 4 needs it.
1. **`fe-style`: the model and resolver** (m). New `crates/fe-style/`
   (`Cargo.toml`, `src/lib.rs`, token/selector/style/resolver modules and tests);
   workspace `Cargo.toml`. Types, selector parsing, the §2 cascade, roles,
   the bridge table from `Highlight` names; unit tests for specificity, per-attribute
   independence and layer order.
2. **fe-syntax emits typed names** (s). `crates/fe-syntax/src/highlight.rs`,
   `Cargo.toml`: `Highlight::token()` per the §1 bridge; captures with modifiers
   from queries (`@variable.readonly`) map through `from_capture`. `Highlight(u8)`
   stays.
3. **fe-config parses `[semantic]`/`[roles]`** (m). `crates/fe-config/src/theme.rs`
   (`EXTENSIONS`, `EditorPalette`), `settings.rs`, bundled themes, settings
   schema. `syntax_color` stays; adds `EditorPalette::rules()`.
4. **The `semantic` kind and layering** (m). fastedit's app setup
   (`extension_kind("semantic", …)`, `on_extensions`), `fe-config` layer merge,
   per-language registry of contributed types/modifiers/rules, diagnostics.
5. **Rendering full styles** (m). `fe-editor/src/decorations.rs`
   (`DecorationKind::Token`), `fe-gpui/src/element.rs` (`StyledSpan<Style>`,
   underline/strike/bold/italic paint), the no-layout-shift test; replaces T173's
   shape.
6. **The Rust extension's manifest** (s). A bundled extension directory with
   `extension.toml` and a `[contributes.semantic]` for `lifetime` and `mutable`
   (and rust-analyzer's `unsafe`, `consuming`, `callable`, `controlFlow`,
   `public`, `crateRoot`, …, each an explicit pick). Its `lsp` entry is T144's.
7. **Respec T173 and T175** (s each, done in place) after card 5's shape is signed.

## 9. Open questions for the user

1. **Case of names.** §1 fixes LSP's camelCase, case-sensitive (`typeParameter`,
   `defaultLibrary`), so a legend entry round-trips. The alternative is
   snake_case in selectors, translating at the legend edge. Which?
2. **Where the user's rules live.** Proposed: `settings.toml` `[semantic]`/`[roles]`
   above the user's `theme.toml`. Or only in `theme.toml`, no settings form.
3. **Role list.** Eight roles (§3), closed. Add, cut or rename any?
4. **`mutable` without a server.** §5 says tree-sitter won't guess it, so a
   Rust file without rust-analyzer shows no underline. Acceptable, or should a
   scope-aware query card follow?
5. **A new crate.** `fe-style` as a leaf (§1). Or fold the model into
   `fe-config` and let fe-syntax and fe-lsp depend on it (against §1's note
   that fe-config stays light)?
6. **Respec T173 and T175 now** (§6) or let T173 land colour-only first and
   replace it in card 5?

**Please sign this record off, or say what to change.** Nothing in §8 is filed
until you do.
