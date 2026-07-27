use std::ops::Range;

use gpui::{EntityInputHandler as _, Focusable as _};
use ideall_application::{
    NoteBlockIndent, NoteBlockMove, NoteBlockStyle, NoteEditError, NoteInlineStyle,
    apply_note_block_style, apply_note_slash_command, delete_note_block, duplicate_note_block,
    indent_note_block, move_note_block, note_slash_commands, note_slash_query,
    toggle_note_inline_style, toggle_note_todo,
};

use super::*;

impl IdeallDesktop {
    pub(crate) fn note_editor_available(&self) -> bool {
        !self.show_trash
            && self.editor.selected_kind == Some(NodeKind::Note)
            && self.editor.body_editable
            && self.editor.active_engine_id.as_deref() != Some("ideall.code")
    }

    pub(crate) fn note_editor_focused(&self, window: &Window, cx: &Context<Self>) -> bool {
        self.note_editor_available()
            && self
                .editor
                .body_input
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
    }

    pub(crate) fn format_note_block(
        &mut self,
        style: NoteBlockStyle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.transform_current_note_block(
            window,
            cx,
            |text, cursor| apply_note_block_style(text, cursor, style),
            "已转换当前块",
            "当前块已经是该类型",
        );
    }

    pub(crate) fn run_note_slash_command(
        &mut self,
        style: NoteBlockStyle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.transform_current_note_block(
            window,
            cx,
            |text, cursor| apply_note_slash_command(text, cursor, style),
            "已通过 / 命令插入块",
            "斜杠命令已经失效，请重新输入 /",
        );
    }

    pub(crate) fn complete_note_slash_after_enter(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.note_editor_available() {
            return;
        }
        let original = self.editor.body_input.read(cx).value().to_string();
        let position = self.editor.body_input.read(cx).cursor_position();
        let cursor = byte_offset_for_position(&original, position);
        let Some(command_cursor) = cursor.checked_sub(1).filter(|offset| {
            original
                .as_bytes()
                .get(*offset)
                .is_some_and(|byte| *byte == b'\n')
        }) else {
            return;
        };
        let Some(query) = note_slash_query(&original, command_cursor) else {
            return;
        };
        let Some(command) = note_slash_commands(query).into_iter().next() else {
            return;
        };

        let mut updated = original.clone();
        updated.remove(command_cursor);
        let mut updated_cursor = command_cursor;
        match apply_note_slash_command(&mut updated, &mut updated_cursor, command.style) {
            Ok(true) => {
                self.apply_note_body_update(&original, &updated, updated_cursor, window, cx);
                self.status = Some(format!("已插入{}", command.label).into());
                cx.notify();
            }
            Ok(false) => {}
            Err(error) => self.report_note_edit_error(error, cx),
        }
    }

    pub(crate) fn move_current_note_block(
        &mut self,
        direction: NoteBlockMove,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ensure_note_editor_available(cx) {
            return;
        }
        let original = self.editor.body_input.read(cx).value().to_string();
        let mut cursor =
            byte_offset_for_position(&original, self.editor.body_input.read(cx).cursor_position());
        let mut updated = original.clone();
        match move_note_block(&mut updated, &mut cursor, direction) {
            Some(true) => {
                self.apply_note_body_update(&original, &updated, cursor, window, cx);
                self.status = Some("已移动当前块".into());
            }
            Some(false) => {
                self.status = Some(
                    match direction {
                        NoteBlockMove::Up => "当前块已经在最前面",
                        NoteBlockMove::Down => "当前块已经在最后面",
                    }
                    .into(),
                );
            }
            None => self.status = Some("正文含未闭合代码围栏，修复后才能移动块".into()),
        }
        cx.notify();
    }

    pub(crate) fn duplicate_current_note_block(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.transform_current_note_block(
            window,
            cx,
            duplicate_note_block,
            "已复制当前块",
            "当前块未发生变化",
        );
    }

    pub(crate) fn delete_current_note_block(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.transform_current_note_block(
            window,
            cx,
            delete_note_block,
            "已删除当前块，可使用撤销恢复",
            "当前块未发生变化",
        );
    }

