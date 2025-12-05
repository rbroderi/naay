from __future__ import annotations

import textwrap
from typing import Any

import naay


def test_empty_lists_and_maps_round_trip() -> None:
    yaml_text = textwrap.dedent(
        """
        _naay_version: "1.0"
        empty_list: []
        empty_map: {}
        seq:
          - []
          - {}
        mapping:
          nested_list: []
          nested_map: {}
        """,
    ).lstrip()
    parsed = naay.loads(yaml_text)

    expected: dict[str, Any] = {
        "_naay_version": "1.0",
        "empty_list": [],
        "empty_map": {},
        "seq": [[], {}],
        "mapping": {
            "nested_list": [],
            "nested_map": {},
        },
    }

    assert parsed == expected

    dumped = naay.dumps(expected)

    assert "empty_list: []" in dumped
    assert "empty_map: {}" in dumped
    assert "- []" in dumped
    assert "- {}" in dumped
    assert "nested_list: []" in dumped
    assert "nested_map: {}" in dumped

    reparsed = naay.loads(dumped)
    assert reparsed == expected
