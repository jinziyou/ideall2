//! UI-independent editing operations for the Markdown projection of a note.
//!
//! Desktop and mobile clients share these operations so a Plate document keeps
//! the same block semantics regardless of the surface used to edit it.

use std::ops::Range;

use crate::note_document::note_block_ranges;

const PROTECTED_PREFIX: &str = "⟦ideall:受保护块:";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoteBlockStyle {
    Paragraph,
    Heading1,
    Heading2,
    Heading3,
    Quote,
    Bullet,
    Ordered,
    Todo,
    Code,
    Rule,
    Callout,
    Toggle,
    Equation,
    Bookmark,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoteBlockMove {
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoteBlockIndent {
    In,
    Out,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoteInlineStyle {
    Bold,
    Italic,
    Strikethrough,
    Code,
    Link,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoteEditError {
    ProtectedBlock,
    CodeBlock,
    UnclosedCodeFence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NoteSlashCommand {
    pub id: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    pub style: NoteBlockStyle,
}

const SLASH_COMMANDS: &[(&str, &str, &str, &[&str], NoteBlockStyle)] = &[
    (
        "paragraph",
        "正文",
        "普通文本块",
        &["p", "text", "正文"],
        NoteBlockStyle::Paragraph,
    ),
    (
        "heading-1",
        "标题 1",
        "页面主标题",
        &["h1", "heading1", "标题1"],
        NoteBlockStyle::Heading1,
    ),
    (
        "heading-2",
        "标题 2",
        "章节标题",
        &["h2", "heading2", "标题2"],
        NoteBlockStyle::Heading2,
    ),
    (
        "heading-3",
        "标题 3",
        "小节标题",
        &["h3", "heading3", "标题3"],
        NoteBlockStyle::Heading3,
    ),
    (
        "quote",
        "引用",
        "突出一段引用",
        &["quote", "blockquote", "引用"],
        NoteBlockStyle::Quote,
    ),
    (
        "bullet",
        "项目列表",
        "创建无序列表",
        &["ul", "bullet", "list", "项目"],
        NoteBlockStyle::Bullet,
    ),
    (
        "ordered",
        "编号列表",
        "创建有序列表",
        &["ol", "ordered", "number", "编号"],
        NoteBlockStyle::Ordered,
    ),
    (
        "todo",
        "待办事项",
        "创建可勾选任务",
        &["todo", "task", "任务", "待办"],
        NoteBlockStyle::Todo,
    ),
    (
        "code",
        "代码块",
        "创建多行代码",
        &["code", "代码"],
        NoteBlockStyle::Code,
    ),
    (
        "rule",
        "分隔线",
        "划分内容区域",
        &["hr", "rule", "divider", "分隔线"],
        NoteBlockStyle::Rule,
    ),
    (
        "callout",
        "提示框",
        "创建带图标的强调内容",
        &["callout", "notice", "提示", "强调"],
        NoteBlockStyle::Callout,
    ),
    (
        "toggle",
        "折叠块",
        "创建可展开和收起的内容",
        &["toggle", "details", "折叠"],
        NoteBlockStyle::Toggle,
    ),
    (
        "equation",
        "公式块",
        "创建数学公式",
        &["math", "equation", "latex", "公式"],
        NoteBlockStyle::Equation,
    ),
    (
        "bookmark",
        "网页书签",
        "把 HTTP(S) 地址显示为书签卡片",
        &["bookmark", "web", "link", "书签", "网页"],
        NoteBlockStyle::Bookmark,
    ),
];

pub fn note_slash_query(target: &str, cursor: usize) -> Option<&str> {
    let cursor = clip_char_boundary(target, cursor);
    let line_start = target[..cursor].rfind('\n').map_or(0, |index| index + 1);
    let before_cursor = &target[line_start..cursor];
    let query = before_cursor.strip_prefix('/')?;
    (!query.chars().any(char::is_whitespace)).then_some(query)
}

pub fn note_slash_commands(query: &str) -> Vec<NoteSlashCommand> {
    let query = query.to_lowercase();
    SLASH_COMMANDS
        .iter()
        .filter(|(_, label, hint, aliases, _)| {
            query.is_empty()
                || label.to_lowercase().contains(&query)
                || hint.to_lowercase().contains(&query)
                || aliases.iter().any(|alias| alias.contains(&query))
        })
        .map(|(id, label, hint, _, style)| NoteSlashCommand {
            id,
            label,
            hint,
            style: *style,
        })
        .collect()
}

pub fn apply_note_slash_command(
    target: &mut String,
    cursor: &mut usize,
    style: NoteBlockStyle,
) -> Result<bool, NoteEditError> {
    *cursor = clip_char_boundary(target, *cursor);
    if note_slash_query(target, *cursor).is_none() {
        return Ok(false);
    }
    let line_start = target[..*cursor].rfind('\n').map_or(0, |index| index + 1);
    let line_end = target[*cursor..]
        .find('\n')
        .map_or(target.len(), |offset| *cursor + offset);
    target.replace_range(line_start..line_end, "");
    *cursor = line_start;
    apply_note_block_style(target, cursor, style)?;
    Ok(true)
}

pub fn move_note_block(
    target: &mut String,
    cursor: &mut usize,
    direction: NoteBlockMove,
) -> Option<bool> {
    *cursor = clip_char_boundary(target, *cursor);
    let ranges = note_block_ranges(target)?;
    let current = current_block(&ranges, *cursor)?;
    let destination = match direction {
        NoteBlockMove::Up => current.checked_sub(1),
        NoteBlockMove::Down => (current + 1 < ranges.len()).then_some(current + 1),
    };
    let Some(destination) = destination else {
        return Some(false);
    };

    let relative_cursor = cursor.saturating_sub(ranges[current].start);
    let mut blocks = ranges
        .iter()
        .map(|range| target[range.clone()].to_owned())
        .collect::<Vec<_>>();
    blocks.swap(current, destination);
    let new_block_start = blocks
        .iter()
        .take(destination)
        .map(|block| block.len() + 1)
        .sum::<usize>();
    *target = blocks.join("\n");
    *cursor = (new_block_start + relative_cursor).min(target.len());
    Some(true)
}

pub fn duplicate_note_block(
    target: &mut String,
    cursor: &mut usize,
) -> Result<bool, NoteEditError> {
    *cursor = clip_char_boundary(target, *cursor);
    let ranges = note_block_ranges(target).ok_or(NoteEditError::UnclosedCodeFence)?;
    let current = current_block(&ranges, *cursor).ok_or(NoteEditError::UnclosedCodeFence)?;
    let range = ranges[current].clone();
    let block = target[range.clone()].to_owned();
    ensure_mutable_block(&block)?;
    let relative_cursor = cursor.saturating_sub(range.start);
    let separator = if block.starts_with("> ") {
        "\n\n"
    } else {
        "\n"
    };
    let insertion = format!("{separator}{block}");
    target.insert_str(range.end, &insertion);
    *cursor = range.end + separator.len() + relative_cursor.min(block.len());
    Ok(true)
}

pub fn delete_note_block(target: &mut String, cursor: &mut usize) -> Result<bool, NoteEditError> {
    *cursor = clip_char_boundary(target, *cursor);
    let ranges = note_block_ranges(target).ok_or(NoteEditError::UnclosedCodeFence)?;
    let current = current_block(&ranges, *cursor).ok_or(NoteEditError::UnclosedCodeFence)?;
    let range = ranges[current].clone();
    ensure_mutable_block(&target[range.clone()])?;

    let deletion = if ranges.len() == 1 {
        range
    } else if current + 1 < ranges.len() {
        range.start..ranges[current + 1].start
    } else {
        ranges[current - 1].end..range.end
    };
    let new_cursor = deletion.start;
    target.replace_range(deletion, "");
    *cursor = new_cursor.min(target.len());
    Ok(true)
}

pub fn toggle_note_todo(target: &mut String, cursor: &mut usize) -> Result<bool, NoteEditError> {
    *cursor = clip_char_boundary(target, *cursor);
    let (line_start, line_end) = line_range(target, *cursor);
    let line = &target[line_start..line_end];
    ensure_mutable_line(target, line_start, line)?;
    let indentation_len = line.len() - line.trim_start_matches(' ').len();
    let value = &line[indentation_len..];
    let replacement = if let Some(text) = value.strip_prefix("- [ ] ") {
        format!("{}- [x] {text}", &line[..indentation_len])
    } else if let Some(text) = value
        .strip_prefix("- [x] ")
        .or_else(|| value.strip_prefix("- [X] "))
    {
        format!("{}- [ ] {text}", &line[..indentation_len])
    } else {
        return apply_note_block_style(target, cursor, NoteBlockStyle::Todo);
    };
    let relative_cursor = cursor.saturating_sub(line_start).min(replacement.len());
    target.replace_range(line_start..line_end, &replacement);
    *cursor = line_start + relative_cursor;
    Ok(true)
}

pub fn indent_note_block(
    target: &mut String,
    cursor: &mut usize,
    direction: NoteBlockIndent,
) -> Result<bool, NoteEditError> {
    *cursor = clip_char_boundary(target, *cursor);
    let (line_start, line_end) = line_range(target, *cursor);
    let line = &target[line_start..line_end];
    ensure_mutable_line(target, line_start, line)?;
    let indentation_len = line.len() - line.trim_start_matches(' ').len();
    let value = &line[indentation_len..];
    let is_list = ["- [ ] ", "- [x] ", "- [X] ", "- ", "* "]
        .iter()
        .any(|prefix| value.starts_with(prefix))
        || value.split_once(". ").is_some_and(|(number, _)| {
            !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())
        });
    if !is_list {
        return Ok(false);
    }

    let (replacement, cursor_delta): (String, isize) = match direction {
        NoteBlockIndent::In => (format!("  {line}"), 2),
        NoteBlockIndent::Out if indentation_len >= 2 => (line[2..].to_owned(), -2),
        NoteBlockIndent::Out if indentation_len == 1 => (line[1..].to_owned(), -1),
        NoteBlockIndent::Out => return Ok(false),
    };
    let new_cursor = (*cursor as isize + cursor_delta).max(line_start as isize) as usize;
    target.replace_range(line_start..line_end, &replacement);
    *cursor = new_cursor.min(target.len());
    Ok(true)
}

pub fn apply_note_block_style(
    target: &mut String,
    cursor: &mut usize,
    style: NoteBlockStyle,
) -> Result<bool, NoteEditError> {
    *cursor = clip_char_boundary(target, *cursor);
    let (line_start, line_end) = line_range(target, *cursor);
    let line = &target[line_start..line_end];
    ensure_mutable_line(target, line_start, line)?;

    let (indent, content_start) = block_content_start(line);
    let content = &line[content_start..];
    let content_cursor = cursor
        .saturating_sub(line_start + content_start)
        .min(content.len());
    let prefix = match style {
        NoteBlockStyle::Paragraph => "",
        NoteBlockStyle::Heading1 => "# ",
        NoteBlockStyle::Heading2 => "## ",
        NoteBlockStyle::Heading3 => "### ",
        NoteBlockStyle::Quote => "> ",
        NoteBlockStyle::Bullet => "- ",
        NoteBlockStyle::Ordered => "1. ",
        NoteBlockStyle::Todo => "- [ ] ",
        NoteBlockStyle::Code
        | NoteBlockStyle::Rule
        | NoteBlockStyle::Callout
        | NoteBlockStyle::Toggle
        | NoteBlockStyle::Equation
        | NoteBlockStyle::Bookmark => "",
    };
    let keep_indent = matches!(
        style,
        NoteBlockStyle::Bullet | NoteBlockStyle::Ordered | NoteBlockStyle::Todo
    );
    let indentation = if keep_indent { indent } else { "" };
    let replacement = match style {
        NoteBlockStyle::Code => format!("```\n{content}\n```"),
        NoteBlockStyle::Rule => "---".to_owned(),
        NoteBlockStyle::Callout => format!(":::callout 💡\n{content}\n:::"),
        NoteBlockStyle::Toggle => format!(":::toggle open\n{content}\n:::"),
        NoteBlockStyle::Equation => format!("$$\n{content}\n$$"),
        NoteBlockStyle::Bookmark => format!(":::bookmark\n{content}\n:::"),
        _ => format!("{indentation}{prefix}{content}"),
    };
    if replacement == line {
        return Ok(false);
    }

    let new_cursor = match style {
        NoteBlockStyle::Code => line_start + "```\n".len() + content_cursor,
        NoteBlockStyle::Rule => line_start + replacement.len(),
        NoteBlockStyle::Callout => line_start + ":::callout 💡\n".len() + content_cursor,
        NoteBlockStyle::Toggle => line_start + ":::toggle open\n".len() + content_cursor,
        NoteBlockStyle::Equation => line_start + "$$\n".len() + content_cursor,
        NoteBlockStyle::Bookmark => line_start + ":::bookmark\n".len() + content_cursor,
        _ => line_start + indentation.len() + prefix.len() + content_cursor,
    };
    target.replace_range(line_start..line_end, &replacement);
    *cursor = new_cursor.min(target.len());
    Ok(true)
}

/// Toggles an inline Markdown mark and updates the selection to the edited text.
pub fn toggle_note_inline_style(
    target: &mut String,
    selection: &mut Range<usize>,
    style: NoteInlineStyle,
) -> Result<bool, NoteEditError> {
    selection.start = clip_char_boundary(target, selection.start);
    selection.end = clip_char_boundary(target, selection.end.max(selection.start));
    if selection_overlaps_protected_block(target, selection.clone()) {
        return Err(NoteEditError::ProtectedBlock);
    }
    if selection_overlaps_code_fence(target, selection.clone()) {
        return Err(NoteEditError::CodeBlock);
    }

    let (prefix, suffix) = match style {
        NoteInlineStyle::Bold => ("**", "**"),
        NoteInlineStyle::Italic => ("*", "*"),
        NoteInlineStyle::Strikethrough => ("~~", "~~"),
        NoteInlineStyle::Code => ("`", "`"),
        NoteInlineStyle::Link => ("[", "](https://)"),
    };
    if style != NoteInlineStyle::Link
        && selection.start >= prefix.len()
        && target[..selection.start].ends_with(prefix)
        && target[selection.end..].starts_with(suffix)
    {
        let content = target[selection.clone()].to_owned();
        let replacement_range = selection.start - prefix.len()..selection.end + suffix.len();
        let start = replacement_range.start;
        target.replace_range(replacement_range, &content);
        *selection = start..start + content.len();
        return Ok(true);
    }

    let content = target[selection.clone()].to_owned();
    let start = selection.start;
    let replacement = format!("{prefix}{content}{suffix}");
    target.replace_range(selection.clone(), &replacement);
    if content.is_empty() {
        *selection = start + prefix.len()..start + prefix.len();
    } else {
        *selection = start + prefix.len()..start + prefix.len() + content.len();
    }
    Ok(true)
}

fn current_block(ranges: &[Range<usize>], cursor: usize) -> Option<usize> {
    ranges
        .iter()
        .position(|range| range.start <= cursor && cursor <= range.end)
}

fn line_range(target: &str, cursor: usize) -> (usize, usize) {
    let line_start = target[..cursor].rfind('\n').map_or(0, |index| index + 1);
    let line_end = target[cursor..]
        .find('\n')
        .map_or(target.len(), |offset| cursor + offset);
    (line_start, line_end)
}

fn ensure_mutable_line(target: &str, line_start: usize, line: &str) -> Result<(), NoteEditError> {
    ensure_mutable_block(line)?;
    let inside_code_fence = target[..line_start]
        .split('\n')
        .filter(|line| line.trim_start().starts_with("```"))
        .count()
        % 2
        == 1;
    if inside_code_fence || line.trim_start().starts_with("```") {
        return Err(NoteEditError::CodeBlock);
    }
    Ok(())
}

fn ensure_mutable_block(block: &str) -> Result<(), NoteEditError> {
    if block
        .lines()
        .any(|line| line.trim_start().starts_with(PROTECTED_PREFIX))
    {
        Err(NoteEditError::ProtectedBlock)
    } else {
        Ok(())
    }
}

fn selection_overlaps_protected_block(target: &str, selection: Range<usize>) -> bool {
    let line_start = target[..selection.start]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    let line_end = target[selection.end..]
        .find('\n')
        .map_or(target.len(), |offset| selection.end + offset);
    target[line_start..line_end]
        .lines()
        .any(|line| line.trim_start().starts_with(PROTECTED_PREFIX))
}

fn selection_overlaps_code_fence(target: &str, selection: Range<usize>) -> bool {
    let starts_inside = target[..selection.start]
        .split('\n')
        .filter(|line| line.trim_start().starts_with("```"))
        .count()
        % 2
        == 1;
    let ends_inside = target[..selection.end]
        .split('\n')
        .filter(|line| line.trim_start().starts_with("```"))
        .count()
        % 2
        == 1;
    let line_start = target[..selection.start]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    let line_end = target[selection.end..]
        .find('\n')
        .map_or(target.len(), |offset| selection.end + offset);
    starts_inside
        || ends_inside
        || target[line_start..line_end]
            .lines()
            .any(|line| line.trim_start().starts_with("```"))
}

fn block_content_start(line: &str) -> (&str, usize) {
    let indentation_len = line.len() - line.trim_start_matches(' ').len();
    let indentation = &line[..indentation_len];
    let value = &line[indentation_len..];

    let heading_len = value.bytes().take_while(|byte| *byte == b'#').count();
    if (1..=6).contains(&heading_len) && value.as_bytes().get(heading_len) == Some(&b' ') {
        return (indentation, indentation_len + heading_len + 1);
    }
    if let Some(rest) = value.strip_prefix("> ") {
        return (indentation, line.len() - rest.len());
    }
    for prefix in ["- [ ] ", "- [x] ", "- [X] ", "- ", "* "] {
        if let Some(rest) = value.strip_prefix(prefix) {
            return (indentation, line.len() - rest.len());
        }
    }
    if let Some((number, rest)) = value.split_once(". ")
        && !number.is_empty()
        && number.bytes().all(|byte| byte.is_ascii_digit())
    {
        return (indentation, line.len() - rest.len());
    }
    if value == "---" {
        return (indentation, line.len());
    }
    (indentation, indentation_len)
}

fn clip_char_boundary(target: &str, offset: usize) -> usize {
    let mut offset = offset.min(target.len());
    while offset > 0 && !target.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_commands_filter_and_apply_heading() {
        let commands = note_slash_commands("h3");
        assert_eq!(
            commands.first().map(|command| command.id),
            Some("heading-3")
        );

        let mut text = "before\n/h3".to_owned();
        let mut cursor = text.len();
        assert_eq!(
            apply_note_slash_command(&mut text, &mut cursor, NoteBlockStyle::Heading3),
            Ok(true)
        );
        assert_eq!(text, "before\n### ");
        assert_eq!(cursor, text.len());

        let commands = note_slash_commands("公式");
        assert_eq!(
            commands.first().map(|command| command.style),
            Some(NoteBlockStyle::Equation)
        );
    }

    #[test]
    fn advanced_directive_blocks_move_as_single_units() {
        let mut text = "alpha\n:::callout 💡\nimportant\n:::\n$$\nx + y\n$$\nomega".to_owned();
        let mut cursor = text.find("important").unwrap();
        assert_eq!(
            move_note_block(&mut text, &mut cursor, NoteBlockMove::Up),
            Some(true)
        );
        assert!(text.starts_with(":::callout 💡\nimportant\n:::\nalpha"));

        cursor = text.find("x + y").unwrap();
        assert_eq!(
            move_note_block(&mut text, &mut cursor, NoteBlockMove::Down),
            Some(true)
        );
        assert!(text.ends_with("omega\n$$\nx + y\n$$"));
    }

    #[test]
    fn block_actions_preserve_multiline_blocks_and_protected_data() {
        let mut text = "alpha\n> one\n> two\nomega".to_owned();
        let mut cursor = text.find("one").unwrap();
        assert_eq!(
            move_note_block(&mut text, &mut cursor, NoteBlockMove::Up),
            Some(true)
        );
        assert_eq!(text, "> one\n> two\nalpha\nomega");

        assert_eq!(duplicate_note_block(&mut text, &mut cursor), Ok(true));
        assert_eq!(text, "> one\n> two\n\n> one\n> two\nalpha\nomega");

        let mut protected = "alpha\n⟦ideall:受保护块:1:x:hash⟧".to_owned();
        let mut protected_cursor = protected.len();
        assert_eq!(
            delete_note_block(&mut protected, &mut protected_cursor),
            Err(NoteEditError::ProtectedBlock)
        );

        let mut plain = "alpha\nbeta\ngamma".to_owned();
        let mut plain_cursor = plain.find("beta").unwrap();
        assert_eq!(delete_note_block(&mut plain, &mut plain_cursor), Ok(true));
        assert_eq!(plain, "alpha\ngamma");

        let mut unclosed = "alpha\n```\ncode".to_owned();
        let mut unclosed_cursor = unclosed.len();
        assert_eq!(
            duplicate_note_block(&mut unclosed, &mut unclosed_cursor),
            Err(NoteEditError::UnclosedCodeFence)
        );
    }

    #[test]
    fn todo_and_indent_actions_are_reversible() {
        let mut text = "task".to_owned();
        let mut cursor = 2;
        assert_eq!(toggle_note_todo(&mut text, &mut cursor), Ok(true));
        assert_eq!(text, "- [ ] task");
        assert_eq!(toggle_note_todo(&mut text, &mut cursor), Ok(true));
        assert_eq!(text, "- [x] task");
        assert_eq!(
            indent_note_block(&mut text, &mut cursor, NoteBlockIndent::In),
            Ok(true)
        );
        assert_eq!(text, "  - [x] task");
        assert_eq!(
            indent_note_block(&mut text, &mut cursor, NoteBlockIndent::Out),
            Ok(true)
        );
        assert_eq!(text, "- [x] task");
    }

    #[test]
    fn inline_marks_wrap_toggle_and_support_empty_cursors() {
        let mut text = "hello".to_owned();
        let mut selection = 0..5;
        assert_eq!(
            toggle_note_inline_style(&mut text, &mut selection, NoteInlineStyle::Bold),
            Ok(true)
        );
        assert_eq!(text, "**hello**");
        assert_eq!(selection, 2..7);
        assert_eq!(
            toggle_note_inline_style(&mut text, &mut selection, NoteInlineStyle::Bold),
            Ok(true)
        );
        assert_eq!(text, "hello");

        let mut cursor = 5..5;
        assert_eq!(
            toggle_note_inline_style(&mut text, &mut cursor, NoteInlineStyle::Link),
            Ok(true)
        );
        assert_eq!(text, "hello[](https://)");
        assert_eq!(cursor, 6..6);

        let mut code = "```\nhello\n```".to_owned();
        let mut code_selection = code.find("hello").unwrap()..code.find("hello").unwrap() + 5;
        assert_eq!(
            toggle_note_inline_style(&mut code, &mut code_selection, NoteInlineStyle::Italic),
            Err(NoteEditError::CodeBlock)
        );
        let mut cross_fence_selection = 0..code.len();
        assert_eq!(
            toggle_note_inline_style(&mut code, &mut cross_fence_selection, NoteInlineStyle::Bold),
            Err(NoteEditError::CodeBlock)
        );

        let mut protected = "⟦ideall:受保护块:1:x:hash⟧".to_owned();
        let mut protected_selection = 0..protected.len();
        assert_eq!(
            toggle_note_inline_style(
                &mut protected,
                &mut protected_selection,
                NoteInlineStyle::Bold
            ),
            Err(NoteEditError::ProtectedBlock)
        );
    }
}
