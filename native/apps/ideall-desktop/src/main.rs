mod agent;
mod assets;
mod browser;
mod documents;
mod editor;
mod explorer;
mod sync;
mod update;
mod webmeta;
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
use ideall_agent::{AgentAuditRecord, AuditStatus, ModelRole};
use ideall_application::{
    AGENT_MODEL_SETTINGS_SAVED_STATUS, AGENT_PROMPT_EMPTY_STATUS, AGENT_THREAD_OPENED_STATUS,
    AGENT_TURN_IN_PROGRESS_STATUS, AgentDocumentContext, AgentModelSettingsDraft, AgentToolRun,
    AgentTranscriptMessage, BookmarkMetadata, CredentialKey, CredentialStore, ExternalAcpSettings,
    HOME_ROOT_ID, INBOX_ROOT_ID, LocalWorkspace, MAX_IMPORT_BYTES, NEW_AGENT_THREAD_STATUS,
    NodeSummary, NoteBlockIndent, NoteBlockKind, NoteBlockMove, NoteBlockSpec, NoteBlockStyle,
    NoteColor, NoteInlineStyle, NoteSlashCommand, PersistedAgentModelSettings,
    PersistedSyncSettings, SEARCH_IN_PROGRESS_STATUS, SYNC_CREDENTIALS_CLEARED_STATUS,
    SYNC_IN_PROGRESS_STATUS, SYNC_SETTINGS_SAVED_STATUS, SyncSettingsDraft, clear_sync_credentials,
    credential_configured, note_mention_query, note_node_id_from_url, note_node_url,
    note_slash_commands, note_slash_query, parse_note_blocks, render_note_block,
    require_credential, search_failed_status, search_results_status, sync_secret_status,
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
use ideall_sync::transport::HttpSyncTransport;
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

/// Backs the application credential use cases with the platform keyring so
/// the application crate never links the secrets adapter.
struct PlatformCredentialStore;

impl CredentialStore for PlatformCredentialStore {
    fn get(&self, key: CredentialKey) -> Result<Option<String>, String> {
        SystemSecretStore
            .get(secret_key(key))
            .map_err(|error| error.to_string())
    }

    fn set(&self, key: CredentialKey, value: &str) -> Result<(), String> {
        SystemSecretStore
            .set(secret_key(key), value)
            .map_err(|error| error.to_string())
    }

    fn delete(&self, key: CredentialKey) -> Result<(), String> {
        SystemSecretStore
            .delete(secret_key(key))
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

fn secret_key(key: CredentialKey) -> SecretKey {
    match key {
        CredentialKey::SyncCode => SecretKey::SyncCode,
        CredentialKey::SyncBearerToken => SecretKey::SyncBearerToken,
        CredentialKey::AgentCredential => SecretKey::AgentCredential,
    }
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
    secret_store: PlatformCredentialStore,
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
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索标题与正文"));
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
        let secret_store = PlatformCredentialStore;
        let agent_key_configured =
            credential_configured(&secret_store, CredentialKey::AgentCredential).unwrap_or(false);
        let agent_thread_id = service.latest_agent_thread_id().ok().flatten();
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
                    "活动" => {
                        // 最近更新只呈现内容项，目录容器不进入时间线。
                        items.retain(|item| item.kind != NodeKind::Folder);
                        items.sort_by(|left, right| {
                            right
                                .updated_at
                                .cmp(&left.updated_at)
                                .then_with(|| left.id.cmp(&right.id))
                        });
                    }
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
