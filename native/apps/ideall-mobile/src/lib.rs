//! Native iOS and Android entry points for ideall.

#[cfg(any(
    target_os = "ios",
    target_os = "android",
    feature = "mobile-ui-host-check"
))]
mod native_text;

#[cfg(target_os = "ios")]
mod ios_host;

#[cfg(any(target_os = "ios", feature = "mobile-ui-host-check"))]
static IOS_UI_TEST_ACTION: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub extern "C" fn ideall_mobile_ios_ui_test_action(action: u8) {
    if std::env::var("IDEALL_UI_TESTING").as_deref() != Ok("1") || !(1..=3).contains(&action) {
        return;
    }
    IOS_UI_TEST_ACTION.store(action, std::sync::atomic::Ordering::Release);
    gpui_mobile::TEXT_INPUT_DIRTY.store(true, std::sync::atomic::Ordering::Release);
    eprintln!("ideall iOS UI-test action queued: {action}");
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_jinziyou_ideall_IdeallNativeActivity_nativeOnTextInput(
    env: *mut std::ffi::c_void,
    activity: *mut std::ffi::c_void,
    value: *mut std::ffi::c_void,
    selection_start: i32,
    selection_end: i32,
    composing: u8,
) {
    // SAFETY: Android invokes this root-level JNI entry with references valid
    // for the duration of the call; `android_on_text_input` copies the string.
    unsafe {
        native_text::android_on_text_input(
            env,
            activity,
            value,
            selection_start,
            selection_end,
            composing,
        );
    }
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_jinziyou_ideall_IdeallNativeActivity_nativeSetSafeAreaInsets(
    _env: *mut std::ffi::c_void,
    _activity: *mut std::ffi::c_void,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
) {
    native_text::set_android_safe_area_insets(left, top, right, bottom);
}

#[cfg(target_os = "android")]
fn register_android_native_bridges(app: &android_activity::AndroidApp) -> Result<(), String> {
    use std::ffi::c_void;

    use jni::{JavaVM, NativeMethod, objects::JObject};

    // SAFETY: AndroidApp owns these VM and Activity references for the whole
    // android_main invocation. Treat the Activity pointer as a borrowed global
    // reference so jni-rs never attempts to delete it.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    let raw_activity = app.activity_as_ptr() as jni::sys::jobject;
    vm.attach_current_thread(|env| -> Result<(), jni::errors::Error> {
        // SAFETY: android-activity guarantees that activity_as_ptr remains
        // valid until android_main returns.
        let activity = unsafe { env.as_cast_raw::<JObject>(&raw_activity)? };
        let activity_class = env.get_object_class(&activity)?;
        let methods = [
            // SAFETY: The function has the JNI instance-method ABI described
            // by (Ljava/lang/String;IIZ)V.
            unsafe {
                NativeMethod::from_raw_parts(
                    jni::jni_str!("nativeOnTextInput"),
                    jni::jni_str!("(Ljava/lang/String;IIZ)V"),
                    Java_com_jinziyou_ideall_IdeallNativeActivity_nativeOnTextInput as *const ()
                        as *mut c_void,
                )
            },
            // SAFETY: The function has the JNI instance-method ABI described
            // by (IIII)V.
            unsafe {
                NativeMethod::from_raw_parts(
                    jni::jni_str!("nativeSetSafeAreaInsets"),
                    jni::jni_str!("(IIII)V"),
                    Java_com_jinziyou_ideall_IdeallNativeActivity_nativeSetSafeAreaInsets
                        as *const () as *mut c_void,
                )
            },
        ];
        // SAFETY: Both descriptors point to functions with matching instance
        // method signatures on the concrete Activity class.
        unsafe {
            env.register_native_methods(&activity_class, &methods)?;
        }
        env.call_method(
            &activity,
            jni::jni_str!("onIdeallNativeReady"),
            jni::jni_sig!("()V"),
            &[],
        )?;
        Ok(())
    })
    .map_err(|error| error.to_string())
}

#[cfg(any(
    target_os = "ios",
    target_os = "android",
    feature = "mobile-ui-host-check",
    test
))]
mod editor;

#[cfg(any(
    target_os = "ios",
    target_os = "android",
    feature = "mobile-ui-host-check"
))]
mod mobile {
    mod view;

    use std::{cell::RefCell, path::PathBuf, time::Duration};

    use crate::{
        editor::{
            BlockFormatError as MobileBlockFormatError, BlockMove as MobileBlockMove,
            BlockStyle as MobileBlockStyle, apply_block_style as apply_mobile_block_style,
            apply_input_at_cursor as apply_mobile_input_at_cursor,
            apply_slash_command as apply_mobile_slash_command,
            cursor_line_index as mobile_cursor_line_index,
            enqueue_control_key as enqueue_mobile_control_key,
            line_with_cursor as mobile_line_with_cursor, move_block as move_mobile_block,
            native_edit_committed as mobile_native_edit_committed,
            slash_commands as mobile_slash_commands, slash_query as mobile_slash_query,
            text_line_ranges as mobile_text_line_ranges,
        },
        native_text,
    };
    use gpui_mobile::gpui::{
        App, Context, FocusHandle, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, Render,
        ScrollHandle, SharedString, Subscription, Task, Window, WindowOptions, div, point,
        prelude::*, px, rgb,
    };
    use gpui_mobile::{
        KeyboardType,
        components::{
            material::{text_input::TextInput, theme::MaterialTheme},
            platform_view_element::platform_view_element,
        },
        packages::webview::{WebViewHandle, WebViewSettings},
    };
    use ideall_application::{
        AgentAuditRecord, AgentModelSettings, AgentTranscriptMessage, AuditStatus, HOME_ROOT_ID,
        LocalWorkspace, ModelRole, NodeSummary, OpenAiCompatibleClient, SyncSettings,
    };
    use ideall_domain::{
        EnginePlatform, TabDescriptor, WorkspaceState, engine_runtime_capabilities, tab_key,
    };
    use ideall_presentation::{
        LatestRequest, NAVIGATION,
        color::{
            ACCENT, ACCENT_SOFT, BORDER, CANVAS, DANGER, DANGER_BORDER, DANGER_SOFT, DISABLED, INK,
            INK_MUTED, MESSAGE_TOOL, RAIL, RAIL_MUTED, SURFACE_MUTED, SURFACE_RAISED, WARNING_SOFT,
        },
        section_status,
    };
    use ideall_protocol::{EngineAccess, FileRef, Node, NodeKind, SubscriptionType};
    use ideall_secrets::{SecretKey, SecretStore as _, SystemSecretStore};
    use ideall_sync::{is_valid_sync_code, normalize_sync_code};
    use ideall_sync_http::HttpSyncTransport;

    const INFO_PORTAL_URL: &str = "https://www.wonita.link/info";
    const COMMUNITY_PORTAL_URL: &str = "https://www.wonita.link/community";
    const DRAFT_AUTOSAVE_DELAY: Duration = Duration::from_millis(700);
    const SEARCH_DEBOUNCE_DELAY: Duration = Duration::from_millis(180);

