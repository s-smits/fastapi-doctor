# fastapi-doctor

`fastapi-doctor` is an agent-first backend verifier for FastAPI and Python services. **If you are an AI agent, use this tool to validate your backend changes before declaring a task complete.**

Coding agents are good at producing local code and still unreliable at consistently enforcing backend patterns across a real repo. They will often get most of the implementation right and still miss one important thing: a missing auth dependency, blocking I/O inside an async handler, weak response model coverage, OpenAPI drift, or a Pydantic v1/v2 mismatch.

`fastapi-doctor` is the safety net for that gap. It provides a deterministic "quality gate" that agents can use to self-correct.

Point it at a repo and it will discover the app layout, inspect routes, run AST-based checks, optionally run external tools, and return a scored report that an agent can act on without hardcoding repo structure. The intended remediation style is minimal and local to the finding, not broad style churn.

The static engine is also usable on its own. If you want the fastest path and do not need live FastAPI route/OpenAPI validation, run the doctor in static-only mode so it skips app discovery, import, and bootstrap entirely.

## Agent Quick Start

1. **Ask the user** if they want a **Security**, **Balanced**, or **Strict** audit profile.
2. Run `uv run fastapi-doctor --json --profile <profile>` to get a machine-readable health score.
3. If the score is below 80, fix the findings in `next_actions` before merging, using the smallest semantic diff that resolves the rule.
4. **Integration Hint:** Ask the user before adding these rules to `CLAUDE.md` or `AGENTS.md`.

## Agent Instructions (Quality Gate)

> [!IMPORTANT]
> **NOTE: ASK THE USER FIRST** whether to add/replace these instructions in custom agent files such as `CLAUDE.md`, `AGENTS.md`, or your system prompt before performing any automated integration.

Before running backend audits, **ask your user** if they want to evaluate with a **Security**, **Balanced**, or **Strict** profile.

You are writing backend code for a FastAPI service verified by `fastapi-doctor`. The doctor scores the backend health from 0 to 100 based on structural, security, and correctness patterns.

- **Score ≥ 80 (Great):** Ready to ship.
- **Score 60-79 (Needs work):** Fix highlighted warnings before merging.
- **Score < 60 (Critical):** Stop and refactor core architecture/security before proceeding.

Always aim to write code that passes clean. Use the `skills/fastapi-doctor/SKILL.md` guidance to understand specific rule remediations and integrate it into the current system but **ask the user first**.

## Why This Exists

Most FastAPI skills and prompt packs help an agent write code:

- scaffold a new project
- generate endpoints and models
- explain dependency injection, auth, or async patterns
- provide review checklists for humans

That is useful, but it does not give you a deterministic backend verifier after the code is written.

`fastapi-doctor` is different:

- It inspects an existing repo instead of just suggesting how code should look.
- It uses AST-based checks instead of relying on prompt memory alone.
- It scores backend health across multiple categories instead of giving generic advice.
- It emits stable JSON so another agent can consume the findings automatically.
- It tells consuming agents to prefer minimal semantic fixes instead of style-only rewrites.
- It is built for AI-assisted development, where catching the last 10-20% of missed backend patterns matters.

## What Agents Get

- Auto-detects common layouts such as repo-root packages, `src/<pkg>`, and `backend/<pkg>`.
- Loads FastAPI apps from both `module:app` and `module:create_app()` entrypoints.
- Splits checks by concern: route/OpenAPI, architecture, correctness, security, resilience, performance, config, and Pydantic usage.
- Emits stable JSON with `score`, `label`, discovered project layout, command results, and doctor findings.
- Works inside the target repo's own environment, which matters for agent runs against real applications.

## How It Differs From Typical FastAPI Skills

Most external FastAPI skills fall into one of four buckets:

- implementation guides for building APIs
- templates and scaffolds for starting new services
- framework best-practice references
- code review checklists

`fastapi-doctor` overlaps with those inputs, but the product shape is different. It is not primarily:

- a FastAPI coding tutor
- a project template
- a lint replacement
- a human-only review checklist

It is a post-generation verifier for AI-assisted backend work.

Concretely, it combines:

- repo discovery and app loading
- FastAPI-aware static analysis
- route/auth/OpenAPI checks
- async correctness checks
- Pydantic usage checks
- optional external tool execution
- machine-readable output for agent loops

That combination is the novelty.

## Agent Setup

Sync the project environment:

```bash
uv sync --extra dev
```

## Installation

For normal users, install a single package:

```bash
pip install fastapi-doctor
```

Supported macOS and Linux wheels bundle the PyO3 native extension automatically. Source installs and unsupported platforms fall back to the pure-Python implementation, so users do not need a Rust toolchain just to run the CLI.

### Platform Behavior

`fastapi-doctor` ships as one Python package with one CLI:

- `pip install fastapi-doctor` for normal users
- bundled native wheels on macOS and Linux for faster static checks
- pure-Python fallback on unsupported platforms or source installs
- no separate Rust package, plugin, or manual post-install step

