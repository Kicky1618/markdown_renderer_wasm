#!/usr/bin/env python3
import argparse
import re
import statistics
import sys
from pathlib import Path

RATE_WITH_APPEND_RE = re.compile(
    r"^([^:]+):.*\([0-9]+(?:\.[0-9]+)? appends/s, ([0-9]+(?:\.[0-9]+)?) MiB/s\)$"
)
RATE_RE = re.compile(r"^([^:]+):.*\(([0-9]+(?:\.[0-9]+)?) MiB/s\)$")


def parse(path: Path) -> dict[str, list[float]]:
    values: dict[str, list[float]] = {}
    for raw in path.read_text().splitlines():
        line = raw.strip()
        match = RATE_WITH_APPEND_RE.match(line) or RATE_RE.match(line)
        if not match:
            continue
        label, value = match.group(1), float(match.group(2))
        values.setdefault(label, []).append(value)
    if not values:
        raise ValueError(f"no benchmark rates found in {path}")
    return values


def relative_mad(samples: list[float]) -> float:
    median = statistics.median(samples)
    if median == 0:
        return 0.0 if all(value == 0 for value in samples) else float("inf")
    mad = statistics.median(abs(value - median) for value in samples)
    return mad / abs(median)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Return success when benchmark medians are stable enough to stop sampling early."
    )
    parser.add_argument("logs", nargs="+", type=Path)
    parser.add_argument("--min-samples", type=int, default=3)
    parser.add_argument("--relative-mad", type=float, default=0.015)
    args = parser.parse_args()

    if args.min_samples < 1:
        parser.error("--min-samples must be positive")
    if args.relative_mad < 0:
        parser.error("--relative-mad must be non-negative")

    stable = True
    try:
        for path in args.logs:
            values = parse(path)
            for label, samples in sorted(values.items()):
                if len(samples) < args.min_samples:
                    print(
                        f"unstable {path.name}: {label}: {len(samples)}/{args.min_samples} samples",
                        file=sys.stderr,
                    )
                    stable = False
                    continue
                score = relative_mad(samples)
                print(
                    f"stability {path.name}: {label}: samples={len(samples)} relative_mad={score:.4%}"
                )
                if score > args.relative_mad:
                    stable = False
    except (OSError, ValueError) as error:
        print(f"benchmark stability check failed: {error}", file=sys.stderr)
        return 2

    return 0 if stable else 1


if __name__ == "__main__":
    raise SystemExit(main())
