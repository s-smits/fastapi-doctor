from __future__ import annotations

"""Security-focused static checks."""

import ast
import re

from .. import project
from ..models import DoctorIssue

def check_unsafe_hash_usage() -> list[DoctorIssue]:
    """SHA1/MD5 without usedforsecurity=False gets flagged by Bandit as high severity."""
    issues: list[DoctorIssue] = []
    pattern = re.compile(r"\b(sha1|md5)\(.*\)\.hexdigest\(\)")
    safe_pattern = re.compile(r"usedforsecurity\s*=\s*False")
    for module in project.parsed_python_modules():
        if "sha1" not in module.source and "md5" not in module.source:
            continue
        lines = module.source.splitlines()
        for i, line in enumerate(lines, 1):
            if pattern.search(line) and not safe_pattern.search(line) and "nosec" not in line:
                issues.append(
                    DoctorIssue(
                        check="security/weak-hash-without-flag",
                        severity="error",
                        message="SHA1/MD5 used without usedforsecurity=False",
                        path=module.rel_path,
                        category="Security",
                        help="Add usedforsecurity=False to signal this is not for security purposes.",
                        line=i,
                    )
                )
    return issues

def check_unsafe_yaml_load() -> list[DoctorIssue]:
    """yaml.load() without SafeLoader/BaseLoader is arbitrary code execution."""
    issues: list[DoctorIssue] = []
    for module in project.parsed_python_modules():
        if "yaml.load(" not in module.source:
            continue
        lines = module.source.splitlines()
        for i, line in enumerate(lines, 1):
            if "yaml.load(" in line and "nosec" not in line:
                # Check it uses a safe loader
                if not re.search(r"Loader\s*=\s*yaml\.(SafeLoader|BaseLoader|CSafeLoader)", line):
                    issues.append(
                        DoctorIssue(
                            check="security/unsafe-yaml-load",
                            severity="error",
                            message="yaml.load() without SafeLoader/BaseLoader allows arbitrary code execution",
                            path=module.rel_path,
                            category="Security",
                            help="Use yaml.safe_load() or pass Loader=yaml.SafeLoader.",
                            line=i,
                        )
                    )
    return issues

def check_sql_fstring_interpolation() -> list[DoctorIssue]:
    """Detect f-string interpolation inside SQLAlchemy text() calls.

    Using ``text(f"SELECT ... WHERE id = {user_id}")`` is a SQL injection risk.
    Use parameterized queries instead. Plain multiline SQL strings are fine; this
    rule only forbids interpolating values into the SQL text itself.

    Honors ``# noqa: sql-safe`` and ``# noqa: security`` pragmas on the text() line
    for cases where f-string fragments are internally generated (e.g. dynamic column
    lists, conditional WHERE clauses built from code, not user input).
    """
    _NOQA_SQL = re.compile(r"#\s*noqa\s*:\s*(sql-safe|security)", re.IGNORECASE)
    issues: list[DoctorIssue] = []
    for module in project.parsed_python_modules():
        if "text(" not in module.source:
            continue
        lines = module.source.splitlines()

        for node in ast.walk(module.tree):
            if not isinstance(node, ast.Call):
                continue
            func = node.func
            if (isinstance(func, ast.Name) and func.id == "text") or (
                isinstance(func, ast.Attribute) and func.attr == "text"
            ):
                if node.args and isinstance(node.args[0], ast.JoinedStr):
                    # Check for noqa pragma on the text() line or the f-string line
                    lineno = node.lineno
                    suppressed = False
                    for check_line in range(max(0, lineno - 1), min(len(lines), lineno + 1)):
                        if _NOQA_SQL.search(lines[check_line]):
                            suppressed = True
                            break
                    if suppressed:
                        continue
                    issues.append(
                        DoctorIssue(
                            check="security/sql-fstring-interpolation",
                            severity="error",
                            message="SQL injection risk: f-string used inside text() call",
                            path=module.rel_path,
                            category="Security",
                            help="Keep SQL parameterized instead of interpolating values into text(). Plain multiline SQL strings are acceptable. Suppress with '# noqa: sql-safe' if a trusted internal SQL fragment must be assembled dynamically.",
                            line=lineno,
                        )
                    )
    return issues

