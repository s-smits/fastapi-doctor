from __future__ import annotations

"""Compatibility helpers for installed metadata and CLI aliases."""

from importlib.metadata import PackageNotFoundError, version as metadata_version


BALANCED_PROFILE_ALIASES = frozenset({"balanced", "medium"})


def get_installed_version() -> str:
    try:
        from ._version import version

        return version
    except ImportError:
        try:
            return metadata_version("fastapi-doctor")
        except PackageNotFoundError:
            return "0.0.0"


def is_balanced_profile(profile: str | None) -> bool:
    return profile in BALANCED_PROFILE_ALIASES


def normalize_cli_profile(value: str) -> str:
    normalized = value.strip().lower()
    if normalized == "medium":
        return "balanced"
    return normalized
