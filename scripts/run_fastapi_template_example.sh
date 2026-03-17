#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EXAMPLE_PARENT="${PYTHON_DOCTOR_EXAMPLE_PARENT:-$ROOT_DIR/.examples}"
EXAMPLE_DIR="${PYTHON_DOCTOR_EXAMPLE_DIR:-$EXAMPLE_PARENT/full-stack-fastapi-template}"
REPO_URL="${PYTHON_DOCTOR_EXAMPLE_REPO:-https://github.com/fastapi/full-stack-fastapi-template.git}"

mkdir -p "$EXAMPLE_PARENT"

if [[ ! -d "$EXAMPLE_DIR/.git" ]]; then
  git clone --depth 1 "$REPO_URL" "$EXAMPLE_DIR"
else
  git -C "$EXAMPLE_DIR" pull --ff-only
fi

cd "$EXAMPLE_DIR"
if [[ -f .env ]]; then
  set -a
  # shellcheck disable=SC1091
  source .env
  set +a
fi

uv run python "$ROOT_DIR/scripts/python_doctor.py" \
  --repo-root "$EXAMPLE_DIR" \
  --skip-ruff \
  --skip-pyright \
  "$@"
