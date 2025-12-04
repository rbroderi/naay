"""Pure-Python fallback implementation of the naay parser/dumper."""

from __future__ import annotations

from .parser import dumps
from .parser import loads

__all__ = ["dumps", "loads"]
