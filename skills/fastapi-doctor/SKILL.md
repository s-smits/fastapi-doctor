---
name: fastapi-doctor
description: |
  Opinionated FastAPI/Python health checker. Audits backend quality, security gates,
  and architectural health (Ruff/Pyright/Bandit/AST). Use for backend verification,
  route/auth audits, and identifying performance-blocking sync calls in async handlers.
  Keywords: FastAPI, Ruff, Pyright, backend health, OpenAPI, route auth, Pydantic v2
last_verified: 2026-03-17
---

# FastAPI Doctor

Opinionated backend health checker for FastAPI applications. Scores the backend 0-100 using severity-weighted unique rule violations across 7 categories. All checks use AST-based static analysis — no regex hacks. Configurable via `.fastapi-doctor.yml` in your project root.

When applying findings, prefer the smallest semantic diff that resolves the rule. Do not introduce style-only rewrites such as replacing imported helpers with module-qualified calls unless the finding explicitly requires that change.

## Configuration

Place `.fastapi-doctor.yml` in your project root. All keys are optional — missing keys use defaults. See `.fastapi-doctor.example.yml` for the full schema.

```yaml
architecture:
  enabled: true           # false → skip ALL architecture rules (defer to ruff)
  giant_function: 400     # 0 → disable specific rule
  large_function: 200
  god_module: 1500
  deep_nesting: 5
  import_bloat: 30
  fat_route_handler: 100

pydantic:
  should_be_model: "boundary"  # or "everywhere"

api:
  create_post_prefixes: []     # optional route prefixes for POST->201 heuristic
  tag_required_prefixes:       # prefixes that should carry OpenAPI tags
    - "/api/"

security:
  forbidden_write_params: []   # optional param names to ban on write routes
```

## Scoring Formula

```
score = 100 - (unique_error_rules × 2.0 + unique_warning_rules × 1.0)
```

Counts **unique rule types** violated, not instances. 60 dataclass violations count as 1 warning penalty (1 point).

**Score bands:**
- **80-100 (Great)**: Ship it
- **60-79 (Needs work)**: Fix before merge
- **Below 60 (Critical)**: Stop and fix

## Categories

| Category | Rules | What it checks | Configurable |
|----------|-------|---------------|--------------|
| **Security** | 10 | Auth deps, IDOR, secrets, SQL injection, error leaks, CORS | No |
| **Correctness** | 12 | Duplicate routes, response models, sync-in-async, naive datetime | No |
| **Architecture** | 8 | Giant functions, god modules, deep nesting, import bloat, passthrough, async misuse | Yes — disable all or per-rule via `.fastapi-doctor.yml` |
| **API Surface** | 5 | OpenAPI operation IDs, tags, endpoint docstrings | No |
| **Pydantic** | 4 | Deprecated validators, mutable defaults, extra="allow", should-be-model | Yes — `boundary` or `everywhere` mode |
| **Resilience** | 4 | Bare except:pass, re-raise without context, swallowed exceptions | No |
| **Config** | 1 | Direct os.environ in services/routers | No |

## Sophisticated Checks

### `pydantic/should-be-model` — Configurable Pydantic adoption detector

Two modes via `.fastapi-doctor.yml` → `pydantic.should_be_model`:

**"boundary"** (default) — Trust-boundary analysis. Only flags at API edges (routers, interfaces, schemas) or with API-suggestive names (`*Request`, `*Response`, `*Schema`, `*Payload`, `*Body`, `*Input`, `*Output`). Internal code is free to use dataclasses and TypedDicts.

**"everywhere"** — Consistency-first. Flags ALL TypedDict/NamedTuple/dataclass. Some teams prefer Pydantic everywhere for uniform serialization and fewer mental-model switches. Both approaches are valid.

Both modes exempt: `@dataclass(slots=True)`, `@dataclass(frozen=True)`, `TypedDict(total=False)` (PATCH pattern), small NamedTuples (≤3 fields), TYPE_CHECKING blocks.

### `resilience/reraise-without-context` — AST-based noise detector
Finds except handlers that catch and immediately re-raise without adding ANY value (no logging, no cleanup, no wrapping). These try/except blocks are pure noise — remove them or add context with `raise NewError(...) from exc`.

### `architecture/passthrough-function` — AST-based abstraction detector
Finds functions whose body is a single `return other_func(same_args)` — unnecessary indirection. Smart exemptions: decorated functions, methods, validators, functions with docstrings, <2 params.

### `architecture/import-bloat` — AST-based module complexity signal
Files with >30 import statements depend on too many things. Use `TYPE_CHECKING` guards, lazy imports, or split the module. Exempts: `__init__.py`, `main.py`. Threshold configurable via `.fastapi-doctor.yml`.

## Commands

Run from the target project's working directory, or pass `--repo-root` when
scanning another checkout.

