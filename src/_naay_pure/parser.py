"""Pure-Python implementation of the naay YAML subset."""

from __future__ import annotations

from collections.abc import Sequence
from dataclasses import dataclass
from typing import Final

try:
    from beartype.claw import beartype_this_package

    beartype_this_package()
except ModuleNotFoundError:  # pragma: no cover - optional dependency
    pass

# ruff: noqa: PLR0911
YamlValue = str | list["YamlValue"] | dict[str, "YamlValue"]


REQUIRED_VERSION = "2025.12.03-0"


class NaayParseError(ValueError):
    """Raised when the pure-Python parser encounters invalid input."""


class NaayDumpError(ValueError):
    """Raised when dumping fails due to unsupported types."""


@dataclass(slots=True)
class Line:
    indent: int
    content: str
    line_no: int


def loads(text: str, /) -> YamlValue:
    """Parse naay text into nested dict/list/scalar structures."""
    parser = _Parser(text)
    return parser.parse()


def dumps(data: YamlValue, /) -> str:
    """Serialize a naay-compatible tree back into text."""
    dumper = _Dumper()
    try:
        dumper.write_value(data, 0)
    except TypeError as exc:  # pragma: no cover - defensive guard
        raise NaayDumpError(str(exc)) from exc
    return dumper.render()