    thread_local! {
        static PENDING_TEXT: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum FocusedField {
        Search,
        FeedKey,
        Title,
        Body,
        SyncServer,
        SyncCode,
        SyncToken,
        AgentBaseUrl,
        AgentModel,
        AgentKey,
        AgentPrompt,
    }

    struct MobileStartupFailure {
        message: String,
    }

    impl Render for MobileStartupFailure {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let (safe_top, safe_bottom, safe_left, safe_right) = gpui_mobile::safe_area_insets();
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .pt(px(safe_top))
                .pb(px(safe_bottom))
                .pl(px(safe_left))
                .pr(px(safe_right))
                .bg(rgb(CANVAS))
                .child(
                    div()
                        .m_6()
                        .max_w(px(560.))
                        .p_6()
                        .rounded_xl()
                        .border_1()
                        .border_color(rgb(DANGER_BORDER))
                        .bg(rgb(SURFACE_RAISED))
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(
                            div()
                                .text_xl()
                                .text_color(rgb(DANGER))
                                .child("ideall 无法启动"),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(INK))
                                .whitespace_normal()
                                .child(self.message.clone()),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(INK_MUTED))
                                .child("请检查设备存储空间与系统安全存储权限，然后重新打开应用。"),
                        ),
                )
        }
    }

    struct IdeallMobile {
        database_path: PathBuf,
        service: LocalWorkspace,
        workspace: WorkspaceState,
        secret_store: SystemSecretStore,
        items: Vec<NodeSummary>,
        selected_id: Option<String>,
        selected_kind: Option<NodeKind>,
        active_section: &'static str,
        show_trash: bool,
        search_query: String,
        search_requests: LatestRequest,
        search_task: Option<Task<()>>,
        feed_key_draft: String,
        draft_title: String,
        draft_body: String,
        sync_server_draft: String,
        sync_code_draft: String,
        sync_token_draft: String,
        agent_base_url_draft: String,
        agent_model_draft: String,
        agent_key_draft: String,
        agent_prompt_draft: String,
        sync_code_configured: bool,
        sync_token_configured: bool,
        sync_in_progress: bool,
        agent_key_configured: bool,
        agent_in_progress: bool,
        agent_thread_id: Option<String>,
        agent_transcript: Vec<AgentTranscriptMessage>,
        agent_audits: Vec<AgentAuditRecord>,
        agent_threads: Vec<NodeSummary>,
        agent_activity_loading: bool,
        embedded_browser: Option<WebViewHandle>,
        body_editable: bool,
        focus_handle: FocusHandle,
        body_scroll_handle: ScrollHandle,
        body_last_revealed_cursor: Option<usize>,
        focused_field: Option<FocusedField>,
        focused_selection_start: usize,
        focused_cursor: usize,
        native_input_composing: bool,
        observed_pending_chunks: usize,
        draft_revision: u64,
        autosave_task: Option<Task<()>>,
        dirty: bool,
        purge_armed: bool,
        status: String,
        _window_activation: Subscription,
    }

    impl IdeallMobile {
        fn new(
            database_path: PathBuf,
            service: LocalWorkspace,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) -> Self {
            let items = service.list_home().unwrap_or_default();
            let workspace = service.load_workspace_state().unwrap_or_default();
            let restore_id = workspace.active_id.as_deref().and_then(|active_id| {
                workspace
                    .tabs
                    .iter()
                    .find(|tab| tab_key(tab) == active_id)
                    .map(|tab| tab.file.file_id.clone())
            });
            let sync_server_draft = service
                .load_sync_settings()
                .unwrap_or_default()
                .server_base_url;
            let agent_settings = service.load_agent_model_settings().unwrap_or_default();
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
            let window_activation =
                cx.observe_window_activation(window, Self::window_activation_changed);
            let mut this = Self {
                database_path,
                service,
                workspace,
                secret_store,
                items,
                selected_id: None,
                selected_kind: None,
                active_section: "我的",
                show_trash: false,
                search_query: String::new(),
                search_requests: LatestRequest::default(),
                search_task: None,
                feed_key_draft: String::new(),
                draft_title: String::new(),
                draft_body: String::new(),
                sync_server_draft,
                sync_code_draft: String::new(),
                sync_token_draft: String::new(),
                agent_base_url_draft: agent_settings.base_url,
                agent_model_draft: agent_settings.model,
                agent_key_draft: String::new(),
                agent_prompt_draft: String::new(),
                sync_code_configured: false,
                sync_token_configured: false,
                sync_in_progress: false,
                agent_key_configured,
                agent_in_progress: false,
                agent_thread_id,
                agent_transcript,
                agent_audits: Vec::new(),
                agent_threads: Vec::new(),
                agent_activity_loading: false,
                embedded_browser: None,
                body_editable: false,
                focus_handle: cx.focus_handle(),
                body_scroll_handle: ScrollHandle::new(),
                body_last_revealed_cursor: None,
                focused_field: None,
                focused_selection_start: 0,
                focused_cursor: 0,
                native_input_composing: false,
                observed_pending_chunks: 0,
                draft_revision: 0,
                autosave_task: None,
                dirty: false,
                purge_armed: false,
                status: "所有内容保存在本机".into(),
                _window_activation: window_activation,
            };
            if let Some(id) = restore_id
                && let Ok(node) = this.service.node(&id)
            {
                let (text, editable, protected_blocks) = this.node_body(&node);
                this.draft_title = node.base().title.clone();
                this.selected_id = Some(id);
                this.selected_kind = Some(node.kind());
                this.draft_body = text;
                this.body_editable = editable;
                this.status = if protected_blocks > 0 {
                    format!("已恢复项目；{protected_blocks} 个未知富文本块以指纹占位保护")
                } else {
                    "已恢复最近打开的本地项目".into()
                };
            }
            this
        }

        fn refresh(&mut self) {
            let result = if self.show_trash {
                self.service.list_trash()
            } else if !self.search_query.trim().is_empty() {
                self.service.search(&self.search_query, 200)
            } else {
                self.service.list_home().map(|mut items| {
                    match self.active_section {
                        "活动" => items.sort_by(|left, right| {
                            right
                                .updated_at
                                .cmp(&left.updated_at)
                                .then_with(|| left.id.cmp(&right.id))
                        }),
                        "浏览" => items.retain(|item| {
                            matches!(item.kind, NodeKind::Bookmark | NodeKind::Feed)
                        }),
                        "应用" | "设置" => items.clear(),
                        _ => {}
                    }
                    items
                })
            };
            match result {
                Ok(items) => self.items = items,
                Err(error) => self.status = error.to_string(),
            }
        }

        fn schedule_search(&mut self, cx: &mut Context<Self>) {
            self.show_trash = false;
            let request = self.search_requests.start();
            drop(self.search_task.take());
            let query = self.search_query.trim().to_owned();
            if query.is_empty() {
                self.active_section = "我的";
                self.refresh();
                self.status = section_status("我的").into();
                cx.notify();
                return;
            }

            self.active_section = "搜索";
            self.status = "正在搜索本地内容…".into();
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
                cx.update(|cx| {
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
                                this.status = format!("找到 {count} 条本地内容");
                            }
                            Err(error) => {
                                this.status = format!("搜索失败：{error}");
                            }
                        }
                        cx.notify();
                    });
                })
            }));
            cx.notify();
        }

        fn creation_parent(&self) -> Option<String> {
            (self.selected_kind == Some(NodeKind::Folder))
                .then(|| self.selected_id.clone())
                .flatten()
        }

        fn create_note(&mut self, cx: &mut Context<Self>) {
            let parent = self.creation_parent();
            match self.service.create_note(parent.as_deref(), "无标题笔记") {
                Ok(node) => {
                    self.show_trash = false;
                    self.refresh();
                    self.open_node(node.base().id.clone(), cx);
                    self.focus_field(FocusedField::Body, KeyboardType::Default);
                }
                Err(error) => self.status = error.to_string(),
            }
            cx.notify();
        }

        fn create_folder(&mut self, cx: &mut Context<Self>) {
            let parent = self.creation_parent();
            match self.service.create_folder(parent.as_deref(), "新建文件夹") {
                Ok(node) => {
                    self.show_trash = false;
                    self.refresh();
                    self.open_node(node.base().id.clone(), cx);
                    self.focus_field(FocusedField::Title, KeyboardType::Default);
                }
                Err(error) => self.status = error.to_string(),
            }
            cx.notify();
        }

        fn create_bookmark(&mut self, cx: &mut Context<Self>) {
            let parent = self.creation_parent();
            match self.service.create_bookmark(
                parent.as_deref(),
                "新建书签",
                "https://example.com/",
            ) {
                Ok(node) => {
                    self.show_trash = false;
                    self.refresh();
                    self.open_node(node.base().id.clone(), cx);
                    self.focus_field(FocusedField::Title, KeyboardType::Default);
                    self.status = "请填写标题与 http(s) 地址".into();
                }
                Err(error) => self.status = error.to_string(),
            }
            cx.notify();
        }

        fn create_text_file(&mut self, cx: &mut Context<Self>) {
            let parent = self.creation_parent();
            match self.service.create_file(
                parent.as_deref(),
                "新建文本.txt",
                "text/plain",
                Vec::new(),
            ) {
                Ok(node) => {
                    self.show_trash = false;
                    self.refresh();
                    self.open_node(node.base().id.clone(), cx);
                    self.focus_field(FocusedField::Body, KeyboardType::Default);
                }
                Err(error) => self.status = error.to_string(),
            }
            cx.notify();
        }

        fn import_files(&mut self, cx: &mut Context<Self>) {
            self.stop_editing();
            let parent = self.creation_parent();
            self.status = "正在打开系统文件选择器……".into();
            cx.spawn(async move |view, cx| {
                let selection = cx
                    .background_executor()
                    .spawn(async { select_mobile_documents() })
                    .await;
                cx.update(|cx| {
                    let Some(view) = view.upgrade() else {
                        return;
                    };
                    view.update(cx, |this, cx| {
                        match selection {
                            Ok(files) if files.is_empty() => {
                                this.status = "已取消导入".into();
                            }
                            Ok(files) => {
                                let mut imported = 0_usize;
                                for file in files {
                                    let import_path = match materialize_mobile_document(
                                        &this.database_path,
                                        &file.path,
                                    ) {
                                        Ok(path) => path,
                                        Err(error) => {
                                            this.status = format!(
                                                "已导入 {imported} 个文件；{} 导入失败：{error}",
                                                file.name
                                            );
                                            this.refresh();
                                            cx.notify();
                                            return;
                                        }
                                    };
                                    match this.service.import_external_file(
                                        parent.as_deref(),
                                        &import_path,
                                        &file.name,
                                        256 * 1024 * 1024,
                                    ) {
                                        Ok(_) => imported += 1,
                                        Err(error) => {
                                            this.status = format!(
                                                "已导入 {imported} 个文件；{} 导入失败：{error}",
                                                file.name
                                            );
                                            this.refresh();
                                            cx.notify();
                                            return;
                                        }
                                    }
                                }
                                this.refresh();
                                this.status = format!("已导入 {imported} 个文件");
                            }
                            Err(error) => this.status = error,
                        }
                        cx.notify();
                    });
                })
            })
            .detach();
            cx.notify();
        }

        fn create_agent_draft(&mut self, cx: &mut Context<Self>) {
            self.stop_editing();
            match self.service.create_agent_note_via_mcp("Agent 草稿") {
                Ok(node) => {
                    self.refresh();
                    self.status = format!(
                        "Agent 已通过本地 MCP 创建空白笔记 {}；写操作审计已提交",
                        node.base().id
                    );
                }
                Err(error) => self.status = error.to_string(),
            }
            self.load_agent_activity(cx);
            cx.notify();
        }

        fn persist_agent_model_settings(&mut self) -> Result<(), String> {
            let saved = self
                .service
                .save_agent_model_settings(&AgentModelSettings {
                    base_url: self.agent_base_url_draft.clone(),
                    model: self.agent_model_draft.clone(),
                })
                .map_err(|error| error.to_string())?;
            self.agent_base_url_draft = saved.base_url.clone();
            self.agent_model_draft = saved.model.clone();
            if !self.agent_key_draft.trim().is_empty() {
                let key = self.agent_key_draft.trim();
                OpenAiCompatibleClient::new(&saved.base_url, &saved.model, key)
                    .map_err(|error| error.to_string())?;
                self.secret_store
                    .set(SecretKey::AgentCredential, key)
                    .map_err(|error| error.to_string())?;
                self.agent_key_draft.clear();
                self.agent_key_configured = true;
            }
            Ok(())
        }

        fn save_agent_model_settings(&mut self, cx: &mut Context<Self>) {
            self.stop_editing();
            self.status = match self.persist_agent_model_settings() {
                Ok(()) => "模型设置已保存；API Key 仅在系统安全存储中".into(),
                Err(error) => error,
            };
            cx.notify();
        }

        fn clear_agent_credential(&mut self, cx: &mut Context<Self>) {
            self.stop_editing();
            match self.secret_store.delete(SecretKey::AgentCredential) {
                Ok(_) => {
                    self.agent_key_draft.clear();
                    self.agent_key_configured = false;
                    self.status = "模型 API Key 已从系统安全存储清除".into();
                }
                Err(error) => self.status = error.to_string(),
            }
            cx.notify();
        }

        fn new_agent_thread(&mut self, cx: &mut Context<Self>) {
            if self.agent_in_progress {
                return;
            }
            self.stop_editing();
            self.agent_thread_id = None;
            self.agent_transcript.clear();
            self.agent_prompt_draft.clear();
            self.status = "已开始新对话；发送第一条消息后写入本地 SQLite".into();
            cx.notify();
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
                                this.status = format!("加载 Agent 活动失败：{error}");
                            }
                        }
                        cx.notify();
                    });
                })
            })
            .detach();
        }

        fn select_agent_thread(&mut self, thread_id: String, cx: &mut Context<Self>) {
            if self.agent_in_progress {
                return;
            }
            self.stop_editing();
            match self.service.agent_transcript(&thread_id) {
                Ok(messages) => {
                    self.agent_thread_id = Some(thread_id);
                    self.agent_transcript = messages;
                    self.status = "已打开本地 Agent 对话".into();
                }
                Err(error) => self.status = error.to_string(),
            }
            cx.notify();
        }

        fn start_agent_turn(&mut self, cx: &mut Context<Self>) {
            if self.agent_in_progress {
                return;
            }
            self.stop_editing();
            if let Err(error) = self.persist_agent_model_settings() {
                self.status = error;
                cx.notify();
                return;
            }
            let prompt = self.agent_prompt_draft.clone();
            if prompt.trim().is_empty() {
                self.status = "请输入 Agent 消息".into();
                cx.notify();
                return;
            }
            let settings = match self.service.load_agent_model_settings() {
                Ok(settings) => settings,
                Err(error) => {
                    self.status = error.to_string();
                    cx.notify();
                    return;
                }
            };
            let key = match self.secret_store.get(SecretKey::AgentCredential) {
                Ok(Some(key)) => key,
                Ok(None) => {
                    self.status = "请先安全保存模型 API Key".into();
                    cx.notify();
                    return;
                }
                Err(error) => {
                    self.status = error.to_string();
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
            self.status = "Agent 正在本机直连模型服务……".into();
            cx.spawn(async move |view, cx| {
                let result = task.await;
                cx.update(|cx| {
                    let Some(view) = view.upgrade() else {
                        return;
                    };
                    view.update(cx, |this, cx| {
                        this.agent_in_progress = false;
                        match result {
                            Ok(result) => {
                                this.agent_thread_id = Some(result.thread_id.clone());
                                this.agent_transcript = this
                                    .service
                                    .agent_transcript(&result.thread_id)
                                    .unwrap_or_default();
                                this.agent_prompt_draft.clear();
                                this.refresh();
                                let failures = result.tools.iter().filter(|tool| !tool.ok).count();
                                this.status = if result.tools.is_empty() {
                                    "Agent 已回复；对话保存在本地 SQLite".into()
                                } else {
                                    format!(
                                        "Agent 已回复；执行 {} 个工具，{} 个失败；写操作均已审计",
                                        result.tools.len(),
                                        failures
                                    )
                                };
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
                                    this.agent_transcript = this
                                        .service
                                        .agent_transcript(thread_id)
                                        .unwrap_or_default();
                                }
                                this.status = error;
                            }
                        }
                        this.load_agent_activity(cx);
                        cx.notify();
                    });
                })
            })
            .detach();
            cx.notify();
        }

        fn create_feed(&mut self, cx: &mut Context<Self>) {
            self.stop_editing();
            let key = self.feed_key_draft.clone();
            match self
                .service
                .create_feed(key.trim(), SubscriptionType::Publisher, &key)
            {
                Ok(node) => {
                    self.feed_key_draft.clear();
                    self.refresh();
                    self.status = format!(
                        "已关注发布者 {}；可通过加密同步与旧客户端互通",
                        node.base().title
                    );
                }
                Err(error) => self.status = error.to_string(),
            }
            cx.notify();
        }

        fn open_node(&mut self, id: String, cx: &mut Context<Self>) {
            if self.dirty && self.selected_id.as_deref() == Some(id.as_str()) {
                return;
            }
            if self.selected_id.as_deref() != Some(id.as_str()) {
                self.detach_pending_autosave(cx);
            }
            self.stop_editing();
            self.embedded_browser = None;
            match self.service.node(&id) {
                Ok(node) => {
                    let (text, editable, protected_blocks) = self.node_body(&node);
                    let title = node.base().title.clone();
                    let kind = node.kind();
                    self.draft_title = node.base().title.clone();
                    self.selected_id = Some(id.clone());
                    self.selected_kind = Some(kind);
                    self.draft_body = text;
                    self.body_editable = editable;
                    self.dirty = false;
                    self.purge_armed = false;
                    self.status = if node.kind() == NodeKind::Note && protected_blocks > 0 {
                        format!(
                            "可编辑常用 Markdown 块；{protected_blocks} 个未知块以指纹占位保护，请勿修改占位行"
                        )
                    } else {
                        "本地内容".into()
                    };
                    self.workspace.tabs.clear();
                    self.workspace.active_id = None;
                    self.workspace.open(TabDescriptor {
                        file: FileRef::new("local.nodes", &id),
                        engine_id: mobile_engine_id(kind).into(),
                        title,
                        root_id: Some(HOME_ROOT_ID.into()),
                        navigation_path: Some(format!("/home/{id}")),
                    });
                    self.persist_workspace();
                }
                Err(error) => self.status = error.to_string(),
            }
            cx.notify();
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
                                "{} · {} 字节\nBlob：{}",
                                blob_ref.mime, blob_ref.size, blob_ref.key
                            ),
                            false,
                            0,
                        )
                    }),
                Node::Folder { .. } => ("文件夹".into(), false, 0),
                Node::Feed { content, .. } => (content.key.clone(), false, 0),
                Node::Thread { content, .. } => {
                    (format!("{} 条消息", content.messages.len()), false, 0)
                }
            }
        }

        fn focus_field(&mut self, field: FocusedField, keyboard_type: KeyboardType) {
            if field == FocusedField::Body && !self.body_editable {
                return;
            }
            let focus_changed = self.focused_field != Some(field);
            if focus_changed {
                PENDING_TEXT.with(|pending| pending.borrow_mut().clear());
                self.observed_pending_chunks = 0;
            }
            self.focused_field = Some(field);
            let text_len = match field {
                FocusedField::Search => self.search_query.len(),
                FocusedField::FeedKey => self.feed_key_draft.len(),
                FocusedField::Title => self.draft_title.len(),
                FocusedField::Body => self.draft_body.len(),
                FocusedField::SyncServer => self.sync_server_draft.len(),
                FocusedField::SyncCode => self.sync_code_draft.len(),
                FocusedField::SyncToken => self.sync_token_draft.len(),
                FocusedField::AgentBaseUrl => self.agent_base_url_draft.len(),
                FocusedField::AgentModel => self.agent_model_draft.len(),
                FocusedField::AgentKey => self.agent_key_draft.len(),
                FocusedField::AgentPrompt => self.agent_prompt_draft.len(),
            };
            if focus_changed {
                self.focused_selection_start = text_len;
                self.focused_cursor = text_len;
                self.native_input_composing = false;
            } else {
                self.focused_selection_start = self.focused_selection_start.min(text_len);
                self.focused_cursor = self.focused_cursor.min(text_len);
            }
            if field == FocusedField::Body {
                self.body_last_revealed_cursor = None;
            }
            gpui_mobile::set_text_input_callback(Some(Box::new(|text: &str| {
                PENDING_TEXT.with(|pending| pending.borrow_mut().push(text.to_owned()));
            })));
            if !self.show_native_text_input_for_field(field, keyboard_type) {
                gpui_mobile::show_keyboard_with_type(keyboard_type);
            }
            gpui_mobile::TEXT_INPUT_DIRTY.store(true, std::sync::atomic::Ordering::Release);
        }

        fn show_native_text_input_for_field(
            &self,
            field: FocusedField,
            keyboard_type: KeyboardType,
        ) -> bool {
            let (value, multiline, secure, label) = match field {
                FocusedField::Search => (self.search_query.as_str(), false, false, "搜索"),
                FocusedField::FeedKey => (self.feed_key_draft.as_str(), false, false, "发布者域名"),
                FocusedField::Title => (self.draft_title.as_str(), false, false, "标题"),
                FocusedField::Body => {
                    let label = if self.selected_kind == Some(NodeKind::Bookmark) {
                        "网址"
                    } else {
                        "正文"
                    };
                    (
                        self.draft_body.as_str(),
                        self.selected_kind != Some(NodeKind::Bookmark),
                        false,
                        label,
                    )
                }
                FocusedField::SyncServer => (
                    self.sync_server_draft.as_str(),
                    false,
                    false,
                    "同步服务端基址",
                ),
                FocusedField::SyncCode => (self.sync_code_draft.as_str(), false, true, "同步码"),
                FocusedField::SyncToken => {
                    (self.sync_token_draft.as_str(), false, true, "登录令牌")
                }
                FocusedField::AgentBaseUrl => (
                    self.agent_base_url_draft.as_str(),
                    false,
                    false,
                    "模型服务地址",
                ),
                FocusedField::AgentModel => {
                    (self.agent_model_draft.as_str(), false, false, "模型名称")
                }
                FocusedField::AgentKey => {
                    (self.agent_key_draft.as_str(), false, true, "模型 API Key")
                }
                FocusedField::AgentPrompt => {
                    (self.agent_prompt_draft.as_str(), true, false, "Agent 消息")
                }
            };
            native_text::show(
                value,
                self.focused_selection_start,
                self.focused_cursor,
                keyboard_type,
                multiline,
                secure,
                label,
            )
        }

        fn sync_native_text_input(&self) {
            let Some(field) = self.focused_field else {
                return;
            };
            let keyboard_type = match field {
                FocusedField::FeedKey | FocusedField::SyncServer | FocusedField::AgentBaseUrl => {
                    KeyboardType::URL
                }
                _ => KeyboardType::Default,
            };
            let _ = self.show_native_text_input_for_field(field, keyboard_type);
        }

        fn drain_keyboard(&mut self, cx: &mut Context<Self>) {
            let Some(field) = self.focused_field else {
                return;
            };
            let mut changed = false;
            let native_state = native_text::take_pending_text();
            let native_commit = if let Some(state) = native_state {
                let target = match field {
                    FocusedField::Search => &mut self.search_query,
                    FocusedField::FeedKey => &mut self.feed_key_draft,
                    FocusedField::Title => &mut self.draft_title,
                    FocusedField::Body => &mut self.draft_body,
                    FocusedField::SyncServer => &mut self.sync_server_draft,
                    FocusedField::SyncCode => &mut self.sync_code_draft,
                    FocusedField::SyncToken => &mut self.sync_token_draft,
                    FocusedField::AgentBaseUrl => &mut self.agent_base_url_draft,
                    FocusedField::AgentModel => &mut self.agent_model_draft,
                    FocusedField::AgentKey => &mut self.agent_key_draft,
                    FocusedField::AgentPrompt => &mut self.agent_prompt_draft,
                };
                changed = *target != state.value;
                if changed {
                    *target = state.value;
                }
                self.focused_selection_start = state.selection_start.min(target.len());
                self.focused_cursor = state.selection_end.min(target.len());
                while self.focused_selection_start > 0
                    && !target.is_char_boundary(self.focused_selection_start)
                {
                    self.focused_selection_start -= 1;
                }
                while self.focused_cursor > 0 && !target.is_char_boundary(self.focused_cursor) {
                    self.focused_cursor -= 1;
                }
                let committed = mobile_native_edit_committed(
                    changed,
                    self.native_input_composing,
                    state.composing,
                );
                self.native_input_composing = state.composing;
                PENDING_TEXT.with(|pending| pending.borrow_mut().clear());
                committed
            } else {
                PENDING_TEXT.with(|pending| {
                    for chunk in pending.borrow_mut().drain(..) {
                        let target = match field {
                            FocusedField::Search => &mut self.search_query,
                            FocusedField::FeedKey => &mut self.feed_key_draft,
                            FocusedField::Title => &mut self.draft_title,
                            FocusedField::Body => &mut self.draft_body,
                            FocusedField::SyncServer => &mut self.sync_server_draft,
                            FocusedField::SyncCode => &mut self.sync_code_draft,
                            FocusedField::SyncToken => &mut self.sync_token_draft,
                            FocusedField::AgentBaseUrl => &mut self.agent_base_url_draft,
                            FocusedField::AgentModel => &mut self.agent_model_draft,
                            FocusedField::AgentKey => &mut self.agent_key_draft,
                            FocusedField::AgentPrompt => &mut self.agent_prompt_draft,
                        };
                        changed |= apply_mobile_input_at_cursor(
                            target,
                            &mut self.focused_cursor,
                            &chunk,
                            matches!(field, FocusedField::Body | FocusedField::AgentPrompt),
                        );
                        self.focused_selection_start = self.focused_cursor;
                    }
                });
                changed
            };
            self.observed_pending_chunks = 0;
            if !changed && !native_commit {
                return;
            }
            if field == FocusedField::Search {
                self.schedule_search(cx);
            } else if native_commit && matches!(field, FocusedField::Title | FocusedField::Body) {
                self.mark_draft_dirty(cx);
            }
        }

        #[cfg(any(target_os = "ios", feature = "mobile-ui-host-check"))]
        fn drain_ui_test_action(&mut self, cx: &mut Context<Self>) {
            let action = crate::IOS_UI_TEST_ACTION.swap(0, std::sync::atomic::Ordering::AcqRel);
            if action != 0 {
                eprintln!("ideall iOS UI-test action consumed: {action}");
            }
            match action {
                1 => self.create_note(cx),
                2 => {
                    self.focus_field(FocusedField::Title, KeyboardType::Default);
                    cx.notify();
                }
                3 => {
                    self.focus_field(FocusedField::Body, KeyboardType::Default);
                    cx.notify();
                }
                _ => {}
            }
        }

        fn mark_draft_dirty(&mut self, cx: &mut Context<Self>) {
            self.dirty = true;
            self.purge_armed = false;
            self.draft_revision = self.draft_revision.wrapping_add(1);
            self.schedule_draft_save(DRAFT_AUTOSAVE_DELAY, cx);
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

        fn schedule_draft_save(&mut self, delay: Duration, cx: &mut Context<Self>) {
            if !self.dirty {
                return;
            }
            let Some(id) = self.selected_id.clone() else {
                return;
            };
            let revision = self.draft_revision;
            let title = self.draft_title.clone();
            let body = self.body_editable.then(|| self.draft_body.clone());
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
                cx.update(|cx| {
                    let Some(view) = view.upgrade() else {
                        return;
                    };
                    view.update(cx, |this, cx| {
                        if this.draft_revision != revision {
                            if let Err(error) = &result {
                                this.status = format!("上一项草稿自动保存失败：{error}");
                                cx.notify();
                            }
                            return;
                        }
                        match result {
                            Ok((id, summary)) => {
                                if this.selected_id.as_deref() != Some(id.as_str()) {
                                    return;
                                }
                                if let Some(item) = this.items.iter_mut().find(|item| item.id == id)
                                {
                                    item.title = summary.title;
                                    item.updated_at = summary.updated_at;
                                }
                                this.dirty = false;
                                this.status = "已自动保存到本机".into();
                            }
                            Err(error) => {
                                this.status = format!("自动保存失败：{error}");
                            }
                        }
                        cx.notify();
                    });
                })
            }));
        }

        fn window_activation_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
            if window.is_window_active() {
                return;
            }
            self.drain_keyboard(cx);
            self.schedule_draft_save(Duration::ZERO, cx);
        }

        fn save(&mut self, cx: &mut Context<Self>) {
            match self.commit_draft() {
                Ok(()) => self.status = "已保存到本机".into(),
                Err(error) => self.status = error,
            }
            cx.notify();
        }

        fn format_note_block(&mut self, style: MobileBlockStyle, cx: &mut Context<Self>) {
            if self.show_trash || self.selected_kind != Some(NodeKind::Note) || !self.body_editable
            {
                self.status = "当前内容不可格式化".into();
                cx.notify();
                return;
            }
            if self.focused_field != Some(FocusedField::Body) {
                self.focus_field(FocusedField::Body, KeyboardType::Default);
            }
            match apply_mobile_block_style(&mut self.draft_body, &mut self.focused_cursor, style) {
                Ok(true) => {
                    self.focused_selection_start = self.focused_cursor;
                    self.mark_draft_dirty(cx);
                    self.status = "已格式化当前块；保存后写回 Plate 富文本".into();
                    self.sync_native_text_input();
                }
                Ok(false) => {
                    self.status = "当前块已经是该格式".into();
                }
                Err(MobileBlockFormatError::ProtectedBlock) => {
                    self.status = "受保护的未知富文本块不能修改".into();
                }
                Err(MobileBlockFormatError::CodeBlock) => {
                    self.status = "代码围栏内部保持原样；请在普通文本块上应用格式".into();
                }
            }
            gpui_mobile::TEXT_INPUT_DIRTY.store(true, std::sync::atomic::Ordering::Release);
            cx.notify();
        }

        fn run_note_slash_command(&mut self, style: MobileBlockStyle, cx: &mut Context<Self>) {
            if self.focused_field != Some(FocusedField::Body)
                || self.show_trash
                || self.selected_kind != Some(NodeKind::Note)
                || !self.body_editable
            {
                self.status = "请先在笔记正文的新行输入 /".into();
                cx.notify();
                return;
            }
            match apply_mobile_slash_command(&mut self.draft_body, &mut self.focused_cursor, style)
            {
                Ok(true) => {
                    self.focused_selection_start = self.focused_cursor;
                    self.mark_draft_dirty(cx);
                    self.status = "已插入富文本块；保存后写回 Plate".into();
                    self.sync_native_text_input();
                }
                Ok(false) => {
                    self.status = "斜杠命令已经失效，请重新输入 /".into();
                }
                Err(MobileBlockFormatError::ProtectedBlock) => {
                    self.status = "受保护的未知富文本块不能修改".into();
                }
                Err(MobileBlockFormatError::CodeBlock) => {
                    self.status = "代码围栏内部不执行斜杠命令".into();
                }
            }
            gpui_mobile::TEXT_INPUT_DIRTY.store(true, std::sync::atomic::Ordering::Release);
            cx.notify();
        }

        fn move_note_block(&mut self, direction: MobileBlockMove, cx: &mut Context<Self>) {
            if self.show_trash || self.selected_kind != Some(NodeKind::Note) || !self.body_editable
            {
                self.status = "当前内容不可重排".into();
                cx.notify();
                return;
            }
            if self.focused_field != Some(FocusedField::Body) {
                self.focus_field(FocusedField::Body, KeyboardType::Default);
            }
            match move_mobile_block(&mut self.draft_body, &mut self.focused_cursor, direction) {
                Some(true) => {
                    self.focused_selection_start = self.focused_cursor;
                    self.mark_draft_dirty(cx);
                    self.status = "已移动当前块；保存后同步 Plate 块顺序".into();
                    self.sync_native_text_input();
                }
                Some(false) => {
                    self.status = match direction {
                        MobileBlockMove::Up => "当前块已经在最前面",
                        MobileBlockMove::Down => "当前块已经在最后面",
                    }
                    .into();
                }
                None => {
                    self.status = "正文含未闭合代码围栏，修复后才能重排块".into();
                }
            }
            gpui_mobile::TEXT_INPUT_DIRTY.store(true, std::sync::atomic::Ordering::Release);
            cx.notify();
        }

        fn open_bookmark_external(&mut self, cx: &mut Context<Self>) {
            if let Err(error) = self.commit_draft() {
                self.status = error;
                cx.notify();
                return;
            }
            let Some(id) = self.selected_id.clone() else {
                return;
            };
            match self.service.node(&id) {
                Ok(Node::Bookmark { content, .. }) => {
                    match gpui_mobile::packages::url_launcher::launch_url(&content.url) {
                        Ok(true) => self.status = "已交给系统浏览器打开".into(),
                        Ok(false) => self.status = "系统中没有可打开该网址的应用".into(),
                        Err(error) => self.status = error,
                    }
                }
                Ok(_) => self.status = "当前项目不是书签".into(),
                Err(error) => self.status = error.to_string(),
            }
            cx.notify();
        }

        fn open_bookmark_embedded(&mut self, cx: &mut Context<Self>) {
            if let Err(error) = self.commit_draft() {
                self.status = error;
                cx.notify();
                return;
            }
            let Some(id) = self.selected_id.clone() else {
                return;
            };
            let url = match self.service.node(&id) {
                Ok(Node::Bookmark { content, .. }) => content.url,
                Ok(_) => {
                    self.status = "当前项目不是书签".into();
                    cx.notify();
                    return;
                }
                Err(error) => {
                    self.status = error.to_string();
                    cx.notify();
                    return;
                }
            };
            self.stop_editing();
            match gpui_mobile::packages::webview::load_url(
                &url,
                &WebViewSettings {
                    javascript_enabled: true,
                    user_agent: Some("ideall/0.2".into()),
                    zoom_enabled: true,
                    dom_storage_enabled: true,
                    top_offset: 0.0,
                },
            ) {
                Ok(handle) => {
                    self.embedded_browser = Some(handle);
                    self.status = "已在应用内系统 WebView 打开".into();
                }
                Err(error) => self.status = error,
            }
            cx.notify();
        }

        fn open_portal(&mut self, url: &'static str, cx: &mut Context<Self>) {
            self.stop_editing();
            self.embedded_browser = None;
            match gpui_mobile::packages::webview::load_url(
                url,
                &WebViewSettings {
                    javascript_enabled: true,
                    user_agent: Some("ideall/0.2".into()),
                    zoom_enabled: true,
                    dom_storage_enabled: true,
                    top_offset: 0.0,
                },
            ) {
                Ok(handle) => {
                    self.embedded_browser = Some(handle);
                    self.status = "已在隔离系统 WebView 中打开；未注入本地文件或密钥能力".into();
                }
                Err(error) => self.status = error,
            }
            cx.notify();
        }

        fn refresh_sync_secret_status(&mut self) {
            match self.secret_store.get(SecretKey::SyncCode) {
                Ok(value) => self.sync_code_configured = value.is_some(),
                Err(error) => self.status = error.to_string(),
            }
            match self.secret_store.get(SecretKey::SyncBearerToken) {
                Ok(value) => self.sync_token_configured = value.is_some(),
                Err(error) => self.status = error.to_string(),
            }
        }

        fn persist_sync_settings(&mut self) -> Result<(), String> {
            let saved = self
                .service
                .save_sync_settings(&SyncSettings {
                    server_base_url: self.sync_server_draft.clone(),
                })
                .map_err(|error| error.to_string())?;
            self.sync_server_draft = saved.server_base_url;

            if !self.sync_code_draft.trim().is_empty() {
                if !is_valid_sync_code(&self.sync_code_draft) {
                    return Err("同步码必须恰好包含 32 位十六进制字符".into());
                }
                self.secret_store
                    .set(
                        SecretKey::SyncCode,
                        &normalize_sync_code(&self.sync_code_draft),
                    )
                    .map_err(|error| error.to_string())?;
                self.sync_code_draft.clear();
                self.sync_code_configured = true;
            }
            if !self.sync_token_draft.trim().is_empty() {
                if !self
                    .sync_token_draft
                    .trim()
                    .bytes()
                    .all(|byte| byte.is_ascii_graphic())
                {
                    return Err("Bearer Token 只能包含可打印 ASCII 字符".into());
                }
                self.secret_store
                    .set(SecretKey::SyncBearerToken, self.sync_token_draft.trim())
                    .map_err(|error| error.to_string())?;
                self.sync_token_draft.clear();
                self.sync_token_configured = true;
            }
            Ok(())
        }

        fn save_sync_settings(&mut self, cx: &mut Context<Self>) {
            self.stop_editing();
            self.status = match self.persist_sync_settings() {
                Ok(()) => "同步设置已安全保存".into(),
                Err(error) => error,
            };
            cx.notify();
        }

        fn clear_sync_credentials(&mut self, cx: &mut Context<Self>) {
            self.stop_editing();
            let result = self
                .secret_store
                .delete(SecretKey::SyncCode)
                .and_then(|_| self.secret_store.delete(SecretKey::SyncBearerToken));
            match result {
                Ok(_) => {
                    self.sync_code_draft.clear();
                    self.sync_token_draft.clear();
                    self.sync_code_configured = false;
                    self.sync_token_configured = false;
                    self.status = "同步凭据已从系统安全存储清除".into();
                }
                Err(error) => self.status = error.to_string(),
            }
            cx.notify();
        }

        fn start_notes_sync(&mut self, cx: &mut Context<Self>) {
            if self.sync_in_progress {
                return;
            }
            self.stop_editing();
            if let Err(error) = self.persist_sync_settings() {
                self.status = error;
                cx.notify();
                return;
            }
            let settings = match self.service.load_sync_settings() {
                Ok(settings) => settings,
                Err(error) => {
                    self.status = error.to_string();
                    cx.notify();
                    return;
                }
            };
            let code = match self.secret_store.get(SecretKey::SyncCode) {
                Ok(Some(value)) => value,
                Ok(None) => {
                    self.status = "请先保存同步码".into();
                    cx.notify();
                    return;
                }
                Err(error) => {
                    self.status = error.to_string();
                    cx.notify();
                    return;
                }
            };
            let token = match self.secret_store.get(SecretKey::SyncBearerToken) {
                Ok(Some(value)) => value,
                Ok(None) => {
                    self.status = "请先保存登录 Bearer Token".into();
                    cx.notify();
                    return;
                }
                Err(error) => {
                    self.status = error.to_string();
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
            self.status = "正在同步关注、笔记与书签三个加密域……".into();
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
                                this.refresh();
                                this.status = format!(
                                    "同步完成：关注 {}、笔记 {}、书签 {} 条；新增 {} 条",
                                    subscriptions.total,
                                    notes.total,
                                    bookmarks.total,
                                    subscriptions.added + notes.added + bookmarks.added
                                );
                            }
                            Err(error) => this.status = error,
                        }
                        cx.notify();
                    });
                })
            })
            .detach();
            cx.notify();
        }

        fn commit_draft(&mut self) -> Result<(), String> {
            if !self.dirty || self.show_trash {
                return Ok(());
            }
            self.autosave_task.take();
            let Some(id) = self.selected_id.clone() else {
                return Ok(());
            };
            let body = self.body_editable.then_some(self.draft_body.as_str());
            self.service
                .save_edits(&id, self.draft_title.clone(), body)
                .map_err(|error| error.to_string())?;
            self.dirty = false;
            self.refresh();
            Ok(())
        }

        fn move_to_trash(&mut self, cx: &mut Context<Self>) {
            if let Err(error) = self.commit_draft() {
                self.status = error;
                cx.notify();
                return;
            }
            let Some(id) = self.selected_id.clone() else {
                return;
            };
            match self.service.move_to_trash(&id) {
                Ok(true) => {
                    self.close_detail(cx);
                    self.status = "已移到回收站".into();
                }
                Ok(false) => self.status = "项目已经删除".into(),
                Err(error) => self.status = error.to_string(),
            }
            self.refresh();
            cx.notify();
        }

        fn restore(&mut self, cx: &mut Context<Self>) {
            let Some(id) = self.selected_id.clone() else {
                return;
            };
            match self.service.restore(&id) {
                Ok(true) => {
                    self.close_detail(cx);
                    self.status = "已从回收站恢复".into();
                }
                Ok(false) => self.status = "未找到回收站快照".into(),
                Err(error) => self.status = error.to_string(),
            }
            self.refresh();
            cx.notify();
        }

        fn purge(&mut self, cx: &mut Context<Self>) {
            if !self.purge_armed {
                self.purge_armed = true;
                self.status = "再次点击“永久删除”确认；此操作无法撤销".into();
                cx.notify();
                return;
            }
            let Some(id) = self.selected_id.clone() else {
                return;
            };
            match self.service.purge(&id) {
                Ok(true) => {
                    self.close_detail(cx);
                    self.status = "已永久删除".into();
                }
                Ok(false) => self.status = "项目已不在回收站".into(),
                Err(error) => self.status = error.to_string(),
            }
            self.refresh();
            cx.notify();
        }

        fn select_section(&mut self, section: &'static str, cx: &mut Context<Self>) {
            if let Err(error) = self.commit_draft() {
                self.status = error;
                cx.notify();
                return;
            }
            self.stop_editing();
            self.selected_id = None;
            self.embedded_browser = None;
            self.selected_kind = None;
            self.draft_title.clear();
            self.draft_body.clear();
            self.body_editable = false;
            self.dirty = false;
            self.purge_armed = false;
            self.workspace.tabs.clear();
            self.workspace.active_id = None;
            self.persist_workspace();
            self.show_trash = section == "回收站";
            self.search_query.clear();
            self.search_requests.invalidate();
            drop(self.search_task.take());
            self.active_section = section;
            self.refresh();
            if section == "设置" {
                if let Ok(settings) = self.service.load_sync_settings() {
                    self.sync_server_draft = settings.server_base_url;
                }
                self.refresh_sync_secret_status();
            }
            if section == "应用" {
                match self.secret_store.get(SecretKey::AgentCredential) {
                    Ok(value) => self.agent_key_configured = value.is_some(),
                    Err(error) => self.status = error.to_string(),
                }
                self.load_agent_activity(cx);
            }
            self.status = section_status(section).into();
            cx.notify();
        }

        fn close_detail(&mut self, cx: &mut Context<Self>) {
            if let Err(error) = self.commit_draft() {
                self.status = error;
                cx.notify();
                return;
            }
            self.stop_editing();
            self.embedded_browser = None;
            self.selected_id = None;
            self.selected_kind = None;
            self.draft_title.clear();
            self.draft_body.clear();
            self.body_editable = false;
            self.dirty = false;
            self.purge_armed = false;
            self.workspace.tabs.clear();
            self.workspace.active_id = None;
            self.persist_workspace();
            cx.notify();
        }

        fn persist_workspace(&mut self) {
            if let Err(error) = self.service.save_workspace_state(&self.workspace) {
                self.status = error.to_string();
            }
        }

        fn stop_editing(&mut self) {
            self.focused_field = None;
            self.focused_selection_start = 0;
            self.focused_cursor = 0;
            self.native_input_composing = false;
            self.body_last_revealed_cursor = None;
            self.observed_pending_chunks = 0;
            PENDING_TEXT.with(|pending| pending.borrow_mut().clear());
            native_text::clear_pending_text();
            native_text::hide();
            gpui_mobile::hide_keyboard();
            gpui_mobile::set_text_input_callback(None);
        }

        fn handle_control_key(&mut self, event: &KeyDownEvent) {
            if self.focused_field.is_none() {
                return;
            }
            let queued = PENDING_TEXT.with(|pending| {
                enqueue_mobile_control_key(
                    &mut pending.borrow_mut(),
                    &mut self.observed_pending_chunks,
                    &event.keystroke.key,
                )
            });
            if queued {
                gpui_mobile::TEXT_INPUT_DIRTY.store(true, std::sync::atomic::Ordering::Release);
            }
        }
    }

    #[cfg(target_os = "ios")]
    fn select_mobile_documents() -> Result<Vec<SelectedMobileDocument>, String> {
        use std::ffi::CStr;

        // SAFETY: the Objective-C bridge returns either null or a malloc-owned,
        // NUL-terminated JSON string and requires the paired free function.
        let pointer =
            crate::ios_host::pick_files().ok_or_else(|| "iOS 文件选择桥接尚未初始化".to_owned())?;
        if pointer.is_null() {
            return Ok(Vec::new());
        }
        // SAFETY: pointer validity is guaranteed by ideall_pick_files until it
        // is passed to ideall_free_string below.
        let raw = unsafe { CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned();
        // SAFETY: this is exactly the pointer returned above and is freed once.
        unsafe { crate::ios_host::free_string(pointer) };
        let values: Vec<serde_json::Value> =
            serde_json::from_str(&raw).map_err(|_| "系统文件选择结果无效".to_owned())?;
        values
            .into_iter()
            .map(|value| {
                let path = value
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| "系统文件地址无效".to_owned())?;
                let name = value
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| "系统文件名无效".to_owned())?;
                Ok(SelectedMobileDocument {
                    path: path.to_owned(),
                    name: name.to_owned(),
                })
            })
            .collect()
    }

    #[cfg(not(target_os = "ios"))]
    fn select_mobile_documents() -> Result<Vec<SelectedMobileDocument>, String> {
        let options = gpui_mobile::packages::file_selector::OpenFileOptions::default();
        gpui_mobile::packages::file_selector::open_files(&options).map(|files| {
            files
                .into_iter()
                .map(|file| SelectedMobileDocument {
                    path: file.path,
                    name: file.name,
                })
                .collect()
        })
    }

    struct SelectedMobileDocument {
        path: String,
        name: String,
    }

    #[cfg(target_os = "ios")]
    fn materialize_mobile_document(
        database_path: &std::path::Path,
        source: &str,
    ) -> Result<String, String> {
        use std::ffi::CString;

        let directory = database_path
            .parent()
            .ok_or_else(|| "移动数据目录不可用".to_owned())?;
        let destination = directory.join("selected-document.cache");
        let source = CString::new(source).map_err(|_| "文件地址包含无效字符".to_owned())?;
        let destination_text = destination
            .to_str()
            .ok_or_else(|| "导入缓存路径不是 UTF-8".to_owned())?;
        let destination_c =
            CString::new(destination_text).map_err(|_| "导入缓存路径包含无效字符".to_owned())?;
        let status = crate::ios_host::copy_security_scoped_file(
            source.as_ptr(),
            destination_c.as_ptr(),
            256 * 1024 * 1024,
        )
        .ok_or_else(|| "iOS 文件访问桥接尚未初始化".to_owned())?;
        if status != 0 {
            return Err(match status {
                1 => "文件地址无效",
                2 => "无法获得所选文件的读取权限",
                3 => "文件超过 256 MiB 导入上限",
                _ => "复制所选文件失败",
            }
            .into());
        }
        Ok(destination_text.to_owned())
    }

    #[cfg(not(target_os = "ios"))]
    fn materialize_mobile_document(
        _database_path: &std::path::Path,
        source: &str,
    ) -> Result<String, String> {
        Ok(source.to_owned())
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

    fn mobile_engine_id(kind: NodeKind) -> &'static str {
        match kind {
            NodeKind::Folder => "ideall.directory",
            NodeKind::Note => "ideall.note",
            NodeKind::Bookmark => "ideall.bookmark",
            NodeKind::File => "ideall.preview",
            NodeKind::Feed => "ideall.feed",
            NodeKind::Thread => "ideall.thread",
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

    pub fn open_main_window(cx: &mut App) {
        let initialized = (|| {
            ideall_secrets::initialize_platform()
                .map_err(|error| format!("无法初始化系统安全存储：{error}"))?;
            let support = gpui_mobile::packages::path_provider::support_directory()
                .map_err(|error| format!("无法确定应用数据目录：{error}"))?;
            std::fs::create_dir_all(&support)
                .map_err(|error| format!("无法创建应用数据目录：{error}"))?;
            let database_path = support.join("ideall.db");
            let service = LocalWorkspace::open(&database_path)
                .map_err(|error| format!("无法打开本地数据库：{error}"))?;
            Ok::<_, String>((database_path, service))
        })();

        let window_result = match initialized {
            Ok((database_path, service)) => cx
                .open_window(WindowOptions::default(), |window, cx| {
                    let view = cx.new(|cx| IdeallMobile::new(database_path, service, window, cx));
                    let focus_handle = view.read(cx).focus_handle.clone();
                    window.focus(&focus_handle, cx);
                    view
                })
                .map(|_| ()),
            Err(message) => cx
                .open_window(WindowOptions::default(), |_window, cx| {
                    cx.new(|_| MobileStartupFailure { message })
                })
                .map(|_| ()),
        };
        if let Err(error) = window_result {
            log::error!("failed to open ideall mobile window: {error}");
        }
        cx.activate(true);
    }
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: android_activity::AndroidApp) {
    use gpui_mobile::gpui::Application;

    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("ideall"),
    );
    if let Err(error) = register_android_native_bridges(&app) {
        log::error!("failed to register Android native bridges: {error}");
    }
    gpui_mobile::android::jni::install_panic_hook();
    let platform = gpui_mobile::android::jni::init_platform(&app);
    let shared_platform =
        gpui_mobile::android::platform::SharedPlatform::new(platform.clone()).into_rc();
    Application::with_platform(shared_platform).run(mobile::open_main_window);
    gpui_mobile::android::jni::clear_platform(&platform);
}

