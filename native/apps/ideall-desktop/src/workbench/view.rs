use super::actions::{
    ImportFiles, NewFolder, NewNote, NewTextFile, OpenSelectedNode, RenameSelectedNode,
    TrashSelectedNode,
};
use crate::*;

impl IdeallDesktop {
    fn render_top_bar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .h(px(54.0))
            .px(px(8.0))
            .pt(px(8.0))
            .bg(rgb(CANVAS))
            .child(
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(SURFACE))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Button::new("toggle-project-panel")
                                    .w(px(28.0))
                                    .h(px(28.0))
                                    .p_0()
                                    .icon(IconName::Menu)
                                    .small()
                                    .ghost()
                                    .tooltip("显示或隐藏项目面板 · Alt 1")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_project_panel(cx);
                                    })),
                            )
                            .child(
                                div()
                                    .w(px(26.0))
                                    .h(px(26.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_md()
                                    .bg(rgb(RAIL))
                                    .text_xs()
                                    .text_color(rgb(ACCENT_ON_RAIL))
                                    .child("i"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_baseline()
                                    .gap_2()
                                    .child(div().text_sm().text_color(rgb(INK)).child("ideall"))
                                    .child(
                                        div()
                                            .text_size(px(9.0))
                                            .text_color(rgb(INK_FAINT))
                                            .child("WORKSPACE"),
                                    ),
                            ),
                    )
                    .child(
                        div().flex_1().flex().justify_center().child(
                            Input::new(&self.search_input)
                                .w(px(400.0))
                                .small()
                                .prefix(
                                    div().pl_1().child(
                                        Icon::new(IconName::Search)
                                            .size(px(14.0))
                                            .text_color(rgb(INK_FAINT)),
                                    ),
                                )
                                .suffix(
                                    div()
                                        .pr_1()
                                        .text_xs()
                                        .text_color(rgb(INK_FAINT))
                                        .child("Ctrl K"),
                                )
                                .cleanable(true),
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .px_2()
                                    .text_xs()
                                    .text_color(rgb(INK_MUTED))
                                    .child(
                                        div().w(px(6.0)).h(px(6.0)).rounded_full().bg(rgb(ACCENT)),
                                    )
                                    .child("本地就绪"),
                            )
                            .child(
                                Button::new("top-new-file")
                                    .label("新建")
                                    .icon(IconName::Plus)
                                    .small()
                                    .primary()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.show_files(window, cx);
                                        this.create_text_file(window, cx);
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_activity_item(
        &self,
        navigation: NavigationItem,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let section = navigation.section;
        let selected = self.active_section.as_ref() == section
            || (section == "我的" && matches!(self.active_section.as_ref(), "搜索" | "回收站"));
        let icon = match section {
            "我的" => IconName::FolderOpen,
            "活动" => IconName::Calendar,
            "浏览" => IconName::Globe,
            "应用" => IconName::Bot,
            "设置" => IconName::Settings2,
            _ => IconName::LayoutDashboard,
        };
        let tooltip = if navigation.shortcut.is_empty() {
            navigation.label.to_string()
        } else {
            format!("{} · {}", navigation.label, navigation.shortcut)
        };

        Button::new(SharedString::from(format!("section-{section}")))
            .w(px(36.0))
            .h(px(36.0))
            .p_0()
            .border_0()
            .rounded_md()
            .ghost()
            .tooltip(tooltip)
            .cursor_pointer()
            .when(selected, |button| button.bg(rgb(RAIL_ACTIVE)))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.select_section(section, window, cx);
            }))
            .child(
                div()
                    .relative()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Icon::new(icon).size(px(19.0)).text_color(rgb(if selected {
                        ACCENT_ON_RAIL
                    } else {
                        RAIL_MUTED
                    })))
                    .when(selected, |content| {
                        content.child(
                            div()
                                .absolute()
                                .left(px(-6.0))
                                .top(px(9.0))
                                .w(px(2.0))
                                .h(px(18.0))
                                .rounded_full()
                                .bg(rgb(ACCENT_ON_RAIL)),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_activity_bar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .w(px(48.0))
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .p(px(6.0))
            .rounded_lg()
            .border_1()
            .border_color(rgb(RAIL))
            .bg(rgb(RAIL))
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.0))
                    .children(
                        NAVIGATION[..3]
                            .iter()
                            .copied()
                            .map(|navigation| self.render_activity_item(navigation, cx)),
                    ),
            )
            .child(div().flex_1())
            .child(
                div()
                    .w(px(22.0))
                    .h(px(1.0))
                    .my(px(7.0))
                    .bg(rgb(RAIL_ACTIVE)),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.0))
                    .children(
                        NAVIGATION[3..]
                            .iter()
                            .copied()
                            .map(|navigation| self.render_activity_item(navigation, cx)),
                    ),
            )
            .into_any_element()
    }

    fn render_tool_window(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let preserve_full_list = self.active_section.as_ref() != "我的";
        let items = self.explorer.visible_items(preserve_full_list);
        let all_items = self.explorer.items.clone();
        let item_count = self.explorer.items.len();
        let tree_selection = self.explorer.selection_id.clone();
        let can_create = !self.show_trash && self.active_section.as_ref() == "我的";
        let show_browse = self.active_section.as_ref() == "浏览";
        let show_context = !matches!(self.active_section.as_ref(), "应用" | "设置");
        let show_private_root = self.active_section.as_ref() == "回收站";
        let show_trash_root = self.active_section.as_ref() == "我的";
        let empty_message: SharedString = match self.active_section.as_ref() {
            "搜索" => "未找到匹配的本地内容".into(),
            "回收站" => "回收站为空".into(),
            "活动" => "还没有最近修改的内容".into(),
            "浏览" => "还没有书签或关注源".into(),
            "应用" => "在编辑区配置模型、ACP 与 Engine".into(),
            "设置" => "在编辑区管理同步、安全与更新".into(),
            _ => "项目为空，创建文件或导入本地内容".into(),
        };
        let section_title = match self.active_section.as_ref() {
            "我的" => "私有页面",
            "活动" => "最近更新",
            "浏览" => "关注与书签",
            "搜索" => "搜索结果",
            "回收站" => "回收站",
            section => section,
        };
        let section_icon = match self.active_section.as_ref() {
            "我的" => IconName::FolderOpen,
            "活动" => IconName::Calendar,
            "浏览" => IconName::Globe,
            "搜索" => IconName::Search,
            "回收站" => IconName::Delete,
            _ => IconName::FolderOpen,
        };

        div()
            .w_full()
            .h_full()
            .flex()
            .flex_col()
            .rounded_lg()
            .border_1()
            .border_color(rgb(BORDER))
            .bg(rgb(SURFACE))
            .overflow_hidden()
            .child(
                div()
                    .h(px(40.0))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(rgb(INK_MUTED))
                            .child("工作区"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(INK_FAINT))
                            .child(item_count.to_string()),
                    ),
            )
            .when(show_browse, |panel| {
                panel.child(
                    div()
                        .p_2()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .border_b_1()
                        .border_color(rgb(BORDER))
                        .child(Input::new(&self.feed_key_input).w_full().small())
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.0))
                                .child(
                                    Button::new("new-feed")
                                        .w_full()
                                        .h(px(32.0))
                                        .p_0()
                                        .primary()
                                        .child(activity_action_content("+", "新增关注"))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.create_feed(window, cx);
                                        })),
                                )
                                .child(
                                    Button::new("open-info-portal")
                                        .w_full()
                                        .h(px(32.0))
                                        .p_0()
                                        .ghost()
                                        .child(activity_action_content("◫", "资讯"))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.open_portal(
                                                "ideall.info",
                                                INFO_PORTAL_URL,
                                                window,
                                                cx,
                                            );
                                        })),
                                )
                                .child(
                                    Button::new("open-community-portal")
                                        .w_full()
                                        .h(px(32.0))
                                        .p_0()
                                        .ghost()
                                        .child(activity_action_content("◌", "社区"))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.open_portal(
                                                "ideall.community",
                                                COMMUNITY_PORTAL_URL,
                                                window,
                                                cx,
                                            );
                                        })),
                                ),
                        ),
                )
            })
            .when(show_context, |panel| {
                panel.child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .p_2()
                        .overflow_y_scrollbar()
                        .when(can_create, |list| {
                            list.drag_over::<DraggedNode>(|style, _, _, _| {
                                style.bg(rgb(ACCENT_SOFT))
                            })
                            .on_drop(cx.listener(
                                |this, node: &DraggedNode, _, cx| {
                                    this.move_explorer_node(node.id.clone(), None, cx);
                                },
                            ))
                        })
                        .when(show_private_root, |list| {
                            list.child(
                                div()
                                    .id("tree-root-private")
                                    .h(px(32.0))
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .px_2()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .text_sm()
                                    .text_color(rgb(INK_MUTED))
                                    .hover(|row| row.bg(rgb(HOVER)))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.show_files(window, cx);
                                    }))
                                    .child(
                                        Icon::new(IconName::ChevronRight)
                                            .size(px(14.0))
                                            .text_color(rgb(INK_FAINT)),
                                    )
                                    .child(
                                        Icon::new(IconName::FolderClosed)
                                            .size(px(16.0))
                                            .text_color(rgb(kind_color(NodeKind::Folder))),
                                    )
                                    .child("私有页面"),
                            )
                        })
                        .child(
                            div()
                                .id("tree-root-active")
                                .h(px(32.0))
                                .flex()
                                .items_center()
                                .gap_2()
                                .px_2()
                                .rounded_md()
                                .bg(rgb(ACCENT_SOFT))
                                .text_sm()
                                .text_color(rgb(INK))
                                .when(can_create, |row| {
                                    row.drag_over::<DraggedNode>(|style, _, _, _| {
                                        style.bg(rgb(SELECTION))
                                    })
                                    .on_drop(cx.listener(
                                        |this, node: &DraggedNode, _, cx| {
                                            this.move_explorer_node(node.id.clone(), None, cx);
                                        },
                                    ))
                                })
                                .child(
                                    Icon::new(IconName::ChevronDown)
                                        .size(px(14.0))
                                        .text_color(rgb(INK_FAINT)),
                                )
                                .child(Icon::new(section_icon).size(px(16.0)).text_color(rgb(
                                    if can_create {
                                        kind_color(NodeKind::Folder)
                                    } else {
                                        ACCENT
                                    },
                                )))
                                .child(
                                    div()
                                        .flex_1()
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .child(section_title.to_string()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(INK_FAINT))
                                        .child(item_count.to_string()),
                                )
                                .when(can_create, |row| {
                                    row.child(
                                        Button::new("tree-root-new-note")
                                            .w(px(22.0))
                                            .h(px(22.0))
                                            .p_0()
                                            .small()
                                            .ghost()
                                            .icon(IconName::Plus)
                                            .tooltip("在根目录新建页面")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.explorer.clear_selection();
                                                this.create_note(window, cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("tree-root-more")
                                            .w(px(22.0))
                                            .h(px(22.0))
                                            .p_0()
                                            .small()
                                            .ghost()
                                            .icon(IconName::Ellipsis)
                                            .tooltip("更多根目录操作")
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(|this, _, _, cx| {
                                                    this.explorer.clear_selection();
                                                    cx.notify();
                                                }),
                                            )
                                            .dropdown_menu(|menu, _, _| {
                                                menu.menu("新建页面", Box::new(NewNote))
                                                    .menu("新建文本文件", Box::new(NewTextFile))
                                                    .menu("新建目录", Box::new(NewFolder))
                                                    .separator()
                                                    .menu("导入文件", Box::new(ImportFiles))
                                            }),
                                    )
                                }),
                        )
                        .when(items.is_empty(), |list| {
                            list.child(
                                div()
                                    .ml(px(22.0))
                                    .mt_1()
                                    .px_2()
                                    .py_3()
                                    .text_xs()
                                    .text_color(rgb(INK_FAINT))
                                    .child(empty_message.clone()),
                            )
                        })
                        .children(items.into_iter().map(|item| {
                            let id = item.id.clone();
                            let is_folder = item.kind == NodeKind::Folder;
                            let selected = tree_selection.as_deref() == Some(id.as_str())
                                || (!is_folder
                                    && self.editor.selected_id.as_deref() == Some(id.as_str()));
                            let expanded = self.explorer.expanded_folders.contains(&id);
                            let renaming =
                                self.explorer.renaming_id.as_deref() == Some(id.as_str());
                            let rename_input = self.explorer.rename_input.clone();
                            let child_count = all_items
                                .iter()
                                .filter(|candidate| {
                                    candidate.parent_id.as_deref() == Some(id.as_str())
                                })
                                .count();
                            let click_id = id.clone();
                            let context_id = id.clone();
                            let folder_id = id.clone();
                            div()
                                .id(SharedString::from(format!("node-{id}")))
                                .h(px(30.0))
                                .flex()
                                .items_center()
                                .gap_1()
                                .pr_2()
                                .pl(px(22.0 + item.depth as f32 * 14.0))
                                .rounded_md()
                                .cursor_pointer()
                                .text_sm()
                                .when(selected, |row| row.bg(rgb(SELECTION)).text_color(rgb(INK)))
                                .hover(|row| row.bg(rgb(HOVER)))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    if is_folder {
                                        this.toggle_folder(click_id.clone(), cx);
                                    } else {
                                        this.open_node(click_id.clone(), window, cx);
                                    }
                                }))
                                .on_mouse_down(
                                    MouseButton::Right,
                                    cx.listener(move |this, _, _, cx| {
                                        this.explorer.select(context_id.clone());
                                        cx.notify();
                                    }),
                                )
                                .on_drag(
                                    DraggedNode {
                                        id: id.clone(),
                                        title: item.title.clone(),
                                    },
                                    |node: &DraggedNode, _, _, cx| cx.new(|_| node.clone()),
                                )
                                .when(is_folder, |row| {
                                    row.drag_over::<DraggedNode>(|style, _, _, _| {
                                        style.bg(rgb(ACCENT_SOFT))
                                    })
                                    .on_drop(cx.listener(
                                        move |this, node: &DraggedNode, _, cx| {
                                            this.move_explorer_node(
                                                node.id.clone(),
                                                Some(folder_id.clone()),
                                                cx,
                                            );
                                        },
                                    ))
                                })
                                .child(
                                    div()
                                        .w(px(14.0))
                                        .text_xs()
                                        .text_color(rgb(INK_FAINT))
                                        .child(if is_folder {
                                            if expanded { "⌄" } else { "›" }
                                        } else {
                                            ""
                                        }),
                                )
                                .child(
                                    div()
                                        .w(px(18.0))
                                        .text_xs()
                                        .text_color(rgb(kind_color(item.kind)))
                                        .child(kind_icon(item.kind)),
                                )
                                .when(renaming, |row| {
                                    row.child(
                                        Input::new(
                                            rename_input
                                                .as_ref()
                                                .expect("rename input is present while renaming"),
                                        )
                                        .w_full()
                                        .small()
                                        .appearance(false),
                                    )
                                    .child(
                                        Button::new(SharedString::from(format!(
                                            "commit-rename-{id}"
                                        )))
                                        .label("✓")
                                        .small()
                                        .ghost()
                                        .on_click(
                                            cx.listener(|this, _, window, cx| {
                                                cx.stop_propagation();
                                                this.commit_explorer_rename(window, cx);
                                            }),
                                        ),
                                    )
                                    .child(
                                        Button::new(SharedString::from(format!(
                                            "cancel-rename-{id}"
                                        )))
                                        .label("×")
                                        .small()
                                        .ghost()
                                        .on_click(
                                            cx.listener(|this, _, _, cx| {
                                                cx.stop_propagation();
                                                this.cancel_explorer_rename(cx);
                                            }),
                                        ),
                                    )
                                })
                                .when(!renaming, |row| {
                                    row.child(
                                        div()
                                            .flex_1()
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .whitespace_nowrap()
                                            .child(item.title),
                                    )
                                })
                                .when(is_folder && child_count > 0, |row| {
                                    row.child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(INK_FAINT))
                                            .child(child_count.to_string()),
                                    )
                                })
                                .context_menu(move |menu, _, _| {
                                    menu.menu("打开", Box::new(OpenSelectedNode))
                                        .when(is_folder && can_create, |menu| {
                                            menu.separator()
                                                .menu("新建页面", Box::new(NewNote))
                                                .menu("新建文本文件", Box::new(NewTextFile))
                                                .menu("新建子目录", Box::new(NewFolder))
                                                .menu("导入到此目录", Box::new(ImportFiles))
                                        })
                                        .menu("重命名  F2", Box::new(RenameSelectedNode))
                                        .menu_with_disabled(
                                            "移到回收站  Delete",
                                            Box::new(TrashSelectedNode),
                                            !can_create,
                                        )
                                })
                        }))
                        .when(show_trash_root, |list| {
                            list.child(
                                div()
                                    .id("tree-root-trash")
                                    .h(px(32.0))
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .px_2()
                                    .mt_1()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .text_sm()
                                    .text_color(rgb(INK_MUTED))
                                    .hover(|row| row.bg(rgb(HOVER)))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.show_trash(window, cx);
                                    }))
                                    .child(
                                        Icon::new(IconName::ChevronRight)
                                            .size(px(14.0))
                                            .text_color(rgb(INK_FAINT)),
                                    )
                                    .child(
                                        Icon::new(IconName::Delete)
                                            .size(px(16.0))
                                            .text_color(rgb(INK_FAINT)),
                                    )
                                    .child("回收站"),
                            )
                        }),
                )
            })
            .when(!show_context, |panel| {
                panel.child(
                    div()
                        .flex_1()
                        .p_4()
                        .text_sm()
                        .text_color(rgb(INK_MUTED))
                        .child(empty_message),
                )
            })
            .into_any_element()
    }

    fn render_tab_bar(
        &self,
        tabs: Vec<TabDescriptor>,
        active_tab: Option<String>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let content = div()
            .h_full()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(2.0))
            .px_1()
            .children(tabs.into_iter().map(|tab| {
                let id = tab_key(&tab);
                let close_id = id.clone();
                let selected = active_tab.as_deref() == Some(id.as_str());
                let dirty = selected && self.editor.dirty;
                div()
                    .id(SharedString::from(format!("tab-{id}")))
                    .relative()
                    .h(px(32.0))
                    .min_w(px(112.0))
                    .max_w(px(200.0))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .rounded_md()
                    .cursor_pointer()
                    .text_sm()
                    .text_color(rgb(if selected { INK } else { INK_MUTED }))
                    .when(selected, |item| {
                        item.bg(rgb(SURFACE)).child(
                            div()
                                .absolute()
                                .bottom(px(0.0))
                                .left(px(14.0))
                                .right(px(14.0))
                                .h(px(2.0))
                                .rounded_full()
                                .bg(rgb(ACCENT)),
                        )
                    })
                    .hover(|item| item.bg(rgb(HOVER)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.activate_tab(id.clone(), window, cx);
                    }))
                    .child(
                        div()
                            .w(px(14.0))
                            .text_xs()
                            .text_color(rgb(kind_color_for_engine(&tab.engine_id)))
                            .child(engine_icon(&tab.engine_id)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(tab.title),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("close-{close_id}")))
                            .w(px(14.0))
                            .text_center()
                            .text_color(rgb(if dirty { ACCENT } else { INK_FAINT }))
                            .child(if dirty { "●" } else { "×" })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.close_tab(close_id.clone(), window, cx);
                            })),
                    )
            }))
            .child(div().min_w(px(32.0)).flex_1());

        // Keep the flex row inside the scrollbar viewport. The component
        // wrapper owns the outer dimensions and otherwise resets the wrapped
        // element's layout style.
        div()
            .h(px(38.0))
            .border_b_1()
            .border_color(rgb(BORDER))
            .bg(rgb(SURFACE_MUTED))
            .child(content)
            .overflow_x_scrollbar()
            .into_any_element()
    }

    fn render_welcome(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .items_center()
            .justify_center()
            .p_6()
            .bg(rgb(SURFACE))
            .child(
                div()
                    .w_full()
                    .max_w(px(680.0))
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_4()
                            .child(
                                div()
                                    .w(px(46.0))
                                    .h(px(46.0))
                                    .rounded_lg()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(rgb(RAIL))
                                    .text_xl()
                                    .text_color(rgb(ACCENT_ON_RAIL))
                                    .child("i"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(div().text_xl().child("开始创作"))
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(rgb(INK_MUTED))
                                            .child("在本地工作区组织文件、笔记和智能任务"),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .pb_1()
                            .text_xs()
                            .text_color(rgb(INK_FAINT))
                            .child("快速开始"),
                    )
                    .child(
                        div()
                            .grid()
                            .grid_cols(2)
                            .gap_2()
                            .child(
                                welcome_action("welcome-new-file", "+", "新建文件", "Ctrl ⇧ N")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.create_text_file(window, cx);
                                    })),
                            )
                            .child(
                                welcome_action("welcome-new-note", "N", "新建笔记", "Ctrl N")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.create_note(window, cx);
                                    })),
                            )
                            .child(
                                welcome_action("welcome-import", "↥", "导入本地文件", "").on_click(
                                    cx.listener(|this, _, _, cx| {
                                        this.import_files(cx);
                                    }),
                                ),
                            )
                            .child(
                                welcome_action("welcome-agent", "✦", "打开 Agent 工具", "")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.select_section("应用", window, cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .pt_3()
                            .border_t_1()
                            .border_color(rgb(BORDER))
                            .text_xs()
                            .text_color(rgb(INK_FAINT))
                            .child(
                                div()
                                    .w(px(6.0))
                                    .h(px(6.0))
                                    .mr_2()
                                    .rounded_full()
                                    .bg(rgb(ACCENT)),
                            )
                            .child("数据保存在本机")
                            .child(div().flex_1())
                            .child("搜索 Ctrl K · 保存 Ctrl S"),
                    ),
            )
            .into_any_element()
    }

    fn render_apps(
        &self,
        engine_capabilities: Vec<ideall_domain::EngineRuntimeCapability>,
        agent_threads: Vec<NodeSummary>,
        agent_transcript: Vec<AgentTranscriptMessage>,
        agent_audits: Vec<AgentAuditRecord>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        div()
            .flex_1()
            .min_h_0()
            .overflow_y_scrollbar()
            .bg(rgb(CANVAS))
            .child(
                div()
                    .w_full()
                    .max_w(px(920.0))
                    .mx_auto()
                    .p_6()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(page_heading(
                        "Agent 工作台",
                        "连接模型、编排本机工具，并保留每一次写操作审计。",
                    ))
                    .child(
                        settings_card("模型连接 · OpenAI-compatible")
                            .child(field_label("API 基址"))
                            .child(Input::new(&self.agent_base_url_input).w_full())
                            .child(field_label("模型名称"))
                            .child(Input::new(&self.agent_model_input).w_full())
                            .child(field_label(if self.agent_key_configured {
                                "API Key · 已配置"
                            } else {
                                "API Key · 未配置"
                            }))
                            .child(Input::new(&self.agent_key_input).w_full().mask_toggle())
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(
                                        Button::new("save-agent-model")
                                            .label("保存模型设置")
                                            .primary()
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.save_agent_model_settings(window, cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("new-bookmark")
                                            .label("+ 书签")
                                            .small()
                                            .ghost()
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.create_bookmark(window, cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("clear-agent-key")
                                            .label("清除 Key")
                                            .danger()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.clear_agent_credential(cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("agent-create-draft")
                                            .label("测试：创建草稿")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.create_agent_draft(cx);
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        settings_card("外部 ACP Agent")
                            .child(div().text_sm().text_color(rgb(INK_MUTED)).child(
                                "仅运行本机明确配置的程序；参数按 argv 规则解析，不经过 shell。",
                            ))
                            .child(field_label("程序"))
                            .child(Input::new(&self.acp_program_input).w_full())
                            .child(field_label("参数"))
                            .child(Input::new(&self.acp_args_input).w_full())
                            .child(field_label("工作目录"))
                            .child(Input::new(&self.acp_cwd_input).w_full())
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(Button::new("save-acp").label("保存 ACP 设置").on_click(
                                        cx.listener(|this, _, window, cx| {
                                            this.save_external_acp_settings(window, cx);
                                        }),
                                    ))
                                    .child(
                                        Button::new("run-acp")
                                            .label(if self.agent_in_progress {
                                                "ACP 处理中…"
                                            } else {
                                                "发送当前消息"
                                            })
                                            .primary()
                                            .disabled(self.agent_in_progress)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.start_external_acp_turn(window, cx);
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        settings_card("本地对话")
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap_2()
                                    .child(
                                        Button::new("new-agent-thread")
                                            .label("+ 新对话")
                                            .small()
                                            .primary()
                                            .disabled(self.agent_in_progress)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.new_agent_thread(cx);
                                            })),
                                    )
                                    .children(agent_threads.into_iter().map(|thread| {
                                        let id = thread.id.clone();
                                        Button::new(SharedString::from(format!(
                                            "agent-thread-{}",
                                            thread.id
                                        )))
                                        .label(thread.title)
                                        .small()
                                        .selected(
                                            self.agent_thread_id.as_deref()
                                                == Some(thread.id.as_str()),
                                        )
                                        .disabled(self.agent_in_progress)
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.select_agent_thread(id.clone(), cx);
                                            }),
                                        )
                                    })),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .when(agent_transcript.is_empty(), |messages| {
                                        messages.child(
                                            div()
                                                .p_3()
                                                .rounded_md()
                                                .bg(rgb(SURFACE))
                                                .text_sm()
                                                .text_color(rgb(INK_MUTED))
                                                .child("输入消息开始本地对话"),
                                        )
                                    })
                                    .children(agent_transcript.into_iter().map(|message| {
                                        div()
                                            .p_3()
                                            .rounded_md()
                                            .border_1()
                                            .border_color(rgb(BORDER))
                                            .bg(rgb(match message.role {
                                                ModelRole::User => ACCENT_SOFT,
                                                ModelRole::Assistant => SURFACE,
                                                ModelRole::Tool => MESSAGE_TOOL,
                                                ModelRole::System => WARNING_SOFT,
                                            }))
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(rgb(INK_FAINT))
                                                    .child(agent_role_label(message.role)),
                                            )
                                            .child(message.content)
                                    })),
                            )
                            .child(Input::new(&self.agent_prompt_input).w_full())
                            .child(
                                Button::new("agent-send")
                                    .label(if self.agent_in_progress {
                                        "Agent 处理中…"
                                    } else {
                                        "发送给 Agent"
                                    })
                                    .primary()
                                    .disabled(self.agent_in_progress)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.start_agent_turn(window, cx);
                                    })),
                            ),
                    )
                    .child(settings_card("平台 Engine").children(
                        engine_capabilities.into_iter().map(|capability| {
                            div()
                                .flex()
                                .items_start()
                                .justify_between()
                                .gap_4()
                                .py_2()
                                .border_b_1()
                                .border_color(rgb(BORDER))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .child(format!(
                                            "{} · {}",
                                            capability.label, capability.engine_id
                                        ))
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(rgb(INK_MUTED))
                                                .child(capability.detail),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(if capability.kind.is_available() {
                                            ACCENT
                                        } else {
                                            DANGER
                                        }))
                                        .child(format!(
                                            "{} / {}",
                                            capability.kind.label(),
                                            engine_access_label(capability.access)
                                        )),
                                )
                        }),
                    ))
                    .child(
                        settings_card("最近写操作审计")
                            .when(agent_audits.is_empty(), |audits| {
                                audits.child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(INK_MUTED))
                                        .child("尚无 Agent 写操作"),
                                )
                            })
                            .children(agent_audits.into_iter().map(|audit| {
                                div()
                                    .flex()
                                    .items_start()
                                    .justify_between()
                                    .gap_4()
                                    .py_2()
                                    .border_b_1()
                                    .border_color(rgb(BORDER))
                                    .child(
                                        div().flex().flex_col().gap_1().child(audit.title).child(
                                            div().text_sm().text_color(rgb(INK_MUTED)).child(
                                                format!("{} · {}", audit.operation, audit.summary),
                                            ),
                                        ),
                                    )
                                    .child(audit_status_label(audit.status))
                            })),
                    ),
            )
            .into_any_element()
    }

    fn render_settings(
        &self,
        available_update: Option<AvailableUpdate>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        div()
            .flex_1()
            .min_h_0()
            .overflow_y_scrollbar()
            .bg(rgb(CANVAS))
            .child(
                div()
                    .w_full()
                    .max_w(px(880.0))
                    .mx_auto()
                    .p_6()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(page_heading(
                        "设置",
                        "管理端到端加密同步、系统凭据与原生应用更新。",
                    ))
                    .child(
                        settings_card("端到端加密同步")
                            .child(div().text_sm().text_color(rgb(INK_MUTED)).child(
                                "服务端只保存 AES-GCM 密文；同步码与 Token 位于系统安全存储。",
                            ))
                            .child(field_label("服务端基址"))
                            .child(Input::new(&self.sync_server_input).w_full())
                            .child(field_label(if self.sync_code_configured {
                                "同步码 · 已配置"
                            } else {
                                "同步码 · 未配置"
                            }))
                            .child(Input::new(&self.sync_code_input).w_full().mask_toggle())
                            .child(field_label(if self.sync_token_configured {
                                "登录 Token · 已配置"
                            } else {
                                "登录 Token · 未配置"
                            }))
                            .child(Input::new(&self.sync_token_input).w_full().mask_toggle())
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(
                                        Button::new("save-sync-settings")
                                            .label("安全保存")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.save_sync_settings(window, cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("sync-notes-now")
                                            .label(if self.sync_in_progress {
                                                "同步中…"
                                            } else {
                                                "立即同步"
                                            })
                                            .primary()
                                            .disabled(self.sync_in_progress)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.start_notes_sync(window, cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("clear-sync-secrets")
                                            .label("清除凭据")
                                            .danger()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.clear_sync_credentials(cx);
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        settings_card("原生版本与更新")
                            .child(div().text_sm().text_color(rgb(INK_MUTED)).child(format!(
                                "当前版本 {}。更新清单与安装器均执行签名和哈希校验。",
                                env!("CARGO_PKG_VERSION")
                            )))
                            .child(
                                Button::new("check-native-update")
                                    .label(if self.update_in_progress {
                                        "检查中…"
                                    } else {
                                        "检查签名更新"
                                    })
                                    .disabled(self.update_in_progress)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.start_update_check(cx);
                                    })),
                            )
                            .when(available_update.is_some(), |updates| {
                                let update = available_update
                                    .as_ref()
                                    .expect("available update was checked")
                                    .clone();
                                let notes = update.notes.chars().take(600).collect::<String>();
                                updates
                                    .child(
                                        div()
                                            .p_3()
                                            .rounded_md()
                                            .border_1()
                                            .border_color(rgb(BORDER))
                                            .bg(rgb(SURFACE))
                                            .child(format!(
                                                "可用版本 {} · {}\n{}",
                                                update.version, update.artifact.kind, notes
                                            )),
                                    )
                                    .child(
                                        Button::new("download-native-update")
                                            .label(format!("下载并打开 {} 安装器", update.version))
                                            .primary()
                                            .disabled(self.update_in_progress)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.start_update_download(cx);
                                            })),
                                    )
                            })
                            .child(
                                div().text_sm().text_color(rgb(INK_MUTED)).child(
                                    self.update_status
                                        .clone()
                                        .unwrap_or_else(|| "尚未检查更新".into()),
                                ),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_editor(&self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let is_body_editable = matches!(
            self.editor.selected_kind,
            Some(NodeKind::Note | NodeKind::Bookmark | NodeKind::File)
        );
        let editor_mode = self.editor.mode();
        let show_text = editor_mode == EditorMode::Text;
        let show_preview = editor_mode == EditorMode::Preview;
        let engine_capabilities = engine_runtime_capabilities(EnginePlatform::current_desktop());
        let delegated_capability = self
            .editor
            .active_engine_id
            .as_deref()
            .and_then(|engine_id| {
                engine_capabilities.iter().copied().find(|capability| {
                    capability.engine_id == engine_id
                        && matches!(
                            capability.kind,
                            EngineRuntimeKind::SystemExternal | EngineRuntimeKind::MetadataOnly
                        )
                })
            });
        let show_delegated_engine = delegated_capability.is_some();
        let note_text = self.editor.body_input.read(cx).value().to_string();
        let preview = TextView::markdown("active-file-preview", note_text.clone(), window, cx)
            .selectable(true)
            .scrollable(true);
        let note_toolbar_visible =
            self.note_editor_available() && !show_text && !show_preview && !show_delegated_engine;
        let slash_commands = (note_toolbar_visible && self.note_editor_focused(window, cx))
            .then(|| {
                let cursor = self.editor.body_input.read(cx).cursor_position();
                let cursor = editor::byte_offset_for_position(&note_text, cursor);
                note_slash_query(&note_text, cursor).map(note_slash_commands)
            })
            .flatten()
            .unwrap_or_default();
        let find_match_count = self.editor.find_match_count(cx);

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(rgb(EDITOR))
            .child(
                div()
                    .h(px(42.0))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(SURFACE))
                    .child(div().text_xs().text_color(rgb(INK_FAINT)).child("项目"))
                    .child(div().text_color(rgb(INK_FAINT)).child("›"))
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_sm()
                            .child(self.editor.title_input.read(cx).value().to_string()),
                    )
                    .when(
                        self.editor.selected_kind == Some(NodeKind::File)
                            && self.editor.body_editable,
                        |toolbar| {
                            let target_engine = if show_preview {
                                "ideall.text"
                            } else {
                                "ideall.preview"
                            };
                            toolbar.child(
                                Button::new("toggle-file-view")
                                    .label(if show_preview { "文本" } else { "预览" })
                                    .small()
                                    .ghost()
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.set_engine_preference(target_engine, window, cx);
                                    })),
                            )
                        },
                    )
                    .when(
                        self.editor.selected_kind == Some(NodeKind::Bookmark),
                        |toolbar| {
                            toolbar.child(
                                Button::new("engine-browser")
                                    .label("浏览器")
                                    .small()
                                    .ghost()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.set_engine_preference("ideall.browser", window, cx);
                                    })),
                            )
                        },
                    )
                    .child(
                        Button::new("editor-find")
                            .label("⌕")
                            .tooltip("在当前文档中查找 · Ctrl/⌘ F")
                            .small()
                            .ghost()
                            .selected(self.editor.find_visible)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.show_editor_find(false, window, cx);
                            })),
                    )
                    .when(self.editor.dirty, |toolbar| {
                        toolbar.child(
                            Button::new("editor-save")
                                .label("保存")
                                .small()
                                .primary()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.save(window, cx);
                                })),
                        )
                    })
                    .when(!self.editor.dirty, |toolbar| {
                        toolbar.child(
                            div()
                                .px_2()
                                .text_xs()
                                .text_color(rgb(INK_FAINT))
                                .child("已保存"),
                        )
                    })
                    .when(
                        !self.show_trash && self.editor.selected_kind == Some(NodeKind::File),
                        |toolbar| {
                            toolbar.child(
                                Button::new("open-file-system")
                                    .label("打开")
                                    .tooltip("使用系统应用打开只读副本")
                                    .small()
                                    .ghost()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.open_file_with_system(cx);
                                    })),
                            )
                        },
                    )
                    .when(
                        !self.show_trash && self.editor.selected_kind == Some(NodeKind::Bookmark),
                        |toolbar| {
                            toolbar.child(
                                Button::new("open-bookmark")
                                    .label("打开")
                                    .tooltip("在系统浏览器中打开")
                                    .small()
                                    .ghost()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_bookmark_external(window, cx);
                                    })),
                            )
                        },
                    ),
            )
            .child(
                div()
                    .h(px(if note_toolbar_visible { 84.0 } else { 50.0 }))
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(EDITOR))
                    .child(
                        div()
                            .w_full()
                            .when(note_toolbar_visible, |title| {
                                title
                                    .max_w(px(760.0))
                                    .mx_auto()
                                    .px_6()
                                    .flex()
                                    .flex_col()
                                    .justify_center()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(px(10.0))
                                            .text_color(rgb(INK_FAINT))
                                            .child("NOTE"),
                                    )
                            })
                            .when(!note_toolbar_visible, |title| title.px_4())
                            .child(
                                Input::new(&self.editor.title_input)
                                    .w_full()
                                    .h(px(if note_toolbar_visible { 48.0 } else { 36.0 }))
                                    .when(note_toolbar_visible, |input| input.large())
                                    .appearance(false)
                                    .bordered(false)
                                    .focus_bordered(false),
                            ),
                    ),
            )
            .when(note_toolbar_visible, |editor| {
                editor.child(self.render_note_toolbar(cx))
            })
            .when(self.editor.find_visible, |editor| {
                editor.child(
                    div()
                        .min_h(px(48.0))
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_4()
                        .py_2()
                        .border_b_1()
                        .border_color(rgb(BORDER))
                        .bg(rgb(SURFACE_MUTED))
                        .child(
                            Input::new(&self.editor.find_input)
                                .w(px(260.0))
                                .small()
                                .cleanable(true),
                        )
                        .when(self.editor.replace_visible, |bar| {
                            bar.child(
                                Input::new(&self.editor.replace_input)
                                    .w(px(220.0))
                                    .small()
                                    .cleanable(true),
                            )
                            .child(
                                Button::new("editor-replace-all")
                                    .label("全部替换")
                                    .small()
                                    .disabled(!self.editor.body_editable || self.show_trash)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.replace_all_in_editor(window, cx);
                                    })),
                            )
                        })
                        .child(
                            Button::new("editor-find-next")
                                .label("下一个")
                                .small()
                                .primary()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.find_next_in_editor(window, cx);
                                })),
                        )
                        .child(
                            Button::new("editor-toggle-replace")
                                .label(if self.editor.replace_visible {
                                    "隐藏替换"
                                } else {
                                    "替换"
                                })
                                .small()
                                .ghost()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    let show_replace = !this.editor.replace_visible;
                                    this.show_editor_find(show_replace, window, cx);
                                })),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(INK_FAINT))
                                .child(format!("{find_match_count} 个匹配")),
                        )
                        .child(div().flex_1())
                        .child(
                            Button::new("editor-close-find")
                                .label("×")
                                .small()
                                .ghost()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.hide_editor_find(cx);
                                })),
                        ),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .relative()
                    .bg(rgb(EDITOR))
                    .when(show_text, |body| {
                        body.child(
                            Input::new(&self.editor.text_input)
                                .h_full()
                                .appearance(false)
                                .bordered(false)
                                .focus_bordered(false)
                                .disabled(!self.editor.body_editable),
                        )
                    })
                    .when(show_preview, |body| {
                        body.child(
                            div()
                                .flex_1()
                                .min_h_0()
                                .flex()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .p_5()
                                        .border_r_1()
                                        .border_color(rgb(BORDER))
                                        .overflow_y_scrollbar()
                                        .child(preview),
                                )
                                .child(
                                    div().w_1_2().min_w_0().bg(rgb(EDITOR_GUTTER)).child(
                                        Input::new(&self.editor.body_input)
                                            .h_full()
                                            .appearance(false)
                                            .bordered(false)
                                            .focus_bordered(false)
                                            .disabled(!self.editor.body_editable),
                                    ),
                                ),
                        )
                    })
                    .when(show_delegated_engine, |body| {
                        let capability =
                            delegated_capability.expect("delegated capability is present");
                        body.child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .gap_3()
                                .p_6()
                                .child(div().text_xl().child(format!(
                                    "{} · {}",
                                    capability.label,
                                    capability.kind.label()
                                )))
                                .child(
                                    div()
                                        .max_w(px(560.0))
                                        .text_center()
                                        .text_color(rgb(INK_MUTED))
                                        .child(capability.detail),
                                )
                                .child(
                                    Button::new("open-delegated-file")
                                        .label("用系统应用打开只读副本")
                                        .primary()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.open_file_with_system(cx);
                                        })),
                                ),
                        )
                    })
                    .when(
                        !show_text
                            && !show_preview
                            && !show_delegated_engine
                            && self.editor.selected_kind == Some(NodeKind::Note),
                        |body| {
                            body.child(
                                div()
                                    .flex_1()
                                    .min_h_0()
                                    .w_full()
                                    .max_w(px(760.0))
                                    .mx_auto()
                                    .px_6()
                                    .py_5()
                                    .child(
                                        Input::new(&self.editor.body_input)
                                            .h_full()
                                            .appearance(false)
                                            .bordered(false)
                                            .focus_bordered(false)
                                            .disabled(
                                                !is_body_editable || !self.editor.body_editable,
                                            ),
                                    ),
                            )
                        },
                    )
                    .when(
                        !show_text
                            && !show_preview
                            && !show_delegated_engine
                            && self.editor.selected_kind != Some(NodeKind::Note),
                        |body| {
                            body.child(
                                Input::new(&self.editor.body_input)
                                    .h_full()
                                    .appearance(false)
                                    .bordered(false)
                                    .focus_bordered(false)
                                    .disabled(!is_body_editable || !self.editor.body_editable),
                            )
                        },
                    )
                    .when(!slash_commands.is_empty(), |body| {
                        body.child(self.render_note_slash_menu(slash_commands, cx))
                    }),
            )
            .when(self.show_trash, |editor| {
                editor.child(
                    div()
                        .h(px(42.0))
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_4()
                        .border_t_1()
                        .border_color(rgb(BORDER))
                        .bg(rgb(SURFACE_MUTED))
                        .text_sm()
                        .text_color(rgb(INK_MUTED))
                        .child("此项目位于回收站")
                        .child(div().flex_1())
                        .child(
                            Button::new("restore")
                                .label("恢复")
                                .small()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.restore_selected(window, cx);
                                })),
                        )
                        .child(
                            Button::new("purge")
                                .label("永久删除")
                                .small()
                                .danger()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.confirm_purge(window, cx);
                                })),
                        ),
                )
            })
            .into_any_element()
    }
}

