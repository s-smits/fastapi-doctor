from __future__ import annotations

"""Pydantic and schema-shape static checks."""

import ast
import re
from pathlib import Path

from .. import project
from ..models import DoctorIssue

def check_should_be_pydantic_model() -> list[DoctorIssue]:
    """Detect TypedDict, NamedTuple, dataclass, and dict-factory patterns that
    should be Pydantic models.

    Two modes (set via ``.fastapi-doctor.yml`` → ``pydantic.should_be_model``):

    **"boundary"** (default) — Trust-boundary analysis.  Pydantic belongs at
    API edges where untrusted data enters.  Internal code is free to use
    dataclasses, TypedDicts, and NamedTuples.  Only flags patterns at API
    boundaries (routers, interfaces, schemas) or with API-suggestive names.

    **"everywhere"** — Consistency-first.  Some teams prefer Pydantic for
    *all* structured data (internal too) for uniform serialization, IDE
    support, and fewer mental-model switches.  This mode flags all
    TypedDict/NamedTuple/dataclass regardless of location.

    Both modes share smart exemptions:
    - Classes in ``TYPE_CHECKING`` blocks (type-only)
    - ``@dataclass(slots=True)`` — performance-optimized internal types
    - ``@dataclass(frozen=True)`` — immutable value objects (legitimate pattern)
    - Classes with ``__slots__`` (same as slots=True)
    - ``TypedDict`` with ``total=False`` (partial update / PATCH pattern)
    - Small ``NamedTuple`` (≤3 fields) — lightweight value objects, cache keys
    - The doctor's own internal modules
    """
    # ── Trust boundary heuristics ────────────────────────────────────────────
    _API_BOUNDARY_DIRS = frozenset({
        "routers", "router", "interfaces", "interface",
        "schemas", "schema", "endpoints", "endpoint",
        "api", "views",
    })
    _INTERNAL_DIRS = frozenset({
        "services", "service", "utils", "util", "helpers", "helper",
        "internal", "core", "domain", "agents", "agent",
        "state", "workflows", "workflow", "lib", "scripts", "script",
        "tests", "test", "migrations", "middleware",
    })
    _API_NAME_RE = re.compile(
        r"(Request|Response|Schema|Payload|Body|Input|Output)$"
    )
    _NAMEDTUPLE_VALUE_OBJECT_RE = re.compile(
        r"(Key|Pair|Coord|Point|Range|Spec|Entry|Record|Tuple|Result|Token|Slot)$"
    )

    def _path_parts(rel_path: str) -> tuple[str, ...]:
        return Path(rel_path).parts

    def _is_api_boundary(rel_path: str) -> bool:
        return any(p.lower() in _API_BOUNDARY_DIRS for p in _path_parts(rel_path))

    def _is_internal(rel_path: str) -> bool:
        return any(p.lower() in _INTERNAL_DIRS for p in _path_parts(rel_path))

    def _has_api_name(name: str) -> bool:
        return bool(_API_NAME_RE.search(name))

    everywhere = project.SHOULD_BE_MODEL_MODE == "everywhere"

    def _should_flag(rel_path: str, class_name: str) -> bool:
        """Decide whether a non-Pydantic type should be flagged.

        In "boundary" mode: flag at API boundaries or with API-suggestive names.
        In "everywhere" mode: flag everything (team prefers Pydantic consistency).
        """
        if everywhere:
            return True
        if _is_api_boundary(rel_path):
            return True
        if _has_api_name(class_name):
            return True
        return False

    def _count_fields(node: ast.ClassDef) -> int:
        return sum(1 for stmt in node.body if isinstance(stmt, ast.AnnAssign))

    def _has_decorator_kwarg(node: ast.ClassDef, kwarg: str) -> bool:
        """Check if @dataclass(..., kwarg=True)."""
        for dec in node.decorator_list:
            if isinstance(dec, ast.Call):
                for kw in dec.keywords:
                    if (
                        kw.arg == kwarg
                        and isinstance(kw.value, ast.Constant)
                        and kw.value.value is True
                    ):
                        return True
        return False

    def _has_total_false(node: ast.ClassDef) -> bool:
        """Check if TypedDict has total=False (partial update pattern)."""
        for base in node.bases:
            if isinstance(base, ast.Call):
                for kw in base.keywords:
                    if (
                        kw.arg == "total"
                        and isinstance(kw.value, ast.Constant)
                        and kw.value.value is False
                    ):
                        return True
        # Also check via class keyword syntax: class Foo(TypedDict, total=False)
        for kw in node.keywords:
            if (
                kw.arg == "total"
                and isinstance(kw.value, ast.Constant)
                and kw.value.value is False
            ):
                return True
        return False

    # ── Main detection loop ──────────────────────────────────────────────────
    issues: list[DoctorIssue] = []
    own_module_name = Path(__file__).stem
    for module in project.parsed_python_modules():
        rel_path = module.rel_path
        # Skip our own checks module
        if module.path.stem == own_module_name:
            continue

        at_boundary = _is_api_boundary(rel_path)
        is_internal = _is_internal(rel_path)

        # Collect classes inside TYPE_CHECKING blocks
        in_type_checking_names: set[str] = set()
        if "TYPE_CHECKING" in module.source:
            for node in ast.walk(module.tree):
                if isinstance(node, ast.If):
                    test = node.test
                    if isinstance(test, ast.Name) and test.id == "TYPE_CHECKING":
                        for child in ast.walk(node):
                            if isinstance(child, ast.ClassDef):
                                in_type_checking_names.add(child.name)

        if not any(kw in module.source for kw in ("class ", "@dataclass", "return {")):
            continue

        for node in ast.walk(module.tree):
            if not isinstance(node, ast.ClassDef):
                continue
            if node.name in in_type_checking_names:
                continue

            # ── 1. TypedDict ─────────────────────────────────────────────
            is_typed_dict = any(
                (isinstance(base, ast.Name) and base.id == "TypedDict")
                or (isinstance(base, ast.Attribute) and base.attr == "TypedDict")
                for base in node.bases
            )
            if is_typed_dict:
                # total=False TypedDicts model partial updates (PATCH bodies,
                # optional kwargs) — a pattern Pydantic handles less naturally
                if _has_total_false(node):
                    continue
                if _should_flag(rel_path, node.name):
                    where = "at API boundary " if at_boundary else ""
                    issues.append(
                        DoctorIssue(
                            check="pydantic/should-be-model",
                            severity="warning",
                            message=f"TypedDict '{node.name}' {where}should be a Pydantic BaseModel",
                            path=rel_path,
                            category="Pydantic",
                            help=(
                                "TypedDicts provide no runtime validation. BaseModel gives you "
                                "validation, serialization, and OpenAPI schema. Use TypedDict "
                                "with total=False for partial-update / PATCH patterns."
                            ),
                            line=node.lineno,
                        )
                    )
                continue

            # ── 2. NamedTuple ────────────────────────────────────────────
            is_named_tuple = any(
                (isinstance(base, ast.Name) and base.id == "NamedTuple")
                or (isinstance(base, ast.Attribute) and base.attr == "NamedTuple")
                for base in node.bases
            )
            if is_named_tuple:
                field_count = _count_fields(node)
                # Small NamedTuples (≤3 fields) are lightweight value objects,
                # cache keys, coordinate pairs — perfectly fine as-is
                if field_count <= 3 and not _has_api_name(node.name):
                    continue
                # Value-object names (Key, Pair, Coord, etc.) are legitimate
                if _NAMEDTUPLE_VALUE_OBJECT_RE.search(node.name) and not at_boundary:
                    continue
                if _should_flag(rel_path, node.name):
                    where = "at API boundary " if at_boundary else ""
                    issues.append(
                        DoctorIssue(
                            check="pydantic/should-be-model",
                            severity="warning",
                            message=f"NamedTuple '{node.name}' {where}should be a Pydantic BaseModel with frozen=True",
                            path=rel_path,
                            category="Pydantic",
                            help=(
                                "BaseModel(frozen=True) provides the same immutability plus "
                                "validation and OpenAPI support. Small NamedTuples (≤3 fields) "
                                "used as value objects or cache keys are exempt."
                            ),
                            line=node.lineno,
                        )
                    )
                continue

            # ── 3. @dataclass ────────────────────────────────────────────
            is_dataclass = any(
                (isinstance(dec, ast.Name) and dec.id == "dataclass")
                or (isinstance(dec, ast.Attribute) and dec.attr == "dataclass")
                or (
                    isinstance(dec, ast.Call)
                    and (
                        (isinstance(dec.func, ast.Name) and dec.func.id == "dataclass")
                        or (
                            isinstance(dec.func, ast.Attribute)
                            and dec.func.attr == "dataclass"
                        )
                    )
                )
                for dec in node.decorator_list
            )
            if is_dataclass:
                # Performance-optimized: slots=True
                has_slots = _has_decorator_kwarg(node, "slots")
                if not has_slots:
                    has_slots = any(
                        isinstance(stmt, ast.AnnAssign)
                        and isinstance(stmt.target, ast.Name)
                        and stmt.target.id == "__slots__"
                        for stmt in node.body
                    )
                if has_slots:
                    continue

                # Immutable value objects: frozen=True (legitimate pattern —
                # hashable, safe to use as dict keys, no validation needed)
                if _has_decorator_kwarg(node, "frozen"):
                    continue

                # In "boundary" mode: skip internal code — dataclasses are fine
                # for trusted data.  In "everywhere" mode: flag it.
                if not everywhere and is_internal:
                    continue

                # At API boundary, API-suggestive name, or "everywhere" mode → flag
                if _should_flag(rel_path, node.name):
                    where = "at API boundary " if at_boundary else ""
                    issues.append(
                        DoctorIssue(
                            check="pydantic/should-be-model",
                            severity="warning",
                            message=f"@dataclass '{node.name}' {where}should be a Pydantic BaseModel",
                            path=rel_path,
                            category="Pydantic",
                            help=(
                                "Pydantic provides validation, serialization, and OpenAPI schema "
                                "generation. Use @dataclass(slots=True) or @dataclass(frozen=True) "
                                "to exempt performance-critical or immutable value types."
                            ),
                            line=node.lineno,
                        )
                    )

        # ── 4. Dict-factory functions ────────────────────────────────────
        # Functions returning dict literals with 7+ hardcoded keys are proto-models.
        # At API boundaries these are stronger signals; internal ones are softer.
        _SERIALIZER_METHOD_NAMES = frozenset(
            {"to_dict", "to_payload", "as_dict", "serialize"}
        )
        for node in ast.walk(module.tree):
            if not isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                continue
            # Skip private/internal functions
            if node.name.startswith("_"):
                continue
            # Skip serialization methods — they convert FROM structured data TO dict
            if node.name in _SERIALIZER_METHOD_NAMES:
                continue
            # Check return annotation for dict
            ret = node.returns
            if ret is None:
                continue
            is_dict_return = False
            if isinstance(ret, ast.Subscript):
                if isinstance(ret.value, ast.Name) and ret.value.id == "dict":
                    is_dict_return = True
            if not is_dict_return:
                continue
            # Check if the function builds and returns a dict literal with >6 keys
            # (7+ keys = strong signal this is a proto-model, not just a quick mapping)
            for child in ast.walk(node):
                if isinstance(child, ast.Return) and child.value is not None:
                    if isinstance(child.value, ast.Dict) and len(child.value.keys) > 6:
                        issues.append(
                            DoctorIssue(
                                check="pydantic/should-be-model",
                                severity="warning",
                                message=f"Function '{node.name}' returns a dict literal with {len(child.value.keys)} keys — consider a Pydantic model",
                                path=rel_path,
                                category="Pydantic",
                                help=(
                                    "Define a BaseModel with typed fields instead of returning raw dicts. "
                                    "You get validation, serialization, and IDE support. "
                                    "At API boundaries this is strongly recommended; "
                                    "for internal helpers, a dataclass is also acceptable."
                                ),
                                line=child.lineno,
                            )
                        )
                        break  # One flag per function
    return issues

