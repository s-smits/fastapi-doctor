# FastAPI Doctor — Rule Definitions

30+ opinionated rules across 8 categories, tuned for Python/FastAPI backends.
Unique rule violations are counted (not instances), then penalized.

## Scoring

```
score = 100 - (unique_error_rules × 2.0 + unique_warning_rules × 1.0)
```

Score bands: **80+ Great** | **60-79 Needs work** | **<60 Critical**

## Issue Classification (v1.1)

Every finding now carries three metadata fields:

| Field | Values | Purpose |
|-------|--------|---------|
| `kind` | `blocker` · `risk` · `opinionated` · `hygiene` | Real-world impact class — gate shipping on blocker count, not only score |
| `confidence` | 0.0–1.0 | How likely this is a true positive |
| `action_type` | `code_fix` · `config_tune` · `suppress_with_reason` · `review_manually` | Recommended response |

The report also exposes `blocker_count`, `has_ship_blockers`, `checks_not_evaluated`, and `suppressions`.

## Suppression Syntax

Preferred (structured, with audit trail — appears in JSON `suppressions`):
```python
x = val  # doctor:ignore security/hardcoded-secret reason="enum label, not a secret"
```

Legacy (still works, but no reason or JSON trail):
```python
x = val  # noqa
x = val  # noqa: security/hardcoded-secret
x = val  # noqa: sql-safe
```

## Security Rules

### `security/missing-auth-dep` (error)
Configured protected routes must carry their declared FastAPI dependencies.
This rule is policy-driven: if no protected route rules are configured, it is a no-op.

### `security/forbidden-write-param` (error)
Write endpoints (POST/PUT/PATCH/DELETE) must not accept configured forbidden
ownership parameters. Identity should come from auth or request context, not
client-supplied fields.

### `security/weak-hash-without-flag` (error)
SHA1/MD5 used for non-security purposes (caching, fingerprinting) must include `usedforsecurity=False`.
Without this flag, Bandit reports high-severity CWE-327 violations.

### `security/unsafe-yaml-load` (error)
`yaml.load()` without SafeLoader or BaseLoader enables arbitrary code execution.
Use `yaml.safe_load()` or explicit `Loader=yaml.SafeLoader`.

### `security/hardcoded-secret` (error, kind: blocker)
Detects hardcoded API keys, tokens, and passwords. Two detection paths:
1. **Value-pattern match** — known prefixes (Stripe `sk_live_`, AWS `AKIA`, GitHub `ghp_`, etc.)
   are always flagged regardless of variable name.
2. **Name + value evidence** — variable names matching secret patterns (`api_key`, `password`,
   `credential`, etc.) are only flagged when the **value** also looks like a real secret
   (mixed character classes, high entropy). Plain identifiers (`"oauth_token"`),
   enum labels (`"credential_type"`), placeholders (`"fake-encrypted"`), and URLs
   are explicitly excluded.

### `security/assert-in-production` (error)
`assert` statements are stripped by Python when running with optimization (`-O`).
In production paths, use explicit `raise` statements instead.

### `security/pydantic-secretstr` (warning)
Sensitive fields in Pydantic models (passwords, tokens, keys) should use `SecretStr`.
This prevents accidental leakage in logs, `repr()`, and error messages.

## Correctness Rules

### `correctness/duplicate-route` (error)
Same HTTP method + path registered twice causes silent route shadowing.

### `correctness/sync-io-in-async` (error)
Synchronous I/O calls (`open()`, `os.open()`, `fcntl.flock()`, `Path.write_text()`,
`time.sleep()`, `requests.*`, and similar blocking helpers) inside async code block
the entire event loop, stalling all concurrent requests. Use `aiofiles`,
`asyncio.sleep()`, `asyncio.to_thread()`, or `httpx.AsyncClient` instead.

### `correctness/missing-response-model` (warning)
API endpoints in `/api/` should declare `response_model=` for type safety and auto-generated OpenAPI docs.
Exempt: streaming, export, download, webhook, and OAuth endpoints.

### `correctness/weak-response-model` (warning)
API endpoints with `response_model=dict`, `dict[...]`, `Any`, `list[dict...]`, or `Mapping[...]`
still have a weak contract even though a response model is technically present.
Prefer concrete Pydantic models so OpenAPI stays explicit and typed.

### `correctness/post-status-code` (warning)
POST endpoints that create resources should return 201, not the default 200.
Only flagged for endpoints with clear creation semantics.

### `correctness/serverless-filesystem-write` (warning)
Writes to local disk outside `/tmp` are risky in serverless environments because the filesystem is usually non-durable and often read-only outside temp storage.
The rule keeps real writes as findings, but treats `/tmp`, `tempfile`, and recognized serverless temp helpers as safe.

## Architecture Rules

### `architecture/giant-function` (error)
Functions >400 lines are genuinely unmanageable. Python functions are naturally longer than
React components (type hints, docstrings, explicit error handling), so the threshold is
higher than react-doctor's. Extract sub-functions with clear responsibilities.

### `architecture/large-function` (warning)
Functions >200 lines should be considered for splitting.

### `architecture/god-module` (warning)
Files >1500 lines are untestable monoliths. Extract cohesive groups of functions into
separate modules — each module should have one reason to change.

### `architecture/deep-nesting` (warning)
Functions with >5 levels of control-flow nesting (if → for → try → if → with → ...)
are unreadable. Use early returns, guard clauses, or extract nested blocks into helpers.

