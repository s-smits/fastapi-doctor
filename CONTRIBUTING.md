# Contributing

## Setup

```bash
uv sync --extra dev
```

## Validation

```bash
uv run pytest -q
PYO3_PYTHON="$PWD/.venv/bin/python" cargo test --manifest-path rust/Cargo.toml
```

## Native Extension Development

The Rust extension lives under `rust/fastapi_doctor_native/`.

```bash
uv sync --extra dev --reinstall-package fastapi-doctor
uv build
```

## Releases

The release version is sourced from the package metadata and Rust workspace versioning.

To cut a release:
1. Update the version in the Rust workspace/package metadata.
2. Run validation locally.
3. Push a matching tag such as `v0.7.1`.

The GitHub Actions release workflow will:
- Build wheels for Linux, Windows, macOS Intel, and macOS Apple Silicon.
- Build a source distribution.
- Attach all artifacts to the corresponding GitHub Release.
- Publish a GitHub Pages simple index so `uv` and `pip` can auto-select the right wheel.

## Design Notes

- Keep the Python surface minimal.
- Put analyzer logic in Rust under `rust/fastapi_doctor_core/`, `rust/fastapi_doctor_project/`, and `rust/fastapi_doctor_rules/`.
- Prefer deterministic static analysis over regex-heavy heuristics.
