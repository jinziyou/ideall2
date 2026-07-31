use std::{collections::HashSet, ops::Range};

use gpui::{EntityInputHandler as _, Focusable as _};
use gpui_component::input::{Redo, Undo};
use ideall_application::{
    NoteBlockIndent, NoteBlockMove, NoteBlockStyle, NoteEditError, NoteInlineStyle,
    note_slash_commands, note_slash_query, toggle_note_inline_style,
};

use super::*;

impl IdeallDesktop {
    pub(crate) fn note_editor_available(&self) -> bool {
        !self.show_trash
            && self.editor.selected_kind == Some(NodeKind::Note)
            && self.editor.body_editable
            && !self.editor.note_preview
            && self.editor.active_engine_id.as_deref() != Some("ideall.text")
    }

    pub(crate) fn note_editor_focused(&self, window: &Window, cx: &Context<Self>) -> bool {
        self.note_editor_available()
            && self.editor.active_note_input().map_or_else(
                || {
                    self.editor
                        .active_input()
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(window)
                },
                |input| input.read(cx).focus_handle(cx).is_focused(window),
            )
    }

    pub(super) fn handle_note_rich_input(
        &mut self,
        id: u64,
        event: &RichInputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editor.suppress_events {
            return;
        }
        let Some(index) = self.editor.note_block_index(id) else {
            return;
        };
        if self.editor.note_blocks[index].input.read(cx).mode() == RichInputMode::Plain {
            self.editor.link_editor = None;
        }
        if !matches!(event, RichInputEvent::Change)
            && !self.editor.note_blocks[index]
                .input
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        {
            return;
        }
        self.editor.active_note_block = index;
        match event {
            RichInputEvent::Change => {
                if !self.convert_note_markdown_shortcut(id, window, cx) {
                    self.sync_note_blocks_to_source(window, cx);
                }
            }
            RichInputEvent::SelectionChange => {
                cx.notify();
            }
            RichInputEvent::OpenLink(url) => {
                if let Some(id) = note_node_id_from_url(url) {
                    self.open_node(id.to_owned(), window, cx);
                } else if url::Url::parse(url)
                    .ok()
                    .is_some_and(|url| matches!(url.scheme(), "http" | "https"))
                {
                    cx.open_url(url);
                }
            }
            RichInputEvent::PressEnter { secondary } => {
                self.complete_rich_note_block_after_enter(id, *secondary, window, cx);
            }
            RichInputEvent::BackspaceAtStart => {
                self.merge_rich_note_block_with_previous(id, window, cx);
            }
            RichInputEvent::Navigate(direction) => {
                self.navigate_from_rich_note_block(id, *direction, window, cx);
            }
        }
    }

    pub(crate) fn format_note_block(
        &mut self,
        style: NoteBlockStyle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ensure_note_editor_available(cx) {
            return;
        }
        self.convert_active_note_block(style, false, window, cx);
    }

    pub(crate) fn run_note_slash_command(
        &mut self,
        style: NoteBlockStyle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ensure_note_editor_available(cx) {
            return;
        }
        let Some(input) = self.editor.active_note_input().cloned() else {
            return;
        };
        let (value, cursor) = {
            let input = input.read(cx);
            (input.text().to_owned(), input.cursor_offset())
        };
        if note_slash_query(&value, cursor).is_none() {
            self.status = Some("斜杠命令已经失效，请重新输入 /".into());
            cx.notify();
            return;
        }
        self.editor.suppress_events = true;
        input.update(cx, |input, cx| input.set_markdown("", cx));
        self.editor.suppress_events = false;
        self.convert_active_note_block(style, true, window, cx);
    }

    pub(crate) fn note_mention_candidates(&self, query: &str) -> Vec<NodeSummary> {
        filter_note_mention_candidates(
            &self.mentionable_notes,
            self.editor.selected_id.as_deref(),
            query,
            8,
        )
    }

    pub(crate) fn run_note_mention(
        &mut self,
        target: NodeSummary,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ensure_note_editor_available(cx) {
            return;
        }
        let Some(input) = self.editor.active_rich_input().cloned() else {
            self.status = Some("当前块不支持页面提及".into());
            cx.notify();
            return;
        };
        let Some((range, _)) = input.read(cx).mention_query() else {
            self.status = Some("页面提及已经失效，请重新输入 @".into());
            cx.notify();
            return;
        };
        let Some(url) = note_node_url(&target.id) else {
            self.status = Some("页面标识无效，无法创建提及".into());
            cx.notify();
            return;
        };
        let label = format!("@{}", target.title);
        let inserted = input.update(cx, |input, cx| input.insert_link(range, &label, &url, cx));
        if inserted {
            self.status = Some(format!("已提及页面「{}」", target.title).into());
            input.update(cx, |input, _| input.focus(window));
        }
        cx.notify();
    }

