"""Basic smoke tests that parse bundled YAML fixtures."""

from __future__ import annotations

import pathlib

import naay


def test_stress_fixture_parses() -> None:
    fixture = (
        pathlib.Path(__file__).resolve().parent.parent
        / "examples"
        / "stress_test0.yaml"
    )
    text = fixture.read_text(encoding="utf-8")
    data = naay.loads(text)
    assert "npc" in data
    assert isinstance(data["npc"], list)
