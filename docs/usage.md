# Usage

This page mirrors the most common tasks highlighted in the README, focusing on the `naay` API.

## Install

```bash
uv pip install naay
```

For local development inside the repo:

```bash
uv pip install -e .
```

## Load & Dump YAML

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

## CLI Utilities

### Validate / Round-Trip

```bash
uv run python - <<'PY'
import pathlib
import naay

text = pathlib.Path("examples/stress_test0.yaml").read_text(encoding="utf-8")
data = naay.loads(text)
path = pathlib.Path("/tmp/roundtrip.yaml")
path.write_text(naay.dumps(data), encoding="utf-8")
print("Wrote", path)
PY
```

### Benchmark vs PyYAML / ruamel.yaml

```bash
uv run python examples/demo.py
```

This script repeatedly loads and dumps `examples/stress_test0.yaml` and
`examples/stress_test1.yaml`, printing average timings similar to those shown in the README
benchmark tables.
