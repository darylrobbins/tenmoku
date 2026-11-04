#!/bin/sh
# Checks for the release workflow (.github/workflows/release.yml) and its signing script:
# actionlint, the workflow's trigger, runner, tag check, build and publish steps, where its secrets and
# write permission appear, the signing script's hygiene, a run of the signing script with no
# secrets against a fixture bundle (a copy of /usr/bin/true, as test-bundle-macos.sh does), the
# Sparkle steps, and a run of make-appcast.sh on that bundle's zip.
# Prints one line per check and exits non-zero on the first failure. macOS only; needs
# actionlint (brew install actionlint).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
wf="$root/.github/workflows/release.yml"
sign="$here/sign-notarize-macos.sh"
t=$(mktemp -d)
trap 'rm -rf "$t"' EXIT

fail() {
	echo "FAIL: $*"
	exit 1
}
ok() { echo "ok: $*"; }
has() { grep -q -E -- "$1" "$wf" || fail "release.yml lacks /$1/ ($2)"; }

# C1: actionlint.
command -v actionlint >/dev/null 2>&1 || fail "actionlint not installed (brew install actionlint)"
actionlint "$wf" || fail "actionlint .github/workflows/release.yml"
ok "actionlint"

# C3: trigger, runner, build, bundle, sign, publish.
has "^  push:$" "push trigger"
has "^    tags: \['v\*'\]$" "v* tags"
has "^  workflow_dispatch:" "manual trigger"
has "^    runs-on: macos-latest$" "arm64 runner"
has "run: cargo build --release -p fastedit --locked$" "release build"
has "scripts/bundle-macos.sh" "bundling"
has "run: scripts/sign-notarize-macos.sh " "signing"
has "gh release create " "release creation"
has "gh release upload " "upload to an existing release"
ok "trigger, runner, build, bundle, sign and gh release steps"

# A tag must name the version the binary is built with: Check for Updates compares that one.
check="$here/check-release-version.sh"
has 'run: scripts/check-release-version.sh "\$GITHUB_REF_NAME"$' "tag and Cargo version check"
awk '
	/check-release-version.sh/ && !checked { checked = NR }
	/cargo build --release/ && !built { built = NR }
	END { exit !(checked && built && checked < built) }