```bash
# Quick scan (doctor checks + ruff + pyright)
uv run fastapi-doctor

# Full scan (add bandit + targeted tests)
uv run fastapi-doctor --with-tests --with-bandit

# Machine-readable output
uv run fastapi-doctor --json

# Scan a different repo explicitly
uv run fastapi-doctor --repo-root /path/to/project
```

## Rule Reference

### Errors (2.0 point penalty per unique rule)

| Rule | Category | What it catches |
|------|----------|----------------|
| `security/missing-auth-dep` | Security | Protected route missing required FastAPI dependencies |
| `security/forbidden-write-param` | Security | Write endpoint accepts configured forbidden ownership parameters |
| `security/weak-hash-without-flag` | Security | SHA1/MD5 without `usedforsecurity=False` |
| `security/unsafe-yaml-load` | Security | `yaml.load()` without SafeLoader/BaseLoader |
| `correctness/duplicate-route` | Correctness | Same method+path registered twice |
| `correctness/sync-io-in-async` | Correctness | `open()`, `time.sleep()`, `requests.*` inside async handlers |
| `architecture/giant-function` | Architecture | Function body >threshold lines (configurable, default 400) |
| `api-surface/missing-operation-id` | API Surface | OpenAPI operation has no operationId |
| `api-surface/duplicate-operation-id` | API Surface | Two operations share an operationId |
| `pydantic/deprecated-validator` | Pydantic | `@validator` (v1) instead of `@field_validator` (v2) |
| `pydantic/mutable-default` | Pydantic | Bare `= []` / `= {}` default in BaseModel |
| `security/sql-fstring-interpolation` | Security | f-string injected into `text()` |
| `security/assert-in-production` | Security | `assert` usage outside of tests (can be stripped out via -O flag) |
| `security/subprocess-shell-true` | Security | Shell injection vector (`shell=True` in subprocess) |

### Warnings (1.0 point penalty per unique rule)

| Rule | Category | What it catches |
|------|----------|----------------|
| `correctness/missing-response-model` | Correctness | API route has no `response_model` |
| `correctness/post-status-code` | Correctness | Resource-creation POST defaults to 200 |
| `architecture/large-function` | Architecture | Function body >threshold lines (configurable, default 200) |
| `architecture/god-module` | Architecture | File >threshold lines (configurable, default 1500) |
| `architecture/deep-nesting` | Architecture | Function with >threshold nesting (configurable, default 5) |
| `architecture/import-bloat` | Architecture | File with >threshold imports (configurable, default 30) |
| `architecture/passthrough-function` | Architecture | Function that purely delegates to another |
| `architecture/async-without-await` | Architecture | `async def` handler that never awaits |
| `architecture/print-in-production` | Architecture | `print()` instead of logger |
| `architecture/fat-route-handler` | Architecture | Route handler >threshold lines (configurable, default 100) |
| `architecture/avoid-sys-exit` | Architecture | Hard exit from internal library logic via `sys.exit()` |
| `api-surface/missing-tags` | API Surface | Route missing tags |
| `api-surface/missing-openapi-tags` | API Surface | OpenAPI operation missing tags |
| `api-surface/missing-docstring` | API Surface | Endpoint handler has no docstring |
| `pydantic/extra-allow-on-request` | Pydantic | Request model uses `extra="allow"` |
| `pydantic/should-be-model` | Pydantic | TypedDict/NamedTuple/dataclass/dict-factory should be BaseModel |
| `resilience/bare-except-pass` | Resilience | `except: pass` without logging or comment |
| `resilience/reraise-without-context` | Resilience | Re-raise without adding any context |
| `config/direct-env-access` | Config | Service/router reads `os.environ` instead of settings |
| `security/exception-detail-leak` | Security | Exposing unhandled internal `Exception` messages to users |
| `correctness/naive-datetime` | Correctness | Usage of `datetime.utcnow()` or `now()` without timezones |
| `correctness/avoid-os-path` | Correctness | Usage of `os.path` APIs instead of `pathlib.Path` |

## Thresholds (configurable via `.fastapi-doctor.yml`)

| Threshold | Default | Config key | Why |
|-----------|---------|-----------|-----|
| Giant function | 400 lines | `architecture.giant_function` | Python is more verbose than React |
| Large function | 200 lines | `architecture.large_function` | Worth splitting but not critical |
| God module | 1500 lines | `architecture.god_module` | Untestable monolith |
| Deep nesting | 5 levels | `architecture.deep_nesting` | Unreadable beyond this |
| Import bloat | 30 imports | `architecture.import_bloat` | Module depends on too many things |
| Fat route handler | 100 lines | `architecture.fat_route_handler` | Business logic belongs in services |
| Dict-factory keys | 7+ keys | — | Strong proto-model signal |
| Score "Great" | ≥80 | — | Backend is the security boundary |
| Score "Needs work" | ≥60 | — | - |

Set any threshold to `0` to disable that specific rule. Set `architecture.enabled: false` to disable all architecture rules (defer to ruff/pylint).
