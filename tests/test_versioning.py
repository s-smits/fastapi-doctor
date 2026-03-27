from __future__ import annotations

import fastapi_doctor
import fastapi_doctor.cli as cli_module
from fastapi_doctor import _version


def test_exported_version_matches_distribution_metadata() -> None:
    expected_version = _version.version

    assert fastapi_doctor.__version__ == expected_version
    assert cli_module.get_cli_version() == expected_version
