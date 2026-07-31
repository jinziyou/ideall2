use crate::*;

impl IdeallDesktop {
    pub(crate) fn load_node_into_editor(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<(NodeKind, String)> {
        match self.service.node(id) {
            Ok(node) => {
                if self.editor.selected_id.as_deref() != Some(id) {
                    self.agent_context_attached = false;
                }
                self.detach_pending_autosave(cx);
                let kind = node.kind();
                let title = node.base().title.clone();
                let (body, editable, protected_blocks) = self.service.node_body(&node);
                let language = text_highlight_for_node(&self.service, id);
                self.editor.load_document(
                    LoadedDocument {
                        id: id.to_owned(),
                        kind,
                        title: title.clone(),
                        body,
                        language,
                        editable,
                    },
                    window,
                    cx,
                );
                self.note_backlinks = if kind == NodeKind::Note {
                    self.service.note_backlinks(id, 32).unwrap_or_default()
                } else {
                    Vec::new()
                };
                self.status = if kind == NodeKind::Note && protected_blocks > 0 {
                    Some(
                        format!(
                            "可编辑常用 Markdown 块；{protected_blocks} 个未知富文本块以指纹占位保护，请勿修改占位行"
                        )
                        .into(),
                    )
                } else if kind == NodeKind::File && !editable {
                    Some("此二进制类型在 ideall 中只读；可导出临时副本并用系统应用打开".into())
                } else {
                    None
                };
                Some((kind, title))
            }
            Err(error) => {
                self.note_backlinks.clear();
                self.status = Some(error.to_string().into());
                None
            }
        }
    }

    pub(crate) fn open_node(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.explorer.select(id.clone());
        if self.editor.dirty && self.editor.selected_id.as_deref() == Some(id.as_str()) {
            return;
        }
        if self.editor.dirty {
            self.save(window, cx);
            if self.editor.dirty {
                return;
            }
        }
        if let Some((kind, title)) = self.load_node_into_editor(&id, window, cx)
            && !self.show_trash
        {
            let engine_id = self
                .service
                .file_metadata(&id)
                .ok()
                .and_then(|file| {
                    resolve_default_engine(
                        &self.editor.engines,
                        &file,
                        &self.editor.engine_preferences,
                    )
                })
                .map(|resolution| resolution.candidate.descriptor.engine_id)
                .unwrap_or_else(|| engine_id(kind).into());
            self.activate_engine(&engine_id, &id, window, cx);
            self.editor.restore_active_session(window, cx);
            self.editor.workspace.open(TabDescriptor {
                file: FileRef::new("local.nodes", &id),
                engine_id,
                title,
                root_id: Some(HOME_ROOT_ID.into()),
                navigation_path: Some(format!("/home/{id}")),
            });
            self.persist_workspace();
        }
        cx.notify();
    }

    pub(crate) fn activate_engine(
        &mut self,
        engine_id: &str,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor.active_engine_id = Some(engine_id.to_owned());
        self.browser.clear(window, cx);
        if engine_id != "ideall.browser" {
            return;
        }
        let url = match self.service.node(id) {
            Ok(Node::Bookmark { content, .. }) => content.url,
            _ => return,
        };
        self.navigate_browser_to(&url, true, window, cx);
    }

    pub(crate) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.editor.draft(cx) else {
            return;
        };
        self.editor.autosave_task.take();
        let result = self.service.save_edits(
            &draft.id,
            draft.title,
            draft.body_editable.then_some(draft.body.as_str()),
        );
        match result {
            Ok(node) => {
                let summary = NodeSummary::from(&node);
                self.editor.dirty = false;
                self.refresh_items();
                for tab in &mut self.editor.workspace.tabs {
                    if tab.file.file_id == draft.id {
                        tab.title = summary.title.clone();
                    }
                }
                if let Node::Bookmark { content, .. } = node {
                    self.editor.suppress_events = true;
                    self.editor.body_input.update(cx, |input, cx| {
                        input.set_value(content.url.clone(), window, cx);
                    });
                    self.editor.suppress_events = false;
                    // 手动编辑保存的书签同样后台补全元数据；apply 只替换占位标题，
                    // 用户改过的标题不会被覆盖，抓取失败完全静默。
                    self.fetch_bookmark_metadata(draft.id.clone(), content.url.clone(), cx);
                }
                if self.editor.selected_kind == Some(NodeKind::File) {
                    let language = text_highlight_for_node(&self.service, &draft.id);
                    self.editor
                        .text_input
                        .update(cx, |input, cx| input.set_highlighter(language, cx));
                }
                self.persist_workspace();
                self.status = Some("已保存到本地 SQLite".into());
            }
            Err(error) => self.status = Some(error.to_string().into()),
        }
        cx.notify();
    }

