import ast
from pathlib import Path
from unittest.mock import MagicMock, patch

from fastapi_doctor.checks.architecture import (
    check_async_without_await,
    check_startup_validation,
)
from fastapi_doctor.checks.correctness import (
    check_asyncio_run_in_async_context,
    check_misused_async_constructs,
    check_sync_io_in_async,
)
from fastapi_doctor.checks.resilience import check_sqlalchemy_pool_pre_ping
from fastapi_doctor.checks.security import (
    check_assert_in_production,
    check_hardcoded_secrets,
    check_pydantic_secretstr,
)
from fastapi_doctor.models import DoctorIssue, DoctorReport
from fastapi_doctor.suppression import collect_suppressions, is_suppressed


class MockModule:
    def __init__(self, rel_path, source):
        self.rel_path = rel_path
        self.source = source
        self.tree = ast.parse(source)
        self.path = Path(rel_path)


def _runtime_high_entropy_value() -> str:
    """Build a secret-like value without storing it as a repo literal."""
    return "".join(chr(code) for code in [97, 66, 51, 36, 120, 89, 55, 33, 109, 78, 57, 112, 81, 50, 119, 69])


def _runtime_known_pattern_value() -> str:
    """Build a provider-style test token without committing the full token literal."""
    prefix = "".join(chr(code) for code in [115, 107, 95, 108, 105, 118, 101, 95])
    return f"{prefix}1234567890abcdef"


# ── Existing tests ───────────────────────────────────────────────────────────


def test_check_assert_in_production():
    # Case 1: assert in production
    m1 = MockModule("app.py", "assert x == 1")
    with patch("fastapi_doctor.project.parsed_python_modules", return_value=[m1]):
        issues = check_assert_in_production()
        assert len(issues) == 1
        assert issues[0].check == "security/assert-in-production"
        assert "Do not wrap in 'if condition:'" in issues[0].help

    # Case 2: assert in tests (exempt)
    m2 = MockModule("tests/test_app.py", "assert x == 1")
    with patch("fastapi_doctor.project.parsed_python_modules", return_value=[m2]):
        issues = check_assert_in_production()
        assert len(issues) == 0


def test_check_sqlalchemy_pool_pre_ping():
    # Case 1: Missing pool_pre_ping
    m1 = MockModule("db.py", "engine = create_engine('sqlite://')")
    with patch("fastapi_doctor.project.parsed_python_modules", return_value=[m1]):
        issues = check_sqlalchemy_pool_pre_ping()
        assert len(issues) == 1
        assert issues[0].check == "resilience/sqlalchemy-pool-pre-ping"

    # Case 2: Has pool_pre_ping=True
    m2 = MockModule("db.py", "engine = create_engine('sqlite://', pool_pre_ping=True)")
    with patch("fastapi_doctor.project.parsed_python_modules", return_value=[m2]):
        issues = check_sqlalchemy_pool_pre_ping()
        assert len(issues) == 0


def test_check_pydantic_secretstr():
    # Case 1: Plain str for password
    m1 = MockModule("models.py", "class User(BaseModel):\n    password: str")
    with patch("fastapi_doctor.project.parsed_python_modules", return_value=[m1]):
        issues = check_pydantic_secretstr()
        assert len(issues) == 1
        assert issues[0].check == "security/pydantic-secretstr"

    # Case 2: Using SecretStr
    m2 = MockModule("models.py", "class User(BaseModel):\n    password: SecretStr")
    with patch("fastapi_doctor.project.parsed_python_modules", return_value=[m2]):
        issues = check_pydantic_secretstr()
        assert len(issues) == 0


def test_check_startup_validation_skips_router_main_modules():
    module = MockModule(
        "app/api/main.py",
        "from fastapi import APIRouter\n\napi_router = APIRouter()\n",
    )
    with patch("fastapi_doctor.project.parsed_python_modules", return_value=[module]):
        issues = check_startup_validation()
        assert issues == []