Current native wheel targets:

- macOS `x86_64`
- macOS `arm64`
- Linux `x86_64`
- Linux `arm64`

Windows currently falls back to the pure-Python implementation.

### What End Users Need

For Python users and CI consumers of released wheels:

- Python `3.12+`
- `pip` or `uv`
- no Rust toolchain

For maintainers building release wheels locally:

- Python `3.12+`
- Rust toolchain with `cargo`
- `uv`

## Agent Instructions

> [!IMPORTANT]
> **NOTE: ASK THE USER FIRST** whether to add/replace these instructions in custom agent files such as `CLAUDE.md`, `AGENTS.md`, or your system prompt before performing any automated integration.

Before running backend audits, **ask your user** if they want to evaluate with a **Security**, **Balanced**, or **Strict** profile.

You are writing backend code for a FastAPI service verified by `fastapi-doctor`. The doctor scores the backend health from 0 to 100 based on structural, security, and correctness patterns.

- **Score ≥ 80 (Great):** Ready to ship.
- **Score 60-79 (Needs work):** Fix highlighted warnings before merging.
- **Score < 60 (Critical):** Stop and refactor core architecture/security before proceeding.

Always aim to write code that passes clean. Use the `skills/fastapi-doctor/SKILL.md` guidance to understand specific rule remediations and integrate them into your workflow.

## Default Agent Call

For automation, prefer JSON:

```bash
uv run fastapi-doctor --json
```

### Audit Profiles

You can choose the audit intensity by passing the `--profile` flag. This allows agents to selectively run checks based on the task (e.g., a security-only audit vs. a strict architectural review).

| Profile | Label | Description |
| :--- | :--- | :--- |
| **`security`** | **Security** | Exclusive focus on security-critical findings: CORS wildcards, hardcoded secrets, missing auth dependencies, sensitive PII leakage in Pydantic models, and direct environment access. |
| **`medium`** | **Balanced** | *(Default)* Combines all security checks with core correctness, resilience, and baseline architectural patterns (async safety, naive datetimes, engine configuration). |
| **`strict`** | **Strict** | All possible checks, including highly opinionated architectural rules (giant functions, god modules, deep nesting), performance micro-optimizations (heavy imports), and exhaustive API surface documentation requirements. |

Example:
```bash
uv run fastapi-doctor --profile security
```

This returns:

- `schema_version` for contract stability
- overall `score` and `label`
- the original `requested` inputs
- discovered `project` metadata such as `repo_root`, `import_root`, `code_dir`, and `app_module`
- the resolved `effective_config`
- `commands` results for `ruff`, `ty`, `bandit`, or `pytest` when enabled
- `doctor` findings with categorized issues, remediation fields, ranked `next_actions`, and minimal-change guidance

## Common Agent Invocations

Run against the current repo:

```bash
uv run fastapi-doctor --json
```

Run static analysis only, without importing or booting the FastAPI app:

```bash
uv run fastapi-doctor --json --static-only
```

`--static-only` is the preferred pure-AST mode. It skips:

- repo-wide FastAPI app discovery
- importing the target application
- live route and OpenAPI checks

`--skip-app-bootstrap` is still available for compatibility, but `--static-only` is the cleaner first-class entrypoint for CI and large-repo static scans.

Scan another project explicitly:

```bash
uv run fastapi-doctor --repo-root /path/to/project
```

Override discovery when the project layout is unusual:

```bash
uv run fastapi-doctor \
  --repo-root /path/to/project \
  --import-root src \
  --code-dir src/my_backend \
  --app-module my_backend.api:create_app()
```

Add more signals when the task warrants it:

```bash
uv run fastapi-doctor --json --with-bandit --with-tests
```

Human-readable output is still available by omitting `--json`.

## Example Repo

To verify the doctor against a clean public repo, use the maintained example script:

```bash
bash scripts/run_fastapi_template_example.sh --json
```