    pub(crate) fn mark_editor_dirty(&mut self, cx: &mut Context<Self>) {
        if self.show_trash || self.editor.mark_dirty().is_none() {
            return;
        }
        self.schedule_draft_save(DRAFT_AUTOSAVE_DELAY, cx);
        cx.notify();
    }

    pub(crate) fn schedule_draft_save(&mut self, delay: Duration, cx: &mut Context<Self>) {
        if !self.editor.dirty {
            return;
        }
        let Some(draft) = self.editor.draft(cx) else {
            return;
        };
        let revision = self.editor.draft_revision;
        let id = draft.id;
        let title = draft.title;
        let body = draft.body_editable.then_some(draft.body);
        let database_path = self.database_path.clone();
        let timer = cx.background_executor().timer(delay);
        self.editor.autosave_task = Some(cx.spawn(async move |view, cx| {
            timer.await;
            let save_task = cx.background_executor().spawn(async move {
                LocalWorkspace::save_draft_at(database_path, &id, title, body.as_deref())
                    .map(|summary| (id, summary))
                    .map_err(|error| error.to_string())
            });
            let result = save_task.await;
            let _ = cx.update(|cx| {
                let Some(view) = view.upgrade() else {
                    return;
                };
                view.update(cx, |this, cx| {
                    if this.editor.draft_revision != revision {
                        if let Err(error) = &result {
                            this.status = Some(format!("上一项草稿自动保存失败：{error}").into());
                            cx.notify();
                        }
                        return;
                    }
                    match result {
                        Ok((id, summary)) => {
                            if this.editor.selected_id.as_deref() != Some(id.as_str()) {
                                return;
                            }
                            if let Some(item) =
                                this.explorer.items.iter_mut().find(|item| item.id == id)
                            {
                                item.title = summary.title.clone();
                                item.updated_at = summary.updated_at;
                            }
                            for tab in &mut this.editor.workspace.tabs {
                                if tab.file.file_id == id {
                                    tab.title = summary.title.clone();
                                }
                            }
                            if this.editor.selected_kind == Some(NodeKind::File) {
                                let language = text_highlight_for_node(&this.service, &id);
                                this.editor
                                    .text_input
                                    .update(cx, |input, cx| input.set_highlighter(language, cx));
                            }
                            this.persist_workspace();
                            this.editor.dirty = false;
                            this.status = Some("已自动保存到本地 SQLite".into());
                        }
                        Err(error) => {
                            this.status = Some(format!("自动保存失败：{error}").into());
                        }
                    }
                    cx.notify();
                });
            });
        }));
    }

    pub(crate) fn detach_pending_autosave(&mut self, cx: &mut Context<Self>) {
        if self.editor.dirty {
            self.schedule_draft_save(Duration::ZERO, cx);
        }
        self.editor.draft_revision = self.editor.draft_revision.wrapping_add(1);
        if let Some(task) = self.editor.autosave_task.take() {
            task.detach();
        }
    }

