#!/bin/sh
# Fixture test for Sparkle in bundle-macos.sh and for fetch-sparkle.sh: a fake Sparkle.framework
# (copies of /usr/bin/true, laid out as the release is) and no network, no cargo build.
# Prints one line per check and exits non-zero on the first failure. macOS only.
set -eu
unset SPARKLE_FRAMEWORK SPARKLE_URL_OVERRIDE TENMOKU_FEED_URL TENMOKU_ED_PUBLIC_KEY
# The fixture checks an ad-hoc signature, whatever identity the caller has exported.
SIGN_IDENTITY=-
export SIGN_IDENTITY
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
bundle="$here/bundle-macos.sh"
fetch="$here/fetch-sparkle.sh"
t=$(mktemp -d)
trap 'rm -rf "$t"' EXIT

fail() {
	echo "FAIL: $*"
	exit 1
}
ok() { echo "ok: $*"; }

# The release's layout, top-level symlinks included: XPCServices among them.
src="$t/src"
fw="$src/Sparkle.framework"
b="$fw/Versions/B"
mkdir -p "$b/Resources" "$b/Updater.app/Contents/MacOS" "$b/XPCServices/Installer.xpc/Contents/MacOS" "$src/bin"
cp /usr/bin/true "$b/Sparkle"
cp /usr/bin/true "$b/Autoupdate"
cp /usr/bin/true "$b/Updater.app/Contents/MacOS/Updater"
cp /usr/bin/true "$b/XPCServices/Installer.xpc/Contents/MacOS/Installer"
info_plist() {
	cat >"$1" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>$2</string>
<key>CFBundleIdentifier</key><string>test.$2</string>
<key>CFBundlePackageType</key><string>$3</string>
</dict></plist>
EOF
}
info_plist "$b/Resources/Info.plist" Sparkle FMWK
info_plist "$b/Updater.app/Contents/Info.plist" Updater APPL
ln -s B "$fw/Versions/Current"
for name in Sparkle Autoupdate Updater.app XPCServices Resources; do
	ln -s "Versions/Current/$name" "$fw/$name"
done
echo "fixture licence" >"$src/LICENSE"
cp /usr/bin/true "$src/bin/generate_keys"
cp /usr/bin/true "$src/bin/sign_update"
cp /usr/bin/true "$t/tenmoku"

feed='https://example.test/a/appcast.xml?x=1&y=2'
key=AbCd+/EfGh=

# Bundles into $t/$1 with the arguments after it; the app is $t/$1/Tenmoku.app. Callers that set
# a variable for it use a subshell: in sh, VAR=x before a function call outlasts the call.
build() {
	name=$1
	shift
	"$bundle" --binary "$t/tenmoku" --out "$t/$name" --version 1.2.3 "$@" >"$t/$name.log" 2>&1 ||
		{ cat "$t/$name.log"; fail "bundle-macos.sh $* exited non-zero"; }
	[ "$(tail -n 1 "$t/$name.log")" = "$t/$name/Tenmoku.app" ] ||
		fail "last line is not the app path: $(tail -n 1 "$t/$name.log")"
}
refused() {
	what=$1
	word=$2
	shift 2
	if "$@" >"$t/refused.log" 2>&1; then
		fail "$what was not refused"
	fi
	grep -q -F -- "$word" "$t/refused.log" || fail "$what: the message lacks '$word': $(cat "$t/refused.log")"
	ok "$what is refused: $(tail -n 1 "$t/refused.log")"
}
su_keys='SUFeedURL SUPublicEDKey SUEnableAutomaticChecks SUScheduledCheckInterval SUAutomaticallyUpdate'
no_su_keys() {
	shown=$(plutil -p "$1/Contents/Info.plist")
	for k in $su_keys; do
		! echo "$shown" | grep -q "\"$k\"" || fail "$2: Info.plist still has $k"
	done
	! grep -q '@FEED_URL@\|@ED_PUBLIC_KEY@' "$1/Contents/Info.plist" || fail "$2: a placeholder is left"
	plutil -lint -s "$1/Contents/Info.plist" || fail "$2: plutil -lint"
	ok "$2: none of the five SU* keys"
}
entitlements() {
	codesign -d --entitlements - "$1" 2>&1 || fail "codesign -d --entitlements - $1"
}
dlv=com.apple.security.cs.disable-library-validation

