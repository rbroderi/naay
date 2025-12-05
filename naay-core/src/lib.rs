use std::collections::{BTreeMap, HashMap};
use std::marker::PhantomData;
use std::mem;
use tailcall::trampoline::{self, Next};

const REQUIRED_VERSION: &str = "1.0";

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

#[derive(Debug, Clone, Copy)]
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

pub fn parse_naay(input: &str) -> Result<YamlValue, ParseError> {
    let lines = preprocess(input)?;
    if lines.is_empty() {
        // empty document -> empty map (but will fail version check)
        return Ok(YamlValue::Map(BTreeMap::new()));
    }

    let machine = ParseMachine::new(&lines)?;
    let value = run_parse_machine(machine)?;

    // Enforce root is a map with a valid _naay_version
    let line_no = lines[0].line_no;
    match &value {
        YamlValue::Map(map) => match map.get("_naay_version").map(|n| &n.value) {
            Some(YamlValue::Str(ver)) => {
                if ver.trim() != REQUIRED_VERSION {
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
struct ParseMachine<'a> {
    env: ParseEnv<'a>,
    stack: Vec<Frame<'a>>,
}

impl<'a> ParseMachine<'a> {
    fn new(lines: &'a [Line<'a>]) -> Result<Self, ParseError> {
        let first = lines.first().ok_or_else(|| ParseError::Generic {
            line: 1,
            column: 1,
            message: "document must contain at least one line".to_string(),
        })?;
        let kind = detect_block_kind(first);
        let env = ParseEnv {
            lines,
            index: 0,
            anchors: HashMap::new(),
        };
        let mut stack = Vec::new();
        stack.push(Frame::new(kind, first.indent));
        Ok(Self { env, stack })
    }

    fn run_step(&mut self) -> Result<Option<YamlValue>, ParseError> {
        loop {
            let frame = self.stack.last_mut().ok_or_else(|| ParseError::Generic {
                line: 1,
                column: 1,
                message: "unexpected empty parser stack".to_string(),
            })?;

            match frame.step(&mut self.env)? {
                FrameStep::Continue => continue,
                FrameStep::NeedChild { indent } => {
                    let line = self.env.peek_line().ok_or_else(|| ParseError::Generic {
                        line: 1,
                        column: 1,
                        message: "expected nested block".to_string(),
                    })?;
                    let kind = detect_block_kind(line);
                    self.stack.push(Frame::new(kind, indent));
                }
                FrameStep::Return(value) => {
                    self.stack.pop();
                    if let Some(parent) = self.stack.last_mut() {
                        parent.handle_child(value, &mut self.env)?;
                    } else {
                        return Ok(Some(value));
                    }
                }
            }
        }
    }

    fn step(mut self) -> Next<Self, Result<YamlValue, ParseError>> {
        match self.run_step() {
            Ok(Some(value)) => Next::Finish(Ok(value)),
            Ok(None) => Next::Recurse(self),
            Err(err) => Next::Finish(Err(err)),
        }
    }
}

fn run_parse_machine<'a>(machine: ParseMachine<'a>) -> Result<YamlValue, ParseError> {
    trampoline::run(ParseMachine::step, machine)
}

struct ParseEnv<'a> {
    lines: &'a [Line<'a>],
    index: usize,
    anchors: HashMap<String, YamlValue>,
}

impl<'a> ParseEnv<'a> {
    fn peek_line(&self) -> Option<&Line<'a>> {
        self.lines.get(self.index)
    }
}

