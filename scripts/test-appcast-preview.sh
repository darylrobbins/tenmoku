#!/bin/sh
# Preview-feed cases for scripts/make-appcast.sh (see test-appcast.sh for the stable ones).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
make="$here/make-appcast.sh"
t=$(mktemp -d)
trap 'rm -rf "$t"' EXIT
fail() { echo "FAIL: $*"; exit 1; }
ok() { echo "ok: $*"; }

mkapp() { # mkapp DIR BUILD_VERSION
	mkdir -p "$1/Tenmoku.app/Contents"
	cat >"$1/Tenmoku.app/Contents/Info.plist" <<PL
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleVersion</key><string>$2</string>
<key>LSMinimumSystemVersion</key><string>11.0</string>
</dict></plist>
PL
	(cd "$1" && zip -q ../"$(basename "$1")".zip Tenmoku.app/Contents/Info.plist)
}
mkapp "$t/pre" 1.2.3.preview.2
mkapp "$t/sta" 1.2.3
url=https://example.test/app.zip
sig='d1S3994qdXXhjyqoBpiQ5mbdjR7kd1LCFmUXWP2/vwj609U7syIB+/Eyvg8+M5IyplR0TC5HLdrWsbitMCsUAQ=='
out="$t/appcast.xml"
run() { z=$1; shift; "$make" --zip "$t/$z.zip" --download-url "$url" --signature "$sig" --out "$out" "$@"; }
x() { xmllint --xpath "string($1)" "$out"; }
elem() { echo "//*[local-name()='$1']"; }

run pre --version 1.2.3-preview.2 --channel-feed preview >/dev/null
xmllint --noout "$out" || fail "xmllint refused the preview feed"
[ "$(x "count(//item)")" = 1 ] || fail "not one item"
[ "$(x "$(elem version)")" = 1.2.3.preview.2 ] || fail "sparkle:version"
[ "$(x "$(elem shortVersionString)")" = 1.2.3-preview.2 ] || fail "shortVersionString"
[ "$(x "count($(elem channel)[ancestor::item])")" = 0 ] || fail "a sparkle:channel element"
ok "preview item validates: build version, display version, no channel element"

run sta --version 1.2.3 --channel-feed preview >/dev/null
[ "$(x "$(elem version)")" = 1.2.3 ] || fail "stable build in the preview feed"
[ "$(x "count($(elem channel)[ancestor::item])")" = 0 ] || fail "a sparkle:channel element"
ok "a stable build is accepted in the preview feed"

rm -f "$out"
if run pre --version 1.2.3-preview.2 >"$t/r.log" 2>&1; then fail "preview in stable feed accepted"; fi
grep -q 'a preview must go in the preview feed' "$t/r.log" || fail "message: $(cat "$t/r.log")"
[ ! -e "$out" ] || fail "left an appcast"
ok "a preview in the stable feed is refused"

if run pre --version 1.2.3-preview.3 --channel-feed preview >"$t/r.log" 2>&1; then fail "wrong build accepted"; fi
grep -q CFBundleVersion "$t/r.log" || fail "message: $(cat "$t/r.log")"
if run pre --version 1.2.3-preview.02 --channel-feed preview >"$t/r.log" 2>&1; then fail "leading zero accepted"; fi
ok "a preview version must match the bundle's build version"

# The default output, byte for byte, against the text the base implementation (c759b2d) wrote
# for these inputs. The date is a stub on PATH and the zip's mtime is pinned.
mkdir -p "$t/bin"
cat >"$t/bin/date" <<'D'
#!/bin/sh
echo 'Sat, 21 Nov 2026 12:00:00 +0000'
D
chmod +x "$t/bin/date"
touch -t 202611211200 "$t/sta/Tenmoku.app/Contents/Info.plist"
rm -f "$t/sta.zip"
(cd "$t/sta" && zip -q -X ../sta.zip Tenmoku.app/Contents/Info.plist)
size=$(wc -c <"$t/sta.zip" | tr -d ' ')
PATH="$t/bin:$PATH" run sta --version 1.2.3 >/dev/null
cat >"$t/expected.xml" <<X
<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
  <channel>
    <title>Tenmoku</title>
    <item>
      <title>Version 1.2.3</title>
      <pubDate>Sat, 21 Nov 2026 12:00:00 +0000</pubDate>
      <sparkle:version>1.2.3</sparkle:version>
      <sparkle:shortVersionString>1.2.3</sparkle:shortVersionString>
      <sparkle:minimumSystemVersion>11.0</sparkle:minimumSystemVersion>
      <enclosure url="$url" length="$size" type="application/octet-stream" sparkle:edSignature="$sig"/>
    </item>
  </channel>
</rss>
X
cmp "$out" "$t/expected.xml" || fail "the default output differs from the base implementation's"
ok "stable item unchanged"
echo ok
