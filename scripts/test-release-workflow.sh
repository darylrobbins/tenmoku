#!/bin/sh
# Checks for the release workflow (.github/workflows/release.yml) and its signing script:
# actionlint, the workflow's trigger, runner, build and publish steps, where its secrets and
# write permission appear, the signing script's hygiene, and a run of the signing script with
# no secrets against a fixture bundle (a copy of /usr/bin/true, as test-bundle-macos.sh does).
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

# C5: no tracing, no echo of secret-named variables.
if grep -n -E "set -x|echo.*(PASSWORD|CERTIFICATE|APPLE_APP)" "$sign"; then
	fail "sign-notarize-macos.sh traces or echoes a secret-named variable (lines above)"
fi
ok "sign-notarize-macos.sh has no set -x and echoes no secret-named variable"
grep -q 'echo "skipping notarization: ' "$sign" || fail "no notarization skip line in sign-notarize-macos.sh"
ok "the notarize step has its own skip line"

# C4: with no secrets the script skips, exits 0 and leaves a verifiable ad-hoc bundle.
cp /usr/bin/true "$t/tenmoku"
SIGN_IDENTITY=- "$here/bundle-macos.sh" --binary "$t/tenmoku" --out "$t/out" --version 1.2.3 >"$t/bundle.log" 2>&1 ||
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

# C6: the Releasing section names the secrets and the tag format.
doc="$root/docs/release/macos-signing.md"
section=$(awk '/^## / { on = ($0 ~ /^## Releasing/) } on' "$doc")
[ -n "$section" ] || fail "macos-signing.md has no Releasing section"
for word in MACOS_CERTIFICATE MACOS_CERTIFICATE_PASSWORD MACOS_SIGN_IDENTITY APPLE_ID APPLE_TEAM_ID \
	APPLE_APP_PASSWORD vX.Y.Z; do
	echo "$section" | grep -q -F -- "$word" || fail "the Releasing section does not name $word"
done
ok "the Releasing section names the six secrets and vX.Y.Z"
echo ok
