use super::*;

use crate::workbench::actions::{
    CopyNoteBlocks, DeleteNoteBlocks, DuplicateNoteBlock, MoveNoteBlockDown, MoveNoteBlockUp,
};
use gpui_component::{
    Sizable as _,
    button::ButtonVariants as _,
    menu::{DropdownMenu as _, PopupMenuItem},
};

fn note_color_label(color: NoteColor) -> &'static str {
    match color {
        NoteColor::Gray => "灰色",
        NoteColor::Brown => "棕色",
        NoteColor::Orange => "橙色",
        NoteColor::Yellow => "黄色",
        NoteColor::Green => "绿色",
        NoteColor::Blue => "蓝色",
        NoteColor::Purple => "紫色",
        NoteColor::Pink => "粉色",
        NoteColor::Red => "红色",
    }
}

fn note_block_style(kind: &NoteEditorBlockKind) -> Option<NoteBlockStyle> {
    match kind {
        NoteEditorBlockKind::Paragraph => Some(NoteBlockStyle::Paragraph),
        NoteEditorBlockKind::Heading(1) => Some(NoteBlockStyle::Heading1),
        NoteEditorBlockKind::Heading(2) => Some(NoteBlockStyle::Heading2),
        NoteEditorBlockKind::Heading(_) => Some(NoteBlockStyle::Heading3),
        NoteEditorBlockKind::Quote => Some(NoteBlockStyle::Quote),
        NoteEditorBlockKind::Bullet => Some(NoteBlockStyle::Bullet),
        NoteEditorBlockKind::Ordered => Some(NoteBlockStyle::Ordered),
        NoteEditorBlockKind::Todo => Some(NoteBlockStyle::Todo),
        NoteEditorBlockKind::Code { .. } => Some(NoteBlockStyle::Code),
        NoteEditorBlockKind::Rule => Some(NoteBlockStyle::Rule),
        NoteEditorBlockKind::Callout { .. } => Some(NoteBlockStyle::Callout),
        NoteEditorBlockKind::Toggle { .. } => Some(NoteBlockStyle::Toggle),
        NoteEditorBlockKind::Equation => Some(NoteBlockStyle::Equation),
        NoteEditorBlockKind::Bookmark => Some(NoteBlockStyle::Bookmark),
        NoteEditorBlockKind::Protected => None,
    }
}

fn note_block_style_label(style: NoteBlockStyle) -> &'static str {
    match style {
        NoteBlockStyle::Paragraph => "正文",
        NoteBlockStyle::Heading1 => "标题 1",
        NoteBlockStyle::Heading2 => "标题 2",
        NoteBlockStyle::Heading3 => "标题 3",
        NoteBlockStyle::Quote => "引用",
        NoteBlockStyle::Bullet => "项目列表",
        NoteBlockStyle::Ordered => "编号列表",
        NoteBlockStyle::Todo => "待办",
        NoteBlockStyle::Code => "代码",
        NoteBlockStyle::Rule => "分隔线",
        NoteBlockStyle::Callout => "提示框",
        NoteBlockStyle::Toggle => "折叠内容",
        NoteBlockStyle::Equation => "公式",
        NoteBlockStyle::Bookmark => "网页书签",
    }
}

fn note_block_style_dropdown(
    current: Option<NoteBlockStyle>,
    selected_count: usize,
    desktop: Entity<IdeallDesktop>,
) -> gpui::AnyElement {
    let label = if selected_count > 1 {
        format!("已选 {selected_count} 块")
    } else {
        current
            .map(note_block_style_label)
            .unwrap_or("受保护块")
            .to_owned()
    };
    Button::new("note-block-style")
        .label(label)
        .tooltip("转换当前内容块")
        .small()
        .ghost()
        .disabled(selected_count > 1 || current.is_none())
        .dropdown_caret(true)
        .dropdown_menu(move |menu, _, _| {
            let menu = menu.label("转换为");
            [
                NoteBlockStyle::Paragraph,
                NoteBlockStyle::Heading1,
                NoteBlockStyle::Heading2,
                NoteBlockStyle::Heading3,
                NoteBlockStyle::Quote,
                NoteBlockStyle::Bullet,
                NoteBlockStyle::Ordered,
                NoteBlockStyle::Todo,
                NoteBlockStyle::Code,
                NoteBlockStyle::Rule,
                NoteBlockStyle::Callout,
                NoteBlockStyle::Toggle,
                NoteBlockStyle::Equation,
                NoteBlockStyle::Bookmark,
            ]
            .into_iter()
            .fold(menu, |menu, style| {
                let style_desktop = desktop.clone();
                menu.item(
                    PopupMenuItem::new(note_block_style_label(style))
                        .checked(current == Some(style))
                        .on_click(move |_, window, cx| {
                            style_desktop.update(cx, |this, cx| {
                                this.format_note_block(style, window, cx);
                            });
                        }),
                )
            })
        })
        .into_any_element()
}