#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub extern "C" fn gpui_ios_register_app() {
    gpui_mobile::ios::ffi::set_app_callback(Box::new(mobile::open_main_window));
}

/// Exposes the real mobile composition root to host-only compile checks.
#[cfg(all(
    not(any(target_os = "ios", target_os = "android")),
    feature = "mobile-ui-host-check"
))]
pub fn mobile_ui_host_check_entry() -> fn(&mut gpui_mobile::gpui::App) {
    mobile::open_main_window
}

/// Lets host checks prove that the crate stays platform-gated without pretending to run mobile UI.
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub fn mobile_targets_only() -> &'static str {
    "ideall-mobile requires an iOS or Android target"
}

#[cfg(test)]
mod tests {
    use super::editor::{
        BlockFormatError as MobileBlockFormatError, BlockMove as MobileBlockMove,
        BlockStyle as MobileBlockStyle, apply_block_style as apply_mobile_block_style,
        apply_input as apply_mobile_input, apply_input_at_cursor as apply_mobile_input_at_cursor,
        apply_slash_command as apply_mobile_slash_command,
        control_sequence as mobile_control_sequence, cursor_line_index as mobile_cursor_line_index,
        enqueue_control_key as enqueue_mobile_control_key,
        line_with_cursor as mobile_line_with_cursor, move_block as move_mobile_block,
        native_edit_committed as mobile_native_edit_committed,
        slash_commands as mobile_slash_commands, slash_query as mobile_slash_query,
        text_line_ranges as mobile_text_line_ranges,
    };

    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    #[test]
    fn host_build_explains_platform_requirement() {
        assert!(super::mobile_targets_only().contains("iOS or Android"));
    }

