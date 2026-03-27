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
# Install the extension into the active environment
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 uvx maturin develop --release

# Build wheel artifacts locally
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 uv build --wheel

# Build an sdist locally
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 uv build --sdist
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

## Design Notes

- Keep agent-facing entry points setup-agnostic.
- Add new checks to the narrowest category module under `src/fastapi_doctor/checks/`.
- Prefer deterministic AST-based checks over regex-heavy heuristics.
- When adding heuristics, bias toward false negatives over noisy false positives.