fn note_color_dropdown(
    id: impl Into<SharedString>,
    foreground: bool,
    selected: Option<NoteColor>,
    disabled: bool,
    desktop: Entity<IdeallDesktop>,
) -> gpui::AnyElement {
    Button::new(id.into())
        .label(if foreground { "A色" } else { "高亮" })
        .tooltip(if foreground {
            "文字颜色"
        } else {
            "背景高亮"
        })
        .xsmall()
        .ghost()
        .selected(selected.is_some())
        .disabled(disabled)
        .dropdown_caret(true)
        .dropdown_menu(move |menu, _, _| {
            let default_desktop = desktop.clone();
            let menu = menu
                .label(if foreground {
                    "文字颜色"
                } else {
                    "背景高亮"
                })
                .item(
                    PopupMenuItem::new(if foreground {
                        "默认文字"
                    } else {
                        "无高亮"
                    })
                    .checked(selected.is_none())
                    .on_click(move |_, window, cx| {
                        default_desktop.update(cx, |this, cx| {
                            this.format_note_color(foreground, None, window, cx);
                        });
                    }),
                )
                .separator();
            NoteColor::ALL.into_iter().fold(menu, |menu, color| {
                let color_desktop = desktop.clone();
                menu.item(
                    PopupMenuItem::element(move |_, _| {
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(if foreground {
                                div()
                                    .w(px(20.0))
                                    .h(px(20.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(rgb(color.foreground_rgb()))
                                    .child("A")
                            } else {
                                div()
                                    .w(px(20.0))
                                    .h(px(20.0))
                                    .rounded_sm()
                                    .border_1()
                                    .border_color(rgb(color.foreground_rgb()))
                                    .bg(rgb(color.background_rgb()))
                            })
                            .child(note_color_label(color))
                    })
                    .checked(selected == Some(color))
                    .on_click(move |_, window, cx| {
                        color_desktop.update(cx, |this, cx| {
                            this.format_note_color(foreground, Some(color), window, cx);
                        });
                    }),
                )
            })
        })
        .into_any_element()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EditorMode {
    Plain,
    Text,
    Preview,
}

impl EditorWorkspace {
    pub(crate) fn mode(&self) -> EditorMode {
        match self.active_engine_id.as_deref() {
            Some("ideall.text") if self.selected_kind == Some(NodeKind::File) => EditorMode::Text,
            Some("ideall.preview") if self.selected_kind == Some(NodeKind::File) => {
                EditorMode::Preview
            }
            _ => EditorMode::Plain,
        }
    }
}

impl IdeallDesktop {
    pub(crate) fn render_note_link_editor(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let has_existing_link = self
            .editor
            .link_editor
            .as_ref()
            .is_some_and(|editor| editor.has_existing_link);
        div()
            .min_h(px(58.0))
            .w_full()
            .flex()
            .items_center()
            .px_4()
            .border_b_1()
            .border_color(rgb(BORDER))
            .bg(rgb(0xf7faf8))
            .child(
                div()
                    .w_full()
                    .max_w(px(760.0))
                    .mx_auto()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(28.0))
                            .h(px(28.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .bg(rgb(ACCENT_SOFT))
                            .text_color(rgb(ACCENT))
                            .child("↗"),
                    )
                    .child(
                        div()
                            .w(px(72.0))
                            .flex_none()
                            .text_xs()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(INK_MUTED))
                            .child(if has_existing_link {
                                "编辑链接"
                            } else {
                                "添加链接"
                            }),
                    )
                    .child(
                        Input::new(&self.editor.link_input)
                            .flex_1()
                            .min_w(px(180.0))
                            .small()
                            .cleanable(true),
                    )
                    .when(has_existing_link, |bar| {
                        bar.child(
                            Button::new("note-link-remove")
                                .label("移除")
                                .tooltip("移除链接但保留文字")
                                .small()
                                .ghost()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.remove_note_link(window, cx);
                                })),
                        )
                    })
                    .child(
                        Button::new("note-link-cancel")
                            .label("取消")
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.cancel_note_link(window, cx);
                            })),
                    )
                    .child(
                        Button::new("note-link-apply")
                            .label("应用")
                            .small()
                            .primary()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.commit_note_link(window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn render_note_block_editor(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let editable = self.editor.body_editable && !self.show_trash;
        let interactive = editable && !self.editor.note_preview;
        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scrollbar()
            .bg(rgb(EDITOR))
            .child(
                div()
                    .w_full()
                    .max_w(px(820.0))
                    .mx_auto()
                    .px_5()
                    .pt_4()
                    .pb_12()
                    .children(self.editor.note_blocks.iter().enumerate().map(
                        |(index, block)| {
                            let id = block.id;
                            let input = block.input.clone();
                            let rich_input = block.rich_input.clone();
                            let kind = block.kind.clone();
                            let is_active = index == self.editor.active_note_block;
                            let is_selected = self.editor.selected_note_blocks.contains(&id);
                            let dragged_ids = if is_selected {
                                self.editor.selected_note_block_ids()
                            } else {
                                vec![id]
                            };
                            let indent = block.indent;
                            let checked = block.checked;
                            let value = input.read(cx).value().to_string();
                            let is_rule = kind == NoteEditorBlockKind::Rule;
                            let is_protected = kind == NoteEditorBlockKind::Protected;
                            let is_todo = kind == NoteEditorBlockKind::Todo;
                            let is_code = matches!(kind, NoteEditorBlockKind::Code { .. });
                            let is_callout =
                                matches!(kind, NoteEditorBlockKind::Callout { .. });
                            let is_toggle = matches!(kind, NoteEditorBlockKind::Toggle { .. });
                            let is_equation = kind == NoteEditorBlockKind::Equation;
                            let is_bookmark = kind == NoteEditorBlockKind::Bookmark;
                            let toggle_expanded = match &kind {
                                NoteEditorBlockKind::Toggle { expanded } => *expanded,
                                _ => true,
                            };
                            let callout_icon = match &kind {
                                NoteEditorBlockKind::Callout { icon } => Some(icon.clone()),
                                _ => None,
                            };
                            let prefix = match &kind {
                                NoteEditorBlockKind::Bullet => Some("•".to_owned()),
                                NoteEditorBlockKind::Ordered => Some(format!("{}.", index + 1)),
                                _ => None,
                            };
                            let show_input = interactive
                                && (!is_toggle || toggle_expanded)
                                && !is_rule
                                && !is_protected;
                            let show_rich_input = show_input && rich_input.is_some() && !is_bookmark;
                            let show_selection_toolbar = show_rich_input
                                && is_active
                                && self.editor.link_editor.is_none()
                                && rich_input
                                    .as_ref()
                                    .is_some_and(|input| input.read(cx).has_selection());
                            let selection_bold = rich_input.as_ref().is_some_and(|input| {
                                input
                                    .read(cx)
                                    .selection_has_style(NoteInlineStyle::Bold)
                            });
                            let selection_italic = rich_input.as_ref().is_some_and(|input| {
                                input
                                    .read(cx)
                                    .selection_has_style(NoteInlineStyle::Italic)
                            });
                            let selection_strike = rich_input.as_ref().is_some_and(|input| {
                                input
                                    .read(cx)
                                    .selection_has_style(NoteInlineStyle::Strikethrough)
                            });
                            let selection_code = rich_input.as_ref().is_some_and(|input| {
                                input
                                    .read(cx)
                                    .selection_has_style(NoteInlineStyle::Code)
                            });
                            let selection_link = rich_input.as_ref().is_some_and(|input| {
                                input
                                    .read(cx)
                                    .selection_has_style(NoteInlineStyle::Link)
                            });
                            let selection_text_color = rich_input
                                .as_ref()
                                .and_then(|input| input.read(cx).selection_color(true));
                            let selection_highlight = rich_input
                                .as_ref()
                                .and_then(|input| input.read(cx).selection_color(false));
                            let rich_source = if is_equation {
                                value.clone()
                            } else if is_toggle && !toggle_expanded {
                                value.lines().next().unwrap_or_default().to_owned()
                            } else {
                                value.clone()
                            };
                            let rich_text = rich_input.clone().map_or_else(
                                || {
                                    TextView::markdown(
                                        SharedString::from(format!("note-block-rich-{id}")),
                                        rich_source,
                                        window,
                                        cx,
                                    )
                                    .selectable(!interactive)
                                    .into_any_element()
                                },
                                |input| input.into_any_element(),
                            );

                            div()
                                .id(SharedString::from(format!("note-block-row-{id}")))
                                .w_full()
                                .flex()
                                .items_start()
                                .relative()
                                .rounded_md()
                                .ml(px((indent.min(8) * 22) as f32))
                                .when(interactive && is_selected, |row| {
                                    row.bg(rgb(SELECTION))
                                        .border_l_2()
                                        .border_color(rgb(ACCENT))
                                })
                                .when(interactive, |row| {
                                    row.on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(
                                            move |this,
                                                  event: &MouseDownEvent,
                                                  window,
                                                  cx| {
                                                this.select_note_block_with_modifiers(
                                                    id, event, window, cx,
                                                );
                                            },
                                        ),
                                    )
                                    .drag_over::<DraggedNoteBlocks>(|style, _, _, _| {
                                        style
                                            .border_t_2()
                                            .border_color(rgb(ACCENT))
                                            .bg(rgb(ACCENT_SOFT))
                                    })
                                    .on_drop(cx.listener(
                                        move |this,
                                              drag: &DraggedNoteBlocks,
                                              window,
                                              cx| {
                                            this.move_note_blocks(
                                                drag.ids.clone(),
                                                Some(id),
                                                window,
                                                cx,
                                            );
                                        },
                                    ))
                                })
                                .child(
                                    div()
                                        .absolute()
                                        .left(px(-22.0))
                                        .top(px(0.0))
                                        .w(px(42.0))
                                        .h(px(34.0))
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .gap(px(1.0))
                                        .when(interactive && (is_active || is_selected), |gutter| {
                                            gutter
                                                .child(
                                                    Button::new(SharedString::from(format!(
                                                        "note-block-add-{id}"
                                                    )))
                                                    .label("+")
                                                    .tooltip("在下方插入块")
                                                    .xsmall()
                                                    .ghost()
                                                    .on_mouse_down(
                                                        MouseButton::Left,
                                                        cx.listener(
                                                            move |this,
                                                                  _:
                                                                      &MouseDownEvent,
                                                                  window,
                                                                  cx| {
                                                                cx.stop_propagation();
                                                            this.insert_note_block_after(
                                                                id, window, cx,
                                                            );
                                                        },
                                                        ),
                                                    ),
                                                )
                                                .child(
                                                    div()
                                                        .id(SharedString::from(format!(
                                                            "note-block-handle-{id}"
                                                        )))
                                                        .w(px(20.0))
                                                        .h(px(24.0))
                                                        .cursor_grab()
                                                        .on_mouse_down(
                                                            MouseButton::Left,
                                                            cx.listener(
                                                                move |this,
                                                                      event:
                                                                          &MouseDownEvent,
                                                                      window,
                                                                      cx| {
                                                                    this.select_note_block_with_modifiers(
                                                                        id, event, window, cx,
                                                                    );
                                                                },
                                                            ),
                                                        )
                                                        .on_drag(
                                                            DraggedNoteBlocks {
                                                                ids: dragged_ids.clone(),
                                                            },
                                                            |drag, _, _, cx| {
                                                                cx.stop_propagation();
                                                                cx.new(|_| drag.clone())
                                                            },
                                                        )
                                                        .child(
                                                            Button::new(SharedString::from(format!(
                                                                "note-block-menu-{id}"
                                                            )))
                                                            .label("⋮⋮")
                                                            .xsmall()
                                                            .ghost()
                                                            .w(px(20.0))
                                                            .h(px(24.0))
                                                            .p_0()
                                                            .dropdown_menu(|menu, _, _| {
                                                                menu.label("内容块")
                                                                    .menu(
                                                                        "上移",
                                                                        Box::new(MoveNoteBlockUp),
                                                                    )
                                                                    .menu(
                                                                        "下移",
                                                                        Box::new(MoveNoteBlockDown),
                                                                    )
                                                                    .separator()
                                                                    .menu(
                                                                        "复制到剪贴板",
                                                                        Box::new(CopyNoteBlocks),
                                                                    )
                                                                    .menu(
                                                                        "创建副本",
                                                                        Box::new(
                                                                            DuplicateNoteBlock,
                                                                        ),
                                                                    )
                                                                    .separator()
                                                                    .menu(
                                                                        "删除",
                                                                        Box::new(
                                                                            DeleteNoteBlocks,
                                                                        ),
                                                                    )
                                                            }),
                                                        ),
                                                )
                                        }),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .ml(px(20.0))
                                        .min_h(px(34.0))
                                        .relative()
                                        .flex()
                                        .items_start()
                                        .when(
                                            matches!(kind, NoteEditorBlockKind::Quote),
                                            |content| {
                                                content
                                                    .border_l_1()
                                                    .border_color(rgb(INK_MUTED))
                                                    .pl_3()
                                            },
                                        )
                                        .when(
                                            is_code,
                                            |content| {
                                                content
                                                    .p_2()
                                                    .rounded_md()
                                                    .border_1()
                                                    .border_color(rgb(BORDER))
                                                    .bg(rgb(SURFACE_MUTED))
                                            },
                                        )
                                        .when(is_callout, |content| {
                                            content
                                                .my_1()
                                                .p_3()
                                                .gap_2()
                                                .rounded_lg()
                                                .border_1()
                                                .border_color(rgb(BORDER))
                                                .bg(rgb(ACCENT_SOFT))
                                        })
                                        .when(is_toggle, |content| {
                                            content
                                                .my_1()
                                                .py_1()
                                                .border_b_1()
                                                .border_color(rgb(BORDER))
                                        })
                                        .when(is_equation, |content| {
                                            content
                                                .my_1()
                                                .p_3()
                                                .justify_center()
                                                .text_size(px(18.0))
                                                .font_weight(gpui::FontWeight::MEDIUM)
                                                .rounded_md()
                                                .border_1()
                                                .border_color(rgb(BORDER))
                                                .bg(rgb(SURFACE))
                                        })
                                        .when(is_bookmark, |content| {
                                            content
                                                .my_1()
                                                .p_3()
                                                .gap_2()
                                                .items_center()
                                                .rounded_lg()
                                                .border_1()
                                                .border_color(rgb(BORDER_STRONG))
                                                .bg(rgb(SURFACE))
                                        })
                                        .when_some(callout_icon, |content, icon| {
                                            content.child(
                                                Button::new(SharedString::from(format!(
                                                    "note-callout-icon-{id}"
                                                )))
                                                .label(icon)
                                                .tooltip("切换提示框图标")
                                                .small()
                                                .ghost()
                                                .disabled(!interactive)
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.cycle_note_callout_icon(
                                                            id, window, cx,
                                                        );
                                                    },
                                                )),
                                            )
                                        })
                                        .when(is_toggle, |content| {
                                            content.child(
                                                Button::new(SharedString::from(format!(
                                                    "note-toggle-{id}"
                                                )))
                                                .label(if toggle_expanded { "⌄" } else { "›" })
                                                .tooltip(if toggle_expanded {
                                                    "收起内容"
                                                } else {
                                                    "展开内容"
                                                })
                                                .small()
                                                .ghost()
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.toggle_note_disclosure(
                                                            id, window, cx,
                                                        );
                                                    },
                                                )),
                                            )
                                        })
                                        .when(is_todo, |content| {
                                            content.child(
                                                Button::new(SharedString::from(format!(
                                                    "note-block-check-{id}"
                                                )))
                                                .label(if checked { "☑" } else { "☐" })
                                                .tooltip("切换完成状态")
                                                .small()
                                                .ghost()
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.toggle_note_block_checked(
                                                            id, window, cx,
                                                        );
                                                    },
                                                )),
                                            )
                                        })
                                        .when_some(prefix, |content, prefix| {
                                            content.child(
                                                div()
                                                    .w(px(30.0))
                                                    .h(px(34.0))
                                                    .flex_none()
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .text_base()
                                                    .text_color(rgb(INK_MUTED))
                                                    .child(prefix),
                                            )
                                        })
                                        .when(is_rule, |content| {
                                            content.child(
                                                div()
                                                    .w_full()
                                                    .h(px(34.0))
                                                    .flex()
                                                    .items_center()
                                                    .child(
                                                        div()
                                                            .w_full()
                                                            .h(px(1.0))
                                                            .bg(rgb(BORDER_STRONG)),
                                                    ),
                                            )
                                        })
                                        .when(is_protected, |content| {
                                            content.child(
                                                div()
                                                    .w_full()
                                                    .my_1()
                                                    .px_3()
                                                    .py_2()
                                                    .rounded_md()
                                                    .border_1()
                                                    .border_color(rgb(BORDER_STRONG))
                                                    .bg(rgb(SURFACE_MUTED))
                                                    .text_sm()
                                                    .text_color(rgb(INK_MUTED))
                                                    .child(
                                                        "该富文本块由兼容层保护，原始数据会原样保留",
                                                    ),
                                            )
                                        })
                                        .when(is_bookmark && show_input, |content| {
                                            content.child(
                                                Input::new(&input)
                                                    .w_full()
                                                    .min_h(px(34.0))
                                                    .text_size(px(15.0))
                                                    .appearance(false)
                                                    .bordered(false)
                                                    .focus_bordered(false),
                                            )
                                            .child(
                                                Button::new(SharedString::from(format!(
                                                    "note-bookmark-open-{id}"
                                                )))
                                                .label("打开")
                                                .tooltip("在系统浏览器中打开")
                                                .xsmall()
                                                .ghost()
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.open_note_bookmark(id, cx);
                                                })),
                                            )
                                        })
                                        .when(is_bookmark && !show_input, |content| {
                                            content
                                                .child(
                                                    div()
                                                        .w(px(30.0))
                                                        .h(px(30.0))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .rounded_md()
                                                        .bg(rgb(ACCENT_SOFT))
                                                        .text_color(rgb(ACCENT))
                                                        .child("↗"),
                                                )
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .flex()
                                                        .flex_col()
                                                        .gap(px(2.0))
                                                        .child(
                                                            div()
                                                                .text_sm()
                                                                .font_weight(
                                                                    gpui::FontWeight::SEMIBOLD,
                                                                )
                                                                .text_color(rgb(INK))
                                                                .child("网页书签"),
                                                        )
                                                        .child(
                                                            div()
                                                                .overflow_hidden()
                                                                .text_ellipsis()
                                                                .whitespace_nowrap()
                                                                .text_xs()
                                                                .text_color(rgb(INK_MUTED))
                                                                .child(value.clone()),
                                                        ),
                                                )
                                                .child(
                                                    Button::new(SharedString::from(format!(
                                                        "note-bookmark-open-card-{id}"
                                                    )))
                                                    .label("打开")
                                                    .xsmall()
                                                    .ghost()
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            this.open_note_bookmark(id, cx);
                                                        },
                                                    )),
                                                )
                                        })
                                        .when(show_selection_toolbar, |content| {
                                            content.child(
                                                div()
                                                    .absolute()
                                                    .when(index == 0, |toolbar| {
                                                        toolbar.top(px(36.0))
                                                    })
                                                    .when(index > 0, |toolbar| {
                                                        toolbar.top(px(-34.0))
                                                    })
                                                    .right(px(8.0))
                                                    .h(px(32.0))
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(1.0))
                                                    .px_1()
                                                    .rounded_md()
                                                    .border_1()
                                                    .border_color(rgb(BORDER_STRONG))
                                                    .bg(rgb(SURFACE_RAISED))
                                                    .shadow_md()
                                                    .children(
                                                        [
                                                            (
                                                                "bold",
                                                                "B",
                                                                "粗体",
                                                                NoteInlineStyle::Bold,
                                                                selection_bold,
                                                            ),
                                                            (
                                                                "italic",
                                                                "I",
                                                                "斜体",
                                                                NoteInlineStyle::Italic,
                                                                selection_italic,
                                                            ),
                                                            (
                                                                "strike",
                                                                "S̶",
                                                                "删除线",
                                                                NoteInlineStyle::Strikethrough,
                                                                selection_strike,
                                                            ),
                                                            (
                                                                "code",
                                                                "`a`",
                                                                "行内代码",
                                                                NoteInlineStyle::Code,
                                                                selection_code,
                                                            ),
                                                            (
                                                                "link",
                                                                "链接",
                                                                "添加或编辑链接",
                                                                NoteInlineStyle::Link,
                                                                selection_link,
                                                            ),
                                                        ]
                                                        .into_iter()
                                                        .map(
                                                            |(
                                                                key,
                                                                label,
                                                                tooltip,
                                                                style,
                                                                selected,
                                                            )| {
                                                                Button::new(
                                                                    SharedString::from(format!(
                                                                        "note-selection-{key}-{id}"
                                                                    )),
                                                                )
                                                                .label(label)
                                                                .tooltip(tooltip)
                                                                .xsmall()
                                                                .ghost()
                                                                .selected(selected)
                                                                .on_click(cx.listener(
                                                                    move |this, _, window, cx| {
                                                                        this.format_note_inline(
                                                                            style, window, cx,
                                                                        );
                                                                    },
                                                                ))
                                                            },
                                                        ),
                                                    )
                                                    .child(note_color_dropdown(
                                                        format!(
                                                            "note-selection-text-color-{id}"
                                                        ),
                                                        true,
                                                        selection_text_color,
                                                        false,
                                                        cx.entity(),
                                                    ))
                                                    .child(note_color_dropdown(
                                                        format!(
                                                            "note-selection-highlight-{id}"
                                                        ),
                                                        false,
                                                        selection_highlight,
                                                        false,
                                                        cx.entity(),
                                                    )),
                                            )
                                        })
                                        .when(
                                            show_rich_input,
                                            |content| {
                                                let rich_input = rich_input
                                                    .clone()
                                                    .expect("rich input was checked above");
                                                content.child(
                                                    div()
                                                        .w_full()
                                                        .min_h(px(34.0))
                                                        .py_1()
                                                        .text_size(match kind {
                                                            NoteEditorBlockKind::Heading(1) => {
                                                                px(28.0)
                                                            }
                                                            NoteEditorBlockKind::Heading(2) => {
                                                                px(23.0)
                                                            }
                                                            NoteEditorBlockKind::Heading(3) => {
                                                                px(19.0)
                                                            }
                                                            NoteEditorBlockKind::Heading(_) => {
                                                                px(17.0)
                                                            }
                                                            _ => px(16.0),
                                                        })
                                                        .line_height(match kind {
                                                            NoteEditorBlockKind::Heading(1) => {
                                                                px(38.0)
                                                            }
                                                            NoteEditorBlockKind::Heading(2) => {
                                                                px(34.0)
                                                            }
                                                            NoteEditorBlockKind::Heading(3) => {
                                                                px(30.0)
                                                            }
                                                            _ => px(26.0),
                                                        })
                                                        .font_weight(
                                                            if matches!(
                                                                kind,
                                                                NoteEditorBlockKind::Heading(_)
                                                            ) {
                                                                gpui::FontWeight::SEMIBOLD
                                                            } else {
                                                                gpui::FontWeight::NORMAL
                                                            },
                                                        )
                                                        .child(rich_input),
                                                )
                                            },
                                        )
                                        .when(
                                            show_input && !is_bookmark && !show_rich_input,
                                            |content| {
                                                content.child(
                                                    Input::new(&input)
                                                        .w_full()
                                                        .min_h(px(34.0))
                                                        .text_size(match kind {
                                                            NoteEditorBlockKind::Heading(1) => {
                                                                px(28.0)
                                                            }
                                                            NoteEditorBlockKind::Heading(2) => {
                                                                px(23.0)
                                                            }
                                                            NoteEditorBlockKind::Heading(3) => {
                                                                px(19.0)
                                                            }
                                                            NoteEditorBlockKind::Heading(_) => {
                                                                px(17.0)
                                                            }
                                                            NoteEditorBlockKind::Code { .. } => {
                                                                px(14.0)
                                                            }
                                                            _ => px(16.0),
                                                        })
                                                        .line_height(match kind {
                                                            NoteEditorBlockKind::Heading(1) => {
                                                                px(38.0)
                                                            }
                                                            NoteEditorBlockKind::Heading(2) => {
                                                                px(34.0)
                                                            }
                                                            NoteEditorBlockKind::Heading(3) => {
                                                                px(30.0)
                                                            }
                                                            _ => px(26.0),
                                                        })
                                                        .font_weight(
                                                            if matches!(
                                                                kind,
                                                                NoteEditorBlockKind::Heading(_)
                                                            ) {
                                                                gpui::FontWeight::SEMIBOLD
                                                            } else {
                                                                gpui::FontWeight::NORMAL
                                                            },
                                                        )
                                                        .appearance(false)
                                                        .bordered(false)
                                                        .focus_bordered(false),
                                                )
                                            },
                                        )
                                        .when(
                                            !show_input
                                                && !is_rule
                                                && !is_protected
                                                && !is_bookmark
                                                && !is_code,
                                            |content| {
                                                content.child(
                                                    div()
                                                        .w_full()
                                                        .min_h(px(34.0))
                                                        .py_1()
                                                        .text_size(match kind {
                                                        NoteEditorBlockKind::Heading(1) => px(28.0),
                                                        NoteEditorBlockKind::Heading(2) => px(23.0),
                                                        NoteEditorBlockKind::Heading(3) => px(19.0),
                                                        NoteEditorBlockKind::Heading(_) => px(17.0),
                                                        _ => px(16.0),
                                                        })
                                                        .line_height(match kind {
                                                            NoteEditorBlockKind::Heading(1) => px(38.0),
                                                            NoteEditorBlockKind::Heading(2) => px(34.0),
                                                            NoteEditorBlockKind::Heading(3) => px(30.0),
                                                            _ => px(26.0),
                                                        })
                                                        .font_weight(
                                                            if matches!(
                                                                kind,
                                                                NoteEditorBlockKind::Heading(_)
                                                            ) {
                                                                gpui::FontWeight::SEMIBOLD
                                                            } else {
                                                                gpui::FontWeight::NORMAL
                                                            },
                                                        )
                                                        .child(rich_text),
                                                )
                                            },
                                        )
                                        .when(!show_input && is_code, |content| {
                                            content.child(
                                                Input::new(&input)
                                                    .w_full()
                                                    .min_h(px(34.0))
                                                    .text_size(px(14.0))
                                                    .line_height(px(24.0))
                                                    .appearance(false)
                                                    .bordered(false)
                                                    .focus_bordered(false)
                                                    .disabled(true),
                                            )
                                        }),
                                )
                        },
                    ))
                    .when(interactive, |canvas| {
                        canvas.child(
                            div()
                                .id("note-block-drop-end")
                                .w_full()
                                .h(px(32.0))
                                .rounded_md()
                                .drag_over::<DraggedNoteBlocks>(|style, _, _, _| {
                                    style.bg(rgb(ACCENT_SOFT))
                                })
                                .on_drop(cx.listener(
                                    |this, drag: &DraggedNoteBlocks, window, cx| {
                                        this.move_note_blocks(
                                            drag.ids.clone(),
                                            None,
                                            window,
                                            cx,
                                        );
                                    },
                                )),
                        )
                    })
                    .when(!self.note_backlinks.is_empty(), |canvas| {
                        canvas.child(
                            div()
                                .mt_8()
                                .ml(px(48.0))
                                .pt_4()
                                .border_t_1()
                                .border_color(rgb(BORDER))
                                .child(
                                    div()
                                        .mb_2()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(rgb(INK_MUTED))
                                        .child(format!(
                                            "反向链接 · {}",
                                            self.note_backlinks.len()
                                        )),
                                )
                                .children(self.note_backlinks.iter().cloned().map(|backlink| {
                                    let id = backlink.id.clone();
                                    div()
                                        .id(SharedString::from(format!(
                                            "note-backlink-{}",
                                            backlink.id
                                        )))
                                        .h(px(34.0))
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .px_2()
                                        .rounded_md()
                                        .cursor_pointer()
                                        .text_sm()
                                        .text_color(rgb(INK))
                                        .hover(|row| row.bg(rgb(HOVER)))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.open_node(id.clone(), window, cx);
                                        }))
                                        .child(
                                            div()
                                                .text_color(rgb(ACCENT))
                                                .child("↗"),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .overflow_hidden()
                                                .text_ellipsis()
                                                .whitespace_nowrap()
                                                .child(backlink.title),
                                        )
                                })),
                        )
                    }),
            )
            .into_any_element()
    }

    pub(crate) fn render_note_toolbar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let selected_count = self.editor.selected_note_block_ids().len();
        let multiple_selected = selected_count > 1;
        let active_block = self.editor.note_blocks.get(
            self.editor
                .active_note_block
                .min(self.editor.note_blocks.len().saturating_sub(1)),
        );
        let current_block_style = active_block.and_then(|block| note_block_style(&block.kind));
        let list_like = active_block.is_some_and(|block| {
            matches!(
                block.kind,
                NoteEditorBlockKind::Bullet
                    | NoteEditorBlockKind::Ordered
                    | NoteEditorBlockKind::Todo
            )
        });
        let is_todo = active_block.is_some_and(|block| block.kind == NoteEditorBlockKind::Todo);
        let active_rich_input = self.editor.active_rich_input();
        let active_text_color = self
            .editor
            .active_rich_input()
            .and_then(|input| input.read(cx).selection_color(true));
        let active_highlight = self
            .editor
            .active_rich_input()
            .and_then(|input| input.read(cx).selection_color(false));
        div()
            .w_full()
            .h(px(44.0))
            .flex_none()
            .border_b_1()
            .border_color(rgb(BORDER))
            .bg(rgb(SURFACE))
            .child(
                div()
                    .w_full()
                    .max_w(px(760.0))
                    .h_full()
                    .mx_auto()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_5()
                    .child(note_block_style_dropdown(
                        current_block_style,
                        selected_count,
                        cx.entity(),
                    ))
                    .child(div().w(px(1.0)).h(px(20.0)).mx_1().bg(rgb(BORDER)))
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
                            ("note-inline-code", "</>", "行内代码", NoteInlineStyle::Code),
                            (
                                "note-inline-link",
                                "链接",
                                "添加或编辑链接",
                                NoteInlineStyle::Link,
                            ),
                        ]
                        .into_iter()
                        .map(|(id, label, tooltip, style)| {
                            let selected = active_rich_input
                                .is_some_and(|input| input.read(cx).selection_has_style(style));
                            Button::new(id)
                                .label(label)
                                .tooltip(tooltip)
                                .xsmall()
                                .ghost()
                                .selected(selected)
                                .disabled(multiple_selected)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.format_note_inline(style, window, cx);
                                }))
                        }),
                    )
                    .child(note_color_dropdown(
                        "note-inline-text-color",
                        true,
                        active_text_color,
                        multiple_selected,
                        cx.entity(),
                    ))
                    .child(note_color_dropdown(
                        "note-inline-highlight",
                        false,
                        active_highlight,
                        multiple_selected,
                        cx.entity(),
                    ))
                    .child(div().flex_1())
                    .when(is_todo, |toolbar| {
                        toolbar.child(
                            Button::new("note-todo-toggle")
                                .label("✓")
                                .tooltip("切换待办完成状态")
                                .xsmall()
                                .ghost()
                                .disabled(multiple_selected)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.toggle_current_note_todo(window, cx);
                                })),
                        )
                    })
                    .when(list_like, |toolbar| {
                        toolbar
                            .child(
                                Button::new("note-indent-out")
                                    .label("←")
                                    .tooltip("减少列表层级")
                                    .xsmall()
                                    .ghost()
                                    .disabled(multiple_selected)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.indent_current_note_block(
                                            NoteBlockIndent::Out,
                                            window,
                                            cx,
                                        );
                                    })),
                            )
                            .child(
                                Button::new("note-indent-in")
                                    .label("→")
                                    .tooltip("增加列表层级")
                                    .xsmall()
                                    .ghost()
                                    .disabled(multiple_selected)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.indent_current_note_block(
                                            NoteBlockIndent::In,
                                            window,
                                            cx,
                                        );
                                    })),
                            )
                    })
                    .child(
                        Button::new("note-block-more")
                            .label("•••")
                            .tooltip("更多块操作")
                            .xsmall()
                            .ghost()
                            .dropdown_menu(|menu, _, _| {
                                menu.label("块操作")
                                    .menu("上移", Box::new(MoveNoteBlockUp))
                                    .menu("下移", Box::new(MoveNoteBlockDown))
                                    .separator()
                                    .menu("复制到剪贴板", Box::new(CopyNoteBlocks))
                                    .menu("创建副本", Box::new(DuplicateNoteBlock))
                                    .separator()
                                    .menu("删除", Box::new(DeleteNoteBlocks))
                            }),
                    ),
            )
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
            .top(px(20.0))
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
                    .child("内容块 · Enter 选择第一项"),
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
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                        }),
                    )
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

    pub(crate) fn render_note_mention_menu(
        &self,
        candidates: Vec<NodeSummary>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        div()
            .id("note-mention-menu")
            .absolute()
            .top(px(20.0))
            .left(px(64.0))
            .w(px(360.0))
            .max_h(px(320.0))
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
                    .child("提及页面 · Enter 选择第一项"),
            )
            .children(candidates.into_iter().map(|candidate| {
                let target = candidate.clone();
                div()
                    .id(SharedString::from(format!("note-mention-{}", candidate.id)))
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_2()
                    .py_2()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|item| item.bg(rgb(HOVER)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                        }),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.run_note_mention(target.clone(), window, cx);
                    }))
                    .child(
                        div()
                            .w(px(28.0))
                            .h(px(28.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .bg(rgb(ACCENT_SOFT))
                            .text_color(rgb(ACCENT))
                            .child("@"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_sm()
                            .text_color(rgb(INK))
                            .child(candidate.title),
                    )
            }))
            .into_any_element()
    }
}

pub(crate) fn engine_icon(engine_id: &str) -> IconName {
    match engine_id {
        "ideall.browser" => IconName::Globe,
        "ideall.directory" => IconName::FolderClosed,
        _ => IconName::File,
    }
}
