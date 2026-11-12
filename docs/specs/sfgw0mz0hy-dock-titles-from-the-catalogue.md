---
task_uid: "01m77sav274dmnggsfgw0mz0hy"
task: "T357"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "No dock_item! arm titles a panel with $sidebar.name; References, Search, Chat and AI Changes resolve their titles from dock.ftl"
    required: true
    manual: false
  - id: "C2"
    text: "en-US byte-equal; cargo nextest run -p fe-workbench passes, including --test l10n"
    required: true
    manual: false
  - id: "C3"
    text: "New tests in crates/fe-workbench/src/tests/dock_titles_locale.rs pass: under qps-ploc the References and Search docks' titles are the fake words, and English again after switching back to en-US"
    required: true
    manual: false
---

# Dock titles from the catalogue: References, Search, Chat and AI Changes headers stop showing SidebarPanel::name (T1561 review F2 follow-up) (Mac T1729)

# Dock titles from the catalogue: References, Search, Chat and AI Changes (T1561 review F2 follow-up)

`dock.rs`'s `dock_item!` default arms (`($panel, $sidebar)`, `actions = …`, `off_rail`) title the dock with
`$sidebar.name`, the `&'static str` in each panel's `SidebarPanel` const. Project, Problems, Changes,
Quickfix, Hierarchy, History and the Git log already resolve their titles from the catalogue; References
(`references_panel::SIDEBAR`), Search (`search_panel::SIDEBAR`), Chat (`chat_panel::SIDEBAR`) and AI
Changes (`ai_changes_panel::SIDEBAR`) still show English. T1561's review (codex F2) carried References as a
known limit; the same arms cover the other three.

Give each of those four `dock_item!` uses a catalogue title (new keys in
`crates/fe-workbench/l10n/en-US/dock.ftl`, e.g. `workbench-dock-references-title`), and remove the arms
that title by `$sidebar.name`, so a new panel can't fall back to it. `SidebarPanel::name` stays as the
panel's internal name (rail order, tests); don't touch the panels' own files. In `l10n-allow.toml`, fix the
References and Search entries' reasons (they're no longer shown). The test-only `Dock::title()` returns
what the header draws (the active item's `DockItem::title`), not `DockPanel::name()`.

Chat and AI Changes' consts are in T1562's files (blocked); this card doesn't add their allow entries.

en-US stays byte-equal. A new test module, `crates/fe-workbench/src/tests/dock_titles_locale.rs`
(declared from `src/tests/mod.rs`, the `qps-ploc` fake-locale pattern of `settings_editor_locale_tests.rs`),
shows References and Search under the fake locale and checks the dock's title is the fake word, and English
again after switching back to en-US.

Platform: **Linux-OK**.
