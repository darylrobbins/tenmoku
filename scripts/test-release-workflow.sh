#!/bin/sh
# Checks for the release workflow (.github/workflows/release.yml) and its signing script:
# actionlint, the workflow's trigger, its build jobs (runners, smoke steps, artifacts) and publish
# job (needs, tag pushes only), tag check, build and publish steps, where its secrets and
# write permission appear, the Windows and Linux packaging scripts (a run of package-linux.sh on
# a fixture binary), the signing script's hygiene, a run of the signing script with no
# secrets against a fixture bundle (a copy of /usr/bin/true, as test-bundle-macos.sh does), the
# Sparkle steps, a run of make-appcast.sh on that bundle's zip, and the Velopack (vpk) steps: vpk
# at the crate's pin, the Setup.exe and AppImage packs (velopack-pack.sh --linux --dry-run runs),
# their smoke steps, the feeds' signing key and the publish guard for their .sig files.
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

# contents: write once, in the publish job's own permissions (job keys sit at 4 spaces).
[ "$(grep -c 'contents: write' "$wf")" -eq 1 ] || fail "contents: write appears other than once"
awk '
	/^  [A-Za-z0-9_-]+:/ { job = $1 }
	/^    permissions:/ { perms = (job == "publish:") ; next }
	/^    [A-Za-z_-]+:/ { perms = 0 }
	/contents: write/ { found = 1; if (!perms || $0 !~ /^      contents: write$/) bad = 1 }
	END { exit !(found && !bad) }
' "$wf" || fail "contents: write is not under jobs.publish.permissions"
grep -q -x 'permissions:' "$wf" && grep -A1 -x 'permissions:' "$wf" | grep -q -x '  contents: read' ||
	fail "the workflow's default permissions are not contents: read"
ok "contents: write only in the publish job's permissions; the rest read"

# The jobs: one per platform, then publish. job NAME prints that job's lines.
job() { awk -v j="  $1:" '/^jobs:$/ { in_jobs = 1 } in_jobs && $0 == j { on = 1; next } on && /^  [^ ]/ { exit } on' "$wf"; }
jobs=$(awk '/^jobs:$/ { on = 1; next } on && /^  [A-Za-z0-9_-]+:$/' "$wf" | tr -d ' :' | tr '\n' ' ')
[ "$jobs" = "version macos-arm64 windows linux-x86_64 publish " ] || fail "the jobs are: $jobs"
job_has() { job "$1" | grep -q -E -- "$2" || fail "job $1 lacks /$2/ ($3)"; }
job_has macos-arm64 "^    runs-on: macos-latest$" "Apple Silicon runner"
job_has windows "^    name: windows-\\\$\{\{ matrix.arch \}\}$" "a job per arch, named windows-<arch>"
job_has windows "^    runs-on: \\\$\{\{ matrix.runner \}\}$" "the arch's runner"
job "windows" | awk '
	/- arch: x64$/ { a = "x64" } /- arch: arm64$/ { a = "arm64" }
	/runner:/ { r[a] = $2 } /target:/ { t[a] = $2 }
	END { exit !(r["x64"] == "windows-2025" && t["x64"] == "x86_64-pc-windows-msvc" &&
		r["arm64"] == "windows-11-arm" && t["arm64"] == "aarch64-pc-windows-msvc") }
' || fail "the windows matrix is not x64 on windows-2025 and arm64 on windows-11-arm"
job_has windows "cargo build --release -p fastedit --locked --target \\\$\{\{ matrix.target \}\} --bin tenmoku --example sign_update_feed$" "the arch's build"
job_has windows "scripts/package-windows.ps1 -Arch " "packaging"
job_has linux-x86_64 "^    runs-on: ubuntu-24.04$" "x86_64 runner"
job_has linux-x86_64 "run: cargo build --release -p fastedit --locked --bin tenmoku --example sign_update_feed$" "the release build"
job_has linux-x86_64 "scripts/package-linux.sh " "packaging"
job_has linux-x86_64 "mesa-vulkan-drivers xvfb$" "Xvfb and software Vulkan"
ok "jobs version, macos-arm64, windows (x64 on windows-2025, arm64 on windows-11-arm), linux-x86_64 on ubuntu-24.04, publish"

