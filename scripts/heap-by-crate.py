#!/usr/bin/env python3
"""Fold live malloc stacks by Rust crate (and C library / system framework).

Recipe (macOS, no code changes):

  1. Build with symbols. [profile.release] strips them; [profile.bench]
     keeps the symbol table:
         cargo build --profile bench -p fastedit        # binary: target/release/tenmoku
         # (the bench profile builds into target/release)
     With LTO many frames are inlined into their callers and are charged to
     the caller's crate. For inlined frames in the stacks add line tables
     (macOS packs a dSYM next to the binary):
         CARGO_PROFILE_BENCH_DEBUG=line-tables-only cargo build --profile bench -p fastedit
  2. Launch with stack logging on, from a shell:
         MallocStackLogging=lite target/release/tenmoku path/to/file
     (`open` on a bundle does not pass your shell's environment; set it for
     launchd with `launchctl setenv MallocStackLogging lite` first, and
     `launchctl unsetenv MallocStackLogging` afterwards.)
  3. Let it settle (scroll, open the file), then, from another terminal:
         python3 scripts/heap-by-crate.py --pid $(pgrep -n tenmoku)
     or save the stacks and fold the file (`-` reads stdin):
         malloc_history $(pgrep -n tenmoku) -allBySize > heap.txt
         python3 scripts/heap-by-crate.py heap.txt
     or save the heap with stacks to analyse offline:
         leaks $(pgrep -n tenmoku) --outputGraph=heap.memgraph
         malloc_history heap.memgraph -allBySize > heap.txt
  4. Add the categories malloc tools cannot see (GPU memory, IOSurface,
     CG Image, __TEXT) beside the heap rows:
         python3 scripts/heap-by-crate.py heap.txt --footprint $(pgrep -n tenmoku)
     (--footprint takes a pid, or a saved `footprint -p <pid>` output file.)

Each live allocation is charged to the first frame, walking from the
allocator up, that is not allocator or std plumbing (libsystem_*,
alloc::, core::, std::, hashbrown::, smallvec::, `<alloc::..>` impls; add
more with --skip PREFIX). Crate is the first path segment; `<X as Y>` impls
are charged to X. ts_* is "tree-sitter (C)", wasmtime_*/wasm_* is
"wasmtime", frames in another image are charged to that image's name
(CoreText, AppKit, Metal, ...). Stacks with no attributable frame are
"unattributed". --depth 2 splits crates by module. The total equals the
input's. Stack logging adds overhead and the process is only read, not
stopped. An xctrace Allocations recording
could not be exported (the record hangs and leaves a trace with no template;
see PERF.md), so there is no --xctrace input.
"""
import argparse
import os
import re
import shutil
import subprocess
import sys

MAIN_IMAGES = ("tenmoku", "fastedit")
PLUMBING_IMAGES = ("libsystem_", "libdyld", "dyld", "libdispatch", "libc++", "libunwind")
RUST_PLUMBING = ("alloc::", "core::", "std::", "hashbrown::", "smallvec::",
                 "__rust_", "__rdl_", "_rust_", "_rdl_", "rust_")
STACK_RE = re.compile(r"^\s*(\d+) calls? for (\d+) bytes: (.*)$")
FRAME_RE = re.compile(r"^(?:0x[0-9a-fA-F]+\s+)?\(([^)]*)\)\s*(.*)$")
HASH_RE = re.compile(r"::h[0-9a-f]{16}$")


def demangle_legacy(s):
    m = re.match(r"^_?_ZN(.*)E$", s)
    if not m:
        return None
    rest, parts = m.group(1), []
    while rest:
        n = re.match(r"(\d+)", rest)
        if not n:
            return None
        k = int(n.group(1))
        parts.append(rest[n.end():n.end() + k])
        rest = rest[n.end() + k:]
    if parts and re.match(r"^h[0-9a-f]{16}$", parts[-1]):
        parts.pop()
    out = []
    for p in parts:
        p = p.lstrip("_") if p.startswith("_$") else p
        for a, b in (("$LT$", "<"), ("$GT$", ">"), ("$u20$", " "), ("$C$", ","),
                     ("$RF$", "&"), ("$LP$", "("), ("$RP$", ")"), ("$u7b$", "{"),
                     ("$u7d$", "}"), ("..", "::")):
            p = p.replace(a, b)
        out.append(p)
    return "::".join(out)