def test_check_startup_validation_accepts_eager_settings_bootstrap():
    module = MockModule(
        "app/main.py",
        (
            "from fastapi import FastAPI\n"
            "from app.core.config import settings\n\n"
            "app = FastAPI(title=settings.PROJECT_NAME)\n"
        ),
    )
    with patch("fastapi_doctor.project.parsed_python_modules", return_value=[module]):
        issues = check_startup_validation()
        assert issues == []


# ── Suppression system tests ────────────────────────────────────────────────


class TestSuppression:
    def test_bare_noqa_suppresses_all(self):
        assert is_suppressed("x = 1  # noqa", "security/hardcoded-secret") is True
        assert is_suppressed("x = 1  # noqa", "correctness/sync-io-in-async") is True

    def test_noqa_with_code_suppresses_matching(self):
        assert is_suppressed("x = 1  # noqa: sql-safe", "security/sql-fstring-interpolation") is True
        assert is_suppressed("x = 1  # noqa: sql-safe", "correctness/sync-io-in-async") is False

    def test_noqa_architecture_alias(self):
        assert is_suppressed("x = 1  # noqa: architecture", "architecture/giant-function") is True
        assert is_suppressed("x = 1  # noqa: architecture", "security/hardcoded-secret") is False

    def test_noqa_direct_rule_id(self):
        assert is_suppressed("x = 1  # noqa: security/hardcoded-secret", "security/hardcoded-secret") is True
        assert is_suppressed("x = 1  # noqa: security/hardcoded-secret", "security/cors-wildcard") is False

    def test_doctor_ignore_exact_rule(self):
        line = 'x = 1  # doctor:ignore security/hardcoded-secret reason="enum label"'
        assert is_suppressed(line, "security/hardcoded-secret") is True
        assert is_suppressed(line, "security/cors-wildcard") is False

    def test_doctor_ignore_wildcard(self):
        line = "x = 1  # doctor:ignore security/*"
        assert is_suppressed(line, "security/hardcoded-secret") is True
        assert is_suppressed(line, "security/cors-wildcard") is True
        assert is_suppressed(line, "correctness/sync-io-in-async") is False

    def test_doctor_ignore_without_reason(self):
        line = "x = 1  # doctor:ignore security/hardcoded-secret"
        assert is_suppressed(line, "security/hardcoded-secret") is True

    def test_collect_suppressions(self):
        source = (
            'x = 1\n'
            'y = "token"  # doctor:ignore security/hardcoded-secret reason="enum label"\n'
            'z = 3  # noqa\n'  # noqa is NOT collected (only doctor:ignore)
        )
        results = collect_suppressions(source, "app.py")
        assert len(results) == 1
        assert results[0]["rule"] == "security/hardcoded-secret"
        assert results[0]["reason"] == "enum label"
        assert results[0]["path"] == "app.py"
        assert results[0]["line"] == 2


# ── Kind / confidence / action_type tests ────────────────────────────────────