# Each build job needs the version, smoke-tests its binary before uploading, and uploads its
# files as an artifact on every run, the dispatch included.
for j in macos-arm64 windows linux-x86_64; do
	body=$(job "$j")
	echo "$body" | grep -q -x '    needs: version' || fail "$j does not need the version job"
	smoke=$(echo "$body" | awk '/^      - name: Smoke test/ { on = 1; print; next } on && /^      - / { exit } on')
	[ -n "$smoke" ] || fail "$j has no smoke step"
	echo "$smoke" | grep -q -e '--version' || fail "$j's smoke step does not run --version"
	echo "$smoke" | grep -q 'tenmoku $VERSION\|tenmoku $env:VERSION' || fail "$j's smoke step does not compare the version"
	echo "$smoke" | grep -q 'if:' && fail "$j's smoke step is conditional"
	echo "$body" | awk '/Smoke test/ && !s { s = NR } /actions\/upload-artifact@/ && !u { u = NR } END { exit !(s && u && s < u) }' ||
		fail "$j does not smoke-test before uploading"
	upload=$(echo "$body" | awk '/^      - name: Upload/ { on = 1 } on')
	echo "$upload" | grep -q 'if:' && fail "$j's upload is conditional: a dispatch must upload too"
	echo "$upload" | grep -q -E '^          name: Tenmoku-\$\{\{ (needs\.version|steps\.bundle)\.outputs\.version \}\}-' ||
		fail "$j's artifact is not named Tenmoku-<v>-..."
	echo "$upload" | grep -q -x '          if-no-files-found: error' || fail "$j's upload allows no files"
done
job windows | grep -q -F "throw \"tenmoku.exe is for machine" || fail "the windows smoke step does not check the exe's machine"
job windows | grep -q -x '          RUSTFLAGS: -C target-feature=+crt-static' ||
	fail "the windows build does not link the C runtime statically"
job windows | grep -q -F '& $dumpbin /nologo /dependents $exe' || fail "the windows smoke step does not list the exe's DLLs"
job windows | grep -q -F "throw \"tenmoku.exe needs the Visual C++ runtime" ||
	fail "the windows smoke step does not refuse a Visual C++ runtime import"
linux_smoke=$(job linux-x86_64 | awk '/- name: Smoke test/ { on = 1 } on && /- name: Upload/ { exit } on')
for word in 'Xvfb :99' 'DISPLAY=:99' 'sleep 5' 'kill -0 "$app"' "grep -n 'panicked'"; do
	echo "$linux_smoke" | grep -q -F -- "$word" || fail "the linux smoke step lacks $word"
done
job linux-x86_64 | grep -q -F 'glibc=$glibc' || fail "the linux job does not output its minimum glibc"
ok "each build job smoke-tests --version (linux also a 5 s launch under Xvfb) before uploading its artifact"

