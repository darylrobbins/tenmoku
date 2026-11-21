#!/bin/sh
# Fixture test for scripts/draft-release-notes.sh: a temp git repository with tagged history
# and a fake rgt (RGT) printing each task's type, no network. Prints one line per check and
# exits non-zero on the first failure. macOS or Linux; needs git.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
draft="$here/draft-release-notes.sh"
t=$(mktemp -d)
trap 'rm -rf "$t"' EXIT

fail() {
	echo "FAIL: $*"
	exit 1
}
ok() { echo "ok: $*"; }

repo="$t/repo"
mkdir -p "$repo"
cd "$repo"
git init -q
git config user.name Test
git config user.email test@example.test
git config commit.gpgsign false
git config tag.gpgsign false
n=0
commit() {
	n=$((n + 1))
	# A second apart, so the log's order is the order made, across the merge too.
	GIT_AUTHOR_DATE="$((1700000000 + n)) +0000"
	GIT_COMMITTER_DATE=$GIT_AUTHOR_DATE
	export GIT_AUTHOR_DATE GIT_COMMITTER_DATE
	echo "$n" >file
	git add file
	git commit -q -m "$1"
}

commit "Before the first release (T1)"
git tag v0.9.0
commit "Old work after 0.9.0 (T2)"
git tag v0.10.0
# v0.10.0 is the latest by version, though v0.9.0 sorts after it as text.
commit "A feature with a lent handle (Mac T100) (T7)"
commit "A bugfix (T101)"
commit "A refactor (T102)"
commit "A chore (T103)"
commit "Docs for something (T104)"
commit "Flake: a feature task's flaky test (T100)"
commit "Spec T105: what a card will do"
commit "Bump the workspace version to 0.11.0 for the v0.11.0 release"
commit "A spike, a type with no section (T106)"
commit "An unknown task (T999)"
commit "No handle at all"
commit "Mentions (T5) mid-line then ends (T101)"
commit "Two trailing handles (T104) (T101)"
commit "A handle that is not trailing (T100) ends in words"
git checkout -q -b side
commit "Merged from a side branch (T101)"
git checkout -q -
git merge -q --no-ff -m "Merge branch side (T100)" side

# The fake rgt: the type of each known task, as rgt prints it; not_found for the rest. It logs
# its arguments.
cat >"$t/rgt" <<'EOF'
#!/bin/sh
echo "$*" >>"$RGT_LOG"
[ "$1 $2" = "task show" ] && [ "$4 $5 $6" = "--json --fields type" ] || { echo "bad call: $*" >&2; exit 2; }
case "$3" in
T100) type=feature ;;
T101) type=bugfix ;;
T102) type=refactor ;;
T103) type=chore ;;
T104) type=docs ;;
T106) type=spike ;;
*)
	echo '{"v":1,"ok":false,"data":null,"error":{"code":"not_found","message":"no task '"$3"' in this repo"}}'
	exit 66
	;;
esac
echo '{"v":1,"ok":true,"data":{"type":"'"$type"'"},"error":null}'
EOF
chmod +x "$t/rgt"
export RGT_LOG="$t/rgt.log"

RGT="$t/rgt" "$draft" >"$t/out.md" 2>"$t/err.log" || { cat "$t/err.log"; fail "the draft exited non-zero"; }
cat >"$t/expected.md" <<'EOF'
## New

- A feature with a lent handle
- A handle that is not trailing (T100) ends in words

## Fixed

- A bugfix
- Mentions (T5) mid-line then ends
- Two trailing handles
- Merged from a side branch

## Internal (delete before release)

- A refactor
- A chore
- Docs for something
- Flake: a feature task's flaky test
- Spec T105: what a card will do
- Bump the workspace version to 0.11.0 for the v0.11.0 release

## Unsorted

- A spike, a type with no section
- An unknown task
- No handle at all
EOF
diff -u "$t/expected.md" "$t/out.md" || fail "the draft since v0.10.0 is not as expected (diff above)"
ok "since the latest v* tag by version (v0.10.0): feature under New, bugfix under Fixed, refactor/chore/docs, Flake:, Spec and the bump under Internal, the rest Unsorted; merges left out"
grep -q -x 'task show T100 --json --fields type' "$t/rgt.log" || fail "rgt was not asked about T100: $(cat "$t/rgt.log")"
grep -q 'show T7 ' "$t/rgt.log" && fail "rgt was asked about T7, the lent handle"
[ "$(grep -c -x 'task show T101 --json --fields type' "$t/rgt.log")" = 1 ] || fail "rgt was asked about T101 other than once: $(cat "$t/rgt.log")"
grep -q 'T105\|T5 ' "$t/rgt.log" && fail "rgt was asked about a handle that is not a task's: $(cat "$t/rgt.log")"
ok "(Mac T<n>) wins over a trailing (T<n>), the last trailing handle is the task, and rgt is asked once per task"
grep -q '0.10.0..HEAD' "$t/err.log" || fail "the range is not named: $(cat "$t/err.log")"
ok "the range goes to stderr: $(cat "$t/err.log")"

RGT="$t/rgt" "$draft" --since v0.9.0 >"$t/since.md" 2>/dev/null || fail "--since v0.9.0 exited non-zero"
grep -q -x -- '- Old work after 0.9.0' "$t/since.md" || fail "--since v0.9.0 lacks the commit after v0.9.0: $(cat "$t/since.md")"
grep -q 'Before the first release' "$t/since.md" && fail "--since v0.9.0 has the commit before it"
ok "--since v0.9.0 starts there"

