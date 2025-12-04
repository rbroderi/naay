use stacker::maybe_grow;
use std::collections::{BTreeMap, HashMap};
use std::mem;

const REQUIRED_VERSION: &str = "2025.12.03-0";
const STACK_RED_ZONE: usize = 32 * 1024;
const STACK_GROW: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub enum YamlValue {
    Str(String),
    Seq(Vec<YamlNode>),
    Map(BTreeMap<String, YamlNode>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct YamlNode {
    pub value: YamlValue,
    pub leading_comments: Vec<CommentLine>,
    pub inline_comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommentLine {
    pub indent: usize,
    pub text: String,
}

impl YamlNode {
    pub fn new(value: YamlValue) -> Self {
        Self {
            value,
            leading_comments: Vec::new(),
            inline_comment: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("parse error at line {line}, column {column}: {message}")]
    Generic {
        line: usize,
        column: usize,
        message: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum DumpError {
    #[error("io error: {0}")]
    Io(#[from] std::fmt::Error),
}

#[derive(Debug)]
struct Line<'a> {
    indent: usize,
    content: &'a str,
    line_no: usize,
}

fn preprocess(input: &str) -> Result<Vec<Line<'_>>, ParseError> {
    let mut out = Vec::new();
    for (idx, raw) in input.lines().enumerate() {
        let line_no = idx + 1;

        if raw.contains('\t') {
            return Err(ParseError::Generic {
                line: line_no,
                column: 1,
                message: "tabs are not allowed; use spaces for indentation".to_string(),
            });
        }

        let trimmed = raw.trim_end();
        let content_trimmed = trimmed.trim_start();

        // Skip blank lines but retain comments for later processing
        if content_trimmed.is_empty() {
            continue;
        }

        let indent = trimmed.chars().take_while(|c| *c == ' ').count();
        out.push(Line {
            indent,
            content: content_trimmed,
            line_no,
        });
    }
    Ok(out)
}

fn split_inline_comment(line: &str) -> (&str, Option<&str>) {
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    for (idx, ch) in line.char_indices() {
        match ch {
            '\'' if !in_double => {
                in_single = !in_single;
            }
            '"' if !in_single => {
                if in_double && !escaped {
                    in_double = false;
                } else if !in_double {
                    in_double = true;
                }
            }
            '#' if !in_single && !in_double => {
                let prev_is_space = idx == 0
                    || line[..idx]
                        .chars()
                        .rev()
                        .next()
                        .map(|c| c.is_whitespace())
                        .unwrap_or(true);
                if prev_is_space {
                    let (before, comment) = line.split_at(idx);
                    return (before.trim_end(), Some(comment));
                }
            }
            _ => {}
        }

        if ch == '\\' && in_double && !escaped {
            escaped = true;
            continue;
        }
        escaped = false;
    }

    (line.trim_end(), None)
}

/// Validate Semantic Date Versioning: YYYY.MM.DD-REV where REV is digits
fn validate_version(ver: &str) -> bool {
    let mut parts = ver.split('-');
    let date = match parts.next() {
        Some(d) => d,
        None => return false,
    };
    let rev = match parts.next() {
        Some(r) => r,
        None => return false,
    };
    if parts.next().is_some() {
        return false;
    }
    if rev.is_empty() || !rev.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let mut date_parts = date.split('.');
    let y = match date_parts.next() {
        Some(y) if y.len() == 4 && y.chars().all(|c| c.is_ascii_digit()) => y,
        _ => return false,
    };
    let m = match date_parts.next() {
        Some(m) if m.len() == 2 && m.chars().all(|c| c.is_ascii_digit()) => m,
        _ => return false,
    };
    let d = match date_parts.next() {
        Some(d) if d.len() == 2 && d.chars().all(|c| c.is_ascii_digit()) => d,
        _ => return false,
    };
    if date_parts.next().is_some() {
        return false;
    }
    // very light sanity check on ranges (not full calendar validation)
    let year: u32 = y.parse().unwrap_or(0);
    let month: u32 = m.parse().unwrap_or(0);
    let day: u32 = d.parse().unwrap_or(0);
    year >= 1970 && (1..=12).contains(&month) && (1..=31).contains(&day)
}

pub fn parse_naay(input: &str) -> Result<YamlValue, ParseError> {
    let lines = preprocess(input)?;
    if lines.is_empty() {
        // empty document -> empty map (but will fail version check)
        return Ok(YamlValue::Map(BTreeMap::new()));
    }

    let mut anchors: HashMap<String, YamlValue> = HashMap::new();
    let mut index = 0usize;
    let first_indent = lines[0].indent;
    let value = if lines[0].content.starts_with("- ") {
        parse_seq(&lines, &mut index, first_indent, &mut anchors)?
    } else {
        parse_map(&lines, &mut index, first_indent, &mut anchors)?
    };

    // Enforce root is a map with a valid _naay_version
    let line_no = lines[0].line_no;
    match &value {
        YamlValue::Map(map) => match map.get("_naay_version").map(|n| &n.value) {
            Some(YamlValue::Str(ver)) => {
                if !validate_version(ver) {
                    return Err(ParseError::Generic {
                        line: line_no,
                        column: 1,
                        message: format!("invalid _naay_version '{ver}', expected YYYY.MM.DD-REV"),
                    });
                }
                if ver != REQUIRED_VERSION {
                    return Err(ParseError::Generic {
                        line: line_no,
                        column: 1,
                        message: format!(
                            "unsupported _naay_version '{ver}', expected {REQUIRED_VERSION}"
                        ),
                    });
                }
            }
            Some(_) => {
                return Err(ParseError::Generic {
                    line: line_no,
                    column: 1,
                    message: "_naay_version must be a string scalar".to_string(),
                });
            }
            None => {
                return Err(ParseError::Generic {
                    line: line_no,
                    column: 1,
                    message: "missing required _naay_version at root (Semantic Date Versioning)"
                        .to_string(),
                });
            }
        },
        _ => {
            return Err(ParseError::Generic {
                line: line_no,
                column: 1,
                message: "root of document must be a mapping".to_string(),
            });
        }
    }

    Ok(value)
}

fn parse_seq<'a>(
    lines: &[Line<'a>],
    index: &mut usize,
    base_indent: usize,
    anchors: &mut HashMap<String, YamlValue>,
) -> Result<YamlValue, ParseError> {
    maybe_grow(STACK_RED_ZONE, STACK_GROW, || {
        parse_seq_impl(lines, index, base_indent, anchors)
    })
}

fn parse_seq_impl<'a>(
    lines: &[Line<'a>],
    index: &mut usize,
    base_indent: usize,
    anchors: &mut HashMap<String, YamlValue>,
) -> Result<YamlValue, ParseError> {
    let mut items = Vec::new();
    let mut pending_comments: Vec<CommentLine> = Vec::new();
    while *index < lines.len() {
        let line = &lines[*index];
        if line.indent < base_indent {
            break;
        }

        if line.content.starts_with('#') {
            pending_comments.push(CommentLine {
                indent: line.indent,
                text: line.content.to_string(),
            });
            *index += 1;
            continue;
        }

        if line.indent > base_indent {
            break;
        }

        if !line.content.starts_with('-') {
            break;
        }

        let (content_no_comment, inline_comment) = split_inline_comment(line.content);
        if content_no_comment.len() == 1 {
            // dash only, treat as empty remainder
        } else if !content_no_comment
            .chars()
            .nth(1)
            .map(|c| c.is_whitespace())
            .unwrap_or(false)
        {
            break;
        }
        let after_dash = content_no_comment[1..].trim_start();
        *index += 1;

        let value = if after_dash.is_empty() {
            if *index >= lines.len() || lines[*index].indent <= base_indent {
                YamlValue::Str(String::new())
            } else {
                let child_indent = lines[*index].indent;
                parse_block(lines, index, child_indent, anchors)?
            }
        } else if after_dash == "|" {
            let s = parse_block_scalar(lines, index, base_indent + 1)?;
            YamlValue::Str(s)
        } else if let Some(colon_pos) = after_dash.find(':') {
            let (k, vpart) = after_dash.split_at(colon_pos);
            let key = parse_key(k.trim(), line.line_no)?;
            let mut map = BTreeMap::new();
            let node = parse_value_inline(
                lines,
                index,
                vpart[1..].trim_start(),
                line.line_no,
                base_indent + 2,
                anchors,
            )?;
            if key == "<<" {
                if let YamlValue::Map(merge_map) = node.value {
                    for (mk, mv) in merge_map {
                        map.entry(mk).or_insert(mv);
                    }
                } else {
                    return Err(ParseError::Generic {
                        line: line.line_no,
                        column: colon_pos + 1,
                        message: "merge source must be a mapping".to_string(),
                    });
                }
            } else {
                map.insert(key, node);
            }

            if *index < lines.len() && lines[*index].indent > base_indent {
                let child_indent = lines[*index].indent;
                let continuation = parse_map(lines, index, child_indent, anchors)?;
                if let YamlValue::Map(extra) = continuation {
                    for (ek, ev) in extra {
                        map.insert(ek, ev);
                    }
                } else {
                    return Err(ParseError::Generic {
                        line: line.line_no,
                        column: colon_pos + 1,
                        message: "inline mapping continuation must be a mapping".to_string(),
                    });
                }
            }

            YamlValue::Map(map)
        } else if after_dash.starts_with('&') {
            let anchor_name = after_dash[1..].trim();
            if *index >= lines.len() || lines[*index].indent <= base_indent {
                return Err(ParseError::Generic {
                    line: line.line_no,
                    column: 1,
                    message: "anchor without nested value".to_string(),
                });
            }
            let child_indent = lines[*index].indent;
            let child = parse_block(lines, index, child_indent, anchors)?;
            anchors.insert(anchor_name.to_string(), child.clone());
            child
        } else if after_dash.starts_with('*') {
            let name = after_dash[1..].trim();
            anchors
                .get(name)
                .cloned()
                .ok_or_else(|| ParseError::Generic {
                    line: line.line_no,
                    column: 1,
                    message: format!("unknown anchor: {name}"),
                })?
        } else {
            let scalar = strip_quotes(after_dash);
            YamlValue::Str(scalar.to_string())
        };

        let mut node = YamlNode::new(value);
        node.leading_comments = mem::take(&mut pending_comments);
        if let Some(comment) = inline_comment {
            node.inline_comment = Some(comment.to_string());
        }
        items.push(node);
    }
    Ok(YamlValue::Seq(items))
}

fn parse_map<'a>(
    lines: &[Line<'a>],
    index: &mut usize,
    base_indent: usize,
    anchors: &mut HashMap<String, YamlValue>,
) -> Result<YamlValue, ParseError> {
    maybe_grow(STACK_RED_ZONE, STACK_GROW, || {
        parse_map_impl(lines, index, base_indent, anchors)
    })
}

fn parse_map_impl<'a>(
    lines: &[Line<'a>],
    index: &mut usize,
    base_indent: usize,
    anchors: &mut HashMap<String, YamlValue>,
) -> Result<YamlValue, ParseError> {
    let mut map: BTreeMap<String, YamlNode> = BTreeMap::new();
    let mut pending_comments: Vec<CommentLine> = Vec::new();
    while *index < lines.len() {
        let line = &lines[*index];
        if line.indent < base_indent {
            break;
        }
        if line.content.starts_with("- ") {
            break;
        }

        if line.content.starts_with('#') {
            pending_comments.push(CommentLine {
                indent: line.indent,
                text: line.content.to_string(),
            });
            *index += 1;
            continue;
        }

        if line.indent > base_indent {
            break;
        }

        let (content_no_comment, inline_comment) = split_inline_comment(line.content);
        let colon_pos = content_no_comment
            .find(':')
            .ok_or_else(|| ParseError::Generic {
                line: line.line_no,
                column: 1,
                message: "expected ':' in mapping entry".to_string(),
            })?;
        let (kpart, rest) = content_no_comment.split_at(colon_pos);
        let key_raw = kpart.trim();
        let key = parse_key(key_raw, line.line_no)?;
        let vpart = rest[1..].trim_start();
        *index += 1;

        if key == "<<" && vpart.starts_with('*') {
            let name = vpart[1..].trim();
            let aliased = anchors
                .get(name)
                .cloned()
                .ok_or_else(|| ParseError::Generic {
                    line: line.line_no,
                    column: colon_pos + 1,
                    message: format!("unknown anchor: {name}"),
                })?;
            if let YamlValue::Map(merge_map) = aliased {
                for (k, v) in merge_map {
                    map.entry(k).or_insert(v);
                }
            } else {
                return Err(ParseError::Generic {
                    line: line.line_no,
                    column: colon_pos + 1,
                    message: "merge source must be a mapping".to_string(),
                });
            }
            pending_comments.clear();
            continue;
        }

        let value = if vpart.is_empty() {
            if *index >= lines.len() || lines[*index].indent <= base_indent {
                YamlValue::Str(String::new())
            } else {
                let child_indent = lines[*index].indent;
                parse_block(lines, index, child_indent, anchors)?
            }
        } else if vpart == "|" {
            let s = parse_block_scalar(lines, index, base_indent + 1)?;
            YamlValue::Str(s)
        } else if vpart.starts_with('&') {
            let anchor_name = vpart[1..].trim();
            if *index >= lines.len() || lines[*index].indent <= base_indent {
                return Err(ParseError::Generic {
                    line: line.line_no,
                    column: colon_pos + 1,
                    message: "anchor without nested value".to_string(),
                });
            }
            let child_indent = lines[*index].indent;
            let child = parse_block(lines, index, child_indent, anchors)?;
            anchors.insert(anchor_name.to_string(), child.clone());
            child
        } else if vpart.starts_with('*') {
            let name = vpart[1..].trim();
            anchors
                .get(name)
                .cloned()
                .ok_or_else(|| ParseError::Generic {
                    line: line.line_no,
                    column: colon_pos + 1,
                    message: format!("unknown anchor: {name}"),
                })?
        } else {
            let scalar = strip_quotes(vpart);
            YamlValue::Str(scalar.to_string())
        };

        let mut node = YamlNode::new(value);
        node.leading_comments = mem::take(&mut pending_comments);
        if let Some(comment) = inline_comment {
            node.inline_comment = Some(comment.to_string());
        }
        map.insert(key, node);
    }
    Ok(YamlValue::Map(map))
}

fn parse_value_inline<'a>(
    lines: &[Line<'a>],
    index: &mut usize,
    vpart: &str,
    line_no: usize,
    expected_indent: usize,
    anchors: &mut HashMap<String, YamlValue>,
) -> Result<YamlNode, ParseError> {
    // Case 1: quoted scalar
    if (vpart.starts_with('"') && vpart.ends_with('"') && vpart.len() >= 2)
        || (vpart.starts_with('\'') && vpart.ends_with('\'') && vpart.len() >= 2)
    {
        return Ok(YamlNode::new(YamlValue::Str(
            strip_quotes(vpart).to_string(),
        )));
    }

    // Case 2: block literal
    if vpart == "|" {
        let s = parse_block_scalar(lines, index, expected_indent)?;
        return Ok(YamlNode::new(YamlValue::Str(s)));
    }

    // Case 3: anchor definition, e.g. key: &foo
    if vpart.starts_with('&') {
        let anchor_name = vpart[1..].trim();
        if *index >= lines.len() || lines[*index].indent <= expected_indent - 1 {
            return Err(ParseError::Generic {
                line: line_no,
                column: 1,
                message: "anchor without nested value".to_string(),
            });
        }
        let child_indent = lines[*index].indent;
        let child = parse_block(lines, index, child_indent, anchors)?;
        anchors.insert(anchor_name.to_string(), child.clone());
        return Ok(YamlNode::new(child));
    }

    // Case 4: anchor lookup e.g. key: *foo
    if vpart.starts_with('*') {
        let name = vpart[1..].trim();
        let aliased = anchors
            .get(name)
            .cloned()
            .ok_or_else(|| ParseError::Generic {
                line: line_no,
                column: 1,
                message: format!("unknown anchor: {name}"),
            })?;
        return Ok(YamlNode::new(aliased));
    }

    // Case 5: simple string scalar
    Ok(YamlNode::new(YamlValue::Str(vpart.to_string())))
}

fn parse_block<'a>(
    lines: &[Line<'a>],
    index: &mut usize,
    base_indent: usize,
    anchors: &mut HashMap<String, YamlValue>,
) -> Result<YamlValue, ParseError> {
    if *index >= lines.len() {
        return Ok(YamlValue::Str(String::new()));
    }
    let line = &lines[*index];
    if line.content.starts_with('-')
        && line
            .content
            .chars()
            .nth(1)
            .map(|c| c.is_whitespace())
            .unwrap_or(true)
    {
        parse_seq(lines, index, base_indent, anchors)
    } else {
        parse_map(lines, index, base_indent, anchors)
    }
}

fn parse_block_scalar<'a>(
    lines: &[Line<'a>],
    index: &mut usize,
    min_indent: usize,
) -> Result<String, ParseError> {
    let mut result_lines: Vec<(&str, usize)> = Vec::new();
    while *index < lines.len() {
        let line = &lines[*index];
        if line.indent <= min_indent {
            break;
        }
        result_lines.push((line.content, line.indent));
        *index += 1;
    }
    if result_lines.is_empty() {
        return Ok(String::new());
    }
    let min = result_lines
        .iter()
        .map(|(_, ind)| *ind)
        .min()
        .unwrap_or(min_indent + 1);
    let mut out = String::new();
    for (i, (content, indent)) in result_lines.into_iter().enumerate() {
        let cut = if indent >= min { indent - min } else { 0 };
        let s = if cut >= content.len() {
            ""
        } else {
            &content[cut..]
        };
        if i > 0 {
            out.push('\n');
        }
        out.push_str(s);
    }
    Ok(out)
}

fn parse_key(raw: &str, _line_no: usize) -> Result<String, ParseError> {
    if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
        Ok(raw[1..raw.len() - 1].to_string())
    } else if raw.starts_with('\'') && raw.ends_with('\'') && raw.len() >= 2 {
        Ok(raw[1..raw.len() - 1].to_string())
    } else {
        Ok(raw.to_string())
    }
}

fn strip_quotes(s: &str) -> &str {
    if (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
        || (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

pub fn dump_naay(value: &YamlValue) -> Result<String, DumpError> {
    let mut out = String::new();
    write_value(&mut out, value, 0)?;
    Ok(out)
}

fn write_value(out: &mut String, value: &YamlValue, indent: usize) -> Result<(), std::fmt::Error> {
    match value {
        YamlValue::Str(s) => write_scalar(out, indent, s, None),
        YamlValue::Seq(seq) => write_seq(out, seq, indent),
        YamlValue::Map(map) => write_map(out, map, indent),
    }
}

fn write_comments(out: &mut String, comments: &[CommentLine]) -> Result<(), std::fmt::Error> {
    for comment in comments {
        for _ in 0..comment.indent {
            out.push(' ');
        }
        out.push_str(&comment.text);
        out.push('\n');
    }
    Ok(())
}

fn write_scalar(
    out: &mut String,
    indent: usize,
    s: &str,
    inline_comment: Option<&String>,
) -> Result<(), std::fmt::Error> {
    if s.contains('\n') {
        out.push('|');
        if let Some(comment) = inline_comment {
            out.push(' ');
            out.push_str(comment);
        }
        out.push('\n');
        for line in s.split('\n') {
            for _ in 0..(indent + 2) {
                out.push(' ');
            }
            out.push_str(line);
            out.push('\n');
        }
    } else {
        out.push('"');
        for ch in s.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                _ => out.push(ch),
            }
        }
        out.push('"');
        if let Some(comment) = inline_comment {
            out.push(' ');
            out.push_str(comment);
        }
        out.push('\n');
    }
    Ok(())
}

fn write_seq(out: &mut String, seq: &[YamlNode], indent: usize) -> Result<(), std::fmt::Error> {
    for node in seq {
        write_comments(out, &node.leading_comments)?;
        for _ in 0..indent {
            out.push(' ');
        }
        out.push_str("- ");
        match &node.value {
            YamlValue::Str(s) => {
                write_scalar(out, indent, s, node.inline_comment.as_ref())?;
            }
            YamlValue::Seq(child) => {
                if let Some(comment) = &node.inline_comment {
                    out.push(' ');
                    out.push_str(comment);
                }
                out.push('\n');
                write_seq(out, child, indent + 2)?;
            }
            YamlValue::Map(map) => {
                if let Some(comment) = &node.inline_comment {
                    out.push(' ');
                    out.push_str(comment);
                }
                out.push('\n');
                write_map(out, map, indent + 2)?;
            }
        }
    }
    Ok(())
}

fn write_map(
    out: &mut String,
    map: &BTreeMap<String, YamlNode>,
    indent: usize,
) -> Result<(), std::fmt::Error> {
    for (k, node) in map {
        write_comments(out, &node.leading_comments)?;
        for _ in 0..indent {
            out.push(' ');
        }
        let needs_quote = k
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, ':' | '?' | '#'));
        if needs_quote {
            out.push('"');
            for ch in k.chars() {
                match ch {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    _ => out.push(ch),
                }
            }
            out.push('"');
        } else {
            out.push_str(k);
        }
        out.push_str(":");
        match &node.value {
            YamlValue::Str(s) => {
                out.push(' ');
                write_scalar(out, indent, s, node.inline_comment.as_ref())?;
            }
            YamlValue::Seq(child) => {
                if let Some(comment) = &node.inline_comment {
                    out.push(' ');
                    out.push_str(comment);
                }
                out.push('\n');
                write_seq(out, child, indent + 2)?;
            }
            YamlValue::Map(child) => {
                if let Some(comment) = &node.inline_comment {
                    out.push(' ');
                    out.push_str(comment);
                }
                out.push('\n');
                write_map(out, child, indent + 2)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_single_line_comments() {
        let input = r#"
# preface
_naay_version: "2025.12.03-0" # force version
defaults:
    # nested
    alignment: "TRUE NEUTRAL"
"#;

        let parsed = parse_naay(input).expect("parse should succeed");
        let dumped = dump_naay(&parsed).expect("dump should succeed");

        assert!(dumped.contains("# preface"));
        assert!(dumped.contains("# force version"));
        assert!(dumped.contains("# nested"));
    }
}
