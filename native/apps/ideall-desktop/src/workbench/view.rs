use super::actions::{
    ImportFiles, NewBookmark, NewFolder, NewNote, NewTextFile, OpenSelectedNode,
    RenameSelectedNode, TrashSelectedNode,
};
use crate::*;

impl IdeallDesktop {
    fn render_top_bar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let local_status = self.status.clone().unwrap_or_else(|| {
            if self.editor.dirty {
                "正在保存".into()
            } else {
                "本地已保存".into()
            }
        });

        div()
            .h(px(48.0))
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .when(cfg!(target_os = "macos"), |bar| bar.pl(px(82.0)))
            .border_b_1()
            .border_color(rgb(BORDER))
            .bg(rgb(SURFACE))
            .child(
                div()
                    .w(px(300.0))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Button::new("toggle-project-panel")
                            .w(px(30.0))
                            .h(px(30.0))
                            .p_0()
                            .icon(IconName::PanelLeft)
                            .small()
                            .ghost()
                            .tooltip("显示或隐藏资料库 · Alt 1")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_project_panel(cx);
                            })),
                    )
                    .child(
                        div()
                            .text_base()
                            .text_color(rgb(INK))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("ideall"),
                    )
                    .child(
                        div()
                            .pl_2()
                            .border_l_1()
                            .border_color(rgb(BORDER))
                            .text_size(px(10.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(INK_FAINT))
                            .child("LOCAL WORKSPACE"),
                    ),
            )
            .child(
                div().flex_1().flex().justify_center().child(
                    div()
                        .id("global-search-pill")
                        .w(px(340.0))
                        .h(px(32.0))
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .rounded_full()
                        .border_1()
                        .border_color(rgb(BORDER))
                        .bg(rgb(SURFACE_MUTED))
                        .cursor_pointer()
                        .hover(|pill| pill.border_color(rgb(BORDER_STRONG)))
                        .on_click(cx.listener(|this, _, window, cx| {
                            if !this.explorer.panel_visible {
                                this.explorer.panel_visible = true;
                                this.persist_workspace();
                            }
                            this.search_input
                                .update(cx, |input, cx| input.focus(window, cx));
                            cx.notify();
                        }))
                        .child(
                            Icon::new(IconName::Search)
                                .size(px(14.0))
                                .text_color(rgb(INK_FAINT)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_sm()
                                .text_color(rgb(INK_FAINT))
                                .child("搜索全部内容"),
                        )
                        .child(
                            div()
                                .px(px(6.0))
                                .py(px(1.0))
                                .rounded(px(5.0))
                                .border_1()
                                .border_color(rgb(BORDER))
                                .bg(rgb(SURFACE))
                                .text_size(px(11.0))
                                .text_color(rgb(INK_FAINT))
                                .child("⌘ K"),
                        ),
                ),
            )
            .child(
                div()
                    .w(px(300.0))
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_2()
                    .overflow_hidden()
                    .child(div().w(px(8.0)).h(px(8.0)).rounded_full().bg(rgb(ACCENT)))
                    .child(
                        div()
                            .max_w(px(220.0))
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_xs()
                            .text_color(rgb(INK_MUTED))
                            .child(local_status),
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
            || (section == "我的"
                && matches!(self.active_section.as_ref(), "搜索" | "回收站" | "收集箱"));
        let icon = match section {
            "我的" => IconName::LayoutDashboard,
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
            .rounded_lg()
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
                                .left(px(-10.0))
                                .top(px(7.0))
                                .w(px(2.0))
                                .h(px(22.0))
                                .rounded_full()
                                .bg(rgb(ACCENT_ON_RAIL)),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_activity_bar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .w(px(56.0))
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .px(px(10.0))
            .py(px(8.0))
            .border_r_1()
            .border_color(rgb(BORDER))
            .bg(rgb(RAIL))
            .child(
                div()
                    .w(px(32.0))
                    .h(px(32.0))
                    .mb_3()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(10.0))
                    .bg(rgb(ACCENT_SOFT))
                    .child(
                        Icon::new(IdeallIcon::Brand)
                            .size(px(22.0))
                            .text_color(rgb(ACCENT)),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.0))
                    .children(
                        NAVIGATION[..4]
                            .iter()
                            .copied()
                            .map(|navigation| self.render_activity_item(navigation, cx)),
                    ),
            )
            .child(div().flex_1())
            .child(div().w(px(22.0)).h(px(1.0)).my(px(7.0)).bg(rgb(BORDER)))
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.0))
                    .children(
                        NAVIGATION[4..]
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
        let open_node_ids = self
            .editor
            .workspace
            .tabs
            .iter()
            .map(|tab| tab.file.file_id.as_str())
            .collect::<std::collections::HashSet<_>>();
        let can_create = !self.show_trash && self.active_section.as_ref() == "我的";
        let show_browse = self.active_section.as_ref() == "浏览";
        let show_context = !matches!(self.active_section.as_ref(), "应用" | "设置");
        let show_private_root = matches!(self.active_section.as_ref(), "回收站" | "收集箱");
        let show_inbox_root = matches!(self.active_section.as_ref(), "我的" | "收集箱");
        let inbox_selected = self.active_section.as_ref() == "收集箱";
        let inbox_count = self.inbox_count;
        let show_trash_root = self.active_section.as_ref() == "我的";
        let empty_message: SharedString = match self.active_section.as_ref() {
            "搜索" => "未找到匹配的本地内容".into(),
            "回收站" => "回收站为空".into(),
            "收集箱" => "还没有未整理的内容，捕获的书签会先进这里".into(),
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
            "收集箱" => "收集箱",
            section => section,
        };

        div()
            .w_full()
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(rgb(BORDER))
            .bg(rgb(SURFACE_MUTED))
            .overflow_hidden()
            .child(
                div()
                    .h(px(48.0))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .child(
                        div()
                            .flex_1()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(INK))
                            .child("资料库"),
                    )
                    .child(
                        Button::new("library-create")
                            .w(px(28.0))
                            .h(px(28.0))
                            .p_0()
                            .small()
                            .ghost()
                            .icon(IconName::Plus)
                            .dropdown_menu(|menu, _, _| {
                                menu.menu("新建页面", Box::new(NewNote))
                                    .menu("新建文本文件", Box::new(NewTextFile))
                                    .menu("新建书签", Box::new(NewBookmark))
                                    .menu("新建目录", Box::new(NewFolder))
                                    .separator()
                                    .menu("导入文件", Box::new(ImportFiles))
                            }),
                    ),
            )
            .child(
                div()
                    .px_3()
                    .pb_3()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(
                        Input::new(&self.search_input)
                            .w_full()
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
                                    .text_size(px(10.0))
                                    .text_color(rgb(INK_FAINT))
                                    .child("⌘ K"),
                            )
                            .cleanable(true),
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
                                    Button::new("new-bookmark-from-browser")
                                        .w_full()
                                        .h(px(32.0))
                                        .p_0()
                                        .ghost()
                                        .child(activity_action_content(
                                            IconName::BookOpen,
                                            "新建书签",
                                        ))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.create_bookmark(window, cx);
                                        })),
                                )
                                .child(
                                    Button::new("new-feed")
                                        .w_full()
                                        .h(px(32.0))
                                        .p_0()
                                        .primary()
                                        .child(activity_action_content(IconName::Plus, "新增关注"))
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
                                        .child(activity_action_content(IconName::Globe, "资讯"))
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
                                        .child(activity_action_content(
                                            IconName::CircleUser,
                                            "社区",
                                        ))
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
                                            .text_color(rgb(INK_MUTED)),
                                    )
                                    .child("私有页面"),
                            )
                        })
                        .child(
                            div()
                                .id("tree-root-active")
                                .h(px(28.0))
                                .flex()
                                .items_center()
                                .px_2()
                                .rounded_md()
                                .text_size(px(11.0))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(rgb(INK_FAINT))
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
                                ),
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
                            let open = open_node_ids.contains(id.as_str());
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
                            let open_deleted_folder = self.show_trash && is_folder;
                            let click_id = id.clone();
                            let context_id = id.clone();
                            let folder_id = id.clone();
                            let icon_color = if selected {
                                ACCENT
                            } else {
                                match item.kind {
                                    NodeKind::Bookmark => ACCENT,
                                    NodeKind::Feed => 0xd39a27,
                                    _ => INK_MUTED,
                                }
                            };
                            div()
                                .id(SharedString::from(format!("node-{id}")))
                                .h(px(28.0))
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
                                    if open_deleted_folder {
                                        this.open_node(click_id.clone(), window, cx);
                                    } else if is_folder {
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
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .when(is_folder, |chevron| {
                                            chevron.child(
                                                Icon::new(if expanded {
                                                    IconName::ChevronDown
                                                } else {
                                                    IconName::ChevronRight
                                                })
                                                .size(px(13.0))
                                                .text_color(rgb(INK_FAINT)),
                                            )
                                        }),
                                )
                                .child(
                                    div()
                                        .w(px(18.0))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            Icon::new(node_kind_icon(item.kind, expanded))
                                                .size(px(15.0))
                                                .text_color(rgb(icon_color)),
                                        ),
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
                                .when(open && !selected && !is_folder, |row| {
                                    row.child(
                                        div().w(px(6.0)).h(px(6.0)).rounded_full().bg(rgb(ACCENT)),
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
                                                .menu("新建书签", Box::new(NewBookmark))
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
                        .when(show_inbox_root, |list| {
                            list.child(
                                div()
                                    .id("tree-root-inbox")
                                    .h(px(28.0))
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .px_2()
                                    .mt_1()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .text_sm()
                                    .text_color(rgb(INK_MUTED))
                                    .drag_over::<DraggedNode>(|style, _, _, _| {
                                        style.bg(rgb(ACCENT_SOFT))
                                    })
                                    .on_drop(cx.listener(|this, node: &DraggedNode, _, cx| {
                                        this.move_explorer_node(
                                            node.id.clone(),
                                            Some(INBOX_ROOT_ID.to_owned()),
                                            cx,
                                        );
                                    }))
                                    .when(inbox_selected, |row| {
                                        row.bg(rgb(SELECTION)).text_color(rgb(INK))
                                    })
                                    .hover(|row| row.bg(rgb(HOVER)))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.show_inbox(window, cx);
                                    }))
                                    .child(
                                        Icon::new(IconName::Inbox)
                                            .size(px(16.0))
                                            .text_color(rgb(INK_FAINT)),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .whitespace_nowrap()
                                            .child("收集箱"),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(INK_FAINT))
                                            .child(inbox_count.to_string()),
                                    ),
                            )
                        })
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
            .items_end()
            .gap_1()
            .px_2()
            .children(tabs.into_iter().map(|tab| {
                let id = tab_key(&tab);
                let close_id = id.clone();
                let selected = active_tab.as_deref() == Some(id.as_str());
                let dirty = selected && self.editor.dirty;
                div()
                    .id(SharedString::from(format!("tab-{id}")))
                    .relative()
                    .h(px(33.0))
                    .min_w(px(132.0))
                    .max_w(px(208.0))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .rounded_t(px(8.0))
                    .border_1()
                    .border_color(rgb(if selected { BORDER } else { SURFACE_MUTED }))
                    .cursor_pointer()
                    .text_sm()
                    .text_color(rgb(if selected { INK } else { INK_MUTED }))
                    .when(selected, |item| {
                        item.h(px(34.0))
                            .mb(px(-1.0))
                            .bg(rgb(SURFACE))
                            .font_weight(gpui::FontWeight::MEDIUM)
                    })
                    .when(!selected, |item| item.hover(|item| item.bg(rgb(HOVER))))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.activate_tab(id.clone(), window, cx);
                    }))
                    .child(
                        Icon::new(engine_icon(&tab.engine_id))
                            .size(px(14.0))
                            .text_color(rgb(if selected { ACCENT } else { INK_FAINT })),
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
                    .when(selected, |item| {
                        item.child(
                            div()
                                .absolute()
                                .left(px(10.0))
                                .right(px(10.0))
                                .bottom(px(0.0))
                                .h(px(2.0))
                                .rounded_full()
                                .bg(rgb(ACCENT)),
                        )
                    })
            }))
            .child(div().min_w(px(24.0)).flex_1());

        // Keep the flex row inside the scrollbar viewport. The component
        // wrapper owns the outer dimensions and otherwise resets the wrapped
        // element's layout style.
        div()
            .h(px(40.0))
            .border_b_1()
            .border_color(rgb(BORDER))
            .bg(rgb(SURFACE_MUTED))
            .child(content)
            .overflow_x_scrollbar()
            .into_any_element()
    }

    fn render_agent_panel(&self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let has_document_context = self.editor.has_selection();
        let context_title = if has_document_context {
            let title = self.editor.title_input.read(cx).value().to_string();
            if title.trim().is_empty() {
                "未命名文档".to_owned()
            } else {
                title
            }
        } else {
            "未选择文档".to_owned()
        };
        let model = self.agent_model_input.read(cx).value().to_string();
        let model = if model.trim().is_empty() {
            "Local Agent".to_owned()
        } else {
            model
        };
        let transcript_start = self.agent_transcript.len().saturating_sub(24);
        let transcript = self.agent_transcript[transcript_start..].to_vec();
        let latest_audit = self.agent_audits.first().cloned();
        let audit_count = self.agent_audits.len();
        let suggestions = if has_document_context {
            vec![
                ("总结当前文档", "请总结当前文档，并提炼三个关键结论。", true),
                ("整理下一步", "结合当前文档，整理接下来可执行的步骤。", true),
                (
                    "改进表达",
                    "请指出当前文档中表达不清楚的地方并给出修改建议。",
                    true,
                ),
            ]
        } else {
            vec![
                ("整理想法", "帮我把下面的想法整理成清晰的结构：", false),
                ("制定计划", "帮我把目标拆成一份可执行计划：", false),
                ("新建笔记", "请为我新建一条笔记，标题是：", false),
            ]
        };

        div()
            .w_full()
            .h_full()
            .min_h_0()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(rgb(BORDER))
            .bg(rgb(SURFACE_MUTED))
            .child(
                div()
                    .h(px(44.0))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(SURFACE_MUTED))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(INK))
                            .child("Ideall Agent"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_xs()
                            .text_color(rgb(ACCENT))
                            .child(div().w(px(6.0)).h(px(6.0)).rounded_full().bg(rgb(ACCENT)))
                            .child(if self.agent_in_progress {
                                "处理中"
                            } else {
                                "就绪"
                            }),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("agent-panel-new-thread")
                            .w(px(26.0))
                            .h(px(26.0))
                            .p_0()
                            .small()
                            .ghost()
                            .icon(IconName::Plus)
                            .tooltip("新建对话")
                            .disabled(self.agent_in_progress)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.new_agent_thread(cx);
                            })),
                    )
                    .child(
                        Button::new("agent-panel-history")
                            .w(px(26.0))
                            .h(px(26.0))
                            .p_0()
                            .small()
                            .ghost()
                            .icon(IconName::GalleryVerticalEnd)
                            .tooltip("对话历史与 Agent 设置")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.select_section("应用", window, cx);
                            })),
                    )
                    .child(
                        Button::new("agent-panel-collapse")
                            .w(px(26.0))
                            .h(px(26.0))
                            .p_0()
                            .small()
                            .ghost()
                            .icon(IconName::PanelRightClose)
                            .tooltip("收起 Agent")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_agent_panel(cx);
                            })),
                    ),
            )
            .child(
                div()
                    .id("agent-document-context")
                    .mx_3()
                    .mt_2()
                    .h(px(36.0))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .rounded_full()
                    .border_1()
                    .border_color(rgb(if self.agent_context_attached {
                        ACCENT_SOFT
                    } else {
                        BORDER
                    }))
                    .bg(rgb(if self.agent_context_attached {
                        ACCENT_SOFT
                    } else {
                        SURFACE_MUTED
                    }))
                    .when(has_document_context, |context| {
                        context
                            .cursor_pointer()
                            .hover(|context| context.bg(rgb(HOVER)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_agent_document_context(cx);
                            }))
                    })
                    .child(Icon::new(IconName::File).size(px(15.0)).text_color(rgb(
                        if self.agent_context_attached {
                            ACCENT
                        } else {
                            INK_FAINT
                        },
                    )))
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_xs()
                            .text_color(rgb(if self.agent_context_attached {
                                ACCENT
                            } else {
                                INK
                            }))
                            .child(context_title),
                    )
                    .child(
                        div()
                            .whitespace_nowrap()
                            .text_xs()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(rgb(if self.agent_context_attached {
                                ACCENT
                            } else {
                                INK_FAINT
                            }))
                            .child(if !has_document_context {
                                "无上下文"
                            } else if self.agent_context_attached {
                                "已附加"
                            } else {
                                "点击附加"
                            }),
                    ),
            )
            .child(
                div()
                    .id("agent-transcript-scroll")
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_3()
                    .bg(rgb(SURFACE_MUTED))
                    .track_scroll(&self.agent_scroll_handle)
                    .overflow_y_scroll()
                    .vertical_scrollbar(&self.agent_scroll_handle)
                    .when(transcript.is_empty(), |messages| {
                        messages.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_3()
                                .py_6()
                                .child(
                                    div()
                                        .w(px(38.0))
                                        .h(px(38.0))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded_lg()
                                        .bg(rgb(ACCENT_SOFT))
                                        .child(
                                            Icon::new(IdeallIcon::Brand)
                                                .size(px(20.0))
                                                .text_color(rgb(ACCENT)),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_base()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(rgb(INK))
                                        .child("从当前文档开始"),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(INK_MUTED))
                                        .child("总结内容、梳理思路，或让 Agent 帮你完成下一步。"),
                                )
                                .child(div().flex().flex_col().gap_2().children(
                                    suggestions.into_iter().enumerate().map(
                                        |(index, (label, prompt, attach_document))| {
                                            Button::new(SharedString::from(format!(
                                                "agent-suggestion-{index}"
                                            )))
                                            .w_full()
                                            .h(px(38.0))
                                            .px_3()
                                            .small()
                                            .ghost()
                                            .rounded_md()
                                            .border_1()
                                            .border_color(rgb(BORDER))
                                            .bg(rgb(SURFACE))
                                            .child(
                                                div()
                                                    .w_full()
                                                    .flex()
                                                    .items_center()
                                                    .gap_2()
                                                    .child(div().flex_1().text_left().child(label))
                                                    .child(
                                                        Icon::new(IconName::ChevronRight)
                                                            .size(px(13.0))
                                                            .text_color(rgb(INK_FAINT)),
                                                    ),
                                            )
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.fill_agent_prompt(
                                                    prompt,
                                                    attach_document,
                                                    window,
                                                    cx,
                                                );
                                            }))
                                        },
                                    ),
                                )),
                        )
                    })
                    .children(transcript.into_iter().enumerate().map(|(index, message)| {
                        let is_assistant = message.role == ModelRole::Assistant;
                        let is_tool = message.role == ModelRole::Tool;
                        let background = match message.role {
                            ModelRole::User => SURFACE,
                            ModelRole::Assistant => SURFACE_MUTED,
                            ModelRole::Tool => MESSAGE_TOOL,
                            ModelRole::System => WARNING_SOFT,
                        };
                        let content = message.content.clone();
                        let markdown = is_assistant.then(|| {
                            TextView::markdown(
                                SharedString::from(format!("agent-message-{index}")),
                                content.clone(),
                                window,
                                cx,
                            )
                            .selectable(true)
                        });

                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .p_3()
                            .when(is_tool, |message| message.rounded_lg())
                            .when(!is_tool, |message| message.rounded_xl())
                            .border_1()
                            .border_color(rgb(match message.role {
                                ModelRole::User | ModelRole::System => BORDER,
                                ModelRole::Assistant => SURFACE_MUTED,
                                ModelRole::Tool => MESSAGE_TOOL,
                            }))
                            .bg(rgb(background))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .text_size(px(11.0))
                                    .text_color(rgb(INK_FAINT))
                                    .when(is_tool, |label| {
                                        label.child(
                                            Icon::new(IdeallIcon::Wrench)
                                                .size(px(11.0))
                                                .text_color(rgb(INK_MUTED)),
                                        )
                                    })
                                    .child(agent_role_label(message.role)),
                            )
                            .when_some(markdown, |message, markdown| {
                                message.child(div().text_sm().text_color(rgb(INK)).child(markdown))
                            })
                            .when(!is_assistant, |message| {
                                message.child(div().text_sm().text_color(rgb(INK)).child(content))
                            })
                    }))
                    .when_some(latest_audit, |messages, audit| {
                        messages.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .px_3()
                                .py_2()
                                .rounded_xl()
                                .border_1()
                                .border_color(rgb(BORDER))
                                .bg(rgb(SURFACE))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            Icon::new(IdeallIcon::ShieldCheck)
                                                .size(px(14.0))
                                                .text_color(rgb(ACCENT)),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                                .text_color(rgb(INK))
                                                .child("写操作审计"),
                                        )
                                        .child(div().flex_1())
                                        .child(
                                            div()
                                                .px_2()
                                                .py(px(1.0))
                                                .rounded_full()
                                                .bg(rgb(MESSAGE_TOOL))
                                                .text_size(px(11.0))
                                                .text_color(rgb(INK_MUTED))
                                                .child(format!("已记录 {} 次", audit_count)),
                                        ),
                                )
                                .child(
                                    div()
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .text_size(px(11.0))
                                        .text_color(rgb(INK_MUTED))
                                        .child(format!(
                                            "最近：{} · {}",
                                            audit.title,
                                            audit_status_label(audit.status)
                                        )),
                                ),
                        )
                    })
                    .child(
                        div()
                            .pt_1()
                            .text_xs()
                            .text_color(rgb(INK_FAINT))
                            .child("写操作会在执行前确认，并记录到本地审计。"),
                    ),
            )
            .when(!self.agent_key_configured, |panel| {
                panel.child(
                    div()
                        .mx_3()
                        .mb_2()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(rgb(ACCENT_SOFT))
                        .child(
                            div()
                                .flex_1()
                                .text_xs()
                                .text_color(rgb(INK_MUTED))
                                .child("配置模型凭据后即可开始对话"),
                        )
                        .child(
                            Button::new("agent-panel-open-settings")
                                .label("去配置")
                                .small()
                                .ghost()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.select_section("应用", window, cx);
                                })),
                        ),
                )
            })
            .child(
                div()
                    .p_3()
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(SURFACE_MUTED))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .p_2()
                            .rounded_xl()
                            .border_1()
                            .border_color(rgb(BORDER))
                            .bg(rgb(SURFACE))
                            .child(
                                Input::new(&self.agent_prompt_input)
                                    .w_full()
                                    .appearance(false),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex_1()
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .whitespace_nowrap()
                                            .text_size(px(11.0))
                                            .text_color(rgb(INK_FAINT))
                                            .child(format!("{model} · Ctrl/⌘ Enter 发送")),
                                    )
                                    .child(
                                        Button::new("agent-panel-send")
                                            .w(px(28.0))
                                            .h(px(28.0))
                                            .p_0()
                                            .rounded_full()
                                            .icon(IconName::ArrowUp)
                                            .primary()
                                            .tooltip(if self.agent_key_configured {
                                                "发送给 Agent · Ctrl/⌘ Enter"
                                            } else {
                                                "请先配置模型 API Key"
                                            })
                                            .disabled(
                                                self.agent_in_progress
                                                    || !self.agent_key_configured,
                                            )
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.start_agent_turn(window, cx);
                                            })),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_collapsed_agent_panel(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .w(px(42.0))
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .pt_2()
            .border_l_1()
            .border_color(rgb(BORDER))
            .bg(rgb(SURFACE_MUTED))
            .child(
                Button::new("agent-panel-expand")
                    .w(px(28.0))
                    .h(px(28.0))
                    .p_0()
                    .small()
                    .ghost()
                    .icon(IconName::PanelRightOpen)
                    .tooltip("展开 Ideall Agent")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_agent_panel(cx);
                    })),
            )
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
                                    .border_1()
                                    .border_color(rgb(BORDER))
                                    .bg(rgb(ACCENT_SOFT))
                                    .child(
                                        Icon::new(IdeallIcon::Brand)
                                            .size(px(28.0))
                                            .text_color(rgb(ACCENT)),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_xl()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .child("开始创作"),
                                    )
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
                                welcome_action(
                                    "welcome-new-file",
                                    IconName::File,
                                    "新建文件",
                                    "Ctrl ⇧ N",
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.create_text_file(window, cx);
                                    },
                                )),
                            )
                            .child(
                                welcome_action(
                                    "welcome-new-note",
                                    IconName::BookOpen,
                                    "新建笔记",
                                    "Ctrl N",
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.create_note(window, cx);
                                    },
                                )),
                            )
                            .child(
                                welcome_action(
                                    "welcome-import",
                                    IconName::FolderOpen,
                                    "导入本地文件",
                                    "",
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.import_files(cx);
                                    },
                                )),
                            )
                            .child(
                                welcome_action(
                                    "welcome-agent",
                                    IconName::Bot,
                                    "开始 Agent 对话",
                                    "",
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.focus_agent_composer(window, cx);
                                    },
                                )),
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

    fn render_collection_landing(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let section = self.active_section.to_string();
        let items = self.explorer.items.clone();
        let (title, description, icon, empty) = match section.as_str() {
            "活动" => (
                "最近更新",
                "按最后修改时间排列的本地内容",
                IconName::Calendar,
                "还没有最近修改的内容",
            ),
            "浏览" => (
                "关注与书签",
                "集中管理本地书签、关注源和浏览入口",
                IconName::Globe,
                "还没有书签或关注源",
            ),
            "搜索" => (
                "搜索结果",
                "匹配标题与笔记正文的本地内容",
                IconName::Search,
                "没有找到匹配内容",
            ),
            "回收站" => (
                "回收站",
                "打开项目后可恢复或永久删除",
                IconName::Delete,
                "回收站为空",
            ),
            "收集箱" => (
                "收集箱",
                "捕获与未整理的内容先进这里，稍后再归类",
                IconName::Inbox,
                "还没有未整理的内容，捕获的书签会先进这里",
            ),
            _ => (
                "本地工作区",
                "所有内容都保存在当前设备",
                IconName::LayoutDashboard,
                "这里还没有内容",
            ),
        };

        div()
            .flex_1()
            .min_h_0()
            .overflow_y_scrollbar()
            .bg(rgb(SURFACE))
            .child(
                div()
                    .w_full()
                    .max_w(px(760.0))
                    .mx_auto()
                    .px_8()
                    .py_8()
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .gap_3()
                            .child(
                                div()
                                    .w(px(38.0))
                                    .h(px(38.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_lg()
                                    .bg(rgb(ACCENT_SOFT))
                                    .child(Icon::new(icon).size(px(19.0)).text_color(rgb(ACCENT))),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_xl()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .child(title),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(rgb(INK_MUTED))
                                            .child(description),
                                    ),
                            )
                            .when(section == "浏览", |heading| {
                                heading.child(
                                    Button::new("collection-new-bookmark")
                                        .label("新建书签")
                                        .icon(IconName::Plus)
                                        .small()
                                        .primary()
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.create_bookmark(window, cx);
                                        })),
                                )
                            }),
                    )
                    .when(section == "搜索" && !items.is_empty(), |content| {
                        let current = self.search_kind;
                        content.child(div().flex().flex_wrap().gap_2().children(
                            SEARCH_KIND_CHIPS.iter().map(|(key, kind, label)| {
                                let active = current == *kind;
                                let kind = *kind;
                                div()
                                    .id(SharedString::from(format!("search-kind-{key}")))
                                    .h(px(26.0))
                                    .px_3()
                                    .rounded_full()
                                    .flex()
                                    .items_center()
                                    .cursor_pointer()
                                    .text_size(px(12.0))
                                    .when(active, |chip| {
                                        chip.bg(rgb(ACCENT_SOFT)).text_color(rgb(ACCENT))
                                    })
                                    .when(!active, |chip| {
                                        chip.text_color(rgb(INK_MUTED))
                                            .hover(|style| style.bg(rgb(HOVER)))
                                    })
                                    .child(*label)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.set_search_kind(kind, cx);
                                    }))
                            }),
                        ))
                    })
                    .when(items.is_empty(), |content| {
                        content.child(
                            div()
                                .py_10()
                                .text_center()
                                .text_sm()
                                .text_color(rgb(INK_FAINT))
                                .child(empty),
                        )
                    })
                    .children(items.into_iter().take(80).map(|item| {
                        let id = item.id.clone();
                        let is_folder = item.kind == NodeKind::Folder;
                        let is_trash = section == "回收站";
                        let snippet = item.snippet.clone();
                        let show_snippet = section == "搜索" && !snippet.is_empty();
                        div()
                            .id(SharedString::from(format!("collection-item-{}", item.id)))
                            .when(!show_snippet, |row| row.h(px(48.0)))
                            .when(show_snippet, |row| row.py_2())
                            .flex()
                            .flex_col()
                            .justify_center()
                            .gap_1()
                            .px_3()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(BORDER))
                            .cursor_pointer()
                            .hover(|row| row.bg(rgb(HOVER)))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if is_folder && !is_trash {
                                    this.show_files(window, cx);
                                    this.explorer.reveal_folder(id.clone());
                                    this.explorer.select(id.clone());
                                    this.persist_workspace();
                                    cx.notify();
                                } else {
                                    this.open_node(id.clone(), window, cx);
                                }
                            }))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .child(
                                        Icon::new(node_kind_icon(item.kind, false))
                                            .size(px(17.0))
                                            .text_color(rgb(INK_MUTED)),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .whitespace_nowrap()
                                            .text_sm()
                                            .child(item.title),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(INK_FAINT))
                                            .child(node_kind_label(item.kind)),
                                    )
                                    .child(
                                        Icon::new(IconName::ChevronRight)
                                            .size(px(14.0))
                                            .text_color(rgb(INK_FAINT)),
                                    ),
                            )
                            .when(show_snippet, |row| {
                                row.child(render_search_snippet(&snippet))
                            })
                    })),
            )
            .into_any_element()
    }

    fn render_apps(
        &self,
        engine_capabilities: Vec<ideall_domain::EngineRuntimeCapability>,
        agent_threads: Vec<NodeSummary>,
        agent_audits: Vec<AgentAuditRecord>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let agent_threads_empty = agent_threads.is_empty();
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
                                    .flex_wrap()
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
                                        Button::new("clear-agent-key")
                                            .label("清除 Key")
                                            .danger()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.clear_agent_credential(cx);
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
                                    .flex_wrap()
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
                        settings_card("本地对话记录")
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_sm()
                                            .text_color(rgb(INK_MUTED))
                                            .child("选择历史对话后，可在右侧 Agent 面板继续。"),
                                    )
                                    .child(
                                        Button::new("new-agent-thread")
                                            .label("新对话")
                                            .icon(IconName::Plus)
                                            .small()
                                            .primary()
                                            .disabled(self.agent_in_progress)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.new_agent_thread(cx);
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .when(agent_threads_empty, |threads| {
                                        threads.child(
                                            div()
                                                .py_4()
                                                .text_sm()
                                                .text_color(rgb(INK_FAINT))
                                                .child("还没有本地对话记录"),
                                        )
                                    })
                                    .children(agent_threads.into_iter().map(|thread| {
                                        let id = thread.id.clone();
                                        let selected = self.agent_thread_id.as_deref()
                                            == Some(thread.id.as_str());
                                        div()
                                            .id(SharedString::from(format!(
                                                "agent-thread-{}",
                                                thread.id
                                            )))
                                            .h(px(40.0))
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .px_2()
                                            .rounded_md()
                                            .cursor_pointer()
                                            .when(selected, |row| row.bg(rgb(ACCENT_SOFT)))
                                            .hover(|row| row.bg(rgb(HOVER)))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if this.agent_in_progress {
                                                    return;
                                                }
                                                this.select_agent_thread(id.clone(), cx);
                                            }))
                                            .child(
                                                Icon::new(IconName::Bot).size(px(15.0)).text_color(
                                                    rgb(if selected { ACCENT } else { INK_MUTED }),
                                                ),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .overflow_hidden()
                                                    .text_ellipsis()
                                                    .whitespace_nowrap()
                                                    .text_sm()
                                                    .child(thread.title),
                                            )
                                            .child(
                                                Icon::new(IconName::ChevronRight)
                                                    .size(px(14.0))
                                                    .text_color(rgb(INK_FAINT)),
                                            )
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
                                    .flex_wrap()
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
        let file_text = self.editor.text_input.read(cx).value().to_string();
        let file_preview =
            TextView::markdown("active-file-preview", file_text, window, cx).selectable(true);
        let is_note_document = self.editor.selected_kind == Some(NodeKind::Note)
            && !show_text
            && !show_preview
            && !show_delegated_engine;
        let show_note_preview = is_note_document && self.editor.note_preview;
        let note_editor_focused =
            self.note_editor_available() && self.note_editor_focused(window, cx);
        let slash_commands = note_editor_focused
            .then(|| {
                let (text, cursor) = self.editor.active_note_query_context(cx)?;
                note_slash_query(&text, cursor).map(note_slash_commands)
            })
            .flatten()
            .unwrap_or_default();
        let mention_candidates = note_editor_focused
            .then(|| {
                let (_, query) = self.editor.active_note_mention_context(cx)?;
                Some(self.note_mention_candidates(&query))
            })
            .flatten()
            .unwrap_or_default();
        let find_match_count = self.editor.find_match_count(cx);
        let current_title = self.editor.title_input.read(cx).value().to_string();
        let active_text = self.editor.active_text(cx);
        let active_cursor = self.editor.active_cursor_position(cx);
        let metrics = editor::editor_metrics(&active_text, active_cursor);
        let editor_mode_label = match (
            self.editor.selected_kind,
            editor_mode,
            show_note_preview,
            show_delegated_engine,
        ) {
            (_, _, _, true) => "外部应用",
            (Some(NodeKind::Note), _, true, _) => "阅读",
            (Some(NodeKind::Note), _, false, _) => "块编辑",
            (Some(NodeKind::File), EditorMode::Preview, _, _) => "文本 + 预览",
            (Some(NodeKind::File), _, _, _) => "文本编辑",
            (Some(NodeKind::Bookmark), _, _, _) => "网址",
            _ => "只读",
        };
        let document_format = match self.editor.selected_kind {
            Some(NodeKind::Note) => "MARKDOWN".to_owned(),
            Some(NodeKind::Bookmark) => "URL".to_owned(),
            Some(NodeKind::File) => current_title
                .rsplit_once('.')
                .map(|(_, extension)| extension.to_ascii_uppercase())
                .filter(|extension| !extension.is_empty() && extension.len() <= 10)
                .unwrap_or_else(|| "TEXT".to_owned()),
            _ => "IDEALL".to_owned(),
        };
        let parent_title = self
            .editor
            .selected_id
            .as_deref()
            .and_then(|selected_id| {
                self.explorer
                    .items
                    .iter()
                    .find(|item| item.id == selected_id)
                    .and_then(|item| item.parent_id.as_deref())
            })
            .and_then(|parent_id| self.explorer.items.iter().find(|item| item.id == parent_id))
            .map(|item| item.title.clone())
            .unwrap_or_else(|| "我的资料库".to_owned());

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(rgb(EDITOR))
            .child(
                div()
                    .h(px(40.0))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_4()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(SURFACE))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(INK_FAINT))
                            .child(parent_title),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(INK_FAINT))
                            .child("/"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_size(px(11.0))
                            .text_color(rgb(INK_MUTED))
                            .child(current_title),
                    )
                    .when(
                        is_body_editable
                            && self.editor.body_editable
                            && !self.show_trash
                            && !show_note_preview
                            && !show_delegated_engine,
                        |toolbar| {
                            toolbar
                                .child(
                                    Button::new("editor-undo")
                                        .label("↶")
                                        .tooltip("撤销 · Ctrl/⌘ Z")
                                        .small()
                                        .ghost()
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.undo_current_editor(window, cx);
                                        })),
                                )
                                .child(
                                    Button::new("editor-redo")
                                        .label("↷")
                                        .tooltip("重做 · Ctrl/⌘ Shift Z / Ctrl Y")
                                        .small()
                                        .ghost()
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.redo_current_editor(window, cx);
                                        })),
                                )
                        },
                    )
                    .when(is_note_document, |toolbar| {
                        toolbar.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(2.0))
                                .p(px(2.0))
                                .rounded_full()
                                .bg(rgb(SURFACE_MUTED))
                                .child(
                                    div()
                                        .id("note-mode-read")
                                        .h(px(24.0))
                                        .flex()
                                        .items_center()
                                        .px_3()
                                        .rounded_full()
                                        .text_xs()
                                        .when(show_note_preview, |option| {
                                            option
                                                .border_1()
                                                .border_color(rgb(BORDER))
                                                .bg(rgb(SURFACE))
                                                .font_weight(gpui::FontWeight::MEDIUM)
                                                .text_color(rgb(INK))
                                        })
                                        .when(!show_note_preview, |option| {
                                            option
                                                .text_color(rgb(INK_MUTED))
                                                .cursor_pointer()
                                                .hover(|option| option.text_color(rgb(INK)))
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.toggle_note_preview(window, cx);
                                                }))
                                        })
                                        .child("阅读"),
                                )
                                .child(
                                    div()
                                        .id("note-mode-edit")
                                        .h(px(24.0))
                                        .flex()
                                        .items_center()
                                        .px_3()
                                        .rounded_full()
                                        .text_xs()
                                        .when(!show_note_preview, |option| {
                                            option
                                                .border_1()
                                                .border_color(rgb(BORDER))
                                                .bg(rgb(SURFACE))
                                                .font_weight(gpui::FontWeight::MEDIUM)
                                                .text_color(rgb(INK))
                                        })
                                        .when(show_note_preview, |option| {
                                            option
                                                .text_color(rgb(INK_MUTED))
                                                .cursor_pointer()
                                                .hover(|option| option.text_color(rgb(INK)))
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.toggle_note_preview(window, cx);
                                                }))
                                        })
                                        .child("编辑"),
                                ),
                        )
                    })
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
                                    .icon(if show_preview {
                                        IconName::EyeOff
                                    } else {
                                        IconName::Eye
                                    })
                                    .label(if show_preview {
                                        "纯文本"
                                    } else {
                                        "并排预览"
                                    })
                                    .tooltip(if show_preview {
                                        "切换到文本编辑"
                                    } else {
                                        "并排预览"
                                    })
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
                    .when(is_body_editable && self.editor.body_editable, |toolbar| {
                        toolbar.child(
                            Button::new("editor-find")
                                .icon(IconName::Search)
                                .label("查找")
                                .tooltip("在当前文档中查找 · Ctrl/⌘ F")
                                .small()
                                .ghost()
                                .selected(self.editor.find_visible)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.show_editor_find(false, window, cx);
                                })),
                        )
                    })
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
                    .when(!self.editor.dirty && !is_note_document, |toolbar| {
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
                    .h(px(if is_note_document { 108.0 } else { 50.0 }))
                    .flex()
                    .items_center()
                    .bg(rgb(EDITOR))
                    .when(!is_note_document, |title| {
                        title.border_b_1().border_color(rgb(BORDER))
                    })
                    .child(
                        div()
                            .w_full()
                            .when(is_note_document, |title| {
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
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(rgb(INK_FAINT))
                                            .child("笔记"),
                                    )
                            })
                            .when(!is_note_document, |title| title.px_4())
                            .child(
                                Input::new(&self.editor.title_input)
                                    .w_full()
                                    .h(px(if is_note_document { 46.0 } else { 36.0 }))
                                    .when(is_note_document, |input| {
                                        input
                                            .large()
                                            .text_size(px(30.0))
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                    })
                                    .appearance(false)
                                    .bordered(false)
                                    .focus_bordered(false)
                                    .disabled(self.show_trash),
                            ),
                    ),
            )
            .when(self.note_editor_available(), |editor| {
                editor.child(self.render_note_toolbar(cx))
            })
            .when(self.editor.link_editor.is_some(), |editor| {
                editor.child(self.render_note_link_editor(cx))
            })
            .when(self.editor.find_visible, |editor| {
                editor.child(
                    div()
                        .min_h(px(48.0))
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_2()
                        .px_4()
                        .py_2()
                        .border_b_1()
                        .border_color(rgb(BORDER))
                        .bg(rgb(SURFACE_MUTED))
                        .child(
                            Input::new(&self.editor.find_input)
                                .flex_1()
                                .min_w(px(180.0))
                                .max_w(px(320.0))
                                .small()
                                .cleanable(true),
                        )
                        .when(self.editor.replace_visible, |bar| {
                            bar.child(
                                Input::new(&self.editor.replace_input)
                                    .flex_1()
                                    .min_w(px(160.0))
                                    .max_w(px(280.0))
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
                                        .border_r_1()
                                        .border_color(rgb(BORDER))
                                        .bg(rgb(EDITOR))
                                        .child(
                                            Input::new(&self.editor.text_input)
                                                .h_full()
                                                .appearance(false)
                                                .bordered(false)
                                                .focus_bordered(false)
                                                .disabled(!self.editor.body_editable),
                                        ),
                                )
                                .child(
                                    div()
                                        .w_1_2()
                                        .min_w_0()
                                        .p_5()
                                        .overflow_y_scrollbar()
                                        .bg(rgb(EDITOR_GUTTER))
                                        .child(file_preview),
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
                    .when(show_note_preview, |body| {
                        body.child(self.render_note_block_editor(window, cx))
                    })
                    .when(is_note_document && !show_note_preview, |body| {
                        body.child(self.render_note_block_editor(window, cx))
                    })
                    .when(
                        !show_text && !show_preview && !show_delegated_engine && !is_note_document,
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
                    })
                    .when(!mention_candidates.is_empty(), |body| {
                        body.child(self.render_note_mention_menu(mention_candidates, cx))
                    }),
            )
            .when(!show_delegated_engine, |editor| {
                editor.child(
                    div()
                        .h(px(30.0))
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap_3()
                        .px_4()
                        .border_t_1()
                        .border_color(rgb(BORDER))
                        .bg(rgb(SURFACE_MUTED))
                        .text_size(px(11.0))
                        .text_color(rgb(INK_MUTED))
                        .child(editor_mode_label)
                        .child(
                            div()
                                .px_2()
                                .py(px(2.0))
                                .rounded_sm()
                                .bg(rgb(SURFACE))
                                .text_color(rgb(INK_MUTED))
                                .child(document_format),
                        )
                        .child(div().flex_1())
                        .when(
                            is_body_editable && !show_note_preview && !self.show_trash,
                            |status| {
                                status.child(format!(
                                    "行 {}，列 {}",
                                    metrics.cursor_line, metrics.cursor_column
                                ))
                            },
                        )
                        .child(format!("{} 行", metrics.lines))
                        .child(format!("{} 字符", metrics.characters))
                        .child("UTF-8")
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .child(
                                    div()
                                        .w(px(5.0))
                                        .h(px(5.0))
                                        .rounded_full()
                                        .bg(rgb(if self.editor.dirty { 0xd39a27 } else { ACCENT })),
                                )
                                .child(if self.editor.dirty {
                                    "保存中"
                                } else {
                                    "已保存"
                                }),
                        ),
                )
            })
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
        let show_home_welcome = !has_selection && self.active_section.as_ref() == "我的";
        let show_collection_landing = !has_selection
            && matches!(
                self.active_section.as_ref(),
                "活动" | "浏览" | "搜索" | "回收站" | "收集箱"
            );
        let active_browser_surface = browser_surface(
            self.editor.active_engine_id.as_deref(),
            self.editor.selected_kind,
        );
        let show_browser_surface = active_browser_surface.is_some();
        let engine_capabilities =
            engine_runtime_capabilities(EnginePlatform::current_desktop()).to_vec();
        let agent_threads = self.agent_threads.clone();
        let agent_audits = self.agent_audits.clone();
        let available_update = self.available_update.clone();
        let show_tab_bar = has_selection && should_show_tab_bar(tabs.len());
        let show_agent_panel = self.editor.workspace.agent.visible;
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
            .bg(rgb(SURFACE))
            .overflow_hidden()
            .when(show_tab_bar, |area| {
                area.child(self.render_tab_bar(tabs, active_tab, cx))
            })
            .when(show_home_welcome && !show_browser_surface, |content| {
                content.child(self.render_welcome(cx))
            })
            .when(
                show_collection_landing && !show_browser_surface,
                |content| content.child(self.render_collection_landing(cx)),
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
        let workspace = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .child(editor_area)
            .when(show_agent_panel, |layout| {
                layout.child(
                    div()
                        .w(px(320.0))
                        .min_w(px(288.0))
                        .h_full()
                        .flex_none()
                        .child(self.render_agent_panel(window, cx)),
                )
            })
            .when(!show_agent_panel, |layout| {
                layout.child(self.render_collapsed_agent_panel(cx))
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
            .on_action(cx.listener(Self::create_bookmark_from_shortcut))
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
            .on_action(cx.listener(Self::copy_note_blocks_from_shortcut))
            .on_action(cx.listener(Self::delete_note_blocks_from_shortcut))
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
                    .bg(rgb(SURFACE))
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
                                .child(resizable_panel().child(workspace)),
                        ),
                    ),
            )
    }
}

fn should_show_tab_bar(tab_count: usize) -> bool {
    tab_count > 1
}

fn node_kind_icon(kind: NodeKind, expanded: bool) -> IconName {
    match kind {
        NodeKind::Folder if expanded => IconName::FolderOpen,
        NodeKind::Folder => IconName::FolderClosed,
        NodeKind::Note | NodeKind::File => IconName::File,
        NodeKind::Bookmark => IconName::BookOpen,
        NodeKind::Feed => IconName::Globe,
        NodeKind::Thread => IconName::Bot,
    }
}

fn node_kind_label(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Folder => "文件夹",
        NodeKind::Note => "笔记",
        NodeKind::Bookmark => "书签",
        NodeKind::File => "文件",
        NodeKind::Feed => "关注源",
        NodeKind::Thread => "对话",
    }
}