    pub(crate) fn complete_note_slash_after_enter(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        // Notes now own one native input per block. Enter handling lives on
        // those inputs; this remains for the shared bookmark/file body input.
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
        let ids = self.editor.selected_note_block_ids();
        let indices = self.editor.selected_note_block_indices();
        let Some(first) = indices.first().copied() else {
            return;
        };
        let Some(last) = indices.last().copied() else {
            return;
        };
        let destination = match direction {
            NoteBlockMove::Up => first
                .checked_sub(1)
                .map(|index| self.editor.note_blocks[index].id),
            NoteBlockMove::Down => {
                if last + 1 >= self.editor.note_blocks.len() {
                    None
                } else {
                    self.editor
                        .note_blocks
                        .get(last + 2)
                        .map(|block| block.id)
                        .or(Some(0))
                }
            }
        };
        if (direction == NoteBlockMove::Up && destination.is_none())
            || (direction == NoteBlockMove::Down && last + 1 >= self.editor.note_blocks.len())
        {
            self.status = Some(
                match direction {
                    NoteBlockMove::Up => "所选内容块已经在最前面",
                    NoteBlockMove::Down => "所选内容块已经在最后面",
                }
                .into(),
            );
            cx.notify();
            return;
        }
        self.move_note_blocks(ids, destination.filter(|id| *id != 0), window, cx);
    }

