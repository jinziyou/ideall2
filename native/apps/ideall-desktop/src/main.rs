mod assets;
mod browser;
mod editor;
mod explorer;
mod windowing;
mod workbench;

use std::{path::PathBuf, time::Duration};

use directories::ProjectDirs;
use gpui::{
    App, Application, ClipboardItem, Context, Entity, IntoElement, MouseButton, MouseDownEvent,
    PathPromptOptions, PromptButton, PromptLevel, Render, ScrollHandle, SharedString, Subscription,
    Task, Window, div, prelude::*, px, rgb, size,
};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use gpui_component::webview::WebView;
use gpui_component::{
    Disableable as _, Icon, IconName, PixelsExt as _, Root, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    input::{Input, InputEvent, InputState},
    menu::{ContextMenuExt as _, DropdownMenu as _},
    resizable::{ResizablePanelEvent, ResizableState, h_resizable, resizable_panel},
    scroll::ScrollableElement as _,
    text::TextView,
    theme::{Theme, ThemeMode},
};
use ideall_acp::{AcpToolStatus, ExternalAcpConfig, command_exists, run_external_acp};
use ideall_application::{
    AgentAuditRecord, AgentDocumentContext, AgentModelSettings, AgentToolRun,
    AgentTranscriptMessage, AuditStatus, BookmarkMetadata, ExternalAcpSettings, HOME_ROOT_ID,
    INBOX_ROOT_ID, LocalWorkspace, ModelRole, NodeSummary, NoteBlockIndent, NoteBlockMove,
    NoteBlockStyle, NoteColor, NoteInlineStyle, NoteSlashCommand, OpenAiCompatibleClient,
    SyncSettings, note_mention_query, note_node_id_from_url, note_node_url, note_slash_commands,
    note_slash_query,
};
use ideall_domain::{
    EnginePlatform, EnginePreferences, EngineRuntimeKind, ExplorerLayoutState, TabDescriptor,
    WorkspaceState, builtin_engines, engine_runtime_capabilities, list_matching_engines,
    resolve_default_engine, tab_key,
};
use ideall_presentation::{
    LatestRequest, NAVIGATION, NavigationItem,
    color::desktop::{
        ACCENT, ACCENT_ACTIVE, ACCENT_HOVER, ACCENT_ON_RAIL, ACCENT_SOFT, BORDER, BORDER_STRONG,
        CANVAS, DANGER, DANGER_BORDER, EDITOR, EDITOR_GUTTER, HOVER, INK, INK_FAINT, INK_MUTED,
        MESSAGE_TOOL, RAIL, RAIL_ACTIVE, RAIL_MUTED, SELECTION, SURFACE, SURFACE_MUTED,
        SURFACE_RAISED, WARNING_SOFT,
    },
};
use ideall_protocol::{EngineAccess, EngineDescriptor, FileRef, Node, NodeKind, SubscriptionType};
use ideall_secrets::{SecretKey, SecretStore as _, SystemSecretStore};
use ideall_sync::{is_valid_sync_code, normalize_sync_code};
use ideall_sync_http::HttpSyncTransport;
use ideall_updater::{AvailableUpdate, UpdateStatus, updater_from_environment};

