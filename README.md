# python-doctor

`python-doctor` is an opinionated backend health checker for FastAPI and Python services.

It combines project discovery, route inspection, AST-based static checks, and optional external tooling into a single CLI that works against common repo layouts without assuming your package is named `app`.

## Features

- Auto-detects common layouts such as repo-root packages, `src/<pkg>`, and `backend/<pkg>`.
- Loads FastAPI apps from both `module:app` and `module:create_app()` entrypoints.
- Splits checks by concern: route/OpenAPI, architecture, correctness, security, resilience, performance, config, and Pydantic usage.
- Produces human-readable output for local use and JSON for CI pipelines.
- Keeps legacy `scripts/` entrypoints as thin compatibility wrappers.

## Install

Sync the project environment:

```bash
uv sync --extra dev
```

## Usage

Run inside the target repository:

```bash
uv run python-doctor
```

Scan another project explicitly:

```bash
uv run python-doctor --repo-root /path/to/project
```

Override discovery when the project layout is unusual:

```bash
uv run python-doctor \
  --repo-root /path/to/project \
  --import-root src \
  --code-dir src/my_backend \
  --app-module my_backend.api:create_app()
```

Machine-readable output:

```bash
uv run python-doctor --json
```

## Real Example

To verify the doctor against a clean public repo, use the maintained example script:

```bash
bash scripts/run_fastapi_template_example.sh --json
```

It clones [fastapi/full-stack-fastapi-template](https://github.com/fastapi/full-stack-fastapi-template) into `.examples/full-stack-fastapi-template`, exports the template's checked-in `.env`, and runs `python-doctor` against that checkout using the target repo's own `uv` environment. By default it skips `ruff` and `pyright` so the example focuses on doctor behavior rather than extra tool setup.

You can override the clone location with `PYTHON_DOCTOR_EXAMPLE_DIR=/path/to/clone`.

## Project Layout

```text
src/python_doctor/
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

`static_checks.py` exists as a compatibility shim. New code should import from the category modules directly.

## Development

Sync dependencies and run tests:

```bash
uv sync --extra dev
uv run pytest -q
```

The doctor is designed to run inside the target project's environment when importing the FastAPI app requires the target project's dependencies.