enum Frame<'a> {
    Seq(SeqFrame<'a>),
    Map(MapFrame<'a>),
}

impl<'a> Frame<'a> {
    fn new(kind: BlockKind, base_indent: usize) -> Self {
        match kind {
            BlockKind::Seq => Frame::Seq(SeqFrame::new(base_indent)),
            BlockKind::Map => Frame::Map(MapFrame::new(base_indent)),
        }
    }

    fn step(&mut self, env: &mut ParseEnv<'a>) -> Result<FrameStep, ParseError> {
        match self {
            Frame::Seq(seq) => seq.step(env),
            Frame::Map(map) => map.step(env),
        }
    }

    fn handle_child(&mut self, value: YamlValue, env: &mut ParseEnv<'a>) -> Result<(), ParseError> {
        match self {
            Frame::Seq(seq) => seq.handle_child(value, env),
            Frame::Map(map) => map.handle_child(value, env),
        }
    }
}

enum FrameStep {
    Continue,
    NeedChild { indent: usize },
    Return(YamlValue),
}

#[derive(Copy, Clone)]
enum BlockKind {
    Seq,
    Map,
}

fn detect_block_kind(line: &Line<'_>) -> BlockKind {
    if looks_like_seq(line.content) {
        BlockKind::Seq
    } else {
        BlockKind::Map
    }
}

fn looks_like_seq(content: &str) -> bool {
    if !content.starts_with('-') {
        return false;
    }
    if content.len() == 1 {
        return true;
    }
    content
        .as_bytes()
        .get(1)
        .map(|c| c.is_ascii_whitespace())
        .unwrap_or(true)
}

struct SeqFrame<'a> {
    base_indent: usize,
    items: Vec<YamlNode>,
    pending_comments: Vec<CommentLine>,
    waiting: Option<SeqWaiting>,
    _marker: PhantomData<&'a ()>,
}

impl<'a> SeqFrame<'a> {
    fn new(base_indent: usize) -> Self {
        Self {
            base_indent,
            items: Vec::new(),
            pending_comments: Vec::new(),
            waiting: None,
            _marker: PhantomData,
        }
    }

    fn step(&mut self, env: &mut ParseEnv<'a>) -> Result<FrameStep, ParseError> {
        if let Some(wait) = &self.waiting {
            return Ok(FrameStep::NeedChild {
                indent: wait.child_indent(),
            });
        }

        let line = match env.peek_line().copied() {
            Some(line) => line,
            None => {
                return Ok(FrameStep::Return(YamlValue::Seq(mem::take(
                    &mut self.items,
                ))));
            }
        };

        if line.indent < self.base_indent || !looks_like_seq(line.content) {
            return Ok(FrameStep::Return(YamlValue::Seq(mem::take(
                &mut self.items,
            ))));
        }

        if line.content.starts_with('#') {
            self.pending_comments.push(CommentLine {
                indent: line.indent,
                text: line.content.to_string(),
            });
            env.index += 1;
            return Ok(FrameStep::Continue);
        }

        if line.indent > self.base_indent {
            return Ok(FrameStep::Return(YamlValue::Seq(mem::take(
                &mut self.items,
            ))));
        }

        let (content_no_comment, inline_comment) = split_inline_comment(line.content);
        if !looks_like_seq(content_no_comment) {
            return Ok(FrameStep::Return(YamlValue::Seq(mem::take(
                &mut self.items,
            ))));
        }
        let after_dash = content_no_comment[1..].trim_start();
        env.index += 1;
        let inline_comment = inline_comment.map(|c| c.to_string());

        if after_dash.is_empty() {
            if env.index >= env.lines.len() || env.lines[env.index].indent <= self.base_indent {
                self.push_node(YamlValue::Str(String::new()), inline_comment);
                return Ok(FrameStep::Continue);
            }
            let child_indent = env.lines[env.index].indent;
            self.waiting = Some(SeqWaiting::Child {
                inline_comment,
                anchor: None,
                child_indent,
            });
            return Ok(FrameStep::NeedChild { indent: child_indent });
        }

        if after_dash == "|" {
            let s = parse_block_scalar(env.lines, &mut env.index, self.base_indent + 1)?;
            self.push_node(YamlValue::Str(s), inline_comment);
            return Ok(FrameStep::Continue);
        }

        if after_dash == "[]" {
            self.push_node(YamlValue::Seq(Vec::new()), inline_comment);
            return Ok(FrameStep::Continue);
        }

        if after_dash == "{}" {
            self.push_node(YamlValue::Map(BTreeMap::new()), inline_comment);
            return Ok(FrameStep::Continue);
        }

        if let Some(colon_pos) = after_dash.find(':') {
            return self.handle_inline_map(
                env,
                line,
                after_dash,
                colon_pos,
                inline_comment,
            );
        }

        if after_dash.starts_with('&') {
            if env.index >= env.lines.len() || env.lines[env.index].indent <= self.base_indent {
                return Err(ParseError::Generic {
                    line: line.line_no,
                    column: 1,
                    message: "anchor without nested value".to_string(),
                });
            }
            let child_indent = env.lines[env.index].indent;
            self.waiting = Some(SeqWaiting::Child {
                inline_comment,
                anchor: Some(after_dash[1..].trim().to_string()),
                child_indent,
            });
            return Ok(FrameStep::NeedChild { indent: child_indent });
        }

        if after_dash.starts_with('*') {
            let name = after_dash[1..].trim();
            let value = env
                .anchors
                .get(name)
                .cloned()
                .ok_or_else(|| ParseError::Generic {
                    line: line.line_no,
                    column: 1,
                    message: format!("unknown anchor: {name}"),
                })?;
            self.push_node(value, inline_comment);
            return Ok(FrameStep::Continue);
        }

        let scalar = strip_quotes(after_dash);
        self.push_node(YamlValue::Str(scalar.to_string()), inline_comment);
        Ok(FrameStep::Continue)
    }