def check_deprecated_validators() -> list[DoctorIssue]:
    """@validator is Pydantic v1 — use @field_validator (v2) for forward compat."""
    issues: list[DoctorIssue] = []
    # Match @validator(...) but not @field_validator(...)
    pattern = re.compile(r"@validator\(")
    for module in project.parsed_python_modules():
        if "@validator" not in module.source:
            continue
        lines = module.source.splitlines()
        for i, line in enumerate(lines, 1):
            stripped = line.strip()
            if pattern.search(stripped):
                issues.append(
                    DoctorIssue(
                        check="pydantic/deprecated-validator",
                        severity="error",
                        message="@validator is deprecated (Pydantic v1) — use @field_validator",
                        path=module.rel_path,
                        category="Pydantic",
                        help="Replace @validator('field', pre=True) with @field_validator('field', mode='before').",
                        line=i,
                    )
                )
    return issues

def check_mutable_model_defaults() -> list[DoctorIssue]:
    """Pydantic models with bare mutable defaults (list, dict, set) cause shared-state bugs."""
    issues: list[DoctorIssue] = []
    mutable_defaults = re.compile(r":\s*(?:list|dict|set)\s*(?:\[.*?\])?\s*=\s*(?:\[\]|\{\}|set\(\))")
    for module in project.parsed_python_modules():
        if not any(kw in module.source for kw in ("BaseModel", "list", "dict", "set")):
            continue
        lines = module.source.splitlines()
        for node in ast.walk(module.tree):
            if not isinstance(node, ast.ClassDef):
                continue
            is_model = any(
                (isinstance(base, ast.Name) and base.id == "BaseModel")
                or (isinstance(base, ast.Attribute) and base.attr == "BaseModel")
                for base in node.bases
            )
            if not is_model:
                continue
            for stmt in node.body:
                if isinstance(stmt, ast.AnnAssign) and stmt.value is not None:
                    line_text = lines[stmt.lineno - 1] if stmt.lineno <= len(lines) else ""
                    if mutable_defaults.search(line_text):
                        issues.append(
                            DoctorIssue(
                                check="pydantic/mutable-default",
                                severity="error",
                                message=f"Mutable default in model '{node.name}' — use Field(default_factory=...)",
                                path=module.rel_path,
                                category="Pydantic",
                                help="Replace `field: list[X] = []` with `field: list[X] = Field(default_factory=list)`.",
                                line=stmt.lineno,
                            )
                        )
    return issues

