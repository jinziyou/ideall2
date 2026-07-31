use crate::*;

impl IdeallDesktop {
    pub(crate) fn create_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let parent = self.creation_parent();
        match self.service.create_note(parent.as_deref(), "无标题笔记") {
            Ok(node) => {
                if let Some(parent) = parent {
                    self.explorer.reveal_folder(parent);
                }
                self.show_trash = false;
                self.refresh_items();
                self.open_node(node.base().id.clone(), window, cx);
                self.editor
                    .title_input
                    .update(cx, |input, cx| input.focus(window, cx));
                self.status = Some("已创建本地笔记".into());
            }
            Err(error) => self.status = Some(error.to_string().into()),
        }
        cx.notify();
    }

    pub(crate) fn create_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let parent = self.creation_parent();
        match self.service.create_folder(parent.as_deref(), "新建文件夹") {
            Ok(node) => {
                if let Some(parent) = parent {
                    self.explorer.reveal_folder(parent);
                }
                self.explorer.reveal_folder(node.base().id.clone());
                self.show_trash = false;
                self.refresh_items();
                self.explorer.begin_rename(
                    node.base().id.clone(),
                    node.base().title.clone(),
                    window,
                    cx,
                );
                self.persist_workspace();
                self.status = Some("输入文件夹名称并按 Enter".into());
            }
            Err(error) => self.status = Some(error.to_string().into()),
        }
        cx.notify();
    }

    pub(crate) fn create_bookmark(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.service.create_bookmark_with_inbox_fallback(
            self.creation_parent().as_deref(),
            "新建书签",
            "https://example.com/",
        ) {
            Ok(node) => {
                if let Some(parent) = node.base().parent_id.clone() {
                    self.explorer.reveal_folder(parent);
                }
                self.show_trash = false;
                self.refresh_items();
                self.open_node(node.base().id.clone(), window, cx);
                self.editor
                    .title_input
                    .update(cx, |input, cx| input.focus(window, cx));
                self.status = Some("请修改书签标题与网址后保存".into());
            }
            Err(error) => self.status = Some(error.to_string().into()),
        }
        cx.notify();
    }

    pub(crate) fn create_text_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let parent = self.creation_parent();
        match self
            .service
            .create_file(parent.as_deref(), "新建文本.txt", "text/plain", Vec::new())
        {
            Ok(node) => {
                if let Some(parent) = parent {
                    self.explorer.reveal_folder(parent);
                }
                self.show_trash = false;
                self.refresh_items();
                self.open_node(node.base().id.clone(), window, cx);
                self.editor
                    .title_input
                    .update(cx, |input, cx| input.focus(window, cx));
                self.status = Some("已创建本地文件与 Blob".into());
            }
            Err(error) => self.status = Some(error.to_string().into()),
        }
        cx.notify();
    }

    pub(crate) fn create_feed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let key = self.feed_key_input.read(cx).value().to_string();
        match self
            .service
            .create_feed(key.trim(), SubscriptionType::Publisher, &key)
        {
            Ok(node) => {
                self.feed_key_input
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.refresh_items();
                self.status = Some(
                    format!(
                        "已关注发布者 {}；可通过加密同步与旧客户端互通",
                        node.base().title
                    )
                    .into(),
                );
            }
            Err(error) => self.status = Some(error.to_string().into()),
        }
        cx.notify();
    }

    pub(crate) fn import_files(&mut self, cx: &mut Context<Self>) {
        let parent = self.creation_parent();
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("导入到 ideall".into()),
        });
        cx.spawn(async move |view, cx| {
            let selection = receiver.await;
            let _ = cx.update(|cx| {
                let Some(view) = view.upgrade() else {
                    return;
                };
                view.update(cx, |this, cx| match selection {
                    Ok(Ok(Some(paths))) => {
                        let mut imported = 0_usize;
                        for path in paths {
                            match this.service.import_file(
                                parent.as_deref(),
                                path,
                                MAX_IMPORT_BYTES,
                            ) {
                                Ok(_) => imported += 1,
                                Err(error) => {
                                    this.status = Some(error.to_string().into());
                                    this.refresh_items();
                                    cx.notify();
                                    return;
                                }
                            }
                        }
                        if let Some(parent) = parent {
                            this.explorer.reveal_folder(parent);
                        }
                        this.refresh_items();
                        this.status = Some(format!("已导入 {imported} 个文件").into());
                        cx.notify();
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => {
                        this.status = Some(error.to_string().into());
                        cx.notify();
                    }
                    Err(_) => {
                        this.status = Some("文件选择器意外关闭".into());
                        cx.notify();
                    }
                });
            });
        })
        .detach();
    }

    pub(crate) fn creation_parent(&self) -> Option<String> {
        self.explorer.selected_folder_id().or_else(|| {
            (self.editor.selected_kind == Some(NodeKind::Folder))
                .then(|| self.editor.selected_id.clone())
                .flatten()
        })
    }

    pub(crate) fn move_explorer_selection_to_trash(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.explorer.selection_id.clone() else {
            return;
        };
        if self.editor.selected_id.as_deref() == Some(id.as_str()) && self.editor.dirty {
            self.save(window, cx);
            if self.editor.dirty {
                return;
            }
        }
        match self.service.move_to_trash(&id) {
            Ok(true) => {
                let tab_ids = self
                    .editor
                    .workspace
                    .tabs
                    .iter()
                    .filter(|tab| tab.file.file_id == id)
                    .map(tab_key)
                    .collect::<Vec<_>>();
                for tab_id in tab_ids {
                    self.editor.workspace.close(&tab_id);
                }
                if self.editor.selected_id.as_deref() == Some(id.as_str()) {
                    self.clear_selection(window, cx);
                }
                self.explorer.clear_selection();
                self.refresh_items();
                self.persist_workspace();
                self.status = Some("已移到回收站".into());
            }
            Ok(false) => self.status = Some("项目已不在当前列表".into()),
            Err(error) => self.status = Some(error.to_string().into()),
        }
        cx.notify();
    }

    pub(crate) fn restore_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.editor.selected_id.clone() else {
            return;
        };
        match self.service.restore(&id) {
            Ok(true) => {
                self.clear_selection(window, cx);
                self.refresh_items();
                self.status = Some("已从回收站恢复".into());
            }
            Ok(false) => self.status = Some("未找到回收站快照".into()),
            Err(error) => self.status = Some(error.to_string().into()),
        }
        cx.notify();
    }

    pub(crate) fn purge_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.editor.selected_id.clone() else {
            return;
        };
        match self.service.purge(&id) {
            Ok(true) => {
                self.clear_selection(window, cx);
                self.refresh_items();
                self.status = Some("已永久删除；此操作无法撤销".into());
            }
            Ok(false) => self.status = Some("项目已不在回收站".into()),
            Err(error) => self.status = Some(error.to_string().into()),
        }
        cx.notify();
    }

    pub(crate) fn confirm_purge(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = window.prompt(
            PromptLevel::Critical,
            "永久删除这个项目？",
            Some("本地节点和关联 Blob 都会删除，此操作无法撤销。"),
            &[PromptButton::cancel("取消"), PromptButton::ok("永久删除")],
            cx,
        );
        cx.spawn_in(window, async move |view, cx| {
            if receiver.await == Ok(1) {
                let _ = view.update_in(cx, |this, window, cx| {
                    this.purge_selected(window, cx);
                });
            }
        })
        .detach();
    }

    pub(crate) fn toggle_folder(&mut self, id: String, cx: &mut Context<Self>) {
        self.explorer.toggle_folder(id);
        self.persist_workspace();
        cx.notify();
    }

    pub(crate) fn begin_explorer_rename(
        &mut self,
        id: String,
        title: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.explorer.begin_rename(id, title, window, cx);
    }

    pub(crate) fn begin_selected_explorer_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selected_id) = self.explorer.selection_id.clone() else {
            return;
        };
        let Some(item) = self
            .explorer
            .items
            .iter()
            .find(|item| item.id == selected_id)
        else {
            return;
        };
        self.begin_explorer_rename(item.id.clone(), item.title.clone(), window, cx);
    }

    pub(crate) fn open_selected_explorer_node(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selected_id) = self.explorer.selection_id.clone() else {
            return;
        };
        let is_folder = self
            .explorer
            .items
            .iter()
            .find(|item| item.id == selected_id)
            .is_some_and(|item| item.kind == NodeKind::Folder);
        if is_folder {
            self.toggle_folder(selected_id, cx);
        } else {
            self.open_node(selected_id, window, cx);
        }
    }

    pub(crate) fn select_relative_explorer_node(&mut self, offset: isize, cx: &mut Context<Self>) {
        let preserve_full_list = self.active_section.as_ref() != "我的";
        if self.explorer.select_relative(preserve_full_list, offset) {
            cx.notify();
        }
    }

    pub(crate) fn cancel_explorer_rename(&mut self, cx: &mut Context<Self>) {
        self.explorer.cancel_rename();
        cx.notify();
    }

    pub(crate) fn commit_explorer_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.explorer.renaming_id.clone() else {
            return;
        };
        let Some(rename_input) = self.explorer.rename_input.clone() else {
            return;
        };
        let title = rename_input.read(cx).value().trim().to_owned();
        match self.service.rename(&id, title.clone()) {
            Ok(_) => {
                self.explorer.cancel_rename();
                if self.editor.selected_id.as_deref() == Some(id.as_str()) {
                    self.editor.suppress_events = true;
                    self.editor
                        .title_input
                        .update(cx, |input, cx| input.set_value(title.clone(), window, cx));
                    self.editor.suppress_events = false;
                    if self.editor.selected_kind == Some(NodeKind::File) {
                        let language = text_highlight_for_node(&self.service, &id);
                        self.editor
                            .text_input
                            .update(cx, |input, cx| input.set_highlighter(language, cx));
                    }
                }
                for tab in &mut self.editor.workspace.tabs {
                    if tab.file.file_id == id {
                        tab.title = title.clone();
                    }
                }
                self.refresh_items();
                self.persist_workspace();
                self.status = Some("名称已更新".into());
            }
            Err(error) => self.status = Some(error.to_string().into()),
        }
        cx.notify();
    }

    pub(crate) fn move_explorer_node(
        &mut self,
        id: String,
        parent_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        match self.service.move_node(&id, parent_id.as_deref()) {
            Ok(_) => {
                if let Some(parent_id) = parent_id {
                    self.explorer.reveal_folder(parent_id);
                }
                self.refresh_items();
                self.persist_workspace();
                self.status = Some("项目已移动".into());
            }
            Err(error) => self.status = Some(error.to_string().into()),
        }
        cx.notify();
    }
}