' "$wf" || fail "the tag is not checked before the build"
version=$(awk '
	/^\[/ { in_pkg = ($0 == "[workspace.package]") }
	in_pkg && /^version[ \t]*=/ { gsub(/.*=[ \t]*"|".*/, ""); print; exit }
' "$root/Cargo.toml")
[ -n "$version" ] || fail "no [workspace.package] version in Cargo.toml"
"$check" "v$version" >/dev/null || fail "the tag v$version was refused at Cargo.toml's version"
for tag in "v$version.1" "v9$version" "$version" "v$version-rc1" v "" v1.2; do
	if "$check" "$tag" >/dev/null 2>&1; then
		fail "check-release-version.sh took the tag '$tag' at Cargo.toml's version $version"
	fi
done
ok "a tag must be v plus Cargo.toml's version, checked before the build"

# contents: write once, in the release job's own permissions (job keys sit at 4 spaces).
[ "$(grep -c 'contents: write' "$wf")" -eq 1 ] || fail "contents: write appears other than once"
awk '
	/^  [A-Za-z_-]+:/ { job = $1 }
	/^    permissions:/ { perms = (job == "release:") ; next }
	/^    [A-Za-z_-]+:/ { perms = 0 }
	/contents: write/ { found = 1; if (!perms || $0 !~ /^      contents: write$/) bad = 1 }
	END { exit !(found && !bad) }
' "$wf" || fail "contents: write is not under jobs.release.permissions"
ok "contents: write only in the release job's permissions"

# No secret inside a run: block; each secret is an env-style KEY: \${{ secrets.KEY }} line.
awk '
	{ indent = match($0, /[^ ]/) - 1 }
	inrun && ($0 ~ /^ *$/ || indent > runindent) { if ($0 ~ /secrets\./) bad = 1; next }
	{ inrun = 0 }
	/^ *(- )?run:/ { inrun = 1; runindent = index($0, "run:") - 1; if ($0 ~ /secrets\./) bad = 1 }
	END { exit bad }
' "$wf" || fail "a secret appears inside a run: block"
if grep -n 'secrets\.' "$wf" | grep -v -E '^[0-9]+:          [A-Z_]+: \$\{\{ secrets\.[A-Z_]+ \}\}$'; then
	fail "the lines above use a secret other than as a step env: value"
fi
for s in MACOS_CERTIFICATE MACOS_CERTIFICATE_PASSWORD MACOS_SIGN_IDENTITY APPLE_ID APPLE_TEAM_ID \
	APPLE_APP_PASSWORD; do
	grep -q -F "$s: \${{ secrets.$s }}" "$wf" || fail "release.yml does not pass secret $s"
done
ok "the six secrets reach the signing step through env: only"
[ "$(grep -c 'secrets\.SPARKLE_ED_PRIVATE_KEY' "$wf")" -eq 1 ] ||
	fail "secrets.SPARKLE_ED_PRIVATE_KEY appears other than once"
grep -q -F 'SPARKLE_ED_PRIVATE_KEY: ${{ secrets.SPARKLE_ED_PRIVATE_KEY }}' "$wf" ||
	fail "release.yml does not pass secret SPARKLE_ED_PRIVATE_KEY"
ok "SPARKLE_ED_PRIVATE_KEY reaches one step through env: only"

# The private key goes to sign_update on stdin and nowhere else. Its only two reads are the
# skip test and the printf into the pipe.
sign_line='signature=$(printf '"'"'%s'"'"' "$SPARKLE_ED_PRIVATE_KEY" | "$SPARKLE_TOOLS/sign_update" --ed-key-file - -p "dist/$name")'
grep -q -F -- "$sign_line" "$wf" || fail "sign_update does not get the key on stdin as expected"
[ "$(grep -c 'sign_update' "$wf")" -eq 1 ] || fail "sign_update appears other than once"
reads=$(grep -E '\$\{?SPARKLE_ED_PRIVATE_KEY' "$wf" | grep -v -F -- "$sign_line" |
	grep -v -x -F -- '          if [ -z "$SPARKLE_ED_PRIVATE_KEY" ]; then' || true)
[ -z "$reads" ] || fail "the private key is read other than on sign_update's stdin: $reads"
if grep -n 'set -x' "$wf"; then
	fail "release.yml traces its shell (line above)"
fi
ok "sign_update gets the key on stdin only, never as an argument or in an echo"
if grep -n -E 'sign_update|PRIVATE_KEY' "$here/make-appcast.sh"; then
	fail "make-appcast.sh must not touch the private key (lines above)"
fi
ok "make-appcast.sh does not touch the private key"

# The Sparkle steps, in order: fetch, bundle, zip, skip test, sign, appcast, publish.
in_order() {
	awk -v a="$1" -v b="$2" '
		index($0, a) && !x { x = NR }
		index($0, b) && !y { y = NR }
		END { exit !(x && y && x < y) }
	' "$wf" || fail "$3"
}
in_order 'scripts/fetch-sparkle.sh' 'scripts/bundle-macos.sh' "Sparkle is not fetched before bundling"
in_order 'ditto -c -k' 'sign_update' "the zip is not made before it is signed"
in_order 'skipping Sparkle signing' 'sign_update' "the skip does not come before signing"
in_order 'sign_update' 'scripts/make-appcast.sh' "the appcast is not written after signing"
in_order 'scripts/make-appcast.sh' 'gh release' "the appcast is not written before publishing"
ok "fetch, bundle, zip, sign, appcast and publish run in that order"
# The bundle step's env:, from its id line to its run: line.
bundle_env=$(awk '/^        id: bundle$/ { on = 1 } on && /^        run:/ { exit } on' "$wf")
for line in 'SPARKLE_FRAMEWORK: ${{ steps.sparkle.outputs.framework }}' \
	"TENMOKU_FEED_URL: \${{ vars.TENMOKU_FEED_URL || format('https://github.com/{0}/releases/latest/download/appcast.xml', github.repository) }}" \
	'TENMOKU_ED_PUBLIC_KEY: ${{ vars.SPARKLE_ED_PUBLIC_KEY }}'; do
	echo "$bundle_env" | grep -q -F -- "$line" || fail "the bundle step's env: lacks $line"
done
ok "the bundle step gets the framework, the feed URL and the public key"
has "vars\.DOWNLOAD_URL_PREFIX \|\| format\('https://github.com/\{0\}/releases/download/v\{1\}'" "download URL default"
# dist/* in both gh release commands is what carries appcast.xml.
has "--out dist/appcast.xml" "the appcast is written into dist/"
has 'gh release upload "\$GITHUB_REF_NAME" dist/\* ' "appcast.xml goes up with an existing release"
has 'gh release create "\$GITHUB_REF_NAME" dist/\* ' "appcast.xml goes up with a new release"
ok "appcast.xml is written to dist/ and uploaded with the zip"

# The real bodies of the sign and publish steps, against a fake gh that logs what it is asked.
step_body() {
	awk -v name="- name: $1" '
		index($0, name) { on = 1 }
		on && /^        run: \|$/ { body = 1; next }
		body && /^      - / { exit }
		body { sub(/^          /, ""); print }
	' "$wf"
}
step_body "Sign the zip for Sparkle" >"$t/sign-step.sh"
step_body "Publish the GitHub Release" >"$t/publish-step.sh"
[ -s "$t/sign-step.sh" ] && [ -s "$t/publish-step.sh" ] || fail "could not extract the sign and publish steps"
mkdir "$t/bin"
cat >"$t/bin/gh" <<'EOF'
#!/bin/sh
case "$*" in
"release view "*" --json "*)
	[ -z "$FAKE_ASSETS_FAIL" ] || exit 1
	printf '%s\n' "$FAKE_ASSETS"
	;;
"release view "*) exit "$FAKE_VIEW_RC" ;;
*) echo "$*" >>"$FAKE_GH_LOG" ;;
esac
EOF
chmod +x "$t/bin/gh"
# Runs the publish step in a fresh dist/ holding the zip, and appcast.xml when $1 is "appcast".
publish() {
	rm -rf "$t/run" "$t/gh.log"
	mkdir -p "$t/run/dist"
	: >"$t/gh.log"
	: >"$t/run/dist/Tenmoku-1.2.3-macos-arm64.zip"
	[ "$1" != appcast ] || : >"$t/run/dist/appcast.xml"
	(cd "$t/run" && PATH="$t/bin:$PATH" GITHUB_REF_NAME=v1.2.3 FAKE_GH_LOG="$t/gh.log" bash -e "$t/publish-step.sh") \
		>"$t/publish.log" 2>&1
}

