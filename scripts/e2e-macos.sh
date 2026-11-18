#!/bin/sh
# Drive a bundled Tenmoku.app with XCUITest (the E2E-1 suite, T1682). See --help.
#
# Never called from regatta.toml, nextest, CI or another script: a run takes over the
# screen, mouse and keyboard of the Mac it runs on.
set -eu

start_dir=$(pwd)
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
spec="$root/tools/e2e-macos/project.yml"
sources="$root/tools/e2e-macos/TenmokuE2E"
declarations_file="$sources/Fixtures.swift"
fixtures="$root/tools/e2e-macos/fixtures"
e2e_id=dev.tenmoku.tenmoku.e2e

usage() {
	cat <<EOF
Usage: scripts/e2e-macos.sh [--no-build] [--iterations N] [--only TEST] [--fixture NAME]
                            [--results DIR] [--timeout SECONDS] [--dry-run]

Builds the release binary, bundles Tenmoku.app with scripts/bundle-macos.sh, gives the
bundle the id $e2e_id and re-signs it, generates the XCUITest project in
tools/e2e-macos/project.yml with xcodegen, and runs its SmokeTests with xcodebuild test.
The tests launch the bundle by URL, each on a scratch set of its own (a project, config,
state and home folder), never one of the installed Tenmoku.app copies.

Each test declares its fixture in tools/e2e-macos/TenmokuE2E/Fixtures.swift: a folder
under tools/e2e-macos/fixtures/ whose project/ becomes the scratch project, optionally
one of its settings/<name>.toml as the scratch config's settings.toml, and whether the
state folder starts empty (a first launch) or with a setup.toml that keeps the setup
wizard away. The runner is sandboxed (it may write only this run's scratch folder, and
nothing in the repo), so this script makes every set before the run, one per test and
iteration.

  --no-build        skip 'cargo build --release -p fastedit' (the bundle is still made,
                    from the binary already built)
  --iterations N    run each test N times, relaunching the runner each time (default 1)
  --only TEST       run one test, such as testSaveWithCmdSAndFileMenu
  --fixture NAME    run only the tests whose declared fixture is NAME, a folder under
                    tools/e2e-macos/fixtures/ (with --only, that test must declare it)
  --results DIR     where the result bundle, attachments, AX dumps and JSON summaries go
                    (default: \${CARGO_TARGET_DIR:-target}/e2e-macos/results/<time>)
  --timeout SECONDS stop the run, the test runner and the app after this long
                    (default: 600 plus 60 per test run)
  --dry-run         print every command and run none
  -h, --help        this text

Everything else is under \${CARGO_TARGET_DIR:-target}/e2e-macos/: bundle/, the generated
TenmokuE2E.xcodeproj, DerivedData/ and scratch/. macOS only, from a logged-in GUI
session with the screen unlocked: a locked screen hangs a UI test. Xcode, xcodegen and
the Accessibility and Automation Mode grants are the user's to set up; this script never
changes System Settings. It prints the result bundle, the attachments folder, each test
run's time and a pass/fail line last, and exits non-zero when a test fails.
EOF
}

die() {
	echo "e2e-macos: $*" >&2
	exit 1
}

need_arg() {
	[ $# -ge 2 ] && [ -n "$2" ] || die "$1 needs a value (see --help)"
}

build=1
iterations=1
only=""
fixture=""
results=""
timeout=""
dry=""
while [ $# -gt 0 ]; do
	case "$1" in
	--no-build) build=""; shift ;;
	--iterations) need_arg "$@"; iterations=$2; shift 2 ;;
	--only) need_arg "$@"; only=$2; shift 2 ;;
	--fixture) need_arg "$@"; fixture=$2; shift 2 ;;
	--results) need_arg "$@"; results=$2; shift 2 ;;
	--timeout) need_arg "$@"; timeout=$2; shift 2 ;;
	--dry-run) dry=1; shift ;;
	-h | --help) usage; exit 0 ;;
	*) die "unknown argument: $1 (see --help)" ;;
	esac
done

case "$iterations" in
"" | *[!0-9]* | 0*) die "--iterations takes a whole number from 1, got '$iterations'" ;;
esac