def check_exception_detail_leak() -> list[DoctorIssue]:
    """Detect potential internal error leakage in exception details."""
    issues: list[DoctorIssue] = []
    # Simplified check for HTTPException(..., detail=str(exc))
    for module in project.parsed_python_modules():
        if "HTTPException" not in module.source:
            continue
        for node in ast.walk(module.tree):
            if not isinstance(node, ast.Call):
                continue

            func = node.func
            is_http_exc = (isinstance(func, ast.Name) and func.id == "HTTPException") or (
                isinstance(func, ast.Attribute) and func.attr == "HTTPException"
            )
            if not is_http_exc:
                continue

            for kw in node.keywords:
                if kw.arg == "detail":
                    if isinstance(kw.value, ast.Call):
                        # detail=str(e) or detail=f"Error: {e}"
                        if (isinstance(kw.value.func, ast.Name) and kw.value.func.id == "str") or isinstance(
                            kw.value, ast.JoinedStr
                        ):
                            issues.append(
                                DoctorIssue(
                                    check="security/exception-detail-leak",
                                    severity="warning",
                                    message="Potential internal error leak in HTTPException detail",
                                    path=module.rel_path,
                                    category="Security",
                                    help="Use a generic error message. Log the real exception with logger.exception().",
                                    line=node.lineno,
                                )
                            )
    return issues


def check_assert_in_production() -> list[DoctorIssue]:
    issues: list[DoctorIssue] = []
    for module in project.parsed_python_modules():
        if "tests/" in str(module.path) or "alembic/" in str(module.path):
            continue
        if "assert " not in module.source and "assert(" not in module.source:
            continue
        for node in ast.walk(module.tree):
            if isinstance(node, ast.Assert):
                issues.append(DoctorIssue(
                    check="security/assert-in-production",
                    severity="error",
                    message="assert statement outside tests — use explicit exception raises",
                    path=module.rel_path,
                    category="Security",
                    help="Asserts are ignored when Python runs with -O. Raise ValueError or custom exceptions instead. Do not wrap in 'if condition:' without raising, as that silently skips the check.",
                    line=node.lineno
                ))
    return issues

def check_shell_true() -> list[DoctorIssue]:
    issues: list[DoctorIssue] = []
    for module in project.parsed_python_modules():
        if "subprocess" not in module.source:
            continue
        for node in ast.walk(module.tree):
            if isinstance(node, ast.Call):
                if isinstance(node.func, ast.Attribute) and node.func.attr in ("Popen", "run", "call", "check_call", "check_output"):
                    if isinstance(node.func.value, ast.Name) and node.func.value.id == "subprocess":
                        for kw in node.keywords:
                            if kw.arg == "shell" and isinstance(kw.value, ast.Constant) and getattr(kw.value, "value", None) is True:
                                issues.append(DoctorIssue(
                                    check="security/subprocess-shell-true",
                                    severity="error",
                                    message="subprocess executed with shell=True — potential shell injection",
                                    path=module.rel_path,
                                    category="Security",
                                    help="Pass arguments as a list and remove shell=True to avoid injection risks.",
                                    line=node.lineno
                                ))
    return issues


def _value_looks_like_identifier_or_label(val: str) -> bool:
    """Return True if value looks like a name, label, or enum — not a real secret."""
    # snake_case or UPPER_SNAKE identifiers (e.g. "oauth_token", "API_KEY_HEADER")
    if re.fullmatch(r"[a-zA-Z_][a-zA-Z0-9_]*", val):
        return True
    # kebab-case identifiers (e.g. "auth-token", "api-key")
    if re.fullmatch(r"[a-zA-Z][a-zA-Z0-9-]*", val):
        return True
    return False


def _value_looks_like_placeholder(val: str) -> bool:
    """Return True if value is a test/placeholder/encrypted pattern."""
    return bool(
        re.search(
            r"(?:fake|test|example|dummy|mock|placeholder|encrypted|sample|your[-_]|change[-_]?me)",
            val,
            re.IGNORECASE,
        )
    )


