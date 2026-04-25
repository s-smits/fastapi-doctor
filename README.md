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

By default it scans the current directory. Profiles are intentionally different:

- `security` — security and config boundary checks only
- `balanced` — high-confidence security, correctness, resilience, and API contract checks
- `strict` — everything in balanced plus broader architecture pressure rules

Common flags:

```bash
# Choose an audit profile: security, balanced (default), or strict
fastapi-doctor --profile strict

# Machine-readable JSON output
fastapi-doctor --json

# Doctor score only (0–100)
fastapi-doctor --score

# Show all findings, not just one per rule
fastapi-doctor --verbose

# Fail in CI on doctor findings
fastapi-doctor --fail-on error
fastapi-doctor --fail-on warning

# Also fail if requested external tools are unavailable or fail
fastapi-doctor --fail-on-tools configured

# Filter rules
fastapi-doctor --only-rules "security/*"
fastapi-doctor --ignore-rules "architecture/giant-function"

# Output formats
fastapi-doctor --output-format sarif   # SARIF 2.1.0 for code scanning
fastapi-doctor --output-format github  # GitHub Actions annotations

# List all available rules
fastapi-doctor --list-rules

# Run external tools alongside
fastapi-doctor --with-bandit --with-tests
```

If your source code lives in a subdirectory:

```bash
fastapi-doctor --repo-root . --code-dir src/myapp
```

When `--code-dir` is set, or when discovery resolves a project code root, Ruff, ty, and Bandit run against that target instead of scanning the entire repository. Structural analysis excludes common noise directories such as virtualenvs, caches, vendored code, generated code, and tests by default. Use `--include-tests` or config if you intentionally want tests included in native structural scoring.

## What It Checks

- **Security** — missing auth dependencies on protected routes, unsafe yaml/pickle loads, SQL injection patterns, hardcoded secrets, CORS misconfiguration
- **Correctness** — weak or missing response models, naive datetimes, mutable defaults, unvalidated path params
- **Serverless correctness** — local writes outside `/tmp`, with `/tmp` and temp-helper flows treated as safe
- **Architecture** — giant functions, god modules, deep nesting, sync-in-async, print in production
- **API surface** — missing pagination, missing route tags, missing endpoint docstrings
- **Pydantic** — deprecated validators, sensitive field types, extra-allow on request models, and snake_case/camelCase alias collisions
- **Resilience** — missing timeouts, bare exception handlers, exception logs without traceback
- **Config** — direct env reads in service/router code, process env mutation outside bootstrap

## Suppressing Findings

Suppress a single rule on one line:

```python
x = yaml.load(data)  # doctor:ignore security/unsafe-yaml-load reason="input is trusted"
```

The `# noqa` syntax also works:

```python
x = yaml.load(data)  # noqa: security/unsafe-yaml-load

# Suppress all rules in a category
x = yaml.load(data)  # noqa: security/*

# Suppress everything on this line (not recommended)
x = yaml.load(data)  # noqa
```

To exclude rules globally, use `--ignore-rules` or the config file's `scan.exclude_rules` list.
For intentional serverless-safe temp writes that still need a local suppression, prefer a narrow `doctor:ignore correctness/serverless-filesystem-write reason="..."`.

## Scoring

fastapi-doctor now reports:

- `doctor_score` — score from doctor findings only
- `composite_score` — optional convenience score that adds Bandit high-severity penalties when Bandit runs successfully
- `toolchain` — separate status for Ruff, ty, Bandit, and pytest (`passed`, `failed`, `not_found`, or `skipped`)

The doctor score is:

```
score = 100 - (unique_error_rules × 2) - (unique_warning_rules × 1)
```

Key details:
- **Unique rules** — multiple findings of the same rule count once
- **Errors** cost 2 points per unique rule, **warnings** cost 1
- Score floors at 0
- Missing optional tools do not reduce `doctor_score`
- Opinionated rules are use-case aware where possible: sequential-await warnings avoid ordered lifecycle/write side effects, and mutating route handlers get modest extra line-count headroom compared with read endpoints.

Labels:
- **Great** — score >= 80
- **Needs work** — score 60–79
- **Critical** — score < 60

## CI Integration

Add to your GitHub Actions workflow:

```yaml
- name: Run fastapi-doctor
  run: |
    uvx --index https://s-smits.github.io/fastapi-doctor/simple/ \
      fastapi-doctor --output-format github --fail-on error --skip-ruff --skip-ty
```

For GitHub Code Scanning with SARIF:

```yaml
- name: Run fastapi-doctor
  run: |
    uvx --index https://s-smits.github.io/fastapi-doctor/simple/ \
      fastapi-doctor --output-format sarif --skip-ruff --skip-ty > results.sarif

- name: Upload SARIF
  uses: github/codeql-action/upload-sarif@v3
  with:
    sarif_file: results.sarif
```

## Configuration

Drop a `.fastapi-doctor.yml` in your project root to tune thresholds:

```yaml
architecture:
  giant_function: 400
  god_module: 1500
  deep_nesting: 5

security:
  forbidden_write_params: []
  auth_required_prefixes: []
  auth_dependency_names: []
  auth_exempt_prefixes:
    - /api/auth

scan:
  exclude_dirs:
    - .venv
    - venv
    - site-packages
    - __pycache__
    - node_modules
    - vendor
    - generated
    - tests
  include_tests: false
  tool_include_dirs: []
  tool_exclude_dirs:
    - tests
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
# Rust tests (110+ tests across core, rules, and project crates)
PYO3_PYTHON="$PWD/.venv/bin/python" cargo test --workspace --manifest-path rust/Cargo.toml

# Python integration tests
uv run pytest -q
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