# The tests and their fixtures, one "test fixture settings=NAME fresh=true" line each,
# from the one-line entries of Fixtures.swift's table. Every test method must have
# exactly one, and every fixture and settings file named must exist, before anything runs.
declarations=$(sed -nE 's/^[[:space:]]*"(test[A-Za-z0-9]+)": Fixture\("([A-Za-z0-9_-]+)"(, settings: "([A-Za-z0-9_-]+)")?(, fresh: (true))?\),?$/\1 \2 settings=\4 fresh=\6/p' "$declarations_file")
[ -n "$declarations" ] || die "no fixture declarations in $declarations_file"
methods=$(sed -nE 's/^[[:space:]]*func (test[A-Za-z0-9]+)\(\).*/\1/p' "$sources"/*.swift | sort)
declared=$(printf '%s\n' "$declarations" | cut -d' ' -f1 | sort)
[ "$(printf '%s\n' "$declared" | uniq -d)" = "" ] ||
	die "a test is declared twice in $declarations_file: $(printf '%s\n' "$declared" | uniq -d | tr '\n' ' ' | sed 's/ $//')"
[ "$methods" = "$declared" ] ||
	die "the test methods and the declarations in $declarations_file differ: methods $(printf '%s\n' "$methods" | tr '\n' ' ' | sed 's/ $//'), declared $(printf '%s\n' "$declared" | tr '\n' ' ' | sed 's/ $//')"
printf '%s\n' "$declarations" | while read -r name fixture_name settings fresh; do
	[ -d "$fixtures/$fixture_name/project" ] ||
		die "$name declares fixture '$fixture_name', but $fixtures/$fixture_name/project is not a folder"
	settings=${settings#settings=}
	if [ -n "$settings" ] && [ ! -f "$fixtures/$fixture_name/settings/$settings.toml" ]; then
		die "$name declares settings '$settings', but $fixtures/$fixture_name/settings/$settings.toml is missing"
	fi
	: "$fresh"
done

if [ -n "$fixture" ]; then
	case "$fixture" in
	*/* | .*) die "--fixture takes a folder name under $fixtures, got '$fixture'" ;;
	esac
	[ -d "$fixtures/$fixture/project" ] ||
		die "--fixture: $fixtures/$fixture/project is not a folder (see $fixtures)"
fi
if [ -n "$only" ]; then
	printf '%s\n' "$declared" | grep -Fqx -- "$only" ||
		die "--only takes one of the declared tests ($(printf '%s\n' "$declared" | tr '\n' ' ' | sed 's/ $//')), got '$only'"
fi
selected=$(printf '%s\n' "$declarations" | while read -r name fixture_name settings fresh; do
	if [ -n "$only" ] && [ "$name" != "$only" ]; then continue; fi
	if [ -n "$fixture" ] && [ "$fixture_name" != "$fixture" ]; then continue; fi
	echo "$name $fixture_name $settings $fresh"
done)
[ -n "$selected" ] || die "no test declares fixture '$fixture'${only:+ among --only $only}"
tests=$(printf '%s\n' "$selected" | wc -l | tr -d ' ')

[ -z "$timeout" ] && timeout=$((600 + 60 * tests * iterations))
case "$timeout" in
"" | *[!0-9]* | 0*) die "--timeout takes a whole number of seconds, got '$timeout'" ;;
esac

# Whatever this script (or a tool it starts) launches keeps its state out of the user's
# real ~/.local/state/tenmoku (T1664). The tests' app gets scratch state dirs of its own.
TENMOKU_STATE_DIR=$(mktemp -d "${TMPDIR:-/tmp}/tenmoku-e2e-state.XXXXXX")
TENMOKU_CONFIG_DIR=$(mktemp -d "${TMPDIR:-/tmp}/tenmoku-e2e-config.XXXXXX")
trap 'rm -rf "$TENMOKU_STATE_DIR" "$TENMOKU_CONFIG_DIR"' EXIT
# Until xcodebuild starts (when on_signal takes over) there is nothing to stop: a signal
# just exits, through the EXIT trap, with the status it would have.
trap 'echo "e2e-macos: FAILED (SIGINT, before the run)"; exit 130' INT
trap 'echo "e2e-macos: FAILED (SIGTERM, before the run)"; exit 143' TERM
export TENMOKU_STATE_DIR TENMOKU_CONFIG_DIR

[ "$(uname -s)" = Darwin ] || die "macOS only (XCUITest drives AppKit apps)"

