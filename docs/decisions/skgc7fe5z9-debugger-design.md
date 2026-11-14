# Debugger in B3: the design calls (debugger D0, T642, Mac T1839)

The debugger record (`docs/decisions/r1b9dw3g0g-debugger.md`, §13 "B3 design
first") asks D0 to fit §5's UI to `docs/design/design-system.md` before the UI
cards (D6, D8, D9, D10, D11) start. The result is the design system's new
Debugger section (revision 19). This file keeps the calls that section makes,
where they part from §5, and the script that checks its colours.

## Calls, and where they part from §5

- **Breakpoints over the line numbers, which they hide.** B3 pins the gutter at
  81 px (`crates/fe-gpui/tests/gutter_b3.rs`), so §5's lane is gone. The
  line-number column (the 4 px inset and the 38 px of numbers, x 0–42 at 13 px
  code) gets two slots: breakpoint (x 14–26) and frame (x 29–41). A row with a
  glyph draws no number, as JetBrains' New UI does over its numbers.
  - Considered: a tab behind the number that keeps it (Xcode's). The numbers are
    right-aligned flush against the problem column (`Gutter::number_gap`), so a
    tab would either leave no room after the last digit or reach into the
    problem dot (6 px, centred in its 10 px column). It also stacks badly with
    the frame arrow on a stopped breakpoint row.
  - Considered: a dot left of the digits, in the room the gutter always leaves
    there (`GUTTER_INSET` and `GUTTER_NUMBERS_PAD`, about 8 px with five digits).
    It fits only a 6 px dot, the size of the problem dot, too small to be the
    debugger's main mark or to tell a ring from a dot.
  - The cost: the row's number. Its tooltip and the accessible name carry it,
    and so do the status bar's position and the numbers above and below.
- **Fixed slots.** The frame arrow has its own slot, so stepping onto a
  breakpoint never moves the breakpoint's glyph. The arrow points into the code.
- **State by shape and lightness.** Filled dot (enabled), ring (unverified),
  grey dot (disabled), filled and outlined arrows (current and caller frame);
  the grey is at least 0.13 OKLab L from the red, so no pair rests on hue
  (Accessibility rules). §5's "dot in the muted tint" for disabled became its
  own role because the muted text sits too close in lightness to shinsha
  in both themes (OKLab L 0.74 against 0.70 in Yohen, 0.44 against 0.45 in
  Hakuji).
- **The current frame is hare's fur.** §5 already tints the arrow with the
  accent, and the stopped line is where you are: the caret goes there and your
  next step acts there. Kiseto (VS Code's yellow) is warnings, and its soft
  ground is the identifier highlight, so a kiseto band would read as a symbol's
  uses.
- **Bands with an edge.** The comment colour sits on the doc's floor on the
  cursor line (6:1 Yohen, 4.5:1 Hakuji), so a band can't be lighter than the
  cursor line in Yohen or darker in Hakuji. That leaves Hakuji almost no room:
  its current line is 3.0 ΔE from the editor and 1.8 from the cursor line, and
  no caller-frame tint clears the floor and sits apart from both. So each band
  has a 2 px edge at the code's left (B3's "rust tint + 2 px rust edge" from
  focused selection), and the arrows carry which frame it is. Hakuji's caller
  band is the cursor line's own value.
- **Breakpoints are red, under their own role.** Every debugger's are, and
  shinsha's job (errors) never sits in the same column. It's a role of its
  own, `debug_breakpoint`, with shinsha's values, so a theme can set it apart.
- **The run widget picks one thing.** One choice, a task or a debug
  configuration, from one picker with two groups. Tasks can't be debugged and
  launch configurations have no run-without-debugging in the record, so the
  buttons follow the choice: a task gets ▷ (■ while running) and 🐞 Debug…
  (which opens the picker at the configurations, `debug.start`'s own picker);
  a configuration gets 🐞 Debug alone.
- **Session controls replace the widget while debugging**, rather than adding
  six buttons beside it: the title bar's right group would otherwise carry
  run, stop, debug and the session's six. With no title bar, §5's floating
  toolbar is kept, restyled as a B3 floating surface (overlay, shadow, 6 px).
  The test is "is there a title bar", not the platform.
- **One Debug tool window, three tabs.** §13 moves §5's right-dock container to
  the bottom panel. In a 250 px panel, four stacked views won't fit, so the
  Variables tab is Call Stack beside Watch and Variables, and Breakpoints gets
  its own tab, used less and needed without a session.
- **The Debug Console moves into the Debug tool window** (§5 had it as a
  terminal panel tab). It evaluates in the selected frame, so it belongs beside
  the frames. The terminal panel then holds only ptys. The debuggee's terminal
  stays there (`TerminalPanel::add_debuggee`). **D10's scope changes:**
  `debug_console*.rs` hosts in the Debug tool window (`debug_panel*.rs`, D9's)
  rather than `terminal_panel*.rs`, so D10 depends on D9.
- **The rail icon** appears once a root has a debug configuration or a session
  starts, as §5's appeared with the first session, so a project with nothing to
  debug isn't given an icon it can't use. A hare's fur dot marks a stopped
  session (rust is you: the debugger waits on you).
- **Debug this test stops at the failure.** Run from a test failure, it adds a
  one-shot breakpoint on the failure's line, as run to cursor does (§5), so it
  lands on the assertion. D13 implements it with the rest of the test path.
- **Fallbacks** for themes without the keys (D6 C4): both bands fall back to
  `active_line`, so a theme without them looks as it does today plus the edge
  and arrow; `debug_breakpoint` to `error`; the disabled grey to
  `gutter_foreground`. §5 named the accent and muted tints; an opaque accent
  band would lose the code text.
- **Sizes.** The new kiln-ui roles are `height::TOOLBAR` (30),
  `width::TITLE_LABEL_MAX` (200, moved from `run_widget.rs`),
  `width::CALL_STACK_DEFAULT` (280), and a `glyph` module: `ICON` (16),
  `BREAKPOINT` (12), `ARROW_HEIGHT` (10), `HALF` (6) and `STROKE` (2). The
  slots' positions are fe-gpui gutter constants (`GUTTER_BREAKPOINT_X`,
  `GUTTER_FRAME_X`), as the gutter's columns are. The gutter glyphs scale with the code size
  as the gutter's columns do. New Lucide icons: `pause`, `redo-dot`,
  `arrow-down-to-dot` and `arrow-up-from-dot`.

## The colour check

Run from the repo root with `python3 -I`. It reads every existing role from the
bundled theme files (a translucent one over the editor's ground) and checks the
new roles against the design's rules: code text on each band at the theme's
floor, every glyph 3:1 wherever it sits (the gutter, the cursor line and the
hover tile; the tool window's rows bare, hovered, selected and focused), each band's edge
3:1, the line number on the hover tile 4.5:1, and the disabled grey's lightness
apart from the red. It prints each role's OKLab distance (×100) to its nearest
role, and to the editor and the cursor line. Once D6 adds the keys, the script
skips them when it lists the existing roles.

```python
"""The debugger's colour roles (docs/design/design-system.md, Debugger):
checks them against the design's contrast rules and measures each one's
OKLab distance to its nearest role. Run from the repo root:
python3 -I debug_roles.py"""

import tomllib

THEMES = {
    "Yohen": ("crates/fe-config/themes/yohen.toml", 6.0),
    "Hakuji": ("crates/fe-config/themes/hakuji.toml", 4.5),
}

# The new roles.
NEW = {
    "Yohen": {
        "debug_current_line": "#351501",
        "debug_frame_line": "#2a1a11",
        "debug_breakpoint": "#f2716f",
        "debug_breakpoint_disabled": "#827468",
    },
    "Hakuji": {
        "debug_current_line": "#fff4ed",
        "debug_frame_line": "#f6f7f9",
        "debug_breakpoint": "#a1100e",
        "debug_breakpoint_disabled": "#7c828a",
    },
}

# Code text: the design's syntax table, whose variable is ink.
CODE = {
    "Yohen": {
        "keyword": "#c7a5e6", "function": "#f2c77a", "type": "#7fc9be",
        "string": "#b5cf86", "number": "#f0a3c2", "macro": "#8db1f2",
        "property": "#dccbb8", "variable": "#ede4d6", "punctuation": "#a89a8c",
        "comment": "#9e9b97",
    },
    "Hakuji": {
        "keyword": "#7b36a8", "function": "#925a00", "type": "#0d6e66",
        "string": "#2f6f1f", "number": "#a32f6b", "macro": "#2457c5",
        "property": "#3d444d", "variable": "#1f2328", "punctuation": "#596069",
        "comment": "#69727c",
    },
}

# The glyphs, by the role each is drawn in. They sit over the line numbers
# and, at the same size, in Call Stack and Breakpoints rows.
GLYPHS = {
    "breakpoint (dot, ring, bar, diamond)": "debug_breakpoint",
    "disabled breakpoint": "debug_breakpoint_disabled",
    "frame arrows (hare's fur)": "accent",
}


def rgb(hex_):
    hex_ = hex_.lstrip("#")
    alpha = int(hex_[6:8], 16) / 255 if len(hex_) == 8 else 1.0
    return tuple(int(hex_[i : i + 2], 16) / 255 for i in (0, 2, 4)), alpha


def over(top, ground):
    (t, a), (g, _) = rgb(top), rgb(ground)
    return "#" + "".join(f"{round((a * x + (1 - a) * y) * 255):02x}" for x, y in zip(t, g))


def linear(c):
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def luminance(hex_):
    r, g, b = (linear(c) for c in rgb(hex_)[0])
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def contrast(a, b):
    high, low = sorted((luminance(a), luminance(b)), reverse=True)
    return (high + 0.05) / (low + 0.05)


def oklab(hex_):
    r, g, b = (linear(c) for c in rgb(hex_)[0])
    l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b
    m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b
    s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b
    l, m, s = (x ** (1 / 3) for x in (l, m, s))
    return (
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    )


def delta_e(a, b):
    """OKLab ΔE ×100, as the design quotes it."""
    return 100 * sum((x - y) ** 2 for x, y in zip(oklab(a), oklab(b))) ** 0.5


def roles(path):
    """Every colour role in a theme file but the debugger's own, by
    `table.key`, a translucent one over the editor's ground, as it's seen."""
    with open(path, "rb") as file:
        theme = tomllib.load(file)
    ground = theme["editor"]["background"]
    out = {}
    for table, keys in theme.items():
        for key, value in keys.items() if isinstance(keys, dict) else ():
            if key.startswith("debug_") or not isinstance(value, str):
                continue
            if value.startswith("#") and len(value) in (7, 9):
                out[f"{table}.{key}"] = over(value, ground) if len(value) == 9 else value
    return theme, out


def verdict(ok):
    results.append(ok)
    return "ok" if ok else "FAIL"


results = []
for name, (path, floor) in THEMES.items():
    theme, existing = roles(path)
    editor = theme["editor"]
    ground, cursor_line = editor["background"], editor["active_line"]
    new = dict(NEW[name], accent=theme["accent"]["base"], muted=theme["text"]["muted"])
    print(f"== {name}: code {floor}:1 on each band, glyphs 3:1")
    for band, edge in (("debug_current_line", "accent"), ("debug_frame_line", "muted")):
        fill = new[band]
        low = min(CODE[name], key=lambda token: contrast(CODE[name][token], fill))
        ok = all(contrast(colour, fill) >= floor for colour in CODE[name].values())
        print(f"{band} {fill}: lowest code text, {low}, {contrast(CODE[name][low], fill):.2f}:1",
              verdict(ok))
        lit = contrast(new[edge], fill), contrast(new[edge], ground)
        print(f"  its 2 px edge, {edge} {new[edge]}: {lit[0]:.2f}:1 on the band,"
              f" {lit[1]:.2f}:1 on the editor", verdict(min(lit) >= 3))
        chosen = over(editor["selection"], fill)
        today = over(editor["selection"], cursor_line)
        low = min(CODE[name], key=lambda token: contrast(CODE[name][token], chosen))
        print(f"  under a selection, {chosen}: lowest {low}"
              f" {contrast(CODE[name][low], chosen):.2f}:1, on the cursor line today"
              f" {contrast(CODE[name][low], today):.2f}:1 (the selection's, not a floor)")
    # Where a glyph sits: the gutter, its cursor line and hover tile; rows in
    # the tool window, bare, hovered, selected and selected with focus.
    grounds = {
        "gutter": ground,
        "cursor line": cursor_line,
        "hover tile": editor["fold_hover"],
        "tool window": theme["surface"]["panel"],
        "hovered row": theme["state"]["hover"],
        "selected row": theme["state"]["selected_inactive"],
        "focused row": theme["state"]["selected"],
    }
    for glyph, role in GLYPHS.items():
        colour = new[role]
        lit = {where: contrast(colour, under) for where, under in grounds.items()}
        low = min(lit, key=lit.get)
        print(f"{glyph} {colour}: " + ", ".join(f"{where} {ratio:.2f}" for where, ratio in lit.items()))
        print(f"  lowest {low} {lit[low]:.2f}:1", verdict(lit[low] >= 3))
    number, tile = editor["gutter_foreground"], editor["fold_hover"]
    print(f"line number {number} on the hover tile {tile}: {contrast(number, tile):.2f}:1",
          verdict(contrast(number, tile) >= 4.5))
    lightness = [oklab(new[role])[0] for role in ("debug_breakpoint", "debug_breakpoint_disabled")]
    print(f"breakpoint and disabled OKLab L: {lightness[0]:.3f} and {lightness[1]:.3f},"
          f" {abs(lightness[0] - lightness[1]):.3f} apart", verdict(abs(lightness[0] - lightness[1]) >= 0.1))
    for role, colour in NEW[name].items():
        others = dict(existing)
        others.update({f"{other}": value for other, value in NEW[name].items() if other != role})
        near = min(others, key=lambda other: delta_e(colour, others[other]))
        print(f"{role} {colour}: nearest {near} {others[near]}, ΔE {delta_e(colour, others[near]):.1f};"
              f" editor {delta_e(colour, ground):.1f}, cursor line {delta_e(colour, cursor_line):.1f}")
print("all pass" if all(results) else "SOME FAIL")
```

Its output on the themes at this commit:

```text
== Yohen: code 6.0:1 on each band, glyphs 3:1
debug_current_line #351501: lowest code text, comment, 6.03:1 ok
  its 2 px edge, accent #e0874c: 6.15:1 on the band, 6.59:1 on the editor ok
  under a selection, #603214: lowest comment 3.86:1, on the cursor line today 3.93:1 (the selection's, not a floor)
debug_frame_line #2a1a11: lowest code text, comment, 6.05:1 ok
  its 2 px edge, muted #b8a99a: 7.32:1 on the band, 7.82:1 on the editor ok
  under a selection, #583520: lowest comment 3.90:1, on the cursor line today 3.93:1 (the selection's, not a floor)
breakpoint (dot, ring, bar, diamond) #f2716f: gutter 6.28, cursor line 5.89, hover tile 5.46, tool window 6.52, hovered row 5.46, selected row 5.19, focused row 4.89
  lowest focused row 4.89:1 ok
disabled breakpoint #827468: gutter 3.96, cursor line 3.72, hover tile 3.44, tool window 4.11, hovered row 3.44, selected row 3.27, focused row 3.08
  lowest focused row 3.08:1 ok
frame arrows (hare's fur) #e0874c: gutter 6.59, cursor line 6.19, hover tile 5.74, tool window 6.85, hovered row 5.74, selected row 5.45, focused row 5.13
  lowest focused row 5.13:1 ok
line number #a09080 on the hover tile #2b221d: 5.04:1 ok
breakpoint and disabled OKLab L: 0.702 and 0.568, 0.133 apart ok
debug_current_line #351501: nearest debug_frame_line #2a1a11, ΔE 3.0; editor 6.0, cursor line 4.8
debug_frame_line #2a1a11: nearest editor.active_line #231c18, ΔE 1.7; editor 3.5, cursor line 1.7
debug_breakpoint #f2716f: nearest status.error #f2716f, ΔE 0.0; editor 51.7, cursor line 49.1
debug_breakpoint_disabled #827468: nearest terminal.ansi_8 #6b5a4c, ΔE 8.8; editor 36.2, cursor line 33.5
== Hakuji: code 4.5:1 on each band, glyphs 3:1
debug_current_line #fff4ed: lowest code text, comment, 4.52:1 ok
  its 2 px edge, accent #a84a16: 5.31:1 on the band, 5.74:1 on the editor ok
  under a selection, #eed2c2: lowest comment 3.40:1, on the cursor line today 3.41:1 (the selection's, not a floor)
debug_frame_line #f6f7f9: lowest code text, comment, 4.56:1 ok
  its 2 px edge, muted #4b525b: 7.37:1 on the band, 7.90:1 on the editor ok
  under a selection, #e6d4cc: lowest comment 3.41:1, on the cursor line today 3.41:1 (the selection's, not a floor)
breakpoint (dot, ring, bar, diamond) #a1100e: gutter 8.09, cursor line 7.55, hover tile 6.85, tool window 7.56, hovered row 6.85, selected row 6.72, focused row 6.76
  lowest selected row 6.72:1 ok
disabled breakpoint #7c828a: gutter 3.88, cursor line 3.62, hover tile 3.28, tool window 3.62, hovered row 3.28, selected row 3.22, focused row 3.24
  lowest selected row 3.22:1 ok
frame arrows (hare's fur) #a84a16: gutter 5.74, cursor line 5.36, hover tile 4.86, tool window 5.37, hovered row 4.86, selected row 4.77, focused row 4.80
  lowest selected row 4.77:1 ok
line number #5f6670 on the hover tile #ebecef: 4.91:1 ok
breakpoint and disabled OKLab L: 0.452 and 0.604, 0.152 apart ok
debug_current_line #fff4ed: nearest surface.panel #f7f7f9, ΔE 1.7; editor 3.0, cursor line 1.8
debug_frame_line #f6f7f9: nearest editor.active_line #f6f7f9, ΔE 0.0; editor 2.4, cursor line 0.0
debug_breakpoint #a1100e: nearest status.error #a1100e, ΔE 0.0; editor 57.6, cursor line 55.4
debug_breakpoint_disabled #7c828a: nearest terminal.ansi_8 #8a919a, ΔE 5.0; editor 39.6, cursor line 37.2
all pass
```

Reading the distances: the breakpoint's 0.0 to `status.error` is the shared
value above, and Hakuji's caller frame's 0.0 to `editor.active_line` is the
cursor line's own value. Yohen's caller frame is 1.7 from the cursor line and
Hakuji's current line 1.8: the floor leaves no more, and the edge and arrow
mark the rows. The disabled grey's nearest is a terminal ANSI colour, which
never sits in the gutter. The grey was `#868c94` in Hakuji until review round 1
(T642 F1) found it under 3:1 on the hover tile and on selected rows
(2.82–2.87:1); `#7c828a` is 3.22:1 at its lowest and still 0.15 OKLab L from
the red. The selection lines show that a selection over a
band reads as it does over the cursor line today.
