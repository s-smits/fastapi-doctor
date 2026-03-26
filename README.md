# fastapi-doctor

`fastapi-doctor` is an agent-first backend verifier for FastAPI and Python services. It provides a deterministic "quality gate" that agents and developers use to validate backend patterns, security, and structural correctness.

## Why This Exists
While AI agents excel at local implementation, they often miss system-wide patterns like missing auth dependencies, blocking I/O in async handlers, OpenAPI drift, or Pydantic version mismatches. `fastapi-doctor` fills this gap by:
- Using **AST-based checks** rather than relying on LLM memory.
- Scoring backend health (0-100) across multiple categories.
- Emitting **stable JSON** for automated agent self-correction.
- Prioritizing **minimal semantic fixes** over broad style rewrites.

## Agent Quick Start
1. **Profile:** Ask the user for a **Security**, **Balanced**, or **Strict** audit profile.
2. **Execute:** Run `uv run fastapi-doctor --json --profile <profile>`.
3. **Remediate:** If the score is below 80, resolve findings in `next_actions` using the smallest possible diff.

> [!IMPORTANT]
> **Integration Hint:** Ask the user before adding these rules to `CLAUDE.md` or `AGENTS.md`.

## Key Features
- **Auto-Discovery:** Detects layouts like `repo-root`, `src/<pkg>`, or `backend/<pkg>`.
- **Hybrid Engine:** Rust-powered static analysis (`v0.2.0+`) with transparent Python fallback.
- **Context Aware:** Works inside the target repo's own environment for accurate importing.
- **Category Split:** Checks route/OpenAPI, architecture, security, performance, and Pydantic usage.

## Installation
```bash
pip install fastapi-doctor
```
Supported macOS and Linux wheels bundle the PyO3 native extension automatically. Source installs and unsupported platforms fall back to the pure-Python implementation, so users do not need a Rust toolchain just to run the CLI.

## Audit Profiles
| Profile | Focus |
| :--- | :--- |
| **`security`** | CORS, secrets, auth dependencies, PII leakage, and env access. |
| **`medium`** | *(Default)* Security + correctness, resilience, and async safety. |
| **`strict`** | All checks + opinionated architecture and performance micro-optimizations. |

## Common Invocations
```bash
# Standard machine-readable report
uv run fastapi-doctor --json

# Fast static-only scan (skips app import/bootstrap)
uv run fastapi-doctor --json --static-only

# Comprehensive audit with external tools
uv run fastapi-doctor --json --with-bandit --with-tests

# Explicitly point to a specific app
uv run fastapi-doctor --app-module my_pkg.main:app
```

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

## Internal Layout
- `src/fastapi_doctor/`: Python wrapper, CLI, and live FastAPI route/OpenAPI checks.
- `rust/doctor_core/`: Modularized Rust engine for high-performance static analysis.
- `scripts/`: Staging and release utilities.
- `tests/`: End-to-end and unit tests.

## Development
```bash
# Run Python tests
uv run pytest -q

# Run Rust tests
cargo test --manifest-path rust/doctor_core/Cargo.toml
```

### Rust Development
The native extension lives under [rust/doctor_core/Cargo.toml](/Users/air/Developer/fastapi-doctor/rust/doctor_core/Cargo.toml).

Useful commands:
```bash
# Build and install into the current venv for testing
uv run maturin develop --release

# Run Rust unit tests
cargo test --manifest-path rust/doctor_core/Cargo.toml
```

To build a wheel for the current platform:
```bash
uv run maturin build --release
```
