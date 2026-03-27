# fastapi-doctor

`fastapi-doctor` is a Rust-native backend doctor for FastAPI and Python codebases. The analysis engine, project discovery, and rule execution live in the Rust workspace under `rust/`; the Python package is now a minimal bootstrap layer so the tool can still be installed with `uv tool install` and run with `uvx`.

## Install

```bash
uv tool install --index https://s-smits.github.io/fastapi-doctor/simple/ fastapi-doctor
```

```bash
uvx --index https://s-smits.github.io/fastapi-doctor/simple/ fastapi-doctor --version
```

If you want a local checkout:

```bash
git clone https://github.com/s-smits/fastapi-doctor.git
cd fastapi-doctor
uv sync --extra dev
```

Run it against a project:

```bash
uv run fastapi-doctor --profile strict --repo-root /path/to/your/project
```

## What It Checks

- Security boundaries
- Correctness and resilience
- Architecture and API surface quality
- Pydantic usage at trust boundaries
- Optional Ruff, ty, Bandit, and pytest signals

## Common Invocations

```bash
uv run fastapi-doctor --json
uv run fastapi-doctor --json --profile security
uv run fastapi-doctor --json --with-bandit --with-tests
uv run fastapi-doctor --score
```

## Configuration

The tool reads project config from `.fastapi-doctor.yml`. The example file at [`.fastapi-doctor.example.yml`](./.fastapi-doctor.example.yml) shows the current schema.

## Repository Layout

- `rust/`: Rust workspace for discovery, rules, and the native extension
- `src/fastapi_doctor/`: minimal Python wrapper for packaging and CLI dispatch
- `tests/`: Python-side smoke tests around the native bridge
- `.github/workflows/`: release and package index automation

## Development

```bash
uv run pytest -q
PYO3_PYTHON="$PWD/.venv/bin/python" cargo test --manifest-path rust/Cargo.toml
```

```bash
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 uv build
```

## Release Artifacts

Tagged releases publish wheels and an sdist, which keeps `uv tool install` working from the published simple index.