const SEARCH_KIND_CHIPS: [(&str, Option<NodeKind>, &str); 6] = [
    ("all", None, "全部"),
    ("note", Some(NodeKind::Note), "笔记"),
    ("bookmark", Some(NodeKind::Bookmark), "书签"),
    ("file", Some(NodeKind::File), "文件"),
    ("feed", Some(NodeKind::Feed), "关注源"),
    ("thread", Some(NodeKind::Thread), "对话"),
];

/// 把 FTS 摘要按 « » 切分为 spans：标记内文本用 ACCENT 加粗，不显示标记原文。
/// 正文中的换行等连续空白先折叠为单空格，保证结果行严格单行省略。
fn render_search_snippet(snippet: &str) -> gpui::AnyElement {
    let snippet = snippet.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut spans = Vec::new();
    let mut rest = snippet.as_str();
    while let Some(start) = rest.find('«') {
        if !rest[..start].is_empty() {
            spans.push((rest[..start].to_owned(), false));
        }
        rest = &rest[start + '«'.len_utf8()..];
        let Some(end) = rest.find('»') else {
            break;
        };
        spans.push((rest[..end].to_owned(), true));
        rest = &rest[end + '»'.len_utf8()..];
    }
    if !rest.is_empty() {
        spans.push((rest.to_owned(), false));
    }
    div()
        .flex_1()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_size(px(12.0))
        .flex()
        .children(spans.into_iter().map(|(text, highlighted)| {
            if highlighted {
                div()
                    .text_color(rgb(ACCENT))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(text)
            } else {
                div().text_color(rgb(INK_MUTED)).child(text)
            }
        }))
        .into_any_element()
}

