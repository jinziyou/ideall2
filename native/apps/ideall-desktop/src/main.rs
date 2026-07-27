mod actions;
mod view;

use std::{path::PathBuf, time::Duration};

use directories::ProjectDirs;
use gpui::{
    App, Application, Context, Entity, IntoElement, PathPromptOptions, PromptButton, PromptLevel,
    Render, SharedString, Subscription, Task, TitlebarOptions, Window, WindowBounds, WindowOptions,
    div, prelude::*, px, rgb, size,
};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use gpui_component::webview::WebView;
use gpui_component::{
    Disableable as _, Root, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    input::{Input, InputEvent, InputState},
    scroll::ScrollableElement as _,
    text::TextView,
};
use ideall_acp::{AcpToolStatus, ExternalAcpConfig, command_exists, run_external_acp};
use ideall_application::{
    AgentAuditRecord, AgentModelSettings, AgentToolRun, AgentTranscriptMessage, AuditStatus,
    ExternalAcpSettings, HOME_ROOT_ID, LocalWorkspace, ModelRole, NodeSummary,
    OpenAiCompatibleClient, SyncSettings,
};
use ideall_domain::{
    EnginePlatform, EnginePreferences, EngineRuntimeKind, TabDescriptor, WorkspaceState,
    builtin_engines, engine_runtime_capabilities, list_matching_engines, resolve_default_engine,
    tab_key,
};
use ideall_presentation::{
    LatestRequest, NAVIGATION,
    color::{
        ACCENT, ACCENT_ON_RAIL, ACCENT_SOFT, BORDER, CANVAS, DANGER, DANGER_BORDER, INK, INK_FAINT,
        INK_MUTED, MESSAGE_TOOL, RAIL, RAIL_ACTIVE, RAIL_INK, RAIL_MUTED, SURFACE, SURFACE_MUTED,
        SURFACE_RAISED, WARNING_SOFT,
    },
    section_description, section_eyebrow,
};
use ideall_protocol::{EngineAccess, EngineDescriptor, FileRef, Node, NodeKind, SubscriptionType};
use ideall_secrets::{SecretKey, SecretStore as _, SystemSecretStore};
use ideall_sync::{is_valid_sync_code, normalize_sync_code};
use ideall_sync_http::HttpSyncTransport;
use ideall_updater::{AvailableUpdate, UpdateStatus, updater_from_environment};

const INFO_PORTAL_URL: &str = "https://www.wonita.link/info";
const COMMUNITY_PORTAL_URL: &str = "https://www.wonita.link/community";
const DRAFT_AUTOSAVE_DELAY: Duration = Duration::from_millis(700);
const SEARCH_DEBOUNCE_DELAY: Duration = Duration::from_millis(180);

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
struct WebView;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl Render for WebView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

struct DesktopStartupFailure {
    message: String,
}

impl Render for DesktopStartupFailure {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(CANVAS))
            .child(
                div()
                    .max_w(px(620.))
                    .p_8()
                    .rounded_xl()
                    .border_1()
                    .border_color(rgb(DANGER_BORDER))
                    .bg(rgb(SURFACE_RAISED))
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .text_2xl()
                            .text_color(rgb(DANGER))
                            .child("ideall 无法启动"),
                    )
                    .child(div().text_color(rgb(INK)).child(self.message.clone()))
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(INK_MUTED))
                            .child("请检查数据目录权限、磁盘空间与系统安全存储后重新启动。"),
                    ),
            )
    }
}