absolute() {
	case "$1" in
	/*) printf '%s\n' "$1" ;;
	*) printf '%s\n' "$start_dir/$1" ;;
	esac
}

target=$(absolute "${CARGO_TARGET_DIR:-$root/target}")
e2e="$target/e2e-macos"
stamp=$(date +%Y%m%d-%H%M%S)
results=$(absolute "${results:-$e2e/results/$stamp}")
bundle_dir="$e2e/bundle"
app="$bundle_dir/Tenmoku.app"
project="$e2e/TenmokuE2E.xcodeproj"
derived="$e2e/DerivedData"
scratch="$e2e/scratch/$stamp"
xcresult="$results/TenmokuE2E.xcresult"
attachments="$results/attachments"
xcodebuild_log="$results/xcodebuild.log"
# How long a stopped run gets between TERM and KILL.
grace=10
# Only for exercising the watchdog with a harmless stand-in (tools/e2e-macos/standins);
# never set it otherwise.
xcodebuild=${E2E_XCODEBUILD:-xcodebuild}

# Shell-quotes one word for display, only when it needs it.
quote() {
	case "$1" in
	"" | *[!A-Za-z0-9_./:=,+@%-]*)
		printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"
		;;
	*) printf '%s' "$1" ;;
	esac
}

show() {
	line=""
	for word in "$@"; do
		line="$line${line:+ }$(quote "$word")"
	done
	printf '+ %s\n' "$line"
}

# show, then " >FILE": for a command whose output goes to FILE.
show_to() {
	file=$1
	shift
	printf '%s >%s\n' "$(show "$@")" "$(quote "$file")"
}

run() {
	show "$@"
	[ -n "$dry" ] || "$@"
}

# These functions also run from the signal trap, under set -e, so every test in them is
# an if: a false [ ] ending a loop would end the script mid-cleanup.
#
# Process ownership. A run owns xcodebuild's process group, the runner (which testmanagerd
# starts) and the app under test (which launchd starts), both outside that group and
# matched by their paths in this run's build products and bundle, and every descendant
# of any of them. Ownership is kept across a parent's exit three ways:
#   - every process found is recorded as "pid start-time", and a recorded process that is
#     still the same process (pid and start time) stays owned, orphaned or not, so its
#     later children are found too (F1);
#   - a process group led by an owned process is owned: its members stay in it when the
#     leader exits, so a child a process starts as it exits (on TERM, say) is still found;
#   - before TERM, and before every KILL, the owned processes are frozen (SIGSTOP) and
#     scanned again until a pass finds nothing new: a stopped process starts nothing and
#     orphans nothing, so what is killed is the whole tree as it stands.
# A scan is one `ps -A` snapshot, so it is cheap enough to repeat every 0.2 s while
# cleanup waits. A tracker scans every second while the run lasts. Cleanup, which only the
# main shell runs, signals only owned processes. What this can't follow, macOS having no
# subreaper: a process that, between two scans, starts a child in a new session of its own
# and exits, leaving the child neither its descendant nor in an owned group.
run_pid=""
watcher=""
tail_pid=""
tracker=""
owned="$results/.owned-processes"
owned_groups="$results/.owned-groups"

# Prints the pids owned now, one per line, and records each new one in $owned, and each
# group one leads in $owned_groups. A group is recorded as "pgid leader-start-time" and
# stays owned while it has members and no live process holding its id started at another
# time: so a child left in it after its leader exits is still found, and a group whose id
# is reused after it emptied is never adopted. A group that fails either test is retired
# for good ($owned_groups.retired). xcodebuild's own group is recorded the same way when it
# starts (run_watched). Paths come through the environment, not argv, so the awk can't
# match itself. E2E_SCAN_SNAPSHOT, for tools/e2e-macos/standins/scan-cases only, names a
# file to read in place of `ps`.
# shellcheck disable=SC2016 # an awk program, its $ fields not the shell's
scan_awk='
BEGIN {
	me = ENVIRON["E2E_SCAN_UID"]
	products = ENVIRON["E2E_SCAN_PRODUCTS"]; exe = ENVIRON["E2E_SCAN_EXE"]
	rf = ENVIRON["E2E_SCAN_OWNED"]; gf = ENVIRON["E2E_SCAN_GROUPS"]; xf = gf ".retired"
	while ((getline line < rf) > 0) rec[line] = 1
	close(rf)
	while ((getline line < xf) > 0) retired[line] = 1
	close(xf)
	while ((getline line < gf) > 0) if (!(line in retired)) grp[line] = 1
	close(gf)
}
$5 !~ /^Z/ {
	pid = $1; ppid[pid] = $2; pgid[pid] = $3; uid[pid] = $4
	start[pid] = $6 " " $7 " " $8 " " $9 " " $10
	n++; order[n] = pid
	members[$3]++
	if (((pid " " start[pid]) in rec) || ($4 == me && (index($0, products) || index($0, exe))))
		own[pid] = 1
}
END {
	for (k in grp) {
		g = k; sub(/ .*/, "", g); since = k; sub(/^[^ ]* /, "", since)
		if (!(g in members) || ((g in start) && start[g] != since)) {
			print k >> xf
			continue
		}
		live[g] = 1
	}
	for (i = 1; i <= n; i++) {
		p = order[i]
		if ((pgid[p] in live) && uid[p] == me) own[p] = 1
	}
	do {
		changed = 0
		for (i = 1; i <= n; i++) {
			p = order[i]
			if (!(p in own) && (ppid[p] in own)) { own[p] = 1; changed = 1 }
		}
	} while (changed)
	for (p in own) {
		if (!((p " " start[p]) in rec)) print p " " start[p] >> rf
		if (pgid[p] == p && !((p " " start[p]) in grp) && !((p " " start[p]) in retired))
			print p " " start[p] >> gf
		print p
	}
}'

