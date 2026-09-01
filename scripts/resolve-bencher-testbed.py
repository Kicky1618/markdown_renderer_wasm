#!/usr/bin/env python3
"""Build a stable Bencher testbed slug from the runner hardware.

GitHub-hosted `ubuntu-latest` jobs can land on different CPU models. Keeping
those machines in one Bencher testbed makes hardware changes look like code
regressions, so we shard history by CPU model, architecture and visible vCPU
count.
"""

from __future__ import annotations

import os
import platform
import re
from pathlib import Path


def cpu_model() -> str:
    cpuinfo = Path("/proc/cpuinfo")
    if cpuinfo.exists():
        for line in cpuinfo.read_text(errors="replace").splitlines():
            if line.lower().startswith("model name") and ":" in line:
                return line.split(":", 1)[1].strip()
    return platform.processor() or "unknown-cpu"


def compact_cpu_model(model: str) -> str:
    # Keep the vendor/family/model information but drop marketing boilerplate
    # that is identical across all runners and only makes the slug longer.
    model = re.sub(r"\(R\)|\(TM\)", "", model, flags=re.IGNORECASE)
    model = re.sub(r"\b(?:processor|cpu)\b", "", model, flags=re.IGNORECASE)
    model = re.sub(r"\b\d+-core\b", "", model, flags=re.IGNORECASE)
    return " ".join(model.split())


def slug(text: str) -> str:
    value = re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")
    return re.sub(r"-+", "-", value)


def main() -> None:
    image = os.environ.get("ImageOS") or os.environ.get("RUNNER_OS") or platform.system()
    arch = os.environ.get("RUNNER_ARCH") or platform.machine()
    cpus = os.cpu_count() or 1
    model = compact_cpu_model(cpu_model())
    testbed = slug(f"gha {image} {arch} {cpus}vcpu {model}")
    # Bencher slugs should stay human-readable and comfortably below common
    # identifier limits. CPU family/model appears near the front in practice.
    print(testbed[:120].rstrip("-"))


if __name__ == "__main__":
    main()
