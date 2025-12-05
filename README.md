# naay
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

## Spec

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