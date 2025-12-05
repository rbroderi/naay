# API Reference

The Python package exposes a very small surface area that mirrors the Rust core. Install it in editable mode so mkdocstrings can import the latest sources:

```bash
uv pip install -e .
```

## Python Module: `naay`

```python
from __future__ import annotations

from typing import Final

REQUIRED_VERSION: Final = "1.0"
USING_PURE_PYTHON: bool
type YamlValue = str | list["YamlValue"] | dict[str, "YamlValue"]

def loads(text: str, /) -> YamlValue:
	"""Parse naay YAML into nested dict/list/string structures."""

def dumps(data: YamlValue, /) -> str:
	"""Serialize a naay-compatible tree back to YAML text."""
```

- `REQUIRED_VERSION` is the string that every document must declare in `_naay_version`.
- `USING_PURE_PYTHON` is `True` when the fallback parser is active (native module missing).
- `loads`/`dumps` delegate to the native Rust extension when available; otherwise they use the pure-Python implementation located in `src/_naay_pure/parser.py`.

### mkdocstrings hook

You can still rely on mkdocstrings to render detailed docstrings for the module:

::: naay

## Rust Core Highlights

The Rust crate `naay-core` underpins both the native extension and the documentation’s feature set:

- `YamlValue` enum captures the three node kinds (`Str`, `Seq`, `Map`).
- `YamlNode` wraps each value with comment metadata so the dumper can round-trip formatting.
- `parse_naay(&str) -> Result<YamlValue, ParseError>` enforces `_naay_version` and whitespace rules.
- `dump_naay(&YamlValue) -> Result<String, DumpError>` emits deterministic YAML, using `[]`/`{}` for empty collections.

When contributing Rust examples to the docs, prefer importing from `naay-core` and referencing the same `REQUIRED_VERSION` constant to keep parity with the Python API.