class _V0:
    """The Rust v0 path grammar, generic arguments parsed and dropped."""
    B62 = "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"

    def __init__(self, s):
        self.s, self.i, self.depth = s, 0, 0

    def peek(self):
        return self.s[self.i] if self.i < len(self.s) else ""

    def take(self):
        c = self.peek()
        if not c:
            raise ValueError("eof")
        self.i += 1
        return c

    def b62(self):
        n = 0
        while True:
            c = self.take()
            if c == "_":
                return n + 1 if n or self.s[self.i - 2:self.i - 1] != "" and False else n + 1
            n = n * 62 + self.B62.index(c)

    def num(self):
        m = re.compile(r"\d+").match(self.s, self.i)
        if not m:
            raise ValueError("num")
        self.i = m.end()
        return int(m.group())

    def ident(self):
        if self.peek() == "s":  # disambiguator
            self.i += 1
            self.b62_tail()
        if self.peek() == "u":
            raise ValueError("punycode")
        n = self.num()
        if self.peek() == "_":
            self.i += 1
        name = self.s[self.i:self.i + n]
        if len(name) != n:
            raise ValueError("ident")
        self.i += n
        return name

    def b62_tail(self):
        while self.take() != "_":
            pass

    def backref(self):
        self.b62_tail_value = None
        start = self.i
        while self.take() != "_":
            pass
        digits = self.s[start:self.i - 1]
        off = 0
        for c in digits:
            off = off * 62 + self.B62.index(c)
        off = off + 1 if digits else 0
        return off

    def at(self, off, fn):
        self.depth += 1
        if self.depth > 16 or off >= self.i:
            raise ValueError("backref")
        save = self.i
        self.i = off
        try:
            return fn()
        finally:
            self.i = save
            self.depth -= 1

    def path(self):
        self.depth += 1
        try:
            if self.depth > 24:
                raise ValueError("too deep")
            return self._path()
        finally:
            self.depth -= 1

    def _path(self):
        c = self.take()
        if c == "C":
            return self.ident()
        if c == "N":
            ns = self.take()
            base = self.path()
            if self.peek() == "s":
                self.i += 1
                self.b62_tail()
            n = self.ident() if self.peek() != "_" else ""
            if ns.isupper() and ns not in "CS":
                pass
            return f"{base}::{n or '{closure}'}"
        if c == "M":
            self.impl_path()
            return f"<{self.type()}>"
        if c == "X":
            self.impl_path()
            t = self.type()
            self.path()
            return f"<{t}>"
        if c == "Y":
            t = self.type()
            self.path()
            return f"<{t}>"
        if c == "I":
            base = self.path()
            while self.peek() != "E":
                if not self.peek():
                    raise ValueError("eof")
                self.generic()
            self.i += 1
            return base
        if c == "B":
            return self.at(self.backref(), self.path)
        raise ValueError("path " + c)

    def impl_path(self):
        if self.peek() == "s":
            self.i += 1
            self.b62_tail()
        self.path()

    def generic(self):
        c = self.peek()
        if c == "L":
            self.i += 1
            self.b62_tail()
        elif c == "K":
            self.i += 1
            self.const()
        else:
            self.type()

    def const(self):
        c = self.peek()
        if c == "p":
            self.i += 1
        elif c == "B":
            self.i += 1
            self.at(self.backref(), self.const)
        else:
            self.type()
            if self.peek() == "n":
                self.i += 1
            while self.take() != "_":
                pass

    def type(self):
        self.depth += 1
        try:
            if self.depth > 24:
                raise ValueError("too deep")
            return self._type()
        finally:
            self.depth -= 1

    def _type(self):
        c = self.peek()
        if not c:
            raise ValueError("eof")
        if c in "abcdefhijlmnopstuvxyz":
            self.i += 1
            return ""
        if c in "RQ":
            self.i += 1
            if self.peek() == "L":
                self.i += 1
                self.b62_tail()
            return self.type()
        if c in "PO":
            self.i += 1
            return self.type()
        if c in "AS":
            self.i += 1
            t = self.type()
            if c == "A":
                self.const()
            return t
        if c == "F":
            self.i += 1
            if self.peek() == "G":
                self.i += 1
                self.b62_tail()
            if self.peek() == "U":
                self.i += 1
            if self.peek() == "K":
                self.i += 1
                if self.peek() == "C":
                    self.i += 1
                else:
                    self.ident()
            while self.peek() != "E":
                self.type()
            self.i += 1
            self.type()
            return ""
        if c == "T":
            self.i += 1
            while self.peek() != "E":
                self.type()  # raises at EOF
            self.i += 1
            return ""
        if c == "B":
            self.i += 1
            return self.at(self.backref(), self.type)
        return self.path()


