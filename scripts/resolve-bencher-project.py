#!/usr/bin/env python3
import json
import os
import sys
import urllib.request

API = "https://api.bencher.dev/v0/projects?per_page=255"


def norm(value: str) -> str:
    return "".join(ch for ch in value.lower() if ch.isalnum())


def main() -> None:
    key = os.environ.get("BENCHER_API_KEY", "")
    if not key:
        raise SystemExit("BENCHER_API_KEY is empty")

    request = urllib.request.Request(
        API,
        headers={
            "Accept": "application/json",
            "Authorization": f"Bearer {key}",
            "User-Agent": "markdown-renderer-wasm-github-actions",
        },
    )
    with urllib.request.urlopen(request, timeout=20) as response:
        projects = json.load(response)

    if not isinstance(projects, list) or not projects:
        raise SystemExit("BENCHER_API_KEY cannot see any Bencher project")

    # A project-scoped bencher_run_* key makes this endpoint return only its
    # own project. This is the preferred and documented GitHub Actions setup.
    if len(projects) == 1:
        print(projects[0]["slug"])
        return

    repository = os.environ.get("GITHUB_REPOSITORY", "").split("/")[-1]
    wanted = norm(repository)
    matches = []
    for project in projects:
        haystack = " ".join(
            str(project.get(field, "")) for field in ("name", "slug", "url")
        )
        if wanted and wanted in norm(haystack):
            matches.append(project)

    if len(matches) == 1:
        print(matches[0]["slug"])
        return

    slugs = ", ".join(str(project.get("slug", "?")) for project in projects[:12])
    print(
        "Unable to identify the Bencher project uniquely. "
        "Use a project-scoped bencher_run_* key for this repository. "
        f"Visible projects: {slugs}",
        file=sys.stderr,
    )
    raise SystemExit(2)


if __name__ == "__main__":
    main()
