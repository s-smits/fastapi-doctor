import ast
from pathlib import Path
from unittest.mock import MagicMock, patch

from fastapi_doctor.checks.resilience import check_sqlalchemy_pool_pre_ping
from fastapi_doctor.checks.security import (
    check_assert_in_production,
    check_pydantic_secretstr,
)


class MockModule:
    def __init__(self, rel_path, source):
        self.rel_path = rel_path
        self.source = source
        self.tree = ast.parse(source)
        self.path = Path(rel_path)


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