struct IdeallDesktop {
    database_path: PathBuf,
    service: LocalWorkspace,
    secret_store: SystemSecretStore,
    engines: Vec<EngineDescriptor>,
    engine_preferences: EnginePreferences,
    workspace: WorkspaceState,
    items: Vec<NodeSummary>,
    selected_id: Option<String>,
    selected_kind: Option<NodeKind>,
    active_section: SharedString,
    show_trash: bool,
    search_query: String,
    search_requests: LatestRequest,
    search_task: Option<Task<()>>,
    suppress_search_events: bool,
    search_input: Entity<InputState>,
    feed_key_input: Entity<InputState>,
    title_input: Entity<InputState>,
    body_input: Entity<InputState>,
    code_input: Entity<InputState>,
    active_engine_id: Option<String>,
    embedded_browser: Option<Entity<WebView>>,
    sync_server_input: Entity<InputState>,
    sync_code_input: Entity<InputState>,
    sync_token_input: Entity<InputState>,
    agent_base_url_input: Entity<InputState>,
    agent_model_input: Entity<InputState>,
    agent_key_input: Entity<InputState>,
    agent_prompt_input: Entity<InputState>,
    acp_program_input: Entity<InputState>,
    acp_args_input: Entity<InputState>,
    acp_cwd_input: Entity<InputState>,
    sync_code_configured: bool,
    sync_token_configured: bool,
    sync_in_progress: bool,
    update_in_progress: bool,
    available_update: Option<AvailableUpdate>,
    update_status: Option<SharedString>,
    agent_key_configured: bool,
    agent_in_progress: bool,
    agent_thread_id: Option<String>,
    agent_transcript: Vec<AgentTranscriptMessage>,
    agent_audits: Vec<AgentAuditRecord>,
    agent_threads: Vec<NodeSummary>,
    agent_activity_loading: bool,
    body_editable: bool,
    suppress_editor_events: bool,
    draft_revision: u64,
    autosave_task: Option<Task<()>>,
    dirty: bool,
    status: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl IdeallDesktop {
    fn new(
        database_path: PathBuf,
        service: LocalWorkspace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("搜索标题与正文（Ctrl/⌘ K）"));
        let feed_key_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("发布者域名，例如 example.com"));
        let title_input = cx.new(|cx| InputState::new(window, cx).placeholder("标题"));
        let body_input = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .rows(12)
                .placeholder("从这里开始记录……")
        });
        let code_input = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("markdown")
                .placeholder("文件内容")
        });
        let sync_server = service.load_sync_settings().unwrap_or_default();
        let sync_server_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(sync_server.server_base_url)
                .placeholder("https://api.wonita.link")
        });
        let sync_code_input = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder("32 位十六进制同步码；留空保持不变")
        });
        let sync_token_input = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder("登录 Bearer Token；留空保持不变")
        });
        let agent_settings = service.load_agent_model_settings().unwrap_or_default();
        let agent_base_url_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(agent_settings.base_url)
                .placeholder("https://api.deepseek.com/v1/")
        });
        let agent_model_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(agent_settings.model)
                .placeholder("deepseek-chat")
        });
        let agent_key_input = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder("API Key；留空保持不变")
        });
        let agent_prompt_input = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .rows(4)
                .placeholder("向本地 Agent 提问，或让它调用已授权工具……")
        });
        let acp_settings = service.load_external_acp_settings().unwrap_or_default();
        let acp_program_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(acp_settings.program)
                .placeholder("例如 codex-acp、gemini 或 node")
        });
        let acp_args_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(acp_settings.args)
                .placeholder("例如 acp；按 argv 引号规则解析，不经 shell")
        });
        let acp_cwd_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(acp_settings.cwd)
                .placeholder("绝对工作目录；留空使用 ideall 当前目录")
        });
        let secret_store = SystemSecretStore;
        let agent_key_configured = secret_store
            .get(SecretKey::AgentCredential)
            .ok()
            .flatten()
            .is_some();
        let agent_thread_id = service
            .list_agent_threads(1)
            .ok()
            .and_then(|mut threads| threads.pop())
            .map(|thread| thread.id);
        let agent_transcript = agent_thread_id
            .as_deref()
            .and_then(|id| service.agent_transcript(id).ok())
            .unwrap_or_default();
        let title_subscription = cx.subscribe(&title_input, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.mark_editor_dirty(cx);
            }
        });
        let body_subscription = cx.subscribe(&body_input, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.mark_editor_dirty(cx);
            }
        });
        let code_subscription = cx.subscribe(&code_input, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.mark_editor_dirty(cx);
            }
        });
        let search_subscription =
            cx.subscribe(&search_input, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) && !this.suppress_search_events {
                    this.schedule_search(input.read(cx).value().to_string(), cx);
                }
            });
        let items = service.list_home().unwrap_or_default();
        let workspace = service.load_workspace_state().unwrap_or_default();
        let engine_preferences = service.load_engine_preferences().unwrap_or_default();
        let window_activation =
            cx.observe_window_activation(window, Self::window_activation_changed);
        let mut this = Self {
            database_path,
            service,
            secret_store,
            engines: builtin_engines(),
            engine_preferences,
            workspace,
            items,
            selected_id: None,
            selected_kind: None,
            active_section: "我的".into(),
            show_trash: false,
            search_query: String::new(),
            search_requests: LatestRequest::default(),
            search_task: None,
            suppress_search_events: false,
            search_input,
            feed_key_input,
            title_input,
            body_input,
            code_input,
            active_engine_id: None,
            embedded_browser: None,
            sync_server_input,
            sync_code_input,
            sync_token_input,
            agent_base_url_input,
            agent_model_input,
            agent_key_input,
            agent_prompt_input,
            acp_program_input,
            acp_args_input,
            acp_cwd_input,
            sync_code_configured: false,
            sync_token_configured: false,
            sync_in_progress: false,
            update_in_progress: false,
            available_update: None,
            update_status: None,
            agent_key_configured,
            agent_in_progress: false,
            agent_thread_id,
            agent_transcript,
            agent_audits: Vec::new(),
            agent_threads: Vec::new(),
            agent_activity_loading: false,
            body_editable: false,
            suppress_editor_events: false,
            draft_revision: 0,
            autosave_task: None,
            dirty: false,
            status: None,
            _subscriptions: vec![
                title_subscription,
                body_subscription,
                code_subscription,
                search_subscription,
                window_activation,
            ],
        };
        let active_tab = this.workspace.active_id.as_deref().and_then(|active_id| {
            this.workspace
                .tabs
                .iter()
                .find(|tab| tab_key(tab) == active_id)
                .map(|tab| (tab.file.file_id.clone(), tab.engine_id.clone()))
        });
        if let Some((id, engine_id)) = active_tab {
            this.load_node_into_editor(&id, window, cx);
            this.activate_engine(&engine_id, &id, window, cx);
        }
        this
    }

    fn refresh_items(&mut self) {
        let result = if self.show_trash {
            self.service.list_trash()
        } else if !self.search_query.trim().is_empty() {
            self.service.search(&self.search_query, 200)
        } else {
            self.service.list_home().map(|mut items| {
                match self.active_section.as_ref() {
                    "活动" => items.sort_by(|left, right| {
                        right
                            .updated_at
                            .cmp(&left.updated_at)
                            .then_with(|| left.id.cmp(&right.id))
                    }),
                    "浏览" => items
                        .retain(|item| matches!(item.kind, NodeKind::Bookmark | NodeKind::Feed)),
                    "应用" | "设置" => items.clear(),
                    _ => {}
                }
                items
            })
        };
        match result {
            Ok(items) => self.items = items,
            Err(error) => self.status = Some(error.to_string().into()),
        }
    }

    fn schedule_search(&mut self, query: String, cx: &mut Context<Self>) {
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
        self.status = Some("正在搜索本地内容…".into());
        let database_path = self.database_path.clone();
        let timer = cx.background_executor().timer(SEARCH_DEBOUNCE_DELAY);
        self.search_task = Some(cx.spawn(async move |view, cx| {
            timer.await;
            let query_for_search = query.clone();
            let search = cx.background_executor().spawn(async move {
                let workspace =
                    LocalWorkspace::open(database_path).map_err(|error| error.to_string())?;
                workspace
                    .search(&query_for_search, 200)
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
                            this.items = items;
                            this.status = Some(format!("找到 {count} 条本地内容").into());
                        }
                        Err(error) => {
                            this.status = Some(format!("搜索失败：{error}").into());
                        }
                    }
                    cx.notify();
                });
            });
        }));
        cx.notify();
    }

    fn create_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let parent = self.creation_parent();
        match self.service.create_note(parent.as_deref(), "无标题笔记") {
            Ok(node) => {
                self.show_trash = false;
                self.refresh_items();
                self.open_node(node.base().id.clone(), window, cx);
                self.status = Some("已创建本地笔记".into());
            }
            Err(error) => self.status = Some(error.to_string().into()),
        }
        cx.notify();
    }

    fn create_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let parent = self.creation_parent();
        match self.service.create_folder(parent.as_deref(), "新建文件夹") {
            Ok(node) => {
                self.show_trash = false;
                self.refresh_items();
                self.open_node(node.base().id.clone(), window, cx);
                self.status = Some("已创建文件夹".into());
            }
            Err(error) => self.status = Some(error.to_string().into()),
        }
        cx.notify();
    }

    fn create_bookmark(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let parent = self.creation_parent();
        match self
            .service
            .create_bookmark(parent.as_deref(), "新建书签", "https://example.com/")
        {
            Ok(node) => {
                self.show_trash = false;
                self.refresh_items();
                self.open_node(node.base().id.clone(), window, cx);
                self.status = Some("请修改书签标题与网址后保存".into());
            }
            Err(error) => self.status = Some(error.to_string().into()),
        }
        cx.notify();
    }

    fn create_text_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let parent = self.creation_parent();
        match self
            .service
            .create_file(parent.as_deref(), "新建文本.txt", "text/plain", Vec::new())
        {
            Ok(node) => {
                self.show_trash = false;
                self.refresh_items();
                self.open_node(node.base().id.clone(), window, cx);
                self.status = Some("已创建本地文件与 Blob".into());
            }
            Err(error) => self.status = Some(error.to_string().into()),
        }
        cx.notify();
    }

    fn create_feed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn import_files(&mut self, cx: &mut Context<Self>) {
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
                                256 * 1024 * 1024,
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

    fn creation_parent(&self) -> Option<String> {
        (self.selected_kind == Some(NodeKind::Folder))
            .then(|| self.selected_id.clone())
            .flatten()
    }

    fn load_node_into_editor(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<(NodeKind, String)> {
        match self.service.node(id) {
            Ok(node) => {
                self.detach_pending_autosave(cx);
                self.suppress_editor_events = true;
                let title = node.base().title.clone();
                let (body, editable, protected_blocks) = self.node_body(&node);
                self.title_input
                    .update(cx, |input, cx| input.set_value(title.clone(), window, cx));
                self.body_input
                    .update(cx, |input, cx| input.set_value(body.clone(), window, cx));
                self.code_input.update(cx, |input, cx| {
                    input.set_highlighter(code_language_for_node(&self.service, id), cx);
                    input.set_value(body, window, cx);
                });
                self.suppress_editor_events = false;
                self.selected_kind = Some(node.kind());
                self.selected_id = Some(id.to_owned());
                self.body_editable = editable;
                self.dirty = false;
                self.status = if node.kind() == NodeKind::Note && protected_blocks > 0 {
                    Some(
                        format!(
                            "可编辑常用 Markdown 块；{protected_blocks} 个未知富文本块以指纹占位保护，请勿修改占位行"
                        )
                        .into(),
                    )
                } else if node.kind() == NodeKind::File && !editable {
                    Some("此二进制类型在 ideall 中只读；可导出临时副本并用系统应用打开".into())
                } else {
                    None
                };
                Some((node.kind(), title))
            }
            Err(error) => {
                self.status = Some(error.to_string().into());
                None
            }
        }
    }

    fn open_node(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.dirty && self.selected_id.as_deref() == Some(id.as_str()) {
            return;
        }
        if let Some((kind, title)) = self.load_node_into_editor(&id, window, cx)
            && !self.show_trash
        {
            let engine_id = self
                .service
                .file_metadata(&id)
                .ok()
                .and_then(|file| {
                    resolve_default_engine(&self.engines, &file, &self.engine_preferences)
                })
                .map(|resolution| resolution.candidate.descriptor.engine_id)
                .unwrap_or_else(|| engine_id(kind).into());
            self.activate_engine(&engine_id, &id, window, cx);
            self.workspace.open(TabDescriptor {
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

    fn activate_engine(
        &mut self,
        engine_id: &str,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.active_engine_id = Some(engine_id.to_owned());
        self.embedded_browser = None;
        if engine_id != "ideall.browser" {
            return;
        }
        let url = match self.service.node(id) {
            Ok(Node::Bookmark { content, .. }) => content.url,
            _ => return,
        };
        match create_embedded_browser(&url, window, cx) {
            Ok(browser) => {
                self.embedded_browser = Some(browser);
                self.status = Some("已在隔离的系统 WebView 中打开书签".into());
            }
            Err(error) => self.status = Some(error.into()),
        }
    }

    fn node_body(&self, node: &Node) -> (String, bool, usize) {
        match node {
            Node::Note { base, .. } => self
                .service
                .plain_note(&base.id)
                .map(|document| (document.text, document.editable, document.protected_blocks))
                .unwrap_or_else(|error| (error.to_string(), false, 0)),
            Node::Bookmark { content, .. } => (content.url.clone(), true, 0),
            Node::File { base, blob_ref, .. } => self
                .service
                .text_file(&base.id)
                .map(|document| (document.text, true, 0))
                .unwrap_or_else(|_| {
                    (
                        format!(
                            "文件类型：{}\n大小：{} 字节\nBlob：{}",
                            blob_ref.mime, blob_ref.size, blob_ref.key
                        ),
                        false,
                        0,
                    )
                }),
            Node::Folder { .. } => ("文件夹中的项目将在这里显示。".into(), false, 0),
            Node::Feed { content, .. } => (format!("关注源：{}", content.key), false, 0),
            Node::Thread { content, .. } => (
                format!("{} 条本地会话消息", content.messages.len()),
                false,
                0,
            ),
        }
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected_id.clone() else {
            return;
        };
        self.autosave_task.take();
        let title = self.title_input.read(cx).value().to_string();
        let body = if self.active_engine_id.as_deref() == Some("ideall.code") {
            self.code_input.read(cx).value().to_string()
        } else {
            self.body_input.read(cx).value().to_string()
        };
        let result =
            self.service
                .save_edits(&id, title, self.body_editable.then_some(body.as_str()));
        match result {
            Ok(_) => {
                self.refresh_items();
                self.open_node(id, window, cx);
                self.status = Some("已保存到本地 SQLite".into());
                self.dirty = false;
            }
            Err(error) => self.status = Some(error.to_string().into()),
        }
        cx.notify();
    }

    fn mark_editor_dirty(&mut self, cx: &mut Context<Self>) {
        if self.suppress_editor_events || self.selected_id.is_none() || self.show_trash {
            return;
        }
        self.dirty = true;
        self.draft_revision = self.draft_revision.wrapping_add(1);
        self.schedule_draft_save(DRAFT_AUTOSAVE_DELAY, cx);
        cx.notify();
    }

    fn schedule_draft_save(&mut self, delay: Duration, cx: &mut Context<Self>) {
        if !self.dirty {
            return;
        }
        let Some(id) = self.selected_id.clone() else {
            return;
        };
        let revision = self.draft_revision;
        let title = self.title_input.read(cx).value().to_string();
        let body = if self.active_engine_id.as_deref() == Some("ideall.code") {
            self.code_input.read(cx).value().to_string()
        } else {
            self.body_input.read(cx).value().to_string()
        };
        let body = self.body_editable.then_some(body);
        let database_path = self.database_path.clone();
        let timer = cx.background_executor().timer(delay);
        self.autosave_task = Some(cx.spawn(async move |view, cx| {
            timer.await;
            let save_task = cx.background_executor().spawn(async move {
                let mut workspace =
                    LocalWorkspace::open(database_path).map_err(|error| error.to_string())?;
                let node = workspace
                    .save_edits(&id, title, body.as_deref())
                    .map_err(|error| error.to_string())?;
                Ok::<_, String>((id, NodeSummary::from(&node)))
            });
            let result = save_task.await;
            let _ = cx.update(|cx| {
                let Some(view) = view.upgrade() else {
                    return;
                };
                view.update(cx, |this, cx| {
                    if this.draft_revision != revision {
                        if let Err(error) = &result {
                            this.status = Some(format!("上一项草稿自动保存失败：{error}").into());
                            cx.notify();
                        }
                        return;
                    }
                    match result {
                        Ok((id, summary)) => {
                            if this.selected_id.as_deref() != Some(id.as_str()) {
                                return;
                            }
                            if let Some(item) = this.items.iter_mut().find(|item| item.id == id) {
                                item.title = summary.title;
                                item.updated_at = summary.updated_at;
                            }
                            this.dirty = false;
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

    fn detach_pending_autosave(&mut self, cx: &mut Context<Self>) {
        if self.dirty {
            self.schedule_draft_save(Duration::ZERO, cx);
        }
        self.draft_revision = self.draft_revision.wrapping_add(1);
        if let Some(task) = self.autosave_task.take() {
            task.detach();
        }
    }

    fn window_activation_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !window.is_window_active() {
            self.schedule_draft_save(Duration::ZERO, cx);
        }
    }

    fn load_agent_activity(&mut self, cx: &mut Context<Self>) {
        if self.agent_activity_loading {
            return;
        }
        let database_path = self.database_path.clone();
        let task = cx.background_executor().spawn(async move {
            let workspace =
                LocalWorkspace::open(database_path).map_err(|error| error.to_string())?;
            let audits = workspace
                .list_agent_audits(50)
                .map_err(|error| error.to_string())?;
            let threads = workspace
                .list_agent_threads(12)
                .map_err(|error| error.to_string())?;
            Ok::<_, String>((audits, threads))
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
                        Ok((audits, threads)) => {
                            this.agent_audits = audits;
                            this.agent_threads = threads;
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

    fn open_bookmark_external(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected_id.clone() else {
            return;
        };
        if self.selected_kind != Some(NodeKind::Bookmark) {
            return;
        }

        if self.dirty {
            let title = self.title_input.read(cx).value().to_string();
            let url = self.body_input.read(cx).value().to_string();
            if let Err(error) = self.service.save_edits(&id, title, Some(&url)) {
                self.status = Some(error.to_string().into());
                cx.notify();
                return;
            }
            self.dirty = false;
            self.refresh_items();
            self.load_node_into_editor(&id, window, cx);
        }

        match self.service.node(&id) {
            Ok(Node::Bookmark { content, .. }) => {
                cx.open_url(&content.url);
                self.status = Some("已交给系统浏览器打开".into());
            }
            Ok(_) => self.status = Some("当前项目不是书签".into()),
            Err(error) => self.status = Some(error.to_string().into()),
        }
        cx.notify();
    }

    fn open_portal(
        &mut self,
        engine_id: &'static str,
        url: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_selection(window, cx);
        self.active_engine_id = Some(engine_id.into());
        match create_embedded_browser(url, window, cx) {
            Ok(browser) => {
                self.embedded_browser = Some(browser);
                self.status =
                    Some("已在隔离的系统 WebView 中打开；未注入本地文件或密钥能力".into());
            }
            Err(error) => {
                cx.open_url(url);
                self.status = Some(format!("{error}；已交给系统浏览器").into());
            }
        }
        cx.notify();
    }

    fn open_file_with_system(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_id.clone() else {
            return;
        };
        let node = match self.service.node(&id) {
            Ok(node @ Node::File { .. }) => node,
            Ok(_) => {
                self.status = Some("当前项目不是文件".into());
                cx.notify();
                return;
            }
            Err(error) => {
                self.status = Some(error.to_string().into());
                cx.notify();
                return;
            }
        };
        let name = std::path::Path::new(&node.base().title)
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("ideall-export.bin");
        let directory = self
            .database_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("viewer-cache")
            .join(&id);
        let target = directory.join(name);
        match self.service.export_file(&id, &target) {
            Ok(size) => {
                cx.open_with_system(&target);
                self.status = Some(format!("已导出 {size} 字节临时副本并交给系统应用").into());
            }
            Err(error) => self.status = Some(error.to_string().into()),
        }
        cx.notify();
    }

    fn move_selected_to_trash(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dirty {
            self.save(window, cx);
            if self.dirty {
                return;
            }
        }
        let Some(id) = self.selected_id.clone() else {
            return;
        };
        match self.service.move_to_trash(&id) {
            Ok(true) => {
                let tab_ids = self
                    .workspace
                    .tabs
                    .iter()
                    .filter(|tab| tab.file.file_id == id)
                    .map(tab_key)
                    .collect::<Vec<_>>();
                for tab_id in tab_ids {
                    self.workspace.close(&tab_id);
                }
                self.persist_workspace();
                self.clear_selection(window, cx);
                self.refresh_items();
                self.status = Some("已移到回收站".into());
            }
            Ok(false) => self.status = Some("项目已不在当前列表".into()),
            Err(error) => self.status = Some(error.to_string().into()),
        }
        cx.notify();
    }

    fn restore_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected_id.clone() else {
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

    fn purge_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected_id.clone() else {
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

    fn confirm_purge(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn show_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_trash = false;
        self.search_query.clear();
        self.search_requests.invalidate();
        drop(self.search_task.take());
        self.suppress_search_events = true;
        self.search_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.suppress_search_events = false;
        self.active_section = "我的".into();
        self.clear_selection(window, cx);
        self.refresh_items();
        cx.notify();
    }

    fn select_section(
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
        self.clear_selection(window, cx);
        self.refresh_items();
        self.status = match section {
            "活动" => Some("按最近修改排序的本地活动".into()),
            "浏览" => Some("本地书签与关注源".into()),
            "应用" => {
                match self.secret_store.get(SecretKey::AgentCredential) {
                    Ok(value) => self.agent_key_configured = value.is_some(),
                    Err(error) => self.status = Some(error.to_string().into()),
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

    fn create_agent_draft(&mut self, cx: &mut Context<Self>) {
        match self.service.create_agent_note_via_mcp("Agent 草稿") {
            Ok(node) => {
                self.refresh_items();
                self.status = Some(
                    format!(
                        "Agent 已通过本地 MCP 创建空白笔记 {}；写操作审计已提交",
                        node.base().id
                    )
                    .into(),
                );
            }
            Err(error) => self.status = Some(error.to_string().into()),
        }
        self.load_agent_activity(cx);
        cx.notify();
    }

    fn persist_agent_model_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let settings = AgentModelSettings {
            base_url: self.agent_base_url_input.read(cx).value().to_string(),
            model: self.agent_model_input.read(cx).value().to_string(),
        };
        let saved = self
            .service
            .save_agent_model_settings(&settings)
            .map_err(|error| error.to_string())?;
        self.agent_base_url_input.update(cx, |input, cx| {
            input.set_value(saved.base_url.clone(), window, cx);
        });
        self.agent_model_input.update(cx, |input, cx| {
            input.set_value(saved.model.clone(), window, cx);
        });

        let key = self.agent_key_input.read(cx).value().to_string();
        if !key.trim().is_empty() {
            let key = key.trim();
            OpenAiCompatibleClient::new(&saved.base_url, &saved.model, key)
                .map_err(|error| error.to_string())?;
            self.secret_store
                .set(SecretKey::AgentCredential, key)
                .map_err(|error| error.to_string())?;
            self.agent_key_configured = true;
            self.agent_key_input
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
        Ok(())
    }

    fn save_agent_model_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.status = match self.persist_agent_model_settings(window, cx) {
            Ok(()) => Some("模型设置已保存；API Key 仅在系统安全存储中".into()),
            Err(error) => Some(error.into()),
        };
        cx.notify();
    }

    fn clear_agent_credential(&mut self, cx: &mut Context<Self>) {
        match self.secret_store.delete(SecretKey::AgentCredential) {
            Ok(_) => {
                self.agent_key_configured = false;
                self.status = Some("模型 API Key 已从系统安全存储清除".into());
            }
            Err(error) => self.status = Some(error.to_string().into()),
        }
        cx.notify();
    }

    fn new_agent_thread(&mut self, cx: &mut Context<Self>) {
        if self.agent_in_progress {
            return;
        }
        self.agent_thread_id = None;
        self.agent_transcript.clear();
        self.status = Some("已开始新对话；发送第一条消息后写入本地 SQLite".into());
        cx.notify();
    }

    fn select_agent_thread(&mut self, thread_id: String, cx: &mut Context<Self>) {
        if self.agent_in_progress {
            return;
        }
        match self.service.agent_transcript(&thread_id) {
            Ok(messages) => {
                self.agent_thread_id = Some(thread_id);
                self.agent_transcript = messages;
                self.status = Some("已打开本地 Agent 对话".into());
            }
            Err(error) => self.status = Some(error.to_string().into()),
        }
        cx.notify();
    }

    fn start_agent_turn(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.agent_in_progress {
            return;
        }
        if let Err(error) = self.persist_agent_model_settings(window, cx) {
            self.status = Some(error.into());
            cx.notify();
            return;
        }
        let prompt = self.agent_prompt_input.read(cx).value().to_string();
        if prompt.trim().is_empty() {
            self.status = Some("请输入 Agent 消息".into());
            cx.notify();
            return;
        }
        let settings = match self.service.load_agent_model_settings() {
            Ok(settings) => settings,
            Err(error) => {
                self.status = Some(error.to_string().into());
                cx.notify();
                return;
            }
        };
        let key = match self.secret_store.get(SecretKey::AgentCredential) {
            Ok(Some(key)) => key,
            Ok(None) => {
                self.status = Some("请先安全保存模型 API Key".into());
                cx.notify();
                return;
            }
            Err(error) => {
                self.status = Some(error.to_string().into());
                cx.notify();
                return;
            }
        };
        let database_path = self.database_path.clone();
        let thread_id = self.agent_thread_id.clone();
        let task = cx.background_executor().spawn(async move {
            let mut workspace =
                LocalWorkspace::open(database_path).map_err(|error| error.to_string())?;
            let mut provider =
                OpenAiCompatibleClient::new(&settings.base_url, &settings.model, &key)
                    .map_err(|error| error.to_string())?;
            workspace
                .run_agent_turn(&mut provider, thread_id.as_deref(), &prompt)
                .map_err(|error| error.to_string())
        });
        self.agent_in_progress = true;
        self.status = Some("Agent 正在本机直连模型服务……".into());
        cx.spawn_in(window, async move |view, cx| {
            let result = task.await;
            let _ = view.update_in(cx, |this, window, cx| {
                this.agent_in_progress = false;
                match result {
                    Ok(result) => {
                        this.agent_thread_id = Some(result.thread_id.clone());
                        this.agent_transcript = this
                            .service
                            .agent_transcript(&result.thread_id)
                            .unwrap_or_default();
                        this.agent_prompt_input
                            .update(cx, |input, cx| input.set_value("", window, cx));
                        this.refresh_items();
                        let failures = result.tools.iter().filter(|tool| !tool.ok).count();
                        this.status = Some(if result.tools.is_empty() {
                            "Agent 已回复；对话保存在本地 SQLite".into()
                        } else {
                            format!(
                                "Agent 已回复；执行 {} 个工具，{} 个失败；写操作均已审计",
                                result.tools.len(),
                                failures
                            )
                            .into()
                        });
                    }
                    Err(error) => {
                        if this.agent_thread_id.is_none()
                            && let Some(thread) = this
                                .service
                                .list_agent_threads(1)
                                .ok()
                                .and_then(|mut threads| threads.pop())
                        {
                            this.agent_thread_id = Some(thread.id);
                        }
                        if let Some(thread_id) = this.agent_thread_id.as_deref() {
                            this.agent_transcript =
                                this.service.agent_transcript(thread_id).unwrap_or_default();
                        }
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

    fn persist_external_acp_settings(
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

    fn save_external_acp_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.status = match self.persist_external_acp_settings(window, cx) {
            Ok(_) => Some("外部 ACP 命令已保存；程序与 argv 直接启动，不经 shell".into()),
            Err(error) => Some(error.into()),
        };
        cx.notify();
    }

    fn start_external_acp_turn(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
            let _ = view.update_in(cx, |this, window, cx| {
                this.agent_in_progress = false;
                match result {
                    Ok((thread_id, result)) => {
                        this.agent_thread_id = Some(thread_id.clone());
                        this.agent_transcript = this
                            .service
                            .agent_transcript(&thread_id)
                            .unwrap_or_default();
                        this.agent_prompt_input
                            .update(cx, |input, cx| input.set_value("", window, cx));
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

    fn refresh_sync_secret_status(&mut self) {
        match self.secret_store.get(SecretKey::SyncCode) {
            Ok(value) => self.sync_code_configured = value.is_some(),
            Err(error) => self.status = Some(error.to_string().into()),
        }
        match self.secret_store.get(SecretKey::SyncBearerToken) {
            Ok(value) => self.sync_token_configured = value.is_some(),
            Err(error) => self.status = Some(error.to_string().into()),
        }
    }

    fn persist_sync_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let server_base_url = self.sync_server_input.read(cx).value().to_string();
        let saved = self
            .service
            .save_sync_settings(&SyncSettings { server_base_url })
            .map_err(|error| error.to_string())?;
        self.sync_server_input.update(cx, |input, cx| {
            input.set_value(saved.server_base_url, window, cx);
        });

        let code = self.sync_code_input.read(cx).value().to_string();
        if !code.trim().is_empty() {
            if !is_valid_sync_code(&code) {
                return Err("同步码必须恰好包含 32 位十六进制字符".into());
            }
            self.secret_store
                .set(SecretKey::SyncCode, &normalize_sync_code(&code))
                .map_err(|error| error.to_string())?;
            self.sync_code_configured = true;
            self.sync_code_input
                .update(cx, |input, cx| input.set_value("", window, cx));
        }

        let token = self.sync_token_input.read(cx).value().to_string();
        if !token.trim().is_empty() {
            self.secret_store
                .set(SecretKey::SyncBearerToken, token.trim())
                .map_err(|error| error.to_string())?;
            self.sync_token_configured = true;
            self.sync_token_input
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
        Ok(())
    }

    fn save_sync_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.status = match self.persist_sync_settings(window, cx) {
            Ok(()) => Some("同步设置已保存；凭据未写入 SQLite".into()),
            Err(error) => Some(error.into()),
        };
        cx.notify();
    }

    fn clear_sync_credentials(&mut self, cx: &mut Context<Self>) {
        let result = self
            .secret_store
            .delete(SecretKey::SyncCode)
            .and_then(|_| self.secret_store.delete(SecretKey::SyncBearerToken));
        match result {
            Ok(_) => {
                self.sync_code_configured = false;
                self.sync_token_configured = false;
                self.status = Some("同步凭据已从系统安全存储清除".into());
            }
            Err(error) => self.status = Some(error.to_string().into()),
        }
        cx.notify();
    }

    fn start_notes_sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sync_in_progress {
            return;
        }
        if let Err(error) = self.persist_sync_settings(window, cx) {
            self.status = Some(error.into());
            cx.notify();
            return;
        }
        let settings = match self.service.load_sync_settings() {
            Ok(settings) => settings,
            Err(error) => {
                self.status = Some(error.to_string().into());
                cx.notify();
                return;
            }
        };
        let code = match self.secret_store.get(SecretKey::SyncCode) {
            Ok(Some(value)) => value,
            Ok(None) => {
                self.status = Some("请先保存同步码".into());
                cx.notify();
                return;
            }
            Err(error) => {
                self.status = Some(error.to_string().into());
                cx.notify();
                return;
            }
        };
        let token = match self.secret_store.get(SecretKey::SyncBearerToken) {
            Ok(Some(value)) => value,
            Ok(None) => {
                self.status = Some("请先保存登录 Bearer Token".into());
                cx.notify();
                return;
            }
            Err(error) => {
                self.status = Some(error.to_string().into());
                cx.notify();
                return;
            }
        };

        let database_path = self.database_path.clone();
        let task = cx.background_executor().spawn(async move {
            let mut workspace =
                LocalWorkspace::open(database_path).map_err(|error| error.to_string())?;
            let mut transport = HttpSyncTransport::new(&settings.server_base_url, &token)
                .map_err(|error| error.to_string())?;
            let subscriptions = workspace
                .sync_subscriptions(&code, &mut transport)
                .map_err(|error| error.to_string())?;
            let notes = workspace
                .sync_notes(&code, &mut transport)
                .map_err(|error| error.to_string())?;
            let bookmarks = workspace
                .sync_bookmarks(&code, &mut transport)
                .map_err(|error| error.to_string())?;
            Ok::<_, String>((subscriptions, notes, bookmarks))
        });
        self.sync_in_progress = true;
        self.status = Some("正在同步关注、笔记与书签三个加密域……".into());
        cx.spawn(async move |view, cx| {
            let result = task.await;
            cx.update(|cx| {
                let Some(view) = view.upgrade() else {
                    return;
                };
                view.update(cx, |this, cx| {
                    this.sync_in_progress = false;
                    match result {
                        Ok((subscriptions, notes, bookmarks)) => {
                            this.refresh_items();
                            this.status = Some(
                                format!(
                                    "同步完成：关注 {}、笔记 {}、书签 {} 条；新增 {} 条",
                                    subscriptions.total,
                                    notes.total,
                                    bookmarks.total,
                                    subscriptions.added + notes.added + bookmarks.added
                                )
                                .into(),
                            );
                        }
                        Err(error) => this.status = Some(error.into()),
                    }
                    cx.notify();
                });
            })
        })
        .detach();
        cx.notify();
    }

    fn start_update_check(&mut self, cx: &mut Context<Self>) {
        if self.update_in_progress {
            return;
        }
        let task = cx.background_executor().spawn(async move {
            let updater = updater_from_environment().map_err(|error| error.to_string())?;
            updater
                .check(env!("CARGO_PKG_VERSION"))
                .map_err(|error| error.to_string())
        });
        self.update_in_progress = true;
        self.update_status = Some("正在获取并验证签名更新清单……".into());
        cx.spawn(async move |view, cx| {
            let result = task.await;
            cx.update(|cx| {
                let Some(view) = view.upgrade() else {
                    return;
                };
                view.update(cx, |this, cx| {
                    this.update_in_progress = false;
                    match result {
                        Ok(UpdateStatus::Current { version }) => {
                            this.available_update = None;
                            this.update_status =
                                Some(format!("当前 {version} 已是最新版本").into());
                        }
                        Ok(UpdateStatus::Available(update)) => {
                            this.update_status =
                                Some(format!("已验证 {} 的签名更新清单", update.version).into());
                            this.available_update = Some(update);
                        }
                        Err(error) => this.update_status = Some(error.into()),
                    }
                    cx.notify();
                });
            })
        })
        .detach();
        cx.notify();
    }

    fn start_update_download(&mut self, cx: &mut Context<Self>) {
        if self.update_in_progress {
            return;
        }
        let Some(update) = self.available_update.clone() else {
            self.update_status = Some("请先检查更新".into());
            cx.notify();
            return;
        };
        let destination_root = self
            .database_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("updates");
        let task = cx.background_executor().spawn(async move {
            let updater = updater_from_environment().map_err(|error| error.to_string())?;
            updater
                .download(&update, &destination_root)
                .map_err(|error| error.to_string())
        });
        self.update_in_progress = true;
        self.update_status = Some("正在下载并校验安装器大小与 SHA-256……".into());
        cx.spawn(async move |view, cx| {
            let result = task.await;
            cx.update(|cx| {
                let Some(view) = view.upgrade() else {
                    return;
                };
                view.update(cx, |this, cx| {
                    this.update_in_progress = false;
                    match result {
                        Ok(path) => {
                            cx.open_with_system(&path);
                            this.update_status = Some(
                                "安装器已通过签名清单与 SHA-256 校验，并交给系统打开；安装前请保存当前编辑"
                                    .into(),
                            );
                        }
                        Err(error) => this.update_status = Some(error.into()),
                    }
                    cx.notify();
                });
            })
        })
        .detach();
        cx.notify();
    }

    fn show_trash(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_trash = true;
        self.search_query.clear();
        self.search_requests.invalidate();
        drop(self.search_task.take());
        self.suppress_search_events = true;
        self.search_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.suppress_search_events = false;
        self.active_section = "回收站".into();
        self.clear_selection(window, cx);
        self.refresh_items();
        cx.notify();
    }

    fn clear_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.detach_pending_autosave(cx);
        self.suppress_editor_events = true;
        self.selected_id = None;
        self.selected_kind = None;
        self.body_editable = false;
        self.dirty = false;
        self.title_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.body_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.code_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.suppress_editor_events = false;
        self.active_engine_id = None;
        self.embedded_browser = None;
    }

    fn activate_tab(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.activate(&id)
            && let Some(tab) = self.workspace.tabs.iter().find(|tab| tab_key(tab) == id)
        {
            self.open_node(tab.file.file_id.clone(), window, cx);
        }
        self.persist_workspace();
    }

    fn close_tab(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if !self.workspace.close(&id) {
            return;
        }
        self.persist_workspace();
        if let Some(active_id) = self.workspace.active_id.clone()
            && let Some(tab) = self
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

    fn persist_workspace(&mut self) {
        if let Err(error) = self.service.save_workspace_state(&self.workspace) {
            self.status = Some(error.to_string().into());
        }
    }

    fn set_engine_preference(
        &mut self,
        engine_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dirty {
            self.save(window, cx);
            if self.dirty {
                return;
            }
        }
        let Some(id) = self.selected_id.clone() else {
            return;
        };
        let result = self.service.file_metadata(&id).and_then(|file| {
            let supported = list_matching_engines(&self.engines, &file)
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
            self.engine_preferences
                .files
                .insert(key, engine_id.to_owned());
            self.service
                .save_engine_preferences(&self.engine_preferences)
        });
        match result {
            Ok(()) => {
                self.open_node(id, window, cx);
                self.status = Some(format!("默认使用 {engine_id}").into());
            }
            Err(error) => self.status = Some(error.to_string().into()),
        }
        cx.notify();
    }
}

fn engine_id(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Folder => "ideall.directory",
        NodeKind::Note => "ideall.note",
        NodeKind::Bookmark => "ideall.bookmark",
        NodeKind::File => "ideall.preview",
        NodeKind::Feed => "ideall.feed",
        NodeKind::Thread => "ideall.thread",
    }
}

fn kind_icon(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Folder => "▸",
        NodeKind::Note => "N",
        NodeKind::Bookmark => "↗",
        NodeKind::File => "F",
        NodeKind::Feed => "◎",
        NodeKind::Thread => "#",
    }
}

fn engine_access_label(access: EngineAccess) -> &'static str {
    match access {
        EngineAccess::ReadOnly => "只读",
        EngineAccess::ReadWrite => "读写",
    }
}

fn audit_status_label(status: AuditStatus) -> &'static str {
    match status {
        AuditStatus::Pending => "执行中",
        AuditStatus::Committed => "已提交",
        AuditStatus::Failed => "失败",
        AuditStatus::Rejected => "已拒绝",
        AuditStatus::Undone => "已撤销",
    }
}

fn agent_role_label(role: ModelRole) -> &'static str {
    match role {
        ModelRole::System => "系统",
        ModelRole::User => "你",
        ModelRole::Assistant => "Agent",
        ModelRole::Tool => "工具",
    }
}

fn acp_tool_status_label(status: AcpToolStatus) -> &'static str {
    match status {
        AcpToolStatus::Pending => "等待",
        AcpToolStatus::InProgress => "执行中",
        AcpToolStatus::Completed => "完成",
        AcpToolStatus::Failed => "失败",
    }
}

fn code_language_for_node(service: &LocalWorkspace, id: &str) -> &'static str {
    let Ok(file) = service.file_metadata(id) else {
        return "markdown";
    };
    let extension = file
        .name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase());
    match (file.media_type.as_str(), extension.as_deref()) {
        (_, Some("rs")) => "rust",
        (_, Some("ts" | "tsx")) => "typescript",
        (_, Some("js" | "jsx" | "mjs" | "cjs")) => "javascript",
        ("application/json", _) | (_, Some("json")) => "json",
        ("text/markdown", _) | (_, Some("md" | "mdx")) => "markdown",
        (_, Some("toml")) => "toml",
        (_, Some("yaml" | "yml")) => "yaml",
        ("text/html", _) | (_, Some("html" | "htm")) => "html",
        ("text/css", _) | (_, Some("css")) => "css",
        (_, Some("py")) => "python",
        (_, Some("go")) => "go",
        (_, Some("c" | "h")) => "c",
        (_, Some("cc" | "cpp" | "cxx" | "hpp")) => "cpp",
        (_, Some("sh" | "bash")) => "bash",
        (_, Some("sql")) => "sql",
        _ => "markdown",
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn create_embedded_browser(
    url: &str,
    window: &mut Window,
    cx: &mut Context<IdeallDesktop>,
) -> Result<Entity<WebView>, String> {
    let webview = wry::WebViewBuilder::new()
        .with_url(url)
        .build_as_child(window)
        .map_err(|error| error.to_string())?;
    Ok(cx.new(|cx| WebView::new(webview, window, cx)))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn create_embedded_browser(
    _url: &str,
    _window: &mut Window,
    _cx: &mut Context<IdeallDesktop>,
) -> Result<Entity<WebView>, String> {
    Err("当前 Linux GPUI 后端不支持可靠的子窗口 WebView；可使用“系统浏览器打开”".into())
}

fn native_database_path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("IDEALL_NATIVE_DATA_DIR") {
        let directory = PathBuf::from(path);
        std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        return Ok(directory.join("ideall.db"));
    }
    let project = ProjectDirs::from("org", "ideall", "ideall")
        .ok_or_else(|| "无法确定 ideall 本地数据目录".to_owned())?;
    std::fs::create_dir_all(project.data_dir()).map_err(|error| error.to_string())?;
    Ok(project.data_dir().join("ideall.db"))
}

#[cfg(target_os = "linux")]
fn select_linux_backend() {
    let has_x11 = std::env::var_os("DISPLAY").is_some_and(|value| !value.is_empty());
    let has_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|value| !value.is_empty());
    let declared_wayland = std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland");
    let force_x11 = std::env::var_os("IDEALL_FORCE_X11").is_some();
    if has_x11 && has_wayland && (force_x11 || !declared_wayland) {
        // SAFETY: this runs as the first statement in `main`, before GPUI or any
        // other thread can read the process environment.
        unsafe { std::env::remove_var("WAYLAND_DISPLAY") };
    }
}

#[cfg(not(target_os = "linux"))]
fn select_linux_backend() {}

fn desktop_window_options(cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(1280.0), px(820.0)), cx)),
        titlebar: Some(TitlebarOptions {
            title: Some("ideall · 本地信息终端".into()),
            ..Default::default()
        }),
        window_min_size: Some(size(px(920.0), px(600.0))),
        app_id: Some("com.jinziyou.ideall".into()),
        ..Default::default()
    }
}

fn main() {
    select_linux_backend();
    let secure_store_error = ideall_secrets::initialize_platform()
        .err()
        .map(|error| format!("无法初始化系统安全存储：{error}"));
    Application::new().run(move |cx: &mut App| {
        gpui_component::init(cx);
        actions::init(cx);
        let initialized = (|| {
            if let Some(error) = secure_store_error {
                return Err(error);
            }
            let database_path = native_database_path()?;
            let service = LocalWorkspace::open(&database_path)
                .map_err(|error| format!("无法打开本地数据库：{error}"))?;
            Ok::<_, String>((database_path, service))
        })();
        let window_result = match initialized {
            Ok((database_path, service)) => cx
                .open_window(desktop_window_options(cx), |window, cx| {
                    let workspace =
                        cx.new(|cx| IdeallDesktop::new(database_path, service, window, cx));
                    cx.new(|cx| Root::new(workspace, window, cx))
                })
                .map(|_| ()),
            Err(message) => cx
                .open_window(desktop_window_options(cx), |_window, cx| {
                    cx.new(|_| DesktopStartupFailure { message })
                })
                .map(|_| ()),
        };
        if let Err(error) = window_result {
            eprintln!("failed to open ideall desktop window: {error}");
        }
        cx.activate(true);
    });
}
