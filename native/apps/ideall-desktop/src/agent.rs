use crate::*;

impl IdeallDesktop {
    pub(crate) fn load_agent_activity(&mut self, cx: &mut Context<Self>) {
        if self.agent_activity_loading {
            return;
        }
        let database_path = self.database_path.clone();
        let task = cx.background_executor().spawn(async move {
            LocalWorkspace::open(database_path)
                .and_then(|workspace| workspace.agent_activity())
                .map_err(|error| error.to_string())
        });
        self.agent_activity_loading = true;
        cx.spawn(async move |view, cx| {
            let result = task.await;
            cx.update(|cx| {
                let Some(view) = view.upgrade() else {
                    return;
                };
                view.update(cx, |this, cx| {
                    this.agent_activity_loading = false;
                    match result {
                        Ok(activity) => {
                            this.agent_audits = activity.audits;
                            this.agent_threads = activity.threads;
                        }
                        Err(error) => {
                            this.status = Some(format!("加载 Agent 活动失败：{error}").into());
                        }
                    }
                    cx.notify();
                });
            })
        })
        .detach();
    }

    pub(crate) fn persist_agent_model_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<PersistedAgentModelSettings, String> {
        let draft = AgentModelSettingsDraft {
            base_url: self.agent_base_url_input.read(cx).value().to_string(),
            model: self.agent_model_input.read(cx).value().to_string(),
            api_key: self.agent_key_input.read(cx).value().to_string(),
        };
        let persisted = self
            .service
            .persist_agent_model_settings(&draft, &self.secret_store)?;
        self.agent_base_url_input.update(cx, |input, cx| {
            input.set_value(persisted.settings.base_url.clone(), window, cx);
        });
        self.agent_model_input.update(cx, |input, cx| {
            input.set_value(persisted.settings.model.clone(), window, cx);
        });
        if persisted.api_key_saved {
            self.agent_key_configured = true;
            self.agent_key_input
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
        Ok(persisted)
    }

    pub(crate) fn save_agent_model_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.status = match self.persist_agent_model_settings(window, cx) {
            Ok(_) => Some(AGENT_MODEL_SETTINGS_SAVED_STATUS.into()),
            Err(error) => Some(error.into()),
        };
        cx.notify();
    }

    pub(crate) fn clear_agent_credential(&mut self, cx: &mut Context<Self>) {
        match self.secret_store.delete(CredentialKey::AgentCredential) {
            Ok(_) => {
                self.agent_key_configured = false;
                self.status = Some("模型 API Key 已从系统安全存储清除".into());
            }
            Err(error) => self.status = Some(error.to_string().into()),
        }
        cx.notify();
    }

    pub(crate) fn new_agent_thread(&mut self, cx: &mut Context<Self>) {
        if self.agent_in_progress {
            return;
        }
        self.agent_thread_id = None;
        self.agent_transcript.clear();
        self.status = Some(NEW_AGENT_THREAD_STATUS.into());
        cx.notify();
    }

