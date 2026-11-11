---
task_uid: "01m71fq511295hme0vh741vsb4"
task: "T1677"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Increase Contrast from the A11Y-10 observer applies the token rule and editor palette section 8 specifies, behind the setting it names"
    required: true
    manual: false
  - id: "C2"
    text: "The high-contrast appearance names are mapped so theme.mode = system keeps the dark or light theme matching the system (section 8's workaround): Tenmoku's own appearance reads do not go through GPUI's appearance mapping and log no \"unknown appearance\". Known limit: GPUI's internal appearance reads (window open, system appearance change) may still print that line; changing GPUI is out of scope and the remaining log is left to human check H9"
    required: true
    manual: false
  - id: "C3"
    text: "Tests in kiln/crates/kiln-ui/src/tests/theme_tokens.rs cover the token rule for every shipped theme, and pass"
    required: true
    manual: false
  - id: "C4"
    text: "The finish notes list this card's section 12 human-check lines (H-numbers and text), for the A11Y-11 VoiceOver pass"
    required: true
    manual: false
---

# A11Y-10b Increase Contrast: token rule, editor palette, setting, and the high-contrast appearance workaround (T294 design)

# A11Y-10b Increase Contrast

Spec: `docs/decisions/qenw9xxdzk-accessibility-design.md`, the **A11Y-10b Increase Contrast** row of section 11 "Proposed card breakdown", and section 8 "System settings to respect" (Increase Contrast and the appearance workaround).
The user approved the design's platform path (section 3, option (a): gpui-pre 0.3.9's
AccessKit, no vendoring) and its first slice (section 10) as written on 2026-11-09.

Build what that row and those sections specify, with the tests section 9 "Testing strategy"
assigns (shape 1 for text elements, shape 2 for chrome). If the design's text differs from
this card's criteria, the design wins.

Platform: **Mac-only reader; the token rule builds and is tested on Linux**.

## Implementation notes

- **Token rule** (`kiln-core/src/theme/tokens.rs`, `high_contrast`): a pass over resolved tokens, applied by `ThemeData::high_contrast` (marked `contrast: Contrast::High`). The four selection tokens double their alpha (capped at opaque); then every row background (`ROW_GROUNDS`: `state.hover/selected/selected_inactive`, `list.row_hover/row_selected/row_selected_inactive`) is brought down, by alpha, to the strongest that still lets white or black read at 7:1 on it over every surface. That's below the theme's own where needed: Monokai's opaque `#75715e` selection only manages 4.91:1 even unchanged, and doubling VS Code Dark Modern's `#ffffff22` leaves 6.79:1. Muted text then moves towards white or black until it reads at 7:1 on every ground (`text_grounds`: `surface.background`, `panel`, `chrome`, `raised`, and each row background over each of them). Faint text goes to 4.5:1 on the same grounds: the design names only muted, but faint would otherwise drop below muted's floor by more than a step. `text.primary` is raised to be never weaker than muted. `border.default/subtle/control` take the raised muted colour unless they're already stronger; `state.focus_ring` and `list.row_focus_border` become opaque. The surfaces are pinned, because `surface.raised` derives from `text.primary`. Tokens derived from the moved ones follow through `resolve`'s rules; everything else is untouched.
- **2 px focus ring:** kiln-ui's `focus_ring()` is already `FOCUS_RING` (2 px, opaque accent), and the prompt's ring is 2 px of `state.focus_ring`, now opaque under high contrast. The setup wizard's own ring (`crates/fe-workbench/src/setup_wizard.rs`, review round 1 F2, in scope since rev 2) is 2 px with no padding under high contrast (`ring_widths`), and 1 px plus 1 px of padding otherwise, so controls don't move. Its test is `setup_wizard_tests::high_contrast_ring_is_two_pixels_in_place`.
- **Applying it** (`kiln-ui/src/theme.rs`): `GlobalTheme` keeps the theme as set (`plain`) and draws it at the current `Contrast` (`Theme::with_contrast`); `GlobalTheme::set_contrast(cx, contrast)` changes it live, and every later `set_theme`/`set_themes`/`set_ui_theme` keeps it. It's an associated function so `kiln-ui/src/lib.rs` (not in scope) needn't change; `Contrast` is reached as `kiln::core::theme::Contrast`.
- **Editor palette** (`fe-config/src/theme.rs`): `EditorPalette::for_theme` runs `EditorPalette::high_contrast` for a theme marked high. It raises `gutter_foreground` and `gutter_active_foreground` to 7:1 on every ground fe-gpui draws them on. I went through every `gutter_foreground`/`gutter_active_foreground` use to list them:
  - line numbers, ghost text, blame and lenses on the background and the caret line;
  - inlay pills, hover cards, signature help, which-key and the diff view's header on the popover surface;
  - a completion's detail and kind on its selected row (`selection` over the popover);
  - sticky scroll's band, plain and hovered;
  - a diff view's hunk headers and hidden-lines labels on `diff_filler`, and their chevrons on `sticky_scroll_hover` over it.

  First the backgrounds under that text (`active_line`, `selection`, `diff_filler`, `sticky_scroll_hover`) are brought down by alpha, as the chrome's rows are (`kiln_core::theme::room_for_text`), to where 7:1 text is possible on them at all. Monokai's mid-tone selection can't host it unchanged (5.72:1 at best), so its selection is fainter under high contrast. Colour swatch borders and the image viewer's frame are non-text and aren't counted. fastedit's `theme::apply` already rebuilds the palette on every `GlobalTheme` change.
