# App icon

`fastedit.svg` is the source. `fastedit-1024.png` is its 1024px render, embedded
by `src/app_icon.rs` for the macOS Dock and Cmd-Tab icon. Re-render after
editing the SVG with any SVG rasteriser, e.g.
`resvg --width 1024 --height 1024 fastedit.svg fastedit-1024.png`.
