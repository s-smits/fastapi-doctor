from __future__ import annotations

from fastapi_doctor.sarif import to_github_annotations, to_sarif

_SAMPLE_ISSUES = [
    {
        "check": "security/unsafe-yaml-load",
        "severity": "error",
        "category": "Security",
        "line": 5,
        "path": "app/main.py",
        "message": "yaml.load() without SafeLoader",
        "help": "Use yaml.safe_load().",
    },
    {
        "check": "architecture/giant-function",
        "severity": "warning",
        "category": "Architecture",
        "line": 20,
        "path": "app/routes.py",
        "message": "Function too large (500 lines)",
        "help": "Break the function into smaller units.",
    },
]


def test_sarif_schema_version() -> None:
    result = to_sarif(issues=_SAMPLE_ISSUES, version="0.6.0")
    assert result["version"] == "2.1.0"
    assert "$schema" in result


def test_sarif_tool_metadata() -> None:
    result = to_sarif(issues=_SAMPLE_ISSUES, version="0.6.0")
    driver = result["runs"][0]["tool"]["driver"]
    assert driver["name"] == "fastapi-doctor"
    assert driver["version"] == "0.6.0"
    assert "helpUri" not in driver["rules"][0]


def test_sarif_rules_deduplication() -> None:
    duplicate_issues = _SAMPLE_ISSUES + [
        {
            "check": "security/unsafe-yaml-load",
            "severity": "error",
            "category": "Security",
            "line": 10,
            "path": "app/other.py",
            "message": "yaml.load() without SafeLoader",
            "help": "Use yaml.safe_load().",
        },
    ]
    result = to_sarif(issues=duplicate_issues, version="0.6.0")
    rules = result["runs"][0]["tool"]["driver"]["rules"]
    rule_ids = [r["id"] for r in rules]
    assert rule_ids.count("security/unsafe-yaml-load") == 1


def test_sarif_result_count() -> None:
    result = to_sarif(issues=_SAMPLE_ISSUES, version="0.6.0")
    assert len(result["runs"][0]["results"]) == 2


def test_sarif_error_level_mapping() -> None:
    result = to_sarif(issues=_SAMPLE_ISSUES, version="0.6.0")
    results = result["runs"][0]["results"]
    error_result = next(r for r in results if r["ruleId"] == "security/unsafe-yaml-load")
    assert error_result["level"] == "error"
    warning_result = next(r for r in results if r["ruleId"] == "architecture/giant-function")
    assert warning_result["level"] == "warning"


def test_sarif_location() -> None:
    result = to_sarif(issues=_SAMPLE_ISSUES, version="0.6.0")
    loc = result["runs"][0]["results"][0]["locations"][0]["physicalLocation"]
    assert loc["artifactLocation"]["uri"] == "app/main.py"
    assert loc["region"]["startLine"] == 5


def test_sarif_empty_issues() -> None:
    result = to_sarif(issues=[], version="0.6.0")
    assert result["runs"][0]["results"] == []
    assert result["runs"][0]["tool"]["driver"]["rules"] == []


def test_github_annotations_format() -> None:
    output = to_github_annotations(_SAMPLE_ISSUES)
    lines = output.split("\n")
    assert len(lines) == 2
    assert lines[0].startswith("::error ")
    assert "file=app/main.py" in lines[0]
    assert "line=5" in lines[0]
    assert "title=security/unsafe-yaml-load" in lines[0]
    assert lines[1].startswith("::warning ")


def test_github_annotations_escape_special_characters() -> None:
    output = to_github_annotations(
        [
            {
                "check": "security/unsafe,yaml:load",
                "severity": "error",
                "category": "Security",
                "line": 0,
                "path": "app,main.py",
                "message": "bad\nnext%line",
                "help": "Use yaml.safe_load().",
            }
        ]
    )
    assert "file=app%2Cmain.py" in output
    assert "title=security/unsafe%2Cyaml%3Aload" in output
    assert "line=0" not in output
    assert output.endswith("::bad%0Anext%25line")


def test_github_annotations_empty() -> None:
    output = to_github_annotations([])
    assert output == ""