    fn handle_inline_map(
        &mut self,
        env: &mut ParseEnv<'a>,
        line: Line<'a>,
        after_dash: &str,
        colon_pos: usize,
        inline_comment: Option<String>,
    ) -> Result<FrameStep, ParseError> {
        let (kpart, rest) = after_dash.split_at(colon_pos);
        let key = parse_key(kpart.trim(), line.line_no)?;
        let vpart = rest[1..].trim_start();
        let mut map = BTreeMap::new();
        let expected_indent = self.base_indent + 2;
        let outcome = parse_inline_value(
            env,
            vpart,
            line.line_no,
            expected_indent,
            colon_pos + 1,
        )?;
        match outcome {
            InlineValueOutcome::Ready(node) => {
                insert_inline_entry(
                    &mut map,
                    key,
                    node,
                    line.line_no,
                    colon_pos + 1,
                )?;
                if env.index < env.lines.len() && env.lines[env.index].indent > self.base_indent {
                    let child_indent = env.lines[env.index].indent;
                    self.waiting = Some(SeqWaiting::InlineMapContinuation {
                        map,
                        inline_comment,
                        child_indent,
                        line_no: line.line_no,
                        column: colon_pos + 1,
                    });
                    return Ok(FrameStep::NeedChild { indent: child_indent });
                }
                self.push_node(YamlValue::Map(map), inline_comment);
                Ok(FrameStep::Continue)
            }
            InlineValueOutcome::NeedsBlock(wait) => {
                self.waiting = Some(SeqWaiting::InlineAnchorValue {
                    map,
                    key,
                    inline_comment,
                    anchor_name: wait.anchor_name,
                    child_indent: wait.child_indent,
                    line_no: line.line_no,
                    column: colon_pos + 1,
                });
                Ok(FrameStep::NeedChild {
                    indent: wait.child_indent,
                })
            }
        }
    }

    fn handle_child(
        &mut self,
        value: YamlValue,
        env: &mut ParseEnv<'a>,
    ) -> Result<(), ParseError> {
        let waiting = self.waiting.take().ok_or_else(|| ParseError::Generic {
            line: 1,
            column: 1,
            message: "sequence not awaiting child".to_string(),
        })?;
        match waiting {
            SeqWaiting::Child {
                inline_comment,
                anchor,
                ..
            } => {
                if let Some(anchor) = anchor {
                    env.anchors.insert(anchor, value.clone());
                }
                self.push_node(value, inline_comment);
            }
            SeqWaiting::InlineMapContinuation {
                mut map,
                inline_comment,
                line_no,
                column,
                ..
            } => {
                let extra = expect_map(value, line_no, column, "inline mapping continuation")?;
                for (k, v) in extra {
                    map.insert(k, v);
                }
                self.push_node(YamlValue::Map(map), inline_comment);
            }
            SeqWaiting::InlineAnchorValue {
                mut map,
                key,
                inline_comment,
                anchor_name,
                line_no,
                column,
                ..
            } => {
                env.anchors.insert(anchor_name, value.clone());
                let node = YamlNode::new(value);
                insert_inline_entry(&mut map, key, node, line_no, column)?;
                if env.index < env.lines.len() && env.lines[env.index].indent > self.base_indent {
                    let child_indent = env.lines[env.index].indent;
                    self.waiting = Some(SeqWaiting::InlineMapContinuation {
                        map,
                        inline_comment,
                        child_indent,
                        line_no,
                        column,
                    });
                    return Ok(());
                }
                self.push_node(YamlValue::Map(map), inline_comment);
            }
        }
        Ok(())
    }

