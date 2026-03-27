# fastapi-doctor

[![Trivy Repository Scan](https://github.com/s-smits/fastapi-doctor/actions/workflows/trivy.yml/badge.svg)](https://github.com/s-smits/fastapi-doctor/actions/workflows/trivy.yml)

`fastapi-doctor` is an agent-first backend verifier for FastAPI and Python services. It enforces route contracts, async safety, security boundaries, and structural correctness with deterministic static analysis.

## Why This Exists
LLM agents are good at local edits and weak at repo-wide invariants. `fastapi-doctor` closes that gap by:
- Using AST-based checks instead of style heuristics or prompt memory.
- Scoring backend health from `0-100`.
- Emitting stable JSON for automated remediation loops.
- Catching security and correctness issues that are easy to miss in review.

## Agent Quick Start
1. Ask the user for a `security`, `balanced`, or `strict` audit profile.
2. Run `uv run fastapi-doctor --json --profile <profile>`.
3. If the score is below `80`, fix findings with the smallest semantic diff.

> [!IMPORTANT]
> Ask before copying these rules into `CLAUDE.md`, `AGENTS.md`, or similar custom agent instructions.

## Key Features
- Auto-discovers `repo-root`, `src/<pkg>`, and `backend/<pkg>` layouts.
- Handles common monorepo layouts such as `apps/<service>`, `services/<service>`, and `packages/<service>` when you pass `--app-module` or rely on static heuristics.
- Uses a Rust-powered static engine with Python fallback.
- Runs route/OpenAPI checks, architecture checks, security checks, performance checks, and Pydantic checks.
- Supports machine-readable JSON for agent workflows.
- Runs GitHub Dependency Review on pull requests and scheduled Trivy scans for dependency, secret, and misconfiguration checks.

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

If the target project still runs on Python `3.11` or older, keep `fastapi-doctor` in an isolated tool environment instead of forcing it into the app runtime:

```bash
uv tool install --python 3.12 --index https://s-smits.github.io/fastapi-doctor/simple/ fastapi-doctor
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

## Monorepo Layout Recipes
Use the service root as `--repo-root` when possible. If you must scan from a larger monorepo root, pass `--code-dir`, `--import-root`, and `--app-module` explicitly.

```text
repo/
  src/my_service/main.py
```

```bash
uv run fastapi-doctor --profile strict --repo-root . --app-module my_service.main:app
```

```text
repo/
  backend/service/app.py
```

```bash
uv run fastapi-doctor --profile strict --repo-root backend --app-module service.app:app
```

```text
repo/
  apps/
    service_api/
      __init__.py
      main.py
      routers/
```

```bash
uv run fastapi-doctor \
  --profile strict \
  --repo-root . \
  --code-dir apps/service_api \
  --import-root . \
  --app-module apps.service_api.main:app
```

For monorepos with unrelated Python under directories like `internal_tools/` or `generated/`, add them to [`scan.exclude_dirs`](./.fastapi-doctor.example.yml).

## Audit Profiles
| Profile | Focus |
| :--- | :--- |
| `security` | Auth dependencies, CORS, secrets, env access, error leaks. |
| `balanced` | Security plus correctness, resilience, and async safety. |
| `strict` | All checks, including opinionated architecture and performance rules. |

`medium` remains accepted as a legacy alias for `balanced`.

## Performance
`0.5.0` strips much more Python out of the strict static-only score path. Static score runs now use a native fast path for rule selection and scoring, avoid importing live FastAPI route/OpenAPI helpers, and use optimized dev builds for the Rust parser and analysis crates.

Measured with:

```bash
uv run fastapi-doctor --static-only --profile strict --skip-ruff --skip-ty --repo-root /path/to/project
```

Measured against the same command shape, `0.5.0` improves CLI import from `0.0522s` in `0.4.1` to `0.0262s`, and improves a strict static-only self-scan from `0.2451s` cold / `0.2229s` warm to `0.0449s` cold / `0.0438s` warm. On a representative external backend, the same strict static-only run keeps score parity while improving from `1.0879s` cold / `1.0758s` warm in `0.4.1` to `0.5652s` cold / `0.5422s` warm in `0.5.0`.

## Native Runtime
Runtime selection order:
1. Native PyO3 extension: `fastapi_doctor._fastapi_doctor_native`
2. Pure-Python fallback

## Internal Layout
- `src/fastapi_doctor/`: CLI, report assembly, live route checks, Python fallback logic.
- `rust/`: Rust workspace for the static engine, project model, rules, and native extension.
- `.github/workflows/`: release, dependency review, and Trivy security scanning.
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
Push a tag like `v0.5.0` and GitHub Actions will:
- Validate that the tag matches `rust/Cargo.toml`
- Build platform wheels
- Build an sdist
- Attach all artifacts to a GitHub Release
- Publish/update the simple package index on GitHub Pages
