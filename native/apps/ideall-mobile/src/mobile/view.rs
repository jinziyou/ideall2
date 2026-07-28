use super::*;

impl Render for IdeallMobile {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.drain_keyboard(cx);
        #[cfg(any(target_os = "ios", feature = "mobile-ui-host-check"))]
        self.drain_ui_test_action(cx);
        let (safe_top, safe_bottom, safe_left, safe_right) = gpui_mobile::safe_area_insets();
        let theme = MaterialTheme::light();
        let items = self.items.clone();
        let selected_id = self.selected_id.clone();
        let has_selection = selected_id.is_some();
        let embedded_browser = self
            .embedded_browser
            .as_ref()
            .and_then(WebViewHandle::platform_view_handle);
        let show_embedded_browser = embedded_browser.is_some();
        let show_settings = !has_selection && self.active_section == "设置";
        let show_apps = !has_selection && self.active_section == "应用";
        let show_browse = !has_selection && self.active_section == "浏览";
        let can_create = !self.show_trash && self.active_section == "我的";
        let engine_capabilities = engine_runtime_capabilities(EnginePlatform::current_mobile());
        let agent_audits = self.agent_audits.clone();
        let agent_threads = self.agent_threads.clone();
        let agent_transcript = self.agent_transcript.clone();
        let can_save = self.dirty && !self.show_trash;
        let show_note_toolbar = has_selection
            && !self.show_trash
            && self.selected_kind == Some(NodeKind::Note)
            && self.body_editable;
        let search_focused = self.focused_field == Some(FocusedField::Search);
        let title_focused = self.focused_field == Some(FocusedField::Title);
        let body_focused = self.focused_field == Some(FocusedField::Body);
        if body_focused && self.body_last_revealed_cursor != Some(self.focused_cursor) {
            self.body_scroll_handle
                .scroll_to_item(mobile_cursor_line_index(
                    &self.draft_body,
                    self.focused_cursor,
                ));
            self.body_last_revealed_cursor = Some(self.focused_cursor);
        }
        let slash_commands = if show_note_toolbar && body_focused {
            mobile_slash_query(&self.draft_body, self.focused_cursor)
                .map(mobile_slash_commands)
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let show_slash_menu = !slash_commands.is_empty();
        let empty_message = match self.active_section {
            "搜索" => "没有匹配的本地内容",
            "回收站" => "回收站为空",
            "活动" => "还没有本地活动",
            "浏览" => "还没有书签或关注源",
            "应用" => "平台 Engine capability 已就绪",
            "设置" => "本地设置已就绪",
            _ => "点击“+ 笔记”开始",
        };
        let focused_cursor = |field: FocusedField, text_len: usize| {
            if self.focused_field == Some(field) {
                self.focused_cursor.min(text_len)
            } else {
                text_len
            }
        };
        let root_focus = self.focus_handle.clone();

        let search_input = TextInput::<IdeallMobile>::new("mobile-search", theme)
            .label("搜索")
            .value(&self.search_query)
            .placeholder("标题与正文")
            .focused(search_focused)
            .cursor(focused_cursor(
                FocusedField::Search,
                self.search_query.len(),
            ))
            .on_tap(|this, _, _, _| {
                this.focus_field(FocusedField::Search, KeyboardType::Default);
            })
            .render(cx);
        let feed_key_input = TextInput::<IdeallMobile>::new("mobile-feed-key", theme)
            .label("发布者域名")
            .value(&self.feed_key_draft)
            .placeholder("例如 example.com")
            .focused(self.focused_field == Some(FocusedField::FeedKey))
            .cursor(focused_cursor(
                FocusedField::FeedKey,
                self.feed_key_draft.len(),
            ))
            .on_tap(|this, _, _, _| {
                this.focus_field(FocusedField::FeedKey, KeyboardType::URL);
            })
            .render(cx);
        let title_input = TextInput::<IdeallMobile>::new("mobile-title", theme)
            .label("标题")
            .value(&self.draft_title)
            .placeholder("标题")
            .focused(title_focused)
            .cursor(focused_cursor(FocusedField::Title, self.draft_title.len()))
            .on_tap(|this, _, _, _| {
                this.focus_field(FocusedField::Title, KeyboardType::Default);
            })
            .render(cx);
        let use_multiline_body =
            matches!(self.selected_kind, Some(NodeKind::Note | NodeKind::File));
        let body_input = if use_multiline_body {
            let body_scroll_handle = self.body_scroll_handle.clone();
            let cursor_line = mobile_cursor_line_index(&self.draft_body, self.focused_cursor);
            let line_elements = mobile_text_line_ranges(&self.draft_body)
                .into_iter()
                .enumerate()
                .map(|(line_index, range)| {
                    let line_start = range.start;
                    let line = self.draft_body[range.clone()].to_owned();
                    let display = if body_focused && line_index == cursor_line {
                        mobile_line_with_cursor(
                            &line,
                            self.focused_cursor.saturating_sub(line_start),
                        )
                    } else if line.is_empty() {
                        "\u{200b}".to_owned()
                    } else {
                        line.clone()
                    };
                    let hit_test_line = line;
                    let hit_test_scroll = body_scroll_handle.clone();
                    div()
                        .w_full()
                        .min_h(px(30.))
                        .px_3()
                        .py_1()
                        .text_sm()
                        .line_height(px(22.))
                        .whitespace_normal()
                        .when(body_focused && line_index == cursor_line, |line| {
                            line.bg(rgb(SURFACE_MUTED))
                        })
                        .when(self.body_editable && !self.show_trash, |line| {
                            line.cursor_text().on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                    this.focus_field(FocusedField::Body, KeyboardType::Default);
                                    let offset = hit_test_scroll
                                        .bounds_for_item(line_index)
                                        .map(|bounds| {
                                            let text_origin =
                                                bounds.origin + point(px(12.), px(4.));
                                            let local_position = point(
                                                event.position.x - text_origin.x,
                                                event.position.y - text_origin.y,
                                            );
                                            let wrap_width =
                                                (bounds.size.width - px(24.)).max(px(1.));
                                            let text_style = window.text_style();
                                            window
                                                .text_system()
                                                .shape_text(
                                                    SharedString::from(hit_test_line.clone()),
                                                    px(14.),
                                                    &[text_style.to_run(hit_test_line.len())],
                                                    Some(wrap_width),
                                                    None,
                                                )
                                                .ok()
                                                .and_then(|mut lines| lines.pop())
                                                .map(|layout| {
                                                    layout
                                                        .closest_index_for_position(
                                                            local_position,
                                                            px(22.),
                                                        )
                                                        .unwrap_or_else(|index| index)
                                                })
                                                .unwrap_or_default()
                                        })
                                        .unwrap_or(hit_test_line.len())
                                        .min(hit_test_line.len());
                                    this.focused_cursor = line_start + offset;
                                    this.focused_selection_start = this.focused_cursor;
                                    this.body_last_revealed_cursor = None;
                                    native_text::update_selection(
                                        this.focused_cursor,
                                        this.focused_cursor,
                                    );
                                    hit_test_scroll.scroll_to_item(line_index);
                                    cx.notify();
                                }),
                            )
                        })
                        .child(display)
                });
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_xs().text_color(rgb(INK_MUTED)).child("正文"))
                .child(
                    div()
                        .id("mobile-body-scroll")
                        .h(px(260.))
                        .min_h(px(180.))
                        .w_full()
                        .overflow_y_scroll()
                        .track_scroll(&body_scroll_handle)
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(if body_focused { ACCENT } else { BORDER }))
                        .bg(rgb(if self.body_editable {
                            SURFACE_RAISED
                        } else {
                            SURFACE_MUTED
                        }))
                        .children(line_elements),
                )
                .into_any_element()
        } else {
            let mut body_builder = TextInput::<IdeallMobile>::new("mobile-body", theme)
                .label(if self.selected_kind == Some(NodeKind::Bookmark) {
                    "网址"
                } else {
                    "正文"
                })
                .value(&self.draft_body)
                .placeholder("从这里开始记录……")
                .focused(body_focused)
                .cursor(focused_cursor(FocusedField::Body, self.draft_body.len()));
            if self.body_editable && !self.show_trash {
                let keyboard_type = if self.selected_kind == Some(NodeKind::Bookmark) {
                    KeyboardType::URL
                } else {
                    KeyboardType::Default
                };
                body_builder = body_builder.on_tap(move |this, _, _, _| {
                    this.focus_field(FocusedField::Body, keyboard_type);
                });
            }
            body_builder.render(cx).into_any_element()
        };
        let sync_server_input = TextInput::<IdeallMobile>::new("mobile-sync-server", theme)
            .label("服务端基址")
            .value(&self.sync_server_draft)
            .placeholder("https://api.wonita.link")
            .focused(self.focused_field == Some(FocusedField::SyncServer))
            .cursor(focused_cursor(
                FocusedField::SyncServer,
                self.sync_server_draft.len(),
            ))
            .on_tap(|this, _, _, _| {
                this.focus_field(FocusedField::SyncServer, KeyboardType::URL);
            })
            .render(cx);
        let masked_code = "*".repeat(self.sync_code_draft.len());
        let sync_code_input = TextInput::<IdeallMobile>::new("mobile-sync-code", theme)
            .label("同步码")
            .value(&masked_code)
            .placeholder("32 位十六进制；留空保持不变")
            .focused(self.focused_field == Some(FocusedField::SyncCode))
            .cursor(focused_cursor(FocusedField::SyncCode, masked_code.len()))
            .on_tap(|this, _, _, _| {
                this.focus_field(FocusedField::SyncCode, KeyboardType::Default);
            })
            .render(cx);
        let masked_token = "*".repeat(self.sync_token_draft.len());
        let sync_token_input = TextInput::<IdeallMobile>::new("mobile-sync-token", theme)
            .label("登录 Bearer Token")
            .value(&masked_token)
            .placeholder("留空保持不变")
            .focused(self.focused_field == Some(FocusedField::SyncToken))
            .cursor(focused_cursor(FocusedField::SyncToken, masked_token.len()))
            .on_tap(|this, _, _, _| {
                this.focus_field(FocusedField::SyncToken, KeyboardType::Default);
            })
            .render(cx);
        let agent_base_url_input = TextInput::<IdeallMobile>::new("mobile-agent-base-url", theme)
            .label("模型 API 基址")
            .value(&self.agent_base_url_draft)
            .placeholder("https://api.deepseek.com/v1/")
            .focused(self.focused_field == Some(FocusedField::AgentBaseUrl))
            .cursor(focused_cursor(
                FocusedField::AgentBaseUrl,
                self.agent_base_url_draft.len(),
            ))
            .on_tap(|this, _, _, _| {
                this.focus_field(FocusedField::AgentBaseUrl, KeyboardType::URL);
            })
            .render(cx);
        let agent_model_input = TextInput::<IdeallMobile>::new("mobile-agent-model", theme)
            .label("模型名称")
            .value(&self.agent_model_draft)
            .placeholder("deepseek-chat")
            .focused(self.focused_field == Some(FocusedField::AgentModel))
            .cursor(focused_cursor(
                FocusedField::AgentModel,
                self.agent_model_draft.len(),
            ))
            .on_tap(|this, _, _, _| {
                this.focus_field(FocusedField::AgentModel, KeyboardType::Default);
            })
            .render(cx);
        let masked_agent_key = "*".repeat(self.agent_key_draft.len());
        let agent_key_input = TextInput::<IdeallMobile>::new("mobile-agent-key", theme)
            .label("API Key")
            .value(&masked_agent_key)
            .placeholder("留空保持不变")
            .focused(self.focused_field == Some(FocusedField::AgentKey))
            .cursor(focused_cursor(
                FocusedField::AgentKey,
                masked_agent_key.len(),
            ))
            .on_tap(|this, _, _, _| {
                this.focus_field(FocusedField::AgentKey, KeyboardType::Default);
            })
            .render(cx);
        let agent_prompt_input = TextInput::<IdeallMobile>::new("mobile-agent-prompt", theme)
            .label("消息")
            .value(&self.agent_prompt_draft)
            .placeholder("向本地 Agent 提问或调用工具……")
            .focused(self.focused_field == Some(FocusedField::AgentPrompt))
            .cursor(focused_cursor(
                FocusedField::AgentPrompt,
                self.agent_prompt_draft.len(),
            ))
            .on_tap(|this, _, _, _| {
                this.focus_field(FocusedField::AgentPrompt, KeyboardType::Default);
            })
            .render(cx);
        let note_toolbar = [
            (
                "mobile-format-paragraph",
                "正文",
                MobileBlockStyle::Paragraph,
            ),
            ("mobile-format-h1", "标题 1", MobileBlockStyle::Heading1),
            ("mobile-format-h2", "标题 2", MobileBlockStyle::Heading2),
            ("mobile-format-quote", "引用", MobileBlockStyle::Quote),
            ("mobile-format-bullet", "项目", MobileBlockStyle::Bullet),
            ("mobile-format-ordered", "编号", MobileBlockStyle::Ordered),
            ("mobile-format-todo", "任务", MobileBlockStyle::Todo),
            ("mobile-format-code", "代码", MobileBlockStyle::Code),
            ("mobile-format-rule", "分隔线", MobileBlockStyle::Rule),
        ]
        .into_iter()
        .fold(
            div()
                .id("mobile-note-format-toolbar")
                .flex()
                .flex_wrap()
                .gap_2(),
            |toolbar, (id, label, style)| {
                toolbar.child(
                    div()
                        .id(id)
                        .px_3()
                        .py_2()
                        .rounded_full()
                        .bg(rgb(ACCENT_SOFT))
                        .text_sm()
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.format_note_block(style, cx);
                            }),
                        )
                        .child(label),
                )
            },
        )
        .child(
            div()
                .id("mobile-move-block-up")
                .px_3()
                .py_2()
                .rounded_full()
                .bg(rgb(WARNING_SOFT))
                .text_sm()
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.move_note_block(MobileBlockMove::Up, cx);
                    }),
                )
                .child("↑ 当前块"),
        )
        .child(
            div()
                .id("mobile-move-block-down")
                .px_3()
                .py_2()
                .rounded_full()
                .bg(rgb(WARNING_SOFT))
                .text_sm()
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.move_note_block(MobileBlockMove::Down, cx);
                    }),
                )
                .child("↓ 当前块"),
        );
        let note_slash_menu = slash_commands.into_iter().fold(
            div()
                .id("mobile-note-slash-menu")
                .flex()
                .flex_wrap()
                .gap_2()
                .p_2()
                .rounded_md()
                .border_1()
                .border_color(rgb(BORDER))
                .bg(rgb(SURFACE_MUTED)),
            |menu, (id, label, style)| {
                menu.child(
                    div()
                        .id(format!("mobile-slash-{id}"))
                        .px_3()
                        .py_2()
                        .rounded_full()
                        .bg(rgb(SURFACE_RAISED))
                        .text_sm()
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.run_note_slash_command(style, cx);
                            }),
                        )
                        .child(label),
                )
            },
        );

        div()
            .size_full()
            .track_focus(&self.focus_handle)
            .on_mouse_down(
                MouseButton::Left,
                move |_, window, cx| window.focus(&root_focus, cx),
            )
            .on_key_down(cx.listener(|this, event, _, cx| {
                this.handle_control_key(event);
                cx.notify();
            }))
            .flex()
            .flex_col()
            .bg(rgb(CANVAS))
            .text_color(rgb(INK))
            .pt(px(safe_top))
            .pr(px(safe_right))
            .pb(px(safe_bottom))
            .pl(px(safe_left))
            .child(
                div()
                    .id("mobile-header")
                    .h(px(60.0))
                    .flex()
                    .items_center()
                    .px_4()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(SURFACE_RAISED))
                    .child(if has_selection || show_embedded_browser {
                        "‹  返回"
                    } else {
                        self.active_section
                    })
                    .when(has_selection || show_embedded_browser, |bar| {
                        bar.cursor_pointer().on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| this.close_detail(cx)),
                        )
                    })
                    .when(!has_selection && !show_embedded_browser && can_create, |bar| {
                        bar.child(
                            div()
                                .ml_auto()
                                .id("mobile-new-note")
                                .px_3()
                                .py_2()
                                .rounded_full()
                                .bg(rgb(ACCENT_SOFT))
                                .text_color(rgb(ACCENT))
                                .cursor_pointer()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _, _, cx| this.create_note(cx)),
                                )
                                .child("+ 笔记"),
                        )
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .p_4()
                    .gap_3()
                    .when(
                        !has_selection
                            && !show_settings
                            && !show_apps
                            && !show_embedded_browser,
                        |content| {
                        content
                            .child(search_input)
                            .when(show_browse, |content| {
                                content.child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_2()
                                        .child(feed_key_input)
                                        .child(
                                            div()
                                                .id("mobile-create-feed")
                                                .px_4()
                                                .py_3()
                                                .rounded_full()
                                                .bg(rgb(ACCENT))
                                                .text_color(rgb(SURFACE_RAISED))
                                                .cursor_pointer()
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.create_feed(cx);
                                                    }),
                                                )
                                                .child("+ 关注发布者"),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .gap_2()
                                        .child(
                                            div()
                                                .id("mobile-open-info")
                                                .px_4()
                                                .py_3()
                                                .rounded_full()
                                                .bg(rgb(ACCENT_SOFT))
                                                .cursor_pointer()
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.open_portal(INFO_PORTAL_URL, cx);
                                                    }),
                                                )
                                                .child("资讯"),
                                        )
                                        .child(
                                            div()
                                                .id("mobile-open-community")
                                                .px_4()
                                                .py_3()
                                                .rounded_full()
                                                .bg(rgb(ACCENT_SOFT))
                                                .cursor_pointer()
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.open_portal(
                                                            COMMUNITY_PORTAL_URL,
                                                            cx,
                                                        );
                                                    }),
                                                )
                                                .child("社区"),
                                        ),
                                )
                            })
                            .when(can_create, |content| {
                                content.child(
                                    div()
                                        .flex()
                                        .flex_wrap()
                                        .gap_2()
                                        .child(
                                            div()
                                                .id("mobile-create-folder")
                                                .px_3()
                                                .py_2()
                                                .rounded_full()
                                                .bg(rgb(ACCENT_SOFT))
                                                .cursor_pointer()
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.create_folder(cx);
                                                    }),
                                                )
                                                .child("+ 文件夹"),
                                        )
                                        .child(
                                            div()
                                                .id("mobile-create-bookmark")
                                                .px_3()
                                                .py_2()
                                                .rounded_full()
                                                .bg(rgb(ACCENT_SOFT))
                                                .cursor_pointer()
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.create_bookmark(cx);
                                                    }),
                                                )
                                                .child("+ 书签"),
                                        )
                                        .child(
                                            div()
                                                .id("mobile-create-file")
                                                .px_3()
                                                .py_2()
                                                .rounded_full()
                                                .bg(rgb(ACCENT_SOFT))
                                                .cursor_pointer()
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.create_text_file(cx);
                                                    }),
                                                )
                                                .child("+ 空文件"),
                                        )
                                        .child(
                                            div()
                                                .id("mobile-import-file")
                                                .px_3()
                                                .py_2()
                                                .rounded_full()
                                                .bg(rgb(ACCENT_SOFT))
                                                .cursor_pointer()
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.import_files(cx);
                                                    }),
                                                )
                                                .child("导入"),
                                        )
                                        .child(
                                            div()
                                                .id("mobile-show-trash")
                                                .px_3()
                                                .py_2()
                                                .rounded_full()
                                                .bg(rgb(MESSAGE_TOOL))
                                                .cursor_pointer()
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.select_section("回收站", cx);
                                                    }),
                                                )
                                                .child("回收站"),
                                        ),
                                )
                            })
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(INK_MUTED))
                                    .child(self.status.clone()),
                            )
                            .child(
                                div()
                                    .id("mobile-list")
                                    .flex_1()
                                    .min_h_0()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .overflow_y_scroll()
                                    .when(items.is_empty(), |list| {
                                        list.child(
                                            div()
                                                .p_4()
                                                .text_color(rgb(INK_MUTED))
                                                .child(empty_message),
                                        )
                                    })
                                    .children(items.into_iter().map(|item| {
                                        let id = item.id.clone();
                                        div()
                                            .id(format!("mobile-node-{id}"))
                                            .flex()
                                            .items_center()
                                            .gap_3()
                                            .px_3()
                                            .pl(px(12.0 + item.depth as f32 * 14.0))
                                            .py_3()
                                            .rounded_lg()
                                            .bg(rgb(SURFACE_RAISED))
                                            .cursor_pointer()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(move |this, _, _, cx| {
                                                    this.open_node(id.clone(), cx);
                                                }),
                                            )
                                            .child(kind_icon(item.kind))
                                            .child(item.title)
                                    })),
                            )
                    },
                    )
                    .when(show_apps, |content| {
                        content.child(
                            div()
                                .id("mobile-apps-panel")
                                .flex_1()
                                .min_h_0()
                                .flex()
                                .flex_col()
                                .gap_3()
                                .overflow_y_scroll()
                                .child(div().text_xl().child("Agent 与原生 Engine"))
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(INK_MUTED))
                                        .child("BYOK 由本机直连模型；默认工具可读取节点元数据并创建笔记，但不能读取既有笔记正文或 Blob。"),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(DANGER))
                                        .child("移动端 capability：不支持启动外部 ACP 子进程；请使用上方 BYOK 模型会话。"),
                                )
                                .child(div().text_lg().child("模型连接（OpenAI-compatible）"))
                                .child(agent_base_url_input)
                                .child(agent_model_input)
                                .child(div().text_sm().child(format!(
                                    "API Key：{}",
                                    if self.agent_key_configured {
                                        "已安全配置"
                                    } else {
                                        "未配置"
                                    }
                                )))
                                .child(agent_key_input)
                                .child(
                                    div()
                                        .flex()
                                        .flex_wrap()
                                        .gap_2()
                                        .child(
                                            div()
                                                .id("mobile-agent-save-settings")
                                                .px_4()
                                                .py_3()
                                                .rounded_full()
                                                .bg(rgb(ACCENT_SOFT))
                                                .cursor_pointer()
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.save_agent_model_settings(cx);
                                                    }),
                                                )
                                                .child("安全保存"),
                                        )
                                        .child(
                                            div()
                                                .id("mobile-agent-clear-key")
                                                .px_4()
                                                .py_3()
                                                .rounded_full()
                                                .bg(rgb(DANGER_SOFT))
                                                .text_color(rgb(DANGER))
                                                .cursor_pointer()
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.clear_agent_credential(cx);
                                                    }),
                                                )
                                                .child("清除 Key"),
                                        )
                                        .child(
                                            div()
                                                .id("mobile-agent-create-draft")
                                                .px_4()
                                                .py_3()
                                                .rounded_full()
                                                .bg(rgb(MESSAGE_TOOL))
                                                .cursor_pointer()
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.create_agent_draft(cx);
                                                    }),
                                                )
                                                .child("MCP 冒烟：创建草稿"),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(div().text_lg().child("本地对话"))
                                        .child(
                                            div()
                                                .id("mobile-agent-new-thread")
                                                .px_4()
                                                .py_2()
                                                .rounded_full()
                                                .bg(rgb(ACCENT_SOFT))
                                                .when(!self.agent_in_progress, |button| {
                                                    button.cursor_pointer().on_mouse_down(
                                                        MouseButton::Left,
                                                        cx.listener(|this, _, _, cx| {
                                                            this.new_agent_thread(cx);
                                                        }),
                                                    )
                                                })
                                                .child("新对话"),
                                        ),
                                )
                                .when(agent_threads.is_empty(), |threads| {
                                    threads.child(
                                        div()
                                            .text_sm()
                                            .text_color(rgb(INK_MUTED))
                                            .child("尚无 Agent 对话"),
                                    )
                                })
                                .child(
                                    div()
                                        .flex()
                                        .flex_wrap()
                                        .gap_2()
                                        .children(agent_threads.into_iter().map(|thread| {
                                            let id = thread.id.clone();
                                            let selected = self.agent_thread_id.as_deref()
                                                == Some(thread.id.as_str());
                                            div()
                                                .id(format!("mobile-agent-thread-{}", thread.id))
                                                .px_3()
                                                .py_2()
                                                .rounded_full()
                                                .bg(rgb(if selected { ACCENT } else { MESSAGE_TOOL }))
                                                .text_color(rgb(if selected { SURFACE_RAISED } else { INK }))
                                                .when(!self.agent_in_progress, |button| {
                                                    button.cursor_pointer().on_mouse_down(
                                                        MouseButton::Left,
                                                        cx.listener(move |this, _, _, cx| {
                                                            this.select_agent_thread(id.clone(), cx);
                                                        }),
                                                    )
                                                })
                                                .child(thread.title)
                                        })),
                                )
                                .when(agent_transcript.is_empty(), |messages| {
                                    messages.child(
                                        div()
                                            .p_3()
                                            .rounded_lg()
                                            .bg(rgb(SURFACE_RAISED))
                                            .text_color(rgb(INK_MUTED))
                                            .child("输入消息开始对话"),
                                    )
                                })
                                .children(agent_transcript.into_iter().map(|message| {
                                    div()
                                        .p_3()
                                        .rounded_lg()
                                        .border_1()
                                        .border_color(rgb(BORDER))
                                        .bg(rgb(match message.role {
                                            ModelRole::User => ACCENT_SOFT,
                                            ModelRole::Assistant => SURFACE_RAISED,
                                            ModelRole::Tool => MESSAGE_TOOL,
                                            ModelRole::System => WARNING_SOFT,
                                        }))
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(rgb(INK_MUTED))
                                                .child(agent_role_label(message.role)),
                                        )
                                        .child(message.content)
                                }))
                                .child(agent_prompt_input)
                                .child(
                                    div()
                                        .id("mobile-agent-send")
                                        .px_4()
                                        .py_3()
                                        .rounded_full()
                                        .bg(rgb(if self.agent_in_progress {
                                            DISABLED
                                        } else {
                                            ACCENT
                                        }))
                                        .text_color(rgb(SURFACE_RAISED))
                                        .when(!self.agent_in_progress, |button| {
                                            button.cursor_pointer().on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(|this, _, _, cx| {
                                                    this.start_agent_turn(cx);
                                                }),
                                            )
                                        })
                                        .child(if self.agent_in_progress {
                                            "Agent 处理中……"
                                        } else {
                                            "发送给 Agent"
                                        }),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(INK_MUTED))
                                        .child(self.status.clone()),
                                )
                                .child(
                                    div()
                                        .text_lg()
                                        .child(format!(
                                            "平台 Engine 能力（{}）",
                                            engine_capabilities.len()
                                        )),
                                )
                                .children(engine_capabilities.into_iter().map(|capability| {
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .px_3()
                                        .py_2()
                                        .rounded_lg()
                                        .border_1()
                                        .border_color(rgb(BORDER))
                                        .bg(rgb(SURFACE_RAISED))
                                        .child(
                                            div()
                                                .flex()
                                                .justify_between()
                                                .child(format!(
                                                    "{} · {}",
                                                    capability.label, capability.engine_id
                                                ))
                                                .child(
                                                    div()
                                                        .text_sm()
                                                        .text_color(rgb(
                                                            if capability.kind.is_available() {
                                                                ACCENT
                                                            } else {
                                                                DANGER
                                                            },
                                                        ))
                                                        .child(format!(
                                                            "{} / {}",
                                                            capability.kind.label(),
                                                            engine_access_label(
                                                                capability.access
                                                            )
                                                        )),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(rgb(INK_MUTED))
                                                .child(capability.detail),
                                        )
                                }))
                                .child(div().text_lg().child("最近写操作审计"))
                                .when(agent_audits.is_empty(), |audits| {
                                    audits.child(
                                        div()
                                            .p_3()
                                            .rounded_lg()
                                            .bg(rgb(SURFACE_RAISED))
                                            .text_color(rgb(INK_MUTED))
                                            .child("尚无 Agent 写操作"),
                                    )
                                })
                                .children(agent_audits.into_iter().map(|audit| {
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .p_3()
                                        .rounded_lg()
                                        .border_1()
                                        .border_color(rgb(BORDER))
                                        .bg(rgb(SURFACE_RAISED))
                                        .child(
                                            div()
                                                .flex()
                                                .justify_between()
                                                .child(audit.title)
                                                .child(audit_status_label(audit.status)),
                                        )
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(rgb(INK_MUTED))
                                                .child(format!(
                                                    "{} · {}",
                                                    audit.operation, audit.summary
                                                )),
                                        )
                                })),
                        )
                    })
                    .when(show_settings, |content| {
                        content
                            .child(div().text_xl().child("端到端加密同步"))
                            .child(
                                div().text_sm().text_color(rgb(INK_MUTED)).child(
                                    "服务端只保存密文；同步码与 Token 由系统安全存储保护。",
                                ),
                            )
                            .child(sync_server_input)
                            .child(div().text_sm().child(format!(
                                "同步码：{}",
                                if self.sync_code_configured {
                                    "已配置"
                                } else {
                                    "未配置"
                                }
                            )))
                            .child(sync_code_input)
                            .child(div().text_sm().child(format!(
                                "登录 Token：{}",
                                if self.sync_token_configured {
                                    "已配置"
                                } else {
                                    "未配置"
                                }
                            )))
                            .child(sync_token_input)
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap_2()
                                    .child(
                                        div()
                                            .id("mobile-save-sync-settings")
                                            .px_4()
                                            .py_3()
                                            .rounded_full()
                                            .bg(rgb(ACCENT_SOFT))
                                            .cursor_pointer()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(|this, _, _, cx| {
                                                    this.save_sync_settings(cx);
                                                }),
                                            )
                                            .child("安全保存"),
                                    )
                                    .child(
                                        div()
                                            .id("mobile-sync-notes")
                                            .px_4()
                                            .py_3()
                                            .rounded_full()
                                            .bg(rgb(if self.sync_in_progress {
                                                DISABLED
                                            } else {
                                                ACCENT
                                            }))
                                            .text_color(rgb(SURFACE_RAISED))
                                            .when(!self.sync_in_progress, |button| {
                                                button.cursor_pointer().on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.start_notes_sync(cx);
                                                    }),
                                                )
                                            })
                                            .child(if self.sync_in_progress {
                                                "同步中……"
                                            } else {
                                                "立即同步全部"
                                            }),
                                    )
                                    .child(
                                        div()
                                            .id("mobile-clear-sync-secrets")
                                            .px_4()
                                            .py_3()
                                            .rounded_full()
                                            .bg(rgb(DANGER_SOFT))
                                            .text_color(rgb(DANGER))
                                            .cursor_pointer()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(|this, _, _, cx| {
                                                    this.clear_sync_credentials(cx);
                                                }),
                                            )
                                            .child("清除凭据"),
                                    ),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(INK_MUTED))
                                    .child(self.status.clone()),
                            )
                    })
                    .when(show_embedded_browser, |content| {
                        content.child(
                            platform_view_element(embedded_browser.clone().unwrap())
                                .flex_1()
                                .min_h_0()
                                .size_full(),
                        )
                    })
                    .when(has_selection && !show_embedded_browser, |content| {
                        content
                            .child(title_input)
                            .when(show_note_toolbar, |content| content.child(note_toolbar))
                            .when(show_slash_menu, |content| content.child(note_slash_menu))
                            .child(body_input)
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(INK_MUTED))
                                    .child(self.status.clone()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_3()
                                    .child(
                                        div()
                                            .id("mobile-save")
                                            .px_4()
                                            .py_3()
                                            .rounded_full()
                                            .bg(rgb(if can_save { ACCENT } else { DISABLED }))
                                            .text_color(rgb(SURFACE_RAISED))
                                            .when(can_save, |button| {
                                                button.cursor_pointer().on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| this.save(cx)),
                                                )
                                            })
                                            .child(if self.dirty {
                                                "保存"
                                            } else {
                                                "已保存"
                                            }),
                                    )
                                    .when(
                                        !self.show_trash
                                            && self.selected_kind == Some(NodeKind::Bookmark),
                                        |actions| {
                                            actions
                                                .child(
                                                    div()
                                                        .id("mobile-open-bookmark")
                                                        .px_4()
                                                        .py_3()
                                                        .rounded_full()
                                                        .bg(rgb(ACCENT_SOFT))
                                                        .cursor_pointer()
                                                        .on_mouse_down(
                                                            MouseButton::Left,
                                                            cx.listener(|this, _, _, cx| {
                                                                this.open_bookmark_external(cx);
                                                            }),
                                                        )
                                                        .child("浏览器打开"),
                                                )
                                                .child(
                                                    div()
                                                        .id("mobile-embed-bookmark")
                                                        .px_4()
                                                        .py_3()
                                                        .rounded_full()
                                                        .bg(rgb(ACCENT_SOFT))
                                                        .cursor_pointer()
                                                        .on_mouse_down(
                                                            MouseButton::Left,
                                                            cx.listener(|this, _, _, cx| {
                                                                this.open_bookmark_embedded(cx);
                                                            }),
                                                        )
                                                        .child("应用内打开"),
                                                )
                                        },
                                    )
                                    .when(!self.show_trash, |actions| {
                                        actions.child(
                                            div()
                                                .id("mobile-trash")
                                                .px_4()
                                                .py_3()
                                                .rounded_full()
                                                .bg(rgb(DANGER_SOFT))
                                                .text_color(rgb(DANGER))
                                                .cursor_pointer()
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.move_to_trash(cx);
                                                    }),
                                                )
                                                .child("删除"),
                                        )
                                    })
                                    .when(self.show_trash, |actions| {
                                        actions
                                            .child(
                                                div()
                                                    .id("mobile-restore")
                                                    .px_4()
                                                    .py_3()
                                                    .rounded_full()
                                                    .bg(rgb(ACCENT_SOFT))
                                                    .cursor_pointer()
                                                    .on_mouse_down(
                                                        MouseButton::Left,
                                                        cx.listener(|this, _, _, cx| {
                                                            this.restore(cx);
                                                        }),
                                                    )
                                                    .child("恢复"),
                                            )
                                            .child(
                                                div()
                                                    .id("mobile-purge")
                                                    .px_4()
                                                    .py_3()
                                                    .rounded_full()
                                                    .bg(rgb(if self.purge_armed {
                                                        DANGER
                                                    } else {
                                                        DANGER_SOFT
                                                    }))
                                                    .text_color(rgb(if self.purge_armed {
                                                        SURFACE_RAISED
                                                    } else {
                                                        DANGER
                                                    }))
                                                    .cursor_pointer()
                                                    .on_mouse_down(
                                                        MouseButton::Left,
                                                        cx.listener(|this, _, _, cx| {
                                                            this.purge(cx);
                                                        }),
                                                    )
                                                    .child("永久删除"),
                                            )
                                    }),
                            )
                    }),
            )
            .child(
                div()
                    .h(px(66.0))
                    .flex()
                    .items_center()
                    .justify_around()
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(RAIL))
                    .children(NAVIGATION.map(|navigation| {
                        let section = navigation.section;
                        let selected = self.active_section == section && !self.show_trash;
                        div()
                            .id(format!("mobile-section-{section}"))
                            .min_w(px(54.0))
                            .px_1()
                            .py_2()
                            .rounded_lg()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_1()
                            .text_color(rgb(if selected {
                                SURFACE_RAISED
                            } else {
                                RAIL_MUTED
                            }))
                            .when(selected, |item| item.bg(rgb(ACCENT)))
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    this.select_section(section, cx);
                                }),
                            )
                            .child(div().text_sm().child(navigation.icon))
                            .child(div().text_xs().child(section))
                    })),
            )
    }
}
