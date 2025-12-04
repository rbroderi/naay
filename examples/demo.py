import io
import math
import os
import pathlib
import subprocess
import sys
from collections.abc import Callable
from time import perf_counter
from typing import Any, TextIO

import ruamel.yaml
import yaml as pyyaml
from ruamel.yaml.comments import CommentedMap, CommentedSeq

import naay

RUNS = 500
TARGETS: tuple[dict[str, str | int | bool], ...] = (
    {"filename": "stress_test0.yaml", "runs": RUNS, "probe_naay": False},
    {
        "filename": "stress_test1.yaml",
        "runs": max(5, RUNS // 25),
        "probe_naay": True,
    },
)


def _naay_supported_for(path: pathlib.Path) -> bool:
    """Check whether naay can load the file in a separate process."""

    script = (
        "import pathlib, naay, sys;"
        "text = pathlib.Path(sys.argv[1]).read_text(encoding='utf-8');"
        "naay.loads(text)"
    )
    try:
        completed = subprocess.run(
            [sys.executable, "-c", script, str(path)],
            capture_output=True,
            text=True,
            check=False,
        )
    except OSError as exc:  # pragma: no cover - environment specific
        print(f"Warning: could not probe naay for {path.name}: {exc}")
        return True
    if completed.returncode == 0:
        return True
    print(
        f"Skipping naay benchmarks for {path.name}: pre-check exited "
        + f"with code {completed.returncode}"
    )
    if completed.stdout.strip():
        print("naay probe stdout:", completed.stdout.strip())
    if completed.stderr.strip():
        print("naay probe stderr:", completed.stderr.strip())
    return False


def _read_yaml_text(path: pathlib.Path) -> str:
    """Read the YAML file, ensuring the handle is closed each time."""

    with path.open("r", encoding="utf-8") as handle:
        return handle.read()


def _time_repeated_loads(
    label: str, loader: Callable[[str], Any], *, path: pathlib.Path, runs: int
) -> tuple[Any | None, float]:
    """Run loader(text) ``runs`` times, reopening the file for every iteration."""

    total = 0.0
    result = None
    for iteration in range(runs):
        text = _read_yaml_text(path)
        start = perf_counter()
        try:
            result = loader(text)
            total += perf_counter() - start
        except Exception as exc:  # pragma: no cover - stress fallback
            print(f"{label} failed on iteration {iteration + 1}: {exc}")
            return None, math.inf
    avg = total / runs
    print(f"{label} average over {runs} runs: {avg * 1000:.2f} ms")
    return result, avg


def _time_repeated_dumps(
    label: str, dumper: Callable[[Any, TextIO], None], data: Any, runs: int
) -> tuple[str | None, float]:
    """Run dumper(data, stream) ``runs`` times, writing to os.devnull each time."""

    total = 0.0
    for iteration in range(runs):
        with open(os.devnull, "w", encoding="utf-8") as handle:
            start = perf_counter()
            try:
                dumper(data, handle)
                total += perf_counter() - start
            except Exception as exc:  # pragma: no cover - stress fallback
                print(f"{label} failed on iteration {iteration + 1}: {exc}")
                return None, math.inf

    sample_buffer = io.StringIO()
    dumper(data, sample_buffer)
    sample = sample_buffer.getvalue()

    avg = total / runs
    print(f"{label} average over {runs} runs: {avg * 1000:.2f} ms")
    return sample, avg


def _naay_dump_to_stream(data: Any, stream: TextIO) -> None:
    stream.write(naay.dumps(data))


def _pyyaml_dump_to_stream(data: Any, stream: TextIO) -> None:
    pyyaml.safe_dump(data, stream)


def _as_plain(value: Any) -> Any:
    """Convert ruamel Commented* containers into plain Python types."""

    if isinstance(value, CommentedMap):
        return {k: _as_plain(v) for k, v in value.items()}  # type: ignore[misc, no-any-return]
    if isinstance(value, CommentedSeq):
        return [_as_plain(v) for v in value]  # pyright: ignore[reportUnknownVariableType]
    if isinstance(value, dict):
        return {k: _as_plain(v) for k, v in value.items()}  # pyright: ignore[reportUnknownVariableType]
    if isinstance(value, list):
        return [_as_plain(v) for v in value]  # pyright: ignore[reportUnknownVariableType]
    return value


def _benchmark_file(yaml_path: pathlib.Path, runs: int, probe_naay: bool) -> None:
    print(f"\n===== Benchmarking {yaml_path.name} =====")
    timings: list[tuple[str, float]] = []
    naay_data = None
    naay_dump = None
    naay_enabled = True
    if probe_naay and not _naay_supported_for(yaml_path):
        naay_enabled = False

    if naay_enabled:
        print("=== naay.loads ===")
        naay_data, elapsed = _time_repeated_loads(
            "naay.loads", naay.loads, path=yaml_path, runs=runs
        )
        timings.append(("naay.loads", elapsed))
        # pprint(naay_data)

        print("\n=== naay.dumps round-trip ===")
        naay_dump, elapsed = _time_repeated_dumps(
            "naay.dumps", _naay_dump_to_stream, naay_data, runs
        )
        timings.append(("naay.dumps", elapsed))
        # pprint(naay_dump)
    else:
        print("naay benchmarks skipped for this file")

    print("\n=== PyYAML safe_load ===")
    pyyaml_data, elapsed = _time_repeated_loads(
        "PyYAML safe_load", pyyaml.safe_load, path=yaml_path, runs=runs
    )
    timings.append(("PyYAML safe_load", elapsed))
    # pprint(pyyaml_data)

    print("\n=== PyYAML safe_dump ===")
    if pyyaml_data is not None:
        _pyyaml_dump, elapsed = _time_repeated_dumps(
            "PyYAML safe_dump", _pyyaml_dump_to_stream, pyyaml_data, runs
        )
    else:
        print("PyYAML safe_dump skipped: load failed")
        _pyyaml_dump, elapsed = None, math.inf
    timings.append(("PyYAML safe_dump", elapsed))
    # print(pyyaml_dump)

    print("\n=== ruamel.yaml (safe) ===")
    ruamel_loader = ruamel.yaml.YAML(typ="safe")
    ruamel_data, elapsed = _time_repeated_loads(
        "ruamel safe_load",
        ruamel_loader.load,  # type: ignore
        path=yaml_path,
        runs=runs,
    )
    timings.append(("ruamel safe_load", elapsed))
    # pprint(_as_plain(ruamel_data))

    print("\n=== ruamel.yaml dump (safe) ===")
    ruamel_dumper = ruamel.yaml.YAML(typ="safe")
    if ruamel_data is not None:
        _ruamel_dump, elapsed = _time_repeated_dumps(
            "ruamel safe_dump",
            ruamel_dumper.dump,  # type: ignore
            ruamel_data,
            runs,
        )
    else:
        print("ruamel safe_dump skipped: load failed")
        _ruamel_dump, elapsed = None, math.inf
    timings.append(("ruamel safe_dump", elapsed))
    # print(ruamel_dump)

    print("\n=== Comparison summary ===")
    ruamel_plain = None
    if ruamel_data is not None:
        try:
            ruamel_plain = _as_plain(ruamel_data)
        except RecursionError:
            print("ruamel flatten skipped: recursion depth exceeded")
    pyyaml_plain = _as_plain(pyyaml_data) if pyyaml_data is not None else None
    print(
        "PyYAML matches ruamel safe output:",
        ruamel_plain == pyyaml_plain
        if None not in (ruamel_plain, pyyaml_plain)
        else "skipped",
    )
    if naay_data is not None:
        print("naay top-level keys:", sorted(naay_data.keys()))
    else:
        print("naay top-level keys: skipped")
    if naay_dump is not None and naay_data is not None:
        print("naay round-trip stable:", naay.loads(naay_dump) == naay_data)
    else:
        print("naay round-trip stable: skipped")
    print(f"\n=== Timing summary for {yaml_path.name} (ms) ===")
    for label, elapsed in timings:
        suffix = f" (avg over {runs})"
        if math.isinf(elapsed):
            display = "infinity"
        else:
            display = f"{elapsed * 1000:.2f}"
        print(f"{label:>20}: {display}{suffix}")


def main() -> None:
    here = pathlib.Path(__file__).resolve().parent
    for target in TARGETS:
        filename = target["filename"]
        runs = target["runs"]
        probe_naay = target["probe_naay"]
        yaml_path = here / str(filename)
        if not yaml_path.exists():
            print(f"Skipping {filename}: file not found")
            continue
        _benchmark_file(yaml_path, int(runs), bool(probe_naay))


if __name__ == "__main__":
    main()