# Velopack: vpk at the velopack crate's pin, in the Windows and Linux jobs, packing what the
# portable smoke step checked into Setup.exe or the AppImage and its feed, then installing or
# launching that before the upload.
pin=$(sed -n 's/^velopack = { version = "=\([^"]*\)".*/\1/p' "$root/Cargo.toml")
[ -n "$pin" ] || fail "no pinned velopack version in Cargo.toml"
feed_default="\${{ vars.TENMOKU_UPDATE_FEED || format('https://github.com/{0}/releases/latest/download', github.repository) }}"
# step JOB NAME prints that step of the job, from its name line to the next step.
step() { job "$1" | awk -v n="      - name: $2" 'index($0, n) == 1 { on = 1; print; next } on && /^      - / { exit } on'; }
for j in windows linux-x86_64; do
	body=$(job "$j")
	echo "$body" | grep -q -x '      - uses: actions/setup-dotnet@v5' || fail "$j does not set up .NET for vpk"
	install=$(step "$j" "Install vpk at the velopack crate's pin")
	echo "$install" | grep -q 'dotnet tool install vpk --version "\?\$pin"\? --tool-path ' || fail "$j does not install vpk at \$pin"
	echo "$install" | grep -q 'velopack = ' || fail "$j does not read the pin from Cargo.toml"
	build=$(step "$j" "Build the release binary")
	echo "$build" | grep -q -F -- "TENMOKU_UPDATE_FEED: $feed_default" || fail "$j does not bake in the feed"
	echo "$build" | grep -q -F -- 'TENMOKU_UPDATE_PUBLIC_KEY: ${{ vars.TENMOKU_UPDATE_PUBLIC_KEY }}' ||
		fail "$j does not bake in the public key variable"
	echo "$body" | awk '
		/- name: Smoke test the (zipped|packed) binary/ && !p { p = NR }
		/- name: Pack (Setup.exe|the AppImage)/ && !k { k = NR }
		/- name: Smoke test (Setup.exe|the AppImage)/ && !s { s = NR }
		/actions\/upload-artifact@/ && !u { u = NR }
		END { exit !(p && k && s && u && p < k && k < s && s < u) }
	' || fail "$j does not pack after the portable smoke step, then smoke-test the pack, then upload"
	pack=$(echo "$body" | awk '/- name: Pack (Setup.exe|the AppImage)/ { on = 1; print; next } on && /^      - / { exit } on')
	echo "$pack" | grep -q -F 'TENMOKU_UPDATE_PRIVATE_KEY: ${{ secrets.TENMOKU_UPDATE_PRIVATE_KEY }}' ||
		fail "$j's pack step does not get the feed signing secret"
	echo "$pack" | grep -q -F 'TENMOKU_UPDATE_PUBLIC_KEY: ${{ vars.TENMOKU_UPDATE_PUBLIC_KEY }}' ||
		fail "$j's pack step does not get the public key to check the signer against"
	echo "$pack" | grep -q 'sign_update_feed' || fail "$j's pack step does not pass the signer"
	echo "$pack" | grep -q 'if:' && fail "$j's pack step is conditional: a dispatch must pack too"
done
step windows "Pack Setup.exe" | grep -q 'scripts/velopack-pack.ps1 -Arch \${{ matrix.arch }} ' || fail "the windows job does not run velopack-pack.ps1"
step linux-x86_64 "Pack the AppImage" | grep -q 'scripts/velopack-pack.sh --linux ' || fail "the linux job does not run velopack-pack.sh --linux"
setup_smoke=$(step windows "Smoke test Setup.exe")
for word in "-ArgumentList '--silent'" '$env:LOCALAPPDATA Tenmoku' "current\\tenmoku.exe" '--version' \
	'Start Menu\Programs' 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall' \
	'QuietUninstallString' "throw \"the uninstall left \$exe\"" 'the uninstall left the Add/Remove Programs entry'; do
	printf "%s\n" "$setup_smoke" | grep -q -F -- "$word" || fail "the Setup.exe smoke step lacks $word"
done
appimage_smoke=$(step linux-x86_64 "Smoke test the AppImage")
for word in 'sha256sum -c' '"$appimage" --version' 'Xvfb :98' 'DISPLAY=:98' 'sleep 5' 'kill -0 "$app"' "grep -n 'panicked'"; do
	printf "%s\n" "$appimage_smoke" | grep -q -F -- "$word" || fail "the AppImage smoke step lacks $word"
done
job linux-x86_64 | grep -q 'apt-get install -y squashfs-tools fuse3 libfuse2t64' || fail "the linux job lacks vpk's mksquashfs or FUSE"
ok "windows and linux install vpk $pin, bake in the feed and key, pack, then install or launch the pack before uploading"

