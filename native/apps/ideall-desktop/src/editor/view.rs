use super::*;

use gpui_component::{Sizable as _, button::ButtonVariants as _, scroll::ScrollableElement as _};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EditorMode {
    Plain,
    Code,
    Preview,
}

impl EditorWorkspace {
    pub(crate) fn mode(&self) -> EditorMode {
        match self.active_engine_id.as_deref() {
            Some("ideall.code") if self.selected_kind == Some(NodeKind::File) => EditorMode::Code,
            Some("ideall.preview") if self.selected_kind == Some(NodeKind::File) => {
                EditorMode::Preview
            }
            _ => EditorMode::Plain,
        }
    }
}

impl IdeallDesktop {
    pub(crate) fn render_note_toolbar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let content = div()
            .h_full()
            .flex()
            .flex_none()
            .items_center()
            .gap_1()
            .px_3()
            .children(
                [
                    (
                        "note-block-paragraph",
                        "正文",
                        "正文块",
                        NoteBlockStyle::Paragraph,
                    ),
                    ("note-block-h1", "H1", "一级标题", NoteBlockStyle::Heading1),
                    ("note-block-h2", "H2", "二级标题", NoteBlockStyle::Heading2),
                    ("note-block-bullet", "•", "项目列表", NoteBlockStyle::Bullet),
                    (
                        "note-block-ordered",
                        "1.",
                        "编号列表",
                        NoteBlockStyle::Ordered,
                    ),
                    ("note-block-todo", "☐", "待办", NoteBlockStyle::Todo),
                    ("note-block-quote", "❝", "引用", NoteBlockStyle::Quote),
                    ("note-block-code", "</>", "代码块", NoteBlockStyle::Code),
                ]
                .into_iter()
                .map(|(id, label, tooltip, style)| {
                    Button::new(id)
                        .label(label)
                        .tooltip(tooltip)
                        .small()
                        .ghost()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.format_note_block(style, window, cx);
                        }))
                }),
            )
            .child(div().w(px(1.0)).h(px(20.0)).mx_1().bg(rgb(BORDER_STRONG)))
            .children(
                [
                    (
                        "note-inline-bold",
                        "B",
                        "粗体 · Ctrl/⌘ B",
                        NoteInlineStyle::Bold,
                    ),
                    (
                        "note-inline-italic",
                        "I",
                        "斜体 · Ctrl/⌘ I",
                        NoteInlineStyle::Italic,
                    ),
                    (
                        "note-inline-strike",
                        "S̶",
                        "删除线 · Ctrl/⌘ Shift S",
                        NoteInlineStyle::Strikethrough,
                    ),
                    ("note-inline-code", "`a`", "行内代码", NoteInlineStyle::Code),
                    (
                        "note-inline-link",
                        "链接",
                        "添加 HTTP(S) 链接",
                        NoteInlineStyle::Link,
                    ),
                ]
                .into_iter()
                .map(|(id, label, tooltip, style)| {
                    Button::new(id)
                        .label(label)
                        .tooltip(tooltip)
                        .small()
                        .ghost()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.format_note_inline(style, window, cx);
                        }))
                }),
            )
            .child(div().w(px(1.0)).h(px(20.0)).mx_1().bg(rgb(BORDER_STRONG)))
            .child(
                Button::new("note-todo-toggle")
                    .label("✓")
                    .tooltip("切换待办完成状态")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_current_note_todo(window, cx);
                    })),
            )
            .child(
                Button::new("note-indent-out")
                    .label("←")
                    .tooltip("减少列表层级")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.indent_current_note_block(NoteBlockIndent::Out, window, cx);
                    })),
            )
            .child(
                Button::new("note-indent-in")
                    .label("→")
                    .tooltip("增加列表层级")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.indent_current_note_block(NoteBlockIndent::In, window, cx);
                    })),
            )
            .child(
                div()
                    .whitespace_nowrap()
                    .text_xs()
                    .text_color(rgb(INK_FAINT))
                    .child("/ 更多"),
            );

        // ScrollableElement moves the wrapped element's own style to an outer
        // viewport. Keep flex-row on a child so scrolling cannot reset it.
        let viewport = div()
            .w_full()
            .h(px(36.0))
            .rounded_md()
            .border_1()
            .border_color(rgb(BORDER))
            .bg(rgb(SURFACE_MUTED))
            .child(content)
            .overflow_x_scrollbar();

        div()
            .h(px(48.0))
            .flex()
            .items_center()
            .px_4()
            .border_b_1()
            .border_color(rgb(BORDER))
            .bg(rgb(EDITOR))
            .child(div().w_full().max_w(px(760.0)).mx_auto().child(viewport))
            .into_any_element()
    }

    pub(crate) fn render_note_slash_menu(
        &self,
        commands: Vec<NoteSlashCommand>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        div()
            .id("note-slash-menu")
            .absolute()
            .top(px(88.0))
            .left(px(64.0))
            .w(px(360.0))
            .max_h(px(320.0))
            // The component scrollbar wrapper forces `position: relative` and
            // `size: full`, which moves this popover into document flow and
            // stretches a single result to the maximum height. Native GPUI
            // scrolling preserves the absolute, content-sized popover.
            .overflow_y_scroll()
            .p_1()
            .rounded_lg()
            .border_1()
            .border_color(rgb(BORDER_STRONG))
            .bg(rgb(SURFACE_RAISED))
            .shadow_lg()
            .child(
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(rgb(INK_FAINT))
                    .child("基础块 · Enter 选择第一项"),
            )
            .children(commands.into_iter().map(|command| {
                let style = command.style;
                div()
                    .id(SharedString::from(format!("note-slash-{}", command.id)))
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_2()
                    .py_2()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|item| item.bg(rgb(HOVER)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.run_note_slash_command(style, window, cx);
                    }))
                    .child(
                        div()
                            .w(px(82.0))
                            .text_sm()
                            .text_color(rgb(INK))
                            .child(command.label),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(rgb(INK_MUTED))
                            .child(command.hint),
                    )
            }))
            .into_any_element()
    }
}

pub(crate) fn engine_icon(engine_id: &str) -> &'static str {
    match engine_id {
        "ideall.code" => "<>",
        "ideall.preview" => "◫",
        "ideall.browser" => "↗",
        "ideall.note" => "N",
        "ideall.directory" => "◆",
        _ => "•",
    }
}

pub(crate) fn kind_color_for_engine(engine_id: &str) -> u32 {
    match engine_id {
        "ideall.code" => 0x9b8afb,
        "ideall.preview" => 0x3574f0,
        "ideall.browser" => 0x478c6c,
        "ideall.directory" => 0xb47b18,
        _ => INK_MUTED,
    }
}