    pub(crate) fn select_note_block_with_modifiers(
        &mut self,
        id: u64,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.editor.note_block_index(id) else {
            return;
        };
        self.editor.link_editor = None;
        self.editor.active_note_block = index;
        if event.modifiers.shift {
            let anchor = self.editor.note_selection_anchor.unwrap_or(index);
            self.editor.selected_note_blocks.clear();
            let start = anchor.min(index);
            let end = anchor.max(index);
            self.editor.selected_note_blocks.extend(
                self.editor.note_blocks[start..=end]
                    .iter()
                    .map(|block| block.id),
            );
            self.status = Some(
                format!(
                    "已选择 {} 个内容块，可拖动手柄或批量操作",
                    self.editor.selected_note_blocks.len()
                )
                .into(),
            );
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if event.modifiers.secondary() {
            if !self.editor.selected_note_blocks.insert(id) {
                self.editor.selected_note_blocks.remove(&id);
            }
            self.editor.note_selection_anchor = Some(index);
            self.status =
                Some(format!("已选择 {} 个内容块", self.editor.selected_note_blocks.len()).into());
            cx.stop_propagation();
            cx.notify();
            return;
        }
        self.editor.selected_note_blocks.clear();
        self.editor.note_selection_anchor = Some(index);
        self.editor.focus_active_editor(window, cx);
        cx.notify();
    }

    pub(crate) fn move_note_blocks(
        &mut self,
        ids: Vec<u64>,
        before_id: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ensure_note_editor_available(cx) || ids.is_empty() {
            return;
        }
        let moving_ids = ids.into_iter().collect::<HashSet<_>>();
        if before_id.is_some_and(|id| moving_ids.contains(&id)) {
            return;
        }
        let mut moving = Vec::new();
        let mut remaining = Vec::with_capacity(self.editor.note_blocks.len());
        for block in self.editor.note_blocks.drain(..) {
            if moving_ids.contains(&block.id) {
                moving.push(block);
            } else {
                remaining.push(block);
            }
        }
        if moving.is_empty() {
            self.editor.note_blocks = remaining;
            return;
        }
        let moved_ids = moving.iter().map(|block| block.id).collect::<Vec<_>>();
        let insert_at = before_id
            .and_then(|id| remaining.iter().position(|block| block.id == id))
            .unwrap_or(remaining.len());
        for (offset, block) in moving.into_iter().enumerate() {
            remaining.insert(insert_at + offset, block);
        }
        self.editor.note_blocks = remaining;
        self.editor.active_note_block = self
            .editor
            .note_block_index(moved_ids[0])
            .unwrap_or_default();
        if moved_ids.len() > 1 {
            self.editor.selected_note_blocks = moved_ids.iter().copied().collect();
        } else {
            self.editor.selected_note_blocks.clear();
        }
        self.editor.note_selection_anchor = Some(self.editor.active_note_block);
        self.sync_note_blocks_to_source(window, cx);
        self.status = Some(
            if moved_ids.len() == 1 {
                "已移动内容块".to_owned()
            } else {
                format!("已移动 {} 个内容块", moved_ids.len())
            }
            .into(),
        );
        cx.notify();
    }

    pub(crate) fn insert_note_block_after(
        &mut self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ensure_note_editor_available(cx) {
            return;
        }
        let index = self.editor.note_block_index(id).unwrap_or_else(|| {
            self.editor
                .active_note_block
                .min(self.editor.note_blocks.len().saturating_sub(1))
        });
        self.editor.suppress_events = true;
        self.editor
            .insert_note_block(index + 1, NoteBlockSpec::paragraph(""), window, cx);
        self.editor.suppress_events = false;
        self.editor.active_note_block = index + 1;
        self.sync_note_blocks_to_source(window, cx);
        self.editor.focus_active_editor(window, cx);
        self.status = Some("已插入文本块".into());
        cx.notify();
    }

    pub(crate) fn toggle_note_block_checked(
        &mut self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.editor.note_block_index(id) else {
            return;
        };
        self.editor.active_note_block = index;
        self.toggle_current_note_todo(window, cx);
    }

    pub(crate) fn toggle_note_disclosure(
        &mut self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.editor.note_block_index(id) else {
            return;
        };
        let NoteBlockKind::Toggle { expanded } = &mut self.editor.note_blocks[index].kind else {
            return;
        };
        *expanded = !*expanded;
        let expanded = *expanded;
        self.editor.active_note_block = index;
        self.sync_note_blocks_to_source(window, cx);
        self.status = Some(
            if expanded {
                "已展开折叠块"
            } else {
                "已收起折叠块"
            }
            .into(),
        );
        cx.notify();
    }

    pub(crate) fn cycle_note_callout_icon(
        &mut self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.editor.note_block_index(id) else {
            return;
        };
        let NoteBlockKind::Callout { icon } = &mut self.editor.note_blocks[index].kind else {
            return;
        };
        *icon = match icon.as_str() {
            "💡" => "📌",
            "📌" => "✅",
            "✅" => "⚠️",
            "⚠️" => "ℹ️",
            _ => "💡",
        }
        .into();
        self.editor.active_note_block = index;
        self.sync_note_blocks_to_source(window, cx);
        self.status = Some("已切换提示框图标".into());
        cx.notify();
    }

    pub(crate) fn open_note_bookmark(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(index) = self.editor.note_block_index(id) else {
            return;
        };
        if self.editor.note_blocks[index].kind != NoteBlockKind::Bookmark {
            return;
        }
        let value = self.editor.note_blocks[index]
            .input
            .read(cx)
            .text()
            .trim()
            .to_owned();
        let safe = url::Url::parse(&value)
            .ok()
            .filter(|url| matches!(url.scheme(), "http" | "https"));
        if safe.is_some() {
            cx.open_url(&value);
            self.status = Some("已在系统浏览器中打开书签".into());
        } else {
            self.status = Some("书签只接受完整的 HTTP(S) 地址".into());
        }
        cx.notify();
    }

    pub(crate) fn duplicate_current_note_block(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ensure_note_editor_available(cx) {
            return;
        }
        let indices = self.editor.selected_note_block_indices();
        if indices
            .iter()
            .any(|index| self.editor.note_blocks[*index].kind == NoteBlockKind::Protected)
        {
            self.report_note_edit_error(NoteEditError::ProtectedBlock, cx);
            return;
        }
        let specs = indices
            .iter()
            .map(|index| self.editor.note_blocks[*index].to_spec(cx))
            .collect::<Vec<_>>();
        let insert_at = indices.last().copied().unwrap_or_default() + 1;
        self.editor.suppress_events = true;
        let mut inserted = Vec::with_capacity(specs.len());
        for (offset, spec) in specs.into_iter().enumerate() {
            inserted.push(
                self.editor
                    .insert_note_block(insert_at + offset, spec, window, cx),
            );
        }
        self.editor.suppress_events = false;
        self.editor.active_note_block = insert_at;
        self.editor.selected_note_blocks = inserted.iter().copied().collect();
        self.editor.note_selection_anchor = Some(insert_at);
        self.sync_note_blocks_to_source(window, cx);
        self.status = Some(
            if inserted.len() == 1 {
                "已重复内容块".to_owned()
            } else {
                format!("已重复 {} 个内容块", inserted.len())
            }
            .into(),
        );
        cx.notify();
    }

    pub(crate) fn copy_selected_note_blocks(&mut self, cx: &mut Context<Self>) {
        let indices = self.editor.selected_note_block_indices();
        if indices.is_empty() {
            return;
        }
        let markdown = indices
            .iter()
            .map(|index| self.editor.note_blocks[*index].markdown(cx))
            .collect::<Vec<_>>()
            .join("\n");
        cx.write_to_clipboard(ClipboardItem::new_string(markdown));
        self.status = Some(
            if indices.len() == 1 {
                "已复制内容块到剪贴板".to_owned()
            } else {
                format!("已复制 {} 个内容块到剪贴板", indices.len())
            }
            .into(),
        );
        cx.notify();
    }

    pub(crate) fn delete_current_note_block(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ensure_note_editor_available(cx) {
            return;
        }
        let indices = self.editor.selected_note_block_indices();
        if indices
            .iter()
            .any(|index| self.editor.note_blocks[*index].kind == NoteBlockKind::Protected)
        {
            self.report_note_edit_error(NoteEditError::ProtectedBlock, cx);
            return;
        }
        let deleted = indices.len();
        let next_index = indices.first().copied().unwrap_or_default();
        self.editor.suppress_events = true;
        for index in indices.into_iter().rev() {
            self.editor.note_blocks.remove(index);
        }
        if self.editor.note_blocks.is_empty() {
            self.editor
                .insert_note_block(0, NoteBlockSpec::paragraph(""), window, cx);
        }
        self.editor.active_note_block = next_index.min(self.editor.note_blocks.len() - 1);
        self.editor.selected_note_blocks.clear();
        self.editor.note_selection_anchor = Some(self.editor.active_note_block);
        self.editor.suppress_events = false;
        self.sync_note_blocks_to_source(window, cx);
        self.editor.focus_active_editor(window, cx);
        self.status = Some(
            if deleted == 1 {
                "已删除内容块".to_owned()
            } else {
                format!("已删除 {} 个内容块", deleted)
            }
            .into(),
        );
        cx.notify();
    }

    pub(crate) fn toggle_current_note_todo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ensure_note_editor_available(cx) {
            return;
        }
        let index = self
            .editor
            .active_note_block
            .min(self.editor.note_blocks.len().saturating_sub(1));
        let block = &mut self.editor.note_blocks[index];
        if block.kind == NoteBlockKind::Protected {
            self.report_note_edit_error(NoteEditError::ProtectedBlock, cx);
            return;
        }
        if block.kind == NoteBlockKind::Todo {
            block.checked = !block.checked;
        } else {
            block.kind = NoteBlockKind::Todo;
            block.checked = false;
        }
        self.sync_note_blocks_to_source(window, cx);
        self.status = Some("已切换待办状态".into());
        cx.notify();
    }

