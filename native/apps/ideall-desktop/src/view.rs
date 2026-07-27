use super::*;

impl Render for IdeallDesktop {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected_id = self.selected_id.clone();
        let items = self.items.clone();
        let visible_item_count = items.len();
        let tabs = self.workspace.tabs.clone();
        let active_tab = self.workspace.active_id.clone();
        let has_selection = self.selected_id.is_some();
        let show_settings = !has_selection && self.active_section.as_ref() == "设置";
        let show_apps = !has_selection && self.active_section.as_ref() == "应用";
        let show_browse = self.active_section.as_ref() == "浏览";
        let can_create = !self.show_trash && self.active_section.as_ref() == "我的";
        let engine_capabilities = engine_runtime_capabilities(EnginePlatform::current_desktop());
        let agent_audits = self.agent_audits.clone();
        let agent_threads = self.agent_threads.clone();
        let agent_transcript = self.agent_transcript.clone();
        let available_update = self.available_update.clone();
        let empty_message: SharedString = match self.active_section.as_ref() {
            "搜索" => "没有匹配的本地内容".into(),
            "回收站" => "回收站为空".into(),
            "活动" => "还没有本地活动".into(),
            "浏览" => "还没有书签或关注源".into(),
            "应用" => format!("已声明 {} 项平台 Engine 能力", engine_capabilities.len()).into(),
            "设置" => "本地设置已就绪；文件 Engine 偏好会自动持久化".into(),
            _ => "点击“+ 笔记”开始".into(),
        };
        let is_body_editable = matches!(
            self.selected_kind,
            Some(NodeKind::Note | NodeKind::Bookmark | NodeKind::File)
        );
        let show_code = self.active_engine_id.as_deref() == Some("ideall.code")
            && self.selected_kind == Some(NodeKind::File);
        let show_preview = self.active_engine_id.as_deref() == Some("ideall.preview")
            && self.selected_kind == Some(NodeKind::File);
        let embedded_browser = self.embedded_browser.clone();
        let show_browser = embedded_browser.is_some()
            && self.active_engine_id.as_deref() == Some("ideall.browser");
        let show_remote_webview = !has_selection
            && embedded_browser.is_some()
            && matches!(
                self.active_engine_id.as_deref(),
                Some("ideall.info" | "ideall.community")
            );
        let remote_browser = embedded_browser.clone();
        let delegated_capability = self.active_engine_id.as_deref().and_then(|engine_id| {
            engine_capabilities.iter().copied().find(|capability| {
                capability.engine_id == engine_id
                    && matches!(
                        capability.kind,
                        EngineRuntimeKind::SystemExternal | EngineRuntimeKind::MetadataOnly
                    )
            })
        });
        let show_delegated_engine = has_selection && delegated_capability.is_some();
        let section_description = section_description(self.active_section.as_ref());
        let section_eyebrow = section_eyebrow(self.active_section.as_ref());
        let preview_text = self.body_input.read(cx).value();
        let preview = TextView::markdown("active-file-preview", preview_text, window, cx)
            .selectable(true)
            .scrollable(true);

