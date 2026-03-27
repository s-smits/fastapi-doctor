# fastapi-doctor

`fastapi-doctor` is an agent-first backend verifier for FastAPI and Python services. It enforces route contracts, async safety, security boundaries, and structural correctness with deterministic static analysis.

## Why This Exists
LLM agents are good at local edits and weak at repo-wide invariants. `fastapi-doctor` closes that gap by:
- Using AST-based checks instead of style heuristics or prompt memory.
- Scoring backend health from `0-100`.
- Emitting stable JSON for automated remediation loops.
- Catching security and correctness issues that are easy to miss in review.

## Agent Quick Start
1. Ask the user for a `security`, `medium`, or `strict` audit profile.
2. Run `uv run fastapi-doctor --json --profile <profile>`.
3. If the score is below `80`, fix findings with the smallest semantic diff.

> [!IMPORTANT]
> Ask before copying these rules into `CLAUDE.md`, `AGENTS.md`, or similar custom agent instructions.

## Key Features
- Auto-discovers `repo-root`, `src/<pkg>`, and `backend/<pkg>` layouts.
- Uses a Rust-powered static engine with Python fallback.
- Runs route/OpenAPI checks, architecture checks, security checks, performance checks, and Pydantic checks.
- Supports machine-readable JSON for agent workflows.

## Installation

### From Source
```bash
git clone https://github.com/s-smits/fastapi-doctor.git
cd fastapi-doctor
uv sync --extra dev
```

Run it from the checked-out repo with:
```bash
uv run fastapi-doctor --profile strict --repo-root /path/to/your/project
```

### From GitHub Release Assets
If you want a prebuilt wheel instead of building from source, install from a GitHub Release artifact:

```bash
uv tool install --from "https://github.com/s-smits/fastapi-doctor/releases/download/v0.3.0/<wheel-file-name>.whl" fastapi-doctor
```

## GitHub Release Artifacts
Tagged releases publish wheel files and an sdist to [GitHub Releases](https://github.com/s-smits/fastapi-doctor/releases).

Each release uploads:
- Linux wheel
- Windows wheel
- macOS Intel wheel
- macOS Apple Silicon wheel
- Source distribution

The native extension is built with `abi3` for Python `3.12+`, so each platform only needs one wheel per architecture instead of one wheel per Python minor version.

To install from a downloaded GitHub Release asset:
```bash
uv tool install --from /path/to/fastapi_doctor-0.3.0-*.whl fastapi-doctor
```

## Common Invocations
```bash
# Standard machine-readable report
uv run fastapi-doctor --json

# Fast static-only scan
uv run fastapi-doctor --json --static-only

# Comprehensive audit with external tools
uv run fastapi-doctor --json --with-bandit --with-tests

# Explicit app entrypoint
uv run fastapi-doctor --app-module my_pkg.main:app
```

## Audit Profiles
| Profile | Focus |
| :--- | :--- |
| `security` | Auth dependencies, CORS, secrets, env access, error leaks. |
| `medium` | Security plus correctness, resilience, and async safety. |
| `strict` | All checks, including opinionated architecture and performance rules. |

## Performance
`0.3.0` moves the static-only path to a native project scan that performs file discovery, source loading, static issue analysis, route extraction, and suppression collection in Rust.

Measured on `toto-scope` with:

```bash
uv run fastapi-doctor --static-only --profile strict --skip-ruff --skip-ty --repo-root /Users/air/Developer/toto-scope
```

| Engine | Strict Static Scan | vs Legacy |
| :--- | :--- | :--- |
| Legacy Python | ~28.0s | 1x |
| Rust subprocess (~0.1.x) | ~11.7s | ~2.4x |
| PyO3 extension (0.2.x) | ~5.9s | ~4.8x |
| Native project bundle (0.3.0) | ~2.1s to ~2.4s | ~11x to ~13x |

## Native Runtime
Runtime selection order:
1. Native PyO3 extension: `fastapi_doctor._fastapi_doctor_native`
2. Pure-Python fallback

## Internal Layout
- `src/fastapi_doctor/`: CLI, report assembly, live route checks, Python fallback logic.
- `rust/doctor_core/`: Rust static engine and native extension.
- `.github/workflows/`: wheel build and GitHub Release publishing.
- `tests/`: unit and integration tests.

## Development
```bash
# Python tests
uv run pytest -q

# Rust tests
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 cargo test --manifest-path rust/doctor_core/Cargo.toml
```

### Local Native Development
```bash
# Build and install the extension into the active environment
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 uvx maturin develop --release

# Build local wheel artifacts
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 uv build --wheel

# Build an sdist
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 uv build --sdist
```

## Release Flow
Push a tag like `v0.3.0` and GitHub Actions will:
- Validate that the tag matches `rust/doctor_core/Cargo.toml`
- Build platform wheels
- Build an sdist
- Attach all artifacts to a GitHub Release