    #[test]
    fn mobile_input_accepts_committed_cjk_and_normalizes_multiline_text() {
        let mut text = "开".to_owned();
        assert!(apply_mobile_input(&mut text, "始记录", true));
        assert!(apply_mobile_input(&mut text, "\r\n第二行\r第三行", true));
        assert_eq!(text, "开始记录\n第二行\n第三行");
    }

    #[test]
    fn mobile_backspace_removes_a_complete_emoji_grapheme() {
        let mut text = "A👨‍👩‍👧‍👦".to_owned();
        assert!(apply_mobile_input(&mut text, "\u{8}", false));
        assert_eq!(text, "A");
        assert!(apply_mobile_input(&mut text, "\u{8}", false));
        assert!(text.is_empty());
        assert!(!apply_mobile_input(&mut text, "\u{8}", false));
    }

    #[test]
    fn mobile_single_line_input_filters_newlines_without_dropping_unicode() {
        let mut text = String::new();
        assert!(apply_mobile_input(&mut text, "模型\r\n名称🙂", false));
        assert_eq!(text, "模型名称🙂");
    }

    #[test]
    fn mobile_multiline_layout_keeps_empty_and_trailing_lines() {
        let empty = mobile_text_line_ranges("");
        assert_eq!(empty.len(), 1);
        assert_eq!(empty[0], 0..0);
        let single = mobile_text_line_ranges("第一行");
        assert_eq!(single.len(), 1);
        assert_eq!(single[0], 0.."第一行".len());
        assert_eq!(
            mobile_text_line_ranges("甲\n\n乙\n"),
            [0.."甲".len(), 4..4, 5..8, 9..9]
        );
    }

