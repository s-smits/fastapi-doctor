import ast
from pathlib import Path
from unittest.mock import MagicMock, patch

from fastapi_doctor.checks.correctness import check_asyncio_run_in_async_context
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
        issues = self._check('API_KEY = "aB3$xY7!mN9pQ2wE"')
        assert len(issues) == 1
        assert issues[0].check == "security/hardcoded-secret"

    def test_known_pattern_always_flagged(self):
        """Stripe key pattern should always be flagged regardless of variable name."""
        issues = self._check('MY_VAR = "sk_live_1234567890abcdef"')
        assert len(issues) == 1

    def test_short_values_not_flagged(self):
        """Values < 8 chars should not be flagged."""
        issues = self._check('PASSWORD = "short"')
        assert len(issues) == 0

    def test_doctor_ignore_suppresses(self):
        """doctor:ignore should suppress the finding."""
        source = 'API_KEY = "aB3$xY7!mN9pQ2wE"  # doctor:ignore security/hardcoded-secret reason="internal test"'
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
