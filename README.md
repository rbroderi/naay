# naay

A tiny YAML-like subset where **all values are strings**, plus support for `|` block literals,
anchors, merges, and YAML-compatible single-line comments — implemented with a Rust core and a
Python binding. Standalone `# ...` lines and inline comments attached to mappings/sequences are
retained in their original positions when you round-trip through the Rust core.

This version enforces a Semantic Date Versioning field at the root:

```yaml
_naay_version: "2025.12.03-0"
```

> **Note:** The Python binding exposes plain `dict` / `list` / `str` objects, so comment metadata
> is dropped as soon as you convert the parsed tree into native Python types. Use the Rust API
> directly if you need to preserve comments while mutating the tree.

## Layout

- `naay-core/` – Rust library that parses/dumps the restricted YAML subset.
- `naay-py/` – Python extension module using `pyo3` that exposes `loads` / `dumps`.
- `examples/` – Example YAML and Python usage.

## Building (with maturin)

1. Install Rust and maturin:

   ```bash
   pip install maturin
   ```

2. Build and install the Python extension (from `naay-py` directory):

   ```bash
   cd naay-py
   maturin develop
   ```

   This will build the `naay` Python module in your current environment.

## Python usage

```python
import naay
from pathlib import Path

text = Path("examples/campaign.yaml").read_text(encoding="utf-8")
data = naay.loads(text)
print("Loaded data:", data)

out = naay.dumps(data)
print("Round-tripped YAML:")
print(out)
```