        div()
            .id("ideall-root")
            .key_context("Ideall")
            .on_action(cx.listener(Self::focus_search))
            .on_action(cx.listener(Self::create_note_from_shortcut))
            .on_action(cx.listener(Self::save_from_shortcut))
            .on_action(cx.listener(Self::close_active_tab))
            .on_action(cx.listener(Self::activate_next_tab))
            .on_action(cx.listener(Self::activate_previous_tab))
            .flex()
            .size_full()
            .bg(rgb(CANVAS))
            .text_color(rgb(INK))
            .child(
                div()
                    .w(px(88.0))
                    .h_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .px_2()
                    .py_4()
                    .bg(rgb(RAIL))
                    .child(
                        div()
                            .w(px(48.0))
                            .h(px(48.0))
                            .mb_5()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_xl()
                            .bg(rgb(ACCENT))
                            .text_xl()
                            .text_color(rgb(RAIL_INK))
                            .child("i."),
                    )
                    .child(
                        div()
                            .flex_1()
                            .w_full()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .children(NAVIGATION.map(|navigation| {
                                let section = navigation.section;
                                let selected = self.active_section.as_ref() == section;
                                div()
                                    .id(SharedString::from(format!("section-{section}")))
                                    .w_full()
                                    .h(px(62.0))
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .gap_1()
                                    .rounded_xl()
                                    .cursor_pointer()
                                    .text_color(rgb(if selected {
                                        RAIL_INK
                                    } else {
                                        RAIL_MUTED
                                    }))
                                    .when(selected, |item| item.bg(rgb(RAIL_ACTIVE)))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.select_section(section, window, cx);
                                    }))
                                    .child(div().text_lg().child(navigation.icon))
                                    .child(div().text_xs().child(section))
                            })),
                    )
                    .child(
                        div()
                            .w(px(50.0))
                            .py_2()
                            .rounded_lg()
                            .text_xs()
                            .text_color(rgb(RAIL_MUTED))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child("LOCAL"),
                    ),
            )
            .child(
                div()
                    .w(px(292.0))
                    .h_full()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .px_4()
                    .py_5()
                    .border_r_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(SURFACE_MUTED))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(
                                div()
                                    .flex()
                                    .items_start()
                                    .justify_between()
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(rgb(ACCENT))
                                                    .child(section_eyebrow),
                                            )
                                            .child(
                                                div()
                                                    .text_xl()
                                                    .child(self.active_section.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(rgb(INK_MUTED))
                                                    .child(section_description),
                                            ),
                                    )
                                    .child(
                                        Button::new("new-note")
                                            .label("+ 笔记")
                                            .small()
                                            .primary()
                                            .disabled(!can_create)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.create_note(window, cx);
                                            })),
                                    ),
                            ),
                    )
                    .child(Input::new(&self.search_input).w_full())
                    .when(show_browse, |sidebar| {
                        sidebar.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .child(Input::new(&self.feed_key_input).w_full())
                                .child(
                                    Button::new("new-feed")
                                        .label("+ 关注发布者")
                                        .small()
                                        .primary()
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.create_feed(window, cx);
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .child(
                                    Button::new("open-info-portal")
                                        .label("资讯")
                                        .small()
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
                                        .label("社区")
                                        .small()
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.open_portal(
                                                "ideall.community",
                                                COMMUNITY_PORTAL_URL,
                                                window,
                                                cx,
                                            );
                                        })),
                                ),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                Button::new("home-list")
                                    .label("全部")
                                    .small()
                                    .selected(!self.show_trash)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.show_files(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("trash-list")
                                    .label("回收站")
                                    .small()
                                    .selected(self.show_trash)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.show_trash(window, cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_1()
                            .child(
                                Button::new("new-bookmark")
                                    .label("+ 书签")
                                    .small()
                                    .ghost()
                                    .disabled(!can_create)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.create_bookmark(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("new-folder")
                                    .label("+ 文件夹")
                                    .small()
                                    .ghost()
                                    .disabled(!can_create)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.create_folder(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("new-file")
                                    .label("+ 空文件")
                                    .small()
                                    .ghost()
                                    .disabled(!can_create)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.create_text_file(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("import-file")
                                    .label("导入")
                                    .small()
                                    .ghost()
                                    .disabled(!can_create)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.import_files(cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .overflow_y_scrollbar()
                            .when(items.is_empty(), |list| {
                                list.child(
                                    div()
                                        .p_4()
                                        .rounded_lg()
                                        .border_1()
                                        .border_color(rgb(BORDER))
                                        .bg(rgb(SURFACE))
                                        .text_sm()
                                        .text_color(rgb(INK_MUTED))
                                        .child(empty_message),
                                )
                            })
                            .children(items.into_iter().map(|item| {
                                let id = item.id.clone();
                                let selected = selected_id.as_deref() == Some(id.as_str());
                                div()
                                    .id(SharedString::from(format!("node-{id}")))
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .px_2()
                                    .pl(px(8.0 + item.depth as f32 * 14.0))
                                    .py(px(10.0))
                                    .rounded_lg()
                                    .cursor_pointer()
                                    .when(selected, |row| {
                                        row.bg(rgb(ACCENT_SOFT)).text_color(rgb(ACCENT))
                                    })
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.open_node(id.clone(), window, cx);
                                    }))
                                    .child(
                                        div()
                                            .w(px(24.0))
                                            .h(px(24.0))
                                            .rounded_md()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .bg(rgb(if selected {
                                                SURFACE_RAISED
                                            } else {
                                                SURFACE
                                            }))
                                            .text_xs()
                                            .child(kind_icon(item.kind)),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .whitespace_nowrap()
                                            .child(item.title),
                                    )
                            })),
                    )
                    .when(has_selection, |sidebar| {
                        sidebar.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .pt_3()
                                .border_t_1()
                                .border_color(rgb(BORDER))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(INK_FAINT))
                                        .child("打开方式"),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_wrap()
                                        .gap_1()
                                        .child(
                                            Button::new("engine-code")
                                                .label("代码")
                                                .small()
                                                .ghost()
                                                .disabled(
                                                    self.selected_kind != Some(NodeKind::File),
                                                )
                                                .on_click(cx.listener(
                                                    |this, _, window, cx| {
                                                        this.set_engine_preference(
                                                            "ideall.code",
                                                            window,
                                                            cx,
                                                        );
                                                    },
                                                )),
                                        )
                                        .child(
                                            Button::new("engine-preview")
                                                .label("预览")
                                                .small()
                                                .ghost()
                                                .disabled(
                                                    self.selected_kind != Some(NodeKind::File),
                                                )
                                                .on_click(cx.listener(
                                                    |this, _, window, cx| {
                                                        this.set_engine_preference(
                                                            "ideall.preview",
                                                            window,
                                                            cx,
                                                        );
                                                    },
                                                )),
                                        )
                                        .child(
                                            Button::new("engine-browser")
                                                .label("浏览器")
                                                .small()
                                                .ghost()
                                                .disabled(
                                                    self.selected_kind != Some(NodeKind::Bookmark),
                                                )
                                                .on_click(cx.listener(
                                                    |this, _, window, cx| {
                                                        this.set_engine_preference(
                                                            "ideall.browser",
                                                            window,
                                                            cx,
                                                        );
                                                    },
                                                )),
                                        ),
                                ),
                        )
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .flex_col()
                    .bg(rgb(SURFACE))
                    .child(
                        div()
                            .h(px(54.0))
                            .flex()
                            .items_center()
                            .gap_1()
                            .px_3()
                            .border_b_1()
                            .border_color(rgb(BORDER))
                            .bg(rgb(SURFACE_RAISED))
                            .child(
                                div()
                                    .pr_3()
                                    .mr_1()
                                    .border_r_1()
                                    .border_color(rgb(BORDER))
                                    .text_xs()
                                    .text_color(rgb(INK_FAINT))
                                    .child("工作台"),
                            )
                            .children(tabs.into_iter().map(|tab| {
                                let id = tab_key(&tab);
                                let close_id = id.clone();
                                let selected = active_tab.as_deref() == Some(id.as_str());
                                div()
                                    .id(SharedString::from(format!("tab-{id}")))
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .px_3()
                                    .py_2()
                                    .rounded_lg()
                                    .cursor_pointer()
                                    .text_sm()
                                    .text_color(rgb(if selected { INK } else { INK_MUTED }))
                                    .when(selected, |item| {
                                        item.bg(rgb(ACCENT_SOFT)).text_color(rgb(ACCENT))
                                    })
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.activate_tab(id.clone(), window, cx);
                                    }))
                                    .child(tab.title)
                                    .child(
                                        Button::new(SharedString::from(format!(
                                            "close-{close_id}"
                                        )))
                                        .label("×")
                                        .small()
                                        .ghost()
                                        .on_click(
                                            cx.listener(move |this, _, window, cx| {
                                                this.close_tab(close_id.clone(), window, cx);
                                            }),
                                        ),
                                    )
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .p_6()
                            .gap_4()
                            .when(
                                !has_selection
                                    && !show_settings
                                    && !show_apps
                                    && !show_remote_webview,
                                |content| {
                                content
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        div()
                                            .w_full()
                                            .max_w(px(920.0))
                                            .flex()
                                            .flex_col()
                                            .gap_6()
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_end()
                                                    .justify_between()
                                                    .child(
                                                        div()
                                                            .max_w(px(620.0))
                                                            .flex()
                                                            .flex_col()
                                                            .gap_3()
                                                            .child(
                                                                div()
                                                                    .text_xs()
                                                                    .text_color(rgb(ACCENT))
                                                                    .child("YOUR INFORMATION, YOUR DEVICE"),
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_3xl()
                                                                    .child("把散落的信息，变成自己的工作台"),
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_color(rgb(INK_MUTED))
                                                                    .child("记录、收藏、阅读与 Agent 协作都从同一个本地空间开始。无需登录，断网也能继续。"),
                                                            ),
                                                    )
                                                    .child(
                                                        div()
                                                            .px_3()
                                                            .py_2()
                                                            .rounded_lg()
                                                            .bg(rgb(ACCENT_SOFT))
                                                            .text_sm()
                                                            .text_color(rgb(ACCENT))
                                                            .child(format!(
                                                                "{visible_item_count} 项本地内容"
                                                            )),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .gap_3()
                                                    .child(
                                                        div()
                                                            .id("welcome-new-note")
                                                            .flex_1()
                                                            .min_h(px(136.0))
                                                            .p_5()
                                                            .rounded_xl()
                                                            .border_1()
                                                            .border_color(rgb(BORDER))
                                                            .bg(rgb(SURFACE_RAISED))
                                                            .cursor_pointer()
                                                            .flex()
                                                            .flex_col()
                                                            .justify_between()
                                                            .on_click(cx.listener(
                                                                |this, _, window, cx| {
                                                                    this.create_note(window, cx);
                                                                },
                                                            ))
                                                            .child(
                                                                div()
                                                                    .text_2xl()
                                                                    .text_color(rgb(ACCENT))
                                                                    .child("＋"),
                                                            )
                                                            .child(
                                                                div()
                                                                    .flex()
                                                                    .flex_col()
                                                                    .gap_1()
                                                                    .child("写一条笔记")
                                                                    .child(
                                                                        div()
                                                                            .text_xs()
                                                                            .text_color(rgb(INK_MUTED))
                                                                            .child("700ms 自动保存到本地"),
                                                                    ),
                                                            ),
                                                    )
                                                    .child(
                                                        div()
                                                            .id("welcome-import")
                                                            .flex_1()
                                                            .min_h(px(136.0))
                                                            .p_5()
                                                            .rounded_xl()
                                                            .border_1()
                                                            .border_color(rgb(BORDER))
                                                            .bg(rgb(SURFACE_RAISED))
                                                            .cursor_pointer()
                                                            .flex()
                                                            .flex_col()
                                                            .justify_between()
                                                            .on_click(cx.listener(
                                                                |this, _, _, cx| {
                                                                    this.import_files(cx);
                                                                },
                                                            ))
                                                            .child(
                                                                div()
                                                                    .text_2xl()
                                                                    .text_color(rgb(ACCENT))
                                                                    .child("↥"),
                                                            )
                                                            .child(
                                                                div()
                                                                    .flex()
                                                                    .flex_col()
                                                                    .gap_1()
                                                                    .child("导入本地文件")
                                                                    .child(
                                                                        div()
                                                                            .text_xs()
                                                                            .text_color(rgb(INK_MUTED))
                                                                            .child("内容进入应用私有存储"),
                                                                    ),
                                                            ),
                                                    )
                                                    .child(
                                                        div()
                                                            .id("welcome-agent")
                                                            .flex_1()
                                                            .min_h(px(136.0))
                                                            .p_5()
                                                            .rounded_xl()
                                                            .border_1()
                                                            .border_color(rgb(BORDER))
                                                            .bg(rgb(RAIL))
                                                            .cursor_pointer()
                                                            .text_color(rgb(RAIL_INK))
                                                            .flex()
                                                            .flex_col()
                                                            .justify_between()
                                                            .on_click(cx.listener(
                                                                |this, _, window, cx| {
                                                                    this.select_section(
                                                                        "应用",
                                                                        window,
                                                                        cx,
                                                                    );
                                                                },
                                                            ))
                                                            .child(
                                                                div()
                                                                    .text_2xl()
                                                                    .text_color(rgb(ACCENT_ON_RAIL))
                                                                    .child("✦"),
                                                            )
                                                            .child(
                                                                div()
                                                                    .flex()
                                                                    .flex_col()
                                                                    .gap_1()
                                                                    .child("与 Agent 协作")
                                                                    .child(
                                                                        div()
                                                                            .text_xs()
                                                                            .text_color(rgb(RAIL_MUTED))
                                                                            .child("每次读写都经过授权与审计"),
                                                                    ),
                                                            ),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap_2()
                                                    .text_xs()
                                                    .text_color(rgb(INK_FAINT))
                                                    .child("● SQLite")
                                                    .child("● 系统凭据库")
                                                    .child("● 端到端加密同步")
                                                    .child("● GPUI 原生渲染"),
                                            ),
                                    )
                            },
                            )
                            .when(show_remote_webview, |content| {
                                content.child(
                                    remote_browser
                                        .clone()
                                        .expect("remote WebView is present when rendered"),
                                )
                            })
                            .when(show_apps, |content| {
                                content.child(
                                    div()
                                        .w_full()
                                        .max_w(px(820.0))
                                        .h_full()
                                        .flex()
                                        .flex_col()
                                        .gap_3()
                                        .overflow_y_scrollbar()
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .child(
                                                    div()
                                                        .flex()
                                                        .flex_col()
                                                        .gap_1()
                                                        .child(
                                                            div()
                                                                .text_xl()
                                                                .child("Agent 与原生 Engine"),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_sm()
                                                                .text_color(rgb(INK_MUTED))
                                                                .child("BYOK 由本机直连模型；默认工具可读取节点元数据并创建笔记，但不能读取既有笔记正文或 Blob。"),
                                                        ),
                                                )
                                                .child(
                                                    Button::new("agent-new-thread")
                                                        .label("新对话")
                                                        .disabled(self.agent_in_progress)
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.new_agent_thread(cx);
                                                        })),
                                                ),
                                        )
                                        .child(div().text_lg().child("模型连接（OpenAI-compatible）"))
                                        .child(div().text_sm().child("API 基址"))
                                        .child(Input::new(&self.agent_base_url_input).w_full())
                                        .child(div().text_sm().child("模型名称"))
                                        .child(Input::new(&self.agent_model_input).w_full())
                                        .child(div().text_sm().child(format!(
                                            "API Key：{}",
                                            if self.agent_key_configured {
                                                "已安全配置"
                                            } else {
                                                "未配置"
                                            }
                                        )))
                                        .child(Input::new(&self.agent_key_input).w_full())
                                        .child(
                                            div()
                                                .flex()
                                                .gap_2()
                                                .child(
                                                    Button::new("agent-save-settings")
                                                        .label("安全保存")
                                                        .on_click(cx.listener(
                                                            |this, _, window, cx| {
                                                                this.save_agent_model_settings(
                                                                    window, cx,
                                                                );
                                                            },
                                                        )),
                                                )
                                                .child(
                                                    Button::new("agent-clear-key")
                                                        .label("清除 Key")
                                                        .danger()
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.clear_agent_credential(cx);
                                                        })),
                                                )
                                                .child(
                                                    Button::new("agent-create-draft")
                                                        .label("MCP 冒烟：创建草稿")
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.create_agent_draft(cx);
                                                        })),
                                                ),
                                        )
                                        .child(div().text_lg().child("外部 ACP Agent（仅桌面）"))
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(rgb(DANGER))
                                                .child("只运行你在本机明确配置的程序。ACP 权限请求默认拒绝，但外部进程仍拥有当前系统账户与所选工作目录本身允许的访问权。"),
                                        )
                                        .child(div().text_sm().child("程序"))
                                        .child(Input::new(&self.acp_program_input).w_full())
                                        .child(div().text_sm().child("参数（argv 引号规则）"))
                                        .child(Input::new(&self.acp_args_input).w_full())
                                        .child(div().text_sm().child("工作目录（绝对路径）"))
                                        .child(Input::new(&self.acp_cwd_input).w_full())
                                        .child(
                                            div()
                                                .flex()
                                                .gap_2()
                                                .child(
                                                    Button::new("acp-save-settings")
                                                        .label("保存 ACP 设置")
                                                        .on_click(cx.listener(
                                                            |this, _, window, cx| {
                                                                this.save_external_acp_settings(
                                                                    window, cx,
                                                                );
                                                            },
                                                        )),
                                                )
                                                .child(
                                                    Button::new("acp-send")
                                                        .label(if self.agent_in_progress {
                                                            "Agent 处理中……"
                                                        } else {
                                                            "用外部 ACP 发送下方消息"
                                                        })
                                                        .disabled(self.agent_in_progress)
                                                        .on_click(cx.listener(
                                                            |this, _, window, cx| {
                                                                this.start_external_acp_turn(
                                                                    window, cx,
                                                                );
                                                            },
                                                        )),
                                                ),
                                        )
                                        .child(div().text_lg().child("本地对话"))
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
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.select_agent_thread(id.clone(), cx);
                                                    }))
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
                                                            .rounded_lg()
                                                            .bg(rgb(SURFACE_RAISED))
                                                            .text_color(rgb(INK_MUTED))
                                                            .child("输入消息开始对话"),
                                                    )
                                                })
                                                .children(agent_transcript.into_iter().map(
                                                    |message| {
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
                                                                    .child(agent_role_label(
                                                                        message.role,
                                                                    )),
                                                            )
                                                            .child(message.content)
                                                    },
                                                )),
                                        )
                                        .child(Input::new(&self.agent_prompt_input).w_full())
                                        .child(
                                            Button::new("agent-send")
                                                .label(if self.agent_in_progress {
                                                    "Agent 处理中……"
                                                } else {
                                                    "发送给 Agent"
                                                })
                                                .primary()
                                                .disabled(self.agent_in_progress)
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.start_agent_turn(window, cx);
                                                })),
                                        )
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(rgb(INK_MUTED))
                                                .child(
                                                    self.status
                                                        .clone()
                                                        .unwrap_or_else(|| "Agent 就绪".into()),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .text_lg()
                                                .child(format!(
                                                    "平台 Engine 能力（{}）",
                                                    engine_capabilities.len()
                                                )),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .flex_wrap()
                                                .gap_2()
                                                .children(engine_capabilities.into_iter().map(
                                                    |capability| {
                                                    div()
                                                        .w(px(250.0))
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
                                                                    capability.label,
                                                                    capability.engine_id
                                                                ))
                                                                .child(
                                                                    div()
                                                                        .text_sm()
                                                                        .text_color(rgb(
                                                                            if capability
                                                                                .kind
                                                                                .is_available()
                                                                            {
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
                                                        .child(capability.detail)
                                                },
                                                )),
                                        )
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
                                content.child(
                                    div()
                                        .w_full()
                                        .max_w(px(720.0))
                                        .h_full()
                                        .flex()
                                        .flex_col()
                                        .gap_3()
                                        .overflow_y_scrollbar()
                                        .child(div().text_xl().child("端到端加密同步"))
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(rgb(INK_MUTED))
                                                .child("服务端只保存 AES-GCM 密文；同步码与登录 Token 保存在系统安全存储。"),
                                        )
                                        .child(div().text_sm().child("服务端基址"))
                                        .child(Input::new(&self.sync_server_input).w_full())
                                        .child(
                                            div().text_sm().child(format!(
                                                "同步码：{}",
                                                if self.sync_code_configured {
                                                    "已配置"
                                                } else {
                                                    "未配置"
                                                }
                                            )),
                                        )
                                        .child(Input::new(&self.sync_code_input).w_full())
                                        .child(
                                            div().text_sm().child(format!(
                                                "登录 Token：{}",
                                                if self.sync_token_configured {
                                                    "已配置"
                                                } else {
                                                    "未配置"
                                                }
                                            )),
                                        )
                                        .child(Input::new(&self.sync_token_input).w_full())
                                        .child(
                                            div()
                                                .flex()
                                                .gap_2()
                                                .child(
                                                    Button::new("save-sync-settings")
                                                        .label("安全保存")
                                                        .on_click(cx.listener(
                                                            |this, _, window, cx| {
                                                                this.save_sync_settings(window, cx);
                                                            },
                                                        )),
                                                )
                                                .child(
                                                    Button::new("sync-notes-now")
                                                        .label(if self.sync_in_progress {
                                                            "同步中……"
                                                        } else {
                                                            "立即同步全部"
                                                        })
                                                        .primary()
                                                        .disabled(self.sync_in_progress)
                                                        .on_click(cx.listener(
                                                            |this, _, window, cx| {
                                                                this.start_notes_sync(window, cx);
                                                            },
                                                        )),
                                                )
                                                .child(
                                                    Button::new("clear-sync-secrets")
                                                        .label("清除凭据")
                                                        .danger()
                                                        .on_click(cx.listener(
                                                            |this, _, _, cx| {
                                                                this.clear_sync_credentials(cx);
                                                            },
                                                        )),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(rgb(INK_MUTED))
                                                .child(
                                                    self.status
                                                        .clone()
                                                        .unwrap_or_else(|| "尚未同步".into()),
                                                ),
                                        )
                                        .child(div().text_xl().child("原生版本与更新"))
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(rgb(INK_MUTED))
                                                .child(format!(
                                                    "当前版本 {}；只接受 ideall 官方 minisign 根签名的清单，安装器下载后还会校验大小与 SHA-256。",
                                                    env!("CARGO_PKG_VERSION")
                                                )),
                                        )
                                        .child(
                                            Button::new("check-native-update")
                                                .label(if self.update_in_progress {
                                                    "更新处理中……"
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
                                            let notes = update
                                                .notes
                                                .chars()
                                                .take(600)
                                                .collect::<String>();
                                            updates
                                                .child(
                                                    div()
                                                        .p_3()
                                                        .rounded_lg()
                                                        .border_1()
                                                        .border_color(rgb(BORDER))
                                                        .bg(rgb(SURFACE_RAISED))
                                                        .child(format!(
                                                            "可用版本 {} · {}\n{}",
                                                            update.version,
                                                            update.artifact.kind,
                                                            notes
                                                        )),
                                                )
                                                .child(
                                                    Button::new("download-native-update")
                                                        .label(format!(
                                                            "下载、校验并打开 {} 安装器",
                                                            update.version
                                                        ))
                                                        .primary()
                                                        .disabled(self.update_in_progress)
                                                        .on_click(cx.listener(
                                                            |this, _, _, cx| {
                                                                this.start_update_download(cx);
                                                            },
                                                        )),
                                                )
                                        })
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(rgb(INK_MUTED))
                                                .child(
                                                    self.update_status
                                                        .clone()
                                                        .unwrap_or_else(|| {
                                                            "尚未检查原生 Preview 更新".into()
                                                        }),
                                                ),
                                        ),
                                )
                            })
                            .when(has_selection, |content| {
                                content
                                    .child(Input::new(&self.title_input).w_full())
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_h_0()
                                            .flex()
                                            .flex_col()
                                            .when(show_browser, |body| {
                                                body.child(embedded_browser.clone().unwrap())
                                            })
                                            .when(show_code, |body| {
                                                body.child(
                                                    Input::new(&self.code_input)
                                                        .h_full()
                                                        .disabled(!self.body_editable),
                                                )
                                            })
                                            .when(show_preview, |body| {
                                                body.child(
                                                    div()
                                                        .flex_1()
                                                        .min_h_0()
                                                        .p_3()
                                                        .border_1()
                                                        .border_color(rgb(BORDER))
                                                        .rounded_md()
                                                        .child(preview),
                                                )
                                                .child(
                                                    Input::new(&self.body_input)
                                                        .h(px(220.0))
                                                        .disabled(!self.body_editable),
                                                )
                                            })
                                            .when(show_delegated_engine, |body| {
                                                let capability = delegated_capability
                                                    .expect("delegated capability is present");
                                                body.child(
                                                    div()
                                                        .flex_1()
                                                        .min_h_0()
                                                        .flex()
                                                        .flex_col()
                                                        .items_center()
                                                        .justify_center()
                                                        .gap_3()
                                                        .p_5()
                                                        .border_1()
                                                        .border_color(rgb(BORDER))
                                                        .rounded_md()
                                                        .child(
                                                            div()
                                                                .text_xl()
                                                                .child(format!(
                                                                    "{} · {}",
                                                                    capability.label,
                                                                    capability.kind.label()
                                                                )),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_color(rgb(INK_MUTED))
                                                                .child(capability.detail),
                                                        )
                                                        .child(
                                                            Button::new("open-delegated-file")
                                                                .label("用系统应用打开只读副本")
                                                                .primary()
                                                                .on_click(cx.listener(
                                                                    |this, _, _, cx| {
                                                                        this.open_file_with_system(cx);
                                                                    },
                                                                )),
                                                        ),
                                                )
                                            })
                                            .when(
                                                !show_browser
                                                    && !show_code
                                                    && !show_preview
                                                    && !show_delegated_engine,
                                                |body| {
                                                body.child(
                                                    Input::new(&self.body_input)
                                                        .h_full()
                                                        .disabled(
                                                            !is_body_editable
                                                                || !self.body_editable,
                                                        ),
                                                )
                                            },
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                div().text_sm().text_color(rgb(INK_MUTED)).child(
                                                    self.status
                                                        .clone()
                                                        .unwrap_or_else(|| "本地数据".into()),
                                                ),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .gap_2()
                                                    .child(
                                                        Button::new("save")
                                                            .label(if self.dirty {
                                                                "保存更改"
                                                            } else {
                                                                "已保存"
                                                            })
                                                            .primary()
                                                            .disabled(!self.dirty)
                                                            .on_click(cx.listener(
                                                                |this, _, window, cx| {
                                                                    this.save(window, cx);
                                                                },
                                                            )),
                                                    )
                                                    .when(
                                                        !self.show_trash
                                                            && self.selected_kind
                                                                == Some(NodeKind::Bookmark),
                                                        |actions| {
                                                            actions.child(
                                                                Button::new("open-bookmark")
                                                                    .label("系统浏览器打开")
                                                                    .on_click(cx.listener(
                                                                        |this, _, window, cx| {
                                                                            this.open_bookmark_external(
                                                                                window, cx,
                                                                            );
                                                                        },
                                                                    )),
                                                            )
                                                        },
                                                    )
                                                    .when(
                                                        !self.show_trash
                                                            && self.selected_kind
                                                                == Some(NodeKind::File),
                                                        |actions| {
                                                            actions.child(
                                                                Button::new("open-file-system")
                                                                    .label("系统应用打开")
                                                                    .on_click(cx.listener(
                                                                        |this, _, _, cx| {
                                                                            this.open_file_with_system(cx);
                                                                        },
                                                                    )),
                                                            )
                                                        },
                                                    )
                                                    .when(self.show_trash, |actions| {
                                                        actions
                                                            .child(
                                                                Button::new("restore")
                                                                    .label("恢复")
                                                                    .on_click(cx.listener(
                                                                        |this, _, window, cx| {
                                                                            this.restore_selected(
                                                                                window, cx,
                                                                            );
                                                                        },
                                                                    )),
                                                            )
                                                            .child(
                                                                Button::new("purge")
                                                                    .label("永久删除")
                                                                    .danger()
                                                                    .on_click(cx.listener(
                                                                        |this, _, window, cx| {
                                                                            this.confirm_purge(
                                                                                window, cx,
                                                                            );
                                                                        },
                                                                    )),
                                                            )
                                                    })
                                                    .when(!self.show_trash, |actions| {
                                                        actions.child(
                                                            Button::new("trash")
                                                                .label("移到回收站")
                                                                .danger()
                                                                .on_click(cx.listener(
                                                                    |this, _, window, cx| {
                                                                        this.move_selected_to_trash(
                                                                            window, cx,
                                                                        );
                                                                    },
                                                                )),
                                                        )
                                                    }),
                                            ),
                                    )
                            }),
                    ),
            )
    }
}
