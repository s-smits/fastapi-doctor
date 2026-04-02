---
name: astral-rust-refactor
description: Use when refactoring fastapi-doctor toward a Ruff/Ty-style Rust-first architecture. This skill defines the exact crate boundaries, function shapes, and reference files to mimic for project walking, parsed-module management, rule selection, suppression handling, and thin crate roots.
---

# Astral Rust Refactor

Use this skill when changing the Rust architecture in the current `fastapi-doctor` workspace.

## Goal

Refactor `fastapi-doctor` toward these constraints:
- crate roots stay thin and mostly re-export modules
- project walking lives in a `project` crate with filter/walker types
- parsing lives behind a core parser facade, not inline in the PyO3 bridge
- rule inventory and rule selection live in a `rules` crate
- suppression handling is explicit and structured, not ad hoc string matching in runners
- the PyO3 crate is only a boundary adapter

## Read Order

Read [MIMIC_RULES.md](references/MIMIC_RULES.md) first.

Then load only the relevant Rust references:
- workspace root / thin crate export shape:
  [REFERENCE_1_workspace_lib.rs](references/REFERENCE_1_workspace_lib.rs)
- workspace resolver / scoped settings resolution:
  [REFERENCE_2_workspace_resolver.rs](references/REFERENCE_2_workspace_resolver.rs)
- parsed-module wrapper and parser facade shape:
  [REFERENCE_3_parsed_module.rs](references/REFERENCE_3_parsed_module.rs)
- project file filter and walker shape:
  [REFERENCE_4_project_walk.rs](references/REFERENCE_4_project_walk.rs)
- project model and rules access shape:
  [REFERENCE_5_project_core.rs](references/REFERENCE_5_project_core.rs)
- rule selector and selection parsing shape:
  [REFERENCE_6_rule_selector.rs](references/REFERENCE_6_rule_selector.rs)
- structured suppression handling shape:
  [REFERENCE_7_suppression.rs](references/REFERENCE_7_suppression.rs)

## Hard Rules

- Do not put project walking in the PyO3 crate.
- Do not put rule selection parsing in Python.
- Do not keep a giant catch-all `lib.rs` when a crate root can re-export modules instead.
- Do not parse the same module repeatedly for unrelated rules when a shared parsed/fact representation can be reused.
- Do not let `runner.py` own the static rule matrix once Rust can own that contract.
- Keep live FastAPI boot and OpenAPI/runtime-only checks in Python unless the user asks to move them.

## Fastapi-doctor Mapping

- `rust/fastapi_doctor_core`
  - mimic `ruff_db::parsed` and thin crate roots
  - own module records, parser facade, source indexing, route/suppression/scoring primitives
- `rust/fastapi_doctor_project`
  - mimic `ty_project::walk` and `ruff_workspace::resolver`
  - own metadata, file filters, file walking, project loading
- `rust/fastapi_doctor_rules`
  - mimic `ruff_linter::rule_selector` and `registry`
  - own static rule ids, selector parsing, rule engine orchestration
- `rust/fastapi_doctor_native`
  - thin boundary only
  - convert native structs to Python tuples/dicts/objects

## Execution Pattern

1. Start by updating the reference-backed module boundaries first.
2. Introduce typed Rust structs before wiring Python changes.
3. Cut Python over only after the native result type is stable.
4. Keep the existing CLI and install flow unchanged while moving internals.
