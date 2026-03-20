# fastapi-doctor

`fastapi-doctor` is an agent-first backend verifier for FastAPI and Python services.

Coding agents are good at producing local code and still unreliable at consistently enforcing backend patterns across a real repo. They will often get most of the implementation right and still miss one important thing: a missing auth dependency, blocking I/O inside an async handler, weak response model coverage, OpenAPI drift, or a Pydantic v1/v2 mismatch.

`fastapi-doctor` is the safety net for that gap.

Point it at a repo and it will discover the app layout, inspect routes, run AST-based checks, optionally run external tools, and return a scored report that an agent can act on without hardcoding repo structure.

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
- `commands` results for `ruff`, `pyright`, `bandit`, or `pytest` when enabled
- `doctor` findings with categorized issues, remediation fields, and ranked `next_actions`

## Common Agent Invocations

Run against the current repo:

```bash
uv run fastapi-doctor --json
```

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

It clones [fastapi/full-stack-fastapi-template](https://github.com/fastapi/full-stack-fastapi-template) into `.examples/full-stack-fastapi-template`, exports the template's checked-in `.env`, and runs `fastapi-doctor` against that checkout using the target repo's own `uv` environment. By default it skips `ruff` and `pyright` so the example focuses on doctor behavior rather than extra tool setup.

You can override the clone location with `FASTAPI_DOCTOR_EXAMPLE_DIR=/path/to/clone`.

## Internal Layout

```text
src/fastapi_doctor/
  app_loader.py
  cli.py
  external_tools.py
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
tests/
.github/workflows/
```

`static_checks.py` re-exports checks from the category modules. New code should import from the category modules directly.

## Development

Sync dependencies and run tests:

```bash
uv sync --extra dev
uv run pytest -q
```

The doctor is designed to run inside the target project's environment when importing the FastAPI app requires the target project's dependencies.