- **Setting:** `workbench.contrast = "system" | "high" | "normal"` (`ContrastSetting::resolve`). `crates/fastedit/src/main.rs` applies it with the system's Increase Contrast (`apply_contrast`) after `theme::init`, on every `SystemA11y` change, and on a settings reload that changes it. As with A11Y-10, the new key needed the schema golden, both settings ftl files, the template toml, the qps-ploc fixture and the fe-workbench Settings label, all in scope since rev 2. The setting's test is `tests::settings::contrast_follows_the_system_unless_overridden`.
- **Appearance workaround:** `kiln_platform::system_a11y::appearance()` (macOS: `NSApp.effectiveAppearance bestMatchFromAppearancesWithNames:@[Aqua, DarkAqua]`, main thread only, never creating `NSApp`; `None` elsewhere). The platform-neutral entry point goes in `kiln/crates/kiln-platform/src/system_a11y.rs` (in scope since rev 2) because `system_a11y/macos.rs` is a private module and kiln-ui may not branch on the OS. While `SystemA11y::increase_contrast` is on, kiln-ui's `system_appearance` takes that reading for `set_themes`, `set_contrast` and the window-appearance observer, and doesn't call GPUI's `window_appearance()` (whose mapping logs "unknown appearance"). This follows the system option, not the setting: the names are high-contrast whenever the option is on.
- **C2's known limit (rev 2):** GPUI itself reads the platform appearance when a window opens and when its appearance changes (`gpui-pre` `window.rs:1643, 2909`), through its own mapping. If AppKit really reports the high-contrast names (H9's open question), those reads may still print "unknown appearance" to stdout. Changing GPUI is out of scope, and H9 checks it. Tenmoku's own reads never go through that mapping while Increase Contrast is on; `tests::theme_tokens::increase_contrast_follows_the_platforms_appearance` asserts that the GPUI reading isn't asked for. If the a11y notification arrives after the appearance change, one frame may pick light before `apply_contrast` re-picks, so H9′ should watch for a flash.
- **C3, "every shipped theme":** kiln-ui's `tests/theme_tokens.rs` checks the whole rule for kiln's shipped themes (Kiln Dark, Kiln Light and the flat default chrome) and for two deliberately low-contrast themes. It works out the rendered grounds itself (every surface, and every hovered or selected row over each), not through kiln-core's `text_grounds`. kiln may not read fastedit files (`scripts/check_kiln_boundary.py`), so Tenmoku's bundled themes (Yohen, Hakuji, Tenmoku, Catppuccin, VS Code, Monokai, …) are covered by `fe-config/src/theme.rs`'s tests, chrome tokens and editor palette both, again with independently listed grounds.
- **Carried T1343 findings in `setup_wizard.rs`:**
  - **F3 (Install off-screen in a short window):** already fixed in the landed code (`move_stop` scrolls the Install row into view). `setup_wizard_tests::tabbing_to_install_reveals_it_in_a_short_window` now pins it: a 260 px-tall window, Install out of view until it's tabbed to, then above the footer. The test fails with the scroll removed.
  - **F2 (Install's result toasts):** the toasts already render over the sheet (`workbench.rs` puts the toast layer after `self.setup`). Install's success and PATH-warning toasts still expire after the usual lifetime while Ready is open; error toasts stay. Making them stay needs `crates/fe-workbench/src/toast.rs` and `workbench.rs`, or fastedit's shell-command handler, none of which is in this card's scope, so it's left to its own card.
- **Section 12 human-check lines for A11Y-11 (C4):**
  - **H9.** System Settings › Accessibility › Display › Increase Contrast on, with the system in Dark mode and Tenmoku on `theme.mode = system`. Does Tenmoku switch to the light theme? Does the log print "unknown appearance"? Restore the setting afterwards.
  - **H9′. Increase Contrast on.** Tenmoku stays dark in Dark mode, with stronger borders, a 2 px focus ring, and readable muted text. Off again: back to normal, live, without a restart.
