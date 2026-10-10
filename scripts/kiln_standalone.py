#!/usr/bin/env python3
"""Lays kiln/ out as a workspace of its own, and lists what it still needs
from this repository. scripts/check-framework-standalone.sh drives it.

    kiln_standalone.py prepare REPO SCRATCH   # copy kiln/, write its manifest
    kiln_standalone.py lock REPO SCRATCH      # its Cargo.lock is a subset of ours
    kiln_standalone.py report REPO            # what kiln/ needs from outside

`prepare` derives the scratch workspace from the root Cargo.toml instead of
hard-coding it: only the [workspace.package] keys, [workspace.dependencies]
entries, lints and patches the kiln crates actually reference are copied, so
the generated manifest is also the list of what kiln would have to own.
"""

import re
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

KILN = "kiln"
DEP_TABLES = ("dependencies", "dev-dependencies", "build-dependencies")


def load(path: Path) -> dict:
    with path.open("rb") as f:
        return tomllib.load(f)


def kiln_manifests(repo: Path) -> dict[str, dict]:
    """Each kiln crate's name and parsed manifest."""
    found = {}
    for manifest in sorted((repo / KILN / "crates").glob("*/Cargo.toml")):
        data = load(manifest)
        if "package" in data:
            found[data["package"]["name"]] = data
    return found


def dep_tables(manifest: dict):
    """Every dependency table, including the per-target ones."""
    for table in DEP_TABLES:
        yield manifest.get(table, {})
    for target in manifest.get("target", {}).values():
        for table in DEP_TABLES:
            yield target.get(table, {})


def workspace_uses(manifests: dict[str, dict]):
    """The [workspace.package] keys, dependency keys and lints the crates inherit."""
    package_keys, dep_keys, lints = set(), set(), False
    for manifest in manifests.values():
        for key, value in manifest["package"].items():
            if isinstance(value, dict) and value.get("workspace"):
                package_keys.add(key)
        for table in dep_tables(manifest):
            for key, value in table.items():
                if isinstance(value, dict) and value.get("workspace"):
                    dep_keys.add(key)
        if manifest.get("lints", {}).get("workspace"):
            lints = True
    return package_keys, dep_keys, lints


def lock_packages(lock: dict) -> dict[str, list[dict]]:
    by_name: dict[str, list[dict]] = {}
    for package in lock["package"]:
        by_name.setdefault(package["name"], []).append(package)
    return by_name


def lock_closure(lock: dict, roots: set[str]) -> set[str]:
    """Package names reachable from `roots` in Cargo.lock. The lock holds every
    target's and feature's dependencies, so this is a superset of any one
    build's graph: what a standalone kiln may need on some platform."""
    by_name = lock_packages(lock)
    seen, todo = set(), list(roots)
    while todo:
        name = todo.pop()
        if name in seen:
            continue
        seen.add(name)
        # Entries are "name", "name version" or "name version (source)";
        # following every version of a name only widens the superset.
        for package in by_name.get(name, []):
            for dep in package.get("dependencies", []):
                todo.append(dep.split(" ")[0])
    return seen


def toml_value(value) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (int, float)):
        return str(value)
    if isinstance(value, str):
        escaped = value.replace("\\", "\\\\").replace('"', '\\"')
        return f'"{escaped}"'
    if isinstance(value, list):
        return "[" + ", ".join(toml_value(v) for v in value) + "]"
    if isinstance(value, dict):
        return "{ " + ", ".join(f"{toml_key(k)} = {toml_value(v)}" for k, v in value.items()) + " }"
    raise TypeError(f"can't write {value!r} as TOML")


def toml_key(key: str) -> str:
    return key if re.fullmatch(r"[A-Za-z0-9_-]+", key) else toml_value(key)


def table(name: str, entries: dict) -> list[str]:
    lines = [f"[{name}]"]
    lines += [f"{toml_key(k)} = {toml_value(v)}" for k, v in entries.items()]
    return lines + [""]


def copy_kiln(repo: Path, scratch: Path) -> int:
    """kiln/ as `git filter-repo --subdirectory-filter kiln` would leave it:
    tracked files plus new ones not yet committed, nothing git ignores."""
    listed = subprocess.run(
        ["git", "-C", str(repo), "ls-files", "-z", "--cached", "--others",
         "--exclude-standard", "--", KILN],
        check=True, capture_output=True,
    ).stdout.decode().split("\0")
    count = 0
    for rel in filter(None, listed):
        source = repo / rel
        if not source.is_file():  # Deleted in the work tree but still tracked.
            continue
        dest = scratch / Path(rel).relative_to(KILN)
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, dest)
        count += 1
    return count


