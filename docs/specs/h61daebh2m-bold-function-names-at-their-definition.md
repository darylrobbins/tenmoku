---
task_uid: "01m7mnabmq4h35wph61daebh2m"
task: "T674"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Tests in cargo nextest run -p fe-gpui --test definition_bold pass: function_names_are_bold_over_their_colour, call_sites_are_not_bold, a_truncated_outline_styles_what_it_lists, a_language_without_an_outline_query_is_unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "No extra work for rows outside the viewport (the test counts outline lookups for a 10,000-row file)"
    required: true
    manual: false
---

# Bold function names at their definition, from the outline's name ranges (Mac T1601)

# Bold function names at their definition

Spec: Syntax ("Function: bold at its definition").

## Change

- A definition span source: `fe_syntax::outline` already lists functions and
  methods with `name_range` for ~40 languages from our outline queries
  (`OutlineItem { kind: Function, name_range, .. }`); the editor view marks
  those name ranges with the `function.definition` capture style, merged over
  the highlight spans (the existing colour stays, bold is added).
  `crates/fe-gpui/src/element.rs` gets the merge; the outline is read from
  what the view already has (no second parse), skipping the visible-row range
  work when the outline is not ready.
- Rows outside the viewport do no extra work; a file whose outline is truncated
  (`Outline::truncated`) styles what it lists.
- Languages without an outline query are unchanged. LSP semantic tokens'
  `declaration` modifier is not used (an open question).

## Not in this card

Other definition kinds (classes, types), a setting to turn it off (the theme
decides).

Platform: **Linux-OK** for the span test; goldens with a function definition
regenerate on the Mac.

## Helium filing note

Base: this needs Mac T1600 (style runs in the shaper, mac/main df5e32d98). Start only from a helium main that contains df5e32d98; if your base lacks it, block and say so rather than building around it. Goldens: none are named here, so regenerate none (the Mac regenerates golden screenshots with a function definition); if one is truly needed, use UPDATE_SCREENSHOTS=1, never UPDATE_GOLDEN. Kill only your own PIDs, never pkill.
