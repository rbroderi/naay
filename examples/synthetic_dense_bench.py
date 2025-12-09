"""Benchmark naay vs other YAML engines using synthetic dense documents."""

from __future__ import annotations

import argparse
import io
import time
from typing import TYPE_CHECKING

import ruamel.yaml
import yaml as pyyaml

import naay
from _naay_pure import (  # type: ignore[import-untyped]
    parser as naay_pure,  # noqa: PLC2701
)

OK = 0
if TYPE_CHECKING:
    from collections.abc import Callable
    from collections.abc import Sequence


def _build_doc(keys: int) -> str:
    header = '_naay_version: "1.0"\n'
    body = "\n".join(f'key{i}: "{i}"' for i in range(keys))
    return header + body


def _bench_load(
    loader: Callable[[str], object],
    doc: str,
    runs: int,
) -> float:
    start = time.perf_counter()
    for _ in range(runs):
        loader(doc)
    return (time.perf_counter() - start) / runs


def _bench_dump(callback: Callable[[], object], runs: int) -> float:
    start = time.perf_counter()
    for _ in range(runs):
        callback()
    return (time.perf_counter() - start) / runs


def _wrap_text_dump(
    dumps_func: Callable[..., str],
    data: object,
) -> Callable[[], str]:
    return lambda: dumps_func(data)


def _wrap_stream_dump(
    dumps_func: Callable[[object, io.StringIO], object],
    data: object,
) -> Callable[[], object]:
    def _call() -> object:
        buffer = io.StringIO()
        return dumps_func(data, buffer)

    return _call


def _format_line(label: str, seconds: float) -> str:
    return f"{label:<22}: {seconds * 1000:.3f} ms"


def _run_benchmarks(runs: int, keys: int) -> list[str]:
    doc = _build_doc(keys)
    ruamel_loader = ruamel.yaml.YAML(typ="safe")
    ruamel_dumper = ruamel.yaml.YAML(typ="safe")

    naay_data = naay.loads(doc)
    naay_pure_data = naay_pure.loads(doc)
    pyyaml_data = pyyaml.safe_load(doc)
    ruamel_data = ruamel_loader.load(doc)  # type: ignore[arg-type]

    lines: list[str] = [
        f"Runs: {runs}",
        f"Scalar keys: {keys}",
    ]
    lines.extend((
        _format_line("naay.loads", _bench_load(naay.loads, doc, runs)),
        _format_line(
            "naay.dumps",
            _bench_dump(_wrap_text_dump(naay.dumps, naay_data), runs),
        ),
        _format_line("naay_pure.loads", _bench_load(naay_pure.loads, doc, runs)),
        _format_line(
            "naay_pure.dumps",
            _bench_dump(_wrap_text_dump(naay_pure.dumps, naay_pure_data), runs),
        ),
        _format_line("PyYAML safe_load", _bench_load(pyyaml.safe_load, doc, runs)),
        _format_line(
            "PyYAML safe_dump",
            _bench_dump(_wrap_text_dump(pyyaml.safe_dump, pyyaml_data), runs),
        ),
        _format_line(
            "ruamel safe_load",
            _bench_load(ruamel_loader.load, doc, runs),  # type: ignore[arg-type]
        ),
        _format_line(
            "ruamel safe_dump",
            _bench_dump(_wrap_stream_dump(ruamel_dumper.dump, ruamel_data), runs),  # type: ignore[arg-type]
        ),
    ))
    return lines


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Benchmark naay vs other YAML engines")
    parser.add_argument(
        "--runs",
        type=int,
        default=200,
        help="Number of iterations per operation (default: 200)",
    )
    parser.add_argument(
        "--keys",
        type=int,
        default=1_500,
        help="Number of flat scalar keys in the synthetic document (default: 1500)",
    )
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    """Run YAML library benchmarks and print results.

    Parameters
    ----------
    argv : Sequence[str] | None, optional
        Command line arguments. If None, uses sys.argv.

    Returns:
    -------
    int
        Exit code (0 for success).
    """
    parser = _build_parser()
    args = parser.parse_args(argv)
    for line in _run_benchmarks(runs=args.runs, keys=args.keys):
        print(line)
    return OK


if __name__ == "__main__":
    raise SystemExit(main())
