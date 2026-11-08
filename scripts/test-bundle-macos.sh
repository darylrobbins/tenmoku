#!/bin/sh
# Fixture test for bundle-macos.sh: bundles a copy of /usr/bin/true as the binary (no cargo
# build) into a temp dir and checks the layout, Info.plist, bundle id, icon and signature.
# Prints one line per check and exits non-zero on the first failure. macOS only.
#
# SCOPE_BASE=<rev> also checks that the diff from <rev> to HEAD touches only the bundling
# files (T1269's scope, plus its own docs/specs/ and docs/decisions/ files) and no Rust file.
# It is opt-in: once the task lands, any later change makes that diff wider, so a default base
# would fail forever.
set -eu
# The fixture checks an ad-hoc signature, whatever identity the caller has exported.
SIGN_IDENTITY=-
export SIGN_IDENTITY
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
bundle="$here/bundle-macos.sh"
t=$(mktemp -d)
trap 'rm -rf "$t"' EXIT

fail() {
	echo "FAIL: $*"
	exit 1
}
ok() { echo "ok: $*"; }

cp /usr/bin/true "$t/tenmoku"
version=9.8.7
"$bundle" --binary "$t/tenmoku" --out "$t/out" --version "$version" >"$t/log" 2>&1 ||
	{ cat "$t/log"; fail "bundle-macos.sh exited non-zero"; }
app="$t/out/Tenmoku.app"
[ "$(tail -n 1 "$t/log")" = "$app" ] || fail "last line is not the app path: $(tail -n 1 "$t/log")"
ok "bundled into $app"

# C2: layout and Info.plist.
[ -f "$app/Contents/MacOS/tenmoku" ] && [ -x "$app/Contents/MacOS/tenmoku" ] ||
	fail "Contents/MacOS/tenmoku missing or not executable"
[ -f "$app/Contents/Resources/Tenmoku.icns" ] || fail "Contents/Resources/Tenmoku.icns missing"
plist="$app/Contents/Info.plist"
[ -f "$plist" ] || fail "Contents/Info.plist missing"
ok "layout"
plutil -lint -s "$plist" || fail "plutil -lint"
ok "plutil -lint"
key() { /usr/libexec/PlistBuddy -c "Print :$1" "$plist"; }
expect() {
	got=$(key "$1") || fail "Info.plist has no $1"
	[ "$got" = "$2" ] || fail "$1 is '$got', expected '$2'"
	ok "$1=$2"
}
expect CFBundleName Tenmoku
expect CFBundleDisplayName Tenmoku
expect CFBundleExecutable tenmoku
expect CFBundleIconFile Tenmoku
expect CFBundlePackageType APPL
expect NSHighResolutionCapable true
expect CFBundleShortVersionString "$version"
expect CFBundleVersion "$version"
! grep -q '@VERSION@' "$plist" || fail "@VERSION@ left in Info.plist"
ok "no @VERSION@ left"

# T1272: document types for folders and text/source/data files, all Alternate and Editor.
types=$(plutil -extract CFBundleDocumentTypes json -o - "$plist") || fail "Info.plist has no CFBundleDocumentTypes"
n=$(/usr/libexec/PlistBuddy -c "Print :CFBundleDocumentTypes" "$plist" | grep -c 'LSHandlerRank = Alternate') || true
[ "$n" -eq 2 ] || fail "expected 2 document types with LSHandlerRank Alternate, found $n"
ok "document types are Alternate"
for uti in public.folder public.text public.source-code public.data; do
	echo "$types" | grep -q "\"$uti\"" || fail "CFBundleDocumentTypes lacks $uti"
done
ok "document types cover public.folder, public.text, public.source-code, public.data"
roles=$(echo "$types" | grep -o '"CFBundleTypeRole":"Editor"' | wc -l | tr -d ' ')
[ "$roles" -eq 2 ] || fail "expected both document types to be role Editor"
ok "document types are role Editor"

