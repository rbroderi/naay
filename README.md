# naay

![PyPI - Python Version](https://img.shields.io/pypi/pyversions/naay?logo=python&label=python)
![License](https://img.shields.io/badge/license-LGPL--3.0--or--later-blue)
![Platforms](https://img.shields.io/badge/os-windows%20%7C%20linux%20%7C%20macOS-brightgreen)
![GitHub release](https://img.shields.io/github/v/release/rbroderi/naay?label=release)
![Dead Code Free](https://img.shields.io/badge/Dead_Code-Free-brightgreen?logo=moleculer&logoColor=white)   
## Vote naay to complicated yaml syntax and parsers.

The intent of this project is to define a tiny strict YAML subset where **all values are strings**, plus support for `|` block literals,
anchors, merges, and YAML-compatible single-line comments — implemented with a Rust core and a
Python binding. Standalone `# ...` lines and inline comments attached to mappings/sequences are
retained in their original positions when you round-trip through the Rust core. Speed of parsing and dumping is a chief concern. As well as a very small syntax.
The syntax used retains full compatablility with yaml while only supporting a very limited subset of yaml.
Good for configs or basic human editable data transfer.

This version enforces a root `_naay_version` field that must match the current release string:

```yaml
_naay_version: "1.0"
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

## Benchmarks

Numbers below were collected on Windows 11 (Ryzen 9 7950X / Python 3.13.2) using
`uv run python examples/demo.py` (which exercises the real YAML fixtures) and a
small synthetic benchmark (snippet shown later). Each value is the average wall
clock time per operation.

### `examples/stress_test0.yaml` (2,000 runs)

| Engine              | Load avg (ms) | Dump avg (ms) | Relative to `naay (native)` |
|---------------------|---------------|---------------|-----------------------------|
| `naay (native)`     | **0.09**      | **0.04**      | baseline                    |
| `naay (pure-python)`| 0.64          | 0.29          | ~7× slower loads, ~7× slower dumps (still >10× faster than PyYAML loads) |
| PyYAML `safe_*`     | 7.53          | 5.01          | ~84× slower loads, ~125× slower dumps |
| `ruamel.yaml` (safe)| 1.59          | 2.34          | ~18× slower loads, ~58× slower dumps |

### `examples/stress_test1.yaml` (80 runs, deeply nested)

| Engine              | Load avg (ms) | Dump avg (ms) | Notes |
|---------------------|---------------|---------------|-------|
| `naay (native)`     | **2.62**      | **3.85**      | baseline |
| `naay (pure-python)`| 5.35          | 4.69          | ~2× slower loads, ~1.2× slower dumps; still stack-safe |
| PyYAML `safe_*`     | fail          | fail          | hit Python recursion depth on the first iteration |
| `ruamel.yaml` (safe)| 18.82         | fail          | ~7× slower on load; dump also exceeded recursion depth |

The pure-Python rows above use `_naay_pure.parser`, the fallback shipped alongside the wheel
for platforms where compiling the Rust extension is not possible.

### Synthetic dense map (1,500 flat scalars, 200 runs)

| Engine              | Load avg (ms) | Dump avg (ms) |
|---------------------|---------------|---------------|
| `naay (native)`     | **0.63**      | **0.45**      |
| `naay (pure-python)`| 4.16          | 2.06          |
| PyYAML `safe_*`     | 51.90         | 32.49         |
| `ruamel.yaml` (safe)| 15.06         | 28.31         |

Even on this uniform synthetic workload, `naay (native)` loads about **82× faster than
PyYAML** and **24× faster than ruamel**, while dumping is **72× faster** than PyYAML and
**62× faster** than ruamel. The pure-Python fallback still loads ~12× faster than PyYAML and
dumps ~16× faster, so the all-Python wheel remains viable when the Rust extension is unavailable.

The synthetic numbers come from `examples/synthetic_dense_bench.py`. Reproduce them with:

```bash
uv run python examples/synthetic_dense_bench.py --runs 200 --keys 1500
```

The helper accepts `--runs` and `--keys` flags if you want to probe different shapes or
shorter smoke tests.

## Spec v1.0

### Required Preamble
- The document root must be a mapping containing `_naay_version: "1.0"` as its first key.
- No other document-level metadata or directives are permitted.

### Scalars
- Every non-block scalar is interpreted as a UTF-8 string; numbers/booleans are not auto-coerced.
- Quoted scalars may use single or double quotes; escaping follows standard YAML rules.
- Multiline content is emitted and parsed via the `|` block literal style only; folded scalars (`>`) are not allowed.
- Trailing whitespace is preserved inside quoted and block scalars but trimmed for bare scalars.

### Sequences
- Denoted with `-` items at consistent indentation; nested collections are indented by two spaces.
- Empty sequences are serialized as `[]` and parsed equivalently anywhere (top-level, nested, inline).
- Inline sequences (`[a, b]`) are not part of the subset; use block form instead.

### Mappings
- Keys must be plain strings; quoting is required when keys contain whitespace or reserved characters `:#?`.
- Empty mappings serialize as `{}` and parse equivalently at any depth.
- Inline mappings (`{a: b}`) are allowed only for a single key/value emitted inline after `- key:`; multi-key inline maps are parsed but immediately expanded into block form.

### Anchors and Aliases
- Anchors are declared via `&name` preceding a nested block; aliases via `*name` anywhere a value is allowed.
- The merge key `<<` supports alias merging; merged values must themselves be mappings.
- Anchors cannot reference scalars that lack a nested block (mirrors YAML behavior).

### Comments
- Full-line comments begin with `#` at any indentation; these are preserved when using the Rust dumper.
- Inline comments (after content, starting with `#`) are preserved when associated with sequences/mappings.
- Comments are dropped when parsing through the Python API (which returns `dict`/`list`/`str`).

### Indentation and Formatting
- Only spaces are allowed; tabs cause a parse error.
- Indentation increments must be exactly two spaces for nested blocks.
- Empty lines are discarded; trailing whitespace on content lines is trimmed before parsing.

### Serialization Guarantees
- Empty lists/maps always emit as `[]`/`{}` so downstream tools can distinguish them from empty strings.
- Scalars containing newlines are emitted as `|` blocks with consistent two-space indentation.
- The dumper preserves comment placement, anchor structure, and ordering of keys/sequences as supplied.