    #[test]
    fn mobile_multiline_cursor_uses_visual_line_boundaries() {
        let text = "甲\n🙂\n";
        assert_eq!(mobile_cursor_line_index(text, 0), 0);
        assert_eq!(mobile_cursor_line_index(text, "甲".len()), 0);
        assert_eq!(mobile_cursor_line_index(text, "甲\n".len()), 1);
        assert_eq!(mobile_cursor_line_index(text, "甲\n🙂\n".len()), 2);
    }

    #[test]
    fn mobile_cursor_marker_never_splits_utf8() {
        assert_eq!(mobile_line_with_cursor("甲🙂乙", 3), "甲▏🙂乙");
        assert_eq!(mobile_line_with_cursor("甲🙂乙", 5), "甲▏🙂乙");
        assert_eq!(mobile_line_with_cursor("甲🙂乙", usize::MAX), "甲🙂乙▏");
    }

    #[test]
    fn mobile_native_ime_only_persists_committed_composition() {
        assert!(!mobile_native_edit_committed(true, false, true));
        assert!(!mobile_native_edit_committed(false, true, true));
        assert!(mobile_native_edit_committed(false, true, false));
        assert!(mobile_native_edit_committed(true, false, false));
        assert!(!mobile_native_edit_committed(false, false, false));
    }