class TestDoctorIssueKind:
    def test_security_error_is_blocker(self):
        issue = DoctorIssue(
            check="security/sql-fstring-interpolation",
            severity="error",
            message="SQL injection",
            path="app.py",
            category="Security",
        )
        assert issue.kind == "blocker"
        assert issue.is_ship_blocker is True
        assert issue.priority == "high"
        assert issue.action_type == "code_fix"

    def test_security_warning_is_risk(self):
        issue = DoctorIssue(
            check="security/cors-wildcard",
            severity="warning",
            message="CORS wildcard",
            path="app.py",
            category="Security",
        )
        assert issue.kind == "risk"
        assert issue.is_ship_blocker is False
        assert issue.priority == "medium"
        assert issue.action_type == "config_tune"  # overridden

    def test_assert_in_production_demoted_to_risk(self):
        issue = DoctorIssue(
            check="security/assert-in-production",
            severity="error",
            message="assert in prod",
            path="app.py",
            category="Security",
        )
        assert issue.kind == "risk"  # overridden from default blocker
        assert issue.is_ship_blocker is False

    def test_architecture_is_opinionated(self):
        issue = DoctorIssue(
            check="architecture/giant-function",
            severity="warning",
            message="Giant function",
            path="app.py",
            category="Architecture",
        )
        assert issue.kind == "opinionated"
        assert issue.priority == "low"
        assert issue.action_type == "review_manually"

    def test_performance_is_hygiene(self):
        issue = DoctorIssue(
            check="performance/sequential-awaits",
            severity="warning",
            message="Sequential awaits",
            path="app.py",
            category="Performance",
        )
        assert issue.kind == "hygiene"
        assert issue.priority == "low"

    def test_confidence_override(self):
        issue = DoctorIssue(
            check="performance/n-plus-one-hint",
            severity="warning",
            message="N+1",
            path="app.py",
            category="Performance",
        )
        assert issue.confidence == 0.4  # overridden

    def test_confidence_default_from_kind(self):
        issue = DoctorIssue(
            check="security/sql-fstring-interpolation",
            severity="error",
            message="SQL injection",
            path="app.py",
            category="Security",
        )
        assert issue.confidence == 0.95  # blocker default

    def test_to_dict_includes_new_fields(self):
        issue = DoctorIssue(
            check="security/hardcoded-secret",
            severity="error",
            message="Secret found",
            path="app.py",
            category="Security",
        )
        d = issue.to_dict()
        assert "kind" in d
        assert "confidence" in d
        assert "action_type" in d
        assert "is_ship_blocker" in d
        assert d["change_scope"] == "minimal"
        assert "Avoid namespace rewrites" in d["autofix_guidance"]
        assert d["kind"] == "blocker"


# ── DoctorReport blocker gating tests ────────────────────────────────────────


class TestDoctorReportBlockerGating:
    def test_blocker_count_and_ship_blockers(self):
        issues = [
            DoctorIssue(check="security/sql-fstring-interpolation", severity="error",
                        message="SQL injection", path="a.py", category="Security"),
            DoctorIssue(check="security/cors-wildcard", severity="warning",
                        message="CORS", path="a.py", category="Security"),
            DoctorIssue(check="architecture/giant-function", severity="warning",
                        message="Giant", path="a.py", category="Architecture"),
        ]
        report = DoctorReport(route_count=0, openapi_path_count=0, issues=issues)
        assert report.blocker_count == 1  # only the SQL injection
        assert report.has_ship_blockers is True

    def test_no_blockers(self):
        issues = [
            DoctorIssue(check="architecture/giant-function", severity="warning",
                        message="Giant", path="a.py", category="Architecture"),
        ]
        report = DoctorReport(route_count=0, openapi_path_count=0, issues=issues)
        assert report.blocker_count == 0
        assert report.has_ship_blockers is False

    def test_checks_not_evaluated_in_output(self):
        report = DoctorReport(
            route_count=0,
            openapi_path_count=0,
            issues=[],
            checks_not_evaluated=["security/missing-auth-dep", "api-surface/*"],
        )
        d = report.to_dict()
        assert d["checks_not_evaluated"] == ["security/missing-auth-dep", "api-surface/*"]

    def test_suppressions_in_output(self):
        report = DoctorReport(
            route_count=0,
            openapi_path_count=0,
            issues=[],
            suppressions=[{"rule": "security/hardcoded-secret", "reason": "enum", "path": "a.py", "line": 5}],
        )
        d = report.to_dict()
        assert len(d["suppressions"]) == 1

    def test_next_actions_sorted_by_kind(self):
        issues = [
            DoctorIssue(check="architecture/giant-function", severity="warning",
                        message="Giant", path="a.py", category="Architecture"),
            DoctorIssue(check="security/sql-fstring-interpolation", severity="error",
                        message="SQL injection", path="b.py", category="Security"),
            DoctorIssue(check="performance/sequential-awaits", severity="warning",
                        message="Sequential", path="c.py", category="Performance"),
        ]
        report = DoctorReport(route_count=0, openapi_path_count=0, issues=issues)
        actions = report.next_actions()
        kinds = [a["kind"] for a in actions]
        # security (tier 0) blocker first, then strict-tier (tier 2) items
        assert kinds == ["blocker", "opinionated", "hygiene"]

    def test_next_actions_include_new_fields(self):
        issues = [
            DoctorIssue(check="security/sql-fstring-interpolation", severity="error",
                        message="SQL injection", path="b.py", category="Security"),
        ]
        report = DoctorReport(route_count=0, openapi_path_count=0, issues=issues)
        action = report.next_actions()[0]
        assert "kind" in action
        assert "confidence" in action
        assert "action_type" in action
        assert action["change_scope"] == "minimal"
        assert "Avoid namespace rewrites" in action["autofix_guidance"]


