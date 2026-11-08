# App icon

`fastedit.svg` is the source. `fastedit-1024.png` is its 1024px render, embedded
by `src/app_icon.rs` for the macOS Dock and Cmd-Tab icon. Re-render after
editing the SVG with any SVG rasteriser, e.g.
`resvg --width 1024 --height 1024 fastedit.svg fastedit-1024.png`.

`../windows/tenmoku.ico` is the Windows icon, embedded in the executable by
`build.rs`: PNG frames of `fastedit-1024.png` at 16, 20, 24, 32, 40, 48, 64,
96, 128 and 256px. Regenerate it after re-rendering the PNG, e.g. with
ImageMagick: `magick fastedit-1024.png -define
icon:auto-resize=256,128,96,64,48,40,32,24,20,16 ../windows/tenmoku.ico`.