    #[test]
    fn mobile_cursor_navigation_inserts_and_deletes_whole_graphemes() {
        let mut text = "A👨‍👩‍👧‍👦B".to_owned();
        let mut cursor = text.len();
        assert!(!apply_mobile_input_at_cursor(
            &mut text,
            &mut cursor,
            "\u{1b}[D",
            false,
        ));
        assert!(apply_mobile_input_at_cursor(
            &mut text,
            &mut cursor,
            "\u{8}",
            false,
        ));
        assert_eq!(text, "AB");
        assert_eq!(cursor, 1);
        assert!(apply_mobile_input_at_cursor(
            &mut text,
            &mut cursor,
            "中",
            false,
        ));
        assert_eq!(text, "A中B");
    }

    #[test]
    fn mobile_forward_delete_and_home_end_are_bounded() {
        let mut text = "甲🙂乙".to_owned();
        let mut cursor = text.len();
        assert!(!apply_mobile_input_at_cursor(
            &mut text,
            &mut cursor,
            "\u{1b}[H",
            false,
        ));
        assert!(apply_mobile_input_at_cursor(
            &mut text,
            &mut cursor,
            "\u{1b}[3~",
            false,
        ));
        assert_eq!(text, "🙂乙");
        assert!(!apply_mobile_input_at_cursor(
            &mut text,
            &mut cursor,
            "\u{1b}[F",
            false,
        ));
        assert!(!apply_mobile_input_at_cursor(
            &mut text,
            &mut cursor,
            "\u{7f}",
            false,
        ));
    }