    pub(crate) fn fill_agent_prompt(
        &mut self,
        prompt: &'static str,
        attach_document: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if attach_document && self.editor.has_selection() {
            self.agent_context_attached = true;
            self.status = Some("已附加当前文档；确认问题后按 Ctrl/⌘ Enter 发送".into());
        }
        self.agent_prompt_input.update(cx, |input, cx| {
            input.set_value(prompt, window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    pub(crate) fn toggle_agent_panel(&mut self, cx: &mut Context<Self>) {
        self.editor.workspace.agent.visible = !self.editor.workspace.agent.visible;
        if self.editor.workspace.agent.visible {
            self.load_agent_activity(cx);
        }
        self.persist_workspace();
        cx.notify();
    }

    pub(crate) fn focus_agent_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.agent_key_configured {
            self.select_section("应用", window, cx);
            return;
        }
        if !self.editor.workspace.agent.visible {
            self.editor.workspace.agent.visible = true;
            self.load_agent_activity(cx);
            self.persist_workspace();
        }
        self.agent_prompt_input
            .update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    pub(crate) fn toggle_agent_document_context(&mut self, cx: &mut Context<Self>) {
        if !self.editor.has_selection() {
            self.agent_context_attached = false;
            self.status = Some("请先打开一个文档".into());
        } else {
            self.agent_context_attached = !self.agent_context_attached;
            self.status = Some(if self.agent_context_attached {
                "当前文档将在本次 Agent 请求中作为参考；切换文档会自动移除".into()
            } else {
                "已从 Agent 请求中移除当前文档".into()
            });
        }
        cx.notify();
    }

    pub(crate) fn select_agent_thread(&mut self, thread_id: String, cx: &mut Context<Self>) {
        if self.agent_in_progress {
            return;
        }
        match self.service.agent_transcript(&thread_id) {
            Ok(messages) => {
                self.agent_thread_id = Some(thread_id);
                self.agent_transcript = messages;
                self.agent_scroll_handle.scroll_to_bottom();
                self.status = Some(AGENT_THREAD_OPENED_STATUS.into());
            }
            Err(error) => self.status = Some(error.to_string().into()),
        }
        cx.notify();
    }

    pub(crate) fn start_agent_turn(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.agent_in_progress {
            return;
        }
        let persisted = match self.persist_agent_model_settings(window, cx) {
            Ok(persisted) => persisted,
            Err(error) => {
                self.status = Some(error.into());
                cx.notify();
                return;
            }
        };
        let prompt = self.agent_prompt_input.read(cx).value().to_string();
        if prompt.trim().is_empty() {
            self.status = Some(AGENT_PROMPT_EMPTY_STATUS.into());
            cx.notify();
            return;
        }
        let key = match require_credential(&self.secret_store, CredentialKey::AgentCredential) {
            Ok(key) => key,
            Err(error) => {
                self.status = Some(error.into());
                cx.notify();
                return;
            }
        };
        let database_path = self.database_path.clone();
        let thread_id = self.agent_thread_id.clone();
        let document_context =
            (self.agent_context_attached && self.editor.has_selection()).then(|| {
                AgentDocumentContext {
                    id: self.editor.selected_id.clone().unwrap_or_default(),
                    title: self.editor.title_input.read(cx).value().to_string(),
                    content: self.editor.active_text(cx),
                }
            });
        self.agent_transcript.push(AgentTranscriptMessage {
            role: ModelRole::User,
            content: prompt.clone(),
        });
        self.agent_prompt_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.agent_scroll_handle.scroll_to_bottom();
        let settings = persisted.settings;
        let task = cx.background_executor().spawn(async move {
            let mut workspace =
                LocalWorkspace::open(database_path).map_err(|error| error.to_string())?;
            workspace
                .run_openai_agent_turn(
                    &settings,
                    &key,
                    thread_id.as_deref(),
                    &prompt,
                    document_context.as_ref(),
                )
                .map_err(|error| error.to_string())
        });
        self.agent_in_progress = true;
        self.status = Some(AGENT_TURN_IN_PROGRESS_STATUS.into());
        cx.spawn_in(window, async move |view, cx| {
            let result = task.await;
            let _ = view.update_in(cx, |this, _window, cx| {
                this.agent_in_progress = false;
                match result {
                    Ok(result) => {
                        this.agent_thread_id = Some(result.thread_id.clone());
                        this.agent_transcript = this
                            .service
                            .agent_transcript(&result.thread_id)
                            .unwrap_or_default();
                        this.agent_scroll_handle.scroll_to_bottom();
                        this.refresh_items();
                        this.status = Some(result.status_line().into());
                    }
                    Err(error) => {
                        if this.agent_thread_id.is_none() {
                            this.agent_thread_id =
                                this.service.latest_agent_thread_id().ok().flatten();
                        }
                        if let Some(thread_id) = this.agent_thread_id.as_deref() {
                            this.agent_transcript =
                                this.service.agent_transcript(thread_id).unwrap_or_default();
                        }
                        this.agent_scroll_handle.scroll_to_bottom();
                        this.status = Some(error.into());
                    }
                }
                this.load_agent_activity(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn persist_external_acp_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<ExternalAcpSettings, String> {
        let saved = self
            .service
            .save_external_acp_settings(&ExternalAcpSettings {
                program: self.acp_program_input.read(cx).value().to_string(),
                args: self.acp_args_input.read(cx).value().to_string(),
                cwd: self.acp_cwd_input.read(cx).value().to_string(),
            })
            .map_err(|error| error.to_string())?;
        self.acp_program_input.update(cx, |input, cx| {
            input.set_value(saved.program.clone(), window, cx);
        });
        self.acp_args_input.update(cx, |input, cx| {
            input.set_value(saved.args.clone(), window, cx);
        });
        self.acp_cwd_input.update(cx, |input, cx| {
            input.set_value(saved.cwd.clone(), window, cx);
        });
        Ok(saved)
    }

    pub(crate) fn save_external_acp_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.status = match self.persist_external_acp_settings(window, cx) {
            Ok(_) => Some("外部 ACP 命令已保存；程序与 argv 直接启动，不经 shell".into()),
            Err(error) => Some(error.into()),
        };
        cx.notify();
    }

    pub(crate) fn start_external_acp_turn(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.agent_in_progress {
            return;
        }
        let settings = match self.persist_external_acp_settings(window, cx) {
            Ok(settings) => settings,
            Err(error) => {
                self.status = Some(error.into());
                cx.notify();
                return;
            }
        };
        let config =
            match ExternalAcpConfig::parse(&settings.program, &settings.args, &settings.cwd) {
                Ok(config) => config,
                Err(error) => {
                    self.status = Some(error.to_string().into());
                    cx.notify();
                    return;
                }
            };
        if !command_exists(&config.program) {
            self.status = Some("未在本机找到外部 ACP 程序".into());
            cx.notify();
            return;
        }
        let prompt = self.agent_prompt_input.read(cx).value().to_string();
        if prompt.trim().is_empty() {
            self.status = Some("请输入 Agent 消息".into());
            cx.notify();
            return;
        }
        let database_path = self.database_path.clone();
        let existing_thread_id = self.agent_thread_id.clone();
        self.agent_transcript.push(AgentTranscriptMessage {
            role: ModelRole::User,
            content: prompt.clone(),
        });
        self.agent_prompt_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.agent_scroll_handle.scroll_to_bottom();
        let task = cx.background_executor().spawn(async move {
            let mut workspace =
                LocalWorkspace::open(database_path).map_err(|error| (None, error.to_string()))?;
            let thread_id = workspace
                .begin_external_agent_turn(existing_thread_id.as_deref(), &prompt)
                .map_err(|error| (None, error.to_string()))?;
            let result = run_external_acp(config, &prompt)
                .map_err(|error| (Some(thread_id.clone()), error.to_string()))?;
            let mut tools = result
                .tools
                .iter()
                .map(|tool| AgentToolRun {
                    name: format!("external-acp.{}", tool.id),
                    ok: tool.status == AcpToolStatus::Completed,
                    summary: format!("{} · {}", tool.title, acp_tool_status_label(tool.status)),
                })
                .collect::<Vec<_>>();
            if result.denied_permissions > 0 {
                tools.push(AgentToolRun {
                    name: "external-acp.permission".into(),
                    ok: false,
                    summary: format!("已拒绝 {} 个外部 Agent 权限请求", result.denied_permissions),
                });
            }
            workspace
                .complete_external_agent_turn(&thread_id, &result.content, &tools)
                .map_err(|error| (Some(thread_id.clone()), error.to_string()))?;
            Ok::<_, (Option<String>, String)>((thread_id, result))
        });
        self.agent_in_progress = true;
        self.status = Some("正在启动外部 ACP v1 Agent；权限请求默认拒绝……".into());
        cx.spawn_in(window, async move |view, cx| {
            let result = task.await;
            let _ = view.update_in(cx, |this, _window, cx| {
                this.agent_in_progress = false;
                match result {
                    Ok((thread_id, result)) => {
                        this.agent_thread_id = Some(thread_id.clone());
                        this.agent_transcript = this
                            .service
                            .agent_transcript(&thread_id)
                            .unwrap_or_default();
                        this.agent_scroll_handle.scroll_to_bottom();
                        this.status = Some(
                            format!(
                                "ACP {}：{} 个工具事件，{} 个权限请求被拒绝",
                                result.stop_reason,
                                result.tools.len(),
                                result.denied_permissions
                            )
                            .into(),
                        );
                    }
                    Err((thread_id, error)) => {
                        if let Some(thread_id) = thread_id {
                            this.agent_thread_id = Some(thread_id.clone());
                            this.agent_transcript = this
                                .service
                                .agent_transcript(&thread_id)
                                .unwrap_or_default();
                        }
                        this.agent_scroll_handle.scroll_to_bottom();
                        this.status = Some(error.into());
                    }
                }
                this.load_agent_activity(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