# The framework with feed and key.
(SPARKLE_FRAMEWORK=$fw build sparkle --feed-url "$feed" --ed-public-key "$key")
app="$t/sparkle/Tenmoku.app"
bundled="$app/Contents/Frameworks/Sparkle.framework"
[ -f "$bundled/Versions/B/Sparkle" ] || fail "no framework in Contents/Frameworks"
ok "the framework is in Contents/Frameworks"
[ ! -e "$bundled/Versions/B/XPCServices" ] || fail "Versions/B/XPCServices is still there"
[ ! -e "$bundled/XPCServices" ] && [ ! -L "$bundled/XPCServices" ] || fail "the XPCServices symlink is still there"
ok "XPCServices is gone, with its symlink"
[ -L "$bundled/Versions/Current" ] && [ -L "$bundled/Sparkle" ] || fail "ditto lost the framework's symlinks"
ok "the framework's symlinks survive"
cmp -s "$src/LICENSE" "$app/Contents/Resources/Sparkle-LICENSE.txt" || fail "no Contents/Resources/Sparkle-LICENSE.txt"
ok "Sparkle-LICENSE.txt is the tarball's"

check_sparkle_signature() {
	codesign --verify --strict --deep "$app" || fail "codesign --verify --strict --deep"
	ok "codesign --verify --strict --deep"
	for part in "$bundled/Versions/B/Autoupdate" "$bundled/Versions/B/Updater.app" "$bundled"; do
		codesign -dvv "$part" 2>&1 | grep -q 'flags=0x10002(adhoc,runtime)' ||
			fail "no flags=0x10002(adhoc,runtime) on $part: $(codesign -dvv "$part" 2>&1)"
		! entitlements "$part" | grep -q 'com.apple.security' || fail "$part has entitlements"
	done
	ok "Autoupdate, Updater.app and the framework: adhoc, runtime, no entitlements"
	ents=$(entitlements "$app")
	echo "$ents" | grep -q "$dlv" || fail "the ad-hoc app lacks $dlv: $ents"
	echo "$ents" | grep -q com.apple.security.cs.allow-jit || fail "the app lost allow-jit: $ents"
	ok "the ad-hoc app has $dlv and the committed entitlements"
	if grep -q "$dlv" "$root/crates/fastedit/macos/Tenmoku.entitlements"; then
		fail "Tenmoku.entitlements itself has $dlv"
	fi
	ok "Tenmoku.entitlements does not have it"
}
check_sparkle_signature

# C4: the five keys, with their values.
shown=$(plutil -p "$app/Contents/Info.plist")
for line in "\"SUFeedURL\" => \"$feed\"" "\"SUPublicEDKey\" => \"$key\"" '"SUEnableAutomaticChecks" => true' \
	'"SUScheduledCheckInterval" => 86400' '"SUAutomaticallyUpdate" => false'; do
	echo "$shown" | grep -q -F "$line" || fail "plutil -p lacks $line: $shown"
	ok "$line"
done
/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$app/Contents/Info.plist" | grep -qx 1.2.3 ||
	fail "CFBundleVersion is not the dotted version"
ok "CFBundleVersion is the dotted version"

# --sign-only re-signs a Sparkle bundle, as sign-notarize-macos.sh does.
codesign --remove-signature "$bundled/Versions/B/Autoupdate"
if codesign --verify --strict --deep "$app" 2>/dev/null; then
	fail "bundle still verifies after removing Autoupdate's signature"
fi
"$bundle" --sign-only "$app" >"$t/sign.log" 2>&1 || { cat "$t/sign.log"; fail "--sign-only exited non-zero"; }
ok "--sign-only re-signed the Sparkle bundle"
check_sparkle_signature