def demangle_v0(s):
    m = re.match(r"^_?_R(.*)$", s)
    if not m:
        return None
    body = m.group(1)
    body = re.sub(r"^\d+", "", body)  # encoding version
    try:
        v = _V0(body)
        out = v.path()
        return out or None
    except (ValueError, IndexError, RecursionError):
        pass
    # Fallback heuristic: the first crate identifier, then the identifiers.
    m = re.search(r"C(?:s[0-9a-zA-Z]*_)?(\d+)", body)
    if not m:
        return None
    segs, rest = [], body[m.start():]
    c = re.match(r"C(?:s[0-9a-zA-Z]*_)?", rest)
    rest = rest[c.end():]
    while True:
        n = re.match(r"(?:[A-Z]|s[0-9a-zA-Z]*_)*?(\d+)", rest)
        if not n or n.start() > 12:
            break
        k = int(n.group(1))
        ident = rest[n.end():n.end() + k]
        if len(ident) != k:
            break
        segs.append(ident)
        rest = rest[n.end() + k:]
    return "::".join(segs) if segs else None


_EXT = {}


def external_demangle(sym):
    """rustfilt, else c++filt (which reads Rust v0 and legacy), when on PATH."""
    if not sym.startswith(("_R", "__R", "_ZN", "__ZN")) or os.environ.get("HEAP_BY_CRATE_NO_EXT"):
        return None
    tool = _EXT.get("tool", False)
    if tool is False:
        tool = next((t for t in ("rustfilt", "c++filt") if shutil.which(t)), None)
        _EXT["tool"] = tool
    if not tool:
        return None
    key = sym.removeprefix("_") if sym.startswith(("__R", "__ZN")) else sym
    if key in _EXT:
        return _EXT[key]
    try:
        out = subprocess.run([tool, key], capture_output=True, text=True, timeout=5).stdout.strip()
    except (OSError, subprocess.SubprocessError):
        out = ""
    out = out if out and out != key else None
    _EXT[key] = out
    return out


def demangle(sym):
    sym = re.sub(r"\s*\+\s*\d+$", "", sym.strip())
    sym = re.sub(r"\s*\(.*:\d+\)$", "", sym)  # (file:line) from dSYM
    sym = re.sub(r"\s+\S+\.\w+:\d+$", "", sym)  # "  file.rs:171" from line tables
    d = external_demangle(sym)
    if d:
        return HASH_RE.sub("", d)
    for fn in (demangle_legacy, demangle_v0):
        d = fn(sym)
        if d:
            return d
    return HASH_RE.sub("", sym)


def strip_generics(s):
    """Drop balanced <...> groups (`->` is not a closer)."""
    s = s.replace("->", "\u2192")
    out, depth = [], 0
    for ch in s:
        if ch == "<":
            depth += 1
        elif ch == ">" and depth:
            depth -= 1
        elif not depth:
            out.append(ch)
    return "".join(out)


def self_type(path):
    # `<A as B>::m` / `<A>::m` / `<impl X for Y>::m`: charge to A (Y).
    if not path.startswith("<"):
        return path
    p = path.replace("->", "\u2192")
    depth, end = 0, len(p)
    for i, ch in enumerate(p):
        depth += ch == "<"
        depth -= ch == ">"
        if depth == 0:
            end = i
            break
    inner = p[1:end]
    depth, cut = 0, len(inner)
    for i, ch in enumerate(inner):
        depth += ch == "<"
        depth -= ch == ">"
        if depth == 0 and inner.startswith(" as ", i):
            cut = i
            break
    inner = inner[:cut]
    m = re.match(r"impl\s+.+?\s+for\s+(.*)$", inner)
    if m:
        inner = m.group(1)
    inner = re.sub(r"^(&(mut\s+)?|dyn\s+|impl\s+|\*(const|mut)\s+)+", "", inner)
    return inner


def classify(image, sym, depth, skip):
    """Return a key for the frame, or None if it is plumbing / unknown."""
    base = re.sub(r"(\.A)?\.dylib$", "", image.removeprefix("com.apple."))
    if image.startswith(PLUMBING_IMAGES):
        return None
    d = demangle(sym)
    if skip and (d.startswith(skip) or sym.startswith(skip) or base.startswith(skip)):
        return None  # --skip applies before any attribution
    if image and not image.startswith(MAIN_IMAGES):
        return base  # another image (CoreText, AppKit, Metal, ...)
    if not d or d in ("???",) or d.startswith("0x"):
        return None
    if d.startswith(("ts_", "_ts_")):
        return "tree-sitter (C)"
    if d.startswith(("wasmtime_", "wasm_", "_wasmtime_", "_wasm_")):
        return "wasmtime"
    if "::" not in d and not d.startswith("<"):
        return None  # an unknown C frame in the app image
    path = self_type(d)
    if path.startswith(RUST_PLUMBING) or path.startswith(skip) or d.startswith(skip):
        return None
    segs = [x for x in re.split(r"::", strip_generics(path)) if x]
    if not segs:
        return None
    if depth > 1 and len(segs) > 1:
        return "::".join(segs[:depth])
    return segs[0]