scan() {
	if [ -n "${E2E_SCAN_SNAPSHOT:-}" ]; then
		cat "$E2E_SCAN_SNAPSHOT"
	else
		ps -A -o pid= -o ppid= -o pgid= -o uid= -o stat= -o lstart= -o command= 2>/dev/null
	fi | E2E_SCAN_UID=$(id -u) \
		E2E_SCAN_PRODUCTS="$derived/Build/Products/" E2E_SCAN_EXE="$app/Contents/MacOS/tenmoku" \
		E2E_SCAN_OWNED="$owned" E2E_SCAN_GROUPS="$owned_groups" awk "$scan_awk" | sort -n
}

# Stops every owned process, then scans and stops again until a pass finds no process it
# hasn't stopped; prints the owned pids, all stopped.
freeze() {
	frozen=""
	passes=0
	while [ "$passes" -lt 20 ]; do
		passes=$((passes + 1))
		now=$(scan)
		if [ "$now" = "$frozen" ]; then break; fi
		frozen=$now
		# shellcheck disable=SC2086 # a list of pids
		if [ -n "$frozen" ]; then kill -STOP $frozen 2>/dev/null || true; fi
	done
	printf '%s\n' "$frozen"
}

# Freezes the owned processes, sends them TERM and lets them go on; KILL, after another
# freeze, to any left $grace seconds later; then KILL again, frozen first, to anything a
# killed process left. Runs only in the main shell, to the end, whatever xcodebuild does
# meanwhile.
cleanup_run() {
	if [ -n "$tracker" ]; then
		kill "$tracker" 2>/dev/null || true
		wait "$tracker" 2>/dev/null || true
		tracker=""
	fi
	pids=$(freeze)
	if [ -n "$pids" ]; then
		# shellcheck disable=SC2086 # a list of pids
		kill -TERM $pids 2>/dev/null || true
		# shellcheck disable=SC2086
		kill -CONT $pids 2>/dev/null || true
	fi
	# By the clock, not by passes: on a loaded Mac a scan can take seconds.
	deadline=$(($(date +%s) + grace))
	while [ -n "$pids" ] && [ "$(date +%s)" -lt "$deadline" ]; do
		sleep 0.2
		pids=$(scan)
	done
	if [ -n "$pids" ]; then
		pids=$(freeze)
		# shellcheck disable=SC2086 # one line of pids
		echo "e2e-macos: still running ${grace}s after TERM, sending KILL to:" $pids >&2
		# shellcheck disable=SC2086
		kill -KILL $pids 2>/dev/null || true
	fi
	tries=0
	while [ "$tries" -lt 3 ]; do
		tries=$((tries + 1))
		sleep 0.5
		pids=$(scan)
		if [ -z "$pids" ]; then break; fi
		pids=$(freeze)
		# shellcheck disable=SC2086 # one line of pids
		echo "e2e-macos: still running after KILL, sending KILL again to:" $pids >&2
		# shellcheck disable=SC2086
		kill -KILL $pids 2>/dev/null || true
	done
	rm -f "$owned" "$owned_groups" "$owned_groups.retired"
}