# ── Hardcoded secret false-positive tests ────────────────────────────────────


class TestHardcodedSecretFalsePositives:
    def _check(self, source: str) -> list:
        m = MockModule("config.py", source)
        with patch("fastapi_doctor.project.parsed_python_modules", return_value=[m]):
            return check_hardcoded_secrets()

    def test_enum_label_not_flagged(self):
        """OAUTH_TOKEN = 'oauth_token' should NOT be flagged."""
        issues = self._check('OAUTH_TOKEN = "oauth_token"')
        assert len(issues) == 0

    def test_identifier_value_not_flagged(self):
        """CREDENTIAL_TYPE = 'credential_type' should NOT be flagged."""
        issues = self._check('CREDENTIAL_TYPE = "credential_type"')
        assert len(issues) == 0

    def test_header_name_not_flagged(self):
        """API_KEY_HEADER = 'X-Api-Key' should NOT be flagged."""
        issues = self._check('API_KEY_HEADER = "X-Api-Key-Header"')
        assert len(issues) == 0

    def test_protocol_label_not_flagged(self):
        """AUTH_TOKEN_TYPE = 'bearer-token' should NOT be flagged."""
        issues = self._check('AUTH_TOKEN_TYPE = "bearer-token"')
        assert len(issues) == 0

    def test_placeholder_not_flagged(self):
        """PASSWORD = 'fake-encrypted-value' should NOT be flagged."""
        issues = self._check('PASSWORD = "fake-encrypted-value-here"')
        assert len(issues) == 0

    def test_url_not_flagged(self):
        """API_KEY_URL = 'https://...' should NOT be flagged."""
        issues = self._check('API_KEY_URL = "https://example.com/keys"')
        assert len(issues) == 0

    def test_real_secret_still_flagged(self):
        """A value that looks like a real secret should still be flagged."""
        # Mixed case + digits + special chars = looks like a real secret
        issues = self._check(f'API_KEY = "{_runtime_high_entropy_value()}"')
        assert len(issues) == 1
        assert issues[0].check == "security/hardcoded-secret"

    def test_known_pattern_always_flagged(self):
        """Stripe key pattern should always be flagged regardless of variable name."""
        issues = self._check(f'MY_VAR = "{_runtime_known_pattern_value()}"')
        assert len(issues) == 1

    def test_short_values_not_flagged(self):
        """Values < 8 chars should not be flagged."""
        issues = self._check('PASSWORD = "short"')
        assert len(issues) == 0

    def test_doctor_ignore_suppresses(self):
        """doctor:ignore should suppress the finding."""
        source = (
            f'API_KEY = "{_runtime_high_entropy_value()}" '
            '# doctor:ignore security/hardcoded-secret reason="internal test"'
        )
        issues = self._check(source)
        assert len(issues) == 0


# ── asyncio.run() exemption tests ────────────────────────────────────────────


class TestAsyncioRunExemptions:
    def _check(self, source: str, rel_path: str = "service.py") -> list:
        m = MockModule(rel_path, source)
        with patch("fastapi_doctor.project.parsed_python_modules", return_value=[m]):
            return check_asyncio_run_in_async_context()

    def test_asyncio_run_in_name_main_exempt(self):
        """asyncio.run() inside if __name__ == '__main__' should NOT be flagged."""
        source = '''
import asyncio

async def main():
    pass

if __name__ == "__main__":
    asyncio.run(main())
'''
        issues = self._check(source)
        assert len(issues) == 0

    def test_asyncio_run_outside_name_main_flagged(self):
        """asyncio.run() at module level (not in __main__ guard) should be flagged."""
        source = '''
import asyncio

async def main():
    pass

asyncio.run(main())
'''
        issues = self._check(source)
        assert len(issues) == 1

    def test_asyncio_run_in_dunder_main_file_exempt(self):
        """asyncio.run() in __main__.py should be exempt."""
        source = '''
import asyncio

async def main():
    pass

asyncio.run(main())
'''
        issues = self._check(source, rel_path="__main__.py")
        assert len(issues) == 0

    def test_asyncio_run_in_cli_file_exempt(self):
        """asyncio.run() in cli.py should be exempt."""
        source = '''
import asyncio

async def main():
    pass

asyncio.run(main())
'''
        issues = self._check(source, rel_path="cli.py")
        assert len(issues) == 0

    def test_doctor_ignore_suppresses(self):
        """# doctor:ignore should suppress the finding."""
        source = '''
import asyncio

async def main():
    pass

asyncio.run(main())  # doctor:ignore correctness/asyncio-run-in-async
'''
        issues = self._check(source)
        assert len(issues) == 0


