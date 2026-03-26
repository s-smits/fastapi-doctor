# Contributing

## Setup

```bash
uv sync --extra dev
```

## Validation

```bash
uv run pytest -q
uv run python -m py_compile $(find src scripts tests -name '*.py')
```

## Rust Development

The native sidecar lives under `rust/doctor_core/Cargo.toml`.

```bash
# Run Rust tests
cargo test --manifest-path rust/doctor_core/Cargo.toml

# Build release binary
cargo build --release --manifest-path rust/doctor_core/Cargo.toml
```

### Staging Native Binaries
To test the native engine locally within the Python package:

```bash
# Build the binary
cargo build --release --manifest-path rust/doctor_core/Cargo.toml

# Stage into the Python package (ensure version matches Cargo.toml)
python scripts/stage_native_binary.py \
  --platform-tag darwin-arm64 \
  --version "0.2.0"

# Build a test wheel
uv build --wheel
```

## Local Release Dry Run
1. Run `cargo test --manifest-path rust/doctor_core/Cargo.toml`.
2. Run `uv run pytest -q`.
3. Stage the native binary with `scripts/stage_native_binary.py`.
4. Build and install the wheel in a clean environment to verify bundled binary discovery.

## Releases
Versions are derived from Git tags via `hatch-vcs`. Tagging `v*` triggers the CI to build:
- Pure-Python source distribution (sdist).
- macOS and Linux wheels with bundled native binaries.

The release workflow verifies binary presence and version matching before optional PyPI publication.

## Design Notes

- Keep the agent-facing entry points setup-agnostic. Avoid assuming a package is called `app` or that code lives only under `src/`.
- Add new checks to the narrowest category module under `src/fastapi_doctor/checks/`.
- Keep public entry points documented via `uv run fastapi-doctor`, not ad hoc wrappers.
- When adding heuristics, prefer false-negative bias over noisy false positives.
