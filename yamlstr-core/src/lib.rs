
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, PartialEq)]
pub enum YamlValue {
    Str(String),
    Seq(Vec<YamlValue>),
    Map(BTreeMap<String, YamlValue>),
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("parse error at line {line}, column {column}: {message}")]
    Generic { line: usize, column: usize, message: String },
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

        // Skip blank and comment-only lines
        if content_trimmed.is_empty() || content_trimmed.starts_with('#') {
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
        YamlValue::Map(map) => {
            match map.get("_naay_version") {
                Some(YamlValue::Str(ver)) => {
                    if !validate_version(ver) {
                        return Err(ParseError::Generic {
                            line: line_no,
                            column: 1,
                            message: format!(
                                "invalid _naay_version '{ver}', expected YYYY.MM.DD-REV"
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
                        message:
                            "missing required _naay_version at root (Semantic Date Versioning)"
                                .to_string(),
                    });
                }
            }
        }
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
    let mut items = Vec::new();
    while *index < lines.len() {
        let line = &lines[*index];
        if line.indent < base_indent {
            break;
        }
        if line.indent > base_indent {
            break;
        }
        let content = line.content;
        if !content.starts_with("- ") {
            break;
        }
        let after_dash = content[2..].trim_start();
        *index += 1;

        if after_dash.is_empty() {
            // nested block
            if *index >= lines.len() || lines[*index].indent <= base_indent {
                items.push(YamlValue::Str(String::new()));
            } else {
                let child_indent = lines[*index].indent;
                let child = parse_block(lines, index, child_indent, anchors)?;
                items.push(child);
            }
        } else if after_dash == "|" {
            let s = parse_block_scalar(lines, index, base_indent + 1)?;
            items.push(YamlValue::Str(s));
        } else if let Some(colon_pos) = after_dash.find(':') {
            // inline single key: value map
            let (k, vpart) = after_dash.split_at(colon_pos);
            let key = parse_key(k.trim(), line.line_no)?;
            let mut map = BTreeMap::new();
            let value = parse_value_inline(
                lines,
                index,
                vpart[1..].trim_start(),
                line.line_no,
                base_indent + 2,
                anchors,
            )?;
            map.insert(key, value);
            items.push(YamlValue::Map(map));
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
            items.push(child);
        } else if after_dash.starts_with('*') {
            let name = after_dash[1..].trim();
            let aliased = anchors.get(name).cloned().ok_or_else(|| ParseError::Generic {
                line: line.line_no,
                column: 1,
                message: format!("unknown anchor: {name}"),
            })?;
            items.push(aliased);
        } else {
            // treat as scalar line; caller spec should ensure quoting
            let scalar = strip_quotes(after_dash);
            items.push(YamlValue::Str(scalar.to_string()));
        }
    }
    Ok(YamlValue::Seq(items))
}

fn parse_map<'a>(
    lines: &[Line<'a>],
    index: &mut usize,
    base_indent: usize,
    anchors: &mut HashMap<String, YamlValue>,
) -> Result<YamlValue, ParseError> {
    let mut map: BTreeMap<String, YamlValue> = BTreeMap::new();
    while *index < lines.len() {
        let line = &lines[*index];
        if line.indent < base_indent {
            break;
        }
        if line.indent > base_indent {
            break;
        }
        let content = line.content;
        if content.starts_with("- ") {
            break;
        }
        let colon_pos = content.find(':').ok_or_else(|| ParseError::Generic {
            line: line.line_no,
            column: 1,
            message: "expected ':' in mapping entry".to_string(),
        })?;
        let (kpart, rest) = content.split_at(colon_pos);
        let key_raw = kpart.trim();
        let key = parse_key(key_raw, line.line_no)?;
        let vpart = rest[1..].trim_start();
        *index += 1;

        if key == "<<" && vpart.starts_with('*') {
            let name = vpart[1..].trim();
            let aliased = anchors.get(name).cloned().ok_or_else(|| ParseError::Generic {
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
            continue;
        }

        if vpart.is_empty() {
            if *index >= lines.len() || lines[*index].indent <= base_indent {
                map.insert(key, YamlValue::Str(String::new()));
            } else {
                let child_indent = lines[*index].indent;
                let child = parse_block(lines, index, child_indent, anchors)?;
                map.insert(key, child);
            }
        } else if vpart == "|" {
            let s = parse_block_scalar(lines, index, base_indent + 1)?;
            map.insert(key, YamlValue::Str(s));
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
            map.insert(key, child);
        } else if vpart.starts_with('*') {
            let name = vpart[1..].trim();
            let aliased = anchors.get(name).cloned().ok_or_else(|| ParseError::Generic {
                line: line.line_no,
                column: colon_pos + 1,
                message: format!("unknown anchor: {name}"),
            })?;
            map.insert(key, aliased);
        } else {
            let scalar = strip_quotes(vpart);
            map.insert(key, YamlValue::Str(scalar.to_string()));
        }
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
) -> Result<YamlValue, ParseError> {

    // Case 1: quoted scalar
    if (vpart.starts_with('"') && vpart.ends_with('"') && vpart.len() >= 2)
        || (vpart.starts_with('\'') && vpart.ends_with('\'') && vpart.len() >= 2)
    {
        return Ok(YamlValue::Str(strip_quotes(vpart).to_string()));
    }

    // Case 2: block literal
    if vpart == "|" {
        let s = parse_block_scalar(lines, index, expected_indent)?;
        return Ok(YamlValue::Str(s));
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
        return Ok(child);
    }

    // Case 4: anchor lookup e.g. key: *foo
    if vpart.starts_with('*') {
        let name = vpart[1..].trim();
        let aliased = anchors.get(name).cloned().ok_or_else(|| ParseError::Generic {
            line: line_no,
            column: 1,
            message: format!("unknown anchor: {name}"),
        })?;
        return Ok(aliased);
    }

    // Case 5: simple string scalar
    Ok(YamlValue::Str(vpart.to_string()))
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
    if line.content.starts_with("- ") {
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
        let s = if cut >= content.len() { "" } else { &content[cut..] };
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
        YamlValue::Str(s) => {
            if s.contains('\n') {
                out.push('|');
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
                out.push('\n');
            }
        }
        YamlValue::Seq(seq) => {
            for item in seq {
                for _ in 0..indent {
                    out.push(' ');
                }
                out.push_str("- ");
                match item {
                    YamlValue::Str(s) => {
                        if s.contains('\n') {
                            out.push('|');
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
                            out.push('\n');
                        }
                    }
                    YamlValue::Map(_) | YamlValue::Seq(_) => {
                        out.push('\n');
                        write_value(out, item, indent + 2)?;
                    }
                }
            }
        }
        YamlValue::Map(map) => {
            for (k, v) in map {
                for _ in 0..indent {
                    out.push(' ');
                }
                let needs_quote =
                    k.chars()
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
                out.push_str(": ");
                match v {
                    YamlValue::Str(s) => {
                        if s.contains('\n') {
                            out.push('|');
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
                            out.push('\n');
                        }
                    }
                    YamlValue::Map(_) | YamlValue::Seq(_) => {
                        out.push('\n');
                        write_value(out, v, indent + 2)?;
                    }
                }
            }
        }
    }
    Ok(())
}
