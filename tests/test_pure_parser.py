"""Tests for the pure-Python fallback parser."""

from __future__ import annotations

# mypy: disable-error-code="import"
import pathlib
import textwrap
from typing import Any

import pytest

from _naay_pure import parser  # noqa: PLC2701


def _load_yaml(text: str) -> dict[str, Any]:
    data = parser.loads(text)
    assert isinstance(data, dict)
    return data


def _fixture_text(name: str) -> str:
    root = pathlib.Path(__file__).resolve().parent.parent
    return (root / "examples" / name).read_text(encoding="utf-8")


def test_pure_parser_matches_fixture() -> None:
    text = _fixture_text("stress_test0.yaml")
    data = _load_yaml(text)
    assert "campaign" in data
    dumped = parser.dumps(data)
    assert parser.loads(dumped) == data


def test_missing_version_is_rejected() -> None:
    text = 'defaults:\n  value: "1"\n'
    with pytest.raises(parser.NaayParseError):
        parser.loads(text)


def test_block_scalar_round_trip() -> None:
    yaml_text = textwrap.dedent(
        """
                _naay_version: "1.0"
                script: |
                    echo one
                    echo two
                """,
    ).strip()
    data = _load_yaml(yaml_text)
    assert data["script"] == "echo one\necho two"

    dumped = parser.dumps(data)
    assert parser.loads(dumped) == data


def test_anchor_and_alias_sequence_round_trip() -> None:
    yaml_text = textwrap.dedent(
        """
                _naay_version: "1.0"
                seq:
                    - &base
                        role: hero
                        hp: "40"
                    - *base
                """,
    ).strip()
    data = _load_yaml(yaml_text)
    assert data["seq"][0] == data["seq"][1]
    assert data["seq"][0] is not data["seq"][1]


def test_merge_key_merges_mappings() -> None:
    yaml_text = textwrap.dedent(
        """
                _naay_version: "1.0"
                defaults: &defs
                    hp: "10"
                    mana: "5"
                encounter:
                    <<: *defs
                    hp: "20"
                    name: ogre
                """,
    ).strip()
    data = _load_yaml(yaml_text)
    encounter = data["encounter"]
    assert isinstance(encounter, dict)
    assert encounter == {"hp": "20", "mana": "5", "name": "ogre"}


def test_inline_map_in_sequence() -> None:
    yaml_text = textwrap.dedent(
        """
                _naay_version: "1.0"
                seq:
                    - item: potion
                        qty: "2"
                    - ability: shield
                """,
    ).strip()
    data = _load_yaml(yaml_text)
    assert data["seq"] == [
        {"item": "potion", "qty": "2"},
        {"ability": "shield"},
    ]


def test_anchor_without_nested_value_errors() -> None:
    yaml_text = textwrap.dedent(
        """
                _naay_version: "1.0"
                seq:
                    - &dangling
                """,
    ).strip()
    with pytest.raises(parser.NaayParseError, match="anchor without nested value"):
        parser.loads(yaml_text)


def test_unknown_anchor_reference_errors() -> None:
    yaml_text = textwrap.dedent(
        """
                _naay_version: "1.0"
                value: *missing
                """,
    ).strip()
    with pytest.raises(parser.NaayParseError, match="unknown anchor"):
        parser.loads(yaml_text)
