from __future__ import annotations

"""OpenAPI export helper."""

import argparse
import json
from pathlib import Path

from .app_loader import build_app_for_doctor


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Export OpenAPI schema without starting the server.")
    parser.add_argument("--output", default="tmp/fastapi-doctor-openapi.json", help="Output path relative to repo root.")
    parser.add_argument("--stdout", action="store_true", help="Print schema to stdout instead of writing a file.")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    schema = build_app_for_doctor().openapi()
    rendered = json.dumps(schema, indent=2, sort_keys=True)
    if args.stdout:
        print(rendered)
        return 0

    repo_root = Path.cwd()
    output_path = repo_root / args.output
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(rendered + "\n", encoding="utf-8")
    print(f"Wrote OpenAPI schema to {output_path}")
    return 0


__all__ = ["main", "parse_args"]
