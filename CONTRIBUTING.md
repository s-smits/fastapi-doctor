# Contributing

## Setup

```bash
uv sync --extra dev
```

## Validation

```bash
uv run pytest -q
uv run python -m py_compile $(find src scripts tests -name '*.py')
```

## Design Notes

- Keep the CLI setup-agnostic. Avoid assuming a package is called `app` or that code lives only under `src/`.
- Add new checks to the narrowest category module under `src/python_doctor/checks/`.
- Keep public entry points documented via `uv run fastapi-doctor`, not ad hoc wrappers.
- When adding heuristics, prefer false-negative bias over noisy false positives.
