use std::collections::HashMap;

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;
use url::Url;

use crate::note_rich_text::{NoteColor, NoteInlineMark, parse_color_tag};

const PROTECTED_PREFIX: &str = "⟦ideall:受保护块:";
const MAX_EDITABLE_TEXT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NoteProjection {
    pub text: String,
    pub protected_blocks: usize,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub(crate) enum NoteDocumentError {
    #[error("笔记正文超过 8 MiB 原生编辑上限")]
    TooLarge,
    #[error("受保护的富文本块占位已被修改、删除或重复")]
    ProtectedBlockChanged,
    #[error("代码块缺少结束的 ``` 标记")]
    UnclosedCodeFence,
    #[error("高级内容块缺少结束标记")]
    UnclosedDirective,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum EditableKind {
    Paragraph,
    Heading(u8),
    Quote,
    Rule,
    Code(String),
    Bullet(usize),
    Ordered(usize),
    Todo { indent: usize, checked: bool },
    Callout(String),
    Toggle(bool),
    Equation,
    Bookmark,
}

#[derive(Clone)]
struct OriginalBlock {
    value: Value,
    rendered: String,
    kind: Option<EditableKind>,
    protected_marker: Option<String>,
}

#[derive(Clone)]
struct ParsedBlock {
    rendered: String,
    kind: Option<EditableKind>,
    text: String,
    protected_index: Option<usize>,
}

pub(crate) fn project_note(content: &[Value]) -> NoteProjection {
    let originals = original_blocks(content);
    NoteProjection {
        text: originals
            .iter()
            .map(|block| block.rendered.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        protected_blocks: originals
            .iter()
            .filter(|block| block.protected_marker.is_some())
            .count(),
    }
}

pub(crate) fn apply_note_text(
    content: &[Value],
    text: &str,
) -> Result<Vec<Value>, NoteDocumentError> {
    if text.len() > MAX_EDITABLE_TEXT_BYTES {
        return Err(NoteDocumentError::TooLarge);
    }
    let originals = original_blocks(content);
    let protected = originals
        .iter()
        .enumerate()
        .filter_map(|(index, block)| {
            block
                .protected_marker
                .as_ref()
                .map(|marker| (marker.clone(), index))
        })
        .collect::<HashMap<_, _>>();
    let parsed = parse_text(text, &protected)?;

    let mut seen_protected = vec![false; originals.len()];
    for block in &parsed {
        if let Some(index) = block.protected_index {
            if seen_protected[index] {
                return Err(NoteDocumentError::ProtectedBlockChanged);
            }
            seen_protected[index] = true;
        }
    }
    if originals
        .iter()
        .enumerate()
        .any(|(index, block)| block.protected_marker.is_some() && !seen_protected[index])
    {
        return Err(NoteDocumentError::ProtectedBlockChanged);
    }

    let mut used_original = vec![false; originals.len()];
    for block in &parsed {
        if let Some(index) = block.protected_index {
            used_original[index] = true;
        }
    }
    let mut assignments = vec![None; parsed.len()];

    // Preserve the exact JSON, including inline marks and plugin properties,
    // whenever the editable projection did not change.
    for (parsed_index, block) in parsed.iter().enumerate() {
        if block.protected_index.is_some() {
            continue;
        }
        if let Some((original_index, _)) = originals.iter().enumerate().find(|(index, original)| {
            !used_original[*index] && original.kind.is_some() && original.rendered == block.rendered
        }) {
            used_original[original_index] = true;
            assignments[parsed_index] = Some(originals[original_index].value.clone());
        }
    }

    for (parsed_index, block) in parsed.iter().enumerate() {
        if let Some(original_index) = block.protected_index {
            assignments[parsed_index] = Some(originals[original_index].value.clone());
            continue;
        }
        if assignments[parsed_index].is_some() {
            continue;
        }
        let original_index = originals
            .get(parsed_index)
            .filter(|original| {
                !used_original[parsed_index]
                    && same_kind(original.kind.as_ref(), block.kind.as_ref())
            })
            .map(|_| parsed_index)
            .or_else(|| {
                originals.iter().enumerate().position(|(index, original)| {
                    !used_original[index] && same_kind(original.kind.as_ref(), block.kind.as_ref())
                })
            });
        let original = original_index.map(|index| {
            used_original[index] = true;
            &originals[index].value
        });
        assignments[parsed_index] = Some(build_block(block, original));
    }

    Ok(assignments.into_iter().flatten().collect())
}

fn original_blocks(content: &[Value]) -> Vec<OriginalBlock> {
    content
        .iter()
        .enumerate()
        .map(|(index, value)| {
            if let Some((kind, rendered)) = render_editable_block(value) {
                OriginalBlock {
                    value: value.clone(),
                    rendered,
                    kind: Some(kind),
                    protected_marker: None,
                }
            } else {
                let marker = protected_marker(index, value);
                OriginalBlock {
                    value: value.clone(),
                    rendered: marker.clone(),
                    kind: None,
                    protected_marker: Some(marker),
                }
            }
        })
        .collect()
}

fn render_editable_block(value: &Value) -> Option<(EditableKind, String)> {
    let object = value.as_object()?;
    let block_type = object.get("type")?.as_str()?;
    if block_type == "code_block" {
        let language = object
            .get("lang")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let lines = object
            .get("children")?
            .as_array()?
            .iter()
            .map(node_text)
            .collect::<Option<Vec<_>>>()?;
        return Some((
            EditableKind::Code(language.clone()),
            format!("```{language}\n{}\n```", lines.join("\n")),
        ));
    }
    if block_type == "hr" {
        return Some((EditableKind::Rule, "---".into()));
    }
    if block_type == "equation" {
        let expression = object
            .get("expression")
            .and_then(Value::as_str)
            .or_else(|| object.get("tex").and_then(Value::as_str))
            .unwrap_or_default();
        return Some((EditableKind::Equation, format!("$$\n{expression}\n$$")));
    }
    if block_type == "bookmark" {
        let url = object.get("url").and_then(Value::as_str)?;
        let parsed = Url::parse(url).ok()?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return None;
        }
        return Some((EditableKind::Bookmark, format!(":::bookmark\n{url}\n:::")));
    }

    let text = render_inline_node(value)?;
    if block_type == "callout" {
        let icon = object
            .get("icon")
            .and_then(Value::as_str)
            .filter(|icon| !icon.trim().is_empty())
            .unwrap_or("💡")
            .to_owned();
        return Some((
            EditableKind::Callout(icon.clone()),
            format!(":::callout {icon}\n{text}\n:::"),
        ));
    }
    if block_type == "toggle" {
        let expanded = object
            .get("expanded")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        return Some((
            EditableKind::Toggle(expanded),
            format!(
                ":::toggle {}\n{text}\n:::",
                if expanded { "open" } else { "closed" }
            ),
        ));
    }
    let indent = object
        .get("indent")
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .clamp(1, 32) as usize;
    if let Some(list_type) = object.get("listStyleType").and_then(Value::as_str) {
        let padding = "  ".repeat(indent.saturating_sub(1));
        return match list_type {
            "todo" => {
                let checked = object
                    .get("checked")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                Some((
                    EditableKind::Todo { indent, checked },
                    format!("{padding}- [{}] {text}", if checked { "x" } else { " " }),
                ))
            }
            "decimal" | "lower-alpha" | "upper-alpha" | "lower-roman" | "upper-roman" => {
                Some((EditableKind::Ordered(indent), format!("{padding}1. {text}")))
            }
            _ => Some((EditableKind::Bullet(indent), format!("{padding}- {text}"))),
        };
    }

    match block_type {
        "p" => Some((EditableKind::Paragraph, text)),
        "blockquote" => Some((
            EditableKind::Quote,
            text.lines()
                .map(|line| format!("> {line}"))
                .collect::<Vec<_>>()
                .join("\n"),
        )),
        value
            if value.len() == 2
                && value.starts_with('h')
                && value[1..]
                    .parse::<u8>()
                    .is_ok_and(|level| (1..=6).contains(&level)) =>
        {
            let level = value[1..].parse::<u8>().ok()?;
            Some((
                EditableKind::Heading(level),
                format!("{} {text}", "#".repeat(level as usize)),
            ))
        }
        _ => None,
    }
}

fn node_text(value: &Value) -> Option<String> {
    let object = value.as_object()?;
    if let Some(text) = object.get("text").and_then(Value::as_str) {
        return Some(text.to_owned());
    }
    let children = object.get("children")?.as_array()?;
    let mut text = String::new();
    for child in children {
        text.push_str(&node_text(child)?);
    }
    Some(text)
}

fn render_inline_node(value: &Value) -> Option<String> {
    let object = value.as_object()?;
    if let Some(text) = object.get("text").and_then(Value::as_str) {
        let mut rendered = escape_inline_text(text);
        if object.get("code").and_then(Value::as_bool) == Some(true) {
            rendered = format!("`{rendered}`");
        }
        if object.get("bold").and_then(Value::as_bool) == Some(true) {
            rendered = format!("**{rendered}**");
        }
        if object.get("italic").and_then(Value::as_bool) == Some(true) {
            rendered = format!("*{rendered}*");
        }
        if object.get("strikethrough").and_then(Value::as_bool) == Some(true) {
            rendered = format!("~~{rendered}~~");
        }
        if let Some(color) = object
            .get("backgroundColor")
            .and_then(Value::as_str)
            .and_then(NoteColor::from_background_css)
        {
            rendered = format!(
                "<mark style=\"background-color:{}\">{rendered}</mark>",
                color.background_css()
            );
        }
        if let Some(color) = object
            .get("color")
            .and_then(Value::as_str)
            .and_then(NoteColor::from_foreground_css)
        {
            rendered = format!(
                "<span style=\"color:{}\">{rendered}</span>",
                color.foreground_css()
            );
        }
        return Some(rendered);
    }

    let children = object.get("children")?.as_array()?;
    let rendered = children
        .iter()
        .map(render_inline_node)
        .collect::<Option<Vec<_>>>()?
        .join("");
    if object.get("type").and_then(Value::as_str) == Some("a") {
        let url = object.get("url").and_then(Value::as_str)?;
        Some(format!("[{rendered}]({url})"))
    } else {
        Some(rendered)
    }
}

fn protected_marker(index: usize, value: &Value) -> String {
    let digest = Sha256::digest(serde_json::to_vec(value).unwrap_or_default());
    let fingerprint = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let block_type = value
        .as_object()
        .and_then(|object| object.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    format!("{PROTECTED_PREFIX}{index}:{block_type}:{fingerprint}⟧")
}

fn parse_text(
    text: &str,
    protected: &HashMap<String, usize>,
) -> Result<Vec<ParsedBlock>, NoteDocumentError> {
    let lines = text.split('\n').collect::<Vec<_>>();
    let mut result = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        if let Some(protected_index) = protected.get(line) {
            result.push(ParsedBlock {
                rendered: line.into(),
                kind: None,
                text: String::new(),
                protected_index: Some(*protected_index),
            });
            index += 1;
            continue;
        }
        if let Some(icon) = line.strip_prefix(":::callout ") {
            let start = index;
            let icon = icon.trim();
            let icon = if icon.is_empty() { "💡" } else { icon };
            let text = parse_directive_body(&lines, &mut index)?;
            result.push(ParsedBlock {
                rendered: lines[start..index].join("\n"),
                kind: Some(EditableKind::Callout(icon.to_owned())),
                text,
                protected_index: None,
            });
            continue;
        }
        if let Some(state) = line.strip_prefix(":::toggle ") {
            let start = index;
            let expanded = state.trim() != "closed";
            let text = parse_directive_body(&lines, &mut index)?;
            result.push(ParsedBlock {
                rendered: lines[start..index].join("\n"),
                kind: Some(EditableKind::Toggle(expanded)),
                text,
                protected_index: None,
            });
            continue;
        }
        if line == ":::bookmark" {
            let start = index;
            let text = parse_directive_body(&lines, &mut index)?;
            result.push(ParsedBlock {
                rendered: lines[start..index].join("\n"),
                kind: Some(EditableKind::Bookmark),
                text,
                protected_index: None,
            });
            continue;
        }
        if line == "$$" {
            let start = index;
            index += 1;
            let mut equation = Vec::new();
            while index < lines.len() && lines[index] != "$$" {
                equation.push(lines[index]);
                index += 1;
            }
            if index == lines.len() {
                return Err(NoteDocumentError::UnclosedDirective);
            }
            index += 1;
            result.push(ParsedBlock {
                rendered: lines[start..index].join("\n"),
                kind: Some(EditableKind::Equation),
                text: equation.join("\n"),
                protected_index: None,
            });
            continue;
        }
        if line.starts_with(PROTECTED_PREFIX) {
            return Err(NoteDocumentError::ProtectedBlockChanged);
        }
        if let Some(language) = line.strip_prefix("```") {
            let start = index;
            index += 1;
            let mut code = Vec::new();
            while index < lines.len() && lines[index] != "```" {
                code.push(lines[index]);
                index += 1;
            }
            if index == lines.len() {
                return Err(NoteDocumentError::UnclosedCodeFence);
            }
            index += 1;
            result.push(ParsedBlock {
                rendered: lines[start..index].join("\n"),
                kind: Some(EditableKind::Code(language.to_owned())),
                text: code.join("\n"),
                protected_index: None,
            });
            continue;
        }
        if line.starts_with("> ") {
            let start = index;
            let mut quote = Vec::new();
            while index < lines.len() {
                let Some(value) = lines[index].strip_prefix("> ") else {
                    break;
                };
                quote.push(value);
                index += 1;
            }
            result.push(ParsedBlock {
                rendered: lines[start..index].join("\n"),
                kind: Some(EditableKind::Quote),
                text: quote.join("\n"),
                protected_index: None,
            });
            continue;
        }

        let start = index;
        let mut logical_line = line.to_owned();
        index += 1;
        while has_markdown_hard_break(&logical_line) && index < lines.len() {
            logical_line.push('\n');
            logical_line.push_str(lines[index]);
            index += 1;
        }
        let (kind, value) = parse_line(&logical_line);
        result.push(ParsedBlock {
            rendered: lines[start..index].join("\n"),
            kind: Some(kind),
            text: value,
            protected_index: None,
        });
    }
    if result.is_empty() {
        result.push(ParsedBlock {
            rendered: String::new(),
            kind: Some(EditableKind::Paragraph),
            text: String::new(),
            protected_index: None,
        });
    }
    Ok(result)
}

fn has_markdown_hard_break(line: &str) -> bool {
    line.chars()
        .rev()
        .take_while(|character| *character == '\\')
        .count()
        % 2
        == 1
}

fn parse_directive_body(lines: &[&str], index: &mut usize) -> Result<String, NoteDocumentError> {
    *index += 1;
    let mut body = Vec::new();
    while *index < lines.len() && lines[*index] != ":::" {
        body.push(lines[*index]);
        *index += 1;
    }
    if *index == lines.len() {
        return Err(NoteDocumentError::UnclosedDirective);
    }
    *index += 1;
    Ok(body.join("\n"))
}

fn parse_line(line: &str) -> (EditableKind, String) {
    if line == "---" {
        return (EditableKind::Rule, String::new());
    }
    let heading = line
        .chars()
        .take_while(|character| *character == '#')
        .count();
    if (1..=6).contains(&heading) && line.as_bytes().get(heading) == Some(&b' ') {
        return (
            EditableKind::Heading(heading as u8),
            line[heading + 1..].to_owned(),
        );
    }

    let leading_spaces = line
        .len()
        .saturating_sub(line.trim_start_matches(' ').len());
    let indent = leading_spaces / 2 + 1;
    let value = &line[leading_spaces..];
    for (prefix, checked) in [("- [ ] ", false), ("- [x] ", true), ("- [X] ", true)] {
        if let Some(text) = value.strip_prefix(prefix) {
            return (EditableKind::Todo { indent, checked }, text.to_owned());
        }
    }
    if let Some(text) = value
        .strip_prefix("- ")
        .or_else(|| value.strip_prefix("* "))
    {
        return (EditableKind::Bullet(indent), text.to_owned());
    }
    if let Some((number, text)) = value.split_once(". ")
        && !number.is_empty()
        && number.bytes().all(|byte| byte.is_ascii_digit())
    {
        return (EditableKind::Ordered(indent), text.to_owned());
    }
    if let Some((number, text)) = value.split_once(") ")
        && !number.is_empty()
        && number.bytes().all(|byte| byte.is_ascii_digit())
    {
        return (EditableKind::Ordered(indent), text.to_owned());
    }
    (EditableKind::Paragraph, line.to_owned())
}

fn same_kind(left: Option<&EditableKind>, right: Option<&EditableKind>) -> bool {
    match (left, right) {
        (Some(EditableKind::Heading(_)), Some(EditableKind::Heading(_)))
        | (Some(EditableKind::Code(_)), Some(EditableKind::Code(_)))
        | (Some(EditableKind::Callout(_)), Some(EditableKind::Callout(_)))
        | (Some(EditableKind::Toggle(_)), Some(EditableKind::Toggle(_)))
        | (Some(EditableKind::Bullet(_)), Some(EditableKind::Bullet(_)))
        | (Some(EditableKind::Ordered(_)), Some(EditableKind::Ordered(_)))
        | (Some(EditableKind::Todo { .. }), Some(EditableKind::Todo { .. })) => true,
        (Some(left), Some(right)) => std::mem::discriminant(left) == std::mem::discriminant(right),
        _ => false,
    }
}

fn build_block(block: &ParsedBlock, original: Option<&Value>) -> Value {
    let mut object = Map::new();
    if let Some(id) = original
        .and_then(Value::as_object)
        .and_then(|object| object.get("id"))
        .and_then(Value::as_str)
    {
        object.insert("id".into(), Value::String(id.into()));
    }
    match block.kind.as_ref().unwrap_or(&EditableKind::Paragraph) {
        EditableKind::Paragraph => set_text_block(&mut object, "p", &block.text),
        EditableKind::Heading(level) => {
            set_text_block(&mut object, &format!("h{level}"), &block.text);
        }
        EditableKind::Quote => {
            object.insert("type".into(), Value::String("blockquote".into()));
            object.insert(
                "children".into(),
                json!([{"type": "p", "children": parse_inline_children(&block.text)}]),
            );
        }
        EditableKind::Rule => {
            object.insert("type".into(), Value::String("hr".into()));
            object.insert("children".into(), json!([{"text": ""}]));
        }
        EditableKind::Code(language) => {
            object.insert("type".into(), Value::String("code_block".into()));
            if !language.is_empty() {
                object.insert("lang".into(), Value::String(language.clone()));
            }
            object.insert(
                "children".into(),
                Value::Array(
                    block
                        .text
                        .split('\n')
                        .map(|line| json!({"type": "code_line", "children": [{"text": line}]}))
                        .collect(),
                ),
            );
        }
        EditableKind::Bullet(indent) => {
            set_list_block(&mut object, &block.text, "disc", *indent, None);
        }
        EditableKind::Ordered(indent) => {
            set_list_block(&mut object, &block.text, "decimal", *indent, None);
        }
        EditableKind::Todo { indent, checked } => {
            set_list_block(&mut object, &block.text, "todo", *indent, Some(*checked));
        }
        EditableKind::Callout(icon) => {
            set_text_block(&mut object, "callout", &block.text);
            object.insert("icon".into(), Value::String(icon.clone()));
        }
        EditableKind::Toggle(expanded) => {
            set_text_block(&mut object, "toggle", &block.text);
            object.insert("expanded".into(), Value::Bool(*expanded));
        }
        EditableKind::Equation => {
            object.insert("type".into(), Value::String("equation".into()));
            object.insert("expression".into(), Value::String(block.text.clone()));
            object.insert("children".into(), json!([{"text": ""}]));
        }
        EditableKind::Bookmark => {
            let safe_url = Url::parse(&block.text)
                .ok()
                .filter(|url| matches!(url.scheme(), "http" | "https"));
            if safe_url.is_some() {
                object.insert("type".into(), Value::String("bookmark".into()));
                object.insert("url".into(), Value::String(block.text.clone()));
                object.insert("children".into(), json!([{"text": ""}]));
            } else {
                set_text_block(&mut object, "p", &block.text);
            }
        }
    }
    Value::Object(object)
}

fn set_text_block(object: &mut Map<String, Value>, block_type: &str, text: &str) {
    object.insert("type".into(), Value::String(block_type.into()));
    object.insert("children".into(), Value::Array(parse_inline_children(text)));
}

#[derive(Clone, Copy, Debug, Default)]
struct InlineMarks {
    bold: bool,
    italic: bool,
    strikethrough: bool,
    code: bool,
    text_color: Option<NoteColor>,
    highlight: Option<NoteColor>,
}

fn parse_inline_children(text: &str) -> Vec<Value> {
    let mut children = Vec::new();
    parse_inline_into(text, InlineMarks::default(), &mut children);
    if children.is_empty() {
        children.push(json!({"text": ""}));
    }
    children
}

fn parse_inline_into(text: &str, marks: InlineMarks, children: &mut Vec<Value>) {
    let mut offset = 0;
    while offset < text.len() {
        let remaining = &text[offset..];
        if remaining.starts_with("\\\n") {
            children.push(text_leaf("\n", marks));
            offset += 2;
            continue;
        }
        if let Some(escaped) = parse_inline_escape(remaining) {
            children.push(text_leaf(escaped, marks));
            offset += 1 + escaped.len();
            continue;
        }
        if let Some((consumed, label, url)) = parse_link(remaining) {
            let mut link_children = Vec::new();
            parse_inline_into(label, marks, &mut link_children);
            if link_children.is_empty() {
                link_children.push(text_leaf("", marks));
            }
            children.push(json!({
                "type": "a",
                "url": url,
                "children": link_children,
            }));
            offset += consumed;
            continue;
        }
        if let Some((opening_len, closing, mark)) = parse_color_tag(remaining) {
            let inner = &remaining[opening_len..];
            if let Some(end) = find_unescaped_inline(inner, closing) {
                let nested_marks = match mark {
                    NoteInlineMark::TextColor(color) => InlineMarks {
                        text_color: Some(color),
                        ..marks
                    },
                    NoteInlineMark::Highlight(color) => InlineMarks {
                        highlight: Some(color),
                        ..marks
                    },
                    _ => marks,
                };
                parse_inline_into(&inner[..end], nested_marks, children);
                offset += opening_len + end + closing.len();
                continue;
            }
        }

        let delimiter = if remaining.starts_with("***") {
            Some((
                "***",
                InlineMarks {
                    bold: true,
                    italic: true,
                    ..marks
                },
            ))
        } else if remaining.starts_with("**") {
            Some((
                "**",
                InlineMarks {
                    bold: true,
                    ..marks
                },
            ))
        } else if remaining.starts_with("~~") {
            Some((
                "~~",
                InlineMarks {
                    strikethrough: true,
                    ..marks
                },
            ))
        } else if remaining.starts_with('`') {
            Some((
                "`",
                InlineMarks {
                    code: true,
                    ..marks
                },
            ))
        } else if remaining.starts_with('*') {
            Some((
                "*",
                InlineMarks {
                    italic: true,
                    ..marks
                },
            ))
        } else {
            None
        };
        if let Some((delimiter, nested_marks)) = delimiter {
            let inner = &remaining[delimiter.len()..];
            if let Some(end) = find_unescaped_inline(inner, delimiter) {
                parse_inline_into(&inner[..end], nested_marks, children);
                offset += delimiter.len() + end + delimiter.len();
                continue;
            }
        }

        let next = remaining
            .char_indices()
            .skip(1)
            .find(|(_, character)| matches!(character, '\\' | '*' | '~' | '`' | '[' | '<'))
            .map_or(remaining.len(), |(index, _)| index);
        let consumed = next.max(remaining.chars().next().map_or(0, char::len_utf8));
        children.push(text_leaf(&remaining[..consumed], marks));
        offset += consumed;
    }
}

fn escape_inline_text(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if character == '\n' {
            escaped.push_str("\\\n");
            continue;
        }
        if is_inline_escapable(character) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn parse_inline_escape(value: &str) -> Option<&str> {
    let escaped = value.strip_prefix('\\')?;
    let character = escaped.chars().next()?;
    is_inline_escapable(character).then(|| &escaped[..character.len_utf8()])
}

fn is_inline_escapable(character: char) -> bool {
    matches!(
        character,
        '\\' | '*' | '_' | '~' | '`' | '[' | ']' | '(' | ')' | '<' | '>'
    )
}

fn find_unescaped_inline(value: &str, needle: &str) -> Option<usize> {
    value.char_indices().find_map(|(index, _)| {
        (value[index..].starts_with(needle) && !inline_character_is_escaped(value, index))
            .then_some(index)
    })
}

fn inline_character_is_escaped(value: &str, index: usize) -> bool {
    value[..index]
        .chars()
        .rev()
        .take_while(|character| *character == '\\')
        .count()
        % 2
        == 1
}

fn parse_link(value: &str) -> Option<(usize, &str, &str)> {
    let body = value.strip_prefix('[')?;
    let label_end = find_unescaped_inline(body, "](")?;
    let label = &body[..label_end];
    let url_start = 1 + label_end + 2;
    let url_end = find_unescaped_inline(&value[url_start..], ")")? + url_start;
    let url = &value[url_start..url_end];
    let safe_external = Url::parse(url)
        .ok()
        .is_some_and(|parsed| matches!(parsed.scheme(), "http" | "https"));
    if !safe_external && crate::note_rich_text::note_node_id_from_url(url).is_none() {
        return None;
    }
    Some((url_end + 1, label, url))
}

fn text_leaf(text: &str, marks: InlineMarks) -> Value {
    let mut leaf = Map::new();
    leaf.insert("text".into(), Value::String(text.into()));
    if marks.bold {
        leaf.insert("bold".into(), Value::Bool(true));
    }
    if marks.italic {
        leaf.insert("italic".into(), Value::Bool(true));
    }
    if marks.strikethrough {
        leaf.insert("strikethrough".into(), Value::Bool(true));
    }
    if marks.code {
        leaf.insert("code".into(), Value::Bool(true));
    }
    if let Some(color) = marks.text_color {
        leaf.insert("color".into(), Value::String(color.foreground_css().into()));
    }
    if let Some(color) = marks.highlight {
        leaf.insert(
            "backgroundColor".into(),
            Value::String(color.background_css().into()),
        );
    }
    Value::Object(leaf)
}

fn set_list_block(
    object: &mut Map<String, Value>,
    text: &str,
    list_type: &str,
    indent: usize,
    checked: Option<bool>,
) {
    set_text_block(object, "p", text);
    object.insert("listStyleType".into(), Value::String(list_type.into()));
    object.insert("indent".into(), json!(indent));
    if let Some(checked) = checked {
        object.insert("checked".into(), Value::Bool(checked));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_supported_blocks_keep_exact_rich_json() {
        let content = vec![json!({
            "id": "heading",
            "type": "h1",
            "children": [
                {"text": "Rich", "bold": true},
                {"text": " heading", "italic": true}
            ],
            "custom": {"kept": true}
        })];
        let projection = project_note(&content);
        assert_eq!(projection.text, "# **Rich*** heading*");
        assert_eq!(
            apply_note_text(&content, &projection.text).unwrap(),
            content
        );
    }

    #[test]
    fn edits_common_blocks_while_preserving_unknown_json() {
        let unknown = json!({
            "id": "image",
            "type": "img",
            "url": "asset://secret",
            "children": [{"text": ""}],
            "future": {"v": 2}
        });
        let content = vec![
            json!({"id": "heading", "type": "h1", "children": [{"text": "Old"}]}),
            unknown.clone(),
            json!({"id": "body", "type": "p", "children": [{"text": "Body"}]}),
        ];
        let projection = project_note(&content);
        assert_eq!(projection.protected_blocks, 1);
        let edited = projection.text.replacen("# Old", "# New", 1);
        let result = apply_note_text(&content, &edited).unwrap();
        assert_eq!(result[0]["id"], "heading");
        assert_eq!(result[0]["children"][0]["text"], "New");
        assert_eq!(result[1], unknown);
        assert_eq!(result[2], content[2]);
    }

    #[test]
    fn protected_marker_cannot_be_removed_changed_or_duplicated() {
        let content = vec![json!({"type": "future", "payload": {"keep": true}})];
        let projection = project_note(&content);
        assert_eq!(
            apply_note_text(&content, "deleted"),
            Err(NoteDocumentError::ProtectedBlockChanged)
        );
        assert_eq!(
            apply_note_text(
                &content,
                &format!("{}\n{}", projection.text, projection.text)
            ),
            Err(NoteDocumentError::ProtectedBlockChanged)
        );
    }

    #[test]
    fn parses_lists_tasks_quotes_rules_and_code() {
        let text = "- bullet\n  1. ordered\n- [x] done\n> quote\n---\n```rust\nfn main() {}\n```";
        let result = apply_note_text(&[], text).unwrap();
        assert_eq!(result.len(), 6);
        assert_eq!(result[0]["listStyleType"], "disc");
        assert_eq!(result[1]["listStyleType"], "decimal");
        assert_eq!(result[1]["indent"], 2);
        assert_eq!(result[2]["checked"], true);
        assert_eq!(result[3]["type"], "blockquote");
        assert_eq!(result[4]["type"], "hr");
        assert_eq!(result[5]["lang"], "rust");
    }

    #[test]
    fn advanced_document_blocks_round_trip_with_safe_metadata() {
        let text = ":::callout ⚠️\nRead **this**\n:::\n:::toggle closed\nDetails\n:::\n$$\nx^2 + y^2\n$$\n:::bookmark\nhttps://example.com/docs\n:::";
        let result = apply_note_text(&[], text).unwrap();

        assert_eq!(result.len(), 4);
        assert_eq!(result[0]["type"], "callout");
        assert_eq!(result[0]["icon"], "⚠️");
        assert_eq!(result[0]["children"][1]["bold"], true);
        assert_eq!(result[1]["type"], "toggle");
        assert_eq!(result[1]["expanded"], false);
        assert_eq!(result[2]["type"], "equation");
        assert_eq!(result[2]["expression"], "x^2 + y^2");
        assert_eq!(result[3]["type"], "bookmark");
        assert_eq!(result[3]["url"], "https://example.com/docs");
        assert_eq!(project_note(&result).text, text);

        let unsafe_bookmark =
            apply_note_text(&[], ":::bookmark\njavascript:alert(1)\n:::").unwrap();
        assert_eq!(unsafe_bookmark[0]["type"], "p");
        assert_eq!(
            unsafe_bookmark[0]["children"][0]["text"],
            "javascript:alert(1)"
        );
    }

    #[test]
    fn advanced_document_blocks_require_closing_markers() {
        assert_eq!(
            apply_note_text(&[], ":::callout 💡\nmissing"),
            Err(NoteDocumentError::UnclosedDirective)
        );
        assert_eq!(
            apply_note_text(&[], "$$\nx + y"),
            Err(NoteDocumentError::UnclosedDirective)
        );
    }

    #[test]
    fn inline_markdown_round_trips_as_plate_marks_and_safe_links() {
        let text = "A **bold** *italic* ***both*** ~~gone~~ `code` [site](https://example.com)";
        let result = apply_note_text(&[], text).unwrap();
        let children = result[0]["children"].as_array().unwrap();

        assert!(children.iter().any(|child| child["bold"] == true));
        assert!(children.iter().any(|child| child["italic"] == true));
        assert!(
            children
                .iter()
                .any(|child| child["bold"] == true && child["italic"] == true)
        );
        assert!(children.iter().any(|child| child["strikethrough"] == true));
        assert!(children.iter().any(|child| child["code"] == true));
        assert!(children.iter().any(|child| {
            child["type"] == "a"
                && child["url"] == "https://example.com"
                && child["children"][0]["text"] == "site"
        }));
        assert_eq!(project_note(&result).text, text);

        let unsafe_link = apply_note_text(&[], "[bad](javascript:alert(1))").unwrap();
        assert_eq!(
            unsafe_link[0]["children"][0]["text"],
            "[bad](javascript:alert(1))"
        );
    }

    #[test]
    fn internal_note_mentions_round_trip_as_strict_plate_links() {
        let text = "See [@Project](ideall://node/page-123)";
        let result = apply_note_text(&[], text).unwrap();

        assert_eq!(result[0]["children"][1]["type"], "a");
        assert_eq!(result[0]["children"][1]["url"], "ideall://node/page-123");
        assert_eq!(project_note(&result).text, text);

        let unsafe_link = apply_note_text(&[], "[@Bad](ideall://node/../secret)").unwrap();
        assert_eq!(
            unsafe_link[0]["children"][0]["text"],
            "[@Bad](ideall://node/../secret)"
        );
    }

    #[test]
    fn semantic_colors_round_trip_as_standard_plate_leaf_properties() {
        let text = concat!(
            "<span style=\"color:#337ea9\">blue</span> ",
            "<mark style=\"background-color:#fbf3db\">highlight</mark>"
        );
        let result = apply_note_text(&[], text).unwrap();
        let children = result[0]["children"].as_array().unwrap();

        assert!(
            children
                .iter()
                .any(|child| { child["text"] == "blue" && child["color"] == "#337ea9" })
        );
        assert!(children.iter().any(|child| {
            child["text"] == "highlight" && child["backgroundColor"] == "#fbf3db"
        }));
        assert_eq!(project_note(&result).text, text);
    }

    #[test]
    fn literal_markdown_punctuation_remains_plain_plate_text() {
        let literal = r"\ *literal* _word_ ~~gone~~ `tick` [label](target)";
        let content = vec![json!({
            "id": "literal",
            "type": "p",
            "children": [{"text": literal}]
        })];
        let projection = project_note(&content);

        assert_eq!(
            projection.text,
            r"\\ \*literal\* \_word\_ \~\~gone\~\~ \`tick\` \[label\]\(target\)"
        );
        assert_eq!(
            apply_note_text(&content, &projection.text).unwrap(),
            content
        );
    }

    #[test]
    fn soft_breaks_remain_inside_one_plate_block() {
        let content = vec![json!({
            "id": "soft-break",
            "type": "p",
            "children": [{"text": "first\nsecond", "bold": true}]
        })];
        let projection = project_note(&content);

        assert_eq!(projection.text, "**first\\\nsecond**");
        assert_eq!(
            apply_note_text(&content, &projection.text).unwrap(),
            content
        );
    }
}