def check_extra_allow_on_request_models() -> list[DoctorIssue]:
    """Request models with extra='allow' accept arbitrary user input — security/data integrity risk.

    In FastAPI, request body models with extra="allow" let clients send any fields they want,
    which can leak into DB operations, logs, or downstream services. Response models and internal
    schemas (scripts/, agents/tools/) are exempt since they don't face user input directly.
    """
    issues: list[DoctorIssue] = []
    # Only check router/interface code where request models live
    check_dirs = {"routers", "interfaces"}
    for module in project.parsed_python_modules():
        if 'extra="allow"' not in module.source and "extra='allow'" not in module.source:
            continue
        parts = module.path.relative_to(project.OWN_CODE_DIR).parts
        if not parts or parts[0] not in check_dirs:
            continue
        lines = module.source.splitlines()
        for i, line in enumerate(lines, 1):
            if 'extra="allow"' in line or "extra='allow'" in line:
                issues.append(
                    DoctorIssue(
                        check="pydantic/extra-allow-on-request",
                        severity="warning",
                        message="Model in request path uses extra='allow' — accepts arbitrary user input",
                        path=module.rel_path,
                        category="Pydantic",
                        help="Use extra='ignore' (drop unknown fields) or extra='forbid' (reject them).",
                        line=i,
                    )
                )
    return issues