use assets::{IdeallAssets, IdeallIcon};
use browser::{BrowserWorkspace, browser_surface};
use editor::{
    EditorInputs, EditorMode, EditorWorkspace, LoadedDocument, engine_icon,
    position_for_byte_offset, text_highlight_for_node,
};
use explorer::{DraggedNode, ExplorerState};
use windowing::desktop_window_options;

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
    editor: EditorWorkspace,
    explorer: ExplorerState,
    browser: BrowserWorkspace,
    active_section: SharedString,
    show_trash: bool,
    inbox_count: usize,
    search_query: String,
    search_kind: Option<NodeKind>,
    mentionable_notes: Vec<NodeSummary>,
    note_backlinks: Vec<NodeSummary>,
    search_requests: LatestRequest,
    search_task: Option<Task<()>>,
    suppress_search_events: bool,
    search_input: Entity<InputState>,
    feed_key_input: Entity<InputState>,
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
    agent_context_attached: bool,
    agent_thread_id: Option<String>,
    agent_transcript: Vec<AgentTranscriptMessage>,
    agent_scroll_handle: ScrollHandle,
    agent_audits: Vec<AgentAuditRecord>,
    agent_threads: Vec<NodeSummary>,
    agent_activity_loading: bool,
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
        let browser_address_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("输入 HTTP(S) 地址并按 Enter"));
        let title_input = cx.new(|cx| InputState::new(window, cx).placeholder("标题"));
        let body_input = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .rows(12)
                .searchable(true)
                .placeholder("从这里开始记录；输入 / 插入内容块……")
        });
        let text_input = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("text")
                .line_number(true)
                .soft_wrap(false)
                .searchable(true)
                .placeholder("文件内容")
        });
        let find_input = cx.new(|cx| InputState::new(window, cx).placeholder("在当前文件中查找"));
        let replace_input = cx.new(|cx| InputState::new(window, cx).placeholder("替换为"));
        let link_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("https://example.com"));
        let rename_input = cx.new(|cx| InputState::new(window, cx).placeholder("输入新名称"));
        let explorer_resize_state = cx.new(|_| ResizableState::default());
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
        let body_subscription = cx.subscribe_in(
            &body_input,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => this.mark_editor_dirty(cx),
                InputEvent::PressEnter { .. } => {
                    this.complete_note_slash_after_enter(window, cx);
                }
                _ => {}
            },
        );
        let text_subscription = cx.subscribe(&text_input, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.mark_editor_dirty(cx);
            }
        });
        let find_subscription = cx.subscribe_in(
            &find_input,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    this.editor.find_cursor = 0;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => {
                    this.find_next_in_editor(window, cx);
                }
                _ => {}
            },
        );
        let replace_subscription = cx.subscribe(&replace_input, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        let link_subscription = cx.subscribe_in(
            &link_input,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.commit_note_link(window, cx);
                }
            },
        );
        let browser_address_subscription = cx.subscribe_in(
            &browser_address_input,
            window,
            |this, input, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    let url = input.read(cx).value().to_string();
                    this.navigate_browser_to(&url, true, window, cx);
                }
            },
        );
        let agent_prompt_subscription = cx.subscribe_in(
            &agent_prompt_input,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { secondary: true }) {
                    this.start_agent_turn(window, cx);
                }
            },
        );
        let body_observer = cx.observe(&body_input, |_, _, cx| cx.notify());
        let text_observer = cx.observe(&text_input, |_, _, cx| cx.notify());
        let search_subscription =
            cx.subscribe(&search_input, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) && !this.suppress_search_events {
                    this.schedule_search(input.read(cx).value().to_string(), cx);
                }
            });
        let rename_subscription = cx.subscribe_in(
            &rename_input,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.commit_explorer_rename(window, cx);
                }
            },
        );
        let explorer_resize_subscription = cx.subscribe(
            &explorer_resize_state,
            |this, state, _: &ResizablePanelEvent, cx| {
                if let Some(width) = state.read(cx).sizes().first() {
                    this.explorer.update_panel_width(width.as_f32());
                    this.persist_workspace();
                }
            },
        );
        let items = service.list_home().unwrap_or_default();
        let workspace = service.load_workspace_state().unwrap_or_default();
        let explorer_layout = workspace.explorer.clone();
        let engine_preferences = service.load_engine_preferences().unwrap_or_default();
        let editor = EditorWorkspace::new(
            builtin_engines(),
            engine_preferences,
            workspace,
            EditorInputs {
                title: title_input,
                body: body_input,
                text: text_input,
                find: find_input,
                replace: replace_input,
                link: link_input,
            },
        );
        let mentionable_notes = items
            .iter()
            .filter(|item| item.kind == NodeKind::Note && !item.deleted)
            .cloned()
            .collect();
        let explorer =
            ExplorerState::new(items, explorer_layout, rename_input, explorer_resize_state);
        let browser = BrowserWorkspace::new(browser_address_input);
        let window_activation =
            cx.observe_window_activation(window, Self::window_activation_changed);
        let mut this = Self {
            database_path,
            service,
            secret_store,
            editor,
            explorer,
            browser,
            active_section: "我的".into(),
            show_trash: false,
            inbox_count: 0,
            search_query: String::new(),
            search_kind: None,
            mentionable_notes,
            note_backlinks: Vec::new(),
            search_requests: LatestRequest::default(),
            search_task: None,
            suppress_search_events: false,
            search_input,
            feed_key_input,
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
            agent_context_attached: false,
            agent_thread_id,
            agent_transcript,
            agent_scroll_handle: ScrollHandle::new(),
            agent_audits: Vec::new(),
            agent_threads: Vec::new(),
            agent_activity_loading: false,
            status: None,
            _subscriptions: vec![
                title_subscription,
                body_subscription,
                text_subscription,
                find_subscription,
                replace_subscription,
                link_subscription,
                browser_address_subscription,
                agent_prompt_subscription,
                body_observer,
                text_observer,
                search_subscription,
                rename_subscription,
                explorer_resize_subscription,
                window_activation,
            ],
        };
        this.agent_scroll_handle.scroll_to_bottom();
        let active_tab = this
            .editor
            .workspace
            .active_id
            .as_deref()
            .and_then(|active_id| {
                this.editor
                    .workspace
                    .tabs
                    .iter()
                    .find(|tab| tab_key(tab) == active_id)
                    .map(|tab| (tab.file.file_id.clone(), tab.engine_id.clone()))
            });
        if let Some((id, engine_id)) = active_tab {
            this.load_node_into_editor(&id, window, cx);
            this.activate_engine(&engine_id, &id, window, cx);
            this.editor.restore_active_session(window, cx);
        }
        this.load_agent_activity(cx);
        // 启动即按当前分区应用过滤并刷新收集箱计数，
        // 避免首帧展示未过滤的初始列表（收集箱重复出现、虚拟根计数为 0）。
        this.refresh_items();
        this
    }

    fn refresh_items(&mut self) {
        if let Ok(items) = self.service.list_home() {
            self.mentionable_notes = items
                .into_iter()
                .filter(|item| item.kind == NodeKind::Note && !item.deleted)
                .collect();
        }
        self.inbox_count = self
            .service
            .list_children(INBOX_ROOT_ID)
            .map(|children| children.len())
            .unwrap_or(0);
        let result = if self.show_trash {
            self.service.list_trash()
        } else if self.active_section.as_ref() == "收集箱" {
            self.service.list_children(INBOX_ROOT_ID)
        } else if !self.search_query.trim().is_empty() {
            self.service
                .search_with_kind(&self.search_query, 200, self.search_kind)
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
                    "我的" => {
                        // 收集箱通过虚拟根进入，不在「我的」树中重复出现。
                        let mut hidden =
                            std::collections::HashSet::from([INBOX_ROOT_ID.to_owned()]);
                        items.retain(|item| {
                            let hide = hidden.contains(item.id.as_str())
                                || item
                                    .parent_id
                                    .as_deref()
                                    .is_some_and(|parent| hidden.contains(parent));
                            if hide {
                                hidden.insert(item.id.clone());
                            }
                            !hide
                        });
                    }
                    _ => {}
                }
                items
            })
        };
        match result {
            Ok(items) => self.explorer.replace_items(items),
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
        let search_kind = self.search_kind;
        let timer = cx.background_executor().timer(SEARCH_DEBOUNCE_DELAY);
        self.search_task = Some(cx.spawn(async move |view, cx| {
            timer.await;
            let query_for_search = query.clone();
            let search = cx.background_executor().spawn(async move {
                let workspace =
                    LocalWorkspace::open(database_path).map_err(|error| error.to_string())?;
                workspace
                    .search_with_kind(&query_for_search, 200, search_kind)
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

    /// 切换搜索类型过滤并立即用当前查询重发搜索（复用同一防抖）。
    fn set_search_kind(&mut self, kind: Option<NodeKind>, cx: &mut Context<Self>) {
        self.search_kind = kind;
        if !self.search_query.trim().is_empty() {
            let query = self.search_query.clone();
            self.schedule_search(query, cx);
        } else {
            cx.notify();
        }
    }

    fn create_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn create_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn create_bookmark(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let parent = match self.creation_parent() {
            Some(parent) => Some(parent),
            // 未选中目录时书签先落入收集箱，不再直接挂到首页。
            None => match self.service.ensure_inbox() {
                Ok(inbox) => Some(inbox),
                Err(error) => {
                    self.status = Some(error.to_string().into());
                    cx.notify();
                    return;
                }
            },
        };
        match self
            .service
            .create_bookmark(parent.as_deref(), "新建书签", "https://example.com/")
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

    fn creation_parent(&self) -> Option<String> {
        self.explorer.selected_folder_id().or_else(|| {
            (self.editor.selected_kind == Some(NodeKind::Folder))
                .then(|| self.editor.selected_id.clone())
                .flatten()
        })
    }

    fn load_node_into_editor(
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
                let (body, editable, protected_blocks) = self.node_body(&node);
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

    fn open_node(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
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

    fn activate_engine(
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
                        input.set_value(content.url, window, cx);
                    });
                    self.editor.suppress_events = false;
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

    fn mark_editor_dirty(&mut self, cx: &mut Context<Self>) {
        if self.show_trash || self.editor.mark_dirty().is_none() {
            return;
        }
        self.schedule_draft_save(DRAFT_AUTOSAVE_DELAY, cx);
        cx.notify();
    }

    fn schedule_draft_save(&mut self, delay: Duration, cx: &mut Context<Self>) {
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

    fn detach_pending_autosave(&mut self, cx: &mut Context<Self>) {
        if self.editor.dirty {
            self.schedule_draft_save(Duration::ZERO, cx);
        }
        self.editor.draft_revision = self.editor.draft_revision.wrapping_add(1);
        if let Some(task) = self.editor.autosave_task.take() {
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
        let Some(id) = self.editor.selected_id.clone() else {
            return;
        };
        if self.editor.selected_kind != Some(NodeKind::Bookmark) {
            return;
        }

        if self.editor.dirty {
            let title = self.editor.title_input.read(cx).value().to_string();
            let url = self.editor.body_input.read(cx).value().to_string();
            if let Err(error) = self.service.save_edits(&id, title, Some(&url)) {
                self.status = Some(error.to_string().into());
                cx.notify();
                return;
            }
            self.editor.dirty = false;
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
        self.editor.active_engine_id = Some(engine_id.into());
        self.navigate_browser_to(url, true, window, cx);
    }

    fn open_file_with_system(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.editor.selected_id.clone() else {
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

    fn move_explorer_selection_to_trash(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn restore_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn purge_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
        self.explorer.clear_selection();
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

    fn fill_agent_prompt(
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

    fn toggle_agent_panel(&mut self, cx: &mut Context<Self>) {
        self.editor.workspace.agent.visible = !self.editor.workspace.agent.visible;
        if self.editor.workspace.agent.visible {
            self.load_agent_activity(cx);
        }
        self.persist_workspace();
        cx.notify();
    }

    fn focus_agent_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn toggle_agent_document_context(&mut self, cx: &mut Context<Self>) {
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

    fn select_agent_thread(&mut self, thread_id: String, cx: &mut Context<Self>) {
        if self.agent_in_progress {
            return;
        }
        match self.service.agent_transcript(&thread_id) {
            Ok(messages) => {
                self.agent_thread_id = Some(thread_id);
                self.agent_transcript = messages;
                self.agent_scroll_handle.scroll_to_bottom();
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
        let task = cx.background_executor().spawn(async move {
            let mut workspace =
                LocalWorkspace::open(database_path).map_err(|error| error.to_string())?;
            let mut provider =
                OpenAiCompatibleClient::new(&settings.base_url, &settings.model, &key)
                    .map_err(|error| error.to_string())?;
            workspace
                .run_agent_turn_with_context(
                    &mut provider,
                    thread_id.as_deref(),
                    &prompt,
                    document_context.as_ref(),
                )
                .map_err(|error| error.to_string())
        });
        self.agent_in_progress = true;
        self.status = Some("Agent 正在本机直连模型服务……".into());
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

        let sync_code = self.sync_code_input.read(cx).value().to_string();
        if !sync_code.trim().is_empty() {
            if !is_valid_sync_code(&sync_code) {
                return Err("同步码必须恰好包含 32 位十六进制字符".into());
            }
            self.secret_store
                .set(SecretKey::SyncCode, &normalize_sync_code(&sync_code))
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
        let sync_code = match self.secret_store.get(SecretKey::SyncCode) {
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
                .sync_subscriptions(&sync_code, &mut transport)
                .map_err(|error| error.to_string())?;
            let notes = workspace
                .sync_notes(&sync_code, &mut transport)
                .map_err(|error| error.to_string())?;
            let bookmarks = workspace
                .sync_bookmarks(&sync_code, &mut transport)
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
        self.explorer.clear_selection();
        self.clear_selection(window, cx);
        self.refresh_items();
        cx.notify();
    }

    fn show_inbox(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn clear_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.detach_pending_autosave(cx);
        self.editor.clear(window, cx);
        self.browser.clear(window, cx);
    }

    fn toggle_folder(&mut self, id: String, cx: &mut Context<Self>) {
        self.explorer.toggle_folder(id);
        self.persist_workspace();
        cx.notify();
    }

    fn show_editor_find(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
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

    fn hide_editor_find(&mut self, cx: &mut Context<Self>) {
        self.editor.find_visible = false;
        self.editor.replace_visible = false;
        self.editor.find_cursor = 0;
        cx.notify();
    }

    fn toggle_note_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn find_next_in_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn replace_all_in_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn toggle_project_panel(&mut self, cx: &mut Context<Self>) {
        self.explorer.toggle_panel();
        self.persist_workspace();
        cx.notify();
    }

    fn begin_explorer_rename(
        &mut self,
        id: String,
        title: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.explorer.begin_rename(id, title, window, cx);
    }

    fn begin_selected_explorer_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn open_selected_explorer_node(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn select_relative_explorer_node(&mut self, offset: isize, cx: &mut Context<Self>) {
        let preserve_full_list = self.active_section.as_ref() != "我的";
        if self.explorer.select_relative(preserve_full_list, offset) {
            cx.notify();
        }
    }

    fn cancel_explorer_rename(&mut self, cx: &mut Context<Self>) {
        self.explorer.cancel_rename();
        cx.notify();
    }

    fn commit_explorer_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn move_explorer_node(
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

    fn activate_tab(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(file_id) = self.editor.activate_tab(&id) {
            self.open_node(file_id, window, cx);
        }
        self.persist_workspace();
    }

    fn close_tab(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
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

    fn persist_workspace(&mut self) {
        self.editor.workspace.explorer = self.explorer.layout_state();
        if let Err(error) = self.service.save_workspace_state(&self.editor.workspace) {
            self.status = Some(error.to_string().into());
        }
    }

    fn set_engine_preference(
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

fn engine_id(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Folder => "ideall.directory",
        NodeKind::Note => "ideall.note",
        NodeKind::Bookmark => "ideall.bookmark",
        NodeKind::File => "ideall.text",
        NodeKind::Feed => "ideall.feed",
        NodeKind::Thread => "ideall.thread",
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

fn main() {
    select_linux_backend();
    let secure_store_error = ideall_secrets::initialize_platform()
        .err()
        .map(|error| format!("无法初始化系统安全存储：{error}"));
    Application::new()
        .with_assets(IdeallAssets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            Theme::change(ThemeMode::Light, None, cx);
            configure_desktop_theme(cx);
            workbench::init(cx);
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

fn configure_desktop_theme(cx: &mut App) {
    let theme = Theme::global_mut(cx);
    theme.primary = rgb(ACCENT).into();
    theme.primary_hover = rgb(ACCENT_HOVER).into();
    theme.primary_active = rgb(ACCENT_ACTIVE).into();
    theme.primary_foreground = rgb(SURFACE).into();
    theme.ring = rgb(ACCENT).into();
    theme.selection = rgb(SELECTION).into();
    theme.accent = rgb(ACCENT_SOFT).into();
    theme.accent_foreground = rgb(INK).into();
    theme.muted = rgb(SURFACE_MUTED).into();
    theme.muted_foreground = rgb(INK_MUTED).into();
    theme.secondary = rgb(SURFACE_MUTED).into();
    theme.secondary_foreground = rgb(INK).into();
    theme.border = rgb(BORDER).into();
    theme.input = rgb(BORDER_STRONG).into();
    theme.radius = px(8.0);
    theme.radius_lg = px(10.0);
    theme.shadow = false;
}