# The feeds' signing secret reaches only the two pack steps, through env:.
[ "$(grep -c 'secrets\.TENMOKU_UPDATE_PRIVATE_KEY' "$wf")" -eq 2 ] ||
	fail "secrets.TENMOKU_UPDATE_PRIVATE_KEY appears other than in the two pack steps"
if grep -n 'TENMOKU_UPDATE_PRIVATE_KEY' "$wf" | grep -v -E '^[0-9]+:          TENMOKU_UPDATE_PRIVATE_KEY: \$\{\{ secrets\.TENMOKU_UPDATE_PRIVATE_KEY \}\}$' |
	grep -v -F 'set TENMOKU_UPDATE_PRIVATE_KEY, or delete that asset first' | grep -v -E '^[0-9]+: *#'; then
	fail "the lines above read the feed signing key in the workflow; only the pack scripts may"
fi
ok "TENMOKU_UPDATE_PRIVATE_KEY reaches the pack steps through env: only"

# The channels: velopack-pack.sh and .ps1 pack on the ones the app reads (release_channel).
rs="$root/crates/fastedit/src/velopack_update.rs"
for c in windows-x64-stable windows-arm64-stable linux-x86_64-stable; do
	grep -q -F "Some(\"$c\")" "$rs" || fail "velopack_update.rs does not read channel $c"
done
ps1="$here/velopack-pack.ps1"
for word in '$channel = "windows-$Arch-stable"' "'--packId', 'Tenmoku'" "'--runtime', \"win-\$Arch\"" \
	"'--shortcuts', 'StartMenuRoot'" "'--noPortable'" "'--delta', 'None'" 'Remove-Item Env:TENMOKU_UPDATE_PRIVATE_KEY' \
	'$private | & $Signer sign $feed' 'if ($signed -ne $public)' '"Tenmoku-$Version-windows-$Arch-Setup.exe"'; do
	grep -q -F -- "$word" "$ps1" || fail "velopack-pack.ps1 lacks $word"
done
ok "velopack-pack.ps1 packs Tenmoku per user with a Start menu shortcut on windows-<arch>-stable, the key on stdin only"

# velopack-pack.sh --linux, dry: its vpk and signer commands, and its key checks.
mkdir -p "$t/appdir/Licenses"
cp /usr/bin/true "$t/appdir/tenmoku"
linux_pack() { env -u TENMOKU_UPDATE_PRIVATE_KEY_FILE VPK=/opt/vpk "$here/velopack-pack.sh" --linux --pack-dir "$t/appdir" \
	--version 1.2.3 --out "$t/vpkout" --dry-run "$@"; }
TENMOKU_UPDATE_PRIVATE_KEY='' TENMOKU_UPDATE_PUBLIC_KEY='' linux_pack >"$t/vpk.log" 2>&1 ||
	{ cat "$t/vpk.log"; fail "velopack-pack.sh --linux --dry-run exited non-zero"; }
for word in "/opt/vpk " " pack --packId Tenmoku --packVersion 1.2.3 " " --mainExe tenmoku " \
	" --channel linux-x86_64-stable --runtime linux-x64 " " --delta None " "releases.linux-x86_64-stable.json is left unsigned"; do
	grep -q -F -- "$word" "$t/vpk.log" || fail "velopack-pack.sh --linux --dry-run lacks '$word': $(cat "$t/vpk.log")"
done
TENMOKU_UPDATE_PRIVATE_KEY=secretseed TENMOKU_UPDATE_PUBLIC_KEY=AbCd= linux_pack --signer /opt/sign_update_feed >"$t/vpk.log" 2>&1 ||
	{ cat "$t/vpk.log"; fail "velopack-pack.sh --linux --dry-run with a key exited non-zero"; }
