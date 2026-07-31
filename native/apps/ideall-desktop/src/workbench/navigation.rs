use crate::*;

impl IdeallDesktop {
    pub(crate) fn schedule_search(&mut self, query: String, cx: &mut Context<Self>) {
        self.search_query = query;
        self.show_trash = false;
        let request = self.search_requests.start();
        drop(self.search_task.take());
        let query = self.search_query.trim().to_owned();
        if query.is_empty() {
            self.active_section = "我的".into();
            self.refresh_items();
            self.status = None;
            cx.notify();
            return;
        }

        self.active_section = "搜索".into();
        self.status = Some(SEARCH_IN_PROGRESS_STATUS.into());
        let database_path = self.database_path.clone();
        let search_kind = self.search_kind;
        let timer = cx.background_executor().timer(SEARCH_DEBOUNCE_DELAY);
        self.search_task = Some(cx.spawn(async move |view, cx| {
            timer.await;
            let query_for_search = query.clone();
            let search = cx.background_executor().spawn(async move {
                LocalWorkspace::search_at(database_path, &query_for_search, 200, search_kind)
                    .map_err(|error| error.to_string())
            });
            let result = search.await;
            let _ = cx.update(|cx| {
                let Some(view) = view.upgrade() else {
                    return;
                };
                view.update(cx, |this, cx| {
                    if !this.search_requests.is_current(request)
                        || this.search_query.trim() != query
                    {
                        return;
                    }
                    match result {
                        Ok(items) => {
                            let count = items.len();
                            this.explorer.replace_items(items);
                            this.status = Some(search_results_status(count).into());
                        }
                        Err(error) => {
                            this.status = Some(search_failed_status(&error).into());
                        }
                    }
                    cx.notify();
                });
            });
        }));
        cx.notify();
    }

    /// 切换搜索类型过滤并立即用当前查询重发搜索（复用同一防抖）。
    pub(crate) fn set_search_kind(&mut self, kind: Option<NodeKind>, cx: &mut Context<Self>) {
        self.search_kind = kind;
        if !self.search_query.trim().is_empty() {
            let query = self.search_query.clone();
            self.schedule_search(query, cx);
        } else {
            cx.notify();
        }
    }

    pub(crate) fn show_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_trash = false;
        self.search_query.clear();
        self.search_requests.invalidate();
        drop(self.search_task.take());
        self.suppress_search_events = true;
        self.search_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.suppress_search_events = false;
        self.active_section = "我的".into();
        self.explorer.clear_selection();
        self.refresh_items();
        if self.editor.selected_id.is_none()
            && let Some(file_id) =
                self.editor
                    .workspace
                    .active_id
                    .as_deref()
                    .and_then(|active_id| {
                        self.editor
                            .workspace
                            .tabs
                            .iter()
                            .find(|tab| tab_key(tab) == active_id)
                            .map(|tab| tab.file.file_id.clone())
                    })
        {
            self.open_node(file_id, window, cx);
        }
        cx.notify();
    }

    pub(crate) fn select_section(
        &mut self,
        section: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if section == "我的" {
            self.show_files(window, cx);
            return;
        }
        self.show_trash = false;
        self.search_query.clear();
        self.search_requests.invalidate();
        drop(self.search_task.take());
        self.suppress_search_events = true;
        self.search_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.suppress_search_events = false;
        self.active_section = section.into();
        self.explorer.clear_selection();
        self.clear_selection(window, cx);
        self.refresh_items();
        self.status = match section {
            "活动" => Some("按最近修改排序的本地活动".into()),
            "浏览" => Some("本地书签与关注源".into()),
            "应用" => {
                match credential_configured(&self.secret_store, CredentialKey::AgentCredential) {
                    Ok(configured) => self.agent_key_configured = configured,
                    Err(error) => self.status = Some(error.into()),
                }
                self.load_agent_activity(cx);
                Some("原生 Agent 可直连 OpenAI-compatible 模型，并通过授权 MCP 使用本地工具".into())
            }
            "设置" => {
                self.refresh_sync_secret_status();
                Some("同步凭据仅保存在系统安全存储中".into())
            }
            _ => None,
        };
        cx.notify();
    }

    pub(crate) fn show_trash(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_trash = true;
        self.search_query.clear();
        self.search_requests.invalidate();
        drop(self.search_task.take());
        self.suppress_search_events = true;
        self.search_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.suppress_search_events = false;
        self.active_section = "回收站".into();
        self.explorer.clear_selection();
        self.clear_selection(window, cx);
        self.refresh_items();
        cx.notify();
    }

    pub(crate) fn show_inbox(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.service.ensure_inbox() {
            Ok(_) => {
                self.show_trash = false;
                self.search_query.clear();
                self.search_requests.invalidate();
                drop(self.search_task.take());
                self.suppress_search_events = true;
                self.search_input
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.suppress_search_events = false;
                self.active_section = "收集箱".into();
                self.explorer.clear_selection();
                self.clear_selection(window, cx);
                self.refresh_items();
            }
            Err(error) => self.status = Some(error.to_string().into()),
        }
        cx.notify();
    }

    pub(crate) fn clear_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.detach_pending_autosave(cx);
        self.editor.clear(window, cx);
        self.browser.clear(window, cx);
    }

    pub(crate) fn toggle_project_panel(&mut self, cx: &mut Context<Self>) {
        self.explorer.toggle_panel();
        self.persist_workspace();
        cx.notify();
    }

    pub(crate) fn activate_tab(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(file_id) = self.editor.activate_tab(&id) {
            self.open_node(file_id, window, cx);
        }
        self.persist_workspace();
    }

    pub(crate) fn close_tab(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor.dirty && self.editor.workspace.active_id.as_deref() == Some(id.as_str()) {
            self.save(window, cx);
            if self.editor.dirty {
                return;
            }
        }
        let closed_file_id = self
            .editor
            .workspace
            .tabs
            .iter()
            .find(|tab| tab_key(tab) == id)
            .map(|tab| tab.file.file_id.clone());
        if !self.editor.workspace.close(&id) {
            return;
        }
        if let Some(file_id) = closed_file_id
            && !self
                .editor
                .workspace
                .tabs
                .iter()
                .any(|tab| tab.file.file_id == file_id)
        {
            self.editor.remove_session(&file_id);
        }
        self.persist_workspace();
        if let Some(active_id) = self.editor.workspace.active_id.clone()
            && let Some(tab) = self
                .editor
                .workspace
                .tabs
                .iter()
                .find(|tab| tab_key(tab) == active_id)
        {
            self.open_node(tab.file.file_id.clone(), window, cx);
        } else {
            self.clear_selection(window, cx);
        }
        cx.notify();
    }

    pub(crate) fn persist_workspace(&mut self) {
        self.editor.workspace.explorer = self.explorer.layout_state();
        if let Err(error) = self.service.save_workspace_state(&self.editor.workspace) {
            self.status = Some(error.to_string().into());
        }
    }
}
