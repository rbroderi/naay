# Installation

## From PyPI

```bash
pip install naay
```

This installs the published wheel or source distribution built by our release workflow. A Rust
compiler is **not** required unless pip falls back to building from source on an unsupported
platform.

## From Source

Clone the repository and install the Python package plus its dependencies using
[`uv`](https://github.com/astral-sh/uv) or `pip`:

```bash
git clone https://github.com/rbroderi/naay.git
cd naay
uv pip install -e .[dev]
```

The editable install gives you access to the CLI utilities, the pure-Python fallback module, and the
Rust-backed extension compiled via `maturin`.

## Building Wheels Manually

If you need to produce wheels by hand (e.g., for offline distribution), install `maturin` and run:

```bash
uv tool install maturin
maturin build --release
```

The artifacts will be placed in the `target/wheels/` directory. You can then install them with
`pip install target/wheels/<wheel>.whl`.