# T1511: each bundled font family's OFL.txt, byte for byte, and nothing else under fonts/.
families=0
for licence in "$root"/crates/fe-fonts/fonts/*/OFL.txt; do
	family=$(basename "$(dirname "$licence")")
	shipped="$app/Contents/Resources/fonts/$family/OFL.txt"
	[ -f "$shipped" ] || fail "Contents/Resources/fonts/$family/OFL.txt missing"
	cmp -s "$licence" "$shipped" || fail "Contents/Resources/fonts/$family/OFL.txt differs from $licence"
	grep -q 'SIL OPEN FONT LICENSE Version 1.1' "$shipped" ||
		fail "Contents/Resources/fonts/$family/OFL.txt is not the OFL 1.1 text"
	families=$((families + 1))
done
[ "$families" -eq 3 ] || fail "expected the OFL.txt of 3 font families in crates/fe-fonts/fonts, found $families"
count=$(find "$app/Contents/Resources/fonts" -type f | wc -l | tr -d ' ')
[ "$count" -eq "$families" ] || fail "Contents/Resources/fonts has $count files, expected $families"
ok "the $families font families' OFL.txt ship in Contents/Resources/fonts"

# T1612: every licence text under Contents/Resources/Licenses, byte for byte, and nothing else.
lic="$app/Contents/Resources/Licenses"
nlic=0
same() { # same SOURCE SHIPPED-NAME
	[ -f "$lic/$2" ] || fail "Licenses/$2 missing"
	cmp -s "$1" "$lic/$2" || fail "Licenses/$2 differs from $1"
	nlic=$((nlic + 1))
}
same "$root/LICENSE" LICENSE
same "$root/NOTICE" NOTICE
same "$root/LICENSES/Apache-2.0.txt" LICENSES/Apache-2.0.txt
same "$root/LICENSES/MIT.txt" LICENSES/MIT.txt
same "$root/THIRD_PARTY.md" THIRD_PARTY.md
same "$root/kiln/crates/kiln-ui/assets/icons/LICENSE" icons-LICENSE.txt
same "$root/kiln/crates/kiln-ui/assets/file-icons/LICENSE" file-icons-LICENSE.txt
same "$root/vendor/tree-sitter/LICENSE" tree-sitter-LICENSE.txt
for dir in "$root"/vendor/tree-sitter-*/; do
	same "$dir/LICENSE" "vendor-$(basename "$dir")-LICENSE.txt"
done
for dir in "$root"/extensions/grammars/*/; do
	[ -f "$dir/extension.toml" ] || continue
	name=$(basename "$dir")
	same "$dir/LICENSE" "grammar-$name-LICENSE.txt"
	for extra in "$dir"LICENSE-* "$dir"NOTICE; do
		[ -f "$extra" ] || continue
		same "$extra" "grammar-$name-$(basename "$extra").txt"
	done
done
found=$(find "$lic" -type f | wc -l | tr -d ' ')
[ "$found" -eq "$nlic" ] || fail "Contents/Resources/Licenses has $found files, expected $nlic"
[ "$nlic" -ge 50 ] || fail "only $nlic licence files checked"
ok "$nlic licence texts ship in Contents/Resources/Licenses, identical to their sources"

# C3: the bundle id is fe_config::paths::app_id()'s reverse DNS.
app_id=$(sed -n 's/.*AppId::new("\([^"]*\)".*/\1/p' "$root/crates/fe-config/src/paths.rs")
[ -n "$app_id" ] || fail "no AppId::new(\"...\") in crates/fe-config/src/paths.rs"
expect CFBundleIdentifier "$app_id"

# C4: all ten iconset sizes, with the 1024 render taken from the committed PNG.
iconutil -c iconset "$app/Contents/Resources/Tenmoku.icns" -o "$t/Tenmoku.iconset" ||
	fail "iconutil -c iconset"
for size in 16 32 128 256 512; do
	for name in "icon_${size}x${size}.png" "icon_${size}x${size}@2x.png"; do
		[ -f "$t/Tenmoku.iconset/$name" ] || fail "iconset lacks $name"
	done
done
count=$(find "$t/Tenmoku.iconset" -type f | wc -l | tr -d ' ')
[ "$count" -eq 10 ] || fail "iconset has $count files, expected 10"
ok "iconset has the ten sizes"
src="$root/crates/fastedit/assets/icon/fastedit-1024.png"
big="$t/Tenmoku.iconset/icon_512x512@2x.png"
for f in "$src" "$big"; do
	w=$(sips -g pixelWidth "$f" | awk '/pixelWidth/ { print $2 }')
	[ "$w" = 1024 ] || fail "$f is ${w}px wide, expected 1024"