impl Render for IdeallDesktop {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs = self.editor.workspace.tabs.clone();
        let active_tab = self.editor.workspace.active_id.clone();
        let has_selection = self.editor.has_selection();
        let show_settings = !has_selection && self.active_section.as_ref() == "设置";
        let show_apps = !has_selection && self.active_section.as_ref() == "应用";
        let active_browser_surface = browser_surface(
            self.editor.active_engine_id.as_deref(),
            self.editor.selected_kind,
        );
        let show_browser_surface = active_browser_surface.is_some();
        let engine_capabilities =
            engine_runtime_capabilities(EnginePlatform::current_desktop()).to_vec();
        let agent_threads = self.agent_threads.clone();
        let agent_transcript = self.agent_transcript.clone();
        let agent_audits = self.agent_audits.clone();
        let available_update = self.available_update.clone();
        let workspace_status = self.status.clone().unwrap_or_else(|| {
            if self.editor.dirty {
                "正在保存更改…".into()
            } else {
                "更改已保存在本机".into()
            }
        });
        let resize_state = self
            .explorer
            .resize_state
            .as_ref()
            .expect("explorer resize state is initialized")
            .clone();
        let editor_area = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .ml(px(8.0))
            .rounded_lg()
            .border_1()
            .border_color(rgb(BORDER))
            .bg(rgb(SURFACE))
            .overflow_hidden()
            .child(self.render_tab_bar(tabs, active_tab, cx))
            .when(
                !has_selection && !show_settings && !show_apps && !show_browser_surface,
                |content| content.child(self.render_welcome(cx)),
            )
            .when(show_browser_surface, |content| {
                content.child(self.render_browser(
                    active_browser_surface.expect("browser surface is present when rendered"),
                    cx,
                ))
            })
            .when(show_apps, |content| {
                content.child(self.render_apps(
                    engine_capabilities,
                    agent_threads,
                    agent_transcript,
                    agent_audits,
                    cx,
                ))
            })
            .when(show_settings, |content| {
                content.child(self.render_settings(available_update, cx))
            })
            .when(has_selection && !show_browser_surface, |content| {
                content.child(self.render_editor(window, cx))
            });

