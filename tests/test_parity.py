"""Parity tests between native, pure-Python, and ruamel loaders."""

from __future__ import annotations

import io
import pathlib

import pytest
import ruamel.yaml
from ruamel.yaml.comments import CommentedMap, CommentedSeq

import naay
from _naay_pure import parser as pure_parser

try:  # pragma: no cover - executed in native-enabled environments
    import _naay_native as native_module  # type: ignore[attr-defined]
except ImportError:  # pragma: no cover - allows testing pure fallback in CI
    native_module = None


def _fixture_text(name: str) -> str:
    root = pathlib.Path(__file__).resolve().parent.parent
    return (root / "examples" / name).read_text(encoding="utf-8")


def _ruamel_load(text: str):
    loader = ruamel.yaml.YAML(typ="safe")
    return loader.load(text)


def _ruamel_dump(data) -> str:
    dumper = ruamel.yaml.YAML(typ="safe")
    buffer = io.StringIO()
    dumper.dump(data, buffer)
    return buffer.getvalue()


def _to_plain(value):
    if isinstance(value, CommentedMap):
        return {k: _to_plain(v) for k, v in value.items()}
    if isinstance(value, CommentedSeq):
        return [_to_plain(v) for v in value]
    if isinstance(value, dict):
        return {k: _to_plain(v) for k, v in value.items()}
    if isinstance(value, list):
        return [_to_plain(v) for v in value]
    return value


def _stringify_scalars(value):
    if isinstance(value, dict):
        return {k: _stringify_scalars(v) for k, v in value.items()}
    if isinstance(value, list):
        return [_stringify_scalars(v) for v in value]
    if isinstance(value, str):
        return value.rstrip("\n")
    return str(value)


@pytest.mark.skipif(native_module is None, reason="native extension not available")
def test_pure_and_native_match_on_fixture() -> None:
    text = _fixture_text("stress_test0.yaml")
    native_data = native_module.loads(text)
    pure_data = pure_parser.loads(text)

    assert pure_data == native_data

    pure_dump = pure_parser.dumps(native_data)
    native_dump = native_module.dumps(native_data)

    assert native_module.loads(pure_dump) == native_data
    assert pure_parser.loads(native_dump) == pure_data


def test_naay_and_ruamel_round_trip_functional_parity() -> None:
    text = _fixture_text("stress_test0.yaml")

    naay_data = naay.loads(text)

    ruamel_data = _ruamel_load(text)
    ruamel_plain = _stringify_scalars(_to_plain(ruamel_data))

    assert naay_data == ruamel_plain

    naay_dump = naay.dumps(naay_data)
    ruamel_plain_from_naay_dump = _stringify_scalars(_to_plain(_ruamel_load(naay_dump)))
    assert ruamel_plain_from_naay_dump == ruamel_plain