# ── Async/sync misuse analysis tests ────────────────────────────────────────


class TestAsyncSyncMisuseAnalysis:
    def _check_sync_io(self, source: str, rel_path: str = "services/worker.py") -> list:
        module = MockModule(rel_path, source)
        with patch("fastapi_doctor.project.parsed_python_modules", return_value=[module]):
            return check_sync_io_in_async()

    def _check_async_without_await(self, source: str, rel_path: str = "services/worker.py") -> list:
        module = MockModule(rel_path, source)
        with patch("fastapi_doctor.project.parsed_python_modules", return_value=[module]):
            return check_async_without_await()

    def test_direct_blocking_call_in_async_non_router_module(self):
        source = '''
async def load_data():
    with open("data.txt") as fh:
        return fh.read()
'''
        issues = self._check_sync_io(source)
        assert len(issues) == 1
        assert issues[0].check == "correctness/sync-io-in-async"
        assert "open()" in issues[0].message
        assert issues[0].line == 3

    def test_transitive_sync_helper_with_blocking_http_is_flagged(self):
        source = '''
import requests

def fetch_profile():
    return requests.get("https://example.com")

async def load_profile():
    return fetch_profile()
'''
        issues = self._check_sync_io(source)
        assert len(issues) == 1
        assert "fetch_profile()" in issues[0].message
        assert "requests.get()" in issues[0].help
        assert issues[0].line == 8

    def test_sync_helper_only_used_from_sync_code_is_ignored(self):
        source = '''
import requests

def fetch_profile():
    return requests.get("https://example.com")

def load_profile():
    return fetch_profile()
'''
        issues = self._check_sync_io(source)
        assert issues == []

    def test_repo_wide_async_without_await_flags_internal_helpers(self):
        source = '''
async def helper():
    return 1
'''
        issues = self._check_async_without_await(source)
        assert len(issues) == 1
        assert issues[0].check == "architecture/async-without-await"
        assert "effectively synchronous" in issues[0].message
        assert "no real async work" in issues[0].help

    def test_async_without_await_keeps_fastapi_route_guidance(self):
        source = '''
from fastapi import APIRouter

router = APIRouter()

@router.get("/items")
async def list_items():
    return {"ok": True}
'''
        issues = self._check_async_without_await(source, rel_path="routers/items.py")
        assert len(issues) == 1
        assert issues[0].check == "architecture/async-without-await"
        assert "route handler" in issues[0].message
        assert "thread pool" in issues[0].help

    def test_async_function_with_real_async_work_is_not_flagged(self):
        source = '''
import asyncio

async def helper():
    await asyncio.sleep(0)
    return 1
'''
        issues = self._check_async_without_await(source)
        assert issues == []

    def test_doctor_ignore_on_async_caller_suppresses_transitive_sync_issue(self):
        source = '''
import requests

def fetch_profile():
    return requests.get("https://example.com")

async def load_profile():
    return fetch_profile()  # doctor:ignore correctness/sync-io-in-async reason="legacy wrapper"
'''
        issues = self._check_sync_io(source)
        assert issues == []

    def test_await_on_sync_function_is_flagged(self):
        source = '''
def sync_helper():
    return 1

async def main():
    return await sync_helper()
'''
        module = MockModule("a.py", source)
        with patch("fastapi_doctor.project.parsed_python_modules", return_value=[module]):
            issues = check_misused_async_constructs()
        assert len(issues) == 1
        assert issues[0].check == "correctness/await-on-sync"
        assert "await used on sync function 'sync_helper()'" in issues[0].message

    def test_await_on_sync_function_returning_awaitable_is_ignored(self):
        source = '''
import asyncio

def create_task():
    return asyncio.create_task(asyncio.sleep(1))

async def main():
    return await create_task()
'''
        module = MockModule("a.py", source)
        with patch("fastapi_doctor.project.parsed_python_modules", return_value=[module]):
            issues = check_misused_async_constructs()
        assert issues == []

    def test_async_for_on_sync_iterable_is_flagged(self):
        source = '''
def get_items():
    return [1, 2, 3]

async def main():
    async for item in get_items():
        print(item)
'''
        module = MockModule("a.py", source)
        with patch("fastapi_doctor.project.parsed_python_modules", return_value=[module]):
            issues = check_misused_async_constructs()
        assert len(issues) == 1
        assert issues[0].check == "correctness/sync-iterable-in-async-for"
        assert "async for used on sync iterable" in issues[0].message

    def test_async_with_on_sync_context_manager_is_flagged(self):
        source = '''
from contextlib import contextmanager

@contextmanager
def sync_cm():
    yield "ok"

async def main():
    async with sync_cm() as res:
        print(res)
'''
        module = MockModule("a.py", source)
        with patch("fastapi_doctor.project.parsed_python_modules", return_value=[module]):
            issues = check_misused_async_constructs()
        assert len(issues) == 1
        assert issues[0].check == "correctness/sync-cm-in-async-with"
        assert "async with used on sync context manager" in issues[0].message

    def test_transitive_unnecessary_async_is_flagged(self):
        source = '''
async def leaf():
    return 1  # No awaits

async def intermediate():
    return await leaf()

async def root():
    return await intermediate()
'''
        issues = self._check_async_without_await(source)
        # All three should be flagged:
        # leaf: direct
        # intermediate: awaits leaf (unnecessary)
        # root: awaits intermediate (unnecessary)
        checks = [i.check for i in issues]
        assert checks.count("architecture/async-without-await") == 3
        
        # Verify message for transitive case
        root_issue = [i for i in issues if "root" in i.message][0]
        assert "effectively synchronous" in root_issue.message
        assert "no real async work" in root_issue.help