grep -q -x -F "<private key from TENMOKU_UPDATE_PRIVATE_KEY> | /opt/sign_update_feed sign $t/vpkout/releases.linux-x86_64-stable.json" "$t/vpk.log" ||
	fail "the feed is not signed with the key on stdin: $(cat "$t/vpk.log")"
grep -q secretseed "$t/vpk.log" && fail "velopack-pack.sh printed the private key"
if TENMOKU_UPDATE_PRIVATE_KEY=secretseed TENMOKU_UPDATE_PUBLIC_KEY='' linux_pack --signer /opt/s >"$t/vpk.log" 2>&1; then
	fail "a private key with no public key was packed"
fi
if TENMOKU_UPDATE_PRIVATE_KEY=secretseed TENMOKU_UPDATE_PUBLIC_KEY=AbCd= linux_pack >"$t/vpk.log" 2>&1; then
	fail "a private key with no --signer was packed"
fi
if TENMOKU_UPDATE_PRIVATE_KEY='' linux_pack --channel stable >"$t/vpk.log" 2>&1; then
	fail "velopack-pack.sh --linux took another channel"
fi
shellcheck "$here/velopack-pack.sh" >/dev/null 2>&1 || ! command -v shellcheck >/dev/null || fail "shellcheck velopack-pack.sh"
ok "velopack-pack.sh --linux packs Tenmoku on linux-x86_64-stable and signs the feed only with both keys"

# Publish: after every build job, on tag pushes only, and the only job with gh release.
publish_job=$(job publish)
echo "$publish_job" | grep -q -x '    needs: \[version, macos-arm64, windows, linux-x86_64\]' || fail "publish does not need every build job"
echo "$publish_job" | grep -q -x "    if: github.event_name == 'push' && github.ref_type == 'tag'" ||
	fail "publish is not limited to tag pushes"
echo "$publish_job" | grep -q 'actions/download-artifact@v8' || fail "publish does not download the build jobs' files"
echo "$publish_job" | grep -q -x '          merge-multiple: true' || fail "publish does not merge the artifacts into dist/"
for j in version macos-arm64 windows linux-x86_64; do
	job "$j" | grep -q 'gh release' && fail "$j runs gh release: only publish may"
done
echo "$publish_job" | grep -q -F -- '--notes "$notes"' || fail "the release notes do not get the downloads text"
echo "$publish_job" | grep -q -F 'needs glibc $GLIBC or later' || echo "$publish_job" | grep -q -F 'needing glibc $GLIBC or later' ||
	fail "the release notes do not name the minimum glibc"
for word in windows-x64-Setup.exe windows-arm64-Setup.exe linux-x86_64.AppImage; do
	echo "$publish_job" | grep -q -F "$word" || fail "the release notes do not name $word"
done
ok "publish needs all four builds, runs on tag pushes only, and is the only job that publishes"

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
[ "$(grep 'sign_update' "$wf" | grep -c -v 'sign_update_feed')" -eq 1 ] || fail "sign_update appears other than once"
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

# The same guard for a Velopack feed's .sig.
FAKE_ASSETS='Tenmoku-1.2.3-macos-arm64.zip
releases.linux-x86_64-stable.json
releases.linux-x86_64-stable.json.sig'
if publish none; then
	fail "a rerun with no feed signature over a release that has one was not refused"
fi
grep -q 'has releases.linux-x86_64-stable.json.sig and this run made none' "$t/publish.log" ||
	fail "no reason given: $(cat "$t/publish.log")"
[ ! -s "$t/gh.log" ] || fail "the refused rerun still changed the release: $(cat "$t/gh.log")"
: >"$t/run/dist/releases.linux-x86_64-stable.json.sig"
(cd "$t/run" && PATH="$t/bin:$PATH" GITHUB_REF_NAME=v1.2.3 FAKE_GH_LOG="$t/gh.log" bash -e "$t/publish-step.sh") \
	>"$t/publish.log" 2>&1 || { cat "$t/publish.log"; fail "a signed rerun of a signed feed failed"; }
