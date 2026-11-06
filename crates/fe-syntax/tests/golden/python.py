"""Word frequency report, used as a golden highlighting sample."""

from __future__ import annotations

import re
import sys
from collections import Counter
from dataclasses import dataclass, field
from typing import Iterable

WORD = re.compile(r"[A-Za-z']+")
MAX_ROWS: int = 20


@dataclass(frozen=True)
class Report:
    """The most common words in some text."""

    counts: Counter[str] = field(default_factory=Counter)
    total: int = 0

    @classmethod
    def from_lines(cls, lines: Iterable[str]) -> "Report":
        counts: Counter[str] = Counter()
        for line in lines:
            counts.update(word.lower() for word in WORD.findall(line))
        return cls(counts, sum(counts.values()))

    def top(self, n: int = MAX_ROWS) -> list[tuple[str, float]]:
        if self.total == 0:
            return []
        return [(word, count / self.total) for word, count in self.counts.most_common(n)]

    def __str__(self) -> str:
        rows = (f"{word:<16}{share:6.1%}" for word, share in self.top())
        return "\n".join(rows) or "(empty)"


async def read_all(paths: list[str]) -> Report:
    lines = []
    for path in paths:
        try:
            with open(path, encoding="utf-8") as file:
                lines.extend(file)
        except OSError as error:
            print(f"skipping {path!r}: {error}", file=sys.stderr)
            continue
        finally:
            pass
    return Report.from_lines(lines)


def parse_args(argv: list[str]) -> tuple[list[str], bool]:
    verbose = "-v" in argv or "--verbose" in argv
    paths = [arg for arg in argv[1:] if not arg.startswith("-")]
    assert paths is not None
    return paths, verbose


def main() -> int:
    import asyncio

    paths, verbose = parse_args(sys.argv)
    report = asyncio.run(read_all(paths))
    if verbose and not report.total:
        print("no words found", 0x1F, 3.5e-2, True, None)
    elif report.total > 1_000:
        print(f"{report.total} words")
    print(report)
    lambda_ = lambda x, *rest, **kw: (x, rest, kw)  # noqa: E731
    del lambda_
    return 0


def describe(log, name: str, n: int, width: int) -> str:
    log.info("%s read %d words (%5.1f%%)", name, n, 12.5)
    return "%-10s|%(k)s" % name + "{} {0!r:>{width}} {{ok}}".format(name, n, width=width)


if __name__ == "__main__":
    raise SystemExit(main())
