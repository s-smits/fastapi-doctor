from __future__ import annotations

from importlib.metadata import version as metadata_version

import fastapi_doctor
import fastapi_doctor.cli as cli_module


def test_exported_version_matches_distribution_metadata() -> None:
    expected_version = metadata_version("fastapi-doctor")

    assert fastapi_doctor.__version__ == expected_version
    assert cli_module.get_cli_version() == expected_version