class _Parser:
    def __init__(self, text: str) -> None:
        self.lines: list[Line] = self._preprocess(text)
        self.index = 0
        self.anchors: dict[str, YamlValue] = {}

    # Public -----------------------------------------------------------------
    def parse(self) -> YamlValue:
        if not self.lines:
            msg = "missing required _naay_version at root (Semantic Date Versioning)"
            raise NaayParseError(
                msg,
            )
        # Decide whether root is a map or sequence based on the first non-comment line
        first_idx = self._skip_comments(self.index)
        if first_idx >= len(self.lines):
            msg = "missing required _naay_version at root (Semantic Date Versioning)"
            raise NaayParseError(
                msg,
            )
        self.index = first_idx
        first_line = self.lines[self.index]
        base_indent = first_line.indent
        value: YamlValue
        if self._looks_like_seq(first_line):
            value = self._parse_seq(base_indent)
        else:
            value = self._parse_map(base_indent)
        self._enforce_root_version(value, first_line.line_no)
        return value

    # High-level parse helpers ------------------------------------------------
    def _parse_seq(self, base_indent: int) -> list[YamlValue]:
        items: list[YamlValue] = []
        while self.index < len(self.lines):
            line = self.lines[self.index]
            if line.indent < base_indent:
                break
            if line.indent > base_indent:
                break
            if line.content.startswith("#"):
                self.index += 1
                continue
            if not self._looks_like_seq(line):
                break
            body, _ = _split_inline_comment(line.content)
            after_dash = body[1:].lstrip()
            self.index += 1
            items.append(self._parse_seq_value(after_dash, base_indent, line))
        return items

    def _parse_map(self, base_indent: int) -> dict[str, YamlValue]:
        mapping: dict[str, YamlValue] = {}
        while self.index < len(self.lines):
            line = self.lines[self.index]
            if line.indent < base_indent:
                break
            if line.content.startswith("- ") and line.indent == base_indent:
                break
            if line.content.startswith("#"):
                self.index += 1
                continue
            if line.indent > base_indent:
                break

            stripped, _ = _split_inline_comment(line.content)
            colon_pos = stripped.find(":")
            if colon_pos == -1:
                msg = f"expected ':' in mapping entry (line {line.line_no})"
                raise NaayParseError(
                    msg,
                )
            key_raw = stripped[:colon_pos].strip()
            value_raw = stripped[colon_pos + 1 :].lstrip()
            key = _parse_key(key_raw)
            self.index += 1

            if key == "<<" and value_raw.startswith("*"):
                merged = self._resolve_alias(value_raw[1:].strip(), line)
                self._merge_into(mapping, merged, line)
                continue

            mapping[key] = self._parse_map_value(value_raw, base_indent, line)
        return mapping

    # Value handlers ----------------------------------------------------------
    def _parse_seq_value(
        self,
        after_dash: str,
        base_indent: int,
        line: Line,
    ) -> YamlValue:
        if not after_dash:
            if (
                self.index >= len(self.lines)
                or self.lines[self.index].indent <= base_indent
            ):
                return ""
            child_indent = self.lines[self.index].indent
            return self._parse_block(child_indent)
        if after_dash == "|":
            return self._parse_block_scalar(base_indent + 1)
        if after_dash.startswith("&"):
            anchor_name = after_dash[1:].strip()
            child = self._parse_block_required(base_indent, line)
            cloned = _clone_value(child)
            self.anchors[anchor_name] = cloned
            return _clone_value(child)
        if after_dash.startswith("*"):
            return _clone_value(self._resolve_alias(after_dash[1:].strip(), line))
        if ":" in after_dash:
            return self._parse_inline_map(after_dash, base_indent, line)
        return _strip_quotes(after_dash)

    def _parse_map_value(self, vpart: str, base_indent: int, line: Line) -> YamlValue:
        if not vpart:
            if (
                self.index >= len(self.lines)
                or self.lines[self.index].indent <= base_indent
            ):
                return ""
            child_indent = self.lines[self.index].indent
            return self._parse_block(child_indent)
        if vpart == "|":
            return self._parse_block_scalar(base_indent + 1)
        if vpart.startswith("&"):
            anchor_name = vpart[1:].strip()
            child = self._parse_block_required(base_indent, line)
            cloned = _clone_value(child)
            self.anchors[anchor_name] = cloned
            return _clone_value(child)
        if vpart.startswith("*"):
            return _clone_value(self._resolve_alias(vpart[1:].strip(), line))
        return _strip_quotes(vpart)

    def _parse_inline_map(
        self,
        payload: str,
        base_indent: int,
        line: Line,
    ) -> dict[str, YamlValue]:
        colon_pos = payload.find(":")
        if colon_pos == -1:
            msg = f"expected ':' inside inline map (line {line.line_no})"
            raise NaayParseError(
                msg,
            )
        key = _parse_key(payload[:colon_pos].strip())
        remainder = payload[colon_pos + 1 :].lstrip()
        value = self._parse_inline_value(remainder, line, base_indent + 2)
        mapping: dict[str, YamlValue] = {}
        if key == "<<":
            self._merge_into(mapping, value, line)
        else:
            mapping[key] = value
        if self.index < len(self.lines) and self.lines[self.index].indent > base_indent:
            child_indent = self.lines[self.index].indent
            extra = self._parse_map(child_indent)
            mapping.update(extra)
        return mapping

    def _parse_inline_value(
        self,
        vpart: str,
        line: Line,
        expected_indent: int,
    ) -> YamlValue:
        min_quote_len: Final = 2
        if (
            vpart.startswith('"')
            and vpart.endswith('"')
            and len(vpart) >= min_quote_len
        ) or (
            vpart.startswith("'")
            and vpart.endswith("'")
            and len(vpart) >= min_quote_len
        ):
            return _strip_quotes(vpart)
        if vpart == "|":
            return self._parse_block_scalar(expected_indent)
        if vpart.startswith("&"):
            anchor_name = vpart[1:].strip()
            if (
                self.index >= len(self.lines)
                or self.lines[self.index].indent <= expected_indent - 1
            ):
                msg = f"anchor without nested value (line {line.line_no})"
                raise NaayParseError(msg)
            child_indent = self.lines[self.index].indent
            child = self._parse_block(child_indent)
            cloned = _clone_value(child)
            self.anchors[anchor_name] = cloned
            return _clone_value(child)
        if vpart.startswith("*"):
            return _clone_value(self._resolve_alias(vpart[1:].strip(), line))
        return vpart

    # Low-level helpers -------------------------------------------------------
    def _merge_into(
        self,
        target: dict[str, YamlValue],
        value: YamlValue,
        line: Line,
    ) -> None:
        if isinstance(value, list):
            for item in value:
                if not isinstance(item, dict):
                    msg = f"merge list entries must be mappings (line {line.line_no})"
                    raise NaayParseError(msg)
                self._merge_into(target, item, line)
            return
        if not isinstance(value, dict):
            msg = f"merge source must be a mapping (line {line.line_no})"
            raise NaayParseError(msg)
        for mk, mv in value.items():
            target.setdefault(mk, _clone_value(mv))

    def _parse_block(self, base_indent: int) -> YamlValue:
        self.index = self._skip_comments(self.index)
        if self.index >= len(self.lines):
            return ""
        line = self.lines[self.index]
        if line.indent < base_indent:
            return ""
        if self._looks_like_seq(line):
            return self._parse_seq(base_indent)
        return self._parse_map(base_indent)

    def _parse_block_required(self, base_indent: int, line: Line) -> YamlValue:
        if (
            self.index >= len(self.lines)
            or self.lines[self.index].indent <= base_indent
        ):
            msg = f"anchor without nested value (line {line.line_no})"
            raise NaayParseError(msg)
        child_indent = self.lines[self.index].indent
        return self._parse_block(child_indent)

    def _parse_block_scalar(self, min_indent: int) -> str:
        result: list[tuple[str, int]] = []
        while self.index < len(self.lines):
            line = self.lines[self.index]
            if line.indent <= min_indent:
                break
            result.append((line.content, line.indent))
            self.index += 1
        if not result:
            return ""
        min_seen = min(indent for _, indent in result)
        lines: list[str] = []
        for content, indent in result:
            cut = max(indent - min_seen, 0)
            lines.append(content[cut:] if cut < len(content) else "")
        return "\n".join(lines)

    def _resolve_alias(self, name: str, line: Line) -> YamlValue:
        if name not in self.anchors:
            msg = f"unknown anchor '{name}' (line {line.line_no})"
            raise NaayParseError(msg)
        return _clone_value(self.anchors[name])

    def _looks_like_seq(self, line: Line) -> bool:
        return line.content.startswith("-") and (
            len(line.content) == 1 or line.content[1].isspace()
        )

    def _skip_comments(self, start: int) -> int:
        idx = start
        while idx < len(self.lines) and self.lines[idx].content.startswith("#"):
            idx += 1
        return idx

    def _enforce_root_version(self, value: YamlValue, _line_no: int) -> None:
        if not isinstance(value, dict):
            msg = "root of document must be a mapping"
            raise NaayParseError(msg)
        ver = value.get("_naay_version")
        if not isinstance(ver, str):
            msg = "missing required _naay_version at root (Semantic Date Versioning)"
            raise NaayParseError(
                msg,
            )
        if not _validate_version(ver):
            msg = f"invalid _naay_version '{ver}', expected YYYY.MM.DD-REV"
            raise NaayParseError(msg)
        if ver != REQUIRED_VERSION:
            msg = f"unsupported _naay_version '{ver}', expected {REQUIRED_VERSION}"
            raise NaayParseError(msg)

    @staticmethod
    def _preprocess(text: str) -> list[Line]:
        lines: list[Line] = []
        for idx, raw in enumerate(text.splitlines()):
            if "\t" in raw:
                msg = (
                    f"tabs are not allowed; use spaces for indentation (line {idx + 1})"
                )
                raise NaayParseError(msg)
            stripped = raw.rstrip()
            content = stripped.lstrip()
            if not content:
                continue
            indent = len(stripped) - len(content)
            lines.append(Line(indent=indent, content=content, line_no=idx + 1))
        return lines