RGT="$t/rgt" "$draft" --out "$t/draft.md" >"$t/out.log" 2>/dev/null || fail "--out exited non-zero"
[ "$(cat "$t/out.log")" = "$t/draft.md" ] || fail "--out did not print the path: $(cat "$t/out.log")"
cmp -s "$t/draft.md" "$t/expected.md" || fail "--out did not write the draft"
ok "--out writes the draft and prints its path"
if RGT="$t/rgt" "$draft" --out "$t/draft.md" >/dev/null 2>"$t/err.log"; then
	fail "--out over an existing file was not refused"
fi
grep -q 'exists' "$t/err.log" || fail "--out over a file: $(cat "$t/err.log")"
cmp -s "$t/draft.md" "$t/expected.md" || fail "--out over a file changed it"
ok "--out over an existing file is refused and leaves it"

# No rgt: RGT unset and none on PATH (git only, from a folder of its own).
mkdir -p "$t/bin"
ln -s "$(command -v git)" "$t/bin/git"
if PATH="$t/bin:/usr/bin:/bin" command -v rgt >/dev/null 2>&1; then
	fail "rgt is in /usr/bin or /bin, so the no-rgt run can't hide it"
fi
: >"$t/rgt.log"
env -u RGT PATH="$t/bin:/usr/bin:/bin" "$draft" >"$t/none.md" 2>"$t/err.log" || { cat "$t/err.log"; fail "the draft with no rgt exited non-zero"; }
[ ! -s "$t/rgt.log" ] || fail "the fake rgt ran with no RGT set"
grep -q 'no rgt' "$t/err.log" || fail "no line about the missing rgt: $(cat "$t/err.log")"
{
	printf '## Unsorted\n\n'
	# The same lines, in commit order.
	for line in "A feature with a lent handle" "A bugfix" "A refactor" "A chore" "Docs for something" \
		"Flake: a feature task's flaky test" "Spec T105: what a card will do" \
		"Bump the workspace version to 0.11.0 for the v0.11.0 release" "A spike, a type with no section" \
		"An unknown task" "No handle at all" "Mentions (T5) mid-line then ends" "Two trailing handles" \
		"A handle that is not trailing (T100) ends in words" "Merged from a side branch"; do
		printf -- '- %s\n' "$line"
	done
} >"$t/none.expected"
diff -u "$t/none.expected" "$t/none.md" || fail "with no rgt, the draft is not all Unsorted (diff above)"
ok "with no rgt every line is under ## Unsorted, handles still stripped"

# Previews: the previous tag is the nearest behind HEAD whatever its kind. By version sort,
# v0.12.0-preview.1 comes after v0.12.0, so the order of the history has to decide.
commit "Work for the first preview (T101)"
git tag v0.12.0-preview.1
commit "Work after the preview (T100)"
git tag v0.12.0
commit "Work for the next preview (T101)"
RGT="$t/rgt" "$draft" --version 0.12.1-preview.1 >"$t/pv.log" 2>"$t/err.log" || { cat "$t/err.log"; fail "--version 0.12.1-preview.1 exited non-zero"; }
[ "$(cat "$t/pv.log")" = "$(pwd -P)/release-notes/0.12.1-preview.1.md" ] || fail "--version did not print its path: $(cat "$t/pv.log")"
printf '## Fixed\n\n- Work for the next preview\n' >"$t/pv.expected"
diff -u "$t/pv.expected" "$repo/release-notes/0.12.1-preview.1.md" || fail "the preview's notes cover more than v0.12.0..HEAD (diff above)"
grep -q 'v0.12.0..HEAD' "$t/err.log" || fail "the range after a stable tag is not v0.12.0..HEAD: $(cat "$t/err.log")"
ok "--version 0.12.1-preview.1 writes release-notes/0.12.1-preview.1.md from the previous tag, v0.12.0"
if RGT="$t/rgt" "$draft" --version 0.12.1-preview.1 >/dev/null 2>"$t/err.log"; then
	fail "--version over an existing notes file was not refused"
fi
git tag v0.12.1-preview.1
commit "Work for the second preview (T100)"
RGT="$t/rgt" "$draft" --version 0.12.1-preview.2 >/dev/null 2>"$t/err.log" || { cat "$t/err.log"; fail "--version 0.12.1-preview.2 exited non-zero"; }
grep -q 'v0.12.1-preview.1..HEAD' "$t/err.log" || fail "the range after a preview tag is not v0.12.1-preview.1..HEAD: $(cat "$t/err.log")"
printf '## New\n\n- Work for the second preview\n' >"$t/pv2.expected"
diff -u "$t/pv2.expected" "$repo/release-notes/0.12.1-preview.2.md" || fail "the second preview's notes are wrong (diff above)"
ok "--version 0.12.1-preview.2 starts at the previous preview's tag"
for bad in 0.12.1-preview 0.12.1-preview.01 0.12.1-rc.1 v0.12.1 -preview.1; do
	if RGT="$t/rgt" "$draft" --version "$bad" >/dev/null 2>&1; then
		fail "--version $bad was accepted"
	fi
done
if RGT="$t/rgt" "$draft" --version 0.12.2 --out "$t/x.md" >/dev/null 2>&1; then
	fail "--version with --out was accepted"
fi
ok "--version refuses malformed versions, and --out beside it"

if RGT="$t/rgt" "$draft" --since v9.9.9 >/dev/null 2>"$t/err.log"; then
	fail "an unknown --since was taken"
fi
grep -q 'v9.9.9' "$t/err.log" || fail "an unknown --since: $(cat "$t/err.log")"
git tag --list | xargs git tag -d >/dev/null
if RGT="$t/rgt" "$draft" >/dev/null 2>"$t/err.log"; then
	fail "no v* tag and no --since was taken"
fi
grep -q -e '--since' "$t/err.log" || fail "no tag: $(cat "$t/err.log")"
ok "an unknown --since, or no v* tag without one, is refused"

echo ok