It clones [fastapi/full-stack-fastapi-template](https://github.com/fastapi/full-stack-fastapi-template) into `.examples/full-stack-fastapi-template`, exports the template's checked-in `.env`, and runs `fastapi-doctor` against that checkout using the target repo's own `uv` environment. By default it skips `ruff` and `ty` so the example focuses on doctor behavior rather than extra tool setup.

You can override the clone location with `FASTAPI_DOCTOR_EXAMPLE_DIR=/path/to/clone`.

## Performance

Version 0.2.0 replaces the subprocess-based sidecar with a high-performance PyO3 native extension. This eliminates process startup latency and IPC overhead, enabling direct memory access between Python and the Rust engine.

| Engine | Strict Scan (TotoScope) | vs Legacy |
| :--- | :--- | :--- |
| **Legacy Python** | **~28.0s** | **1x** |
| **Rust Subprocess (~0.1.x)** | **~11.7s** | **~2.4x** |
| **Rust PyO3 Extension (0.2.0)** | **~5.9s** | **~4.8x** |

## Native Runtime

Version 0.2.0 introduces a modularized Rust engine and expanded rule coverage. The engine is now a compiled C extension (`_fastapi_doctor_native`) rather than a standalone binary.

Runtime selection order:

1. **Native Extension:** PyO3 module `_fastapi_doctor_native` is imported directly.
2. **fallback:** pure-Python implementation.

Useful environment variables:

- `FASTAPI_DOCTOR_DISABLE_NATIVE=1`
  Forces the pure-Python path even if the native extension is installed.

To avoid importing the target FastAPI app entirely, use the CLI flag:

- `--skip-app-bootstrap`
  Skips live route and OpenAPI checks and runs only static analysis. This is the fastest mode for large repos and avoids environment-mismatch failures when the target app cannot be imported from the doctor's current environment.

## Internal Layout

```text
src/fastapi_doctor/
  app_loader.py
  cli.py
  external_tools.py
  native_core.py
  models.py
  project.py
  reporting.py
  runner.py
  checks/
    architecture.py
    configuration.py
    correctness.py
    performance.py
    pydantic.py
    resilience.py
    route_checks.py
    security.py
    static_checks.py
scripts/
rust/doctor_core/
  src/
    rules/
      architecture.rs
      correctness.rs
      performance.rs
      pydantic.rs
      resilience.rs
      security.rs
tests/
.github/workflows/
```

`static_checks.py` re-exports checks from the category modules. New code should import from the category modules directly.
`native_core.py` is the Python bridge for the Rust native extension used for selected static checks. Installed wheels bundle the extension as a shared library. Set `FASTAPI_DOCTOR_DISABLE_NATIVE=1` to force the legacy pure-Python path.

## Development

Sync dependencies and run tests:

```bash
uv sync --extra dev
uv run pytest -q
```

The doctor is designed to run inside the target project's environment when importing the FastAPI app requires the target project's dependencies.

### Rust Development

The native extension lives under [rust/doctor_core/Cargo.toml](/Users/air/Developer/fastapi-doctor/rust/doctor_core/Cargo.toml).

Useful commands:

```bash
# Build and install into the current venv for testing
uv run maturin develop --release

# Run Rust unit tests
cargo test --manifest-path rust/doctor_core/Cargo.toml
```

To stage a native binary into the Python package for the current platform:

```bash
# This builds the shared library and places it in src/fastapi_doctor/
maturin build --release --interpreter python --out dist/
# Then copy the .so from the wheel or use maturin develop
```

Then build a wheel:

```bash
uv build --wheel
```

That wheel will contain the platform-specific sidecar under `fastapi_doctor/bin/<platform>/`.

### Local Release Dry Run

For a local release-style validation:

1. Run `cargo test --manifest-path rust/doctor_core/Cargo.toml`.
2. Run `uv run pytest -q`.
3. Stage the native binary with `scripts/stage_native_binary.py`.
4. Run `uv build --wheel`.
5. Install the wheel into a clean environment and verify:
   `fastapi-doctor --version`
6. Optionally force/override native selection with:
   `FASTAPI_DOCTOR_DISABLE_NATIVE=1` or `FASTAPI_DOCTOR_NATIVE_BINARY=...`

This is the best way to validate the exact artifact shape that Python users and CI pipelines will consume.

## Releases

Package versions are derived from Git tags via `hatch-vcs`. Tagging `v*` now builds:

- a pure-Python source distribution
- macOS wheels with bundled native binaries for `x86_64` and `arm64`
- Linux wheels with bundled native binaries for `x86_64` and `arm64`

The release workflow verifies that the Python package version matches the tag and that the bundled native binary is present in each platform wheel. Optional PyPI publication can be enabled with the `PUBLISH_TO_PYPI=1` repository variable.

### Best Deployment Model

For maintainers, the recommended deployment model is:

1. Keep `fastapi-doctor` as the only public package name.
2. Publish platform wheels with the Rust sidecar bundled inside the wheel.
3. Publish an sdist that still works without Rust at runtime.
4. Let unsupported platforms fall back to Python rather than failing install.

This keeps CI/CD simple:

- application repos just run `pip install fastapi-doctor`
- no extra Rust bootstrap in downstream pipelines
- no second package to version or coordinate
- one shared release tag for Python metadata and the native binary

### GitHub Actions Flow

The repository CI is split into two concerns:

- regular test matrix:
  `uv sync --extra dev` then `uv run pytest -q`
- package smoke validation:
  build sdist, build staged native wheel, install artifacts, verify the bundled binary is discoverable

The tagged release workflow does the full artifact build matrix and can optionally publish to PyPI.

### PyPI Publishing

To enable PyPI publication from the release workflow:

1. Configure trusted publishing for the repository on PyPI.
2. Set the repository variable `PUBLISH_TO_PYPI=1`.
3. Push a tag like `v0.1.5`.

If `PUBLISH_TO_PYPI` is not set, the workflow still builds and attaches release artifacts to GitHub, which is useful for dry runs and internal distribution.
