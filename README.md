# fastapi-doctor

Rust-native static analyzer for FastAPI and Python codebases. Scans your project for security issues, correctness bugs, architecture smells, and API surface problems — no runtime import of your app required.

## Install

Install as a standalone tool:

```bash
uv tool install --index https://s-smits.github.io/fastapi-doctor/simple/ fastapi-doctor
```

Or run it directly without installing:

```bash
uvx --index https://s-smits.github.io/fastapi-doctor/simple/ fastapi-doctor --version
```

## Usage

Point it at your project:

```bash
fastapi-doctor --repo-root /path/to/your/project
```

By default it scans the current directory. Common flags:

```bash
# Choose an audit profile: security, balanced (default), or strict
fastapi-doctor --profile strict

# Machine-readable JSON output
fastapi-doctor --json

# Score only (0–100)
fastapi-doctor --score

# Show all findings, not just one per rule
fastapi-doctor --verbose

# Fail in CI on errors or warnings
fastapi-doctor --fail-on error
fastapi-doctor --fail-on warning

# Filter rules
fastapi-doctor --only-rules "security/*"
fastapi-doctor --ignore-rules "architecture/giant-function"

# Run external tools alongside
fastapi-doctor --with-bandit --with-tests
```

If your source code lives in a subdirectory:

```bash
fastapi-doctor --repo-root . --code-dir src/myapp
```

## What It Checks

- **Security** — unsafe yaml/pickle loads, SQL injection patterns, hardcoded secrets, CORS misconfiguration
- **Correctness** — naive datetimes, mutable defaults, unvalidated path params
- **Architecture** — giant functions, god modules, deep nesting, sync-in-async, print in production
- **API surface** — missing pagination, missing OpenAPI tags, duplicate operation IDs
- **Pydantic** — deprecated validators, sensitive field types, extra-allow on request models
- **Resilience** — missing timeouts, bare exception handlers

## Configuration

Drop a `.fastapi-doctor.yml` in your project root to tune thresholds:

```yaml
architecture:
  giant_function: 400
  god_module: 1500
  deep_nesting: 5

security:
  forbidden_write_params: []

scan:
  exclude_dirs:
    - vendor
    - generated
  exclude_rules: []
```

See [`.fastapi-doctor.example.yml`](./.fastapi-doctor.example.yml) for the full schema.

## Development

```bash
git clone https://github.com/s-smits/fastapi-doctor.git
cd fastapi-doctor
uv sync --extra dev
```

Run tests:

```bash
uv run pytest -q
PYO3_PYTHON="$PWD/.venv/bin/python" cargo test --manifest-path rust/Cargo.toml
```

Build a wheel locally:

```bash
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 uv build
```

## Repository Layout

- `rust/` — Rust workspace: `fastapi_doctor_core`, `fastapi_doctor_project`, `fastapi_doctor_rules`, `fastapi_doctor_native` (PyO3 bridge)
- `src/fastapi_doctor/` — minimal Python bootstrap for packaging and CLI dispatch
- `tests/` — Python-side smoke tests for the native bridge
- `.github/workflows/` — CI, release wheels, and GitHub Pages package index
