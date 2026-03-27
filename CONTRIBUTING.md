# Contributing

## Setup

```bash
uv sync --extra dev
```

## Validation

```bash
uv run pytest -q
uv run python -m py_compile $(find src scripts tests -name '*.py')
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 cargo test --manifest-path rust/doctor_core/Cargo.toml
```

## Native Extension Development

The Rust extension lives under `rust/doctor_core/`.

```bash
# Reinstall the editable package, including Rust changes
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 uv sync --extra dev --reinstall-package fastapi-doctor

# Build release artifacts locally through the standard PEP 517 frontend
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 uv build
```

## Releases

The release version is sourced from `rust/doctor_core/Cargo.toml`.

To cut a release:
1. Bump the version in `rust/doctor_core/Cargo.toml`.
2. Run validation locally.
3. Push a matching tag such as `v0.3.0`.

The GitHub Actions release workflow will:
- Validate that the git tag matches the package version.
- Build wheels for Linux, Windows, macOS Intel, and macOS Apple Silicon.
- Build a source distribution.
- Attach all artifacts to the corresponding GitHub Release.
- Publish a GitHub Pages simple index so `uv` and `pip` can auto-select the right wheel.

## Design Notes

- Keep agent-facing entry points setup-agnostic.
- Add new checks to the narrowest category module under `src/fastapi_doctor/checks/`.
- Prefer deterministic AST-based checks over regex-heavy heuristics.
- When adding heuristics, bias toward false negatives over noisy false positives.
