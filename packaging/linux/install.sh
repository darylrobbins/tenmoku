#!/bin/sh
# Installs this unpacked Tenmoku for the current user, with no root: the binary is linked from
# ~/.local/bin/tenmoku, and the desktop entry and icon go under ~/.local/share so the app shows
# in the launcher. Run it again after moving the folder; --uninstall removes all three.
# Usage: ./install.sh [--uninstall]
set -eu
here=$(cd "$(dirname "$0")" && pwd)
data=${XDG_DATA_HOME:-$HOME/.local/share}
bin="$HOME/.local/bin"
id=dev.tenmoku.tenmoku
desktop="$data/applications/$id.desktop"
icon="$data/icons/hicolor/scalable/apps/$id.svg"

case "${1:-}" in
--uninstall)
	rm -f "$bin/tenmoku" "$desktop" "$icon"
	echo "removed $bin/tenmoku, $desktop and $icon"
	exit 0
	;;
"") ;;
*)
	echo "usage: ./install.sh [--uninstall]" >&2
	exit 2
	;;
esac

mkdir -p "$bin" "$(dirname "$desktop")" "$(dirname "$icon")"
ln -sf "$here/tenmoku" "$bin/tenmoku"
# The absolute path, so the launcher finds it with ~/.local/bin off its PATH.
sed "s|^Exec=tenmoku |Exec=\"$here/tenmoku\" |" "$here/$id.desktop" >"$desktop"
cp "$here/$id.svg" "$icon"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$data/applications" || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q "$data/icons/hicolor" || true
echo "installed: $bin/tenmoku -> $here/tenmoku"
case ":$PATH:" in
*":$bin:"*) ;;
*) echo "note: $bin is not on your PATH; add it to run tenmoku from a terminal" ;;
esac
