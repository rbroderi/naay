"""Python-friendly, typed surface for the native naay YAML engine."""

from __future__ import annotations

from typing import Final
from typing import Protocol
from typing import cast

REQUIRED_VERSION: Final = "1.0"

USING_PURE_PYTHON = False

try:  # pragma: no cover - exercised indirectly via tests
    import _naay_native as _native  # type: ignore[attr-defined]
except ImportError:  # pragma: no cover - fallback exercised in dedicated tests
    from _naay_pure import parser as _native  # type: ignore[no-redef]  # noqa: PLC2701

    USING_PURE_PYTHON = True  # pyright: ignore[reportConstantRedefinition]

type YamlValue = str | list[YamlValue] | dict[str, YamlValue]


class _NativeModule(Protocol):
    def loads(self, text: str, /) -> YamlValue: ...
    def dumps(self, data: YamlValue, /) -> str: ...


_native_typed = cast("_NativeModule", _native)


def loads(text: str, /) -> YamlValue:
    """Parse naay YAML text into nested dict/list structures.

    Returns:
        Parsed YAML data as nested dict/list structures.
    """
    return _native_typed.loads(text)


def dumps(data: YamlValue, /) -> str:
    """Serialize naay-supported objects back to YAML text.

    Returns:
        YAML text representation of the data.
    """
    return _native_typed.dumps(data)