# ── Bootstrap failure finding test ───────────────────────────────────────────


class TestBootstrapFailureFinding:
    def test_bootstrap_failure_creates_finding(self):
        """If app bootstrap fails, the report should contain a doctor/app-bootstrap-failed issue."""
        issue = DoctorIssue(
            check="doctor/app-bootstrap-failed",
            severity="error",
            message="FastAPI app failed to boot — route-level checks were skipped: ImportError",
            path="myapp.main:app",
            category="Doctor",
            help="Fix the import/startup error.",
        )
        assert issue.kind == "blocker"  # Doctor category + error severity = blocker
        assert issue.is_ship_blocker is True

    def test_bootstrap_failure_in_report(self):
        """Bootstrap failure should appear in report with checks_not_evaluated."""
        issues = [
            DoctorIssue(
                check="doctor/app-bootstrap-failed",
                severity="error",
                message="Boot failed",
                path="unknown",
                category="Doctor",
            )
        ]
        report = DoctorReport(
            route_count=0,
            openapi_path_count=0,
            issues=issues,
            checks_not_evaluated=["security/missing-auth-dep", "api-surface/*"],
        )
        d = report.to_dict()
        assert d["has_ship_blockers"] is True
        assert "security/missing-auth-dep" in d["checks_not_evaluated"]
        assert d["blocker_count"] == 1


# ── Profile tier ordering tests ─────────────────────────────────────────


