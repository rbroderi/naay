"""Python-friendly, typed surface for the native naay YAML engine."""

from __future__ import annotations

from typing import Final

REQUIRED_VERSION: Final = "1.0"

USING_PURE_PYTHON = False

try:  # pragma: no cover - exercised indirectly via tests
    import _naay_native as _native  # type: ignore[attr-defined]
except ImportError:  # pragma: no cover - fallback exercised in dedicated tests
    from _naay_pure import parser as _native  # type: ignore[no-redef]

    USING_PURE_PYTHON = True  # pyright: ignore[reportConstantRedefinition]

type YamlValue = str | list["YamlValue"] | dict[str, "YamlValue"]


def loads(text: str, /) -> YamlValue:
    """Parse naay YAML text into nested dict/list structures."""
    return _native.loads(text)  # type: ignore


def dumps(data: YamlValue, /) -> str:
    """Serialize naay-supported objects back to YAML text."""
    return _native.dumps(data)  # type: ignore
