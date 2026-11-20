#!/bin/sh
# Tests scripts/check-user-docs.sh against a good tree and one broken tree per rule.
here=$(cd "$(dirname "$0")" && pwd)
check=$here/check-user-docs.sh
tmp=$(mktemp -d) || exit 1
trap 'rm -rf "$tmp"' EXIT
fails=0

good() {
  rm -rf "$tmp/b"; mkdir -p "$tmp/b/src/reference"
  printf '# Summary\n\n- [A](a.md)\n- [R](reference/r.md)\n' > "$tmp/b/src/SUMMARY.md"
  printf '# A\n\n> Status: stub\n\nSee [r](reference/r.md#r).\n' > "$tmp/b/src/a.md"
  printf '# R\n\nGenerated.\n' > "$tmp/b/src/reference/r.md"
}
expect() { # name want(0|1|2) [pattern]
  sh "$check" "$tmp/b" > "$tmp/out" 2>&1; rc=$?
  if [ "$rc" -ne "$2" ] || { [ -n "$3" ] && ! grep -q "$3" "$tmp/out"; }; then
    echo "FAIL: $1 (rc=$rc)"; cat "$tmp/out"; fails=$((fails+1))
  else echo "ok: $1"; fi
}

good; expect "good tree" 0
good; printf -- '- [B](b.md)\n' >> "$tmp/b/src/SUMMARY.md"; expect "missing file" 1 "no file: b.md"
good; printf '# B\n\n> Status: stub\n' > "$tmp/b/src/b.md"; expect "unlisted file" 1 "not in SUMMARY: b.md"
good; printf '# A\n\n> Status: stub\n\n[x](nope.md)\n' > "$tmp/b/src/a.md"; expect "broken link" 1 "broken link in a.md"
good; printf '> Status: stub\n\ntext\n' > "$tmp/b/src/a.md"; expect "no H1" 1 "no H1: a.md"
good; printf '# A\n\ntext\n' > "$tmp/b/src/a.md"; expect "no Status" 1 "no Status line: a.md"
good; printf '# A\n\n> Status: stub\n\n[x](#sec) [y](reference/r.md#whatever)\n' > "$tmp/b/src/a.md"; expect "fragments ignored" 0
good; mkdir "$tmp/fake"; printf '#!/bin/sh\nexit 3\n' > "$tmp/fake/awk"; chmod +x "$tmp/fake/awk"; PATH="$tmp/fake:$PATH" expect "failing utility is not success" 2
good; mkdir -p "$tmp/fake2"; printf '#!/bin/sh\ncase "$1" in *\\./*) exit 3 ;; esac\nexec /usr/bin/env -i PATH=/usr/bin:/bin sed "$@"\n' > "$tmp/fake2/sed"; chmod +x "$tmp/fake2/sed"; PATH="$tmp/fake2:$PATH" expect "failing listing sed is not success" 2
good; printf 'x' > "$tmp/b/src/pic.png"; expect "unlisted non-markdown file" 1 "not in SUMMARY: pic.png"
good; mkdir "$tmp/b/src/sub"; printf '# S\n' > "$tmp/b/src/sub/SUMMARY.md"; expect "unlisted nested SUMMARY" 1 "not in SUMMARY: sub/SUMMARY.md"
good; printf '# A\n\n> Status: stub\n\n[r]: x.md\n' > "$tmp/b/src/a.md"; expect "reference definition fails" 1 "reference-style"
good; printf '# A\n\n> Status: stub\n\n[x](<a b.md>)\n' > "$tmp/b/src/a.md"; expect "angle destination fails" 1 "angle-bracketed"
good; printf '# A\n\n> Status: stub\n\n[x](reference/\nr.md)\n' > "$tmp/b/src/a.md"; expect "multi-line destination fails" 1 "not closed"
good; printf '# Summary\n\n- [A][a]\n- [R](reference/r.md)\n\n[a]: a.md\n' > "$tmp/b/src/SUMMARY.md"; expect "reference-style SUMMARY fails" 1 "SUMMARY.md: unsupported"
good; printf '```\n# A\n> Status: stub\n```\n' > "$tmp/b/src/a.md"; expect "H1 and Status only in a fence fail" 1 "no H1: a.md"
good; printf '# A\n\n> Status: stub\n\n[x](a(b).md)\n' > "$tmp/b/src/a.md"; expect "parenthesis in destination fails" 1 "parenthesis"
good; printf '# Summary\n\n-  [A](a.md)\n- [R](reference/r.md)\n' > "$tmp/b/src/SUMMARY.md"; expect "odd SUMMARY spacing fails" 1 "SUMMARY list entry"
good; printf '````\n```\n# A\n> Status: stub\n````\n' > "$tmp/b/src/a.md"; expect "shorter inner fence does not close" 1 "no H1: a.md"
good; printf '~~~\n```\n~~~\n# A\n> Status: stub\n' > "$tmp/b/src/a.md"; expect "fence closes on matching char" 0
good; printf '# A\n\n````\n```\n> Status: stub\n````\n' > "$tmp/b/src/a.md"; expect "Status only in a fence fails" 1 "no Status line: a.md"
good; printf '# A\n\n> Status: stub\n\n````\n```\n[x](nope.md)\n````\n' > "$tmp/b/src/a.md"; expect "link in a fence is skipped" 0
good; printf '# A\n\n> Status: stub\n\n~~~\n```\n[x](nope.md)\n```\n~~~\n' > "$tmp/b/src/a.md"; expect "mismatched fence keeps skipping links" 0
good; printf '# A\n\n> Status: stub\n\n```\n[x](nope.md)\n```\n\n[y](nope.md)\n' > "$tmp/b/src/a.md"; expect "link after the fence is checked" 1 "broken link in a.md"
good; printf '# Summary\n\n- [A](a.md)\n- [R](reference/r.md)\n\n```\n- example\n```\n' > "$tmp/b/src/SUMMARY.md"; expect "fenced list line in SUMMARY passes" 0
good; printf '# Summary\n\n- [A](a.md)\n- [R](reference/r.md)\n\n```\n- [Ghost](ghost.md)\n```\n' > "$tmp/b/src/SUMMARY.md"; expect "fenced chapter-shaped line in SUMMARY is ignored" 0
good; printf '# Summary\n\n- [A](a.md)\n- [R](reference/r.md)\n- [B](b.md)\n\n```\n- [B](b.md)\n```\n' > "$tmp/b/src/SUMMARY.md"; expect "fenced entry does not hide a missing file" 1 "no file: b.md"
good; printf '# B\n\n> Status: stub\n' > "$tmp/b/src/b.md"; printf '# Summary\n\n- [A](a.md)\n- [R](reference/r.md)\n\n```\n- [B](b.md)\n```\n' > "$tmp/b/src/SUMMARY.md"; expect "page listed only in a fence is unlisted" 1 "not in SUMMARY: b.md"
good; printf '# Summary\n\n- [A](a.md#top)\n- [R](reference/r.md)\n' > "$tmp/b/src/SUMMARY.md"; expect "chapter entry with a fragment passes" 0
[ "$fails" -eq 0 ] || exit 1
