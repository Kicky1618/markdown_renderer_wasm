#!/usr/bin/env python3
import argparse
import json
import re
import statistics
from pathlib import Path

RATE_WITH_APPEND_RE = re.compile(
    r"^([^:]+):.*\([0-9]+(?:\.[0-9]+)? appends/s, ([0-9]+(?:\.[0-9]+)?) MiB/s\)$"
)
RATE_RE = re.compile(r"^([^:]+):.*\(([0-9]+(?:\.[0-9]+)?) MiB/s\)$")


def slug_label(label: str) -> str:
    return "-".join(label.strip().lower().replace("/", " ").split())


def parse(path: Path):
    values: dict[str, list[float]] = {}
    for raw in path.read_text().splitlines():
        line = raw.strip()
        match = RATE_WITH_APPEND_RE.match(line) or RATE_RE.match(line)
        if not match:
            continue
        label, value = match.group(1), float(match.group(2))
        values.setdefault(label, []).append(value)
    if not values:
        raise SystemExit(f"no benchmark rates found in {path}")
    return values


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("input", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--summary", type=Path)
    args = parser.parse_args()

    values = parse(args.input)
    bmf = {}
    rows = []
    for label, samples in sorted(values.items()):
        median = statistics.median(samples)
        low = min(samples)
        high = max(samples)
        benchmark = slug_label(label)
        bmf[benchmark] = {
            "mib-per-second": {
                "value": median,
                "lower_value": low,
                "upper_value": high,
            }
        }
        rows.append((label, median, low, high, len(samples)))

    args.output.write_text(json.dumps(bmf, indent=2, sort_keys=True) + "\n")

    if args.summary:
        lines = [
            "## Local benchmark samples",
            "",
            "Bencher receives one metric per benchmark: median as value, min/max as bounds.",
            "",
            "| Benchmark | Median MiB/s | Min | Max | Samples |",
            "|---|---:|---:|---:|---:|",
        ]
        for label, median, low, high, count in rows:
            lines.append(f"| {label} | {median:.1f} | {low:.1f} | {high:.1f} | {count} |")
        args.summary.write_text("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