def fold(text, depth, skip):
    rows, total, n = {}, 0, 0
    for line in text.splitlines():
        m = STACK_RE.match(line)
        if not m:
            continue
        n += 1
        count, size = int(m.group(1)), int(m.group(2))
        total += size
        frames = [f.strip() for f in m.group(3).split(" | ")]
        key = None
        for f in reversed(frames):  # leaf (allocator) first
            fm = FRAME_RE.match(re.sub(r"^0x[0-9a-fA-F]+\s+", "", f) if f.startswith("0x") else f)
            if not fm:
                continue
            key = classify(fm.group(1), fm.group(2), depth, skip)
            if key:
                break
        key = key or "unattributed"
        r = rows.setdefault(key, [0, 0])
        r[0] += size
        r[1] += count
    return rows, total, n


def human(b):
    for u, s in (("GB", 1 << 30), ("MB", 1 << 20), ("KB", 1 << 10)):
        if b >= s:
            return f"{b / s:.1f} {u}"
    return f"{b} B"


def main():
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("input", nargs="?", help="malloc_history -allBySize output file, or - for stdin")
    ap.add_argument("--pid", type=int, help="run malloc_history <pid> -allBySize")
    ap.add_argument("--depth", type=int, default=1, help="path segments in a row (2 = crate::module)")
    ap.add_argument("--top", type=int, default=0, help="show only the N biggest rows")
    ap.add_argument("--skip", action="append", default=[], metavar="PREFIX",
                    help="extra frame path prefix to treat as plumbing")
    ap.add_argument("--footprint", metavar="PID|FILE",
                    help="append footprint -p <pid> (or a saved output file)")
    a = ap.parse_args()
    try:
        if a.pid:
            p = subprocess.run(["malloc_history", str(a.pid), "-allBySize"],
                               capture_output=True, text=True)
            if p.returncode:
                sys.exit(f"heap-by-crate: malloc_history failed: {p.stderr.strip() or p.returncode}")
            text = p.stdout
        elif a.input == "-":
            text = sys.stdin.read()
        elif a.input:
            with open(a.input, errors="replace") as f:
                text = f.read()
        else:
            ap.error("give an input file, - or --pid")
    except OSError as e:
        sys.exit(f"heap-by-crate: cannot read input: {e}")
    rows, total, n = fold(text, max(a.depth, 1), tuple(a.skip))
    if not n:
        sys.exit("heap-by-crate: no 'N calls for B bytes:' stacks in the input "
                 "(was the process launched with MallocStackLogging=lite?)")
    ordered = sorted(rows.items(), key=lambda kv: -kv[1][0])
    shown = ordered[:a.top] if a.top else ordered
    w = max([len(k) for k, _ in shown] + [5])
    print(f"{'crate':<{w}}  {'live bytes':>12}  {'%':>6}  {'allocations':>11}")
    for k, (b, c) in shown:
        print(f"{k:<{w}}  {b:>12}  {100 * b / total:>5.1f}%  {c:>11}")
    if len(shown) < len(ordered):
        b = sum(v[0] for _, v in ordered[len(shown):])
        c = sum(v[1] for _, v in ordered[len(shown):])
        print(f"{'(others)':<{w}}  {b:>12}  {100 * b / total:>5.1f}%  {c:>11}")
    print(f"{'total':<{w}}  {total:>12}  100.0%  {sum(v[1] for v in rows.values()):>11}"
          f"   ({human(total)} live heap)")
    if a.footprint:
        try:
            if a.footprint.isdigit():
                p = subprocess.run(["footprint", "-p", a.footprint],
                                   capture_output=True, text=True)
                if p.returncode:
                    sys.exit(f"heap-by-crate: footprint failed: {p.stderr.strip() or p.returncode}")
                out = p.stdout
            else:
                with open(a.footprint, errors="replace") as f:
                    out = f.read()
        except OSError as e:
            sys.exit(f"heap-by-crate: cannot read footprint: {e}")
        print("\nfootprint (malloc tools see the heap rows only):")
        print(out.rstrip())


if __name__ == "__main__":
    main()