    pub(crate) fn toggle_current_note_todo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.transform_current_note_block(
            window,
            cx,
            toggle_note_todo,
            "已切换待办状态",
            "当前块未发生变化",
        );
    }

    pub(crate) fn indent_current_note_block(
        &mut self,
        direction: NoteBlockIndent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.transform_current_note_block(
            window,
            cx,
            |text, cursor| indent_note_block(text, cursor, direction),
            "已调整列表层级",
            "只有列表和待办块可以调整层级",
        );
    }

    pub(crate) fn format_note_inline(
        &mut self,
        style: NoteInlineStyle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ensure_note_editor_available(cx) {
            return;
        }
        let input = self.editor.body_input.clone();
        let original = input.read(cx).value().to_string();
        let selection_utf16 = input.update(cx, |input, cx| {
            input
                .selected_text_range(false, window, cx)
                .map(|selection| selection.range)
        });
        let mut selection = selection_utf16
            .map(|range| {
                byte_offset_for_utf16(&original, range.start)
                    ..byte_offset_for_utf16(&original, range.end)
            })
            .unwrap_or_else(|| {
                let cursor = byte_offset_for_position(&original, input.read(cx).cursor_position());
                cursor..cursor
            });
        let mut updated = original.clone();
        match toggle_note_inline_style(&mut updated, &mut selection, style) {
            Ok(true) => {
                self.apply_note_body_update(&original, &updated, selection.end, window, cx);
                self.status = Some("已应用行内格式；重新选择已格式文本可取消同类标记".into());
            }
            Ok(false) => self.status = Some("所选文本未发生变化".into()),
            Err(error) => {
                self.report_note_edit_error(error, cx);
                return;
            }
        }
        cx.notify();
    }

    fn transform_current_note_block(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        transform: impl FnOnce(&mut String, &mut usize) -> Result<bool, NoteEditError>,
        success: &'static str,
        unchanged: &'static str,
    ) {
        if !self.ensure_note_editor_available(cx) {
            return;
        }
        let original = self.editor.body_input.read(cx).value().to_string();
        let mut cursor =
            byte_offset_for_position(&original, self.editor.body_input.read(cx).cursor_position());
        let mut updated = original.clone();
        match transform(&mut updated, &mut cursor) {
            Ok(true) => {
                self.apply_note_body_update(&original, &updated, cursor, window, cx);
                self.status = Some(success.into());
            }
            Ok(false) => self.status = Some(unchanged.into()),
            Err(error) => {
                self.report_note_edit_error(error, cx);
                return;
            }
        }
        cx.notify();
    }

    fn ensure_note_editor_available(&mut self, cx: &mut Context<Self>) -> bool {
        if self.note_editor_available() {
            true
        } else {
            self.status = Some("当前内容不是可编辑笔记".into());
            cx.notify();
            false
        }
    }

    fn apply_note_body_update(
        &mut self,
        original: &str,
        updated: &str,
        cursor: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((old_range, new_range)) = changed_ranges(original, updated) else {
            return;
        };
        let range_utf16 = utf16_offset_for_byte(original, old_range.start)
            ..utf16_offset_for_byte(original, old_range.end);
        let replacement = &updated[new_range];
        self.editor.body_input.update(cx, |input, cx| {
            input.replace_text_in_range(Some(range_utf16), replacement, window, cx);
            input.set_cursor_position(position_for_byte_offset(updated, cursor), window, cx);
        });
    }

    fn report_note_edit_error(&mut self, error: NoteEditError, cx: &mut Context<Self>) {
        self.status = Some(
            match error {
                NoteEditError::ProtectedBlock => "受保护的未知富文本块不能修改",
                NoteEditError::CodeBlock => "代码围栏内部保持原样，请在普通文本块上操作",
                NoteEditError::UnclosedCodeFence => "正文含未闭合代码围栏，请先修复",
            }
            .into(),
        );
        cx.notify();
    }
}

fn changed_ranges(original: &str, updated: &str) -> Option<(Range<usize>, Range<usize>)> {
    if original == updated {
        return None;
    }
    let prefix = original
        .char_indices()
        .zip(updated.char_indices())
        .take_while(|((_, left), (_, right))| left == right)
        .map(|((offset, character), _)| offset + character.len_utf8())
        .last()
        .unwrap_or(0);
    let suffix = original[prefix..]
        .chars()
        .rev()
        .zip(updated[prefix..].chars().rev())
        .take_while(|(left, right)| left == right)
        .map(|(character, _)| character.len_utf8())
        .sum::<usize>();
    Some((
        prefix..original.len() - suffix,
        prefix..updated.len() - suffix,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn change_ranges_are_minimal_and_unicode_safe() {
        assert_eq!(
            changed_ranges("你abc好", "你axc好"),
            Some(("你a".len().."你ab".len(), "你a".len().."你ax".len()))
        );
        assert_eq!(changed_ranges("same", "same"), None);
    }
}