def _value_looks_like_real_secret(val: str) -> bool:
    """Return True if the string value has characteristics of a real secret.

    Real secrets tend to have high entropy: mixed case, digits, and special
    characters.  Plain identifiers, labels, HTTP headers, and URL-like values
    are excluded.
    """
    if _value_looks_like_identifier_or_label(val):
        return False
    if _value_looks_like_placeholder(val):
        return False
    # URL-like / protocol prefixes are not secrets
    if val.startswith(("http://", "https://", "ws://", "wss://", "ftp://")):
        return False
    # Check character-class entropy
    has_upper = bool(re.search(r"[A-Z]", val))
    has_lower = bool(re.search(r"[a-z]", val))
    has_digit = bool(re.search(r"[0-9]", val))
    has_special = bool(re.search(r"[^a-zA-Z0-9_\-]", val))
    char_classes = sum([has_upper, has_lower, has_digit, has_special])
    # Real secrets typically mix 3+ character classes, or are long with 2+
    return char_classes >= 3 or (char_classes >= 2 and len(val) >= 24)


def check_hardcoded_secrets() -> list[DoctorIssue]:
    """Detect hardcoded API keys, tokens, and passwords in source code.

    Inspired by react-doctor's ``no-secrets-in-client-code`` rule. Checks:
    1. String literals matching known secret patterns (Stripe, AWS, GitHub, OpenAI, etc.)
    2. Variable assignments where the name suggests a secret **and** the value
       looks like a real secret (not a label, identifier, or placeholder).

    Skips test files, config defaults, and environment variable fallback patterns.
    """
    from ..suppression import is_suppressed

    _SECRET_PATTERNS: list[re.Pattern[str]] = [
        re.compile(r"^sk_live_"),              # Stripe live key
        re.compile(r"^sk_test_"),              # Stripe test key
        re.compile(r"^AKIA[0-9A-Z]{16}$"),    # AWS access key
        re.compile(r"^ghp_[a-zA-Z0-9]{36}$"), # GitHub PAT
        re.compile(r"^github_pat_"),           # GitHub fine-grained PAT
        re.compile(r"^glpat-"),                # GitLab PAT
        re.compile(r"^xox[bporas]-"),          # Slack tokens
        re.compile(r"^sk-[a-zA-Z0-9]{32,}$"), # OpenAI key
        re.compile(r"^eyJ[a-zA-Z0-9_-]{20,}\.eyJ"),  # JWT token
        re.compile(r"^Bearer\s+[A-Za-z0-9\-._~+/]+=*$"),  # Bearer token
    ]
    _SECRET_VAR_PATTERN = re.compile(r"(?:api_?key|secret_?key|auth_?token|password|credential|private_?key)", re.IGNORECASE)
    _FALSE_POSITIVE_VALUES = frozenset({
        "", "changeme", "xxx", "your-api-key", "CHANGE_ME", "TODO",
        "placeholder", "test", "dummy", "fake", "mock", "example",
        "none", "null", "undefined", "n/a", "na",
    })

    issues: list[DoctorIssue] = []
    for module in project.parsed_python_modules():
        if "tests/" in str(module.path) or "test_" in module.path.name:
            continue
        lines = module.source.splitlines()

        for node in ast.walk(module.tree):
            if not isinstance(node, ast.Assign):
                continue
            if not isinstance(node.value, ast.Constant) or not isinstance(node.value.value, str):
                continue
            val = node.value.value
            if not val or val.lower() in _FALSE_POSITIVE_VALUES or len(val) < 8:
                continue
            if node.lineno <= len(lines) and is_suppressed(lines[node.lineno - 1], "security/hardcoded-secret"):
                continue

            # Check 1: Does the value match a known secret pattern?
            value_match = any(p.search(val) for p in _SECRET_PATTERNS)
            if value_match:
                issues.append(DoctorIssue(
                    check="security/hardcoded-secret",
                    severity="error",
                    message="Hardcoded secret detected — use environment variables or a secrets manager",
                    path=module.rel_path,
                    category="Security",
                    help="Move secrets to environment variables: os.environ['KEY'] or a secrets manager like AWS SM / Vault.",
                    line=node.lineno,
                ))
                continue

            # Check 2: Variable name suggests a secret — require value evidence.
            # Plain identifiers, labels, enum members, and placeholders are NOT flagged.
            for target in node.targets:
                name = ""
                if isinstance(target, ast.Name):
                    name = target.id
                elif isinstance(target, ast.Attribute):
                    name = target.attr
                if name and _SECRET_VAR_PATTERN.search(name) and _value_looks_like_real_secret(val):
                    issues.append(DoctorIssue(
                        check="security/hardcoded-secret",
                        severity="error",
                        message=f"Variable '{name}' looks like a secret with a hardcoded string value",
                        path=module.rel_path,
                        category="Security",
                        help="Move secrets to environment variables or a secrets manager. Never commit real credentials.",
                        line=node.lineno,
                    ))
                    break
    return issues

