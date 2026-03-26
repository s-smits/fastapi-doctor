from __future__ import annotations

"""Rust sidecar integration for a subset of static checks.

This keeps FastAPI app loading and report assembly in Python while allowing the
static rule engine to move behind a native boundary incrementally.
"""

import os
import platform
import subprocess
import sys
from importlib import resources
from importlib.metadata import PackageNotFoundError, version as metadata_version
from pathlib import Path
from tempfile import NamedTemporaryFile

from . import project
from .models import DoctorIssue

NATIVE_STATIC_RULES = frozenset(
    {
        "architecture/import-bloat",
        "architecture/print-in-production",
        "architecture/star-import",
        "config/direct-env-access",
        "correctness/avoid-os-path",
        "correctness/deprecated-typing-imports",
        "correctness/naive-datetime",
        "performance/heavy-imports",
        "security/assert-in-production",
        "security/cors-wildcard",
        "security/exception-detail-leak",
        "security/subprocess-shell-true",
        "security/unsafe-yaml-load",
        "security/weak-hash-without-flag",
        "resilience/sqlalchemy-pool-pre-ping",
    }
)

_BINARY_ENV_VAR = "FASTAPI_DOCTOR_NATIVE_BINARY"
_BINARY_NAME = "fastapi-doctor-native"
_SUPPORTED_PLATFORMS = {
    ("darwin", "arm64"): "darwin-arm64",
    ("darwin", "x86_64"): "darwin-x86_64",
    ("linux", "aarch64"): "linux-arm64",
    ("linux", "arm64"): "linux-arm64",
    ("linux", "x86_64"): "linux-x86_64",
}
_VERSION_OK_CACHE: dict[Path, bool] = {}
_LAST_NATIVE_REASON = "native not evaluated yet"


def _set_last_native_reason(reason: str) -> None:
    global _LAST_NATIVE_REASON
    _LAST_NATIVE_REASON = reason


def last_native_reason() -> str:
    return _LAST_NATIVE_REASON


def _native_enabled() -> bool:
    value = os.environ.get("FASTAPI_DOCTOR_DISABLE_NATIVE", "").strip().lower()
    return value not in {"1", "true", "yes", "on"}


def _binary_filename() -> str:
    return f"{_BINARY_NAME}.exe" if sys.platform.startswith("win") else _BINARY_NAME


def _platform_tag() -> str | None:
    system = sys.platform
    if system.startswith("linux"):
        system = "linux"
    elif system == "darwin":
        system = "darwin"
    machine = platform.machine().lower()
    return _SUPPORTED_PLATFORMS.get((system, machine))


def _package_version() -> str:
    try:
        return metadata_version("fastapi-doctor")
    except PackageNotFoundError:
        try:
            from ._version import version
        except ImportError:
            return "0.0.0"
        return str(version)


def _normalize_version(value: str) -> str:
    return value.strip().removeprefix("v")


def _binary_version(path: Path) -> str | None:
    proc = subprocess.run(
        [str(path), "--version"],
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        return None
    version = proc.stdout.strip()
    return version or None


def _binary_version_matches_package(path: Path) -> bool:
    cached = _VERSION_OK_CACHE.get(path)
    if cached is not None:
        return cached

    binary_version = _binary_version(path)
    if binary_version is None:
        _VERSION_OK_CACHE[path] = False
        return False

    matches = _normalize_version(binary_version) == _normalize_version(_package_version())
    _VERSION_OK_CACHE[path] = matches
    return matches


def _override_binary() -> Path | None:
    value = os.environ.get(_BINARY_ENV_VAR, "").strip()
    if not value:
        return None
    path = Path(value).expanduser()
    if not path.exists():
        _set_last_native_reason(f"native override missing: {path}")
        return None
    return path


def _bundled_binary() -> Path | None:
    platform_tag = _platform_tag()
    if platform_tag is None:
        _set_last_native_reason("native unsupported on this platform")
        return None

    package_root = Path(str(resources.files("fastapi_doctor")))
    candidate = package_root / "bin" / platform_tag / _binary_filename()
    if not candidate.is_file():
        _set_last_native_reason(f"native bundle missing for {platform_tag}")
        return None
    return candidate


def _resolve_binary() -> Path | None:
    override = _override_binary()
    if override is not None:
        if _binary_version_matches_package(override):
            _set_last_native_reason(f"using override native binary: {override}")
            return override
        _set_last_native_reason("native override version mismatch")
        return None

    bundled = _bundled_binary()
    if bundled is None:
        return None
    if not _binary_version_matches_package(bundled):
        _set_last_native_reason("bundled native binary version mismatch")
        return None

    _set_last_native_reason(f"using bundled native binary for {_platform_tag()}")
    return bundled


def _hex_encode(text: str) -> str:
    return text.encode("utf-8").hex()


def _hex_decode(text: str) -> str:
    return bytes.fromhex(text).decode("utf-8")


def _parse_output(stdout: str) -> list[DoctorIssue]:
    issues: list[DoctorIssue] = []
    for raw_line in stdout.splitlines():
        if not raw_line:
            continue
        parts = raw_line.split("\t")
        if len(parts) != 8 or parts[0] != "ISSUE":
            raise ValueError(f"Unexpected native output line: {raw_line!r}")
        _, check, severity, category, line_text, path_hex, message_hex, help_hex = parts
        issues.append(
            DoctorIssue(
                check=check,
                severity=severity,
                message=_hex_decode(message_hex),
                path=_hex_decode(path_hex),
                category=category,
                help=_hex_decode(help_hex),
                line=int(line_text),
            )
        )
    return issues


def run_native_static_checks(requested_rules: set[str]) -> list[DoctorIssue] | None:
    """Run the native sidecar for supported rules, or return None on fallback."""
    if not requested_rules:
        return []
    if not _native_enabled():
        _set_last_native_reason("native disabled by FASTAPI_DOCTOR_DISABLE_NATIVE")
        return None

    binary = _resolve_binary()
    if binary is None:
        return None

    modules = project.parsed_python_modules()
    with NamedTemporaryFile("w", encoding="utf-8", delete=False) as request_file:
        request_path = Path(request_file.name)
        request_file.write("VERSION\t1\n")
        request_file.write(f"CONFIG\tIMPORT_BLOAT_THRESHOLD\t{project._IMPORT_BLOAT_THRESHOLD}\n")
        for rule_id in sorted(requested_rules):
            request_file.write(f"RULE\t{rule_id}\n")
        for module in modules:
            request_file.write(f"MODULE\t{_hex_encode(module.rel_path)}\t{_hex_encode(module.source)}\n")

    try:
        proc = subprocess.run(
            [str(binary), str(request_path)],
            capture_output=True,
            text=True,
        )
    finally:
        request_path.unlink(missing_ok=True)

    if proc.returncode != 0:
        _set_last_native_reason(f"native execution failed with exit code {proc.returncode}")
        return None

    try:
        return _parse_output(proc.stdout)
    except Exception:
        _set_last_native_reason("native output parse failure")
        return None


__all__ = [
    "NATIVE_STATIC_RULES",
    "last_native_reason",
    "run_native_static_checks",
]
