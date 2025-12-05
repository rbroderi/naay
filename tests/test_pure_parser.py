"""Tests for the pure-Python fallback parser."""

from __future__ import annotations

# mypy: disable-error-code="import"
import pathlib

import pytest

from _naay_pure import parser


def _fixture_text(name: str) -> str:
    root = pathlib.Path(__file__).resolve().parent.parent
    return (root / "examples" / name).read_text(encoding="utf-8")


def test_pure_parser_matches_fixture() -> None:
    text = _fixture_text("stress_test0.yaml")
    data = parser.loads(text)
    assert "campaign" in data
    dumped = parser.dumps(data)
    assert parser.loads(dumped) == data


def test_missing_version_is_rejected() -> None:
    text = 'defaults:\n  value: "1"\n'
    with pytest.raises(parser.NaayParseError):
        parser.loads(text)