on_signal() { # on_signal NAME STATUS
	trap - INT TERM
	echo "e2e-macos: got SIG$1; stopping xcodebuild, the runner and the app" >&2
	if [ -n "$watcher" ]; then kill "$watcher" 2>/dev/null || true; fi
	cleanup_run
	if [ -n "$tail_pid" ]; then kill "$tail_pid" 2>/dev/null || true; fi
	echo "e2e-macos: FAILED (SIG$1)"
	exit "$2"
}

# Runs a command in a process group of its own, its output in $xcodebuild_log and shown
# as it comes, then cleans up after it (cleanup_run). After $timeout seconds a watchdog
# sends TERM, and KILL $grace seconds later (after a freeze), to the command's group, so
# the wait below returns, and the status is 124. Everything else is cleanup_run's.
run_watched() {
	: >"$xcodebuild_log"
	: >"$owned"
	: >"$owned_groups"
	: >"$owned_groups.retired"
	set -m
	"$@" >"$xcodebuild_log" 2>&1 &
	run_pid=$!
	set +m
	# Its group, owned with its leader's start time (see scan).
	run_started=$(ps -o lstart= -p "$run_pid" 2>/dev/null | tr -s ' ' | sed 's/^ //; s/ $//')
	if [ -n "$run_started" ]; then echo "$run_pid $run_started" >>"$owned_groups"; fi
	tail -f "$xcodebuild_log" &
	tail_pid=$!
	(
		while :; do
			scan >/dev/null
			sleep 1
		done
	) &
	tracker=$!
	timed_out="$results/.timed-out"
	rm -f "$timed_out"
	(
		waited=0
		while [ "$waited" -lt "$timeout" ]; do
			sleep 1
			kill -0 "$run_pid" 2>/dev/null || exit 0
			waited=$((waited + 1))
		done
		: >"$timed_out"
		echo "e2e-macos: timed out after ${timeout}s; stopping xcodebuild, the runner and the app" >&2
		kill -TERM -- "-$run_pid" 2>/dev/null || true
		waited=0
		while kill -0 -- "-$run_pid" 2>/dev/null && [ "$waited" -lt "$grace" ]; do
			sleep 1
			waited=$((waited + 1))
		done
		# Frozen and recorded first, so a child the group's members started outside it
		# stays owned once they're killed; cleanup_run deals with everything else.
		if kill -0 -- "-$run_pid" 2>/dev/null; then
			freeze >/dev/null
			kill -KILL -- "-$run_pid" 2>/dev/null || true
		fi
	) &
	watcher=$!
	status=0
	wait "$run_pid" || status=$?
	# The watcher only ever touches the group, which cleanup_run covers too.
	kill "$watcher" 2>/dev/null || true
	wait "$watcher" 2>/dev/null || true
	watcher=""
	cleanup_run
	sleep 1
	kill "$tail_pid" 2>/dev/null || true
	wait "$tail_pid" 2>/dev/null || true
	tail_pid=""
	if [ -f "$timed_out" ]; then
		rm -f "$timed_out"
		status=124
	fi
	return "$status"
}

# The tests' own text attachments (ax-tree-*, launched-*, window-title-*, disk-*,
# backups-*, pasteboard-*, scratch-*, tenmoku-log-* and so on, each named
# <kind>-<test>-<iteration>-<n>, n counting the test's records of that kind, or
# tenmoku-log-<test>-<iteration>), copied out of the export under the names the tests
# gave them. Two attachments that would land on one name fail the copy.
copy_named_attachments() {
	manifest="$attachments/manifest.json"
	[ -f "$manifest" ] || return 1
	t=0
	while plutil -extract "$t" raw -o - "$manifest" >/dev/null 2>&1; do
		a=0
		while file=$(plutil -extract "$t.attachments.$a.exportedFileName" raw -o - "$manifest" 2>/dev/null); do
			name=$(plutil -extract "$t.attachments.$a.suggestedHumanReadableName" raw -o - "$manifest") ||
				return 1
			case "$name" in
			failure-*) ;;
			*-test[A-Za-z0-9]*-[0-9][0-9][0-9]*)
				# XCTest appends _<n>_<UUID> to a name; drop it and the extension.
				base=$(printf '%s' "$name" | sed -E 's/(_[0-9]+_[0-9A-F-]{36})?(\.[a-z]+)?$//')
				if [ -e "$results/$base.txt" ]; then
					echo "e2e-macos: two attachments named $base" >&2
					return 1
				fi
				cp "$attachments/$file" "$results/$base.txt" || return 1
				;;
			esac
			a=$((a + 1))
		done
		t=$((t + 1))
	done
}

