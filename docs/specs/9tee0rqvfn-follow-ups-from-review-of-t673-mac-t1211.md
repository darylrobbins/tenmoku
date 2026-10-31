---
task_uid: "01m69aetq1nhwvpe9tee0rqvfn"
task: "T385"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T673 (Mac T1211)

Findings of the review of T673 that don't block the merge (below merge.block_severity):

- F7 medium kiln/crates/kiln-shell/src/menu_bar.rs:168: The new reconciliation checks positional validity rather than menu and item identity. With contributed menus [Tools(A), Sharing(B)], open Tools and replace A with a spec without a menu: Sharing moves into index 0, so the removed menu stays open and Enter executes B. Similarly, inserting a lower-order item into an open menu can retain the highlight on a different command. The render checks repeat this mistake; the regression only removes the entire bar.
  fix: Track the open menu by name and the highlighted item by ID, reconciling their new positions and closing or clearing selection when they disappear; alternatively close the menu whenever its contents change. Test removal with a surviving sibling menu and insertion/removal before the highlighted item.
  instances: kiln/crates/kiln-shell/src/menu_bar.rs:168, kiln/crates/kiln-shell/src/menu_bar.rs:172, kiln/crates/kiln-shell/src/menu_bar.rs:270, kiln/crates/kiln-shell/src/menu_bar.rs:282

- F8 medium kiln/crates/kiln/src/contributions.rs:209: A sibling of F5 remains in the unchanged post-init registration path: the callback runs before the outer spec is stored. If it registers its command and then contributes a replacement for the same ID, the nested replacement updates typed metadata, but the outer retain/push subsequently discards its keys, menu and registry metadata. The palette can therefore show the replacement while bindings and menus use the older spec.
  fix: Store the contribution before invoking its registrar, or use registration generations so a returning callback cannot overwrite a newer nested replacement. Add a post-init same-ID replacement regression.
  instances: kiln/crates/kiln/src/contributions.rs:209, kiln/crates/kiln/src/contributions.rs:217

## Implementation notes

- F7: already fixed on the base by 9ebf55fa (T134): `menu_bar.rs` locates the open menu by name and the highlight by command, with `tests/menu_bar_identity.rs`. No change made.
- F8: `Contributions::add_commands` stores the spec before running its registrar, so a nested same-ID replacement is kept. Regression: `a_replacement_contributed_while_registering_after_init_is_kept`.
