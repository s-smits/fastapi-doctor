#!/usr/bin/env python3
from __future__ import annotations

import argparse
import html
import json
import re
from pathlib import Path

DIST_EXTENSIONS = (".whl", ".tar.gz")


def canonicalize_name(name: str) -> str:
    return re.sub(r"[-_.]+", "-", name).lower()


def dist_prefixes(project_name: str) -> tuple[str, ...]:
    normalized = canonicalize_name(project_name)
    tokens = normalized.split("-")
    return tuple(f"{separator.join(tokens)}-" for separator in ("-", "_", "."))


def is_distribution_asset(filename: str, project_name: str) -> bool:
    lowered = filename.lower()
    if not lowered.endswith(DIST_EXTENSIONS):
        return False
    return any(lowered.startswith(prefix) for prefix in dist_prefixes(project_name))


def flatten_release_pages(raw_releases: object) -> list[dict[str, object]]:
    if isinstance(raw_releases, list) and raw_releases and isinstance(raw_releases[0], list):
        flattened: list[dict[str, object]] = []
        for page in raw_releases:
            if isinstance(page, list):
                flattened.extend(item for item in page if isinstance(item, dict))
        return flattened
    if isinstance(raw_releases, list):
        return [item for item in raw_releases if isinstance(item, dict)]
    raise ValueError("release JSON must be a list of releases or a paginated list of release lists")


def collect_release_assets(releases: list[dict[str, object]], project_name: str) -> list[dict[str, str]]:
    assets: list[dict[str, str]] = []
    for release in releases:
        if release.get("draft"):
            continue
        tag_name = str(release.get("tag_name", "")).strip()
        release_assets: list[dict[str, str]] = []
        for asset in release.get("assets", []):
            if not isinstance(asset, dict):
                continue
            name = str(asset.get("name", "")).strip()
            url = str(asset.get("browser_download_url", "")).strip()
            if not name or not url or not is_distribution_asset(name, project_name):
                continue
            release_assets.append(
                {
                    "name": name,
                    "url": url,
                    "tag": tag_name,
                }
            )
        assets.extend(sorted(release_assets, key=lambda item: item["name"], reverse=True))
    return assets


def collect_dist_assets(dist_dir: Path, project_name: str, repository: str, tag: str) -> list[dict[str, str]]:
    base_url = f"https://github.com/{repository}/releases/download/{tag}"
    assets: list[dict[str, str]] = []
    for path in sorted(dist_dir.iterdir()):
        if not path.is_file() or not is_distribution_asset(path.name, project_name):
            continue
        assets.append(
            {
                "name": path.name,
                "url": f"{base_url}/{path.name}",
                "tag": tag,
            }
        )
    assets.sort(key=lambda item: item["name"], reverse=True)
    return assets


def render_root_index(project_name: str) -> str:
    normalized = canonicalize_name(project_name)
    return (
        "<!DOCTYPE html>\n"
        "<html>\n"
        "  <body>\n"
        f'    <a href="{html.escape(normalized)}/">{html.escape(project_name)}</a>\n'
        "  </body>\n"
        "</html>\n"
    )


def render_project_index(project_name: str, assets: list[dict[str, str]]) -> str:
    links = [
        f'    <a href="{html.escape(asset["url"], quote=True)}">{html.escape(asset["name"])}</a>'
        for asset in assets
    ]
    return (
        "<!DOCTYPE html>\n"
        "<html>\n"
        "  <body>\n"
        + ("\n".join(links) if links else "    <!-- No distributions found -->")
        + "\n  </body>\n"
        "</html>\n"
    )


def render_landing_page(project_name: str, repository: str) -> str:
    owner, repo = repository.split("/", 1)
    index_url = f"https://{owner}.github.io/{repo}/simple/"
    return f"""<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <title>{html.escape(project_name)} package index</title>
  </head>
  <body>
    <h1>{html.escape(project_name)}</h1>
    <p>This site exposes a PEP 503 simple package index for prebuilt release artifacts.</p>
    <p>uv and pip will automatically select the matching wheel for the current platform and Python ABI tags.</p>
    <pre>uv tool install --index {html.escape(index_url)} {html.escape(project_name)}</pre>
    <pre>python -m pip install --extra-index-url {html.escape(index_url)} {html.escape(project_name)}</pre>
    <p><a href="simple/">Open the simple index</a></p>
  </body>
</html>
"""


def write_index(output_dir: Path, project_name: str, repository: str, assets: list[dict[str, str]]) -> None:
    simple_dir = output_dir / "simple"
    project_dir = simple_dir / canonicalize_name(project_name)
    project_dir.mkdir(parents=True, exist_ok=True)

    (output_dir / ".nojekyll").write_text("", encoding="utf-8")
    (output_dir / "index.html").write_text(render_landing_page(project_name, repository), encoding="utf-8")
    (simple_dir / "index.html").write_text(render_root_index(project_name), encoding="utf-8")
    (project_dir / "index.html").write_text(render_project_index(project_name, assets), encoding="utf-8")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build a PEP 503 simple index for GitHub release artifacts.")
    parser.add_argument("--project-name", default="fastapi-doctor")
    parser.add_argument("--repository", required=True, help="GitHub repository in OWNER/REPO form.")
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--releases-json", type=Path, help="JSON from `gh api --paginate --slurp repos/<repo>/releases`.")
    parser.add_argument("--dist-dir", type=Path, help="Local dist directory for generating a current-release-only index.")
    parser.add_argument("--tag", help="Release tag to use with --dist-dir, for example v0.3.0.")
    return parser.parse_args()


def main() -> None:
    args = parse_args()

    if args.releases_json:
        raw_releases = json.loads(args.releases_json.read_text(encoding="utf-8"))
        releases = flatten_release_pages(raw_releases)
        assets = collect_release_assets(releases, args.project_name)
    elif args.dist_dir and args.tag:
        assets = collect_dist_assets(args.dist_dir, args.project_name, args.repository, args.tag)
    else:
        raise SystemExit("pass either --releases-json or both --dist-dir and --tag")

    if not assets:
        raise SystemExit("no matching wheel or sdist assets found")

    write_index(args.output_dir, args.project_name, args.repository, assets)


if __name__ == "__main__":
    main()
