# Contributing

## Setup

```bash
pip install -e .[dev]
```

## Validation

```bash
pytest -q
python -m py_compile $(find src scripts tests -name '*.py')
```

## Design Notes

- Keep the CLI setup-agnostic. Avoid assuming a package is called `app` or that code lives only under `src/`.
- Add new checks to the narrowest category module under `src/python_doctor/checks/`.
- Preserve the `scripts/` wrappers until downstream users no longer rely on them.
- When adding heuristics, prefer false-negative bias over noisy false positives.