# sign-notarize-macos.sh with no secrets leaves it alone and verifies it.
env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin HOME="$HOME" TMPDIR="${TMPDIR:-/tmp}" \
	"$here/sign-notarize-macos.sh" "$app" "$t/notarize.zip" >"$t/notarize.log" 2>&1 ||
	{ cat "$t/notarize.log"; fail "sign-notarize-macos.sh with no secrets exited non-zero"; }
ok "sign-notarize-macos.sh with no secrets verifies the Sparkle bundle"

# A Developer ID identity, through a codesign that logs its real arguments and then signs ad hoc.
identity='Developer ID Application: Fixture (TEAM000000)'
mkdir "$t/shim"
cat >"$t/shim/codesign" <<EOF
#!/bin/sh
printf '%s\n' "\$*" >>"$t/codesign.log"
for arg; do
	shift
	case "\$arg" in
	--timestamp) set -- "\$@" --timestamp=none ;;
	"$identity") set -- "\$@" - ;;
	*) set -- "\$@" "\$arg" ;;
	esac
done
exec /usr/bin/codesign "\$@"
EOF
chmod +x "$t/shim/codesign"
PATH="$t/shim:$PATH" SIGN_IDENTITY=$identity "$bundle" --sign-only "$app" >"$t/devid.log" 2>&1 ||
	{ cat "$t/devid.log"; fail "--sign-only with a Developer ID exited non-zero"; }
grep -e --force "$t/codesign.log" >"$t/signed.log" || fail "the shim logged no signing"
[ "$(wc -l <"$t/signed.log" | tr -d ' ')" -eq 4 ] || fail "expected 4 signings: $(cat "$t/signed.log")"
n=0
for part in "$bundled/Versions/B/Autoupdate" "$bundled/Versions/B/Updater.app" "$bundled" "$app"; do
	n=$((n + 1))
	line=$(sed -n "${n}p" "$t/signed.log")
	case "$line" in
	"--force --options runtime --timestamp "*"-s $identity $part") ;;
	*) fail "signing $n is not of $part with the runtime, timestamp and identity: $line" ;;
	esac
done
ok "Developer ID signs inside out with the hardened runtime and a timestamp"
! grep -q -e --deep "$t/signed.log" || fail "a signing uses --deep: $(cat "$t/signed.log")"
ok "no signing uses --deep"
[ "$(grep -c -e --entitlements "$t/signed.log")" -eq 1 ] || fail "not only the app has entitlements: $(cat "$t/signed.log")"
grep -q -F -- "--entitlements $root/crates/fastedit/macos/Tenmoku.entitlements -s" "$t/signed.log" ||
	fail "the app is not signed with the committed entitlements: $(cat "$t/signed.log")"
ok "Developer ID keeps the committed entitlements, on the app only"
! entitlements "$app" | grep -q "$dlv" || fail "the Developer ID app has $dlv"
ok "the Developer ID app has no $dlv"

# C4: none of the keys when the feed or the key is empty; the variables work like the flags.
(SPARKLE_FRAMEWORK=$fw build no-feed --ed-public-key "$key")
no_su_keys "$t/no-feed/Tenmoku.app" "a key and no feed"
(SPARKLE_FRAMEWORK=$fw build no-key --feed-url "$feed")
no_su_keys "$t/no-key/Tenmoku.app" "a feed and no key"
(TENMOKU_FEED_URL=$feed TENMOKU_ED_PUBLIC_KEY=$key build env-empty --feed-url '')
no_su_keys "$t/env-empty/Tenmoku.app" "an empty --feed-url over the variables"
(TENMOKU_FEED_URL=$feed TENMOKU_ED_PUBLIC_KEY=$key build env)
plutil -p "$t/env/Tenmoku.app/Contents/Info.plist" | grep -q -F "\"SUFeedURL\" => \"$feed\"" ||
	fail "TENMOKU_FEED_URL did not fill SUFeedURL"
plutil -p "$t/env/Tenmoku.app/Contents/Info.plist" | grep -q -F "\"SUPublicEDKey\" => \"$key\"" ||
	fail "TENMOKU_ED_PUBLIC_KEY did not fill SUPublicEDKey"
