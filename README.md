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

### Prebuilt Wheels
Install from the package index hosted on GitHub Pages:

```bash
uv tool install --index https://s-smits.github.io/fastapi-doctor/simple/ fastapi-doctor
```

`uv` will pick the matching Linux, macOS, or Windows wheel automatically from the index, and fall back to the sdist only if no compatible wheel exists. This is the right path for CI as well:

```bash
uvx --index https://s-smits.github.io/fastapi-doctor/simple/ fastapi-doctor --version
```

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

## GitHub Release Artifacts
Tagged releases publish wheel files and an sdist to [GitHub Releases](https://github.com/s-smits/fastapi-doctor/releases), and the release workflow updates a PEP 503 simple index on GitHub Pages at [s-smits.github.io/fastapi-doctor/simple/](https://s-smits.github.io/fastapi-doctor/simple/).

Each release uploads:
- Linux wheel
- Windows wheel
- macOS Intel wheel
- macOS Apple Silicon wheel
- Source distribution

The native extension is built with `abi3` for Python `3.12+`, so each platform only needs one wheel per architecture instead of one wheel per Python minor version.

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
`0.4.1` trims more Python startup overhead from the static-only path while keeping the native project bundle responsible for discovery, config loading, route extraction, route-policy checks, and suppression collection.

Measured with:

```bash
uv run fastapi-doctor --static-only --profile strict --skip-ruff --skip-ty --repo-root /path/to/project
```

On the maintainer repo, CLI import dropped from `0.1865s` with the legacy Python runner to `0.0419s` with the current path, and a strict static-only self-scan dropped from `0.6663s` cold / `0.6401s` warm to `0.2362s` cold / `0.2276s` warm. On a representative external backend, the same strict static-only run held score parity while improving from `11.6632s` cold / `11.0076s` warm to `1.1090s` cold / `1.1517s` warm.

## Native Runtime
Runtime selection order:
1. Native PyO3 extension: `fastapi_doctor._fastapi_doctor_native`
2. Pure-Python fallback

## Internal Layout
- `src/fastapi_doctor/`: CLI, report assembly, live route checks, Python fallback logic.
- `rust/`: Rust workspace for the static engine, project model, rules, and native extension.
- `.github/workflows/`: wheel build and GitHub Release publishing.
- `tests/`: unit and integration tests.

## Development
```bash
# Python tests
uv run pytest -q

# Rust tests
PYO3_PYTHON=.venv/bin/python cargo test --manifest-path rust/Cargo.toml
```

### Local Native Development
```bash
# Reinstall the editable package, including native changes
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 uv sync --extra dev --reinstall-package fastapi-doctor

# Build local wheel and sdist artifacts through the standard frontend
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 uv build
```

## Release Flow
Push a tag like `v0.4.1` and GitHub Actions will:
- Validate that the tag matches `rust/Cargo.toml`
- Build platform wheels
- Build an sdist
- Attach all artifacts to a GitHub Release
- Publish/update the simple package index on GitHub Pages
