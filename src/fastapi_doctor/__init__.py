"""Public package API for fastapi-doctor."""

from ._version import version as __version__
from .cli import main
from .native_core import get_native_rule_ids

__all__ = ["__version__", "get_native_rule_ids", "main"]