def check_sensitive_fields_in_models() -> list[DoctorIssue]:
    """Detect sensitive fields in Pydantic models that aren't using SecretStr.

    Fields like api_key, password, token, and secret should use Pydantic's
    SecretStr type to prevent accidental leakage in logs or JSON responses.
    """
    sensitive_names = re.compile(
        r"(?:api_?key|password|secret|auth_?token|credential|private_?key)",
        re.IGNORECASE,
    )
    issues: list[DoctorIssue] = []

    for module in project.parsed_python_modules():
        if "BaseModel" not in module.source:
            continue

        for node in ast.walk(module.tree):
            if not isinstance(node, ast.ClassDef):
                continue

            is_model = any(
                (isinstance(base, ast.Name) and base.id == "BaseModel")
                or (isinstance(base, ast.Attribute) and base.attr == "BaseModel")
                for base in node.bases
            )
            if not is_model:
                continue

            for stmt in node.body:
                if isinstance(stmt, ast.AnnAssign) and isinstance(stmt.target, ast.Name):
                    field_name = stmt.target.id
                    if sensitive_names.search(field_name):
                        # Check if the type is SecretStr or similar
                        type_str = ast.dump(stmt.annotation)
                        if "SecretStr" not in type_str:
                            issues.append(
                                DoctorIssue(
                                    check="pydantic/sensitive-field-type",
                                    severity="warning",
                                    message=f"Sensitive field '{field_name}' in model '{node.name}' should use SecretStr",
                                    path=module.rel_path,
                                    category="Pydantic",
                                    help="Use pydantic.SecretStr to prevent accidental leakage in logs or JSON.",
                                    line=stmt.lineno,
                                )
                            )
    return issues


__all__ = [
    "check_deprecated_validators",
    "check_extra_allow_on_request_models",
    "check_mutable_model_defaults",
    "check_sensitive_fields_in_models",
    "check_should_be_pydantic_model",
]