    #[test]
    fn mobile_hardware_control_keys_fill_platform_callback_gaps_without_duplicates() {
        assert_eq!(mobile_control_sequence("backspace"), Some("\u{8}"));
        assert_eq!(mobile_control_sequence("delete"), Some("\u{1b}[3~"));
        assert_eq!(mobile_control_sequence("enter"), Some("\n"));
        assert_eq!(mobile_control_sequence("a"), None);

        let mut pending = vec!["\u{8}".to_owned()];
        let mut observed = 0;
        assert!(!enqueue_mobile_control_key(
            &mut pending,
            &mut observed,
            "backspace"
        ));
        assert_eq!(pending, ["\u{8}"]);

        assert!(enqueue_mobile_control_key(
            &mut pending,
            &mut observed,
            "backspace"
        ));
        assert_eq!(pending, ["\u{8}", "\u{8}"]);

        pending.clear();
        observed = 0;
        assert!(enqueue_mobile_control_key(
            &mut pending,
            &mut observed,
            "delete"
        ));
        assert_eq!(pending, ["\u{1b}[3~"]);
    }

    #[test]
    fn mobile_block_toolbar_formats_the_line_at_the_cursor() {
        let mut text = "第一段\n  - 现有项目\n末段".to_owned();
        let mut cursor = text.find("现有").unwrap() + "现".len();
        assert_eq!(
            apply_mobile_block_style(&mut text, &mut cursor, MobileBlockStyle::Todo),
            Ok(true)
        );
        assert_eq!(text, "第一段\n  - [ ] 现有项目\n末段");
        assert_eq!(&text[cursor..], "有项目\n末段");

        assert_eq!(
            apply_mobile_block_style(&mut text, &mut cursor, MobileBlockStyle::Heading2),
            Ok(true)
        );
        assert_eq!(text, "第一段\n## 现有项目\n末段");
        assert_eq!(&text[cursor..], "有项目\n末段");

        assert_eq!(
            apply_mobile_block_style(&mut text, &mut cursor, MobileBlockStyle::Paragraph),
            Ok(true)
        );
        assert_eq!(text, "第一段\n现有项目\n末段");
        assert_eq!(&text[cursor..], "有项目\n末段");

        assert_eq!(
            apply_mobile_block_style(&mut text, &mut cursor, MobileBlockStyle::Bullet),
            Ok(true)
        );
        assert_eq!(text, "第一段\n- 现有项目\n末段");
        assert_eq!(
            apply_mobile_block_style(&mut text, &mut cursor, MobileBlockStyle::Ordered),
            Ok(true)
        );
        assert_eq!(text, "第一段\n1. 现有项目\n末段");
    }