    fn push_node(&mut self, value: YamlValue, inline_comment: Option<String>) {
        let mut node = YamlNode::new(value);
        node.leading_comments = mem::take(&mut self.pending_comments);
        node.inline_comment = inline_comment;
        self.items.push(node);
    }
}

enum SeqWaiting {
    Child {
        inline_comment: Option<String>,
        anchor: Option<String>,
        child_indent: usize,
    },
    InlineMapContinuation {
        map: BTreeMap<String, YamlNode>,
        inline_comment: Option<String>,
        child_indent: usize,
        line_no: usize,
        column: usize,
    },
    InlineAnchorValue {
        map: BTreeMap<String, YamlNode>,
        key: String,
        inline_comment: Option<String>,
        anchor_name: String,
        child_indent: usize,
        line_no: usize,
        column: usize,
    },
}

impl SeqWaiting {
    fn child_indent(&self) -> usize {
        match self {
            SeqWaiting::Child { child_indent, .. } => *child_indent,
            SeqWaiting::InlineMapContinuation { child_indent, .. } => *child_indent,
            SeqWaiting::InlineAnchorValue { child_indent, .. } => *child_indent,
        }
    }
}

struct MapFrame<'a> {
    base_indent: usize,
    entries: BTreeMap<String, YamlNode>,
    pending_comments: Vec<CommentLine>,
    waiting: Option<MapWaiting>,
    _marker: PhantomData<&'a ()>,
}

impl<'a> MapFrame<'a> {
    fn new(base_indent: usize) -> Self {
        Self {
            base_indent,
            entries: BTreeMap::new(),
            pending_comments: Vec::new(),
            waiting: None,
            _marker: PhantomData,
        }
    }

