import io
import time
from collections.abc import Callable
from typing import Any

import ruamel.yaml
import yaml as pyyaml

import naay
from _naay_pure import parser as naay_pure  # noqa: PLC2701

RUNS = 200
KEYS = 1_500
DOC = '_naay_version: "1.0"\n' + "\n".join(f'key{i}: "{i}"' for i in range(KEYS))

ruamel_loader = ruamel.yaml.YAML(typ="safe")
ruamel_dumper = ruamel.yaml.YAML(typ="safe")


def bench_load(fn: Callable[[], object]) -> float:
    start = time.perf_counter()
    for _ in range(RUNS):
        fn()
    return (time.perf_counter() - start) / RUNS


def bench_dump(fn: Callable[[], object]) -> float:
    start = time.perf_counter()
    for _ in range(RUNS):
        fn()
    return (time.perf_counter() - start) / RUNS


naay_data = naay.loads(DOC)
naay_pure_data = naay_pure.loads(DOC)
pyyaml_data = pyyaml.safe_load(DOC)
ruamel_data: Any = ruamel_loader.load(DOC)  # pyright: ignore[reportUnknownMemberType, reportUnknownVariableType]

print("naay.loads", bench_load(lambda: naay.loads(DOC)))
print("naay.dumps", bench_dump(lambda: naay.dumps(naay_data)))
print("naay_pure.loads", bench_load(lambda: naay_pure.loads(DOC)))
print("naay_pure.dumps", bench_dump(lambda: naay_pure.dumps(naay_pure_data)))
print("PyYAML safe_load", bench_load(lambda: pyyaml.safe_load(DOC)))
print("PyYAML safe_dump", bench_dump(lambda: pyyaml.safe_dump(pyyaml_data)))
print("ruamel safe_load", bench_load(lambda: ruamel_loader.load(DOC)))  # pyright: ignore[reportUnknownMemberType, reportUnknownLambdaType]
print(
    "ruamel safe_dump",
    bench_dump(lambda: ruamel_dumper.dump(ruamel_data, io.StringIO())),  # pyright: ignore[reportUnknownMemberType, reportUnknownLambdaType]
)
