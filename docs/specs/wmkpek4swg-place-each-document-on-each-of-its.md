---
task_uid: "01m78t0pwfvnpwj2wmkpek4swg"
task: "T363"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "`tests::add_ons::sync::both_servers_get_did_open_and_each_edit_at_its_own_version` and `tests::add_ons::sync::max_file_bytes_closes_the_document_on_both` pass"
    required: true
    manual: false
  - id: "C2"
    text: "`tests::add_ons::crash::add_on_crash_leaves_primary_alone` and `tests::add_ons::crash::primary_crash_leaves_add_on_alone` pass"
    required: true
    manual: false
  - id: "C3"
    text: "`tests::add_ons::lead_unavailable::untrusted_primary_still_places_trusted_add_on` passes: lead_of is None, servers_of lists the add-on, route_servers lists the primary first with no id"
    required: true
    manual: false
  - id: "C4"
    text: "`tests::add_ons::holders::same_lead_different_add_on_sets_have_one_holder` and `tests::add_ons::holders::reload_merging_two_groups_keeps_earlier_holder` pass"
    required: true
    manual: false
---

# Place each document on each of its servers independently, with one holder per path and lead, and expose servers_of, lead_of, is_served, served_identities and language_servers (LSP add-ons A4, T1645) (Mac T1734)

# Place each document on each of its servers independently, with one holder per path and lead, and expose servers_of, lead_of, is_served, served_identities and language_servers (LSP add-ons A4, T1645)

Spec: §3 (spec model, placement, holders), §4 (document sync), §6.1 (the new fe-lsp API), §7 (Sync, Isolation, Lead unavailable), §8 row A4.

Row: placement per (document, server) at open, close, `changed` and crash: `Route.roots`, keyed places and lot, one holder per (URI, lead name) chosen apart from placement, each server placed independently (trust, crashes), and a configured lead that is never replaced, plus `servers_of`, `lead_of`, `is_served`, `served_identities`, `routes_of`, `language_servers`. Requests still go to the lead.

Test files (new, declared from tests/add_ons.rs and tests/mod.rs): add_ons.rs, add_ons/sync.rs (both replicas, versions, `max_file_bytes`, aliases), add_ons/crash.rs (one crashes, the other unaffected), add_ons/lead_unavailable.rs (untrusted, missing or crashed-out primary with a healthy add-on; holder kept; `route_servers` lead first with no id), add_ons/holders.rs (same lead with different add-on sets, groups merging on a reload, two leads meeting on one add-on).

Read the design sections named above before starting. Do not widen --scope; if a file outside it is needed, stop and say so.

## Implementation notes

- **Keys.** `Places.places` and `Places.lot` are keyed by `(BufferId, server name)`. `Routes` is now a handle naming its server (`Routes::named`), so each server thread's hand-over (server.rs and restart.rs's `Carry`) forwards under its own key with no change to restart.rs. A document no server lists keeps one opening in the lot under the empty name, with no place, so `changed` still drains its subscription; the first server it gains takes that opening.
- **Route.** `roots: Vec<(String, PathBuf)>` (lead first, then add-ons), plus `lead: Option<String>` (the configured lead, `None` while no server lists the language, when the roots are kept as they were), `primary`, the open order, the generation and the size limit, so an opening for a server gained later can be made from the hub.
- **Holders.** `Route.holder` is explicit state, chosen apart from placement: a buffer opened into a (URI, lead) group that has a holder waits; `close` hands the path to the group's next earliest opened; and `regroup`, run after each `reroot` in `reconcile` (and after a `restart` retires), gives each group one holder: of groups that merged, the earliest opened of their holders keeps it, placed or not (review round 1, F1: it once chose from placement, so an earlier holder that lost its only server on the reload lost the path), and the others are closed on every server they're on. `place_for` places only the holder, so open, close, retire, `place_waiting` and restart all respect it.
- **Per-server close.** A server a document loses (reroot) or an alias released on a merge gets `Command::Close`; its identity on that server alone is retired by the manager before the close and again behind it (a `Command::Documents` closure), for an open still on its way there. No new `Command` variant, since restart.rs matches them exhaustively and is outside the scope.
- **API.** `servers_of`, `lead_of` (`server_of` is now its alias), `is_served`, `served_identities`, `route_servers` (the A4 row's `routes_of`; §6.1 names it `route_servers`), `status_at`, and `language_servers` with a `bool` for the add-on flag (`language_server` is its first entry). `route_of` is the configured lead's name. Requests (`admit`, `admit_once_known`, on-type triggers) still go to the lead.
- **A lead that keeps running stays.** As today (fe-workbench's `a_document_kept_on_a_running_server_survives_a_preferred_route_change`), `reroot` keeps a route's lead while the document is placed on a running instance of it that still lists the language, even when another primary now comes first. Only that instance keeps it: `target` never starts a server that isn't the language's first primary or one of its add-ons, and `retire` moves a document off a kept lead to the language's own lead (review round 1, F2: a spec change or `restart` of the kept lead used to place it back on the new instance). `reconcile` re-roots again after retiring servers. `route_of` is that lead's name, as `route_of` was where the document actually is. So groups merge on a reload only when the alias's old lead stops.
- **`open`'s answer.** It returns the instance the lead was placed on, as before, even if that has failed since (not `lead_of` afterwards, which races a server that fails at once).
- **Tests.** Beyond the criteria: `sync::opened_over_the_limit_opens_on_neither`, `sync::aliases_are_placed_on_both_servers_by_one_holder`, `lead_unavailable::missing_primary_still_places_add_on`, `lead_unavailable::crashed_out_primary_still_places_add_on`, `holders::two_leads_meeting_on_one_add_on_keep_the_first_placed`, `holders::reload_keeps_the_earlier_holder_when_it_loses_its_only_server` (F1), `lead_kept::a_kept_lead_restarted_by_a_spec_change_gives_way` and `lead_kept::a_kept_lead_restarted_by_hand_gives_way` (F2).
