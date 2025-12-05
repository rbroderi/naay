import io
import time

import ruamel.yaml
import yaml as pyyaml

import naay
from _naay_pure import parser as naay_pure

RUNS = 200
KEYS = 1_500
DOC = '_naay_version: "1.0"\n' + "\n".join(f'key{i}: "{i}"' for i in range(KEYS))

ruamel_loader = ruamel.yaml.YAML(typ="safe")
ruamel_dumper = ruamel.yaml.YAML(typ="safe")


def bench_load(fn):
    start = time.perf_counter()
    for _ in range(RUNS):
        fn(DOC)
    return (time.perf_counter() - start) / RUNS


def bench_dump(fn):
    start = time.perf_counter()
    for _ in range(RUNS):
        fn()
    return (time.perf_counter() - start) / RUNS


naay_data = naay.loads(DOC)
naay_pure_data = naay_pure.loads(DOC)
pyyaml_data = pyyaml.safe_load(DOC)
ruamel_data = ruamel_loader.load(DOC)

print("naay.loads", bench_load(naay.loads))
print("naay.dumps", bench_dump(lambda: naay.dumps(naay_data)))
print("naay_pure.loads", bench_load(naay_pure.loads))
print("naay_pure.dumps", bench_dump(lambda: naay_pure.dumps(naay_pure_data)))
print("PyYAML safe_load", bench_load(pyyaml.safe_load))
print("PyYAML safe_dump", bench_dump(lambda: pyyaml.safe_dump(pyyaml_data)))
print("ruamel safe_load", bench_load(ruamel_loader.load))
print(
    "ruamel safe_dump",
    bench_dump(lambda: ruamel_dumper.dump(ruamel_data, io.StringIO())),
)
