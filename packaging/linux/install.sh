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

case "$here" in
*'
'*)
	echo "install.sh: a desktop entry cannot name a folder with a line break in it: move it first" >&2
	exit 1
	;;
esac
# The Exec line runs the binary by its absolute path, so the launcher finds it with ~/.local/bin
# off its PATH, quoted as the Desktop Entry spec says: in a quoted argument " ` $ \ take a
# backslash, the string-value escape doubles every backslash after that, and % is %%.
exec_path=$(printf '%s\n' "$here/tenmoku" | sed -e 's/\\/\\\\\\\\/g' -e 's/["`$]/\\\\&/g' -e 's/%/%%/g')
staged="$(dirname "$desktop")/.$id.desktop.$$"
mkdir -p "$bin" "$(dirname "$desktop")" "$(dirname "$icon")"
EXEC_PATH=$exec_path awk '
	/^Exec=tenmoku / { sub(/^Exec=tenmoku /, ""); print "Exec=\"" ENVIRON["EXEC_PATH"] "\" " $0; next }
	{ print }
' "$here/$id.desktop" >"$staged" || { rm -f "$staged"; exit 1; }
mv -f "$staged" "$desktop"
ln -sf "$here/tenmoku" "$bin/tenmoku"
cp "$here/$id.svg" "$icon"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$data/applications" || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q "$data/icons/hicolor" || true
echo "installed: $bin/tenmoku -> $here/tenmoku"
case ":$PATH:" in
*":$bin:"*) ;;
*) echo "note: $bin is not on your PATH; add it to run tenmoku from a terminal" ;;
esac
