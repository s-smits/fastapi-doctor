from __future__ import annotations

import tomllib
from pathlib import Path

import fastapi_doctor
import fastapi_doctor.cli as cli_module
from fastapi_doctor import _version


def test_exported_version_matches_distribution_metadata() -> None:
    expected_version = _version.version

    assert fastapi_doctor.__version__ == expected_version
    assert cli_module.get_cli_version() == expected_version


def test_python_and_rust_package_versions_match() -> None:
    cargo_toml = Path(__file__).parents[1] / "rust" / "Cargo.toml"
    cargo = tomllib.loads(cargo_toml.read_text())

    assert _version.version == cargo["workspace"]["package"]["version"]