class _Dumper:
    def __init__(self) -> None:
        self._parts: list[str] = []

    def write_value(self, value: YamlValue, indent: int) -> None:
        if isinstance(value, list):
            self._write_seq(value, indent)
        elif isinstance(value, dict):
            self._write_map(value, indent)
        else:
            self._write_scalar(value, indent)

    def render(self) -> str:
        return "".join(self._parts)

    # Writers ----------------------------------------------------------------
    def _write_scalar(self, value: str, indent: int) -> None:
        if "\n" in value:
            self._parts.append("|")
            self._parts.append("\n")
            for line in value.split("\n"):
                self._parts.append(" " * (indent + 2) + line + "\n")
            return
        escaped = value.replace("\\", "\\\\").replace('"', '\\"')
        self._parts.append(f'"{escaped}"\n')

    def _write_seq(self, seq: Sequence[YamlValue], indent: int) -> None:
        for item in seq:
            prefix = " " * indent + "- "
            self._parts.append(prefix)
            if isinstance(item, str):
                self._write_scalar(item, indent)
            elif isinstance(item, list):
                self._parts.append("\n")
                self._write_seq(item, indent + 2)
            else:
                self._parts.append("\n")
                self._write_map(item, indent + 2)

    def _write_map(self, mapping: dict[str, YamlValue], indent: int) -> None:
        for key, value in mapping.items():
            formatted_key = self._format_key(key)
            prefix = " " * indent + formatted_key + ":"
            if isinstance(value, str):
                self._parts.append(prefix + " ")
                self._write_scalar(value, indent)
            elif isinstance(value, list):
                self._parts.append(prefix + "\n")
                self._write_seq(value, indent + 2)
            else:
                self._parts.append(prefix + "\n")
                self._write_map(value, indent + 2)

    @staticmethod
    def _format_key(key: str) -> str:
        if not key or any(c.isspace() or c in ":#?" for c in key):
            escaped = key.replace("\\", "\\\\").replace('"', '\\"')
            return f'"{escaped}"'
        return key