if [ -n "$dry" ]; then
	echo "# dry run: nothing below is run"
	echo "# would refuse unless 'launchctl managername' is Aqua, /dev/console is $(id -un) and the screen is unlocked"
else
	[ "$(launchctl managername 2>/dev/null)" = Aqua ] ||
		die "no GUI session (launchctl managername is not Aqua): run from a Terminal on the Mac's own login session"
	[ "$(stat -f %Su /dev/console)" = "$(id -un)" ] ||
		die "the console belongs to $(stat -f %Su /dev/console), not $(id -un): log in at the Mac first"
	# A stand-in xcodebuild (E2E_XCODEBUILD) drives no UI, so a locked screen can't hang it.
	if [ -z "${E2E_XCODEBUILD:-}" ] &&
		ioreg -n Root -d1 -a | grep -A1 CGSSessionScreenIsLocked | grep -q '<true/>'; then
		die "the screen is locked, and a UI test hangs on a locked screen: unlock it and rerun"
	fi
	command -v xcodegen >/dev/null 2>&1 || die "xcodegen not found (brew install xcodegen; it is in the Brewfile)"
	command -v xcodebuild >/dev/null 2>&1 || die "xcodebuild not found: install Xcode and select it with xcode-select"
fi

cd "$root"

if [ -n "$build" ]; then
	cargo=cargo
	if ! command -v cargo >/dev/null 2>&1 && [ -x "$HOME/.cargo/bin/cargo" ]; then
		cargo="$HOME/.cargo/bin/cargo"
	fi
	run env CARGO_TARGET_DIR="$target" "$cargo" build --release -p fastedit
fi

# A bundle of its own with an id of its own: the installed copies in /Applications and
# ~/Applications share dev.tenmoku.tenmoku, and are never launched or touched.
run "$root/scripts/bundle-macos.sh" --binary "$target/release/tenmoku" --out "$bundle_dir"
run /usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier $e2e_id" "$app/Contents/Info.plist"
run "$root/scripts/bundle-macos.sh" --sign-only "$app"