    fn step(&mut self, env: &mut ParseEnv<'a>) -> Result<FrameStep, ParseError> {
        if let Some(wait) = &self.waiting {
            return Ok(FrameStep::NeedChild {
                indent: wait.child_indent,
            });
        }

        let line = match env.peek_line().copied() {
            Some(line) => line,
            None => {
                return Ok(FrameStep::Return(YamlValue::Map(mem::take(
                    &mut self.entries,
                ))));
            }
        };

        if line.indent < self.base_indent || looks_like_seq(line.content) {
            return Ok(FrameStep::Return(YamlValue::Map(mem::take(
                &mut self.entries,
            ))));
        }

        if line.content.starts_with('#') {
            self.pending_comments.push(CommentLine {
                indent: line.indent,
                text: line.content.to_string(),
            });
            env.index += 1;
            return Ok(FrameStep::Continue);
        }

        if line.indent > self.base_indent {
            return Ok(FrameStep::Return(YamlValue::Map(mem::take(
                &mut self.entries,
            ))));
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
        let key = parse_key(kpart.trim(), line.line_no)?;
        let vpart = rest[1..].trim_start();
        env.index += 1;
        let inline_comment = inline_comment.map(|c| c.to_string());

        if key == "<<" && vpart.starts_with('*') {
            let name = vpart[1..].trim();
            let aliased = env
                .anchors
                .get(name)
                .cloned()
                .ok_or_else(|| ParseError::Generic {
                    line: line.line_no,
                    column: colon_pos + 1,
                    message: format!("unknown anchor: {name}"),
                })?;
            let map = expect_map(aliased, line.line_no, colon_pos + 1, "merge source")?;
            for (k, v) in map {
                self.entries.entry(k).or_insert(v);
            }
            self.pending_comments.clear();
            return Ok(FrameStep::Continue);
        }

        if vpart.is_empty() {
            if env.index >= env.lines.len() || env.lines[env.index].indent <= self.base_indent {
                let value = YamlValue::Str(String::new());
                self.push_entry(key, value, inline_comment);
                return Ok(FrameStep::Continue);
            }
            let child_indent = env.lines[env.index].indent;
            self.waiting = Some(MapWaiting {
                key,
                inline_comment,
                anchor: None,
                child_indent,
            });
            return Ok(FrameStep::NeedChild { indent: child_indent });
        }

        if vpart == "|" {
            let s = parse_block_scalar(env.lines, &mut env.index, self.base_indent + 1)?;
            self.push_entry(key, YamlValue::Str(s), inline_comment);
            return Ok(FrameStep::Continue);
        }

        if vpart == "[]" {
            self.push_entry(key, YamlValue::Seq(Vec::new()), inline_comment);
            return Ok(FrameStep::Continue);
        }

        if vpart == "{}" {
            self.push_entry(key, YamlValue::Map(BTreeMap::new()), inline_comment);
            return Ok(FrameStep::Continue);
        }

        if vpart.starts_with('&') {
            if env.index >= env.lines.len() || env.lines[env.index].indent <= self.base_indent {
                return Err(ParseError::Generic {
                    line: line.line_no,
                    column: colon_pos + 1,
                    message: "anchor without nested value".to_string(),
                });
            }
            let child_indent = env.lines[env.index].indent;
            self.waiting = Some(MapWaiting {
                key,
                inline_comment,
                anchor: Some(vpart[1..].trim().to_string()),
                child_indent,
            });
            return Ok(FrameStep::NeedChild { indent: child_indent });
        }

        if vpart.starts_with('*') {
            let name = vpart[1..].trim();
            let value = env
                .anchors
                .get(name)
                .cloned()
                .ok_or_else(|| ParseError::Generic {
                    line: line.line_no,
                    column: colon_pos + 1,
                    message: format!("unknown anchor: {name}"),
                })?;
            self.push_entry(key, value, inline_comment);
            return Ok(FrameStep::Continue);
        }

        let scalar = strip_quotes(vpart);
        self.push_entry(key, YamlValue::Str(scalar.to_string()), inline_comment);
        Ok(FrameStep::Continue)
    }

    fn handle_child(
        &mut self,
        value: YamlValue,
        env: &mut ParseEnv<'a>,
    ) -> Result<(), ParseError> {
        let waiting = self.waiting.take().ok_or_else(|| ParseError::Generic {
            line: 1,
            column: 1,
            message: "mapping not awaiting child".to_string(),
        })?;
        if let Some(anchor) = waiting.anchor {
            env.anchors.insert(anchor, value.clone());
        }
        self.push_entry(waiting.key, value, waiting.inline_comment);
        Ok(())
    }

    fn push_entry(&mut self, key: String, value: YamlValue, inline_comment: Option<String>) {
        let mut node = YamlNode::new(value);
        node.leading_comments = mem::take(&mut self.pending_comments);
        node.inline_comment = inline_comment;
        self.entries.insert(key, node);
    }
}

struct MapWaiting {
    key: String,
    inline_comment: Option<String>,
    anchor: Option<String>,
    child_indent: usize,
}

enum InlineValueOutcome {
    Ready(YamlNode),
    NeedsBlock(InlineValueWait),
}

struct InlineValueWait {
    anchor_name: String,
    child_indent: usize,
}

fn parse_inline_value(
    env: &mut ParseEnv<'_>,
    vpart: &str,
    line_no: usize,
    expected_indent: usize,
    column: usize,
) -> Result<InlineValueOutcome, ParseError> {
    if (vpart.starts_with('"') && vpart.ends_with('"') && vpart.len() >= 2)
        || (vpart.starts_with('\'') && vpart.ends_with('\'') && vpart.len() >= 2)
    {
        return Ok(InlineValueOutcome::Ready(YamlNode::new(YamlValue::Str(
            strip_quotes(vpart).to_string(),
        ))));
    }

    if vpart == "|" {
        let s = parse_block_scalar(env.lines, &mut env.index, expected_indent)?;
        return Ok(InlineValueOutcome::Ready(YamlNode::new(YamlValue::Str(s))));
    }

    if vpart == "[]" {
        return Ok(InlineValueOutcome::Ready(YamlNode::new(YamlValue::Seq(Vec::new()))));
    }
    if vpart == "{}" {
        return Ok(InlineValueOutcome::Ready(YamlNode::new(YamlValue::Map(
            BTreeMap::new(),
        ))));
    }

    if vpart.starts_with('&') {
        let next = env.lines.get(env.index).ok_or_else(|| ParseError::Generic {
            line: line_no,
            column,
            message: "anchor without nested value".to_string(),
        })?;
        if next.indent <= expected_indent - 1 {
            return Err(ParseError::Generic {
                line: line_no,
                column,
                message: "anchor without nested value".to_string(),
            });
        }
        return Ok(InlineValueOutcome::NeedsBlock(InlineValueWait {
            anchor_name: vpart[1..].trim().to_string(),
            child_indent: next.indent,
        }));
    }

    if vpart.starts_with('*') {
        let name = vpart[1..].trim();
        let aliased = env
            .anchors
            .get(name)
            .cloned()
            .ok_or_else(|| ParseError::Generic {
                line: line_no,
                column,
                message: format!("unknown anchor: {name}"),
            })?;
        return Ok(InlineValueOutcome::Ready(YamlNode::new(aliased)));
    }

    Ok(InlineValueOutcome::Ready(YamlNode::new(YamlValue::Str(
        vpart.to_string(),
    ))))
}

fn insert_inline_entry(
    map: &mut BTreeMap<String, YamlNode>,
    key: String,
    mut node: YamlNode,
    line_no: usize,
    column: usize,
) -> Result<(), ParseError> {
    if key == "<<" {
        if let YamlValue::Map(extra) = node.value {
            for (k, v) in extra {
                map.entry(k).or_insert(v);
            }
            return Ok(());
        }
        return Err(ParseError::Generic {
            line: line_no,
            column,
            message: "merge source must be a mapping".to_string(),
        });
    }
    node.leading_comments.clear();
    node.inline_comment = None;
    map.insert(key, node);
    Ok(())
}

fn expect_map(
    value: YamlValue,
    line_no: usize,
    column: usize,
    context: &str,
) -> Result<BTreeMap<String, YamlNode>, ParseError> {
    match value {
        YamlValue::Map(map) => Ok(map),
        _ => Err(ParseError::Generic {
            line: line_no,
            column,
            message: format!("{context} must be a mapping"),
        }),
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
        YamlValue::Seq(seq) => {
            if seq.is_empty() {
                for _ in 0..indent {
                    out.push(' ');
                }
                out.push_str("[]\n");
                Ok(())
            } else {
                write_seq(out, seq, indent)
            }
        }
        YamlValue::Map(map) => {
            if map.is_empty() {
                for _ in 0..indent {
                    out.push(' ');
                }
                out.push_str("{}\n");
                Ok(())
            } else {
                write_map(out, map, indent)
            }
        }
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
                if child.is_empty() {
                    out.push_str("[]");
                    if let Some(comment) = &node.inline_comment {
                        out.push(' ');
                        out.push_str(comment);
                    }
                    out.push('\n');
                } else {
                    if let Some(comment) = &node.inline_comment {
                        out.push(' ');
                        out.push_str(comment);
                    }
                    out.push('\n');
                    write_seq(out, child, indent + 2)?;
                }
            }
            YamlValue::Map(map) => {
                if map.is_empty() {
                    out.push_str("{}");
                    if let Some(comment) = &node.inline_comment {
                        out.push(' ');
                        out.push_str(comment);
                    }
                    out.push('\n');
                } else {
                    if let Some(comment) = &node.inline_comment {
                        out.push(' ');
                        out.push_str(comment);
                    }
                    out.push('\n');
                    write_map(out, map, indent + 2)?;
                }
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
                if child.is_empty() {
                    out.push_str(" []");
                    if let Some(comment) = &node.inline_comment {
                        out.push(' ');
                        out.push_str(comment);
                    }
                    out.push('\n');
                } else {
                    if let Some(comment) = &node.inline_comment {
                        out.push(' ');
                        out.push_str(comment);
                    }
                    out.push('\n');
                    write_seq(out, child, indent + 2)?;
                }
            }
            YamlValue::Map(child) => {
                if child.is_empty() {
                    out.push_str(" {}");
                    if let Some(comment) = &node.inline_comment {
                        out.push(' ');
                        out.push_str(comment);
                    }
                    out.push('\n');
                } else {
                    if let Some(comment) = &node.inline_comment {
                        out.push(' ');
                        out.push_str(comment);
                    }
                    out.push('\n');
                    write_map(out, child, indent + 2)?;
                }
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
    _naay_version: "1.0" # force version
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
