# Mimic Rules

This file defines the exact Ruff/Ty constructs to mimic during the `fastapi-doctor` Rust refactor.

## Crate Root Rules

- Mimic `ruff_workspace/src/lib.rs`:
  - crate roots should mostly declare modules and re-export a narrow public API
  - avoid long implementation-heavy `lib.rs` files
- Mimic `ruff_linter/src/lib.rs`:
  - keep category/rule modules private where possible
  - expose only selection, registry, and public engine entrypoints
- Mimic `ty_project/src/lib.rs`:
  - crate root should expose the project model and a few core operations, not all implementation details

## Project Rules

- Mimic `Resolver` from Ruff:
  - use a dedicated resolver/metadata object instead of free functions with many args
- Mimic `ProjectFilesFilter` and `ProjectFilesWalker` from Ty:
  - use filter structs and walker structs instead of inline recursive directory traversal inside the boundary crate
- Prefer `ProjectMetadata` or equivalent typed config holder over loose tuples

## Parser / Facts Rules

- Mimic `ParsedModule` from Ruff DB:
  - wrap parsed modules in a dedicated type instead of passing raw ASTs through many layers
- Keep parsing behind a parser facade
- Shared source/index facts belong in `core`, not `rules`

## Rule Selection Rules

- Mimic `RuleSelector`:
  - parse rule selectors in Rust
  - keep rule inventory and rule parsing close together
- Mimic `registry`:
  - keep a canonical Rust-owned rule registry or enum instead of Python-owned string lists

## Suppression Rules

- Mimic Ruff suppression structs:
  - suppression parsing should produce structured types, not only ad hoc dictionaries
- keep suppression matching logic close to the rule engine, not in the Python runner

## Boundary Rules

- `fastapi_doctor_native` must stay thin
- `fastapi_doctor_project` must own project walking
- `fastapi_doctor_rules` must own static rule selection and orchestration
- `fastapi_doctor_core` must own reusable facts, parsing, routing, suppression primitives, and score aggregation primitives

## Local Target Mapping

- `src/fastapi_doctor/runner.py`
  - should consume one native project result for static mode
- `src/fastapi_doctor/native_core.py`
  - should become an adapter, not an orchestrator
- `rust/fastapi_doctor_project`
  - target `ProjectFilesFilter`, `ProjectFilesWalker`, `ProjectMetadata`, `LoadedProject`
- `rust/fastapi_doctor_rules`
  - target `StaticRule`, `RuleSelector`-style parsing, `analyze_module` orchestration
- `rust/fastapi_doctor_core`
  - target `ParsedModule`-style wrapper and thin crate root exports
