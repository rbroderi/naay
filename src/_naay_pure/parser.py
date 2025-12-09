"""Pure-Python implementation of the naay YAML subset."""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING
from typing import Any
from typing import Final
from typing import Literal

from naay import REQUIRED_VERSION

if TYPE_CHECKING:
    from collections.abc import Sequence

try:
    from beartype.claw import beartype_this_package

    beartype_this_package()
except ModuleNotFoundError:  # pragma: no cover - optional dependency
    pass

YamlValue = str | list["YamlValue"] | dict[str, "YamlValue"]


class NaayParseError(ValueError):
    """Raised when the pure-Python parser encounters invalid input."""


class NaayDumpError(ValueError):
    """Raised when dumping fails due to unsupported types."""


@dataclass(slots=True)
class Line:
    indent: int
    content: str
    line_no: int


@dataclass(slots=True)
class _ParentRef:
    sequence: list[YamlValue] | None = None
    index: int | None = None
    mapping: dict[str, YamlValue] | None = None
    key: str | None = None

    def replace(self, value: YamlValue) -> None:
        if self.sequence is not None and self.index is not None:
            self.sequence[self.index] = value
            return
        if self.mapping is not None and self.key is not None:
            self.mapping[self.key] = value
            return
        msg = "invalid parent reference for replacement"
        raise NaayParseError(msg)


@dataclass(slots=True)
class _Context:
    kind: Literal["map", "seq"]
    indent: int
    container: list[YamlValue] | dict[str, YamlValue]
    anchor_name: str | None = None
    parent: _ParentRef | None = None


def loads(text: str, /) -> YamlValue:
    """Parse naay text into nested dict/list/scalar structures.

    Returns:
        The parsed YAML value as nested dict/list/scalar structures.
    """
    parser = _Parser(text)
    return parser.parse()


def dumps(data: YamlValue, /) -> str:
    """Serialize a naay-compatible tree back into text.

    Returns:
        The serialized YAML text representation.

    Raises:
        NaayDumpError: If serialization fails due to unsupported types.
    """
    dumper = _Dumper()
    try:
        dumper.write_value(data, 0)
    except TypeError as exc:  # pragma: no cover - defensive guard
        raise NaayDumpError(str(exc)) from exc
    return dumper.render()


