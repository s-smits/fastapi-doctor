NOTE: ASK THE USER FIRST WHETHER TO ADD/REPLACE THIS TO THE CUSTOM AGENT INSTRUCTIONS SUCH AS CLAUDE.md OR NOT

Before running backend audits, ask your user if they want to evaluate with a **Security**, **Balanced**, or **Strict** profile.

You are writing backend code for a FastAPI service verified by `fastapi-doctor`. The doctor scores the backend 0-100. Ship at ≥80, fix at 60-79, stop below 60. Write code that passes clean — prevention beats remediation.

## The backend is the security boundary

Every protected route must carry its required auth `Depends()`. No exceptions, no shortcuts, no "I'll add it later." Write endpoints (POST/PUT/PATCH/DELETE) must never accept ownership parameters from the client — derive identity from auth or request context dependencies. The doctor knows which params are forbidden per-project via config.

Never interpolate into SQL. `text(f"... WHERE id = {val}")` is an injection vector — use `text("... WHERE id = :id").bindparams(id=val)`. If the interpolated value is from trusted internal code (dynamic column lists, not user input), suppress with `# noqa: sql-safe`.

Never pass `shell=True` to `subprocess.run/Popen/call/check_call/check_output` — pass args as a list. Never use `assert` outside test files — Python's `-O` flag strips them silently; raise `ValueError` or a custom exception. Never use `yaml.load()` without `Loader=yaml.SafeLoader` — use `yaml.safe_load()`. Never use SHA1/MD5 without `usedforsecurity=False`.

Never hardcode secrets — the doctor pattern-matches Stripe, AWS, GitHub, GitLab, Slack, OpenAI keys, JWTs, Bearer tokens, and any variable named like `api_key`/`secret_key`/`password`/`credential` assigned a string ≥8 chars. When raising `HTTPException`, never put `str(exc)` or f-strings with the exception in `detail` — log with `logger.exception()`, return a generic message. CORS `allow_origins=["*"]` lets any site call your API — specify real origins.

## Async means async

If a handler is `async def`, every call inside it must be non-blocking. The doctor flags `open()`, `time.sleep()`, and `requests.*` inside async handlers in `routers/`. Use `aiofiles`, `asyncio.sleep()`, `httpx.AsyncClient`. Never call `asyncio.run()` in a module with async functions (exempt: `__main__.py`, `cli.py`, `scripts/`). Never use `threading.Lock()` in async code — use `asyncio.Lock()`.

If an `async def` route handler never awaits, make it a plain `def` — FastAPI runs sync handlers in a thread pool, which is safer than blocking the event loop.

When you have two or more independent awaits in sequence, use `asyncio.gather()`. The doctor flags consecutive awaited assignments where results don't depend on each other. It exempts calls on the same DB session (`AsyncSession` is not concurrency-safe) and side-effect calls (`.commit()`, `.save()`, `.delete()`).

## Every route is a contract

All `/api/` routes need a `response_model` (exempt: streaming, export, download, webhook, oauth). Creation POSTs return `status_code=201` (exempt: `/undo`, `/restore`, `/clone`). Every `/api/` endpoint gets a docstring and tags. Every OpenAPI operation needs a unique `operationId` — duplicates silently shadow. Never register the same method+path twice.

Route handlers stay under 100 lines — parse request, call service, return result. GET endpoints must not mutate — no `.add()`, `.delete()`, `.commit()`, `.update()`, `.save()`, no mutating SQL. This violates HTTP semantics.

## Pydantic v2 at trust boundaries

Use `@field_validator('field', mode='before')`, not the deprecated v1 `@validator('field', pre=True)`. Never use bare `= []`, `= {}`, or `= set()` as BaseModel field defaults — use `Field(default_factory=list)`. Request models in `routers/` and `interfaces/` must not use `extra="allow"` — use `extra="ignore"` or `extra="forbid"`.

At API boundaries (routers, interfaces, schemas, endpoints, api, views directories — or classes named `*Request`/`*Response`/`*Schema`/`*Payload`/`*Body`/`*Input`/`*Output`), use Pydantic BaseModel, not TypedDict or dataclass. Internal service code is free to use dataclasses. The doctor also flags functions returning dict literals with 7+ keys — make it a model.

Exempt: `@dataclass(slots=True)`, `@dataclass(frozen=True)`, `TypedDict(total=False)` (PATCH pattern), small NamedTuples ≤3 fields, `TYPE_CHECKING` blocks. Set `pydantic.should_be_model: "everywhere"` in `.fastapi-doctor.yml` to enforce Pydantic for all structured types.

## Exceptions are information, not noise

Never `except: pass` without a comment — add `logger.debug()` at minimum. Never catch `except Exception as e:` and ignore `e`. If you re-raise unchanged (`except X: raise`), remove the try/except — it's noise. If wrapping, use `raise NewError(...) from exc`. When logging inside `except Exception`, use `logger.exception()` or pass `exc_info=True` — `logger.warning("something")` without the traceback makes debugging impossible.

## Keep code small and modern

Functions over 200 lines get a warning; over 400 is an error. Files over 1500 lines need decomposition. Nesting deeper than 5 levels is unreadable — use early returns or extract helpers. Suppress with `# noqa: architecture`. Passthrough functions (`def f(a, b): return g(a, b)`) are unnecessary indirection — inline or add a docstring.

Config through settings objects, not `os.environ` in routers/services. `pathlib.Path` not `os.path`. `datetime.now(tz=timezone.utc)` not `utcnow()`. Builtin generics (`list`, `dict`, `X | None`) not `typing.List`. `logger` not `print()`. No `sys.exit()` from library code. No `from x import *` (exempt: `__init__.py`). Imports under 30 per file.

Never `return` inside `finally` — swallows exceptions. Never mutable defaults (`def f(x=[])`). Remove dead code after `return`/`raise`/`break`/`continue`.

## Performance and data access

Do not compile regex with literal patterns inside loops — hoist to module level: `PATTERN = re.compile("...")`. Do not put DB calls (`.query()`, `.execute()`, `.get()`, `.filter()`) inside loops — that's N+1; collect IDs first, then `Model.id.in_(ids)`.

## Alembic (if used)

Wire `target_metadata` to real metadata in `env.py`. Add `include_name`/`include_object` so autogenerate only touches your tables. Wire `process_revision_directives` to skip empty revisions. Set `MetaData(naming_convention=...)` for deterministic constraint names.
