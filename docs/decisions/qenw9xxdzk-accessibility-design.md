# VoiceOver accessibility for the first release

## 1. Status

Status: **proposed**. Approver: the user. Approval has two parts:
the platform path (section 3) and the first slice (section 10). This card
files no implementation card. The lead readies the cards in section 11 only
after the user approves. Source card T294 (Mac T1430), written 2026-11-07.
It follows the spike in `docs/accessibility-audit.md` (T389, Mac T1266).

Scope: a **VoiceOver first pass** for the first public release, which is
macOS-only and editor-first. The doc covers how Tenmoku exposes itself to
VoiceOver, how it follows the system's accessibility settings, and how the
keyboard reaches every control. Terminal and chat get only a note on where
they slot in (cards A11Y-17 and A11Y-18).

**Decision:** status is `proposed`. Nothing is readied until the user
approves sections 3 and 10.

## 2. What was read and spiked

### Versions and sources

| Crate | Version (Cargo.lock) | Source read |
|---|---|---|
| gpui-pre | 0.3.9 = zed@93d7456 (2026-10-10), published 2026-10-12 | `~/.cargo/registry/src/*/gpui-pre-0.3.9/src/`: `_accessibility.rs`, `window/a11y.rs`, `element.rs`, `elements/div.rs`, `elements/text.rs`, `window.rs`, `app.rs`, `platform.rs`, `app/test_context.rs` |
| gpui-pre-macos | 0.3.9 | `src/window.rs` (a11y at 2367–2398), `src/window_appearance.rs` |
| gpui-pre-linux | 0.3.9 | `src/linux/x11/window.rs:1988–2008`, `src/linux/wayland/window.rs:2234` |
| accesskit | 0.24.1 | `src/lib.rs` (`Live` 584, `TreeUpdate` 2829, text properties 2043–2114) |
| accesskit_macos | 0.26.3 | `src/node.rs` (1338 lines), `src/event.rs`, `src/filters.rs` |
| accesskit_consumer | 0.38.0 | `src/text.rs`, `src/node.rs`, `src/filters.rs`, `src/tree.rs` |
| accesskit_unix | 0.22.1 | `src/context.rs` (activation on `org.a11y.Status.IsEnabled`, 184–200) |

GPUI isn't vendored or patched. `[patch.crates-io]` covers only
tree-sitter. `rust-toolchain.toml` pins 1.98.1 "to match zed@93d7456".

**What gpui-pre 0.3.9 provides, beyond the audit's API table:**

- **The tree is rebuilt in full every frame while a11y is active.**
  - `draw_roots` calls `A11y::sync_active_flag` and `begin_frame` (`window.rs:3673`).
  - Every element with an id and a role pushes a node during prepaint (`element.rs:428–462`).
  - `end_frame` sends the whole `TreeUpdate`, and AccessKit diffs it against the previous one (`window.rs:3774–3797`).
  - So per-frame cost grows with the number of nodes, even when nothing changed.
- **Synthetic children run after the element's prepaint and its children's** (`element.rs:476–497`).
  - A div can supply them through a closure: `.a11y_synthetic_children(|builder| …)` (`elements/div.rs:1385`).
  - The closure can also mutate its own node through `builder.parent_node()`.
  - That makes two things possible without a custom element:
    - a live region, using `Node::set_live`;
    - text runs that a child element computed in prepaint, read from a shared cell.
- **`push_child` adds leaves only** (`window/a11y.rs:346`). Synthetic nodes can't have children of their own. That is enough for `TextRun`s.
- **A custom `Element` can't become the focused node.**
  - Focus reaches AccessKit only from a div that has `track_focus`, an id and a role (`div.rs:2326–2344`).
  - The active-descendant pattern is also div-only (`aria_active_descendant`, `div.rs:1371`, applied at :2346–2352).
  - It's honoured only when the focused node is an *ancestor* of the descendant (`window/a11y.rs:258–272`).
  - `A11y::set_focusable`, `set_focus` and `set_active_descendant` are `pub(crate)`.
- **gpui handles no text action.** It never registers `SetTextSelection`, `ScrollIntoView` or `ReplaceSelectedText`.
  - The only actions it registers are `Click`, from `on_click` (`div.rs:3595`), and `Focus` for divs with `track_focus` (:3598).
  - When no listener matches, `Click` falls back to a synthetic mouse click at the node's centre (`window.rs:7054–7074`), and `Focus` focuses a tracked handle (:7075–7080).
  - Any other action needs `.on_a11y_action(Action, …)` (`div.rs:1503`).
- **gpui never sets `clips_children`.** So AccessKit's filter keeps off-screen children in the tree (`accesskit_consumer/src/filters.rs:54–92`).
- **On macOS a11y never deactivates.**
  - `gpui-pre-macos` `a11y_init` builds `accesskit_macos::SubclassingAdapter::for_window` with an activation handler and an action handler only (`window.rs:2367–2383`).
  - So once VoiceOver has touched a window, the tree is built every frame until the app quits.
  - Linux does pass a deactivation handler (`x11/window.rs:1988–2001`).
- **The test platform can't be activated.**
  - `PlatformWindow::a11y_init` defaults to a no-op (`platform.rs:1387`), and `platform/test/window.rs` doesn't override it.
  - The active flag is set only by the activation callback that `Window::new` hands to the platform (`window.rs:1667–1705`).
  - `TestAppContext` offers only `disable_accessibility` (`app/test_context.rs:191`).
- **gpui re-exports `accesskit`** (`gpui.rs:97–99`). Production code needs no Cargo change to use `Role`, `Live` or `TextSelection`.
- **Reduce motion.**
  - `App::reduce_motion` / `set_reduce_motion` are at `app.rs:1293/1299`. The setter refreshes windows.
  - The readers are `elements/animation.rs:301,407` and `elements/img.rs:321,385`.
  - Nothing in `gpui-pre-macos` reads `accessibilityDisplayShouldReduceMotion` or any other NSWorkspace display option.
- **Appearance.**
  - `gpui-pre-macos/src/window_appearance.rs:10–28` maps Aqua, DarkAqua and the two Vibrant names.
  - Any other name prints "unknown appearance" and returns **Light**. That includes the `NSAppearanceNameAccessibilityHighContrast*` names that AppKit reports while Increase Contrast is on.
  - So, unverified: a user in Dark mode with Increase Contrast on, on `theme.mode = system`, may get the light theme. See check H9.

**Tenmoku today:**

- The only a11y code is the setup wizard (`crates/fe-workbench/src/setup_wizard.rs`).
  - Its `node()` helper (:567–578) sets the role and `aria_label`, and records `(Role, label)` under `cfg(test)`.
  - Tests: `labels_and_roles` (`setup_wizard_tests.rs:486`), `every_control_is_labelled` (`setup_wizard_model_tests.rs:430`) and `sample_follows_settings` (`setup_wizard_preview_tests.rs:285`).
- `crates/fe-workbench/tests/a11y_tree_dump.rs` pins "headless has no tree".
- The text widgets have focus divs **without an `.id()`**: `TextInput` (`kiln/crates/kiln-ui/src/text_input.rs:804–806`) and `TextField` (`text_field.rs:608–610`).
  - The editor's focus div has one: `div().id("editor").track_focus(&self.focus)` (`crates/fe-gpui/src/view.rs:1929–1931`).
  - `EditorElement` is a child of that div (:1948), as are the completion, hover and signature popovers (:1949–1951).

### Zed upstream since the 0.3.9 snapshot, and later gpui-pre releases

- **gpui-pre is a third-party republish of Zed's gpui.** Its sole crates.io owner is `huacnlee`. It comes out about weekly.
  - **0.3.10** came out 2026-10-26 (zed@08cbb88) and **0.3.11** on 2026-11-02 (zed@f816e1f).
  - Neither changes the AccessKit requirements: `accesskit ^0.24`, `accesskit_macos ^0.26` and `accesskit_unix ^0.22`, the same as Zed main today.
  - `window/a11y.rs` and `_accessibility.rs` are byte-identical between 0.3.7 and 0.3.9.
- **Upstream a11y PRs that are already in 0.3.9:**
  - #56065 AccessKit support (2026-05-27)
  - #57954 `Application::inaccessible()`
  - #59429 Settings UI aria (gated behind `ZED_EXPERIMENTAL_A11Y`)
  - #60397 landmarks, F6 landmark navigation and tree dump actions
  - #60614 a `reduce_motion` setting that is *not* read from the system. It closed #48577, which would have read it, without merging it.
  - #61926 accessibility identifiers
  - #64143 releasing the adapter when a window is dropped
- **Open upstream PRs, not in any gpui-pre:**
  - #57579: Full Keyboard Access swallows Space.
  - #65186: replace `SubclassingAdapter` to fix a crash when closing windows on Touch Bar Macs.
  - #65740: roll back a11y nodes when a prepaint transaction fails. Issue #65623 is a "Duplicate a11y node id" panic on `List` autoscroll.
  - #66631: Windows Narrator focus.