done
# iconutil re-encodes the PNG, so compare decoded pixels (as BMP) rather than bytes.
sips -s format bmp "$src" --out "$t/src.bmp" >/dev/null || fail "sips could not decode $src"
sips -s format bmp "$big" --out "$t/big.bmp" >/dev/null || fail "sips could not decode $big"
cmp -s "$t/src.bmp" "$t/big.bmp" || fail "icon_512x512@2x.png is not the pixels of $src"
ok "1024 render is fastedit-1024.png"
[ -z "$(git -C "$root" ls-files '*.icns')" ] || fail "an .icns is committed"
ok "no .icns committed"

# C5: ad-hoc signature with the hardened runtime and the JIT entitlement.
check_signature() {
	codesign --verify --strict --deep "$1" || fail "codesign --verify --strict --deep"
	ok "codesign --verify --strict --deep"
	info=$(codesign -dvv "$1" 2>&1) || fail "codesign -dvv"
	echo "$info" | grep -q '^Signature=adhoc$' || fail "signature is not adhoc: $info"
	ok "Signature=adhoc"
	echo "$info" | grep -q '^CodeDirectory .*flags=.*runtime' || fail "no runtime flag: $info"
	ok "flags contain runtime"
	ents=$(codesign -d --entitlements - "$1" 2>&1) || fail "codesign -d --entitlements -"
	echo "$ents" | grep -q 'com.apple.security.cs.allow-jit' || fail "no allow-jit entitlement: $ents"
	ok "entitlement com.apple.security.cs.allow-jit"
}
check_signature "$app"

# C6: a missing binary is refused by name, --sign-only re-signs, --help documents the options.
if "$bundle" --binary /nonexistent --out "$t/missing" >"$t/missing.log" 2>&1; then
	fail "--binary /nonexistent exited 0"
fi
grep -q '/nonexistent' "$t/missing.log" || fail "missing-binary message does not name the path: $(cat "$t/missing.log")"
ok "--binary /nonexistent refused: $(cat "$t/missing.log")"

codesign --remove-signature "$app/Contents/MacOS/tenmoku"
if codesign --verify --strict --deep "$app" 2>/dev/null; then
	fail "bundle still verifies after removing the signature"
fi
"$bundle" --sign-only "$app" >"$t/sign.log" 2>&1 || { cat "$t/sign.log"; fail "--sign-only exited non-zero"; }
ok "--sign-only re-signed the bundle"
check_signature "$app"

help=$("$bundle" --help) || fail "--help exited non-zero"
for word in --binary --out --version --sign-only SIGN_IDENTITY; do
	echo "$help" | grep -q -- "$word" || fail "--help does not mention $word"
done
ok "--help documents --binary, --out, --version, --sign-only and SIGN_IDENTITY"

# C7: the person's signing and notarization steps, each named and in this order.
doc="$root/docs/release/macos-signing.md"
[ -f "$doc" ] || fail "docs/release/macos-signing.md missing"
prev=0
for step in "Developer ID Application certificate" "SIGN_IDENTITY= scripts/bundle-macos.sh" \
	"ditto -c -k --keepParent" "xcrun notarytool store-credentials" "xcrun notarytool submit ... --wait" \
	"xcrun stapler staple" "spctl -a -vv"; do
	line=$(grep -n -F -m 1 -- "$step" "$doc" | cut -d: -f1)
	[ -n "$line" ] || fail "macos-signing.md does not name '$step'"
	[ "$line" -gt "$prev" ] || fail "macos-signing.md names '$step' (line $line) before the step above it"
	prev=$line
done
ok "macos-signing.md names the seven steps in order"

# C8: opt-in scope check (see the header).
if [ -n "${SCOPE_BASE:-}" ]; then
	changed=$(git -C "$root" diff --name-only "$SCOPE_BASE" HEAD) || fail "git diff against $SCOPE_BASE"
	# A for loop, not a piped while, so fail exits the script rather than a subshell.
	for path in $changed; do
		case "$path" in
		TESTING.md | crates/fastedit/macos/Info.plist | crates/fastedit/macos/Tenmoku.entitlements | \
			docs/release/macos-signing.md | scripts/bundle-macos.sh | scripts/test-bundle-macos.sh | \
			docs/specs/26vab1rz3w-*.md | docs/decisions/26vab1rz3w-*.md) ;;
		*) fail "out-of-scope change since $SCOPE_BASE: $path" ;;
		esac
	done
	ok "only the bundling files changed since $SCOPE_BASE, no Rust file"
else
	echo "skip: scope check (set SCOPE_BASE=<rev> to run it)"
fi
echo ok