# A signed release, then a rerun with the secret gone: the sign step skips, the publish refuses.
FAKE_VIEW_RC=0 FAKE_ASSETS='Tenmoku-1.2.3-macos-arm64.zip
appcast.xml' FAKE_ASSETS_FAIL=''
export FAKE_VIEW_RC FAKE_ASSETS FAKE_ASSETS_FAIL
SPARKLE_ED_PRIVATE_KEY='' bash -e "$t/sign-step.sh" >"$t/skip.log" 2>&1 || fail "the sign step did not skip on an empty secret"
grep -q 'skipping Sparkle signing' "$t/skip.log" || fail "no skip line: $(cat "$t/skip.log")"
ok "an empty secret skips Sparkle signing: $(cat "$t/skip.log")"
if publish none; then
	fail "a rerun with no appcast over a release that has one was not refused"
fi
grep -q 'has an appcast.xml and this run made none' "$t/publish.log" || fail "no reason given: $(cat "$t/publish.log")"
[ ! -s "$t/gh.log" ] || fail "the refused rerun still changed the release: $(cat "$t/gh.log")"
ok "a rerun with no appcast over a signed release is refused and uploads nothing"

publish appcast || { cat "$t/publish.log"; fail "a signed rerun of a signed release failed"; }
grep -q '^release upload v1.2.3 .*dist/appcast.xml .*--clobber' "$t/gh.log" || fail "the signed rerun: $(cat "$t/gh.log")"
ok "a signed rerun replaces appcast.xml with the zip"

FAKE_ASSETS='Tenmoku-1.2.3-macos-arm64.zip'
publish none || { cat "$t/publish.log"; fail "an unsigned rerun of an unsigned release failed"; }
grep -q '^release upload v1.2.3 ' "$t/gh.log" || fail "the unsigned rerun: $(cat "$t/gh.log")"
ok "an unsigned rerun of an unsigned release still uploads"

FAKE_ASSETS_FAIL=1
if publish none; then
	fail "the upload went ahead though the release's assets could not be listed"
fi
[ ! -s "$t/gh.log" ] || fail "an unlisted release was changed: $(cat "$t/gh.log")"
ok "a release whose assets cannot be listed is left alone"
FAKE_ASSETS_FAIL=''

FAKE_VIEW_RC=1
publish appcast || { cat "$t/publish.log"; fail "a new signed release failed"; }
grep -q '^release create v1.2.3 .*dist/appcast.xml ' "$t/gh.log" || fail "the new release: $(cat "$t/gh.log")"
ok "a new release is created with appcast.xml"

# C5: no tracing, no echo of secret-named variables.
if grep -n -E "set -x|echo.*(PASSWORD|CERTIFICATE|APPLE_APP)" "$sign"; then
	fail "sign-notarize-macos.sh traces or echoes a secret-named variable (lines above)"
