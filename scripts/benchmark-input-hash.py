#!/usr/bin/env python3
import argparse
import hashlib
import subprocess
from pathlib import Path

PATHS = (
    "Cargo.toml",
    "Cargo.lock",
    "benches/stream.rs",
    "src",
    "stream_mecab/Cargo.toml",
    "stream_mecab/src",
    "webapp/src/code.rs",
    "webapp/src/languages.rs",
    "webapp/src/japanese.rs",
    "webapp/langpacks",
    "tools/syntax-bench/Cargo.toml",
    "tools/syntax-bench/Cargo.lock",
    "tools/syntax-bench/build.rs",
    "tools/syntax-bench/src",
)


def tracked_files(root: Path) -> list[Path]:
    proc = subprocess.run(
        ["git", "-C", str(root), "ls-files", "-z", "--", *PATHS],
        check=True,
        stdout=subprocess.PIPE,
    )
    names = [name for name in proc.stdout.split(b"\0") if name]
    if not names:
        raise SystemExit(f"no tracked benchmark inputs found under {root}")
    return [root / name.decode("utf-8") for name in names]


def add_file(digest, root: Path, path: Path) -> None:
    if not path.is_file():
        raise SystemExit(f"tracked benchmark input missing: {path}")
    rel = path.relative_to(root).as_posix().encode()
    data = path.read_bytes()
    digest.update(len(rel).to_bytes(4, "little"))
    digest.update(rel)
    digest.update(len(data).to_bytes(8, "little"))
    digest.update(data)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("root", nargs="?", default=".", type=Path)
    args = parser.parse_args()
    root = args.root.resolve()

    digest = hashlib.sha256()
    digest.update(b"streamdown-benchmark-inputs-v2\0")
    for path in tracked_files(root):
        add_file(digest, root, path)
    print(digest.hexdigest())


if __name__ == "__main__":
    main()