- **Upstream has no editor text-area accessibility.** Zed's `crates/editor/src/element.rs` has no `Role::`, `accesskit` or `set_text_selection`, and no PR adds them. There is no upstream work on announcements or live regions either. **Upgrading gpui-pre brings nothing for the editor.**
- **AccessKit upstream:**
  - accesskit 0.25.2, accesskit_macos 0.27.2, accesskit_unix 0.24.1 and accesskit_consumer 0.39.2 all came out 2026-10-25.
  - accesskit_macos 0.27.0 adds "live updates for non-label nodes" (#782). An announcement then uses the node's computed label, not only `value`.
  - accesskit_macos 0.27.2 drops `SubclassingAdapter` when it detects KVO (#835), which is the Touch Bar class of bug.
  - accesskit 0.25.0 is **breaking**: the consumer moves from `Node`/`NodeId` to `NodeRef`/`FullNodeId`. Taking it means patching gpui-pre and gpui-pre-macos, because no gpui-pre has moved.
  - `accessibilityVisibleCharacterRange` is in neither 0.26.3 nor 0.27.2.
- **Licences:**
  - Every gpui-pre crate is `Apache-2.0`.
  - accesskit, accesskit_macos, accesskit_unix and accesskit_consumer are `MIT OR Apache-2.0`. AccessKit also carries Chromium-derived code under a BSD-style licence (`LICENSE.chromium`).
  - All of these are Apache-2.0-compatible, so vendoring any of them under `vendor/` is allowed if the licence files are kept.

**Upgrade cost:**

- A gpui-pre bump (to 0.3.11 or later) is independent of a11y work, and costs what bumps always cost: API churn in the app, with the toolchain bumped alongside.
- An AccessKit 0.25/0.27 upgrade is the expensive one. It is a vendor-and-patch of gpui-pre's a11y glue, which is option (b).

### Spike 1: the editor text model in accesskit_consumer (offline, not in the repo)

**What it is:**

- A throwaway crate in the session scratchpad depending on `accesskit =0.24.1` and `accesskit_consumer =0.38.0`. No repo file changed.
- It builds the proposed editor tree: a `Window` root holding one `MultilineTextInput` with one `TextRun` per display row.
  - Each run gets `character_lengths` set.
  - A buffer line's last segment ends with `\n`.
  - The caret is a collapsed `TextSelection`.
- It loads the tree in `accesskit_consumer::Tree` and asks the questions that accesskit_macos 0.26.3 forwards from NSAccessibility (section 4).
- It times the build, the first load, a one-keystroke update and a one-row scroll update, for windows of 60 to 50,000 rows.

**To reproduce:** outside the repo, make a crate with the manifest and
`src/main.rs` below, then run it twice: `cargo +1.98.1 run --release` prints
the semantics with wrap segments unlinked, and `cargo +1.98.1 run --release --
link` links them with `next_on_line`. Both print the timing table rows. The
numbers below are the fastest of repeated runs of each, release profile, on the
Linux x86-64 lane machine while other lanes were building, so they are upper
bounds; only their growth with the row count matters.

`Cargo.toml`:

```toml
[package]
name = "a11y-spike"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
accesskit = "=0.24.1"
accesskit_consumer = "=0.38.0"

[profile.release]
debug = false
```

<details><summary><code>src/main.rs</code> (127 lines): the fixture rows, the tree builder, the queries and the timings</summary>

```rust
// Throwaway spike for T294. Not part of the repo.
// Models the proposed editor text area: one MultilineTextInput with one TextRun per
// display row; soft-wrap segments of one buffer line are linked with next_on_line,
// and a buffer line's last segment ends in '\n'. Then asks accesskit_consumer the
// questions accesskit_macos 0.26.3 forwards from NSAccessibility.
use accesskit::{Node, NodeId, Role, TextPosition, TextSelection, Tree, TreeId, TreeUpdate};
use accesskit_consumer::{TreeChangeHandler as ChangeHandler, Tree as CTree};
use std::time::Instant;

struct Count(usize, usize, usize);
impl ChangeHandler for Count {
    fn node_added(&mut self, _: &accesskit_consumer::Node) { self.0 += 1 }
    fn node_updated(&mut self, _: &accesskit_consumer::Node, _: &accesskit_consumer::Node) { self.1 += 1 }
    fn focus_moved(&mut self, _: Option<&accesskit_consumer::Node>, _: Option<&accesskit_consumer::Node>) {}
    fn node_removed(&mut self, _: &accesskit_consumer::Node) { self.2 += 1 }
}

static LINK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const ROOT: NodeId = NodeId(0);
const EDITOR: NodeId = NodeId(1);
fn run_id(row: usize, seg: usize) -> NodeId { NodeId(1_000_000 + (row as u64) * 16 + seg as u64) }

/// rows: (buffer row, segments). Caret at (first row, char 3).
fn build(rows: &[(usize, Vec<String>)], caret_row: usize, caret_char: usize) -> TreeUpdate {
    let mut nodes = Vec::new();
    let mut children = Vec::new();
    for (row, segs) in rows {
        for (i, seg) in segs.iter().enumerate() {
            let mut run = Node::new(Role::TextRun);
            let mut text = seg.clone();
            let last = i + 1 == segs.len();
            if last { text.push('\n'); }
            run.set_character_lengths(text.chars().map(|c| c.len_utf8() as u8).collect::<Vec<_>>());
            run.set_value(text);
            let link = LINK.load(std::sync::atomic::Ordering::SeqCst);
            if link && !last { run.set_next_on_line(run_id(*row, i + 1)); }
            if link && i > 0 { run.set_previous_on_line(run_id(*row, i - 1)); }
            nodes.push((run_id(*row, i), run));
            children.push(run_id(*row, i));
        }
    }
    let mut editor = Node::new(Role::MultilineTextInput);
    editor.set_children(children);
    let caret = TextPosition { node: run_id(caret_row, 0), character_index: caret_char };
    editor.set_text_selection(TextSelection { anchor: caret, focus: caret });
    nodes.push((EDITOR, editor));
    let mut root = Node::new(Role::Window);
    root.set_children(vec![EDITOR]);
    nodes.push((ROOT, root));
    TreeUpdate { nodes, tree: Some(Tree::new(ROOT)), tree_id: TreeId::ROOT, focus: EDITOR }
}

fn file(lines: usize) -> Vec<(usize, Vec<String>)> {
    (0..lines).map(|r| (r, vec![format!("    let value_{r} = compute(alpha, beta, gamma); // é")])).collect()
}

fn main() {
    let link = std::env::args().nth(1).as_deref() == Some("link");
    LINK.store(link, std::sync::atomic::Ordering::SeqCst);
    println!("wrap segments linked with next_on_line: {link}");
    // 1. Semantics: 3 buffer lines, the middle one soft-wrapped into 2 segments.
    let rows = vec![
        (10, vec!["fn main() {".to_string()]),
        (11, vec!["    let a = ".to_string(), "1;".to_string()]),
        (12, vec!["}".to_string()]),
    ];
    let tree = CTree::new(build(&rows, 11, 4), true);
    let st = tree.state();
    let ed = st.focus().unwrap();
    println!("supports_text_ranges={}", ed.supports_text_ranges());
    println!("value()={:?}  (accessibilityValue for a multiline input)", ed.value());
    println!("numberOfCharacters={}", ed.document_range().end().to_global_utf16_index());
    let sel = ed.text_selection().unwrap();
    println!("selection start utf16={}", sel.start().to_global_utf16_index());
    println!("insertionPointLineNumber={}", ed.text_selection_focus().unwrap().to_line_index());
    for i in 0..4 {
        match ed.line_range_from_index(i) {
            Some(r) => println!("rangeForLine({i}) = {:?}", r.text()),
            None => println!("rangeForLine({i}) = None"),
        }
    }
    for idx in [0usize, 12, 24, 27] {
        let p = ed.text_position_from_global_utf16_index(idx).unwrap();
        println!("lineForIndex({idx}) = {}", p.to_line_index());
    }

    // 2. Cost: build + first update + one-keystroke update, for window sizes.
    for &n in &[60usize, 180, 300, 600, 2_000, 10_000, 50_000] {
        let rows = file(n);
        let t0 = Instant::now();
        let up = build(&rows, n / 2, 3);
        let t_build = t0.elapsed();
        let t1 = Instant::now();
        let mut tree = CTree::new(up, true);
        let t_init = t1.elapsed();
        // keystroke: one row's text changes, caret moves one char.
        let mut rows2 = rows.clone();
        rows2[n / 2].1[0].insert(4, 'x');
        let t2 = Instant::now();
        let up2 = build(&rows2, n / 2, 4);
        let mut c = Count(0, 0, 0);
        tree.update_and_process_changes(up2, &mut c);
        let t_update = t2.elapsed();
        // scroll by one row: window slides (drop first row, add one at the end)
        let mut rows3 = rows2.clone();
        rows3.remove(0);
        rows3.push((n, vec!["new row".into()]));
        let up3 = build(&rows3, n / 2, 4);
        let t3 = Instant::now();
        let mut c3 = Count(0, 0, 0);
        tree.update_and_process_changes(up3, &mut c3);
        let t_scroll = t3.elapsed();
        // queries macOS would make after a caret move
        let st = tree.state();
        let ed = st.focus().unwrap();
        let t4 = Instant::now();
        let line = ed.text_selection_focus().unwrap().to_line_index();
        let r = ed.line_range_from_index(line).unwrap();
        let _ = r.text();
        let nchars = ed.document_range().end().to_global_utf16_index();
        let t_query = t4.elapsed();
        println!(
            "rows={n:>6} build={:>8.2?} init={:>8.2?} keystroke-update={:>8.2?} (upd {}) scroll-update={:>8.2?} (+{} -{}) line-query={:>8.2?} line={line} chars={nchars}",
            t_build, t_init, t_update, c.1, t_scroll, c3.0, c3.2, t_query
        );
    }
}
```

</details>

**What it showed:**

- `supports_text_ranges` is true once there is at least one `TextRun`.
- **`value()` is `None` for a multiline input.** So `accessibilityValue` is nil unless the node is given an explicit value (`accesskit_consumer/src/node.rs:823–833`).
- **Lines are visual.**
  - With wrap segments as separate, unlinked runs, a wrapped line counts as two lines. `rangeForLine(1)` is `"    let a = "` and `rangeForLine(2)` is `"1;\n"`.
  - If the segments are linked with `next_on_line`, they count as one line, `"    let a = 1;\n"`. But `next_on_line` means "same visual line", which makes the bounds wrong. See section 4.
- Line and index answers are window-relative. `numberOfCharacters` is the UTF-16 length of the exposed text, nothing more.
- Consumer cost (release build, loaded machine, same session, fastest run, rounded):

| Rows exposed | Build update | Keystroke update | Scroll update | Line query at the caret |
|---:|---:|---:|---:|---:|
| 60 | 0.03 ms | 0.06 ms | 0.03 ms | 0.02 ms |
| 180 | 0.08 ms | 0.12 ms | 0.08 ms | 0.05 ms |
| 300 | 0.12 ms | 0.20 ms | 0.13 ms | 0.2–3 ms (noisy) |
| 600 | 0.27 ms | 0.46 ms | 0.44 ms | 0.20 ms |
| 2,000 | 1.0 ms | 2.5 ms | 2.9 ms | 1.2 ms |
| 10,000 | 15 ms | 14–34 ms | 19–25 ms | 9–11 ms |
| 50,000 | 63–118 ms | 135–202 ms | 133–150 ms | 65–113 ms |

  - A line query walks every run from the start (`text.rs:259–279, 281–293`), so it is linear in the rows exposed.
  - From about 2,000 rows up, exposing the whole file misses the editor's 2 ms budget before gpui's own per-frame rebuild is even counted.

### Spike 2: what Linux AT-SPI sees of the running app

**What it is:**

- No code change. Today's `tenmoku` debug build ran under Xvfb inside a private D-Bus session with the AT-SPI bus up.
- `accesskit_unix` activates when `org.a11y.Status.IsEnabled` turns true (`accesskit_unix/src/context.rs:184–200`).
- Busctl then walked `org.a11y.atspi.Accessible` from the registry root, reading `GetChildren`, `Name` and `GetLocalizedRoleName`.

**To reproduce:**

```
dbus-run-session -- bash -c '
  /usr/libexec/at-spi-bus-launcher --launch-immediately &
  sleep 1
  busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true
  A=$(busctl --user call org.a11y.Bus /org/a11y/bus org.a11y.Bus GetAddress | cut -d\" -f2)
  Xvfb :93 -screen 0 1280x800x24 &
  sleep 1
  DISPLAY=:93 XDG_CONFIG_HOME=/tmp/a11y/config XDG_STATE_HOME=/tmp/a11y/state \
    RUST_LOG=info target/debug/tenmoku /tmp/a11y/demo/src/main.rs &
  sleep 12
  busctl --address="$A" call org.a11y.atspi.Registry /org/a11y/atspi/accessible/root \
    org.a11y.atspi.Accessible GetChildren
  # then GetChildren and get-property Name, recursively, on the app bus name it returns
'
```

**What it showed:**

- **The tree reaches AT-SPI on a running app.**
  - The app log says `Accessibility activated`.
  - With a fresh config, the setup wizard's nodes appear under the window `main.rs — Tenmoku` with their labels: "Set Up Tenmoku, step 1 of 5, Appearance", "Mode" › "Dark"/"Light"/"Match System", "Dark theme" › the theme names, "Code sample", "Skip setup", "Continue".
  - So the setup wizard's roles and labels work end to end.
- **The workbench baseline.** After Escape (skip setup) and typing "hello" in the editor, the tree is **only** the window node, `main.rs ● — Tenmoku`, with no children. This is the audit's baseline, now measured on a running app rather than read from the source.
- **The editor isn't exposed.** gpui logs `a11y: focused element (FocusId(5v1)) has no accessibility node (it has an id but no role)`. That is the editor's `div().id("editor")` (`view.rs:1929`).
- **There is a Linux end-to-end path.** An AT-SPI probe under Xvfb can read the live tree with no code change and no human. Section 9 uses it.

**Not seen:** anything VoiceOver-specific. accesskit_unix and accesskit_macos
share accesskit_consumer, but not the platform mapping, so macOS behaviour
still needs the human checks (section 12).

**Decision:** build on gpui-pre 0.3.9's AccessKit as shipped. Upgrading
gpui-pre brings no editor or announcement work, and an AccessKit 0.25 upgrade
is a patch job that the first slice doesn't need.

## 3. Platform path options

| | (a) gpui-pre's AccessKit as shipped | (b) vendor and patch | (c) direct NSAccessibility for the editor | (d) wait for upstream |
|---|---|---|---|---|
| What | Div builders and `a11y_synthetic_children` for everything, including the editor's text runs | `vendor/accesskit_macos` (2.6k lines) and/or `vendor/gpui-pre*` under `[patch.crates-io]`, with a README of patches as `vendor/tree-sitter/README.md` has | Override `accessibility*` on GPUI's NSView for the editor only, bypassing AccessKit | Ship chrome only; take the editor when Zed does |
| Effort | The first slice is 1 xs, 3 s, 8 m and 1 l of implementation (section 11), plus a human pass | (a) plus 1–3 days per patch, plus a re-diff on every gpui-pre bump | (a) for chrome, plus an objc2 text-area implementation (about l–xl). It has to coexist with `SubclassingAdapter`, which already subclasses that view and answers its accessibility methods | Nothing now, and an unknown wait: upstream has no editor a11y PR or plan |
| Risks | Gaps found under VoiceOver (section 4) can't be fixed from the app | Patch drift. gpui-pre ships weekly, so vendoring gpui-pre itself means re-applying patches each bump. accesskit_macos alone is small and moves slowly | Two accessibility implementations on one view. The SubclassingAdapter's subclass wins or fights. Mac-only, so Linux gets nothing | No editor for the first release, which is the main surface |
| Upgrade cost | A gpui-pre bump is a normal bump | Each bump re-checks the patches. Taking accesskit 0.25 means patching gpui-pre's glue (`window/a11y.rs`, macOS and Linux windows) | Breaks whenever gpui-pre-macos changes its view class | — |
| Linux impact | The same kiln code drives `accesskit_unix` (AT-SPI) for the neon port | Linux unaffected unless accesskit_unix is also patched | None. Linux would need its own path | — |

**Decision: (a).**

- Build everything on gpui-pre 0.3.9's AccessKit, through div builders, `a11y_synthetic_children` closures and the active-descendant pattern. Section 4 shows how the editor works within (a)'s limits.
- Keep (b) as the fallback, scoped to **`vendor/accesskit_macos` only**. It's small, slow-moving and dual-licensed, and patching it doesn't touch gpui-pre.
- Reject (c): it fights the adapter that already owns the view, and gives Linux nothing.
- Reject (d): upstream has no editor plan.

**Switch to (b), vendoring accesskit_macos 0.26.3, if any of these turns up:**

1. **Silent arrow keys** (check H5). VoiceOver doesn't speak the line or character after arrow keys in the editor.
   - Cause: accesskit_macos posts `NSAccessibilitySelectedTextChangedNotification` without `AXTextStateChangeType` or direction user info (`event.rs:287–297`).
   - Patch: add that user info.
2. **No visible range** (check H6). VoiceOver needs `accessibilityVisibleCharacterRange` for the editor to make sense, for example for "read visible text" or VO-cursor tracking. Patch: implement it from the runs' bounds against the parent's bounds.
3. **Announcement priority** (check H7). Announcements need a priority other than Medium or High, or the label-based live updates from 0.27 (#782). Patch: backport.
4. **Touch Bar crash.** The crash on closing windows from upstream PR #65186 reproduces on a Touch Bar Mac. Patch: backport accesskit #835.

Vendor gpui-pre-macos only if both of these hold: the high-contrast appearance
mapping (section 8) can't be worked around from kiln-platform, and a
test-support activation hook is still wanted after section 9's approach.
Revisit (a) versus an AccessKit 0.25 upgrade when a gpui-pre release moves
to accesskit 0.25. Each switch is its own card, and none is in the first
slice.

## 4. Editor text area

### Where the node lives

- The editor's focus div (`view.rs:1929`, `div().id("editor")…track_focus`) gets `role(Role::Group)` and the label "Editor".
- Inside it, a new wrapper div sits around `EditorElement` only, sized like the element (`size_full`). The element already fills its parent, `element.rs:704`.
  - The wrapper has `.id("editor-text")`, `.role(Role::MultilineTextInput)` and `.aria_label("<file name>")`.
  - It has `.aria_active_descendant()` whenever the editor is focused, **including while the completion menu is open**. Nothing else in the editor is a focus target until A11Y-13 makes the completion list one (section 6), so in the first slice focus never falls back to the "Editor" group.
  - It has `.a11y_synthetic_children(closure)`.
- The closure reads an `Rc<RefCell<Option<A11yText>>>`, which `EditorElement::prepaint` (`element.rs:717`) fills only when `window.is_a11y_active()`.
  - The closure runs after the wrapper's prepaint, so after the element's (`element.rs:476–497`). The data is always from this frame.
- Why the wrapper:
  - A custom element can't be focused or be an active descendant (section 2).
  - Keeping the popovers out of the text node keeps `MultilineTextInput`'s children pure text runs (`accesskit_consumer/src/text.rs:932–938` filters them anyway).
  - Later, the completion list can become the active descendant instead, because it is under the focused div. That is A11Y-13 (section 6); until then the text area keeps it, and A11Y-9 announces the suggestions.
- `A11yText` holds:
  - the run nodes, keyed by `(buffer_row, wrap_segment)`;
  - the selection as `(run key, character index)` pairs, **always naming runs the builder emitted** (see "Selection projection" below);
  - a run-to-buffer-offset table for `SetTextSelection`.
- A pure builder makes it: `crates/fe-gpui/src/element/a11y_text.rs`, declared from `element.rs`. The file viewer reuses it.

### The NSAccessibility text methods and what accesskit_macos 0.26.3 answers

All the text selectors below are allowed only when the node "supports text
ranges" (`node.rs:1222–1237`):

- It must be a text input, `Label`, `Document` or `Terminal`.
- It must have at least one `TextRun` child (`accesskit_consumer/src/text.rs:1402–1406`).
- So **an empty file still gets one empty run.**
- `MultilineTextInput` maps to `NSAccessibilityTextAreaRole` (`node.rs:84`).

| NSAccessibility | Answered? | Where (accesskit_macos 0.26.3 `src/node.rs`, then consumer) | What our tree makes it return |
|---|---|---|---|
| `accessibilityValue` | Yes, but **nil for multiline** unless `value` is set | `value` :536 → `NodeWrapper::value` :322–339 → consumer `Node::value` `node.rs:823–833` (computes text only when `!is_multiline()`) | Nil. **Decision:** don't set `value`; it would copy the window's text every frame. If H4 shows VoiceOver needs it, set it to the exposed text, which the cap keeps bounded |
| `accessibilityNumberOfCharacters` | Yes | :730–740 → `document_range().end().to_global_utf16_index()` (consumer `text.rs:1438, 259`) | The UTF-16 length of the **exposed window**, not the file |
| `accessibilitySelectedTextRange` | Yes | :756–767 → `text_selection()` (`text.rs:1448–1458`) → `to_ns_range`. `clamped_upgrade` (`text.rs:37–53`) clamps only the character index within a run that exists; an endpoint naming a missing node or a non-`TextRun` returns `None`, and `text_selection` **unwraps it and panics** | The primary selection **projected onto the window** (see "Selection projection"), as UTF-16 offsets into the window |
| `accessibilitySelectedText` | Yes | :742–754 | The text of the primary selection |
| `accessibilityInsertionPointLineNumber` | Yes | :769–780 → `text_selection_focus().to_line_index()` (`text.rs:281–293`) | The caret's **visual line within the window**, zero-based |
| `accessibilityLineForIndex:` | Yes | :960–971 → `text_position_from_global_utf16_index` (`text.rs:1610`) → `to_line_index` | The window line. Linear in the runs before the index |
| `accessibilityRangeForLine:` | Yes | :782–793 → `line_range_from_index` (`text.rs:1548–1568`) | The visual row's text, with its `\n` on a buffer line's last segment. Past the end it returns `{0,0}` |
| `accessibilityStringForRange:` | Yes | :815–827 → `from_ns_range` → `Range::text` | The window's text |
| `accessibilityAttributedStringForRange:` | Yes | :829–933: per run, foreground and background colour, font family, size, bold, italic, underline, strikethrough, language and alignment | Plain text. **Decision:** set no style attributes in the first slice. Splitting runs at syntax or diagnostic boundaries multiplies nodes |
| `accessibilityFrameForRange:` (bounds for range) | Yes | :935–958 → `Range::bounding_boxes` (`text.rs:740–765`) | Boxes only if **every** run the range touches has `bounds`, `character_positions`, `character_widths` and a text direction; one missing makes the whole result empty, and accesskit_macos then returns `NSRect::ZERO`. There is no row-box fallback. **Decision:** every run gets all four (see "Geometry" below) |
| `accessibilityRangeForPosition:` | Yes | :795–813 → `text_position_at_point` (`text.rs:1485`) | Hit-tests the runs' bounds and character positions (VoiceOver cursor routing, the pointer). Margin runs lie outside the view, so points inside it only ever hit visible rows |
| `accessibilityRangeForIndex:` | Yes | :973–984 | One character |
| `accessibilityStyleRangeForIndex:` | Yes | :986–1004 | A whole run, since runs carry no style |
| `setAccessibilitySelectedTextRange:` | Forwarded as `Action::SetTextSelection` | :1006–1022 | **Decision:** handle it with `.on_a11y_action(Action::SetTextSelection, …)` on the wrapper. Map the run keys to buffer offsets and set the primary selection; that is VoiceOver moving the caret |
| `setAccessibilityValue:` | Forwarded as `Action::SetValue` | :555–580; allowed when the node supports text ranges and isn't read-only (:1238–1239) | **Decision:** no handler, so the request is dropped. Replacing the whole text from an assistive tool is out of scope |
| `accessibilityVisibleCharacterRange` | **No** | Not in `node.rs`, not in `isAccessibilitySelectorAllowed:` (:1207–1290), and not in 0.27.2 either | VoiceOver can't ask which part is on screen. **Decision:** accept this for the first slice; it is trigger 2 in section 3 |

### Decisions

**How much text: viewport ± one viewport, capped.**

- The window is the frame's display rows, plus as many rows again above and below. That is 3 × the visible rows.
- It is clipped to the file and capped at **300 display rows and 64 KiB of UTF-8**. **Choosing** rows is by priority: the visible rows first and always (truncated if one alone passes the byte cap), then margin rows alternately above and below until either cap is reached. **Emitting** is by position: the chosen runs are pushed in ascending display-row order (and segment order within a row), because `A11ySubtreeBuilder::push_child` keeps emission order and AccessKit reads the document in child order, with no sort by key. A consumer test with margins on both sides checks that `document_range().text()` and `line_range_from_index` follow the buffer's order.
- It is centred on the viewport, not the caret. When the caret is off screen, VoiceOver still reads what the user sees.
- Visible rows come from `Frame.rows` (`fe-display/src/layout.rs:158, 249`). Margin rows come from `DisplayMap::row(r)` (`display_map.rs:320`) and `Text::chunks_in_range` (`fe-text/src/lib.rs:323`), unshaped.
- Rationale: spike 1 puts 300 rows at about 0.2 ms of consumer work per keystroke. The whole file is out: 2,000 rows already cost 2.5 ms, past the editor's 2 ms budget.

**Selection projection: every endpoint names an emitted run.**

The consumer panics on an endpoint that names no run (table above), and
the window can hold a fraction of the selection, or none of it. So the
builder projects the primary selection onto the runs it emitted, for both
the editor and the file viewer, by these rules, in order:

1. **Hidden by a fold.** An endpoint inside a folded range moves to the end of the fold header's run, before its `\n`.
2. **Past a truncated row.** A display row longer than the remaining byte budget is cut at a character boundary (see the cap below), and its run ends there. An endpoint past the cut moves to the run's end.
3. **Above the window.** An endpoint before the first emitted run moves to that run's start, character 0.
4. **Below the window.** An endpoint after the last emitted run moves to that run's end.
5. **Otherwise** it maps to its `(buffer_row, segment)` run and the character index within that segment.

Consequences:

- A collapsed caret off screen (the user scrolled away with the mouse) becomes a collapsed caret at the nearer window edge. VoiceOver reads the window, which is the screen; the caret line returns once the caret moves and the editor scrolls back to it.
- **Select All** on a 10,000-line file becomes the whole window, and `accessibilitySelectedText` returns the window's text, not the file's. When the projection clips a non-empty selection (rules 2–4 moved an endpoint of a non-collapsed selection), A11Y-9 announces "N lines selected" once per selection change, from the model's real selection.
- The anchor and focus keep their order, so a backwards selection stays backwards.
- An empty file has one empty run, so there is always a run to name.

The builder's tests (section 9, shape 1) load the projected tree in
`accesskit_consumer::Tree` and call `text_selection()`,
`text_selection_focus()` and `Range::text()` for: Select All over the cap,
a selection starting above and ending below the window, a caret scrolled
off each edge, an endpoint in a fold, an endpoint past a truncated 100 KiB
row, the empty file, and the viewer's byte-offset selection over invalid
bytes. None may panic, and each must return the projected range above.

**Geometry: all four properties on every run.**

- Every run sets `text_direction(LeftToRight)`. The editor and the viewer lay out left to right; they have no bidi.
- **Visible rows** take `bounds` from the row's rect in window coordinates, and `character_positions`/`character_widths` from `RowLayout::x_for_offset` (`fe-display/src/layout.rs:326`), one entry per character, so inlay hints add nothing.
- **Margin rows** (above or below the viewport, never shaped) take `bounds` where the row would be drawn: the row's offset from the viewport's first row times the line height, so above or below the visible rect. Positions and widths come from the same `CharWidth` measurer that soft wrap already uses (`fe-display/src/wrap_map.rs:794`, `char_width`/`cluster_width`), summed along the row, so no shaping happens. It ignores kerning and ligatures, so it can be a little off from the shaped row; that matters only once the row scrolls on screen, when it becomes a visible row with shaped positions.
- **One geometry entry per character, line breaks included.** `character_positions` and `character_widths` always have exactly as many entries as `character_lengths`; the builder asserts it. A buffer line's last segment ends in its line break, which is one character: `\n`, or `\r\n` as a single two-byte character so the caret can't land between them. Its entry is a zero-width position at the end of the row's text (x = 0 on a blank line). This matters because a range that starts or ends inside a run indexes `positions[start_index]` (`accesskit_consumer/src/text.rs:778–787`); a collapsed caret on a blank line's break with empty arrays would panic.
- **Genuinely empty runs** have no characters at all: the empty file's single run, and an empty last line with no break after it. They set empty `character_lengths`, positions and widths, and a zero-width rect at the row's start. The consumer then takes the whole-run path and indexes nothing (`text.rs:778`), so a degenerate range still gets a box.
- Tests call `bounding_boxes` on a **collapsed caret** range, not only whole runs, at: a character mid-row, the end of a row before its break, an LF blank line, a CRLF blank line, the empty file, and an empty last line, for both the editor and the viewer. None may panic, and each returns one box.
- Tests also assert `Range::bounding_boxes` is non-empty for a visible row, a margin row and a range spanning both, and that `text_position_at_point` at a visible character's centre returns that character.

**What VoiceOver can't reach.**

- To VoiceOver the window is the whole document. `numberOfCharacters`, line numbers and `rangeForLine` are all window-relative, so VoiceOver can't ask for a line outside it.
- The user reaches other lines the normal way. The arrow keys, `j`/`k` or Page Down move the caret, and the editor scrolls to keep the caret visible. The next frame rebuilds the window around the new viewport, and the selection change makes VoiceOver read the new caret line.
- VoiceOver's own text navigation stops at the window's edges. VO-A ("read all") reads to the end of the window, at most one page past the screen.
- **Open question Q1:** should a `SetTextSelection` or VO read that reaches the window edge scroll the editor, so reading can continue? It's not in the first slice; check H6 decides whether it's needed.

**Line numbers.**

- `InsertionPointLineNumber` is window-relative, so the real position comes from two places:
  - the status bar's "Ln N, Col M" item (`pane_item.rs:1624`), exposed as a labelled `Status` child;
  - a new palette command, "Editor: Announce Position", which posts "line N, column M of T" through the announcer (section 5).
- No new key binding, to avoid binding-count tests.

**Run identity.**

- Run ids are `synthetic_node_id((buffer_row, segment))`. A scroll then adds and removes rows at the edges, and doesn't rewrite every run (spike 1: one added, one removed).
- An inserted line shifts the keys of the rows below it, so their values all change. The cap bounds that.

**Folds, wrap, inlays, ghost text, sticky scroll and the minimap:**

| Feature | Decision |
|---|---|
| Folds | Hidden rows aren't display rows, so they aren't in the tree. The fold header's run holds its buffer text **without** the `⋯` marker (`FOLD_MARKER`, `layout.rs:33`), so character indexes map 1:1 to buffer characters. Fold and unfold commands announce "Folded N lines" or "Unfolded" |
| Soft wrap | One unlinked run per wrap segment, so VoiceOver lines are visual rows, as in TextEdit with wrapping on. Only a buffer line's last segment ends in `\n`. This matches the caret, which moves by display row |
| Inlay hints | Not in the runs. Their glyphs are in `RowLayout.line` and `ExpandedRow::hint_ranges` (`fe-display/src/lib.rs:183`), so x positions go through `x_for_offset`, never glyph indexes. Exposing them, for example in the description, is a later card |
| Ghost text (inline completion) | Not in the runs. When a suggestion first appears, the announcer says "Suggestion available" once, politely; there is no announcement when it changes. Its text isn't spoken in the first slice |
| Sticky scroll | Hidden. Its headers repeat rows above the viewport and paint over rows that are still in `Frame.rows` (`element.rs:1815`). Rows under the headers stay in the tree, since they are the real text |
| Minimap, code lens, git gutter, indent guides, rulers, blocks (proposals, lens rows) | Hidden: no role. Blocks are screen rows outside the DisplayMap (`block_map.rs`), so they never become runs |

**Multi-cursor.**

- AccessKit has one `TextSelection`, so the tree exposes only the primary selection (`SelectionSet::primary_in`, `fe-buffer/src/selection.rs:150`), projected as above.
- When the selection count goes from one to more, or back, the announcer says "N cursors" or "One cursor".

**Vim normal mode.**

- The model's selection is an empty point at the head (`fe-vim/src/motion.rs:108–109`). The block caret is drawn over the grapheme after it (`caret_shape`, `fe-gpui/src/vim.rs:1741`).
- It is exposed as a collapsed selection at the head. VoiceOver speaks the character after the insertion point, which is the one under the block.
- In visual modes the model selection already includes the cursor's grapheme, and is exposed as it is.
- In normal, visual and operator-pending modes the text node is **`read_only`**, so VoiceOver doesn't treat the command keys as typing. Insert and replace modes clear it, as does running with vim off. This follows `accepts_text()` (`vim.rs:1393`).
- Mode changes are announced (section 5).
- Check H5 verifies the speech.

**Typing echo and caret-move speech.**

- Neither has its own API. Both follow from tree diffs.
- A changed run value posts `ValueChanged` on the text node. It is ordered before the selection change (`event.rs:174–191, 243–245`).
- A changed `TextSelection` posts `SelectedTextChanged` (`event.rs:287–297`).
- VoiceOver's typing echo comes from its own key echo. Its line and character reading comes from the selection change.
- Neither notification carries `AXTextStateChangeType` user info, so H5 is the gate (section 3, trigger 1).

**Diagnostics at the caret.**

- When the caret settles in a diagnostic's range for 300 ms (the hover delay, `HOVER_DELAY`), the announcer reads "error: <message>" politely.
- This happens once per diagnostic per visit. It uses the hover filter's containment logic (`Under::items`, `fe-gpui/src/hover.rs:195`) and `diagnostic_text` (:213).
- The next and previous diagnostic commands announce the same way, with no delay.
- Underline attributes aren't set (see the attributed string row).

**Read-only file viewer** (`crates/fe-gpui/src/file_viewer/element.rs:183`):

- It uses the same builder, cap, selection projection and geometry, with `read_only` always on.
- It has no wrap or folds (module doc :1–4) and a single selection.
- Invalid bytes are already `?` in `FileText` (`fe-fileview/src/source.rs:206`). Offsets are file bytes, read through `TextSource::chunks_in_range` (`fe-text/src/source.rs:48`).

**`TextLine` and `TextInput` as the template.**

- The same shape, smaller: a single-line `TextInput` role with one run.
- `TextInput` is drawn through a `canvas` (`text_input.rs:831`). It is the widely used one: find bar, pickers, settings, the vim line and rename. `TextField`/`TextLine` (`text_field.rs:677`) is used by chat and the project panel filter.
- Both get the role on their focus div, which needs an `.id()`. The run and selection are built from `TextEdit` state (`shown_text`, `selected_range`, `caret`), which is masked-aware, so password fields never expose their text.
- They use `Role::PasswordInput` when masked and `Role::SearchInput` for find and picker queries.

**Performance.**

- All building is gated on `window.is_a11y_active()` (`window.rs:7006`). With no assistive technology the cost is one bool per frame.
- On macOS the flag stays on once VoiceOver has looked at the window (section 2).
- **Budget:** at most **0.5 ms p99 per keystroke** for the builder plus the consumer update at the cap, on the Mac. That leaves the editor's own stages inside PERF.md's 2 ms p99 (`PERF.md:1258–1259`; for example `type/100-carets` measures 1.16 ms p50 and 1.46 ms p99 for the editor stages, `PERF.md:1256`) and keystroke-to-paint inside 4 ms p50 and 8 ms p99 (`PERF.md:1020`).
- The headless keystroke bench can't activate a11y (section 9). So the card adds a bench, `crates/fe-gpui/benches/a11y_text.rs`, that times the pure builder and an `accesskit_consumer::Tree` update for a 10k-line Rust file at the cap.

**Decision:**

- The editor is a `MultilineTextInput` wrapper with visual-row runs, covering the viewport ± one viewport, capped at 300 rows and 64 KiB.
- It exposes the primary selection only, projected so every endpoint names an emitted run; every run carries bounds, positions, widths and direction.
- The text area stays the active descendant while completion is open, until A11Y-13.
- Inlays, ghost text, sticky headers, the minimap and blocks are hidden.
- The text node is read-only outside vim insert mode.
- **Open questions:** Q1 (scroll at the window edge), and H4–H6 (whether VoiceOver needs `value`, the visible range, or richer selection notifications).

## 5. Announcements

**Does accesskit_macos 0.26.3 turn `Live` into
`NSAccessibilityAnnouncementRequested`? Yes, for nodes that have a `value`.**

- `QueuedEvent::live_region_announcement` takes `node.value().unwrap()` (`event.rs:35–44`) and posts `NSAccessibilityAnnouncementRequestedNotification` on the window, with the text and a priority (`event.rs:65–101`):
  - `Live::Assertive` gets High priority;
  - any other value gets Medium.
- Announcements fire in two cases:
  - when a live node with a value is added (`event.rs:236–239`);
  - when its value or `live` changes, or it re-enters the filter (`event.rs:297–306`).
- A live node without a value says nothing in 0.26.3. 0.27 adds label-based live updates.

**The workaround is no workaround: gpui just lacks a builder.**

- gpui-pre has no `aria_live`, but `.aria_value(text)` plus `.a11y_synthetic_children(|b| b.parent_node().set_live(Live::Polite))` on a div with an id and a role is enough (section 2). No custom element is needed.
- To speak the same text twice, the node needs a new id, for example a per-message sequence key, so the add path fires.

**Design: one `Announcer` per window, in kiln-ui (card A11Y-2).**

- It is a zero-size `Role::Status` div (`AXApplicationStatus`, `accesskit_macos/src/node.rs:273`), mounted as the last child of the window's root element. Its value is the latest message, and its `live` is Polite or Assertive.
- **Where it mounts.** Tenmoku's windows are not kiln `Root`s (`crates/fastedit/src/main.rs:380`: "Tenmoku's windows aren't kiln `Root`s"). The workbench draws its own root div in `Workbench::render` (`crates/fe-workbench/src/workbench.rs:962`), around kiln-shell's `Shell` (`kiln/crates/kiln-shell/src/shell.rs:319`). So A11Y-2 mounts the announcer in **`Workbench::render`**, and in `kiln-shell/src/root.rs` too, for other kiln apps such as the gallery. A11Y-2's evidence is the A11Y-1 dump, or the AT-SPI probe, of a running Tenmoku window showing the `Status` node.
- Each message gets a fresh child key, so repeats are spoken.
- API: `announce(cx, text, Politeness)` from any view. It is coalesced to one message per frame, and the last one wins.
- Announcements are made only while a11y is active.

**Sources:**

| What | Politeness | Source | Rule |
|---|---|---|---|
| Toasts | Polite (Assertive for errors) | `crates/fe-workbench/src/toast.rs` `show` :358 / `render_toast` :464 | Title and body when shown. Action buttons are buttons in the toast's `Role::Alert` group |
| Vim mode | Polite | `status::vim_mode_label` (`fe-gpui/src/pane_item.rs:133`) | On change only. "Normal", "Insert" and so on |
| Vim messages and command-line errors | Polite (Assertive for errors) | `VIM_MESSAGE` item (`pane_item.rs:1663`), `EditorView::set_vim_message` (:204); `vim_host::message` (`fe-workbench/src/vim_host.rs:695`) | When set |
| Vim command line | — | `crates/fe-workbench/src/vim_line.rs` (a `TextInput`) | A `TextInput` labelled by its prefix (`prefix` :169), "Command" or "Search forward". Not announced; it takes focus |
| Find match count | Polite | `FindBar::label` (`fe-workbench/src/find_bar.rs:681`), `EditorView::find_count` (`fe-gpui/src/view/find.rs:448`) | "3 of 12" or "No results". Debounced 300 ms after the last query keystroke, and immediately on next and previous |
| Status bar | — | `crates/fe-workbench/src/status_bar.rs`, kiln-shell `status.rs` | **Not live as a whole**: the cursor item changes on every keystroke. Each item is a labelled child of the "Status bar" `Region` landmark (section 6), read on demand |
| Diagnostics at the caret, folds, cursor count, ghost text, "Announce Position" | Polite | section 4 | As section 4 says |

**Decision:**

- accesskit_macos's Live support is enough. Live regions go through one kiln-ui `Announcer` that uses `aria_value` plus `set_live` from a synthetic-children closure.
- Nothing is made live except the sources above.
- **Open question:** H7, whether Medium priority is interrupted by the focus speech that follows a toast's action.

## 6. Chrome semantics

**Rule:**

- Put roles and labels on the shared kiln-ui widgets, so that one card covers every view using them.
- Views that build their own rows get the same helper calls from card A11Y-2's `a11y` module, which generalises the setup wizard's `node()`.

| Surface | Widget (file:symbol) | Role and properties | Used by |
|---|---|---|---|
| Window regions (landmarks) | The render path Tenmoku actually uses: `Shell::render` (`kiln/crates/kiln-shell/src/shell.rs:319`), which lays out the docks, the centre and the status bar, and `kiln-shell/src/dock.rs` for each dock's content; `Workbench::render` (`fe-workbench/src/workbench.rs:962`) wraps the shell. Not kiln `Root`, which Tenmoku doesn't use | Activity rail → `Navigation` "Activity bar". Side panel → `Complementary` labelled by its panel ("Explorer", "Source Control"). Editor area (the shell's centre) → `Main` "Editor". Bottom dock → `Region` "Panel". Status bar → **`Region` "Status bar"**, because `Role::Status` maps to `AXApplicationStatus`, which is no landmark (`accesskit_macos/src/node.rs:273`), while `Region` maps to `AXLandmarkRegion` (:268). `Role::Status` is kept for the announcer. VoiceOver's Landmarks rotor lists the `AXLandmark*` subroles (`node.rs:236–284`) | All windows |
| Rail buttons | `kiln-ui/src/activity_rail.rs:RailButton` :104 | `Button`, label = the panel name, `aria_toggled` for the open one, `aria_keyshortcuts` | kiln-shell dock |
| Icon and toggle buttons | `icon_button.rs:IconButton` :16, `toggle_button.rs:ToggleButton` :14 | `Button` with a **required** label argument (the tooltip text). Toggles set `aria_toggled`, which maps to `AXToggle` (`node.rs:242`) | find bar, search, git, terminal, toasts, panes, diff view |
| Buttons, checkboxes, switches | `controls.rs:Button` :91, `Checkbox` :300, `Switch` :399 | `Button` / `CheckBox` / `Switch`, label from the button text or the caller, toggled state | settings, git, chat, toasts, wizard |
| Tabs | `tab_bar.rs:TabBar` :18, `Tab` :103 | `TabList` (horizontal) and `Tab` with `selected`. Label "name", description "modified, pinned, preview" as applicable. The close button is a `Button` "Close name". accesskit_macos exposes tabs as radio buttons in an `AXTabGroup` (`node.rs:165–166, 1151–1165`) | kiln-shell `pane.rs`, fe-workbench `pane.rs` |
| Project tree | `tree_row.rs:TreeRow` :41 in `project_panel.rs`'s own `uniform_list` | The list → `Tree` "Files". Rows → `TreeItem` with `level`, `expanded`, `selected`, and `position_in_set`/`size_of_set` **from the model** (a `uniform_list` renders only visible rows). Git status letters go in the description ("modified"). The selected row is the active descendant of the focused panel | project, outline and hierarchy panels |
| Palette and pickers | `palette.rs:PaletteFrame` :17, `list.rs:List`/`ListRow` :137/:66, `kiln-shell/src/picker.rs`, `modal.rs:69` | `Dialog` labelled by the picker title. Query → `SearchInput` (card A11Y-5). List → `ListBox`, rows → `ListBoxOption` with `selected`, position and size. **Focus is in the query, a sibling of the rows, so the active descendant can't apply** (`window/a11y.rs:258–272`). Instead the picker announces the selected row's text politely on every selection change. Open question Q2: restructure so the picker container holds focus and rows can be active descendants | every picker, `tab_switcher.rs` |
| In-window prompts | `kiln-ui/src/prompt.rs:Prompt` :43 | `AlertDialog`, label = title, description = message, buttons. **Only on Linux**: macOS uses NSAlert (`native_prompts: mac`, `kiln-platform/src/capabilities.rs:31`; `kiln-shell/src/lib.rs:89–90`) | Linux port (A11Y-19) |
| In-window menu bar | `menu_bar.rs:MenuStrip` :25, `MenuPanel` :137 | `MenuBar`/`Menu`/`MenuItem`. Linux only for the strip, since macOS has native menus. `MenuPanel` is also used in-window by `git_log_menu.rs` and `shortcuts_editor.rs` on every OS | A11Y-19 |
| Settings controls | `fe-workbench/src/settings_editor.rs`, `settings_editor_controls.rs` | Each row is a `Group` labelled by the setting name, with its control labelled the same and description = help text. The search box is a `SearchInput` | Second slice (A11Y-12) |
| Find bar | `fe-workbench/src/find_bar.rs` | `Search` landmark "Find". Query and replace are `SearchInput`/`TextInput`. Case, word and regex are `ToggleButton`s with labels. Next, previous and close are `IconButton`s. The match count is announced (section 5) | — |
| Completion | `fe-gpui/src/completion_menu.rs` (own divs) | **First slice (A11Y-9):** no role; the text area stays the active descendant, so focus and typing echo stay on the text. The announcer says "N suggestions, <first label>" politely when the menu opens, the selected label on each selection change and after the list refilters, nothing on accept (the inserted text is echoed as typing), and nothing on dismiss. **Second slice (A11Y-13):** `ListBox` "Suggestions" with `ListBoxOption` rows (label + detail). It is inside the focused editor div (`view.rs:1950`), so the selected row becomes the active descendant while the menu is open, and the text area takes it back on accept or dismiss | A11Y-9, then A11Y-13 |
| Hover and signature help | `fe-gpui/src/hover.rs`, `signature_help.rs` | `Tooltip` with the text as a `Label`. Mouse hover isn't announced. Showing hover from the keyboard announces its text politely | A11Y-13 |
| Toasts | `fe-workbench/src/toast.rs` | `Alert` group; announced (section 5); buttons labelled | A11Y-9 |
| Setup wizard | `setup_wizard.rs` | Already done (T1339, T1341); human check T1344 | — |

**Native or in-window, on macOS:**

- Native, and so already accessible through AppKit:
  - Open File/Folder uses NSOpenPanel (`cx.prompt_for_paths`, `workbench_files.rs:53`).
  - Save As uses NSSavePanel (`prompt_for_new_path`, :138).
  - Every `window.prompt(...)` is an NSAlert (`gpui-pre-macos window.rs:1728`). That covers close-dirty (`pane.rs:752`), disk sync, LSP consent, git commit and branch, project-panel delete and rename confirmations, chat and AI changes, trust (`kiln-shell trust.rs:417`), plugin prompts and config problems.
- In-window everywhere, so ours to make accessible:
  - the setup wizard
  - every picker and palette (`ModalLayer`, `kiln-shell/src/modal.rs:69`)
  - the find bar
  - inline edit
  - rename input (`lsp_rename.rs`)
  - the shortcuts recorder
  - toasts
  - the settings and shortcuts editors
  - the git log menus

**Decision:**

- Roles go on the shared widgets, and `IconButton`/`ToggleButton` take a required label.
- Landmarks are as in the table.
- Pickers announce their selection, since the active descendant can't apply there (Q2). Completion announces in the first slice and uses the active descendant from A11Y-13.
- The status bar is a `Region` landmark, not `Status`.
- The in-window prompt and menu strip wait for the Linux port, because macOS uses NSAlert and native menus.
- `tabs.rs:Tabs` (no app users) and `tree.rs:Tree` (gallery only) are skipped.

## 7. Keyboard reachability

Keymap: `crates/fe-keymap/keymaps/fastedit.toml`. Commands are kiln
`#[command]`s and appear in the palette unless `palette = false`.

**Model today.**

- **No kiln button widget takes focus.** That covers `IconButton` (`kiln-ui/src/icon_button.rs:16`), `Button`, `Checkbox` and `Switch` (`controls.rs:91, 300, 399`), `ToggleButton`, `RailButton` (`activity_rail.rs:104`), `Tab` (`tab_bar.rs:103`) and `StatusEntry` (`status_bar.rs:133`). None of them has `track_focus` or `tab_stop`; each has only `on_click`.
- A control is reachable from the keyboard only when a command or the surface's own keys do the same thing.
- There is **no Tab order between controls**. Nothing calls gpui's `focus_next`/`focus_prev` (`gpui-pre window.rs:2440`), and `tab`/`shift-tab` are bound only in the editor, vim, ghost text and completion.
- The only `tab_stop(true)` handles are the view-stack headers (`kiln-shell/src/view_stack.rs:190, 215`), and Tab can't reach those either.
- The setup wizard has its own Tab order (`setup_wizard.rs:on_key` :537–556).

**What keyboard alone can't reach or operate today:**

| Surface | Control | Source | Reachable? |
|---|---|---|---|
| Toasts | Choice buttons (LSP `showMessageRequest`, grammar install, force-push "Yes", update "Download", terminal "Show", vim filter "Cancel") | `crates/fe-workbench/src/toast.rs:render_toast` choice `Button`s :505–520 | **No.** `toast.dismiss_all` (cmd-k escape) answers "none", so only the negative choice is possible |
| Toasts | Action button; close × | `toast.rs:537`, :479 | Only by running the same command from the palette; close only via dismiss-all |
| In-window prompt (Linux) | Buttons other than the default and the cancel one | `kiln/crates/kiln-ui/src/prompt.rs:Prompt::on_key_down` :66, buttons :100–128 | **No.** Enter is the default and Escape the cancel; a third button is mouse-only |
| Editor | Code lens items | `crates/fe-gpui/src/view.rs:press_lens` :1260; `fe-workbench/src/workbench_commands.rs` `lsp.run_lens` :1406 (`palette = false`, unbound) | **No** |
| Editor | Inlay hint double-click (apply its edits) | `crates/fe-gpui/src/inlay_hints.rs:double_click_inlay_hint` :309 | **No** |
| Tab bar | "More Actions" (…), breadcrumb path and symbol crumbs | `crates/fe-workbench/src/pane.rs` :2370, :2554, :2597 | **No.** The palette lists the same commands. The crumbs' pickers have no command |
| Tab bar | Reorder by drag | `kiln-ui/src/tab_bar.rs:on_drag` :240 | No (a guess, unverified: `pane.move_tab_*` moves a tab between splits, not within a strip) |
| View stack | Section header collapse; divider | `kiln-shell/src/view_stack.rs` :477–489, `drag_divider` :373 | The header is a tab stop with a focus ring, but nothing moves focus to it. The divider can't be reached |
| Settings editor | Text-list items after the first; Add Item (non-empty list); Remove item; Reset to Default; User/Workspace scope tabs | `fe-workbench/src/settings_editor_controls.rs` `activate` :866, :1068, :1088; `settings_editor.rs` :833, :877 | **No** |
| Shortcuts editor | Menu items: Copy Command ID, Reset, Show Same Keybindings | `fe-workbench/src/shortcuts_editor.rs:open_menu` :496, `MenuItem` :120, `on_key_down` :654 | **No.** Only Enter (add or change) and Delete work |
| Git log | Right-click menu: arrows and Enter; filter clear; commit card web link | `fe-workbench/src/git_log.rs` :1654, :2398, :734 | No for all three. The menu closes on Escape, and its items are palette commands (`git_log_menu.rs:94–130`). The filter field is reachable only by `/` with vim on (:1658) |
| Git panel | Per-row Stage/Unstage/Discard; header buttons | `fe-workbench/src/git_panel.rs` :1020–1031, :1065 | **Only with vim** (`s`/`u`/`-`/`X`) |
| Commit diff view | Previous/next change, collapse/expand all, section toggle, "N hidden lines" | `fe-workbench/src/commit_diff_view.rs` :759–772, :856, :886 (context `CommitDiffView` :72 has no bindings) | **No** |
| AI changes | Refactor-preview Apply/Cancel; per-file Accept/Reject | `fe-workbench/src/ai_changes_panel.rs` :321–335 ("Buttons, not commands"), :392 | **No** |
| Search panel | Per-file Replace; dismiss × | `fe-workbench/src/search_panel.rs` :1272, :1283 | No. The toolbar has `search.*` commands |
| Hover | Scroll the content; related locations and the code link | `crates/fe-gpui/src/hover.rs` | No. It opens with `editor.show_hover` and closes on Escape |
| Chat (after the first release) | Code block Apply/Copy; thinking toggle; attachment remove; history | `crates/fe-chat-view/src/reply.rs` :645, :651; `view.rs` :963–978, :1213, :1240, :1289; `fe-workbench/src/chat_panel.rs` :870, :1344, :1816 | **No.** Only "Review N changes" for the latest turn (`chat.review_latest_changes`) |

**Reachable through commands or the surface's own keys, so not gaps:**

- **Rail panels:** each panel's `show_command` (`fe-workbench/src/dock.rs:PANELS` :131), and the terminal through `terminal.toggle`.
- **Dock and pane resizing:** `dock.grow`/`dock.shrink` and `pane.grow_*`/`pane.shrink_*`/`pane.equalize`.
- **Tabs:** switch with `workbench.next_tab` and the 1–9 index commands; close with `workbench.close_tab`; keep a preview tab with `workbench.keep_editor`; split.
- **The project tree:** every operation is a `project_panel.*` command, and it has no context menu.
- **Gutter:** fold, `git.diff_head`, `lsp.code_actions`, `git.open_line_commit`, `proposal.*`.
- **Status items:** each dispatches a command, apart from one LSP item that has only a tooltip (`lsp.rs:2041`).
- **The find bar:** `find.toggle_*`, Enter/Shift-Enter/F3, Escape.
- **Pickers and the palette:** arrows, Enter and Escape (keymap 800–829).
- **The completion list.**
- **The terminal dock.**
- **The settings editor's switches, dropdowns and text fields.**
- **The Linux menu bar:** F10, arrows, Enter, Escape.
- **The setup wizard.**

**Modals: Escape and Return.**

| Modal | Escape | Return |
|---|---|---|
| Prompt (Linux) | cancel | default |
| Pickers | dismiss | confirm |
| Setup wizard | skip | yes |
| Settings dropdown | close | pick |
| Menu bar | close | choose |
| Inline edit | cancel | submit |
| Peek | close | open |
| Hover and signature help | close | n/a |
| Chat API key form | close | submit (a guess, unverified) |
| **Git log and shortcuts context menus** | close | **nothing** |
| **Choice toasts** | not covered | not covered: toasts never take focus (`toast.rs:22`) |

NSAlert, native menus and the file panels follow AppKit's own keyboard rules.

**Focus ring.**

- Only these draw one:
  - the view-stack headers, the only `focus_visible` use (`view_stack.rs:488`);
  - selected and focused list and tree rows (`kiln-ui/src/list.rs:47`, `tree_row.rs:183`);
  - text fields (`text_field.rs:658`);
  - the setup wizard (`setup_wizard.rs:597`).
- No button, toggle, tab or rail item draws one, since none can be focused.
- The ring's look (colour, width, offset) is the polish audit's. This doc owns only *which* controls can take focus. A11Y-10b makes it 2 px under Increase Contrast.

**Full Keyboard Access.**

- No code reads `AppleKeyboardUIMode`.
- Full Keyboard Access moves through focusable accessibility elements.
  - In Tenmoku's window, only the setup wizard has any today. Spike 2 shows the workbench exposes nothing below the window.
  - Upstream #57579 (open) also reports that Full Keyboard Access swallows Space in GPUI windows.
- Once A11Y-3 gives buttons a role, their `on_click` registers `Action::Click` (`div.rs:3595`), so **VoiceOver can press them with VO-Space** (`accessibilityPerformPress`, `accesskit_macos/src/node.rs:668`) even though they never take keyboard focus.
- Full Keyboard Access without VoiceOver needs `setAccessibilityFocused:`, which becomes AccessKit `Focus`. gpui honours that only for divs with `track_focus` (`div.rs:3598`; `window.rs:7075–7080`).
- **Open question Q3:** should every kiln button become focusable (a `tab_stop` focus handle, Tab/Shift-Tab through a panel's controls, Enter/Space to press), so that Full Keyboard Access works without VoiceOver? That is a kiln-ui change to every button widget, and the polish audit's focus ring depends on it. Checks H11 (Keyboard navigation) and H11a (Full Keyboard Access, a separate setting under Accessibility › Keyboard) decide.

**Decision: commands first, with focus inside transient surfaces only.**

Every gap in the table above gets either a command (palette-listed, bound
only where a peer surface already binds one) or a focus operation, with an
owning card, or is deferred by name. A11Y-16 is split in two so each half
is one m: **A11Y-16a** for the editor and window chrome, **A11Y-16b** for the
panels.

| Gap | Fix | Card |
|---|---|---|
| Toast choice buttons, action button, close × | **Focus:** a new `toast.focus` command focuses the newest toast; Tab and Shift-Tab move between its buttons, Enter or Space presses, Escape dismisses with "none". The buttons take focus and draw the ring | A11Y-16a |
| In-window prompt, third button | **Focus:** Tab and Shift-Tab between the prompt's buttons, Enter presses the focused one | A11Y-16a (built and tested on Linux; it's NSAlert on the Mac) |
| Code lens items | `lsp.run_lens` becomes palette-visible as "Code Lens: Run…", a picker over the lenses on the caret line | A11Y-16a |
| Inlay hint double-click | `editor.apply_inlay_hint`: applies the edits of the hint at or nearest the caret on its line | A11Y-16a |
| Tab bar "More Actions" | Already palette commands; no change | — |
| Breadcrumb path and symbol crumbs | `workbench.pick_breadcrumb_path` and `workbench.pick_breadcrumb_symbol` open the same pickers the crumbs open | A11Y-16a |
| Tab reorder by drag | `pane.reorder_tab_left` / `pane.reorder_tab_right` move the active tab within its strip (distinct from the existing `pane.move_tab_*`, which move it between splits) | A11Y-16a |
| View-stack header collapse | `view_stack.toggle_section` toggles the section holding focus; `view_stack.focus_next_section` / `focus_previous_section` focus the headers, which already draw a ring | A11Y-16a |
| View-stack divider | **Deferred** to a later card: section heights follow collapse and the window, and resizing by keyboard is not needed to read or operate a section | — |
| Settings: text-list items after the first; Add Item; Remove item | Up and Down move between a list's items; `settings.add_list_item` and `settings.remove_list_item` act on the focused list and item | A11Y-16b |
| Settings: Reset to Default; User/Workspace scope tabs | `settings.reset_to_default` for the selected row; `settings.switch_scope` | A11Y-16b |
| Shortcuts editor menu items | Arrows and Enter in the menu (`MenuPanel` as opened by `open_menu`), and palette commands `shortcuts.copy_command_id`, `shortcuts.reset_binding`, `shortcuts.show_same_keybindings` on the selected row | A11Y-16b |
| Git log right-click menu | Arrows and Enter in the menu; its items are already palette commands | A11Y-16b |
| Git log filter field; filter clear | `git_log.focus_filter` (works with vim off); Escape in the filter clears it, then a second Escape returns focus to the list | A11Y-16b |
| Git log commit card web link | `git_log.open_commit_link` for the selected commit | A11Y-16b |
| Git panel per-row Stage/Unstage/Discard; header buttons | Non-vim `git_panel.stage_row`, `unstage_row`, `discard_row` on the selected row; the header buttons' actions are already `git.*` commands | A11Y-16b |
| Commit diff: previous/next change, collapse/expand all | Bind the existing actions in the `CommitDiffView` context (`commit_diff_view.rs:72`) | A11Y-16b |
| Commit diff: section toggle; "N hidden lines" | `commit_diff.toggle_section` and `commit_diff.expand_hidden` at the caret's section | A11Y-16b |
| AI changes: refactor-preview Apply/Cancel | `ai_changes.apply_preview` and `ai_changes.cancel_preview` | A11Y-16b |
| AI changes: per-file Accept/Reject | `ai_changes.accept_file` and `ai_changes.reject_file` on the selected file row | A11Y-16b |
| Search panel per-file Replace; dismiss × | `search.replace_in_file` and `search.dismiss_file` on the selected file | A11Y-16b |
| Hover: scrolling, related locations, code link | **Deferred** to A11Y-13, which gives hover a focusable body | A11Y-13 |
| Chat controls | **Deferred** | A11Y-18 |
| Git log and shortcuts menus: Return does nothing | Enter chooses the highlighted item (the arrows-and-Enter rows above) | A11Y-16b |

- No global Tab order through the chrome in the first slice (Q3). So, outside the toasts, the prompt, list and tree rows, text fields and the view-stack headers, **controls are reached by command, not by focus, and draw no focus ring**. Section 12 checks command reach and the focus ring separately.
- New commands are palette-listed and unbound unless a sibling surface binds the same key, to keep binding-count tests stable (they live in fe-gpui, fe-workbench and fe-editor tests outside these scopes).

## 8. System settings to respect

**How changes are observed live:**

- AppKit posts `NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification` on **`NSWorkspace.sharedWorkspace.notificationCenter`**, not the default centre, whenever any display option below changes.
- gpui-pre-macos already observes that centre for sleep and wake (`gpui-pre-macos/src/platform.rs:1350–1363`). The same block-based observer pattern works from our code.
- Read all four booleans at startup and again on each notification. Then hop to the GPUI foreground through a channel and a `cx.spawn` loop, and update a global.

**Where the code goes:**

- **`kiln-platform`**, the one kiln crate allowed to branch on the OS (`kiln/crates/kiln-platform/src/lib.rs:3–5`). A new `system_a11y.rs` holds the platform-neutral part:
  - a `SystemA11y { reduce_motion, increase_contrast, reduce_transparency, differentiate_without_color }` global;
  - `system_a11y(cx)` to read it;
  - `observe_system_a11y(cx, f)` to watch it.
- The macOS reader goes in `system_a11y/macos.rs` behind `cfg(target_os = "macos")`. It uses `objc2-app-kit 0.2`, which is already in Cargo.lock through `crates/fastedit`, so kiln-platform gains a target dependency but the lock gains no new crate.
- The Linux reader is a later card (the xdg portal's `org.freedesktop.appearance` `contrast`, and GNOME's `enable-animations`). Until then everything is off.
- **The app applies the values**: `crates/fastedit/src/main.rs`, next to the existing `follow_system_appearance` wiring (:381).
- Kiln-level code stays platform-neutral, so the neon port only adds a reader.

| Setting | AppKit API (`NSWorkspace.sharedWorkspace`) | What Tenmoku does | Where |
|---|---|---|---|
| Reduce Motion | `accessibilityDisplayShouldReduceMotion` (macOS 10.12+) | `cx.set_reduce_motion(value)` (`gpui-pre app.rs:1299`), which refreshes windows. A setting `workbench.reduce_motion = "system" \| "on" \| "off"` (default `system`; Zed's #60614 shape) overrides it. Tenmoku has no motion today (`docs/design-language.md:395–418`), so the visible effect is gpui's animated images (`img.rs:321`). The polish audit's motion rules must check `cx.reduce_motion()`: this card owns the plumbing, the polish audit the visual rule | kiln-platform reads; fastedit `main.rs` applies; `fe-config/src/settings.rs` for the setting |
| Increase Contrast | `accessibilityDisplayShouldIncreaseContrast` | A **high-contrast token rule**, not new theme files, so every bundled theme gets it. In `kiln/crates/kiln-core/src/theme/tokens.rs` (`RULES` :198), a `contrast: High` pass raises borders and dividers to the foreground-muted level, makes `state.focus_ring` 2 px and opaque, raises the selection and `list.row_focus_border` alpha, and makes muted text at least 7:1 against its background. In `fe-config/src/theme.rs:EditorPalette` it raises the gutter, line-number, inlay and ghost-text contrast. A setting `workbench.contrast = "system" \| "high" \| "normal"`. **Also:** while Increase Contrast is on, AppKit reports `NSAppearanceNameAccessibilityHighContrast(Dark)Aqua`, which gpui-pre-macos maps to Light (section 2). So kiln-platform also resolves dark or light itself, with `NSApp.effectiveAppearance bestMatchFromAppearancesWithNames:@[Aqua, DarkAqua]`, and `kiln-ui/src/theme.rs:follow_system_appearance` (:383) uses that while contrast is high. That is a workaround, not a patch (section 3) | kiln-platform reads; kiln-core tokens and kiln-ui theme apply; fe-config palette and setting |
| Reduce Transparency | `accessibilityDisplayShouldReduceTransparency` | Read and exposed, **no behaviour**. Tenmoku has no translucent windows or vibrancy (no `WindowBackgroundAppearance` use in `crates/` or `kiln/`). The polish audit must keep overlays opaque, or check the flag if it adds translucency | kiln-platform only |
| Differentiate Without Colour | `accessibilityDisplayShouldDifferentiateWithoutColor` | Add shape or letter cues where colour is the only signal today. Diagnostics: the underline and gutter dot share one shape across severities (`fe-gpui/src/element.rs:2137–2160`), so give the gutter a severity glyph (✕ ▲ i) and use dotted versus wavy underlines. Folder git dots (`project_panel.rs:955–969`) become the letter of the highest change. Change-gutter added versus modified bars are the same shape (`element/decorations.rs:30–35`), so draw modified hatched. Already distinct: file letters M/A/D/U/C (`git_panel.rs:1916–1924`), problem letters E/W/I/H (`problems_panel.rs:756`), the deletion wedge. A setting `workbench.differentiate_without_color = "system" \| "on" \| "off"` | kiln-platform reads; fe-gpui element and decorations, fe-workbench project panel apply. Second slice (A11Y-14) |
| Larger text | **No AppKit dynamic type for third-party Mac apps**, so there is nothing to observe | Propose `workbench.ui_font_size` (default 12, the workbench root's `text_size(px(12.))`, `fe-workbench/src/workbench.rs:1013`), plus "Zoom In/Out/Reset" commands that step `editor.font_size` (`fe-config/src/settings.rs:616`) and the terminal's. Chrome sizes are literal px in 45 places (`docs/design-language.md:318–353`), so UI scaling needs a **type ramp first**: tokens for the 11–15 px roles, owned by the polish audit. Then `ui_font_size` scales the ramp (or `window.set_rem_size` once sizes are rems). It depends on the polish audit's ramp card | fe-config setting; kiln-ui theme ramp; fe-workbench commands. Second slice (A11Y-15) |

**Decision:**

- One kiln-platform `SystemA11y` observer, with a macOS reader on `NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification`; the app applies it.
- Reduce Motion and Increase Contrast are in the first slice, including the high-contrast appearance workaround.
- Reduce Transparency is observed but does nothing.
- Differentiate Without Colour and larger text are in the second slice, and larger text waits on the polish audit's type ramp.
- **Open question:** H9, whether the high-contrast appearance names really reach gpui's mapping on current macOS.

## 9. Testing strategy

- **Can the test platform be forced active? No, not without a patch.**
  - The test window doesn't implement `a11y_init` (`platform.rs:1387` default), so the activation callback that sets the flag (`window.rs:1686–1705`) never runs.
  - `A11y::new`, `A11ySubtreeBuilder::new` and the active flag are `pub(crate)`.
  - `debug_a11y_tree_json` returns `None` headless (`a11y_tree_dump.rs`).
  - A test-support hook needs gpui-pre itself vendored. It isn't worth that on its own (section 3).
- **Shape 1, for text: pure builders, with accesskit_consumer as the oracle.**
  - Each text element's a11y data comes from a pure function, for example `a11y_text::build(display, text, frame_rows, selection, cap) -> A11yText`.
  - A thin adapter pushes the result into the `A11ySubtreeBuilder`.
  - Tests turn `A11yText` into a `TreeUpdate`, load it in `accesskit_consumer::Tree` (a dev-dependency at the locked `=0.38.0`), and assert what accesskit_macos would answer: `line_range_from_index`, `to_line_index` of the focus, `text_position_from_global_utf16_index`, `Range::text`, `document_range` length.
  - This is spike 1's harness, and it covers folds, wrap, CRLF, non-BMP characters (UTF-16 lengths), the cap, the empty file and vim's collapsed selection.
  - It also covers section 4's selection projection (Select All over the cap, endpoints above and below the window, a caret scrolled off, an endpoint in a fold or past a truncated row; none may panic) and geometry (`bounding_boxes` non-empty for visible, margin and empty runs, and for collapsed carets on LF and CRLF blank lines; `text_position_at_point` on a visible character), and run order (document text in buffer order with margins on both sides), for both the editor and the viewer.
- **Shape 2, for chrome: the recorder.**
  - The setup wizard's pattern is a `node()` that records `(Role, label)` under `cfg(test)` (`setup_wizard.rs:567–578`), and tests assert the list (`labels_and_roles`).
  - Card A11Y-2 generalises it. `kiln_ui::a11y` records `(element id, Role, label, states)` per window draw under `cfg(any(test, feature = "test-support"))`, and its `nodes(cx)` reads them back. kiln-ui has no `test-support` feature today, so A11Y-2 adds one, and fe-workbench's dev-dependencies turn it on; a dependent crate's tests never see kiln-ui's `cfg(test)`.
  - Every chrome card asserts its surfaces' roles and labels, and that every focusable control has a label.
- **Shape 3: the running-app dump.**
  - `FASTEDIT_A11Y_DUMP=<path>` (card A11Y-1, `crates/fastedit/src/main.rs`) writes `Window::debug_a11y_tree_json()` to `<path>` after each frame where a11y is active, throttled to once a second.
  - It works wherever the platform activates a11y: on the Mac under VoiceOver or Accessibility Inspector, and on Linux under AT-SPI (spike 2).
  - It's the bridge between the unit tests and the human checks.
- **Shape 4: Linux end-to-end, with the AT-SPI probe from spike 2.**
  - Spike 2 runs the real app under Xvfb with `dbus-run-session`, `at-spi-bus-launcher` and `IsEnabled = true`, then walks the live tree with `busctl`.
  - It needs no assistive technology and no person.
  - Card A11Y-1 turns it into an `a11y-tree <out.json>` command in `.claude/skills/run-fastedit-linux/driver.sh`.
  - It isn't a cargo test: it needs Xvfb and a D-Bus session, which nextest's environment doesn't promise.
  - Cards use it as manual evidence that their roles reach a real platform adapter.
- **Needs a person:**
  - everything VoiceOver *says* and in what order: typing echo, caret moves, announcements and their priority;
  - Accessibility Inspector's view of roles and actions;
  - Full Keyboard Access;
  - high-contrast appearance;
  - the focus ring under Increase Contrast.
  - Section 12 is the script. Card A11Y-11 is the action card that runs it.

**Decision:**

- Implementation cards use shape 1 for text elements and shape 2 for chrome. A regression test for a bugfix is a shape-1 or shape-2 unit test in the touched crate, named for the behaviour, for example `wrapped_row_is_its_own_line`.
- Each card's criteria include its section 12 lines, run by the human card.
- The headless pin in `a11y_tree_dump.rs` stays.

## 10. Recommended first slice

For the first public release, in order:

1. **A11Y-1** Dump hook.
2. **A11Y-2** kiln-ui a11y kit: the label and role helper with the recorder, and the `Announcer`.
3. **A11Y-3** Landmarks and labelled chrome: rail, status bar, icon and toggle buttons, controls.
4. **A11Y-4** Tabs and project tree.
5. **A11Y-5** Text-input template: `TextInput` and `TextField`/`TextLine`.
6. **A11Y-6** Palette, pickers and find bar.
7. **A11Y-7** Editor text area.
8. **A11Y-8** File viewer.
9. **A11Y-9** Editor and chrome announcements: toasts, vim mode and messages, find count, diagnostics at the caret, folds, cursors, "Announce Position".
10. **A11Y-10** System settings: the observer and Reduce Motion.
11. **A11Y-10b** Increase Contrast, with the appearance workaround.
12. **A11Y-11** VoiceOver pass (human).
13. **A11Y-16a** Keyboard reach: editor and chrome (section 7).
14. **A11Y-16b** Keyboard reach: panels (section 7).

This follows the spec's suggestion, with three changes:

- The **prompt** moves out, because it's NSAlert on the Mac.
- **TextInput** joins `TextLine`, because it's the widely used one.
- The **keyboard-reach fixes** join the slice, because VoiceOver users drive by keyboard.

**Non-goals for the first release:**

- the terminal and chat
- completion, hover and signature help as lists (A11Y-13)
- the settings and shortcuts editors (A11Y-12)
- the git panels, git log, problems, references and search panels beyond what shared widgets give them
- inlay hints and ghost-text content in speech
- code lenses as buttons
- the minimap
- continuous reading past the window (Q1)
- attributed-string styles
- braille-specific labels
- Voice Control names beyond the labels
- Differentiate Without Colour and larger text
- Windows
- AT-SPI readers on Linux (the kiln code is shared; only the system-settings reader and the human check are Mac-only)

**Decision:** the first slice is the 14 items above. The user approves it, or reorders it, before any card is readied.

## 11. Proposed card breakdown

Sizes run xs to xl. "Linux-OK" means the card can be built and tested on
Linux. Every card here is platform-neutral kiln or app code, apart from the
macOS reader in A11Y-10 and the human check. No scope includes
`crates/*/src/lib.rs`:

- New app-crate modules are declared from an existing module file, as `element.rs` declares `element/*.rs` and `workbench.rs` uses `#[path]`.
- Two kiln crates need one forced line in their `lib.rs` for a new public module: kiln-ui for `a11y`, kiln-platform for `system_a11y`. Those are named explicitly.
- Tests that need the consumer oracle add `accesskit_consumer = "=0.38.0"` as a dev-dependency. It is already in Cargo.lock, so the lock changes only in the dependent's entry.

| Card | Size | Depends on | Platform | Scope |
|---|---|---|---|---|
| **A11Y-1** Dump hook: `FASTEDIT_A11Y_DUMP=<path>` writes `debug_a11y_tree_json` after active frames; adds the driver's `a11y-tree` AT-SPI probe (section 9, shape 4) | xs | — | Linux-OK | `crates/fastedit/src/main.rs`, `.claude/skills/run-fastedit-linux/driver.sh`, `.claude/skills/run-fastedit-linux/SKILL.md` |
| **A11Y-2** kiln-ui a11y kit: role and label helpers with the test recorder, `Announcer` (live region), and `Politeness`; the announcer mounted in Tenmoku's real root, `Workbench::render`, and in kiln `Root` for other kiln apps. Evidence: the A11Y-1 dump or AT-SPI probe of a running Tenmoku window shows the `Status` node | s | 1 | Linux-OK | `kiln/crates/kiln-ui/src/a11y.rs`, `kiln/crates/kiln-ui/src/tests/a11y.rs`, `kiln/crates/kiln-ui/src/tests/mod.rs`, `kiln/crates/kiln-ui/src/lib.rs` (forced: `mod a11y;` and its `pub use`), `kiln/crates/kiln-ui/Cargo.toml` (a new `[features] test-support = []`, since kiln-ui has none and a dependent's tests don't see its `cfg(test)`), `crates/fe-workbench/Cargo.toml` (dev-dependency `fe-ui = { workspace = true, features = ["test-support"] }`; `fe-ui` is kiln-ui renamed, root `Cargo.toml:29`), `crates/fe-workbench/src/workbench.rs` (mount), `crates/fe-workbench/tests/a11y_announcer.rs`, `kiln/crates/kiln-shell/src/root.rs` (mount) |
| **A11Y-3** Landmarks and labelled chrome, on the render path Tenmoku uses (`Shell::render`, the docks, `Workbench::render`); the status bar is a `Region`. Validation includes `cargo build -p kiln --examples` for the gallery. Evidence: the A11Y-1 dump or AT-SPI probe of a running Tenmoku window lists the five landmarks | m | 1, 2 | Linux-OK | `kiln/crates/kiln-ui/src/activity_rail.rs`, `kiln/crates/kiln-ui/src/status_bar.rs`, `kiln/crates/kiln-ui/src/icon_button.rs`, `kiln/crates/kiln-ui/src/toggle_button.rs`, `kiln/crates/kiln-ui/src/controls.rs`, `kiln/crates/kiln-ui/src/tests/a11y_chrome.rs`, `kiln/crates/kiln-ui/src/tests/mod.rs`, `kiln/crates/kiln-shell/src/shell.rs`, `kiln/crates/kiln-shell/src/dock.rs`, `kiln/crates/kiln-shell/src/status.rs`, `crates/fe-workbench/src/workbench.rs`, `crates/fe-workbench/src/workbench_chrome.rs`, `crates/fe-workbench/src/status_bar.rs`, `crates/fe-workbench/tests/a11y_landmarks.rs`, plus every `IconButton::new`/`ToggleButton::new` caller made to pass a label: `crates/fe-workbench/src/commit_diff_view.rs`, `crates/fe-workbench/src/find_bar.rs`, `crates/fe-workbench/src/git_log.rs`, `crates/fe-workbench/src/git_panel.rs`, `crates/fe-workbench/src/hierarchy_panel.rs`, `crates/fe-workbench/src/pane.rs`, `crates/fe-workbench/src/recent_picker.rs`, `crates/fe-workbench/src/search_panel.rs`, `crates/fe-workbench/src/terminal_panel.rs`, `crates/fe-workbench/src/terminal_find.rs`, `crates/fe-workbench/src/toast.rs`, `crates/fe-gpui/src/diff_view.rs`, `kiln/crates/kiln/examples/gallery/pages/chrome.rs` |
| **A11Y-4** Tabs and project tree | m | 2, 3 | Linux-OK | `kiln/crates/kiln-ui/src/tab_bar.rs`, `kiln/crates/kiln-ui/src/tree_row.rs`, `kiln/crates/kiln-ui/src/tests/tree_row.rs`, `kiln/crates/kiln-ui/src/tests/tabs.rs`, `kiln/crates/kiln-shell/src/pane.rs`, `crates/fe-workbench/src/pane.rs`, `crates/fe-workbench/src/project_panel.rs`, `crates/fe-workbench/tests/a11y_tabs_tree.rs` |
| **A11Y-5** Text-input template: `TextInput` and `TextField`/`TextLine` get an id, role (`TextInput`/`SearchInput`/`PasswordInput`), one run, selection and a `SetTextSelection` handler | m | 2 | Linux-OK | `kiln/crates/kiln-ui/src/text_input.rs`, `kiln/crates/kiln-ui/src/text_field.rs`, `kiln/crates/kiln-ui/src/tests/text_input_a11y.rs`, `kiln/crates/kiln-ui/src/tests/text_field.rs`, `kiln/crates/kiln-ui/src/tests/mod.rs`, `kiln/crates/kiln-ui/Cargo.toml` (dev-dep), `Cargo.lock` |
| **A11Y-6** Palette, pickers and find bar | m | 2, 5 | Linux-OK | `kiln/crates/kiln-ui/src/palette.rs`, `kiln/crates/kiln-ui/src/list.rs`, `kiln/crates/kiln-ui/src/tests/list.rs`, `kiln/crates/kiln-shell/src/picker.rs`, `kiln/crates/kiln-shell/src/modal.rs`, `crates/fe-workbench/src/find_bar.rs`, `crates/fe-workbench/src/tab_switcher.rs`, `crates/fe-workbench/tests/a11y_pickers.rs` |
| **A11Y-7** Editor text area (section 4), with the a11y-text bench | l | 5 | Linux-OK (behaviour is checked on the Mac by A11Y-11) | `crates/fe-gpui/src/element.rs`, `crates/fe-gpui/src/element/a11y_text.rs`, `crates/fe-gpui/src/element/a11y_text_tests.rs`, `crates/fe-gpui/src/view.rs`, `crates/fe-gpui/benches/a11y_text.rs`, `crates/fe-gpui/Cargo.toml` (dev-dep, `[[bench]]`), `Cargo.lock` |
| **A11Y-8** File viewer text area | s | 7 | Linux-OK | `crates/fe-gpui/src/file_viewer/element.rs`, `crates/fe-gpui/src/file_viewer.rs`, `crates/fe-gpui/src/file_viewer/tests.rs` |
| **A11Y-9** Announcements (section 5): toasts, vim mode and messages, find count, diagnostics at the caret, folds, cursor count, clipped selections, ghost text, completion open/selection (section 6), "Editor: Announce Position". Tests cover completion opening, refiltering, accepting and dismissing with the text area still the focused node | m | 2, 6, 7 | Linux-OK | `crates/fe-gpui/src/completion_menu.rs`, `crates/fe-workbench/src/toast.rs`, `crates/fe-workbench/src/vim_host.rs`, `crates/fe-workbench/src/vim_line.rs`, `crates/fe-workbench/src/find_bar.rs`, `crates/fe-gpui/src/pane_item.rs`, `crates/fe-gpui/src/hover.rs`, `crates/fe-gpui/src/view/a11y_announce.rs`, `crates/fe-gpui/src/view/a11y_announce_tests.rs`, `crates/fe-gpui/src/view.rs`, `crates/fe-workbench/tests/a11y_announce.rs` |
| **A11Y-10** System a11y observer and Reduce Motion (plus Reduce Transparency read) | s | — | **Mac-only reader**; API Linux-OK | `kiln/crates/kiln-platform/src/system_a11y.rs`, `kiln/crates/kiln-platform/src/system_a11y/macos.rs`, `kiln/crates/kiln-platform/src/lib.rs` (forced: `pub mod system_a11y;`), `kiln/crates/kiln-platform/Cargo.toml` (macOS target deps), `Cargo.lock`, `crates/fastedit/src/main.rs`, `crates/fe-config/src/settings.rs`, `crates/fe-config/src/settings_tests.rs` |
| **A11Y-10b** Increase Contrast: token rule, editor palette, setting, appearance workaround | m | 10 | Mac-only reader; rule Linux-OK | `kiln/crates/kiln-core/src/theme/tokens.rs`, `kiln/crates/kiln-core/src/theme/mod.rs`, `kiln/crates/kiln-ui/src/theme.rs`, `kiln/crates/kiln-ui/src/tests/theme_tokens.rs`, `kiln/crates/kiln-platform/src/system_a11y/macos.rs`, `crates/fe-config/src/theme.rs`, `crates/fe-config/src/settings.rs`, `crates/fastedit/src/main.rs` |
| **A11Y-11** VoiceOver pass on the first slice (action, human) | m | 1–10b, 16a, 16b | **Mac-only** (human) | `docs/accessibility-audit.md` (results), then bugs filed as their own cards |
| **A11Y-12** Settings and shortcuts editors | m | 3, 5 | Linux-OK | `crates/fe-workbench/src/settings_editor.rs`, `crates/fe-workbench/src/settings_editor_controls.rs`, `crates/fe-workbench/src/shortcuts_editor.rs`, `crates/fe-workbench/tests/a11y_settings.rs` |
| **A11Y-13** Completion, hover and signature help | m | 7, 9 | Linux-OK | `crates/fe-gpui/src/completion_menu.rs`, `crates/fe-gpui/src/hover.rs`, `crates/fe-gpui/src/signature_help.rs`, `crates/fe-gpui/src/view.rs`, `crates/fe-gpui/tests/a11y_completion.rs` |
| **A11Y-14** Differentiate Without Colour cues | m | 10 | Mac-only reader; drawing Linux-OK | `crates/fe-gpui/src/element.rs`, `crates/fe-gpui/src/element/decorations.rs`, `crates/fe-gpui/src/element/decorations_tests.rs`, `crates/fe-workbench/src/project_panel.rs`, `crates/fe-config/src/settings.rs` |
| **A11Y-15** UI font size and zoom commands | m | polish audit's type-ramp card | Linux-OK | `crates/fe-config/src/settings.rs`, `kiln/crates/kiln-ui/src/theme.rs`, `crates/fe-workbench/src/workbench.rs`, `crates/fe-workbench/src/workbench_palettes.rs`, `crates/fe-workbench/tests/ui_font_size.rs` |
| **A11Y-16a** Keyboard reach, editor and chrome (section 7 table): toast focus, prompt Tab order, code lens picker, `editor.apply_inlay_hint`, breadcrumb pickers, tab reorder, view-stack section commands | m | 3 | Linux-OK | `crates/fe-workbench/src/toast.rs`, `crates/fe-workbench/src/workbench_commands.rs`, `crates/fe-workbench/src/pane.rs`, `kiln/crates/kiln-shell/src/pane.rs`, `kiln/crates/kiln-shell/src/view_stack.rs`, `kiln/crates/kiln-shell/tests/view_stack_sections.rs`, `crates/fe-gpui/src/inlay_hints.rs`, `crates/fe-gpui/src/view.rs`, `kiln/crates/kiln-ui/src/prompt.rs`, `kiln/crates/kiln-ui/src/tests/prompt.rs`, `crates/fe-keymap/keymaps/fastedit.toml`, `crates/fe-keymap/tests/shortcuts_model.rs`, `crates/fe-workbench/tests/a11y_reach_chrome.rs` |
| **A11Y-16b** Keyboard reach, panels (section 7 table): settings and shortcuts editors, git log, git panel, commit diff, AI changes, search panel | m | 3, 16a | Linux-OK | `crates/fe-workbench/src/settings_editor.rs`, `crates/fe-workbench/src/settings_editor_controls.rs`, `crates/fe-workbench/src/shortcuts_editor.rs`, `crates/fe-workbench/src/git_log.rs`, `crates/fe-workbench/src/git_log_menu.rs`, `crates/fe-workbench/src/git_panel.rs`, `crates/fe-workbench/src/commit_diff_view.rs`, `crates/fe-workbench/src/ai_changes_panel.rs`, `crates/fe-workbench/src/search_panel.rs`, `crates/fe-keymap/keymaps/fastedit.toml`, `crates/fe-keymap/tests/shortcuts_model.rs`, `crates/fe-workbench/tests/a11y_reach_panels.rs`. Shares the keymap with 16a, so the two run one after the other |
| **A11Y-17** Terminal (audit card 8) | l | 2, 5 | Linux-OK | `crates/fe-terminal-view/src/element.rs`, `crates/fe-terminal-view/src/a11y.rs` (declared from `element.rs`), `crates/fe-terminal-view/tests/a11y_terminal.rs` |
| **A11Y-18** Chat (audit card 9) | m | 2, 5, 9 | Linux-OK | `crates/fe-chat-view/src/view.rs`, `crates/fe-chat-view/src/reply.rs`, `crates/fe-chat-view/tests/a11y_chat.rs`, `crates/fe-workbench/src/chat_panel.rs` |
| **A11Y-19** In-window prompt and menu strip (Linux port) | s | 2, 3 | Linux-only relevance | `kiln/crates/kiln-ui/src/prompt.rs`, `kiln/crates/kiln-ui/src/menu_bar.rs`, `kiln/crates/kiln-ui/src/tests/prompt.rs`, `kiln/crates/kiln-shell/src/menu_bar.rs` |

Human VoiceOver checks: A11Y-11 runs the first slice. The setup wizard's
T1344 stays where it is. Every second-slice card adds its own section 12
lines to the next human pass.

**Reconciling `docs/accessibility-audit.md`'s ten follow-ups:**

| Audit card | Verdict | Where |
|---|---|---|
| 1. Dump from the running app | **Kept** | A11Y-1 |
| 2. Window landmarks and labelled chrome | **Kept**, trimmed: `menu_bar.rs` moves to A11Y-19 (native menus on the Mac); gains `toggle_button.rs` and `controls.rs` from card 4 | A11Y-3 |
| 3. Tabs and project tree | **Kept**, trimmed: drops `tabs.rs` (no app users) and `tree.rs` (gallery only) | A11Y-4 |
| 4. Find bar, palette and settings controls | **Split**: find bar and palette → A11Y-6; settings → A11Y-12, **reordered** to the second slice; `controls.rs` → A11Y-3 | A11Y-6, A11Y-12 |
| 5. Live regions for toasts and status | **Merged**: the mechanism (`Announcer`) → A11Y-2; the toasts and status sources → A11Y-9 | A11Y-2, A11Y-9 |
| 6. `TextLine` and kiln text fields | **Kept**, widened to `TextInput` (the canvas one most surfaces use), and **reordered** before the pickers | A11Y-5 |
| 7. Editor and file viewer | **Kept**, split: editor A11Y-7, viewer A11Y-8 | A11Y-7, A11Y-8 |
| 8. Terminal | **Kept**, **reordered** after the first release | A11Y-17 |
| 9. Chat | **Kept**, **reordered** after the first release | A11Y-18 |
| 10. VoiceOver pass and fixes | **Kept** as the human action card for the first slice; fixes become their own bugfix cards, not this card's scope | A11Y-11 |

New, not in the audit:

- A11Y-2 (kit)
- A11Y-10 and A11Y-10b (system settings)
- A11Y-13 (completion and hover)
- A11Y-14 (Differentiate Without Colour)
- A11Y-15 (UI font size)
- A11Y-16a and A11Y-16b (keyboard reach)
- A11Y-19 (Linux in-window prompt and menus)

**Decision:** the 21 cards above, readied in table order once the user
approves. A11Y-1 and A11Y-10 have no dependencies and can run in
parallel.

## 12. Human checks

These need the running app on a Mac, with VoiceOver (cmd-F5) and Accessibility
Inspector (Xcode › Open Developer Tool). Use a project with a Rust file over
200 lines that has a soft-wrapped line, a fold and an error diagnostic. Turn
Typing Echo to "Characters and words" (VoiceOver Utility › Verbosity).

**Baseline: today's build, before any card.** Record what is heard and seen
next to `docs/accessibility-audit.md`.

- **H1. Inspector over the window, tab bar, a tree row, the editor and the status bar.**
  - Expect only the `AXWindow` and the system title-bar buttons.
  - Record any role or label.
- **H2. VoiceOver: VO-U (rotor) › Landmarks, then VO-arrows through the window.**
  - Expect no landmarks.
  - Expect the whole window to be read as one element.
- **H3. Editor: type a word, arrow up and down, press option-right.**
  - Expect silence apart from VoiceOver's own key echo.
  - Note whether it echoes characters at all.
- **H9. System Settings › Accessibility › Display › Increase Contrast on, with the system in Dark mode and Tenmoku on `theme.mode = system`.**
  - Does Tenmoku switch to the light theme?
  - Does the log print "unknown appearance"?
  - Restore the setting afterwards.
- **H10. Reduce Motion on, then off.**
  - Expect no visible change today.
  - This is the baseline for the polish audit.
- **H11. Keyboard navigation (System Settings › Keyboard › "Keyboard navigation" on), VoiceOver off.**
  - Tab and Shift-Tab through the window.
  - Record which controls take focus and whether a focus ring shows.
- **H11a. Full Keyboard Access (System Settings › Accessibility › Keyboard › Full Keyboard Access on), VoiceOver off.** This is a separate setting from H11.
  - Tab through the window, and press Space and Return on whatever is highlighted.
  - Record what Full Keyboard Access highlights, and whether Space reaches Tenmoku or is swallowed (upstream #57579).
  - In the editor, type a sentence with spaces: are the spaces inserted?
  - Turn it off afterwards.

**After the first slice (A11Y-11).** Also run with `FASTEDIT_A11Y_DUMP=/tmp/a11y.json`
and keep the dump.

- **H1′. Inspector, chrome.**
  - The rail, side panel, editor area and bottom dock show `AXLandmark*` subroles with labels, and the status bar shows `AXLandmarkRegion` "Status bar".
  - The announcer is one `AXGroup` with subrole `AXApplicationStatus`.
  - Tabs are radio buttons in an `AXTabGroup` with the selected one checked.
  - Tree rows are `AXRow`/`AXOutlineRow` with disclosure level.
  - Every button has a title.
- **H2′. Landmarks rotor.**
  - VO-U › Landmarks lists Activity bar, Explorer, Editor, Panel and Status bar.
  - Choosing one moves the VoiceOver cursor there.
- **H4. Editor: Inspector on the text area.**
  - Role `AXTextArea`.
  - `AXNumberOfCharacters` is about three screens of text.
  - `AXInsertionPointLineNumber` changes with the caret.
  - `AXValue` is empty. Note whether VoiceOver still reads the text with VO-A and VO-L; if not, A11Y-7 must set `value`.
- **H5. Editor speech.**
  - Type a word: the characters and the word are echoed.
  - Arrow down: the new line is read.
  - Arrow right: the character is read.
  - Down onto the wrapped line, then down again: each visual row is read.
  - Repeat in vim normal mode with `j`/`l`: lines and characters are read, and command keys aren't echoed as typing.
  - **If arrows are silent**, that is section 3 trigger 1.
- **H6. Window edges.**
  - Page Down to the end of a long file and back. Reading follows the viewport.
  - VO-A from the top of the screen stops about one screen below the last visible line.
  - Note whether that's acceptable; this is Q1 and trigger 2.
- **H7. Announcements.**
  - Save, which shows a toast: the toast is spoken.
  - `i`, then Escape: "Insert", then "Normal".
  - Find with three matches: "1 of 3", and next gives "2 of 3".
  - Rest the caret on the error: the message is read once.
  - Fold a block: "Folded N lines".
  - Run "Editor: Announce Position": the line and column are read.
  - Does a later focus change cut any of them off?
- **H8. Pickers.**
  - Open the command palette and type. The query is echoed.
  - Down arrow reads the selected command.
  - Escape returns to the editor, and VoiceOver says so.
  - The same for quick open.
- **H9′. Increase Contrast on.**
  - Tenmoku stays dark in Dark mode, with stronger borders, a 2 px focus ring, and readable muted text.
  - Off again: back to normal, live, without a restart.
- **H10′. Reduce Motion on.** `cx.reduce_motion()` is true in the dump-hook log line, and an animated image (if one is open) stops.
- **H11′. Command reach, keyboard only, VoiceOver off.**
  - For every row of section 7's decision table owned by A11Y-16a or A11Y-16b, run its command from the palette (or its key) and confirm it does what the mouse control does. Deferred rows are skipped.
  - Escape closes, and Return confirms, in every in-window modal in section 7's modal table, including the git log and shortcuts menus.
- **H11a′. Focus and focus ring, keyboard only.**
  - Only the surfaces that take focus are checked: a toast after "Toast: Focus" (Tab between its buttons, Space and Return press), list and tree rows, text fields, and the view-stack headers. Each shows the focus ring. Other buttons take no focus in the first slice (Q3), so no ring is expected on them.
  - Repeat with Full Keyboard Access on (as H11a): Space presses a focused toast button, and Space still types in the editor and text fields.
- **H12. File viewer.** Open a large log file in the read-only viewer and repeat H4–H5. The text node is read-only, and typing isn't echoed as text.

**Decision:** H1–H3, H9, H10, H11 and H11a run on today's build and are recorded as the
baseline. H1′–H12 are A11Y-11's script. A failure in H4–H7 maps to a named
trigger or open question in sections 3–5.