def check_cors_wildcard() -> list[DoctorIssue]:
    """Detect CORSMiddleware configured with allow_origins=['*'].

    A wildcard origin policy means any website can make authenticated requests
    to your API. This is almost never correct for production APIs that use
    cookies or tokens. Specify explicit allowed origins instead.
    """
    issues: list[DoctorIssue] = []
    for module in project.parsed_python_modules():
        if "CORSMiddleware" not in module.source:
            continue
        lines = module.source.splitlines()

        for node in ast.walk(module.tree):
            if not isinstance(node, ast.Call):
                continue
            # Match: app.add_middleware(CORSMiddleware, allow_origins=["*"])
            # or CORSMiddleware(..., allow_origins=["*"])
            func = node.func
            is_cors = False
            if isinstance(func, ast.Name) and func.id == "CORSMiddleware":
                is_cors = True
            elif isinstance(func, ast.Attribute) and func.attr == "add_middleware":
                if node.args and isinstance(node.args[0], ast.Name) and node.args[0].id == "CORSMiddleware":
                    is_cors = True

            if not is_cors:
                continue

            for kw in node.keywords:
                if kw.arg == "allow_origins":
                    val = kw.value
                    # Check for ["*"]
                    if (isinstance(val, ast.List)
                        and len(val.elts) == 1
                        and isinstance(val.elts[0], ast.Constant)
                        and val.elts[0].value == "*"):
                        if node.lineno <= len(lines) and "# noqa" in lines[node.lineno - 1]:
                            continue
                        issues.append(DoctorIssue(
                            check="security/cors-wildcard",
                            severity="warning",
                            message="CORSMiddleware with allow_origins=['*'] — any site can call your API",
                            path=module.rel_path,
                            category="Security",
                            help="Specify explicit allowed origins: allow_origins=['https://yourdomain.com']",
                            line=node.lineno,
                        ))
    return issues


def check_pydantic_secretstr() -> list[DoctorIssue]:
    """Detect sensitive fields (password, token) in Pydantic models not using SecretStr.

    Using SecretStr prevents accidental leakage of sensitive values in logs,
    repr, and error messages.
    """
    _SENSITIVE_VAR_PATTERN = re.compile(r"(?:api_?key|auth_?token|password|secret|credential|private_?key)", re.IGNORECASE)
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
                    if _SENSITIVE_VAR_PATTERN.search(stmt.target.id):
                        # Check if it uses SecretStr
                        ann = stmt.annotation
                        is_secret_str = False
                        if isinstance(ann, ast.Name) and ann.id == "SecretStr":
                            is_secret_str = True
                        elif isinstance(ann, ast.Attribute) and ann.attr == "SecretStr":
                            is_secret_str = True
                        
                        if not is_secret_str:
                            issues.append(DoctorIssue(
                                check="security/pydantic-secretstr",
                                severity="warning",
                                message=f"Field '{stmt.target.id}' in model '{node.name}' should use SecretStr",
                                path=module.rel_path,
                                category="Security",
                                help="Use pydantic.SecretStr for sensitive fields to prevent leakage. Access the value via .get_secret_value().",
                                line=stmt.lineno,
                            ))
    return issues
__all__ = [
    "check_assert_in_production",
    "check_cors_wildcard",
    "check_exception_detail_leak",
    "check_hardcoded_secrets",
    "check_pydantic_secretstr",
    "check_shell_true",
    "check_sql_fstring_interpolation",
    "check_unsafe_hash_usage",
    "check_unsafe_yaml_load",
]