    pub(crate) fn indent_current_note_block(
        &mut self,
        direction: NoteBlockIndent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ensure_note_editor_available(cx) {
            return;
        }
        let index = self
            .editor
            .active_note_block
            .min(self.editor.note_blocks.len().saturating_sub(1));
        let block = &mut self.editor.note_blocks[index];
        if !matches!(
            block.kind,
            NoteBlockKind::Bullet | NoteBlockKind::Ordered | NoteBlockKind::Todo
        ) {
            self.status = Some("只有列表和待办块可以调整层级".into());
            cx.notify();
            return;
        }
        block.indent = match direction {
            NoteBlockIndent::In => block.indent.saturating_add(1).min(8),
            NoteBlockIndent::Out => block.indent.saturating_sub(1),
        };
        self.sync_note_blocks_to_source(window, cx);
        self.status = Some("已调整列表层级".into());
        cx.notify();
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
        let index = self
            .editor
            .active_note_block
            .min(self.editor.note_blocks.len().saturating_sub(1));
        if matches!(
            self.editor.note_blocks[index].kind,
            NoteBlockKind::Protected
                | NoteBlockKind::Rule
                | NoteBlockKind::Equation
                | NoteBlockKind::Bookmark
        ) {
            self.report_note_edit_error(NoteEditError::ProtectedBlock, cx);
            return;
        }
        if matches!(
            self.editor.note_blocks[index].kind,
            NoteBlockKind::Code { .. }
        ) {
            self.report_note_edit_error(NoteEditError::CodeBlock, cx);
            return;
        }
        let input = self.editor.note_blocks[index].input.clone();
        if input.read(cx).mode() == RichInputMode::Rich {
            if style == NoteInlineStyle::Link {
                let Some((range, existing_url)) = input.read(cx).link_target() else {
                    self.status = Some("请先选择要添加链接的文字".into());
                    input.update(cx, |input, _| input.focus(window));
                    cx.notify();
                    return;
                };
                self.editor.link_editor = Some(NoteLinkEditor {
                    block_id: self.editor.note_blocks[index].id,
                    range,
                    has_existing_link: existing_url.is_some(),
                });
                self.editor.link_input.update(cx, |link_input, cx| {
                    link_input.set_value(
                        existing_url.unwrap_or_else(|| "https://".into()),
                        window,
                        cx,
                    );
                    link_input.focus(window, cx);
                });
                self.status = Some("输入 HTTP(S) 地址后按 Enter 应用链接".into());
                cx.notify();
                return;
            }
            let changed = input.update(cx, |input, cx| input.toggle_style(style, cx));
            input.update(cx, |input, _| input.focus(window));
            self.status = Some(
                if changed {
                    "已更新原生行内格式；折叠光标会将样式延续到后续输入"
                } else if style == NoteInlineStyle::Link {
                    "现有链接可以取消；新建链接需要先输入并确认 HTTP(S) 地址"
                } else {
                    "所选文本未发生变化"
                }
                .into(),
            );
            cx.notify();
            return;
        }
        if style == NoteInlineStyle::Link {
            self.status = Some("链接编辑目前支持正文、标题、列表和待办块".into());
            cx.notify();
            return;
        }
        let original = input.read(cx).text().to_owned();
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
            .unwrap_or_else(|| input.read(cx).cursor_offset()..input.read(cx).cursor_offset());
        let mut updated = original.clone();
        match toggle_note_inline_style(&mut updated, &mut selection, style) {
            Ok(true) => {
                self.apply_note_input_update(
                    &input,
                    &original,
                    &updated,
                    selection.end,
                    window,
                    cx,
                );
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

    pub(crate) fn format_note_color(
        &mut self,
        foreground: bool,
        color: Option<NoteColor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ensure_note_editor_available(cx) {
            return;
        }
        let index = self
            .editor
            .active_note_block
            .min(self.editor.note_blocks.len().saturating_sub(1));
        if matches!(
            self.editor.note_blocks[index].kind,
            NoteBlockKind::Protected
                | NoteBlockKind::Rule
                | NoteBlockKind::Equation
                | NoteBlockKind::Bookmark
        ) {
            self.report_note_edit_error(NoteEditError::ProtectedBlock, cx);
            return;
        }
        if matches!(
            self.editor.note_blocks[index].kind,
            NoteBlockKind::Code { .. }
        ) {
            self.report_note_edit_error(NoteEditError::CodeBlock, cx);
            return;
        }
        let input = self.editor.note_blocks[index].input.clone();
        if input.read(cx).mode() != RichInputMode::Rich {
            self.status = Some("文字颜色目前支持正文、标题、列表、待办、引用与提示块".into());
            cx.notify();
            return;
        }
        let changed = input.update(cx, |input, cx| input.set_color(foreground, color, cx));
        input.update(cx, |input, _| input.focus(window));
        self.status = Some(
            if changed {
                if color.is_some() {
                    if foreground {
                        "已应用文字颜色；折叠光标会延续到后续输入"
                    } else {
                        "已应用背景高亮；折叠光标会延续到后续输入"
                    }
                } else if foreground {
                    "已恢复默认文字颜色"
                } else {
                    "已清除背景高亮"
                }
            } else {
                "所选文本的颜色未发生变化"
            }
            .into(),
        );
        cx.notify();
    }

    pub(crate) fn commit_note_link(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.link_editor.clone() else {
            return;
        };
        let Some(index) = self.editor.note_block_index(editor.block_id) else {
            self.editor.link_editor = None;
            cx.notify();
            return;
        };
        let input = self.editor.note_blocks[index].input.clone();
        let url = self.editor.link_input.read(cx).value().trim().to_owned();
        let changed = input.update(cx, |input, cx| {
            input.apply_link(editor.range.clone(), &url, cx)
        });
        if !changed {
            self.status = Some("链接地址无效，仅支持完整的 HTTP(S) URL".into());
            self.editor
                .link_input
                .update(cx, |input, cx| input.focus(window, cx));
            cx.notify();
            return;
        }
        self.status = Some("已保存安全链接".into());
        let desktop = cx.entity();
        window.on_next_frame(move |window, cx| {
            desktop.update(cx, |this, cx| {
                this.editor.link_editor = None;
                input.update(cx, |input, _| input.focus(window));
                cx.notify();
            });
        });
        cx.notify();
    }

    pub(crate) fn remove_note_link(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.link_editor.clone() else {
            return;
        };
        let Some(index) = self.editor.note_block_index(editor.block_id) else {
            self.editor.link_editor = None;
            cx.notify();
            return;
        };
        let input = self.editor.note_blocks[index].input.clone();
        let changed = input.update(cx, |input, cx| input.remove_link(editor.range.clone(), cx));
        self.editor.link_editor = None;
        input.update(cx, |input, _| input.focus(window));
        self.status = Some(
            if changed {
                "已移除链接，文字内容保持不变"
            } else {
                "所选文字没有链接"
            }
            .into(),
        );
        cx.notify();
    }

    pub(crate) fn cancel_note_link(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let block_id = self.editor.link_editor.take().map(|editor| editor.block_id);
        if let Some(input) = block_id
            .and_then(|id| self.editor.note_block_index(id))
            .and_then(|index| {
                self.editor
                    .note_blocks
                    .get(index)
                    .map(|block| block.input.clone())
            })
        {
            input.update(cx, |input, _| input.focus(window));
        }
        self.status = None;
        cx.notify();
    }

    pub(crate) fn undo_current_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.editor.body_editable || self.show_trash {
            return;
        }
        if self.editor.selected_kind == Some(NodeKind::Note) && self.editor.has_note_blocks() {
            if self.editor.undo_note_document(window, cx) {
                self.mark_editor_dirty(cx);
                self.editor.focus_active_editor(window, cx);
                self.status = Some("已撤销笔记编辑".into());
            } else {
                self.status = Some("没有可撤销的笔记编辑".into());
            }
            cx.notify();
            return;
        }
        self.editor.focus_active_editor(window, cx);
        window.dispatch_action(Box::new(Undo), cx);
    }

    pub(crate) fn redo_current_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.editor.body_editable || self.show_trash {
            return;
        }
        if self.editor.selected_kind == Some(NodeKind::Note) && self.editor.has_note_blocks() {
            if self.editor.redo_note_document(window, cx) {
                self.mark_editor_dirty(cx);
                self.editor.focus_active_editor(window, cx);
                self.status = Some("已重做笔记编辑".into());
            } else {
                self.status = Some("没有可重做的笔记编辑".into());
            }
            cx.notify();
            return;
        }
        self.editor.focus_active_editor(window, cx);
        window.dispatch_action(Box::new(Redo), cx);
    }

    fn focus_note_block_on_next_frame(&self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let desktop = cx.entity();
        window.on_next_frame(move |window, cx| {
            desktop.update(cx, |this, cx| {
                let Some(index) = this.editor.note_block_index(id) else {
                    return;
                };
                this.editor.active_note_block = index;
                this.editor.focus_active_editor(window, cx);
                cx.notify();
            });
        });
    }

    fn complete_rich_note_block_after_enter(
        &mut self,
        id: u64,
        _secondary: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.editor.note_block_index(id) else {
            return;
        };
        let input = self.editor.note_blocks[index].input.clone();
        // Code, equation and protected blocks keep Enter as a plain newline.
        if matches!(
            self.editor.note_blocks[index].kind,
            NoteBlockKind::Code { .. } | NoteBlockKind::Equation | NoteBlockKind::Protected
        ) {
            input.update(cx, |input, cx| {
                input.replace_text_in_range(None, "\n", window, cx)
            });
            return;
        }
        let plain = input.read(cx).text().to_owned();
        let cursor = input.read(cx).cursor_offset();

        if let Some((_, query)) = input.read(cx).mention_query()
            && let Some(target) = self.note_mention_candidates(&query).into_iter().next()
        {
            self.run_note_mention(target, window, cx);
            return;
        }

        if let Some(query) = note_slash_query(&plain, cursor)
            && let Some(command) = note_slash_commands(query).into_iter().next()
        {
            self.editor.suppress_events = true;
            input.update(cx, |input, cx| input.set_markdown("", cx));
            self.editor.suppress_events = false;
            self.editor.active_note_block = index;
            self.convert_active_note_block(command.style, true, window, cx);
            self.status = Some(format!("已插入{}", command.label).into());
            cx.notify();
            return;
        }

        let current_kind = self.editor.note_blocks[index].kind.clone();
        if matches!(
            current_kind,
            NoteBlockKind::Bullet | NoteBlockKind::Ordered | NoteBlockKind::Todo
        ) && plain.is_empty()
        {
            self.editor.note_blocks[index].kind = NoteBlockKind::Paragraph;
            self.editor.note_blocks[index].indent = 0;
            self.editor.note_blocks[index].checked = false;
            self.editor.refresh_note_block_input(index, cx);
            self.sync_note_blocks_to_source(window, cx);
            self.editor.focus_active_editor(window, cx);
            cx.notify();
            return;
        }

        let (left, right) = input.read(cx).split_markdown_at_cursor();
        let continuation = match current_kind {
            NoteBlockKind::Bullet => NoteBlockSpec {
                kind: NoteBlockKind::Bullet,
                text: right,
                indent: self.editor.note_blocks[index].indent,
                checked: false,
            },
            NoteBlockKind::Ordered => NoteBlockSpec {
                kind: NoteBlockKind::Ordered,
                text: right,
                indent: self.editor.note_blocks[index].indent,
                checked: false,
            },
            NoteBlockKind::Todo => NoteBlockSpec {
                kind: NoteBlockKind::Todo,
                text: right,
                indent: self.editor.note_blocks[index].indent,
                checked: false,
            },
            _ => NoteBlockSpec::paragraph(right),
        };
        self.editor.suppress_events = true;
        input.update(cx, |input, cx| input.set_markdown(&left, cx));
        let continuation_id = self
            .editor
            .insert_note_block(index + 1, continuation, window, cx);
        self.editor.suppress_events = false;
        self.editor.active_note_block = index + 1;
        self.sync_note_blocks_to_source(window, cx);
        self.editor.focus_active_editor(window, cx);
        self.focus_note_block_on_next_frame(continuation_id, window, cx);
        cx.notify();
    }

    fn navigate_from_rich_note_block(
        &mut self,
        id: u64,
        direction: RichNavigation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.editor.note_block_index(id) else {
            return;
        };
        let target = match direction {
            RichNavigation::Previous => index.checked_sub(1),
            RichNavigation::Next => {
                (index + 1 < self.editor.note_blocks.len()).then_some(index + 1)
            }
        };
        let Some(target) = target else {
            return;
        };
        self.editor.active_note_block = target;
        let block = &self.editor.note_blocks[target];
        let offset = match direction {
            RichNavigation::Previous => block.input.read(cx).text().len(),
            RichNavigation::Next => 0,
        };
        block.input.update(cx, |input, cx| {
            input.set_cursor_offset(offset, cx);
            input.focus(window);
        });
        cx.notify();
    }

    fn merge_rich_note_block_with_previous(
        &mut self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.editor.note_block_index(id) else {
            return;
        };
        let Some(previous_index) = index.checked_sub(1) else {
            return;
        };
        if !self.editor.note_blocks[previous_index]
            .kind
            .supports_inline_rich_text()
        {
            self.navigate_from_rich_note_block(id, RichNavigation::Previous, window, cx);
            return;
        }
        let previous = self.editor.note_blocks[previous_index].input.clone();
        let current_markdown = self.editor.note_blocks[index].input.read(cx).markdown();

        self.editor.suppress_events = true;
        let join_offset =
            previous.update(cx, |input, cx| input.append_markdown(current_markdown, cx));
        self.editor.note_blocks.remove(index);
        self.editor.suppress_events = false;
        self.editor.active_note_block = previous_index;
        self.editor.selected_note_blocks.clear();
        self.editor.note_selection_anchor = Some(previous_index);
        self.sync_note_blocks_to_source(window, cx);
        previous.update(cx, |input, cx| {
            input.set_cursor_offset(join_offset, cx);
            input.focus(window);
        });
        self.status = Some("已与上一个文本块合并".into());
        cx.notify();
    }

    fn convert_note_markdown_shortcut(
        &mut self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(index) = self.editor.note_block_index(id) else {
            return false;
        };
        if self.editor.note_blocks[index].kind != NoteBlockKind::Paragraph {
            return false;
        }
        let input = self.editor.note_blocks[index].input.clone();
        let value = input.read(cx).markdown();
        let style = match value.as_str() {
            "# " => NoteBlockStyle::Heading1,
            "## " => NoteBlockStyle::Heading2,
            "### " => NoteBlockStyle::Heading3,
            "- " | "* " => NoteBlockStyle::Bullet,
            "1. " => NoteBlockStyle::Ordered,
            "[] " | "[ ] " => NoteBlockStyle::Todo,
            "> " => NoteBlockStyle::Quote,
            "```" => NoteBlockStyle::Code,
            "---" => NoteBlockStyle::Rule,
            "$$" => NoteBlockStyle::Equation,
            _ => return false,
        };
        self.editor.suppress_events = true;
        input.update(cx, |input, cx| input.set_markdown("", cx));
        self.editor.suppress_events = false;
        self.editor.active_note_block = index;
        self.convert_active_note_block(style, false, window, cx);
        true
    }

    fn convert_active_note_block(
        &mut self,
        style: NoteBlockStyle,
        from_slash: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editor.note_blocks.is_empty() {
            return;
        }
        let index = self
            .editor
            .active_note_block
            .min(self.editor.note_blocks.len() - 1);
        if self.editor.note_blocks[index].kind == NoteBlockKind::Protected {
            self.report_note_edit_error(NoteEditError::ProtectedBlock, cx);
            return;
        }
        let kind = note_block_kind_for_style(style);
        self.editor.note_blocks[index].kind = kind.clone();
        self.editor.refresh_note_block_input(index, cx);
        self.editor.note_blocks[index].checked = false;
        if !matches!(
            kind,
            NoteBlockKind::Bullet | NoteBlockKind::Ordered | NoteBlockKind::Todo
        ) {
            self.editor.note_blocks[index].indent = 0;
        }
        if kind == NoteBlockKind::Rule {
            self.editor.suppress_events = true;
            self.editor.note_blocks[index]
                .input
                .update(cx, |input, cx| input.set_markdown("", cx));
            self.editor
                .insert_note_block(index + 1, NoteBlockSpec::paragraph(""), window, cx);
            self.editor.suppress_events = false;
            self.editor.active_note_block = index + 1;
        }
        self.sync_note_blocks_to_source(window, cx);
        self.editor.focus_active_editor(window, cx);
        self.status = Some(
            if from_slash {
                "已通过 / 命令插入块"
            } else {
                "已转换当前块"
            }
            .into(),
        );
        cx.notify();
    }

    fn sync_note_blocks_to_source(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor.suppress_events {
            return;
        }
        self.editor.commit_note_history(cx);
        let markdown = note_blocks_markdown(&self.editor.note_blocks, cx);
        self.editor.suppress_events = true;
        self.editor
            .body_input
            .update(cx, |input, cx| input.set_value(markdown, window, cx));
        self.editor.suppress_events = false;
        self.mark_editor_dirty(cx);
    }

    fn apply_note_input_update(
        &mut self,
        input: &Entity<RichInputState>,
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
        input.update(cx, |input, cx| {
            input.replace_text_in_range(Some(range_utf16), replacement, window, cx);
            input.set_cursor_offset(cursor, cx);
            input.focus(window);
        });
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

    fn report_note_edit_error(&mut self, error: NoteEditError, cx: &mut Context<Self>) {
        self.status = Some(
            match error {
                NoteEditError::ProtectedBlock => "受保护的未知富文本块不能修改",
                NoteEditError::CodeBlock => "代码块内容保持原样，请在普通文本块上应用行内格式",
                NoteEditError::UnclosedCodeFence => "正文含未闭合代码或高级块标记，请先修复",
            }
            .into(),
        );
        cx.notify();
    }
}

fn note_block_kind_for_style(style: NoteBlockStyle) -> NoteBlockKind {
    match style {
        NoteBlockStyle::Paragraph => NoteBlockKind::Paragraph,
        NoteBlockStyle::Heading1 => NoteBlockKind::Heading(1),
        NoteBlockStyle::Heading2 => NoteBlockKind::Heading(2),
        NoteBlockStyle::Heading3 => NoteBlockKind::Heading(3),
        NoteBlockStyle::Quote => NoteBlockKind::Quote,
        NoteBlockStyle::Bullet => NoteBlockKind::Bullet,
        NoteBlockStyle::Ordered => NoteBlockKind::Ordered,
        NoteBlockStyle::Todo => NoteBlockKind::Todo,
        NoteBlockStyle::Code => NoteBlockKind::Code {
            language: String::new(),
        },
        NoteBlockStyle::Rule => NoteBlockKind::Rule,
        NoteBlockStyle::Callout => NoteBlockKind::Callout {
            icon: "💡".into()
        },
        NoteBlockStyle::Toggle => NoteBlockKind::Toggle { expanded: true },
        NoteBlockStyle::Equation => NoteBlockKind::Equation,
        NoteBlockStyle::Bookmark => NoteBlockKind::Bookmark,
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

fn filter_note_mention_candidates(
    items: &[NodeSummary],
    current_id: Option<&str>,
    query: &str,
    limit: usize,
) -> Vec<NodeSummary> {
    let query = query.trim().to_lowercase();
    let mut candidates = items
        .iter()
        .filter(|item| Some(item.id.as_str()) != current_id)
        .filter(|item| query.is_empty() || item.title.to_lowercase().contains(&query))
        .cloned()
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        let left_title = left.title.to_lowercase();
        let right_title = right.title.to_lowercase();
        (!left_title.starts_with(&query))
            .cmp(&(!right_title.starts_with(&query)))
            .then_with(|| left_title.cmp(&right_title))
            .then_with(|| left.id.cmp(&right.id))
    });
    candidates.truncate(limit);
    candidates
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

    #[test]
    fn mention_candidates_prioritize_prefixes_and_exclude_the_current_page() {
        let item = |id: &str, title: &str| NodeSummary {
            id: id.into(),
            parent_id: None,
            kind: NodeKind::Note,
            title: title.into(),
            updated_at: 0,
            deleted: false,
            depth: 0,
            snippet: String::new(),
        };
        let items = vec![
            item("current", "Project current"),
            item("inside", "My project archive"),
            item("prefix", "Project plan"),
            item("other", "Random"),
        ];

        let candidates = filter_note_mention_candidates(&items, Some("current"), "project", 8);

        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.id.as_str())
                .collect::<Vec<_>>(),
            vec!["prefix", "inside"]
        );
    }
}
