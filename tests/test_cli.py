from __future__ import annotations

import argparse
import json
from pathlib import Path

import fastapi_doctor.cli as cli_module


def test_main_emits_json_from_native_payload(
    monkeypatch, capsys, tmp_path: Path
) -> None:
    monkeypatch.chdir(tmp_path)
    monkeypatch.setattr(
        cli_module,
        "parse_args",
        lambda: argparse.Namespace(
            json=True,
            score=False,
            verbose=False,
            fail_on="none",
            profile="balanced",
            ignore_rules=None,
            only_rules=None,
            repo_root=str(tmp_path),
            code_dir=None,
            import_root=None,
            app_module=None,
            skip_ruff=True,
            skip_ty=True,
            skip_structure=False,
            skip_openapi=False,
            static_only=True,
            skip_app_bootstrap=True,
            with_bandit=False,
            with_tests=False,
            pytest_args="tests/ -q",
        ),
    )
    monkeypatch.setattr(
        cli_module,
        "analyze_selected_current_project_v2",
        lambda **_: {
            "issues": [
                {
                    "check": "security/unsafe-yaml-load",
                    "severity": "error",
                    "category": "Security",
                    "line": 2,
                    "path": "pkg/bad.py",
                    "message": "yaml.load() without SafeLoader",
                    "help": "Use yaml.safe_load().",
                }
            ],
            "routes": [],
            "suppressions": [],
            "route_count": 0,
            "openapi_path_count": None,
            "categories": {"Security": 1},
            "score": 98,
            "label": "A",
            "checks_not_evaluated": [],
            "engine_reason": "rust-native",
            "project_context": {
                "layout": {
                    "repo_root": str(tmp_path),
                    "import_root": str(tmp_path),
                    "code_dir": str(tmp_path / "pkg"),
                    "app_module": "pkg.main:app",
                    "discovery_source": "explicit overrides",
                },
                "effective_config": {
                    "architecture": {"enabled": True},
                    "pydantic": {"should_be_model": "boundary"},
                    "api": {"create_post_prefixes": [], "tag_required_prefixes": ["/api/"]},
                    "security": {"forbidden_write_params": []},
                    "scan": {"exclude_dirs": [], "exclude_rules": []},
                },
            },
        },
    )

    assert cli_module.main() == 1

    payload = json.loads(capsys.readouterr().out)
    assert payload["doctor"]["issues"][0]["check"] == "security/unsafe-yaml-load"
    assert payload["project"]["repo_root"] == str(tmp_path)
    assert payload["effective_config"]["pydantic"]["should_be_model"] == "boundary"
