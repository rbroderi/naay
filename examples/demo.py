import io
import math
import os
import pathlib
import subprocess
import sys
from collections.abc import Callable
from dataclasses import dataclass
from time import perf_counter
from typing import Any
from typing import TextIO

import ruamel.yaml
import yaml as pyyaml
from ruamel.yaml.comments import CommentedMap
from ruamel.yaml.comments import CommentedSeq

import naay
from _naay_pure import parser as naay_pure

RUNS = 2000
TARGETS: tuple[dict[str, str | int | bool], ...] = (
    {"filename": "stress_test0.yaml", "runs": RUNS, "probe_naay": False},
    {
        "filename": "stress_test1.yaml",
        "runs": max(5, RUNS // 25),
        "probe_naay": True,
    },
)


@dataclass(frozen=True)
class NaayVariant:
    """
    A variant of the naay YAML parser for benchmarking.

    :param label: The display name for this variant.
    :type label: str
    :param loads: Function to parse YAML text into Python objects.
    :type loads: Callable[[str], Any]
    :param dumps: Function to serialize Python objects to YAML text.
    :type dumps: Callable[[Any], str]
    :param module_path: The Python module path for this variant.
    :type module_path: str
    """

    label: str
    loads: Callable[[str], Any]
    dumps: Callable[[Any], str]
    module_path: str


@dataclass(slots=True)
class NaayResult:
    """
    Result container for a naay variant benchmark run.

    :param variant: The naay variant that was benchmarked.
    :type variant: NaayVariant
    :param data: The parsed YAML data, or None if parsing failed.
    :type data: Any | None
    :param dump_sample: A sample of the dumped YAML output, or None if dumping failed.
    :type dump_sample: str | None
    """

    variant: NaayVariant
    data: Any | None = None
    dump_sample: str | None = None


def _build_naay_variants() -> tuple[NaayVariant, ...]:
    variants: list[NaayVariant] = []
    if not naay.USING_PURE_PYTHON:
        variants.append(
            NaayVariant(
                label="naay (native)",
                loads=naay.loads,
                dumps=naay.dumps,
                module_path="naay",
            ),
        )
    variants.append(
        NaayVariant(
            label="naay (pure-python)",
            loads=naay_pure.loads,
            dumps=naay_pure.dumps,
            module_path="_naay_pure.parser",
        ),
    )
    return tuple(variants)


NAAY_VARIANTS: tuple[NaayVariant, ...] = _build_naay_variants()


def _naay_supported_for(path: pathlib.Path, *, module_path: str, label: str) -> bool:
    """Check whether a given naay implementation can load the file."""
    script = (
        "import importlib, pathlib, sys;"
        "text = pathlib.Path(sys.argv[1]).read_text(encoding='utf-8');"
        "module = importlib.import_module(sys.argv[2]);"
        "module.loads(text)"
    )
    try:
        completed = subprocess.run(
            [sys.executable, "-c", script, str(path), module_path],
            capture_output=True,
            text=True,
            check=False,
        )
    except OSError as exc:  # pragma: no cover - environment specific
        print(f"Warning: could not probe {label} for {path.name}: {exc}")
        return True
    if completed.returncode == 0:
        return True
    print(
        f"Skipping {label} benchmarks for {path.name}: pre-check exited "
        f"with code {completed.returncode}",
    )
    if completed.stdout.strip():
        print(f"{label} probe stdout:", completed.stdout.strip())
    if completed.stderr.strip():
        print(f"{label} probe stderr:", completed.stderr.strip())
    return False


def _read_yaml_text(path: pathlib.Path) -> str:
    """Read the YAML file, ensuring the handle is closed each time."""
    with path.open("r", encoding="utf-8") as handle:
        return handle.read()


def _time_repeated_loads(
    label: str,
    loader: Callable[[str], Any],
    *,
    path: pathlib.Path,
    runs: int,
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
    label: str,
    dumper: Callable[[Any, TextIO], None],
    data: Any,
    runs: int,
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


def _wrap_text_dumper(
    dumps_func: Callable[[Any], str],
) -> Callable[[Any, TextIO], None]:
    def _writer(data: Any, stream: TextIO) -> None:
        stream.write(dumps_func(data))

    return _writer


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
    naay_results: list[NaayResult] = []
    for variant in NAAY_VARIANTS:
        if probe_naay and not _naay_supported_for(
            yaml_path,
            module_path=variant.module_path,
            label=variant.label,
        ):
            naay_results.append(NaayResult(variant=variant))
            continue

        print(f"\n=== {variant.label} loads ===")
        naay_data, elapsed = _time_repeated_loads(
            f"{variant.label} loads",
            variant.loads,
            path=yaml_path,
            runs=runs,
        )
        timings.append((f"{variant.label} loads", elapsed))
        result = NaayResult(variant=variant, data=naay_data)

        if naay_data is None:
            print(f"\n=== {variant.label} dumps round-trip ===")
            print(f"{variant.label} dumps skipped: load failed")
            timings.append((f"{variant.label} dumps", math.inf))
            naay_results.append(result)
            continue

        print(f"\n=== {variant.label} dumps round-trip ===")
        naay_dump, dump_elapsed = _time_repeated_dumps(
            f"{variant.label} dumps",
            _wrap_text_dumper(variant.dumps),
            naay_data,
            runs,
        )
        result.dump_sample = naay_dump
        timings.append((f"{variant.label} dumps", dump_elapsed))
        naay_results.append(result)

    print("\n=== PyYAML safe_load ===")
    pyyaml_data, elapsed = _time_repeated_loads(
        "PyYAML safe_load",
        pyyaml.safe_load,
        path=yaml_path,
        runs=runs,
    )
    timings.append(("PyYAML safe_load", elapsed))
    # pprint(pyyaml_data)

    print("\n=== PyYAML safe_dump ===")
    if pyyaml_data is not None:
        _pyyaml_dump, elapsed = _time_repeated_dumps(
            "PyYAML safe_dump",
            _pyyaml_dump_to_stream,
            pyyaml_data,
            runs,
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
    if naay_results:
        for result in naay_results:
            label = result.variant.label
            if isinstance(result.data, dict):
                keys: list[Any] = sorted(result.data.keys())  # type: ignore[arg-type]
                print(f"{label} top-level keys:", keys)
            else:
                print(f"{label} top-level keys: skipped")
            if result.dump_sample is not None and result.data is not None:
                try:
                    data: Any = result.data
                    stable = result.variant.loads(result.dump_sample) == data
                except (
                    Exception  # noqa: BLE001
                ) as exc:  # pragma: no cover - defensive guard
                    print(f"{label} round-trip stable: error ({exc})")
                else:
                    print(f"{label} round-trip stable: {stable}")
            else:
                print(f"{label} round-trip stable: skipped")
    else:
        print("naay benchmarks skipped for this file")
    print(f"\n=== Timing summary for {yaml_path.name} (ms) ===")
    for label, elapsed in timings:
        suffix = f" (avg over {runs})"
        display = "infinity" if math.isinf(elapsed) else f"{elapsed * 1000:.2f}"
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