def prepare(repo: Path, scratch: Path) -> int:
    copied = copy_kiln(repo, scratch)
    print(f"copied:      {repo / KILN}/ -> {scratch}/ ({copied} files)")
    root = load(repo / "Cargo.toml")
    manifests = kiln_manifests(repo)
    package_keys, dep_keys, lints = workspace_uses(manifests)
    workspace = root["workspace"]
    errors = []

    deps = {}
    for key in sorted(dep_keys):
        entry = workspace.get("dependencies", {}).get(key)
        if entry is None:
            errors.append(f"[workspace.dependencies] has no `{key}`")
            continue
        if isinstance(entry, dict) and "path" in entry:
            path = Path(entry["path"])
            if path.parts[:1] != (KILN,):
                errors.append(f"`{key}` is a path dependency outside {KILN}/: {entry['path']}")
                continue
            # The scratch workspace is kiln/ itself.
            entry = {**entry, "path": path.relative_to(KILN).as_posix()}
        deps[key] = entry

    lock = load(repo / "Cargo.lock")
    closure = lock_closure(lock, set(manifests))
    patches, vendored = {}, []
    for name, entry in workspace_patches(root).items():
        if name not in closure:
            continue
        if "path" in entry:
            source = Path(entry["path"])
            if source.parts[:1] == (KILN,):
                entry = {**entry, "path": source.relative_to(KILN).as_posix()}
            else:
                # Copied beside kiln's crates under the same relative path.
                shutil.copytree(repo / source, scratch / source, symlinks=True)
                vendored.append(source.as_posix())
        patches[name] = entry

    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        return 1

    lines = [
        "# Generated by scripts/check-framework-standalone.sh from the fastedit",
        "# root Cargo.toml: only the entries the kiln crates reference.",
        "[workspace]",
        f'resolver = {toml_value(workspace.get("resolver", "3"))}',
        'members = ["crates/*"]',
        "",
    ]
    if package_keys:
        missing = package_keys - workspace.get("package", {}).keys()
        if missing:
            print(f"error: [workspace.package] lacks {sorted(missing)}", file=sys.stderr)
            return 1
        lines += table(
            "workspace.package", {k: workspace["package"][k] for k in sorted(package_keys)}
        )
    if deps:
        lines += table("workspace.dependencies", deps)
    if lints:
        for tool, entries in workspace.get("lints", {}).items():
            lines += table(f"workspace.lints.{tool}", entries)
    if patches:
        lines += table("patch.crates-io", patches)
    # Profiles don't change what builds, but GPUI is very slow unoptimised:
    # keep them, with per-package overrides only for packages kiln pulls in.
    for profile, settings in root.get("profile", {}).items():
        flat = {k: v for k, v in settings.items() if k != "package"}
        if flat:
            lines += table(f"profile.{profile}", flat)
        for package, overrides in settings.get("package", {}).items():
            if package in closure:
                lines += table(f"profile.{profile}.package.{toml_key(package)}", overrides)

    manifest = scratch / "Cargo.toml"
    manifest.write_text("\n".join(lines), encoding="utf-8")
    load(manifest)  # Fails loudly if the writer above produced bad TOML.

    print(f"crates:      {', '.join(sorted(manifests))}")
    print(f"package:     {', '.join(sorted(package_keys)) or '(none)'}")
    print(f"deps:        {', '.join(deps) or '(none)'}")
    print(f"lints:       {'workspace' if lints else '(none)'}")
    unused = sorted(set(workspace_patches(root)) - set(patches))
    skipped = f"; not needed: {', '.join(unused)}" if unused else ""
    print(f"patches:     {', '.join(patches) or '(none)'}{skipped}")
    print(f"vendored:    {', '.join(vendored) or '(none)'}")
    print(f"manifest:    {manifest}")
    return 0


def workspace_patches(root: dict) -> dict:
    return root.get("patch", {}).get("crates-io", {})


def check_lock(repo: Path, scratch: Path) -> int:
    """Every package the scratch lock keeps is pinned exactly as in ours."""
    ours = {
        (p["name"], p["version"]): p.get("source") for p in load(repo / "Cargo.lock")["package"]
    }
    errors, kept = [], 0
    for package in load(scratch / "Cargo.lock")["package"]:
        key = (package["name"], package["version"])
        if key not in ours:
            errors.append(f"{key[0]} {key[1]} isn't in the repo's Cargo.lock")
        elif ours[key] != package.get("source"):
            errors.append(f"{key[0]} {key[1]} comes from {package.get('source')}, not {ours[key]}")
        kept += 1
    for error in errors:
        print(f"error: {error}", file=sys.stderr)
    if not errors:
        print(f"lock:        {kept} of {len(ours)} packages, each at the repo's version")
    return 1 if errors else 0