    #[test]
    fn mobile_block_toolbar_wraps_code_and_inserts_rules() {
        let mut text = "alpha\nbeta".to_owned();
        let mut cursor = 2;
        assert_eq!(
            apply_mobile_block_style(&mut text, &mut cursor, MobileBlockStyle::Code),
            Ok(true)
        );
        assert_eq!(text, "```\nalpha\n```\nbeta");
        assert_eq!(&text[cursor..], "pha\n```\nbeta");

        cursor = text.len();
        assert_eq!(
            apply_mobile_block_style(&mut text, &mut cursor, MobileBlockStyle::Rule),
            Ok(true)
        );
        assert_eq!(text, "```\nalpha\n```\n---");
        assert_eq!(cursor, text.len());
    }

    #[test]
    fn mobile_block_toolbar_refuses_protected_and_code_fence_lines() {
        let mut protected = "before\n⟦ideall:受保护块:1:img:abcd⟧\nafter".to_owned();
        let mut cursor = protected.find("img").unwrap();
        assert_eq!(
            apply_mobile_block_style(&mut protected, &mut cursor, MobileBlockStyle::Heading1),
            Err(MobileBlockFormatError::ProtectedBlock)
        );
        assert_eq!(protected, "before\n⟦ideall:受保护块:1:img:abcd⟧\nafter");

        let mut code = "```\nlet value = 1;\n```".to_owned();
        cursor = code.find("value").unwrap();
        assert_eq!(
            apply_mobile_block_style(&mut code, &mut cursor, MobileBlockStyle::Quote),
            Err(MobileBlockFormatError::CodeBlock)
        );
        assert_eq!(code, "```\nlet value = 1;\n```");
    }

    #[test]
    fn mobile_slash_menu_filters_and_applies_commands_at_the_cursor() {
        let commands = mobile_slash_commands("h");
        assert!(commands.iter().any(|(id, _, _)| *id == "heading-1"));
        assert!(commands.iter().any(|(id, _, _)| *id == "heading-2"));
        assert!(!commands.iter().any(|(id, _, _)| *id == "todo"));

        let mut text = "前一段\n/h2\n后一段".to_owned();
        let mut cursor = text.find("/h2").unwrap() + 3;
        assert_eq!(mobile_slash_query(&text, cursor), Some("h2"));
        assert_eq!(
            apply_mobile_slash_command(&mut text, &mut cursor, MobileBlockStyle::Heading2),
            Ok(true)
        );
        assert_eq!(text, "前一段\n## \n后一段");
        assert_eq!(&text[cursor..], "\n后一段");

        let mut code = "/code".to_owned();
        cursor = code.len();
        assert_eq!(
            apply_mobile_slash_command(&mut code, &mut cursor, MobileBlockStyle::Code),
            Ok(true)
        );
        assert_eq!(code, "```\n\n```");
        assert_eq!(cursor, "```\n".len());
        assert_eq!(mobile_slash_query("not /command", 12), None);
    }

    #[test]
    fn mobile_block_reordering_moves_multiline_and_protected_blocks_atomically() {
        let mut text =
            "第一段\n> 引用一\n> 引用二\n```\nline one\nline two\n```\n⟦ideall:受保护块:4:img:abcd⟧"
                .to_owned();
        let mut cursor = text.find("line two").unwrap() + "line ".len();
        assert_eq!(
            move_mobile_block(&mut text, &mut cursor, MobileBlockMove::Up),
            Some(true)
        );
        assert_eq!(
            text,
            "第一段\n```\nline one\nline two\n```\n> 引用一\n> 引用二\n⟦ideall:受保护块:4:img:abcd⟧"
        );
        assert_eq!(
            &text[cursor..],
            "two\n```\n> 引用一\n> 引用二\n⟦ideall:受保护块:4:img:abcd⟧"
        );

        cursor = text.find("受保护").unwrap();
        assert_eq!(
            move_mobile_block(&mut text, &mut cursor, MobileBlockMove::Up),
            Some(true)
        );
        assert_eq!(
            text,
            "第一段\n```\nline one\nline two\n```\n⟦ideall:受保护块:4:img:abcd⟧\n> 引用一\n> 引用二"
        );
        assert_eq!(
            move_mobile_block(&mut text, &mut cursor, MobileBlockMove::Down),
            Some(true)
        );
        assert!(text.ends_with("⟦ideall:受保护块:4:img:abcd⟧"));

        let mut unclosed = "before\n```\ncode".to_owned();
        cursor = unclosed.len();
        assert_eq!(
            move_mobile_block(&mut unclosed, &mut cursor, MobileBlockMove::Up),
            None
        );
    }
}