        div()
            .id("ideall-root")
            .key_context("Ideall")
            .on_action(cx.listener(Self::focus_search))
            .on_action(cx.listener(Self::create_note_from_shortcut))
            .on_action(cx.listener(Self::save_from_shortcut))
            .on_action(cx.listener(Self::close_active_tab))
            .on_action(cx.listener(Self::activate_next_tab))
            .on_action(cx.listener(Self::activate_previous_tab))
            .on_action(cx.listener(Self::toggle_project_panel_from_shortcut))
            .on_action(cx.listener(Self::select_recent_section_from_shortcut))
            .on_action(cx.listener(Self::select_browser_section_from_shortcut))
            .on_action(cx.listener(Self::select_apps_section_from_shortcut))
            .on_action(cx.listener(Self::select_settings_section_from_shortcut))
            .on_action(cx.listener(Self::create_text_file_from_shortcut))
            .on_action(cx.listener(Self::create_folder_from_tree))
            .on_action(cx.listener(Self::import_files_from_tree))
            .on_action(cx.listener(Self::find_in_editor))
            .on_action(cx.listener(Self::replace_in_editor))
            .on_action(cx.listener(Self::open_selected_node))
            .on_action(cx.listener(Self::rename_selected_node))
            .on_action(cx.listener(Self::trash_selected_node))
            .on_action(cx.listener(Self::select_previous_node))
            .on_action(cx.listener(Self::select_next_node))
            .on_action(cx.listener(Self::format_note_bold_from_shortcut))
            .on_action(cx.listener(Self::format_note_italic_from_shortcut))
            .on_action(cx.listener(Self::format_note_strikethrough_from_shortcut))
            .on_action(cx.listener(Self::format_note_code_from_shortcut))
            .on_action(cx.listener(Self::move_note_block_up_from_shortcut))
            .on_action(cx.listener(Self::move_note_block_down_from_shortcut))
            .on_action(cx.listener(Self::duplicate_note_block_from_shortcut))
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(CANVAS))
            .text_color(rgb(INK))
            .child(self.render_top_bar(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .gap(px(8.0))
                    .px(px(8.0))
                    .pt(px(7.0))
                    .pb(px(7.0))
                    .child(self.render_activity_bar(cx))
                    .child(
                        div().flex_1().min_w_0().h_full().child(
                            h_resizable("ideall-workbench-panels")
                                .with_state(&resize_state)
                                .child(
                                    resizable_panel()
                                        .visible(self.explorer.panel_visible)
                                        .size(px(self.explorer.panel_width as f32))
                                        .size_range(px(220.0)..px(520.0))
                                        .child(self.render_tool_window(cx)),
                                )
                                .child(resizable_panel().child(editor_area)),
                        ),
                    ),
            )
            .child(
                div()
                    .h(px(22.0))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px(px(12.0))
                    .bg(rgb(CANVAS))
                    .text_xs()
                    .text_color(rgb(INK_MUTED))
                    .child(div().w(px(6.0)).h(px(6.0)).rounded_full().bg(rgb(ACCENT)))
                    .child(workspace_status)
                    .child(div().flex_1())
                    .child(format!("{} 项", self.explorer.items.len()))
                    .child(format!("v{}", env!("CARGO_PKG_VERSION"))),
            )
    }
}