# Repository paths kiln's sources and docs mention, e.g. docs/decisions/….
REPO_PATH = re.compile(r"\b((?:docs|crates|scripts|vendor|tools|\.github)/[\w./-]*\w)")
# THIRD_PARTY.md table rows for kiln files: | `kiln/…` | source | …
THIRD_PARTY_ROW = re.compile(r"^\|\s*`(kiln/[^`]*)`\s*\|\s*([^|]*)\|", re.MULTILINE)
NOTICE_FILES = ("LICENSE", "LICENSE-APACHE", "LICENSE-MIT", "NOTICE", "THIRD_PARTY.md", "README.md", "CLEAN_ROOM.md")


def report(repo: Path) -> int:
    """What kiln/ needs from outside itself. Informational: never fails."""
    root = load(repo / "Cargo.toml")
    manifests = kiln_manifests(repo)
    package_keys, dep_keys, lints = workspace_uses(manifests)
    closure = lock_closure(load(repo / "Cargo.lock"), set(manifests))
    workspace = root["workspace"]

    print("kiln/ needs from outside itself:")
    inherited = [f"{k} = {workspace['package'][k]}" for k in sorted(package_keys)]
    print(f"  [workspace.package]: {', '.join(inherited)}")
    external = sorted(k for k in dep_keys
                      if not (isinstance(workspace["dependencies"][k], dict)
                              and "path" in workspace["dependencies"][k]))
    print(f"  workspace dependencies ({len(external)}): {', '.join(external)}")
    if lints:
        flat = [f"{tool}::{name}={level}" for tool, entries in workspace.get("lints", {}).items()
                for name, level in entries.items()]
        print(f"  workspace lints: {', '.join(flat)}")
    needed = [n for n in workspace_patches(root) if n in closure]
    print(f"  [patch.crates-io]: {', '.join(needed) or 'none needed'}")
    kept = [p for s in root.get("profile", {}).values() for p in s.get("package", {}) if p in closure]
    if kept:
        print(f"  profile opt-levels: {', '.join(sorted(set(kept)))}")
    for name in ("rust-toolchain.toml", "rust-toolchain"):
        if (repo / name).exists() and not (repo / KILN / name).exists():
            print(f"  toolchain: {name} (kiln/ has none)")
    nextest = repo / ".config" / "nextest.toml"
    if nextest.exists():
        text = nextest.read_text(encoding="utf-8")
        pkgs = sorted(set(re.findall(r"package\((kiln[\w-]*)\)", text)))
        if pkgs and not (repo / KILN / ".config" / "nextest.toml").exists():
            print(f"  .config/nextest.toml: retry overrides for {', '.join(pkgs)} (kiln/ has none)")
    if (repo / "deny.toml").exists() and not (repo / KILN / "deny.toml").exists():
        print("  deny.toml: licence and advisory policy (kiln/ has none)")
    for build in sorted((repo / KILN).rglob("build.rs")):
        print(f"  build script: {build.relative_to(repo)}")

    mentions: dict[str, set[str]] = {}
    for path in sorted((repo / KILN).rglob("*")):
        if not path.is_file() or path.suffix not in (".rs", ".toml", ".md"):
            continue
        text = path.read_text(encoding="utf-8")
        for target in REPO_PATH.findall(text):
            mentions.setdefault(target, set()).add(path.relative_to(repo).as_posix())
    if mentions:
        print("  repo paths mentioned in kiln/ (docs and comments; not shipped with it):")
        for target, files in sorted(mentions.items()):
            print(f"    {target}  ({len(files)} file{'s' if len(files) > 1 else ''})")

    third = repo / "THIRD_PARTY.md"
    rows = THIRD_PARTY_ROW.findall(third.read_text(encoding="utf-8")) if third.exists() else []
    if rows:
        print("  THIRD_PARTY.md rows for kiln files (kiln/ needs its own copy):")
        for path, source in rows:
            print(f"    {path}  from {source.strip()}")
    missing = [n for n in NOTICE_FILES if not (repo / KILN / n).exists()]
    if missing:
        print(f"  licensing files kiln/ lacks: {', '.join(missing)}")
    licences = sorted({m["package"].get("license", "?") for m in manifests.values()})
    print(f"  crate licence: {', '.join(licences)}")
    return 0


def main(argv: list[str]) -> int:
    if len(argv) == 3 and argv[0] == "prepare":
        return prepare(Path(argv[1]), Path(argv[2]))
    if len(argv) == 3 and argv[0] == "lock":
        return check_lock(Path(argv[1]), Path(argv[2]))
    if len(argv) == 2 and argv[0] == "report":
        return report(Path(argv[1]))
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