ok "TENMOKU_FEED_URL and TENMOKU_ED_PUBLIC_KEY fill the keys"

# No framework asked for: no trace of Sparkle.
build plain
plain="$t/plain/Tenmoku.app"
no_su_keys "$plain" "no feed, no framework"
[ ! -e "$plain/Contents/Frameworks" ] || fail "a Sparkle-free bundle has Contents/Frameworks"
[ ! -e "$plain/Contents/Resources/Sparkle-LICENSE.txt" ] || fail "a Sparkle-free bundle has the licence"
! entitlements "$plain" | grep -q "$dlv" || fail "a Sparkle-free ad-hoc bundle has $dlv"
ok "without Sparkle: no Frameworks, no licence, no $dlv"

# Refusals, none of which may touch the bundle already there.
marker="$t/plain/Tenmoku.app/Contents/marker"
touch "$marker"
for bad in http://example.test/appcast.xml https:// ftp://example.test/a 'https://example.test/a b'; do
	refused "the feed URL '$bad'" 'feed URL' \
		"$bundle" --binary "$t/tenmoku" --out "$t/plain" --feed-url "$bad" --ed-public-key "$key"
done
refused "an http feed URL with no key" 'must be https' \
	"$bundle" --binary "$t/tenmoku" --out "$t/plain" --feed-url http://example.test/a
refused "an http TENMOKU_FEED_URL" 'must be https' \
	env TENMOKU_FEED_URL=http://example.test/a TENMOKU_ED_PUBLIC_KEY=$key \
	"$bundle" --binary "$t/tenmoku" --out "$t/plain"
mkdir -p "$t/nolicence"
cp -R "$fw" "$t/nolicence/"
refused "a framework with no licence beside it" LICENSE \
	env SPARKLE_FRAMEWORK="$t/nolicence/Sparkle.framework" "$bundle" --binary "$t/tenmoku" --out "$t/plain"
refused "a SPARKLE_FRAMEWORK that is not one" 'not a Sparkle.framework' \
	env SPARKLE_FRAMEWORK="$t/nonexistent" "$bundle" --binary "$t/tenmoku" --out "$t/plain"
[ -e "$marker" ] || fail "a refused run replaced the existing bundle"
ok "a refused run leaves the existing bundle"
for opt in --sparkle '--feed-url https://example.test/a' '--ed-public-key k'; do
	# shellcheck disable=SC2086
	refused "--sign-only with $opt" 'no other options' "$bundle" --sign-only "$plain" $opt
done

# --sparkle goes through fetch-sparkle.sh, which SPARKLE_FRAMEWORK short-circuits.
(SPARKLE_FRAMEWORK=$fw build flag --sparkle)
[ -f "$t/flag/Tenmoku.app/Contents/Frameworks/Sparkle.framework/Versions/B/Sparkle" ] ||
	fail "--sparkle with SPARKLE_FRAMEWORK embedded nothing"
ok "--sparkle takes SPARKLE_FRAMEWORK"
[ "$(SPARKLE_FRAMEWORK=$fw SPARKLE_URL_OVERRIDE=file:///nonexistent "$fetch")" = "$fw" ] ||
	fail "fetch-sparkle.sh does not print SPARKLE_FRAMEWORK"
ok "fetch-sparkle.sh with SPARKLE_FRAMEWORK prints it and downloads nothing"

# fetch-sparkle.sh: a tarball whose SHA-256 is not the pinned one is refused.
tarball="$t/Sparkle.tar.xz"
tar -cJf "$tarball" -C "$src" ./Sparkle.framework ./LICENSE ./bin || fail "could not build the fixture tarball"
sha=$(shasum -a 256 "$tarball" | awk '{ print $1 }')
refused "a tarball with the wrong SHA-256" 'SHA-256 mismatch' \
	env SPARKLE_URL_OVERRIDE="file://$tarball" "$fetch" --out "$t/mismatch"
