# FastAPI Doctor Checklist

> **Prerequisite:** Run from the target project's working directory, or pass
> `--repo-root /path/to/project` when scanning a different checkout.

## Fast Pass (score + ruff + pyright)

```bash
uv run fastapi-doctor
```

Ask the user to pick `security`, `balanced`, or `strict` first. If the repo runtime is older than Python `3.12`, install the wheel in an isolated tool env:

```bash
uv tool install --python 3.12 --index https://s-smits.github.io/fastapi-doctor/simple/ fastapi-doctor
```

Use for:
- normal backend edits
- route refactors
- schema changes
- auth/dependency edits

Target: **score >= 80** (Great)

## Deep Pass (add bandit + targeted tests)

```bash
uv run fastapi-doctor --with-tests --with-bandit
```

Use for:
- security-sensitive work
- owner/IDOR changes
- internal auth changes
- before pushing backend refactors

Target: **score >= 80**, 0 high-severity bandit issues, all tests passing

## Schema Pass

```bash
uv run fastapi-doctor-export-openapi --stdout
```

Use for:
- inspecting generated operation IDs
- checking route/tag drift
- validating endpoint additions/removals

## Machine-Readable Output

```bash
uv run fastapi-doctor --json
```

Returns JSON with `score`, `label`, `categories`, `issues[]`, and external tool results.
Useful for CI integration.

## Monorepo Pass

```bash
uv run fastapi-doctor \
  --json \
  --profile strict \
  --repo-root . \
  --code-dir apps/service_api \
  --import-root . \
  --app-module apps.service_api.main:app
```

Use for:
- `apps/<service>` layouts
- `services/<service>` layouts
- repo roots that also contain unrelated Python

## Interpreting the Score

The score uses severity-weighted unique rule violations:
`100 - (error_rules × 2.0 + warning_rules × 1.0)`.

It counts **unique rule types** violated, not instances. So 11 giant-function violations
only cost 2 points (one error rule), but violating 10 different rules costs 20 points.

Penalties are tuned for Python/FastAPI backends (higher than react-doctor) because
backends are the security boundary.

### Quick Score Improvement Levers

1. Fix all **error-severity** rules first (2 pts each)
2. Migrate `@validator` → `@field_validator` in Pydantic models
3. Fix mutable defaults in BaseModel classes
4. Add `# reason` comments to intentional `except: pass` blocks
5. Add `response_model=` to API endpoints
6. Add docstrings to endpoint handlers
7. Replace `print()` with `logger`

## Follow-up Commands

```bash
uv run pytest tests/routers/ -q
uv run pytest tests/interfaces/ -q
uv run pytest tests/test_doctor_checks.py -q
uv run pyright
uv run ruff check <code-dir-or-package>
```