class _Parser:
    def __init__(self, text: str) -> None:
        super().__init__()
        self.lines: list[Line] = self._preprocess(text)
        self.index = 0
        self.anchors: dict[str, YamlValue] = {}
        self._root_replacement: YamlValue | None = None

    # Public -----------------------------------------------------------------
    def parse(self) -> YamlValue:  # noqa: C901
        if not self.lines:
            msg = "missing required _naay_version at root (Semantic Date Versioning)"
            raise NaayParseError(msg)
        first_idx = self._skip_comments(self.index)
        if first_idx >= len(self.lines):
            msg = "missing required _naay_version at root (Semantic Date Versioning)"
            raise NaayParseError(msg)
        self.index = first_idx
        first_line = self.lines[self.index]
        base_indent = first_line.indent
        if self._looks_like_seq(first_line):
            root: list[YamlValue] | dict[str, YamlValue] = []
            stack: list[_Context] = [
                _Context(kind="seq", indent=base_indent, container=root),
            ]
        else:
            root = {}
            stack = [
                _Context(kind="map", indent=base_indent, container=root),
            ]
        self._root_replacement = None
        while stack:
            context = stack[-1]
            if self.index >= len(self.lines):
                self._finalize_context(stack)
                continue
            line = self.lines[self.index]
            if line.content.startswith("#"):
                self.index += 1
                continue
            if line.indent < context.indent:
                self._finalize_context(stack)
                continue
            if line.indent > context.indent:
                msg = f"unexpected indentation (line {line.line_no})"
                raise NaayParseError(msg)
            if context.kind == "seq":
                if not self._process_seq_line(context, stack):
                    continue
            elif not self._process_map_line(context, stack):
                continue
        result: YamlValue = (
            self._root_replacement if self._root_replacement is not None else root
        )
        self._enforce_root_version(result, first_line.line_no)
        return result

    # Iterative helpers -------------------------------------------------------
    def _finalize_context(self, stack: list[_Context]) -> None:
        finished = stack.pop()
        if finished.anchor_name:
            cloned = _clone_value(finished.container)
            self.anchors[finished.anchor_name] = cloned
            replacement = _clone_value(finished.container)
            if finished.parent is not None:
                finished.parent.replace(replacement)
            else:
                self._root_replacement = replacement

    def _consume_until_depth(self, stack: list[_Context], target_depth: int) -> None:
        while len(stack) > target_depth:
            context = stack[-1]
            if self.index >= len(self.lines):
                self._finalize_context(stack)
                continue
            line = self.lines[self.index]
            if line.content.startswith("#"):
                self.index += 1
                continue
            if line.indent < context.indent:
                self._finalize_context(stack)
                continue
            if line.indent > context.indent:
                msg = f"unexpected indentation (line {line.line_no})"
                raise NaayParseError(msg)
            if context.kind == "seq":
                if not self._process_seq_line(context, stack):
                    continue
            elif not self._process_map_line(context, stack):
                continue

    def _process_seq_line(self, context: _Context, stack: list[_Context]) -> bool:
        line = self.lines[self.index]
        if not self._looks_like_seq(line):
            self._finalize_context(stack)
            return False
        body, _ = _split_inline_comment(line.content)
        after_dash = body[1:].lstrip()
        self.index += 1
        self._assign_seq_value(context, stack, line, after_dash)
        return True

    def _process_map_line(self, context: _Context, stack: list[_Context]) -> bool:
        line = self.lines[self.index]
        if line.content.startswith("- ") and line.indent == context.indent:
            self._finalize_context(stack)
            return False
        stripped, _ = _split_inline_comment(line.content)
        colon_pos = stripped.find(":")
        if colon_pos == -1:
            msg = f"expected ':' in mapping entry (line {line.line_no})"
            raise NaayParseError(msg)
        key_raw = stripped[:colon_pos].strip()
        value_raw = stripped[colon_pos + 1 :].lstrip()
        key = _parse_key(key_raw)
        self.index += 1
        if not isinstance(context.container, dict):
            msg = f"expected mapping context (line {line.line_no})"
            raise NaayParseError(msg)
        mapping = context.container
        if key == "<<" and value_raw.startswith("*"):
            merged = self._resolve_alias(value_raw[1:].strip(), line)
            self._merge_into(mapping, merged, line)
            return True
        self._assign_map_value(context, stack, line, key, value_raw)
        return True

    def _assign_seq_value(
        self,
        context: _Context,
        stack: list[_Context],
        line: Line,
        token: str,
    ) -> None:
        items: list[YamlValue] = context.container  # type: ignore[assignment]
        if not token:
            if not self._start_sequence_child(context, stack, line, required=False):
                items.append("")
            return
        if token == "|":  # noqa: S105
            items.append(self._parse_block_scalar(context.indent + 1))
            return
        if token.startswith("&"):
            anchor_name = token[1:].strip()
            if not anchor_name:
                msg = f"invalid anchor name (line {line.line_no})"
                raise NaayParseError(msg)
            self._start_sequence_child(
                context,
                stack,
                line,
                required=True,
                anchor_name=anchor_name,
            )
            return
        if token.startswith("*"):
            items.append(_clone_value(self._resolve_alias(token[1:].strip(), line)))
            return
        literal = _empty_literal(token)
        if literal is not None:
            items.append(literal)
            return
        if ":" in token:
            inline_map = self._parse_inline_map(token, context.indent, line, stack)
            items.append(inline_map)
            return
        items.append(_strip_quotes(token))

    def _assign_map_value(
        self,
        context: _Context,
        stack: list[_Context],
        line: Line,
        key: str,
        value_raw: str,
    ) -> None:
        if not isinstance(context.container, dict):
            msg = f"expected mapping context (line {line.line_no})"
            raise NaayParseError(msg)
        mapping: dict[str, YamlValue] = context.container
        if not value_raw:
            if not self._start_map_child(context, stack, line, key, required=False):
                mapping[key] = ""
            return
        if value_raw == "|":
            mapping[key] = self._parse_block_scalar(context.indent + 1)
            return
        if value_raw.startswith("&"):
            anchor_name = value_raw[1:].strip()
            if not anchor_name:
                msg = f"invalid anchor name (line {line.line_no})"
                raise NaayParseError(msg)
            self._start_map_child(
                context,
                stack,
                line,
                key,
                required=True,
                anchor_name=anchor_name,
            )
            return
        if value_raw.startswith("*"):
            mapping[key] = _clone_value(
                self._resolve_alias(value_raw[1:].strip(), line),
            )
            return
        literal = _empty_literal(value_raw)
        if literal is not None:
            mapping[key] = literal
            return
        mapping[key] = _strip_quotes(value_raw)

    def _start_sequence_child(
        self,
        context: _Context,
        stack: list[_Context],
        line: Line,
        *,
        required: bool,
        anchor_name: str | None = None,
    ) -> bool:
        parent_list = context.container
        return self._start_child_context(
            parent_container=parent_list,
            is_list=True,
            base_indent=context.indent,
            line=line,
            stack=stack,
            required=required,
            anchor_name=anchor_name,
        )

    def _start_map_child(  # noqa: PLR0913
        self,
        context: _Context,
        stack: list[_Context],
        line: Line,
        key: str,
        *,
        required: bool,
        anchor_name: str | None = None,
    ) -> bool:
        parent_map = context.container
        return self._start_child_context(
            parent_container=parent_map,
            is_list=False,
            base_indent=context.indent,
            line=line,
            stack=stack,
            required=required,
            anchor_name=anchor_name,
            key=key,
        )

    def _start_child_context(  # noqa: PLR0913
        self,
        *,
        parent_container: list[YamlValue] | dict[str, YamlValue],
        is_list: bool,
        base_indent: int,
        line: Line,
        stack: list[_Context],
        required: bool,
        anchor_name: str | None,
        key: str | None = None,
    ) -> bool:
        next_idx = self._skip_comments(self.index)
        if next_idx >= len(self.lines) or self.lines[next_idx].indent <= base_indent:
            if required:
                msg = f"anchor without nested value (line {line.line_no})"
                raise NaayParseError(msg)
            return False
        child_line = self.lines[next_idx]
        child_kind: Literal["map", "seq"] = (
            "seq" if self._looks_like_seq(child_line) else "map"
        )
        container: list[YamlValue] | dict[str, YamlValue]
        parent_ref: _ParentRef | None = None
        container = [] if child_kind == "seq" else {}
        if is_list:
            if not isinstance(parent_container, list):
                msg = "expected list container"
                raise NaayParseError(msg)
            parent_container.append(container)
            if anchor_name:
                parent_ref = _ParentRef(
                    sequence=parent_container,
                    index=len(parent_container) - 1,
                )
        else:
            if key is None:
                msg = "missing mapping key for nested context"
                raise NaayParseError(msg)
            if not isinstance(parent_container, dict):
                msg = "expected dict container"
                raise NaayParseError(msg)
            parent_container[key] = container
            if anchor_name:
                parent_ref = _ParentRef(mapping=parent_container, key=key)
        stack.append(
            _Context(
                kind=child_kind,
                indent=child_line.indent,
                container=container,
                anchor_name=anchor_name,
                parent=parent_ref,
            ),
        )
        self.index = next_idx
        return True

    def _start_inline_child_context(  # noqa: PLR0913
        self,
        mapping: dict[str, YamlValue],
        key: str,
        expected_indent: int,
        line: Line,
        stack: list[_Context],
        *,
        anchor_name: str | None = None,
    ) -> None:
        next_idx = self._skip_comments(self.index)
        if (
            next_idx >= len(self.lines)
            or self.lines[next_idx].indent <= expected_indent - 1
        ):
            msg = f"anchor without nested value (line {line.line_no})"
            raise NaayParseError(msg)
        child_line = self.lines[next_idx]
        child_kind: Literal["map", "seq"] = (
            "seq" if self._looks_like_seq(child_line) else "map"
        )
        container: list[YamlValue] | dict[str, YamlValue]
        container = [] if child_kind == "seq" else {}
        mapping[key] = container
        parent_ref = _ParentRef(mapping=mapping, key=key) if anchor_name else None
        stack.append(
            _Context(
                kind=child_kind,
                indent=child_line.indent,
                container=container,
                anchor_name=anchor_name,
                parent=parent_ref,
            ),
        )
        self.index = next_idx

    def _parse_inline_map(
        self,
        payload: str,
        base_indent: int,
        line: Line,
        stack: list[_Context],
    ) -> dict[str, YamlValue]:
        colon_pos = payload.find(":")
        if colon_pos == -1:
            msg = f"expected ':' inside inline map (line {line.line_no})"
            raise NaayParseError(msg)
        key = _parse_key(payload[:colon_pos].strip())
        remainder = payload[colon_pos + 1 :].lstrip()
        mapping: dict[str, YamlValue] = {}
        value = self._parse_inline_value(
            remainder,
            line,
            base_indent + 2,
            stack,
            mapping,
            key,
        )
        if key == "<<":
            self._merge_into(mapping, value, line)
            mapping.pop("<<", None)
        else:
            mapping[key] = value
        next_idx = self._skip_comments(self.index)
        if next_idx < len(self.lines) and self.lines[next_idx].indent > base_indent:
            child_indent = self.lines[next_idx].indent
            before_len = len(stack)
            stack.append(
                _Context(
                    kind="map",
                    indent=child_indent,
                    container=mapping,
                ),
            )
            self.index = next_idx
            self._consume_until_depth(stack, before_len)
        return mapping

    def _parse_inline_value(  # noqa: PLR0913, PLR0917
        self,
        vpart: str,
        line: Line,
        expected_indent: int,
        stack: list[_Context],
        mapping: dict[str, YamlValue],
        key: str,
    ) -> YamlValue:
        min_quote_len: Final = 2
        is_double_quoted = (
            vpart.startswith('"')
            and vpart.endswith('"')
            and len(vpart) >= min_quote_len
        )
        is_single_quoted = (
            vpart.startswith("'")
            and vpart.endswith("'")
            and len(vpart) >= min_quote_len
        )
        if is_double_quoted or is_single_quoted:
            return _strip_quotes(vpart)
        if vpart == "|":
            return self._parse_block_scalar(expected_indent)
        if vpart.startswith("&"):
            anchor_name = vpart[1:].strip()
            if not anchor_name:
                msg = f"invalid anchor name (line {line.line_no})"
                raise NaayParseError(msg)
            before_len = len(stack)
            self._start_inline_child_context(
                mapping,
                key,
                expected_indent,
                line,
                stack,
                anchor_name=anchor_name,
            )
            self._consume_until_depth(stack, before_len)
            return mapping[key]
        if vpart.startswith("*"):
            return _clone_value(self._resolve_alias(vpart[1:].strip(), line))
        literal = _empty_literal(vpart)
        if literal is not None:
            return literal
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

    @staticmethod
    def _looks_like_seq(line: Line) -> bool:
        return line.content.startswith("-") and (
            len(line.content) == 1 or line.content[1].isspace()
        )

    def _skip_comments(self, start: int) -> int:
        idx = start
        while idx < len(self.lines) and self.lines[idx].content.startswith("#"):
            idx += 1
        return idx

    @staticmethod
    def _enforce_root_version(value: YamlValue, _line_no: int) -> None:
        if not isinstance(value, dict):
            msg = "root of document must be a mapping"
            raise NaayParseError(msg)
        ver = value.get("_naay_version")
        if not isinstance(ver, str):
            msg = "missing required _naay_version at root (Semantic Date Versioning)"
            raise NaayParseError(
                msg,
            )
        if ver.strip() != REQUIRED_VERSION:
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
        super().__init__()
        self._parts: list[str] = []
        self._tasks: list[tuple[str, tuple[Any, ...]]] = []

    def write_value(self, value: YamlValue, indent: int) -> None:
        self._tasks.append(("value", (indent, value)))
        while self._tasks:
            task, payload = self._tasks.pop()
            if task == "value":
                self._process_value(*payload)
            elif task == "seq":
                self._process_seq(*payload)
            elif task == "map":
                self._process_map(*payload)
            else:  # pragma: no cover - defensive
                msg = f"unknown dump task: {task}"
                raise NaayDumpError(msg)

    def render(self) -> str:
        return "".join(self._parts)

    # Writers ----------------------------------------------------------------
    def _process_value(self, indent: int, value: YamlValue) -> None:
        if isinstance(value, str):
            self._write_scalar(value, indent)
            return
        if isinstance(value, list):
            if not value:
                self._parts.append(" " * indent + "[]\n")
                return
            self._tasks.append(("seq", (indent, value, 0)))
            return
        if not value:
            self._parts.append(" " * indent + "{}\n")
            return
        items = list(value.items())
        self._tasks.append(("map", (indent, items, 0)))

    def _process_seq(
        self,
        indent: int,
        seq: Sequence[YamlValue],
        index: int,
    ) -> None:
        if index >= len(seq):
            return
        item = seq[index]
        prefix = " " * indent + "- "
        self._parts.append(prefix)
        if isinstance(item, str):
            self._write_scalar(item, indent)
            self._tasks.append(("seq", (indent, seq, index + 1)))
            return
        if isinstance(item, list):
            if not item:
                self._parts.append("[]\n")
                self._tasks.append(("seq", (indent, seq, index + 1)))
                return
            self._parts.append("\n")
            self._tasks.append(("seq", (indent, seq, index + 1)))
            self._tasks.append(("seq", (indent + 2, item, 0)))
            return
        if not item:
            self._parts.append("{}\n")
            self._tasks.append(("seq", (indent, seq, index + 1)))
            return
        self._parts.append("\n")
        items = list(item.items())
        self._tasks.append(("seq", (indent, seq, index + 1)))
        self._tasks.append(("map", (indent + 2, items, 0)))

    def _process_map(
        self,
        indent: int,
        items: Sequence[tuple[str, YamlValue]],
        index: int,
    ) -> None:
        if index >= len(items):
            return
        key, value = items[index]
        formatted_key = self._format_key(key)
        prefix = " " * indent + formatted_key + ":"
        if isinstance(value, str):
            self._parts.append(prefix + " ")
            self._write_scalar(value, indent)
            self._tasks.append(("map", (indent, items, index + 1)))
            return
        if isinstance(value, list):
            if not value:
                self._parts.append(prefix + " []\n")
                self._tasks.append(("map", (indent, items, index + 1)))
                return
            self._parts.append(prefix + "\n")
            self._tasks.append(("map", (indent, items, index + 1)))
            self._tasks.append(("seq", (indent + 2, value, 0)))
            return
        if not value:
            self._parts.append(prefix + " {}\n")
            self._tasks.append(("map", (indent, items, index + 1)))
            return
        self._parts.append(prefix + "\n")
        nested_items = list(value.items())
        self._tasks.append(("map", (indent, items, index + 1)))
        self._tasks.append(("map", (indent + 2, nested_items, 0)))

    def _write_scalar(self, value: str, indent: int) -> None:
        if "\n" in value:
            self._parts.append("|")
            self._parts.append("\n")
            for line in value.split("\n"):
                self._parts.append(" " * (indent + 2) + line + "\n")
            return
        escaped = value.replace("\\", "\\\\").replace('"', '\\"')
        self._parts.append(f'"{escaped}"\n')

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
    is_double_quoted = (
        value.startswith('"') and value.endswith('"') and len(value) >= min_quote_len
    )
    is_single_quoted = (
        value.startswith("'") and value.endswith("'") and len(value) >= min_quote_len
    )
    if is_double_quoted or is_single_quoted:
        return value[1:-1]
    return value


def _clone_value(value: YamlValue) -> YamlValue:
    if isinstance(value, dict):
        return {k: _clone_value(v) for k, v in value.items()}
    if isinstance(value, list):
        return [_clone_value(v) for v in value]
    return value


def _empty_literal(token: str) -> YamlValue | None:
    if token == "[]":  # noqa: S105
        return []
    if token == "{}":  # noqa: S105
        return {}
    return None