[ ! -e "$t/mismatch/Sparkle.framework" ] || fail "the mismatch extracted a framework"
[ -z "$(ls -A "$t/mismatch")" ] || fail "the mismatch left files: $(ls -A "$t/mismatch")"
ok "the mismatch leaves nothing behind"
refused "a failed download" 'could not download' \
	env SPARKLE_URL_OVERRIDE="file://$t/nonexistent" "$fetch" --out "$t/nodownload"

# The accepting path, through a copy pinned to the fixture: the real script has no way to
# override its checksum, and shouldn't.
sed "s/^SHA256=.*/SHA256=$sha/" "$fetch" >"$t/fetch-pinned.sh"
chmod +x "$t/fetch-pinned.sh"
got=$(SPARKLE_URL_OVERRIDE="file://$tarball" "$t/fetch-pinned.sh" --out "$t/cache" 2>"$t/fetch.log") ||
	{ cat "$t/fetch.log"; fail "fetching a matching tarball failed"; }
[ "$got" = "$t/cache/Sparkle.framework" ] || fail "fetch-sparkle.sh printed '$got'"
[ -L "$t/cache/Sparkle.framework/Versions/Current" ] || fail "extraction lost the framework's symlinks"
[ -x "$t/cache/bin/generate_keys" ] && [ -x "$t/cache/bin/sign_update" ] && [ -f "$t/cache/LICENSE" ] ||
	fail "generate_keys, sign_update or LICENSE not extracted: $(ls -R "$t/cache")"
ok "a matching tarball extracts the framework, tools and licence and prints the framework"
touch "$t/cache/Sparkle.framework/damage"
got=$(SPARKLE_URL_OVERRIDE="file://$t/nonexistent" "$t/fetch-pinned.sh" --out "$t/cache" 2>"$t/fetch.log") ||
	{ cat "$t/fetch.log"; fail "the verified cache was not reused"; }
[ "$got" = "$t/cache/Sparkle.framework" ] || fail "the second run printed '$got'"
[ ! -e "$t/cache/Sparkle.framework/damage" ] || fail "the second run kept a changed framework"
ok "a second run reuses the verified tarball, with no download, and extracts it afresh"
echo tampered >>"$t/cache/Sparkle-$(sed -n 's/^VERSION=//p' "$fetch").tar.xz"
refused "a tampered cache with no good download" 'could not download' \
	env SPARKLE_URL_OVERRIDE="file://$t/nonexistent" "$t/fetch-pinned.sh" --out "$t/cache"
got=$(SPARKLE_URL_OVERRIDE="file://$tarball" "$t/fetch-pinned.sh" --out "$t/cache" 2>"$t/fetch.log") ||
	{ cat "$t/fetch.log"; fail "a tampered cache was not replaced"; }
ok "a tampered cache is discarded and downloaded again"

# The pin, the docs and the third-party row name the same release.
version=$(sed -n 's/^VERSION=//p' "$fetch")
pinned=$(sed -n 's/^SHA256=//p' "$fetch")
row=$(grep '^| Sparkle |' "$root/THIRD_PARTY.md") || fail "THIRD_PARTY.md has no Sparkle row"
for word in "$version" "$pinned" MIT Contents/Resources/Sparkle-LICENSE.txt; do
	echo "$row" | grep -q -F -- "$word" || fail "the Sparkle row lacks $word: $row"
done
ok "the THIRD_PARTY.md row has the pinned version and SHA-256"
section=$(awk '/^## / { on = ($0 ~ /^## Sparkle/) } on' "$root/docs/release/macos-signing.md")
for word in fetch-sparkle.sh Autoupdate Updater.app SUFeedURL SUPublicEDKey disable-library-validation; do
	echo "$section" | grep -q -F -- "$word" || fail "the Sparkle section of macos-signing.md lacks $word"
done
ok "macos-signing.md has a Sparkle section"
help=$("$bundle" --help) || fail "--help exited non-zero"
for word in --sparkle --feed-url --ed-public-key SPARKLE_FRAMEWORK TENMOKU_FEED_URL TENMOKU_ED_PUBLIC_KEY; do
	echo "$help" | grep -q -- "$word" || fail "--help does not mention $word"
done
ok "--help documents the Sparkle options"
echo ok