fi
ok "sign-notarize-macos.sh has no set -x and echoes no secret-named variable"
grep -q 'echo "skipping notarization: ' "$sign" || fail "no notarization skip line in sign-notarize-macos.sh"
ok "the notarize step has its own skip line"

# C4: with no secrets the script skips, exits 0 and leaves a verifiable ad-hoc bundle.
cp /usr/bin/true "$t/tenmoku"
SIGN_IDENTITY=- "$here/bundle-macos.sh" --binary "$t/tenmoku" --out "$t/out" --version 1.2.3 \
	--feed-url https://example.test/appcast.xml --ed-public-key AbCd= >"$t/bundle.log" 2>&1 ||
	{ cat "$t/bundle.log"; fail "bundle-macos.sh exited non-zero"; }
app="$t/out/Tenmoku.app"
env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin HOME="$HOME" TMPDIR="${TMPDIR:-/tmp}" \
	"$sign" "$app" "$t/notarize.zip" >"$t/sign.log" 2>&1 ||
	{ cat "$t/sign.log"; fail "sign-notarize-macos.sh with no secrets exited non-zero"; }
grep -q "skipping Developer ID signing and notarization" "$t/sign.log" ||
	fail "no skip line: $(cat "$t/sign.log")"
ok "no secrets: $(head -n 1 "$t/sign.log")"
# An empty secret, as Actions passes for a missing one, counts as absent.
MACOS_CERTIFICATE='' "$sign" "$app" "$t/notarize.zip" >"$t/empty.log" 2>&1 ||
	{ cat "$t/empty.log"; fail "sign-notarize-macos.sh with an empty certificate exited non-zero"; }
grep -q "skipping Developer ID signing and notarization" "$t/empty.log" || fail "empty secret not skipped"
ok "an empty certificate secret is skipped too"
[ ! -e "$t/notarize.zip" ] || fail "the skip path wrote a zip"
codesign --verify --strict "$app" || fail "codesign --verify --strict after the skip"
codesign -dvv "$app" 2>&1 | grep -q '^Signature=adhoc$' || fail "the bundle is no longer ad-hoc signed"
ok "the bundle stays ad-hoc signed and verifies"

if "$sign" "$t" "$t/notarize.zip" >"$t/bad.log" 2>&1; then
	fail "a non-bundle path exited 0"
fi
ok "a non-bundle path is refused: $(cat "$t/bad.log")"

# make-appcast.sh on the zip the workflow would make from that bundle.
ditto -c -k --keepParent "$app" "$t/app.zip"
"$here/make-appcast.sh" --zip "$t/app.zip" --version 1.2.3 --download-url https://example.test/app.zip \
	--signature AbCd== --out "$t/appcast.xml" >"$t/appcast.log" 2>&1 ||
	{ cat "$t/appcast.log"; fail "make-appcast.sh on the fixture zip exited non-zero"; }
appcast() { xmllint --xpath "string(//*[local-name()='$1']$2)" "$t/appcast.xml"; }
min=$(/usr/libexec/PlistBuddy -c 'Print :LSMinimumSystemVersion' "$app/Contents/Info.plist")
[ "$(appcast version '')" = 1.2.3 ] || fail "the appcast's version is $(appcast version '')"
[ "$(appcast minimumSystemVersion '')" = "$min" ] || fail "the appcast's minimum system is not the bundle's $min"
[ "$(appcast enclosure /@length)" = "$(wc -c <"$t/app.zip" | tr -d ' ')" ] ||
	fail "the appcast's length is not the zip's size"
ok "make-appcast.sh takes the version and minimum system from a real bundle's zip"

# C6: the Releasing section names the secrets and the tag format.
doc="$root/docs/release/macos-signing.md"
section=$(awk '/^## / { on = ($0 ~ /^## Releasing/) } on' "$doc")
[ -n "$section" ] || fail "macos-signing.md has no Releasing section"
for word in MACOS_CERTIFICATE MACOS_CERTIFICATE_PASSWORD MACOS_SIGN_IDENTITY APPLE_ID APPLE_TEAM_ID \
	APPLE_APP_PASSWORD vX.Y.Z; do
	echo "$section" | grep -q -F -- "$word" || fail "the Releasing section does not name $word"
done
ok "the Releasing section names the six secrets and vX.Y.Z"
for word in SPARKLE_ED_PRIVATE_KEY SPARKLE_ED_PUBLIC_KEY TENMOKU_FEED_URL DOWNLOAD_URL_PREFIX appcast.xml reinstall; do
	echo "$section" | grep -q -F -- "$word" || fail "the Releasing section does not name $word"
done
ok "the Releasing section names Sparkle's secret, variables and appcast, and what key loss means"
echo ok