### `architecture/async-without-await` (warning)
`async def` route handlers that never use `await`, `async for`, or `async with` block the event loop.
FastAPI runs plain `def` handlers in a threadpool, which is safer for synchronous code.

### `architecture/print-in-production` (warning)
Production code should use the structured logger, not `print()`.
`print()` output is unstructured, unleveled, and easy to miss in production logging.

## API Surface Rules

### `api-surface/missing-tags` (warning)
Route-level: API routes should have `tags=` for OpenAPI grouping.

### `api-surface/missing-docstring` (warning)
Endpoint handler functions should have docstrings.
FastAPI uses them as OpenAPI operation descriptions.

### `api-surface/missing-pagination` (warning)
Collection endpoints should expose standard pagination query parameters.
This helps OpenAPI consumers discover `limit`/`offset`-style pagination contracts consistently.

## Pydantic Rules

### `pydantic/deprecated-validator` (error)
`@validator` is Pydantic v1 — use `@field_validator` (v2). The v1 decorator has different
semantics (receives the value positionally, `pre=True` vs `mode='before'`) and will be
removed in Pydantic v3. Migrate now.

### `pydantic/mutable-default` (error)
Bare mutable defaults (`field: list[X] = []`) in BaseModel classes cause shared-state bugs.
Use `field: list[X] = Field(default_factory=list)` instead. Pydantic v2 handles this better
than v1, but explicit `default_factory` is clearer and safer.

### `pydantic/extra-allow-on-request` (warning)
Request models (in routers/interfaces) with `extra="allow"` accept arbitrary user input.
Unknown fields can leak into DB operations, logs, or downstream services.
Use `extra="ignore"` (silently drop) or `extra="forbid"` (reject with 422).

### `pydantic/normalized-name-collision` (warning)
Pydantic models should not define the same conceptual field twice with spelling-only variations
such as `message_id`, `messageId`, or `message-id`. The rule also catches constructor calls that
pass multiple keyword variants for the same normalized name. Keep one canonical field name and
use a single alias on that field for wire-format compatibility.

### `pydantic/should-be-model` (warning)
TypedDict, NamedTuple, @dataclass, or dict-factory patterns that should be Pydantic BaseModels.
Uses **trust-boundary analysis** — only flags patterns at API boundaries (routers/, interfaces/,
schemas/, endpoints/) or with API-suggestive names (*Request, *Response, *Schema, *Payload,
*Body, *Input, *Output).

**When Pydantic IS the right choice (API boundaries):**
- Request/response schemas in FastAPI endpoints
- Any data from outside the trust boundary (user input, webhook payloads, external APIs)
- Configuration loading (pydantic-settings)

**When alternatives are legitimate (internal code):**
- `@dataclass` in services/utils/agents — trusted data, no validation overhead needed
- `@dataclass(frozen=True)` anywhere — immutable value objects, hashable
- `@dataclass(slots=True)` anywhere — performance-optimized internal types
- `TypedDict` in services — static typing without runtime cost
- `TypedDict(total=False)` — partial update / PATCH patterns
- `NamedTuple` with ≤3 fields — lightweight value objects, cache keys, coordinates
- Library code (`lib/`) — external dependency, not your API surface

## Resilience Rules

### `resilience/bare-except-pass` (warning)
`except Exception: pass` without any logging or explanatory comment silently swallows errors.
At minimum, add a `# reason` comment or `logger.debug()` call.

### `resilience/sqlalchemy-pool-pre-ping` (warning)
SQLAlchemy engine without `pool_pre_ping=True` can't automatically recover from dropped
database connections, leading to `OperationalError` on subsequent requests.

### `resilience/exception-log-without-traceback` (warning)
Inside `except ... as exc` blocks, logging `exc` via `logger.warning/error/...`
without `logger.exception(...)` or `exc_info=True` loses the traceback. This rule
fires when the handler swallows or converts control flow instead of plainly re-raising.

## Config Rules

### `config/direct-env-access` (warning)
Router and service code should use a typed settings/config layer, not raw
`os.environ` access. Direct env access bypasses validation, typing, and defaults.

### `config/env-mutation` (warning)
Mutating `os.environ` or calling `os.putenv()` outside bootstrap entrypoints
(`main.py`, `__main__.py`, `cli.py`, `scripts/`) creates hidden global side effects.
Perform env setup once at startup, then pass values through typed settings.

### `config/alembic-target-metadata` (warning)
Alembic `env.py` should be wired to your SQLAlchemy/SQLModel metadata object
for reliable autogeneration.

### `config/alembic-empty-autogen-revision` (warning)
Configure `process_revision_directives` in `env.py` to skip generating empty migration files.

### `config/sqlalchemy-naming-convention` (warning)
Database metadata should use a naming convention for constraints to ensure
deterministic migration names across environments.

## Doctor Rules

### `doctor/app-bootstrap-failed` (error, kind: blocker)
The FastAPI app failed to import/start, so all route-level checks (auth deps,
duplicate routes, OpenAPI schema, response models, pagination, docstrings) were
skipped. The report lists skipped checks in `checks_not_evaluated`. The score
may appear clean, but route-level verification did not run. Fix the startup
error before trusting the report.

## Extension Guidance

If you add a new rule, update:
- `src/fastapi_doctor/runner.py` and the appropriate `src/fastapi_doctor/checks/*.py` module
- `tests/test_doctor_checks.py` (regression tests)
- this reference file (rule definitions)
- `skills/fastapi-doctor/SKILL.md` (rule tables)