fn activity_action_content(icon: IconName, label: &'static str) -> gpui::Div {
    div()
        .size_full()
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .text_sm()
        .child(
            div()
                .w(px(18.0))
                .flex()
                .justify_center()
                .child(Icon::new(icon).size(px(15.0)).text_color(rgb(INK_MUTED))),
        )
        .child(label)
}

fn welcome_action(
    id: &'static str,
    icon: IconName,
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
        .rounded_xl()
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
                .child(Icon::new(icon).size(px(15.0)).text_color(rgb(ACCENT))),
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
        .rounded_xl()
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(SURFACE))
        .child(
            div()
                .text_sm()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(INK))
                .child(title),
        )
}

fn page_heading(title: &'static str, subtitle: &'static str) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .pb_2()
        .child(
            div()
                .text_xl()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(div().text_sm().text_color(rgb(INK_MUTED)).child(subtitle))
}

fn field_label(label: &'static str) -> gpui::Div {
    div().text_sm().text_color(rgb(INK_MUTED)).child(label)
}

#[cfg(test)]
mod tests {
    use super::should_show_tab_bar;

    #[test]
    fn tab_bar_only_appears_for_multiple_open_documents() {
        assert!(!should_show_tab_bar(0));
        assert!(!should_show_tab_bar(1));
        assert!(should_show_tab_bar(2));
        assert!(should_show_tab_bar(8));
    }
}