fn activity_action_content(icon: &'static str, label: &'static str) -> gpui::Div {
    div()
        .size_full()
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .text_sm()
        .child(div().w(px(18.0)).text_center().child(icon))
        .child(label)
}

fn welcome_action(
    id: &'static str,
    icon: &'static str,
    label: &'static str,
    shortcut: &'static str,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .h(px(54.0))
        .flex()
        .items_center()
        .gap_3()
        .px_3()
        .rounded_md()
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(SURFACE))
        .cursor_pointer()
        .hover(|item| item.bg(rgb(HOVER)).border_color(rgb(BORDER_STRONG)))
        .child(
            div()
                .w(px(26.0))
                .h(px(26.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .bg(rgb(ACCENT_SOFT))
                .text_color(rgb(ACCENT))
                .child(icon),
        )
        .child(div().flex_1().child(label))
        .child(div().text_xs().text_color(rgb(INK_FAINT)).child(shortcut))
}

fn settings_card(title: &'static str) -> gpui::Div {
    div()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .rounded_lg()
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(SURFACE))
        .child(div().text_sm().text_color(rgb(INK)).child(title))
}

fn page_heading(title: &'static str, subtitle: &'static str) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .pb_2()
        .child(div().text_xl().child(title))
        .child(div().text_sm().text_color(rgb(INK_MUTED)).child(subtitle))
}

fn field_label(label: &'static str) -> gpui::Div {
    div().text_sm().text_color(rgb(INK_MUTED)).child(label)
}