# Xcode 27's test runner is sandboxed and can't write here, so the scratch sets are made
# now: <scratch>/<test>/<iteration>/, each a copy of the test's fixture project, a config
# folder (with the declared settings.toml, if any), a state folder (holding a setup.toml
# that keeps the setup wizard away, unless the test declares a fresh state) and a home
# for the app's log.
run mkdir -p "$results"
last=$(printf '%03d' "$iterations")
echo "# fixtures: $tests test(s) x $iterations iteration(s), one scratch set each"
printf '%s\n' "$selected" | while read -r name fixture_name settings fresh; do
	settings=${settings#settings=}
	fresh=${fresh#fresh=}
	sets="$(quote "$scratch/$name")/{001..$last}"
	echo "+ $name: mkdir -p $sets/{config,state,home}; cp -R $(quote "$fixtures/$fixture_name/project") $sets/project"
	if [ -n "$settings" ]; then
		echo "+ $name:   cp $(quote "$fixtures/$fixture_name/settings/$settings.toml") $sets/config/settings.toml"
	fi
	if [ "$fresh" = true ]; then
		echo "+ $name:   (fresh state: no setup.toml, an empty state folder)"
	else
		echo "+ $name:   echo 'outcome = \"skipped\"' >$sets/state/setup.toml"
	fi
	if [ -z "$dry" ]; then
		n=1
		while [ "$n" -le "$iterations" ]; do
			set_dir="$scratch/$name/$(printf '%03d' "$n")"
			mkdir -p "$set_dir/config" "$set_dir/state" "$set_dir/home"
			cp -R "$fixtures/$fixture_name/project" "$set_dir/project"
			if [ -n "$settings" ]; then
				cp "$fixtures/$fixture_name/settings/$settings.toml" "$set_dir/config/settings.toml"
			fi
			if [ "$fresh" != true ]; then
				echo 'outcome = "skipped"' >"$set_dir/state/setup.toml"
			fi
			n=$((n + 1))
		done
	fi
done
run xcodegen generate --spec "$spec" --project "$e2e"

# The runner stays sandboxed: Xcode signs it with the template's entitlements (app
# sandbox, read-only "/") merged with the test target's CODE_SIGN_ENTITLEMENTS. The
# target's add one read-write exception, for this run's scratch folder only, so a test
# can change a scratch file behind the app's back (testRunnerWritesOnlyItsScratch checks
# both sides of it).
entitlements="$e2e/TenmokuE2E.entitlements"
echo "+ write $(quote "$entitlements"): read-write exception for $(quote "$scratch/") only"
# XML-escaped: a checkout or CARGO_TARGET_DIR may hold & or <.
scratch_xml=$(printf '%s/' "$scratch" | sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g')
if [ -z "$dry" ]; then
	cat >"$entitlements" <<ENTITLEMENTS
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>com.apple.security.temporary-exception.files.absolute-path.read-write</key>
	<array>
		<string>$scratch_xml</string>
	</array>
</dict>
</plist>
ENTITLEMENTS
	plutil -lint -s "$entitlements" || die "$entitlements is not a valid property list"
	[ "$(/usr/libexec/PlistBuddy -c 'Print :com.apple.security.temporary-exception.files.absolute-path.read-write:0' "$entitlements" 2>/dev/null)" = "$scratch/" ] ||
		die "$entitlements does not grant $scratch/"
fi

only_args=""
for name in $(printf '%s\n' "$selected" | cut -d' ' -f1); do
	only_args="$only_args -only-testing:TenmokuE2E/SmokeTests/$name"
done
# Every test is named, so a run of the whole suite selects the declared tests only.
iteration_args=""
[ "$iterations" -gt 1 ] && iteration_args="-test-iterations $iterations -test-repetition-relaunch-enabled YES"

trap 'on_signal INT 130' INT
trap 'on_signal TERM 143' TERM

expected_runs=$((tests * iterations))
echo "# in a process group of its own, output also in $(quote "$xcodebuild_log"), with a ${timeout}s"
echo "# watchdog that then sends TERM, and KILL ${grace}s later, to it, the runner and the app"
# shellcheck disable=SC2086 # the two option lists split into words on purpose
show env TEST_RUNNER_E2E_APP="$app" TEST_RUNNER_E2E_SCRATCH="$scratch" \
	TEST_RUNNER_E2E_ITERATIONS="$iterations" TEST_RUNNER_E2E_FIXTURES="$fixtures" \
	xcodebuild test -project "$project" -scheme TenmokuE2E -destination platform=macOS \
	-derivedDataPath "$derived" -resultBundlePath "$xcresult" CODE_SIGN_ENTITLEMENTS="$entitlements" \
	-test-timeouts-enabled YES -default-test-execution-time-allowance 120 \
	-maximum-test-execution-time-allowance 300 $iteration_args $only_args
test_status=0
if [ -z "$dry" ]; then
	# shellcheck disable=SC2086
	run_watched env TEST_RUNNER_E2E_APP="$app" TEST_RUNNER_E2E_SCRATCH="$scratch" \
		TEST_RUNNER_E2E_ITERATIONS="$iterations" TEST_RUNNER_E2E_FIXTURES="$fixtures" \
		"$xcodebuild" test -project "$project" -scheme TenmokuE2E -destination platform=macOS \
		-derivedDataPath "$derived" -resultBundlePath "$xcresult" CODE_SIGN_ENTITLEMENTS="$entitlements" \
		-test-timeouts-enabled YES -default-test-execution-time-allowance 120 \
		-maximum-test-execution-time-allowance 300 $iteration_args $only_args ||
		test_status=$?
fi

# Each step is checked on its own, so a failure here still ends in the result locations
# and a failing final line, and never hides the tests' own status.
artifacts_ok=1
missing=""
note_missing() {
	artifacts_ok=""
	missing="$missing${missing:+, }$1"
}
run xcrun xcresulttool export attachments --path "$xcresult" --output-path "$attachments" ||
	note_missing "attachments"
show_to "$results/tests.json" xcrun xcresulttool get test-results tests --path "$xcresult"
if [ -z "$dry" ]; then
	xcrun xcresulttool get test-results tests --path "$xcresult" >"$results/tests.json" ||
		note_missing "tests.json"
fi
show_to "$results/summary.json" xcrun xcresulttool get test-results summary --path "$xcresult"
if [ -z "$dry" ]; then
	xcrun xcresulttool get test-results summary --path "$xcresult" >"$results/summary.json" ||
		note_missing "summary.json"
fi
echo "+ copy each named text attachment in $(quote "$attachments/manifest.json") to $(quote "$results")/<name>.txt"
if [ -z "$dry" ]; then
	copy_named_attachments || note_missing "named attachments"
fi

# A pass must have run every test the options asked for, as often as they asked: the
# summary counts tests, and the log counts each run of one.
echo "# check: summary.json has $tests tests, all passed; $(quote "$xcodebuild_log") has $expected_runs passing test runs"
count_problem=""
if [ -z "$dry" ]; then
	finished=$(grep -cE "^Test Case '-\[TenmokuE2E\.SmokeTests [A-Za-z0-9_]+\]' (passed|failed) " "$xcodebuild_log" || true)
	passed=$(grep -cE "^Test Case '-\[TenmokuE2E\.SmokeTests [A-Za-z0-9_]+\]' passed " "$xcodebuild_log" || true)
	total=$(plutil -extract totalTestCount raw -o - "$results/summary.json" 2>/dev/null || echo "?")
	summary_passed=$(plutil -extract passedTests raw -o - "$results/summary.json" 2>/dev/null || echo "?")
	# Each test run in the order it ran, with its time.
	grep -E "^Test Case '-\[TenmokuE2E\.SmokeTests [A-Za-z0-9_]+\]' (passed|failed) " "$xcodebuild_log" |
		sed -E "s/^Test Case '-\[TenmokuE2E\.SmokeTests ([A-Za-z0-9_]+)\]' (passed|failed) \(([0-9.]+) seconds\)\.?$/run: \1 \2 \3 s/" || true
	echo "test runs: $passed passed of $finished finished, $expected_runs expected; summary: $summary_passed passed of $total tests"
	if [ "$passed" != "$expected_runs" ] || [ "$finished" != "$expected_runs" ] ||
		[ "$total" != "$tests" ] || [ "$summary_passed" != "$tests" ]; then
		count_problem="expected $expected_runs passing test runs of $tests tests; the log shows $passed of $finished and the summary $summary_passed of $total"
	fi
	# Every launch record, a relaunch's second one included, must name only the bundle
	# under test.
	records=0
	others=0
	for record in "$results"/launched-*.txt; do
		[ -f "$record" ] || continue
		records=$((records + 1))
		if ! grep -q '^pid ' "$record" ||
			grep '^pid ' "$record" | grep -Fqv " executable $app/Contents/MacOS/tenmoku "; then
			others=$((others + 1))
			echo "e2e-macos: $(basename "$record") names no process or another one: $(head -n 1 "$record")" >&2
		fi
	done
	echo "launch records: $records, of which $others name a $e2e_id process other than $(quote "$app"), or none"
	if [ "$others" != 0 ] && [ -z "$count_problem" ]; then
		count_problem="$others of $records launch records name no process or another one than the bundle under test"
	fi
fi
echo "result bundle: $xcresult"
echo "attachments: $attachments"
echo "AX dumps, launch and disk records: $results"
if [ -n "$dry" ]; then
	echo "e2e-macos: dry run, nothing ran"
elif [ "$test_status" -eq 124 ]; then
	echo "e2e-macos: FAILED (timed out after ${timeout}s)"
	exit 124
elif [ "$test_status" -ne 0 ]; then
	echo "e2e-macos: FAILED (xcodebuild test exited $test_status)"
	exit "$test_status"
elif [ -z "$artifacts_ok" ]; then
	echo "e2e-macos: FAILED (xcodebuild exited 0, but these could not be exported: $missing)"
	exit 1
elif [ -n "$count_problem" ]; then
	echo "e2e-macos: FAILED ($count_problem)"
	exit 1
else
	echo "e2e-macos: passed"
fi
