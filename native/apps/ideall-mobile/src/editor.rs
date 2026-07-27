//! Mobile input, cursor helpers, and compatibility aliases for shared note editing.

use unicode_segmentation::UnicodeSegmentation as _;

pub(crate) use ideall_application::{
    NoteBlockMove as BlockMove, NoteBlockStyle as BlockStyle, NoteEditError as BlockFormatError,
    apply_note_block_style as apply_block_style, apply_note_slash_command as apply_slash_command,
    move_note_block as move_block, note_slash_query as slash_query,
};

pub(crate) fn slash_commands(query: &str) -> Vec<(&'static str, &'static str, BlockStyle)> {
    ideall_application::note_slash_commands(query)
        .into_iter()
        .map(|command| (command.id, command.label, command.style))
        .collect()
}

pub(crate) fn control_sequence(key: &str) -> Option<&'static str> {
    match key {
        "backspace" => Some("\u{8}"),
        "delete" => Some("\u{1b}[3~"),
        "left" => Some("\u{1b}[D"),
        "right" => Some("\u{1b}[C"),
        "home" => Some("\u{1b}[H"),
        "end" => Some("\u{1b}[F"),
        "enter" => Some("\n"),
        _ => None,
    }
}

pub(crate) fn enqueue_control_key(
    pending: &mut Vec<String>,
    observed_pending_chunks: &mut usize,
    key: &str,
) -> bool {
    let Some(sequence) = control_sequence(key) else {
        return false;
    };
    let delivered_by_platform = pending.len() > *observed_pending_chunks
        && pending.last().is_some_and(|chunk| chunk == sequence);
    if !delivered_by_platform {
        pending.push(sequence.to_owned());
    }
    *observed_pending_chunks = pending.len();
    !delivered_by_platform
}

#[cfg(test)]
pub(crate) fn apply_input(target: &mut String, chunk: &str, multiline: bool) -> bool {
    let mut cursor = target.len();
    apply_input_at_cursor(target, &mut cursor, chunk, multiline)
}

pub(crate) fn apply_input_at_cursor(
    target: &mut String,
    cursor: &mut usize,
    chunk: &str,
    multiline: bool,
) -> bool {
    if chunk.is_empty() {
        return false;
    }
    *cursor = (*cursor).min(target.len());
    while *cursor > 0 && !target.is_char_boundary(*cursor) {
        *cursor -= 1;
    }
    match chunk {
        "\u{1b}[D" => {
            if let Some((start, _)) = target[..*cursor].grapheme_indices(true).next_back() {
                *cursor = start;
            }
            return false;
        }
        "\u{1b}[C" => {
            *cursor = target[*cursor..]
                .grapheme_indices(true)
                .nth(1)
                .map(|(offset, _)| *cursor + offset)
                .unwrap_or(target.len());
            return false;
        }
        "\u{1b}[H" => {
            *cursor = 0;
            return false;
        }
        "\u{1b}[F" => {
            *cursor = target.len();
            return false;
        }
        "\u{7f}" | "\u{1b}[3~" => {
            if let Some((next, _)) = target[*cursor..].grapheme_indices(true).nth(1) {
                target.replace_range(*cursor..*cursor + next, "");
                return true;
            }
            if *cursor < target.len() {
                target.truncate(*cursor);
                return true;
            }
            return false;
        }
        _ => {}
    }
    if chunk.chars().all(|character| character == '\u{8}') {
        let mut changed = false;
        for _ in chunk.chars() {
            let Some((start, _)) = target[..*cursor].grapheme_indices(true).next_back() else {
                break;
            };
            target.replace_range(start..*cursor, "");
            *cursor = start;
            changed = true;
        }
        return changed;
    }

    let insertion = if multiline {
        chunk.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        chunk
            .chars()
            .filter(|character| !matches!(character, '\r' | '\n'))
            .collect()
    };
    if insertion.is_empty() {
        false
    } else {
        target.insert_str(*cursor, &insertion);
        *cursor += insertion.len();
        true
    }
}

pub(crate) fn text_line_ranges(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = 0;
    for segment in text.split_inclusive('\n') {
        let end = start + segment.trim_end_matches('\n').len();
        ranges.push(start..end);
        start += segment.len();
    }
    if text.is_empty() || text.ends_with('\n') {
        ranges.push(text.len()..text.len());
    }
    ranges
}

pub(crate) fn cursor_line_index(text: &str, cursor: usize) -> usize {
    let cursor = cursor.min(text.len());
    let ranges = text_line_ranges(text);
    ranges
        .iter()
        .position(|range| cursor <= range.end)
        .unwrap_or_else(|| ranges.len().saturating_sub(1))
}

pub(crate) fn line_with_cursor(line: &str, offset: usize) -> String {
    let mut offset = offset.min(line.len());
    while offset > 0 && !line.is_char_boundary(offset) {
        offset -= 1;
    }
    let mut display = String::with_capacity(line.len() + "▏".len());
    display.push_str(&line[..offset]);
    display.push('▏');
    display.push_str(&line[offset..]);
    display
}

pub(crate) fn native_edit_committed(
    value_changed: bool,
    was_composing: bool,
    is_composing: bool,
) -> bool {
    !is_composing && (value_changed || was_composing)
}