    pub(crate) fn window_activation_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !window.is_window_active() {
            self.schedule_draft_save(Duration::ZERO, cx);
        }
    }

    pub(crate) fn show_editor_find(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.editor.has_selection() {
            self.status = Some("请先打开一个文件或笔记".into());
            cx.notify();
            return;
        }
        if browser_surface(
            self.editor.active_engine_id.as_deref(),
            self.editor.selected_kind,
        )
        .is_some()
        {
            self.status = Some("浏览器查找与文档查找相互独立".into());
            cx.notify();
            return;
        }
        if self.editor.selected_kind == Some(NodeKind::Note) && self.editor.note_preview {
            self.editor.note_preview = false;
            self.editor.set_note_rich_inputs_read_only(false, cx);
        }
        self.editor.find_visible = true;
        self.editor.replace_visible = replace;
        self.editor.find_input.update(cx, |input, cx| {
            input.focus(window, cx);
        });
        cx.notify();
    }

    pub(crate) fn hide_editor_find(&mut self, cx: &mut Context<Self>) {
        self.editor.find_visible = false;
        self.editor.replace_visible = false;
        self.editor.find_cursor = 0;
        cx.notify();
    }

    pub(crate) fn toggle_note_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor.selected_kind != Some(NodeKind::Note) {
            return;
        }
        self.editor.note_preview = !self.editor.note_preview;
        self.editor
            .set_note_rich_inputs_read_only(self.editor.note_preview, cx);
        if self.editor.note_preview {
            self.status = Some("阅读视图 · 内容仍会自动保存到本机".into());
        } else {
            self.editor.focus_active_editor(window, cx);
            self.status = Some("块编辑视图 · 输入 / 插入内容块".into());
        }
        cx.notify();
    }

    pub(crate) fn find_next_in_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.editor.find_input.read(cx).value().to_string();
        if query.is_empty() {
            self.status = Some("请输入查找内容".into());
            cx.notify();
            return;
        }
        let text = self.editor.active_text(cx);
        let start = self.editor.find_cursor.min(text.len());
        let found = text[start..]
            .find(&query)
            .map(|offset| start + offset)
            .or_else(|| text[..start].find(&query));
        let Some(offset) = found else {
            self.status = Some("当前文件中没有匹配项".into());
            cx.notify();
            return;
        };
        self.editor.find_cursor = offset + query.len();
        let position =
            if self.editor.selected_kind == Some(NodeKind::Note) && self.editor.has_note_blocks() {
                self.editor
                    .focus_note_match(offset, window, cx)
                    .unwrap_or_else(|| position_for_byte_offset(&text, offset))
            } else {
                let position = position_for_byte_offset(&text, offset);
                self.editor.active_input().update(cx, |input, cx| {
                    input.set_cursor_position(position, window, cx)
                });
                position
            };
        self.status = Some(format!("第 {} 行找到匹配项", position.line.saturating_add(1)).into());
        cx.notify();
    }

    pub(crate) fn replace_all_in_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.editor.body_editable || self.show_trash {
            self.status = Some("当前内容为只读，不能替换".into());
            cx.notify();
            return;
        }
        let query = self.editor.find_input.read(cx).value().to_string();
        if query.is_empty() {
            self.status = Some("请输入查找内容".into());
            cx.notify();
            return;
        }
        let replacement = self.editor.replace_input.read(cx).value().to_string();
        let text = self.editor.active_text(cx);
        let count =
            if self.editor.selected_kind == Some(NodeKind::Note) && self.editor.has_note_blocks() {
                self.editor
                    .replace_all_note_blocks(&query, &replacement, window, cx)
            } else {
                text.match_indices(&query).count()
            };
        if count == 0 {
            self.status = Some("当前文件中没有匹配项".into());
            cx.notify();
            return;
        }
        if self.editor.selected_kind != Some(NodeKind::Note) || !self.editor.has_note_blocks() {
            let replaced = text.replace(&query, &replacement);
            self.editor.suppress_events = true;
            if self.editor.uses_text_input() {
                self.editor
                    .text_input
                    .update(cx, |input, cx| input.set_value(replaced, window, cx));
            } else {
                self.editor
                    .body_input
                    .update(cx, |input, cx| input.set_value(replaced, window, cx));
            }
            self.editor.suppress_events = false;
        }
        self.editor.find_cursor = 0;
        self.mark_editor_dirty(cx);
        self.status = Some(format!("已替换 {count} 处，等待自动保存").into());
        cx.notify();
    }

    pub(crate) fn set_engine_preference(
        &mut self,
        engine_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editor.dirty {
            self.save(window, cx);
            if self.editor.dirty {
                return;
            }
        }
        let previous_tab_id = self.editor.workspace.active_id.clone();
        let Some(id) = self.editor.selected_id.clone() else {
            return;
        };
        let result = self.service.file_metadata(&id).and_then(|file| {
            let supported = list_matching_engines(&self.editor.engines, &file)
                .iter()
                .any(|candidate| candidate.descriptor.engine_id == engine_id);
            if !supported {
                return Err(ideall_application::ApplicationError::EngineNotSupported {
                    engine_id: engine_id.into(),
                    media_type: file.media_type,
                });
            }
            let key = file.r#ref.to_key().map_err(|error| {
                ideall_application::ApplicationError::InvalidFileRef(error.to_string())
            })?;
            self.editor
                .engine_preferences
                .files
                .insert(key, engine_id.to_owned());
            self.service
                .save_engine_preferences(&self.editor.engine_preferences)
        });
        match result {
            Ok(()) => {
                self.open_node(id, window, cx);
                if let (Some(previous_tab_id), Some(active_tab_id)) =
                    (previous_tab_id, self.editor.workspace.active_id.as_deref())
                    && previous_tab_id != active_tab_id
                {
                    self.editor.workspace.close(&previous_tab_id);
                    self.persist_workspace();
                }
                self.status = Some(format!("默认使用 {engine_id}").into());
            }
            Err(error) => self.status = Some(error.to_string().into()),
        }
        cx.notify();
    }
}
