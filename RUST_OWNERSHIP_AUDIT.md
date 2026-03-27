# Rust Ownership Audit

This audit reflects the verified post-port state after moving the remaining
high-value static route policy checks into Rust and shrinking the Python
boundary around them.

## Verification

- `uv run python -m pytest tests/ -q` -> `99 passed`
- `uv run python -m pytest tests/test_doctor_checks.py tests/test_native_core.py -q` -> `34 passed`
- Strict static-only parity on a representative external backend stayed flat at:
  - score: `96`
  - issues: `145`
  - route_count: `101`
  - checks_not_evaluated: `3`

## Ownership Matrix

| Bucket | Canonical owner | Python status | Notes |
| --- | --- | --- | --- |
| Static AST/rule logic | Rust | Compatibility baggage only | Canonical analysis lives in `rust/fastapi_doctor_core` and `rust/fastapi_doctor_rules`. The legacy `src/fastapi_doctor/checks/*.py` modules are no longer the main runtime path. |
| Route/static policy logic | Rust | Thin runtime-only boundary plus lazy legacy shims | Rust now owns static route extraction and these checks: `security/forbidden-write-param`, `correctness/duplicate-route`, `correctness/missing-response-model`, `correctness/post-status-code`, `api-surface/missing-tags`, `api-surface/missing-docstring`, `api-surface/missing-pagination`, `architecture/fat-route-handler`. |
| Project/config/discovery glue | Rust-first | Python fallback/adapter remains | Rust is the default owner of layout discovery, library detection, and effective config payload via `fastapi_doctor_project` and native `get_project_context()`. `src/fastapi_doctor/project.py` still carries fallback heuristics and `parsed_python_modules()` for non-native/test paths. |
| Runtime-only FastAPI/OpenAPI glue | Python | Active owner | FastAPI import/bootstrap, live route objects, OpenAPI schema inspection, external tool execution, and output formatting remain Python-owned by design. |

## Rust-Owned Static Route Policy

These route/static checks are now native-owned on the main path:

- `security/forbidden-write-param`
- `correctness/duplicate-route`
- `correctness/missing-response-model`
- `correctness/post-status-code`
- `api-surface/missing-tags`
- `api-surface/missing-docstring`
- `api-surface/missing-pagination`
- `architecture/fat-route-handler`

Implementation lives in:

- `rust/fastapi_doctor_core/src/analysis.rs`
- `rust/fastapi_doctor_rules/src/routes.rs`
- `rust/fastapi_doctor_rules/src/engine.rs`
- `rust/fastapi_doctor_native/src/lib.rs`

Python now consumes those results through `src/fastapi_doctor/native_core.py` and
`src/fastapi_doctor/runner.py`.

## Explicit Keep-In-Python Exceptions

These should stay Python-owned until there is a better native representation:

- `app_loader.py`
  - FastAPI import/bootstrap and live app construction.
- `check_openapi_schema()`
  - Requires a live FastAPI app and `app.openapi()` output.
- `security/missing-auth-dep`
  - Still depends on `PROTECTED_ROUTE_RULES`, which has no real native config model yet.
- `cli.py`, `console.py`, `reporting.py`, `external_tools.py`
  - CLI, presentation, and external command execution are boundary concerns.

## Compatibility Baggage

The following Python code still exists primarily for compatibility, not main-path
ownership:

- `src/fastapi_doctor/checks/__init__.py`
  - Lazy compatibility shim for the legacy checks package surface.
- `src/fastapi_doctor/checks/static_checks.py`
  - Lazy compatibility shim for the old static checks import surface.
- `src/fastapi_doctor/checks/route_checks.py`
  - Active owner only for `check_route_dependency_policies()` and `check_openapi_schema()`.
  - Old Python static route checks moved behind lazy imports in `src/fastapi_doctor/checks/_legacy_route_checks.py`.
- `src/fastapi_doctor/project.py`
  - Still contains fallback discovery/config parsing and `parsed_python_modules()` for native-unavailable/test scenarios.

## Migration Targets Ordered By Value

1. Native protected-route policy config
   - Add a real Rust-owned config model for protected route dependency requirements.
   - Port `security/missing-auth-dep` into Rust.
   - Delete `check_route_dependency_policies()` from the main path.

2. Delete or quarantine legacy Python rule modules
   - Convert the remaining `checks/*.py` implementations into explicit deprecated shims or remove them once test/import compatibility allows.
   - The goal is to stop carrying duplicate rule logic entirely.

3. Remove Python suppression fallback from the main path
   - Native suppressions are already preferred.
   - Once native becomes mandatory for static runs, delete the fallback reparsing path in `runner.py`.

4. Collapse `project.py` further
   - Keep only native-context application plus a very small fallback.
   - Long term, `parsed_python_modules()` should not be a first-class ownership path.

## Performance Snapshot

Measured against the legacy Python-first tool on the same machine:

- Python package import:
  - legacy: `0.186542s`
  - current: `0.041857s`

- Strict static-only self-scan:
  - legacy: `0.666288s` cold, `0.640081s` warm
  - current: `0.236197s` cold, `0.227556s` warm

- Strict static-only scan on a representative external backend:
  - legacy: `11.663199s` cold, `11.007550s` warm
  - current: `1.108972s` cold, `1.151745s` warm