def _split_inline_comment(line: str) -> tuple[str, str | None]:
    in_single = False
    in_double = False
    escaped = False
    for idx, ch in enumerate(line):
        if ch == "'" and not in_double:
            in_single = not in_single
        elif ch == '"' and not in_single:
            if in_double and not escaped:
                in_double = False
            elif not in_double:
                in_double = True
        elif (
            ch == "#"
            and not in_single
            and not in_double
            and (idx == 0 or line[idx - 1].isspace())
        ):
            return line[:idx].rstrip(), line[idx:]
        if ch == "\\" and in_double and not escaped:
            escaped = True
            continue
        escaped = False
    return line.rstrip(), None


def _parse_key(raw: str) -> str:
    return _strip_quotes(raw)


def _strip_quotes(value: str) -> str:
    min_quote_len: Final = 2
    if (
        value.startswith('"') and value.endswith('"') and len(value) >= min_quote_len
    ) or (
        value.startswith("'") and value.endswith("'") and len(value) >= min_quote_len
    ):
        return value[1:-1]
    return value


def _clone_value(value: YamlValue) -> YamlValue:
    if isinstance(value, dict):
        return {k: _clone_value(v) for k, v in value.items()}
    if isinstance(value, list):
        return [_clone_value(v) for v in value]
    return value


def _validate_version(ver: str) -> bool:
    expected_parts: Final = 2
    expected_date_parts: Final = 3
    year_length: Final = 4
    month_day_length: Final = 2
    min_year: Final = 1970
    max_month: Final = 12
    max_day: Final = 31

    parts = ver.split("-")
    if len(parts) != expected_parts:
        return False
    date_part, rev = parts
    if not rev.isdigit() or not rev:
        return False
    date_bits = date_part.split(".")
    if len(date_bits) != expected_date_parts:
        return False
    year, month, day = date_bits
    if not (len(year) == year_length and year.isdigit()):
        return False
    if not (len(month) == month_day_length and month.isdigit()):
        return False
    if not (len(day) == month_day_length and day.isdigit()):
        return False
    year_i = int(year)
    month_i = int(month)
    day_i = int(day)
    return year_i >= min_year and 1 <= month_i <= max_month and 1 <= day_i <= max_day
