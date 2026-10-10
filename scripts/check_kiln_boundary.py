#!/usr/bin/env python3
"""Checks that kiln/ (docs/decisions/app-framework.md) could leave this
repository as it is: nothing in it names, includes or depends on fastedit,
only its platform directories branch on the operating system, and every
file and crate carries the same licence.

    check_kiln_boundary.py [ROOT]          # the checks; ROOT defaults to the repo
    check_kiln_boundary.py --self-test     # the checks against fixtures

The dependency check runs `cargo tree`, so only the full run makes it; the
self-test covers the file checks.
"""

import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

KILN = "kiln"
# The only places that may branch on the operating system.
OS_DIRS = ("kiln/crates/kiln-core/src/platform", "kiln/crates/kiln-platform/src")
SPDX = re.compile(r"^// SPDX-License-Identifier: \S+")
OS_CFG = re.compile(r"cfg!?\([^)]*target_os")
INCLUDE = re.compile(r'(?:#\[path\s*=\s*|include_(?:str|bytes)!\(\s*)"([^"]+)"')
FASTEDIT = re.compile(r"fastedit|\bfe[-_][a-z]", re.IGNORECASE)
LICENSE = re.compile(r'^license\s*=\s*"([^"]+)"', re.MULTILINE)


def rust_files(root: Path):
    return sorted((root / KILN).rglob("*.rs"))


def check_files(root: Path) -> list[str]:
    """Every file check: names, includes, OS branches, licence headers."""
    errors = []
    for path in rust_files(root):
        rel = path.relative_to(root).as_posix()
        text = path.read_text(encoding="utf-8")
        if not SPDX.match(text):
            errors.append(f"{rel}: doesn't start with an SPDX-License-Identifier line")
        for number, line in enumerate(text.splitlines(), 1):
            if FASTEDIT.search(line):
                errors.append(f"{rel}:{number}: names fastedit: {line.strip()}")
            if OS_CFG.search(line) and not rel.startswith(OS_DIRS):
                errors.append(f"{rel}:{number}: branches on the OS outside {', '.join(OS_DIRS)}")
            for target in INCLUDE.findall(line):
                resolved = (path.parent / target).resolve()
                if not resolved.is_relative_to((root / KILN).resolve()):
                    errors.append(f"{rel}:{number}: includes {target}, outside {KILN}/")
    for manifest in sorted((root / KILN).rglob("Cargo.toml")):
        rel = manifest.relative_to(root).as_posix()
        text = manifest.read_text(encoding="utf-8")
        if FASTEDIT.search(text):
            errors.append(f"{rel}: names fastedit")
    return errors


def check_licences(root: Path) -> list[str]:
    """Each kiln crate sets its licence itself, and they all agree."""
    errors, seen = [], {}
    for manifest in sorted((root / KILN).rglob("Cargo.toml")):
        text = manifest.read_text(encoding="utf-8")
        if "[package]" not in text:
            continue
        rel = manifest.relative_to(root).as_posix()
        found = LICENSE.search(text)
        if not found:
            errors.append(f"{rel}: sets no `license = \"…\"` of its own")
        else:
            seen[rel] = found.group(1)
    if len(set(seen.values())) > 1:
        errors.append(f"kiln crates disagree on their licence: {seen}")
    return errors


def check_dependencies(root: Path) -> list[str]:
    """No kiln crate reaches a workspace crate outside kiln/, in any build."""
    metadata = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"],
            cwd=root, check=True, capture_output=True, text=True,
        ).stdout
    )
    kiln_dir = (root / KILN).resolve()
    inside, outside = [], []
    for package in metadata["packages"]:
        manifest = Path(package["manifest_path"]).resolve()
        (inside if manifest.is_relative_to(kiln_dir) else outside).append(package["name"])
    outside = set(outside) - set(inside)
    errors = []
    for crate in inside:
        tree = subprocess.run(
            ["cargo", "tree", "-p", crate, "-e", "normal,build,dev", "--target", "all",
             "--prefix", "none", "--format", "{p}"],
            cwd=root, check=True, capture_output=True, text=True,
        ).stdout
        reached = {line.split(" ")[0] for line in tree.splitlines() if line}
        for name in sorted(reached & outside):
            errors.append(f"{crate} depends on {name}, which isn't in {KILN}/")
    return errors


def self_test() -> int:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        src = root / KILN / "crates" / "kiln-x" / "src"
        src.mkdir(parents=True)
        header = "// SPDX-License-Identifier: LicenseRef-Proprietary\n"
        manifest = root / KILN / "crates" / "kiln-x" / "Cargo.toml"
        manifest.write_text('[package]\nname = "kiln-x"\nlicense = "MIT"\n')
        good = src / "lib.rs"
        good.write_text(header + "pub fn f() {}\n")
        assert check_files(root) == [] and check_licences(root) == [], "a clean tree passes"

        cases = {
            "no header": ("pub fn f() {}\n", "SPDX"),
            "names it": (header + "// see fastedit\n", "names fastedit"),
            "names a crate": (header + "use fe_gpui::x;\n", "names fastedit"),
            "branches on the OS": (header + "#[cfg(target_os = \"linux\")]\nfn f() {}\n", "branches on the OS"),
            "includes outside": (header + 'const X: &str = include_str!("../../../../crates/a.txt");\n', "outside kiln/"),
            "path outside": (header + '#[path = "../../../../crates/m.rs"]\nmod m;\n', "outside kiln/"),
        }
        failed = 0
        for name, (text, expected) in cases.items():
            good.write_text(text)
            errors = check_files(root)
            if not any(expected in error for error in errors):
                print(f"self-test FAILED: {name}: no error with {expected!r} in {errors}")
                failed = 1
        good.write_text(header)

        platform = root / OS_DIRS[1]
        platform.mkdir(parents=True)
        (platform / "lib.rs").write_text(header + "#[cfg(target_os = \"macos\")]\nfn f() {}\n")
        if check_files(root):
            print("self-test FAILED: the platform directory may branch on the OS")
            failed = 1

        manifest.write_text('[package]\nname = "kiln-x"\nlicense.workspace = true\n')
        if not any("sets no" in error for error in check_licences(root)):
            print("self-test FAILED: an inherited licence isn't caught")
            failed = 1
        other = root / KILN / "crates" / "kiln-y" / "Cargo.toml"
        other.parent.mkdir(parents=True)
        other.write_text('[package]\nname = "kiln-y"\nlicense = "Apache-2.0"\n')
        manifest.write_text('[package]\nname = "kiln-x"\nlicense = "MIT"\n')
        if not any("disagree" in error for error in check_licences(root)):
            print("self-test FAILED: differing licences aren't caught")
            failed = 1
    print("self-test ok" if not failed else "self-test failed")
    return failed


def main() -> int:
    if sys.argv[1:] == ["--self-test"]:
        return self_test()
    root = Path(sys.argv[1] if len(sys.argv) > 1 else Path(__file__).resolve().parent.parent)
    errors = check_files(root) + check_licences(root) + check_dependencies(root)
    for error in errors:
        print(f"error: {error}", file=sys.stderr)
    if not errors:
        print(f"ok: {KILN}/ stands alone")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