grep -q '^release upload v1.2.3 .*dist/releases.linux-x86_64-stable.json.sig .*--clobber' "$t/gh.log" ||
	fail "the signed feed rerun: $(cat "$t/gh.log")"
ok "a rerun with no feed .sig over a signed feed is refused; a signed one replaces it"
FAKE_ASSETS='Tenmoku-1.2.3-macos-arm64.zip'

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

# The Windows and Linux archives carry the licence texts bundle-macos.sh ships: each of its
# ship lines appears in package-linux.sh as is and in package-windows.ps1 as a Join-Path.
grep -E '^ship "\$root/' "$here/bundle-macos.sh" >"$t/ships"
[ -s "$t/ships" ] || fail "no ship lines in bundle-macos.sh"
while IFS= read -r line; do
	grep -q -x -F -- "$line" "$here/package-linux.sh" || fail "package-linux.sh does not ship: $line"
	src=$(echo "$line" | sed -E 's|^ship "\$root/([^"]*)" ([^ ]*)$|\1|' | tr / '\\')
	as=$(echo "$line" | sed -E 's|^ship "\$root/([^"]*)" ([^ ]*)$|\2|' | tr / '\\')
	grep -q -F -- "Ship (Join-Path \$root '$src') '$as'" "$here/package-windows.ps1" ||
		fail "package-windows.ps1 does not ship $src as $as"
done <"$t/ships"
for f in "$here/package-linux.sh" "$here/package-windows.ps1"; do
	grep -q 'vendor' "$f" && grep -q 'extensions' "$f" && grep -q 'OFL.txt' "$f" ||
		fail "$(basename "$f") does not ship the grammars' and fonts' licences"
done
ok "package-linux.sh and package-windows.ps1 ship bundle-macos.sh's licence texts"

# package-linux.sh on a fixture binary: one folder with the binary, desktop entry, icon,
# installer and licences, and a .sha256 that checks.
cp /usr/bin/true "$t/linux-tenmoku"
"$here/package-linux.sh" --binary "$t/linux-tenmoku" --version 1.2.3 --out "$t/linux" >"$t/linux.log" 2>&1 ||
	{ cat "$t/linux.log"; fail "package-linux.sh exited non-zero"; }
[ "$(tail -n 1 "$t/linux.log")" = "$t/linux/Tenmoku-1.2.3-linux-x86_64.tar.gz" ] ||
	fail "package-linux.sh did not print the archive: $(cat "$t/linux.log")"
(cd "$t/linux" && shasum -a 256 -c Tenmoku-1.2.3-linux-x86_64.tar.gz.sha256 >/dev/null) || fail "the .sha256 does not check"
tar -tzf "$t/linux/Tenmoku-1.2.3-linux-x86_64.tar.gz" >"$t/linux.list"
grep -v -q '^Tenmoku-1.2.3-linux-x86_64/' "$t/linux.list" && fail "the archive has files outside its one folder"
for f in tenmoku dev.tenmoku.tenmoku.desktop dev.tenmoku.tenmoku.svg install.sh Licenses/LICENSE \
	Licenses/NOTICE Licenses/THIRD_PARTY.md Licenses/tree-sitter-LICENSE.txt; do
	grep -q -x "Tenmoku-1.2.3-linux-x86_64/$f" "$t/linux.list" || fail "the archive lacks $f"
done
grep -q -E '^Tenmoku-1.2.3-linux-x86_64/Licenses/fonts/[^/]+/OFL.txt$' "$t/linux.list" || fail "the archive lacks the fonts' OFL.txt"
tar -xzf "$t/linux/Tenmoku-1.2.3-linux-x86_64.tar.gz" -C "$t/linux"
[ -x "$t/linux/Tenmoku-1.2.3-linux-x86_64/tenmoku" ] && [ -x "$t/linux/Tenmoku-1.2.3-linux-x86_64/install.sh" ] ||
	fail "the binary or install.sh is not executable in the archive"