class TestProfileTierOrdering:
    def test_security_rule_is_tier_0(self):
        issue = DoctorIssue(
            check="security/sql-fstring-interpolation", severity="error",
            message="SQL injection", path="a.py", category="Security",
        )
        assert issue.profile_tier == 0
        assert issue.profile_tier_label == "security"

    def test_pydantic_sensitive_field_is_tier_0(self):
        issue = DoctorIssue(
            check="pydantic/sensitive-field-type", severity="warning",
            message="Use SecretStr", path="a.py", category="Pydantic",
        )
        assert issue.profile_tier == 0

    def test_correctness_rule_is_tier_1(self):
        issue = DoctorIssue(
            check="correctness/sync-io-in-async", severity="error",
            message="Blocking I/O", path="a.py", category="Correctness",
        )
        assert issue.profile_tier == 1
        assert issue.profile_tier_label == "balanced"

    def test_resilience_rule_is_tier_1(self):
        issue = DoctorIssue(
            check="resilience/bare-except-pass", severity="warning",
            message="Bare except", path="a.py", category="Resilience",
        )
        assert issue.profile_tier == 1

    def test_architecture_balanced_rule_is_tier_1(self):
        """architecture/async-without-await is in the balanced set."""
        issue = DoctorIssue(
            check="architecture/async-without-await", severity="warning",
            message="Async no await", path="a.py", category="Architecture",
        )
        assert issue.profile_tier == 1

    def test_architecture_strict_only_is_tier_2(self):
        """architecture/giant-function is strict-only."""
        issue = DoctorIssue(
            check="architecture/giant-function", severity="warning",
            message="Giant function", path="a.py", category="Architecture",
        )
        assert issue.profile_tier == 2
        assert issue.profile_tier_label == "strict"

    def test_performance_is_tier_2(self):
        issue = DoctorIssue(
            check="performance/sequential-awaits", severity="warning",
            message="Sequential awaits", path="a.py", category="Performance",
        )
        assert issue.profile_tier == 2

    def test_next_actions_sorts_by_tier_then_kind(self):
        """Security tier blocker should come before balanced tier blocker,
        which should come before strict tier issues."""
        issues = [
            # strict tier (2), opinionated
            DoctorIssue(check="architecture/giant-function", severity="warning",
                        message="Giant", path="a.py", category="Architecture"),
            # balanced tier (1), blocker
            DoctorIssue(check="correctness/sync-io-in-async", severity="error",
                        message="Blocking I/O", path="b.py", category="Correctness"),
            # security tier (0), blocker
            DoctorIssue(check="security/sql-fstring-interpolation", severity="error",
                        message="SQL injection", path="c.py", category="Security"),
            # balanced tier (1), risk
            DoctorIssue(check="resilience/bare-except-pass", severity="warning",
                        message="Bare except", path="d.py", category="Resilience"),
            # security tier (0), risk
            DoctorIssue(check="security/cors-wildcard", severity="warning",
                        message="CORS", path="e.py", category="Security"),
        ]
        report = DoctorReport(route_count=0, openapi_path_count=0, issues=issues)
        actions = report.next_actions()
        order = [(a["profile_tier_label"], a["kind"], a["rule"]) for a in actions]
        # Expected: security blockers, security risks, balanced blockers, balanced risks, strict
        assert order == [
            ("security", "blocker", "security/sql-fstring-interpolation"),
            ("security", "risk", "security/cors-wildcard"),
            ("balanced", "blocker", "correctness/sync-io-in-async"),
            ("balanced", "risk", "resilience/bare-except-pass"),
            ("strict", "opinionated", "architecture/giant-function"),
        ]

    def test_to_dict_includes_profile_tier(self):
        issue = DoctorIssue(
            check="security/hardcoded-secret", severity="error",
            message="Secret", path="a.py", category="Security",
        )
        d = issue.to_dict()
        assert d["profile_tier"] == 0
        assert d["profile_tier_label"] == "security"

    def test_next_actions_include_profile_tier(self):
        issues = [
            DoctorIssue(check="security/sql-fstring-interpolation", severity="error",
                        message="SQL injection", path="b.py", category="Security"),
        ]
        report = DoctorReport(route_count=0, openapi_path_count=0, issues=issues)
        action = report.next_actions()[0]
        assert action["profile_tier"] == 0
        assert action["profile_tier_label"] == "security"
