---
task_uid: "01m77zvsgey8kh16ks9rjc4j4j"
task: "T1736"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "`tests::add_ons::provider::primary_only_feature_goes_to_add_on_when_primary_lacks_it` and `tests::add_ons::provider::nobody_asked_when_neither_has_the_provider` pass"
    required: true
    manual: false
  - id: "C2"
    text: "`tests::add_ons::first_answer::primary_non_empty_answer_wins_even_when_add_on_is_first` and `tests::add_ons::first_answer::add_on_answer_used_when_primary_is_empty` pass"
    required: true
    manual: false
  - id: "C3"
    text: "`tests::add_ons::first_answer::rename_goes_to_accepting_owner_only` and `tests::add_ons::first_answer::rename_to_restarted_owner_is_unavailable_and_sends_nothing` pass"
    required: true
    manual: false
  - id: "C4"
    text: "`tests::add_ons::owners::follow_ups_reach_only_the_producing_server_never_a_restarted_session` passes (colour presentation, link resolve, hierarchy expansion)"
    required: true
    manual: false
---

# Route features across servers in fe-lsp: provider selection, first-that-answers, and owner-carrying results and rename (LSP add-ons A6, T1645)

# Route features across servers in fe-lsp: provider selection, first-that-answers, and owner-carrying results and rename (LSP add-ons A6, T1645)

Spec: §5 (Feature routing: provider rows, first-that-answers, owners), §6.1 (`provider_of`, `Feature`), §7 (Routing, Owners), §8 row A6.

Row: routing core: `provider_of` and `Feature`, plus `ask_all` / `first_that_answers` helpers over `document_request` aimed at one server. Owner fields on `ColorInformation`, `DocumentLink` and `HierarchyItem`, so colour presentations, link resolve and hierarchy follow-ups go to the owner. The provider rows of section 5 (symbols, formatting, on-type, folding, semantic tokens, linked editing), plus signature help, highlights, rename and hierarchy preparation as first-that-answers. `RenameTarget.owner`, and `rename` taking it and sending only to that session (`request_if`; restarted owner gives `Unavailable("the language server restarted")`).

Test files: crates/fe-lsp/src/tests/add_ons/provider.rs, first_answer.rs, owners.rs.

Read the design sections named above before starting. Do not widen --scope; if a file outside it is needed, stop and say so.

## Implementation notes

- **Scope added by respec**: `crates/fe-lsp/src/lib.rs` (export `Feature`, `Owner`),
  `crates/fe-lsp/src/tests/add_ons.rs` (declare the three test modules, fixture helpers),
  `crates/fe-lsp/l10n/en-US/fe-lsp.ftl` (`lsp-unavailable-restarted`), `crates/fe-lsp/src/server.rs`
  (carried T1735 F1), and the sibling tests `rename_tests.rs`, `document_color_tests.rs`,
  `hierarchy_tests.rs` and `tests/provider_wait.rs`, which compare targets and colours with `==` or
  forge an item's opening, and so see the new owner.
- **Routing core** (`manager.rs`): `admit_to_first` admits on the first of a list of servers whose
  capabilities accept, waiting for each in turn while unknown (one lone server behaves as
  `admit_once_known` did, which it replaces). On it: `provider_request` (§5 provider),
  `document_request_if_provided_on` / `document_request_on` (one server, for `ask_all`),
  `owner_request` / `admit_to_owner` (follow-ups). `first_that_answers` (`requests.rs`) polls every
  server's request at once and takes the first non-empty answer in server order, waiting for earlier
  servers; with none, the earliest empty answer, else error, else "not provided". Later requests are
  dropped (cancelled) once the answer is known.
- **`Feature`** has `RangeFormatting` beside the design's list, since range formatting has its own
  capability. `provider_of` reads capabilities known now; requests wait for them.
- **`Owner`** (server, buffer, opening) is on `ColorInformation::owner()`, `DocumentLink::owner()`,
  `HierarchyItem::owner()` and `RenameTarget.owner`. A follow-up whose owner has stopped, restarted
  (crash restart in place or `restart`), or reopened the document sends nothing:
  `Unavailable("the language server restarted")`; hierarchy follow-ups keep their existing
  `Unavailable("stale")`. Items made with `new` (no owner) go to the lead as before.
- **Rename**: `rename(buffer, version, offset, new_name)` keeps its signature, so fe-workbench builds
  until W1; with no prepared target it goes to the provider of `renameProvider`.
  `rename_prepared(buffer, version, &target, offset, new_name)` goes to the target's owner alone;
  W1 moves the workbench's rename picker to it.
- **Colours and links** go to the provider of `colorProvider` / `documentLinkProvider` until A7
  merges them, so that each carries an owner now.
- **Signature help**: a buffer on one server asks it whatever it advertises (unchanged); on several,
  only servers with `signatureHelpProvider`, and for a typed character only those listing it.
  `signature_help_triggers` is the union, the lead's first.
- **T1735 F1**: `server::hand_over` (and the actor's own hand-over of its backlog) now treats a queued
  `Close` as ending the openings of that buffer before it, as `Carry::hold` already did. Regression:
  `tests::add_ons::reconcile::reroot_while_old_launch_held_opens_once_on_the_new_root` (fails at base
  with `["didOpen", "didClose", "didOpen"]` on the new instance).