grep -q -x 'Exec=tenmoku %F' "$root/packaging/linux/dev.tenmoku.tenmoku.desktop" || fail "the desktop entry's Exec line changed"
grep -q -x 'StartupWMClass=dev.tenmoku.tenmoku' "$root/packaging/linux/dev.tenmoku.tenmoku.desktop" ||
	fail "the desktop entry's window class is not the app id"
ok "package-linux.sh packs one folder with the binary, desktop entry, icon, installer and licences"

# install.sh into a fake home, then --uninstall.
home="$t/home"
mkdir -p "$home"
HOME="$home" XDG_DATA_HOME='' PATH=/usr/bin:/bin "$t/linux/Tenmoku-1.2.3-linux-x86_64/install.sh" >"$t/install.log" 2>&1 ||
	{ cat "$t/install.log"; fail "install.sh exited non-zero"; }
[ "$(readlink "$home/.local/bin/tenmoku")" = "$t/linux/Tenmoku-1.2.3-linux-x86_64/tenmoku" ] ||
	fail "install.sh did not link ~/.local/bin/tenmoku"
grep -q -x -F "Exec=\"$t/linux/Tenmoku-1.2.3-linux-x86_64/tenmoku\" %F" "$home/.local/share/applications/dev.tenmoku.tenmoku.desktop" ||
	fail "the installed desktop entry does not run the unpacked binary"
[ -f "$home/.local/share/icons/hicolor/scalable/apps/dev.tenmoku.tenmoku.svg" ] || fail "install.sh did not install the icon"
HOME="$home" XDG_DATA_HOME='' "$t/linux/Tenmoku-1.2.3-linux-x86_64/install.sh" --uninstall >/dev/null ||
	fail "install.sh --uninstall exited non-zero"
[ -z "$(find "$home" -type f -o -type l)" ] || fail "--uninstall left: $(find "$home" -type f -o -type l)"
ok "install.sh installs for the user and --uninstall removes it"

# A folder name with every character the Desktop Entry spec or sed treats specially: the Exec
# line quotes the path ( \" \` \$ \\ ), doubles those backslashes, and writes % as %%.
odd=$(printf 'A b&c|d"e%sf\\g%%h$i`j' "'")
escaped=$(printf 'A b&c|d\\\\"e%sf\\\\\\\\g%%%%h\\\\$i\\\\`j' "'")
mkdir -p "$t/odd/$odd"
cp -R "$t/linux/Tenmoku-1.2.3-linux-x86_64" "$t/odd/$odd/"
rm -rf "$home" && mkdir -p "$home"
HOME="$home" XDG_DATA_HOME='' PATH=/usr/bin:/bin "$t/odd/$odd/Tenmoku-1.2.3-linux-x86_64/install.sh" >"$t/install.log" 2>&1 ||
	{ cat "$t/install.log"; fail "install.sh in an oddly named folder exited non-zero"; }
[ "$(readlink "$home/.local/bin/tenmoku")" = "$t/odd/$odd/Tenmoku-1.2.3-linux-x86_64/tenmoku" ] ||
	fail "install.sh linked $(readlink "$home/.local/bin/tenmoku")"
exec_line=$(grep '^Exec=' "$home/.local/share/applications/dev.tenmoku.tenmoku.desktop")
[ "$exec_line" = "Exec=\"$t/odd/$escaped/Tenmoku-1.2.3-linux-x86_64/tenmoku\" %F" ] ||
	fail "the Exec line for folder '$odd' is: $exec_line"
[ -z "$(find "$home/.local/share/applications" -name '.*')" ] || fail "install.sh left its staged desktop entry"
ok "install.sh escapes a folder named $odd for the desktop entry"

[ -f "$root/crates/fastedit/tests/version_flag.rs" ] || fail "no crates/fastedit/tests/version_flag.rs"
ok "crates/fastedit/tests/version_flag.rs covers the --version the smoke steps run"

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
