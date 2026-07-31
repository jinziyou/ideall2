//! Orchestration shared by the desktop and mobile shells.
//!
//! The GPUI apps keep their entity/input state and task plumbing; the
//! credential validation, settings persistence, and sync/agent orchestration
//! rules that used to be copied into both shells live here so they cannot
//! drift apart.

use std::path::Path;

use ideall_agent::{AgentAuditRecord, OpenAiCompatibleClient};
use ideall_protocol::{Node, NodeKind};
use ideall_sync::{SyncRunResult, SyncTransport, is_valid_sync_code, normalize_sync_code};

use crate::workspace::{
    AgentDocumentContext, AgentModelSettings, AgentTurnResult, ApplicationError, LocalWorkspace,
    NodeSummary, SyncSettings,
};

/// Largest single file the shells accept through the import flow.
pub const MAX_IMPORT_BYTES: u64 = 256 * 1024 * 1024;

/// Status line shown while a debounced local search is running.
pub const SEARCH_IN_PROGRESS_STATUS: &str = "正在搜索本地内容…";

/// Status line shown after sync settings are saved. Credentials only reach
/// the platform secure store, never SQLite.
pub const SYNC_SETTINGS_SAVED_STATUS: &str = "同步设置已保存；凭据未写入 SQLite";
/// Status line shown after sync credentials are removed from the secure store.
pub const SYNC_CREDENTIALS_CLEARED_STATUS: &str = "同步凭据已从系统安全存储清除";
/// Status line shown while the three encrypted sync domains are running.
pub const SYNC_IN_PROGRESS_STATUS: &str = "正在同步关注、笔记与书签三个加密域……";
/// Status line shown after agent model settings are saved.
pub const AGENT_MODEL_SETTINGS_SAVED_STATUS: &str = "模型设置已保存；API Key 仅在系统安全存储中";
/// Status line shown while an agent turn is waiting on the model.
pub const AGENT_TURN_IN_PROGRESS_STATUS: &str = "Agent 正在本机直连模型服务……";
/// Status line shown when an agent turn is started without a message.
pub const AGENT_PROMPT_EMPTY_STATUS: &str = "请输入 Agent 消息";
/// Status line shown after starting a fresh agent thread.
pub const NEW_AGENT_THREAD_STATUS: &str = "已开始新对话；发送第一条消息后写入本地 SQLite";
/// Status line shown after opening an existing agent thread.
pub const AGENT_THREAD_OPENED_STATUS: &str = "已打开本地 Agent 对话";

const INVALID_SYNC_CODE_STATUS: &str = "同步码必须恰好包含 32 位十六进制字符";
const INVALID_BEARER_TOKEN_STATUS: &str = "Bearer Token 只能包含可打印 ASCII 字符";

/// Credentials the shells keep in the platform secure store. The values
/// never enter SQLite; use cases reach them only through [`CredentialStore`].
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CredentialKey {
    SyncCode,
    SyncBearerToken,
    AgentCredential,
}

/// Minimal secure-store surface the workspace use cases need. Each app backs
/// this with its platform keyring so this crate never links the secrets
/// adapter.
pub trait CredentialStore {
    fn get(&self, key: CredentialKey) -> Result<Option<String>, String>;
    fn set(&self, key: CredentialKey, value: &str) -> Result<(), String>;
    fn delete(&self, key: CredentialKey) -> Result<(), String>;
}

/// Whether `key` currently holds a value in the secure store.
pub fn credential_configured(
    credentials: &dyn CredentialStore,
    key: CredentialKey,
) -> Result<bool, String> {
    Ok(credentials.get(key)?.is_some())
}

/// Reads a credential that must exist before an operation can start.
pub fn require_credential(
    credentials: &dyn CredentialStore,
    key: CredentialKey,
) -> Result<String, String> {
    credentials.get(key)?.ok_or_else(|| {
        match key {
            CredentialKey::SyncCode => "请先保存同步码",
            CredentialKey::SyncBearerToken => "请先保存登录 Bearer Token",
            CredentialKey::AgentCredential => "请先安全保存模型 API Key",
        }
        .to_owned()
    })
}

/// Which sync credentials currently exist in the secure store.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SyncSecretStatus {
    pub sync_code_configured: bool,
    pub sync_token_configured: bool,
}

pub fn sync_secret_status(credentials: &dyn CredentialStore) -> Result<SyncSecretStatus, String> {
    Ok(SyncSecretStatus {
        sync_code_configured: credential_configured(credentials, CredentialKey::SyncCode)?,
        sync_token_configured: credential_configured(credentials, CredentialKey::SyncBearerToken)?,
    })
}

/// Removes both sync credentials from the secure store.
pub fn clear_sync_credentials(credentials: &dyn CredentialStore) -> Result<(), String> {
    credentials
        .delete(CredentialKey::SyncCode)
        .and_then(|_| credentials.delete(CredentialKey::SyncBearerToken))
}

/// Status line reporting how many local items a search matched.
pub fn search_results_status(count: usize) -> String {
    format!("找到 {count} 条本地内容")
}

/// Status line reporting a failed local search.
pub fn search_failed_status(error: &str) -> String {
    format!("搜索失败：{error}")
}

/// Raw sync settings editor values. Empty credential fields keep the stored
/// credential unchanged.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SyncSettingsDraft {
    pub server_base_url: String,
    pub sync_code: String,
    pub bearer_token: String,
}

/// Result of persisting a [`SyncSettingsDraft`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistedSyncSettings {
    pub settings: SyncSettings,
    pub sync_code_saved: bool,
    pub bearer_token_saved: bool,
}

/// Raw agent model settings editor values. An empty `api_key` keeps the
/// stored credential unchanged.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AgentModelSettingsDraft {
    pub base_url: String,
    pub model: String,
    pub api_key: String,
}

/// Result of persisting an [`AgentModelSettingsDraft`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistedAgentModelSettings {
    pub settings: AgentModelSettings,
    pub api_key_saved: bool,
}

/// Combined result of syncing the three encrypted domains in the fixed
/// order both apps always used: subscriptions, notes, then bookmarks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotesSyncSummary {
    pub subscriptions: SyncRunResult,
    pub notes: SyncRunResult,
    pub bookmarks: SyncRunResult,
}

impl NotesSyncSummary {
    pub fn status_line(&self) -> String {
        format!(
            "同步完成：关注 {}、笔记 {}、书签 {} 条；新增 {} 条",
            self.subscriptions.total,
            self.notes.total,
            self.bookmarks.total,
            self.subscriptions.added + self.notes.added + self.bookmarks.added
        )
    }
}

/// Agent audits and recent threads loaded together for the agent panel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentActivity {
    pub audits: Vec<AgentAuditRecord>,
    pub threads: Vec<NodeSummary>,
}

impl AgentTurnResult {
    /// Status line summarizing a completed turn and its audited tool runs.
    pub fn status_line(&self) -> String {
        if self.tools.is_empty() {
            return "Agent 已回复；对话保存在本地 SQLite".into();
        }
        let failures = self.tools.iter().filter(|tool| !tool.ok).count();
        format!(
            "Agent 已回复；执行 {} 个工具，{} 个失败；写操作均已审计",
            self.tools.len(),
            failures
        )
    }
}

impl LocalWorkspace {
    /// Runs the subscriptions, notes, and bookmarks encrypted sync passes in
    /// the fixed order both apps always used.
    pub fn sync_all_domains<T: SyncTransport>(
        &mut self,
        sync_code: &str,
        transport: &mut T,
    ) -> Result<NotesSyncSummary, ApplicationError> {
        let subscriptions = self.sync_subscriptions(sync_code, transport)?;
        let notes = self.sync_notes(sync_code, transport)?;
        let bookmarks = self.sync_bookmarks(sync_code, transport)?;
        Ok(NotesSyncSummary {
            subscriptions,
            notes,
            bookmarks,
        })
    }

    /// Loads the agent panel data: the latest audits and the newest threads.
    pub fn agent_activity(&self) -> Result<AgentActivity, ApplicationError> {
        Ok(AgentActivity {
            audits: self.list_agent_audits(50)?,
            threads: self.list_agent_threads(12)?,
        })
    }

    /// The most recently updated agent thread, if any exists.
    pub fn latest_agent_thread_id(&self) -> Result<Option<String>, ApplicationError> {
        Ok(self
            .list_agent_threads(1)?
            .into_iter()
            .next()
            .map(|thread| thread.id))
    }

    /// Runs one durable agent turn through an OpenAI-compatible provider.
    pub fn run_openai_agent_turn(
        &mut self,
        settings: &AgentModelSettings,
        api_key: &str,
        thread_id: Option<&str>,
        prompt: &str,
        document_context: Option<&AgentDocumentContext>,
    ) -> Result<AgentTurnResult, ApplicationError> {
        let mut provider =
            OpenAiCompatibleClient::new(&settings.base_url, &settings.model, api_key)?;
        self.run_agent_turn_with_context(&mut provider, thread_id, prompt, document_context)
    }

    /// Editor body projection shared by both shells: the text to show,
    /// whether it is editable, and how many fingerprint-protected rich-text
    /// blocks it carries.
    pub fn node_body(&self, node: &Node) -> (String, bool, usize) {
        match node {
            Node::Note { base, .. } => self
                .plain_note(&base.id)
                .map(|document| (document.text, document.editable, document.protected_blocks))
                .unwrap_or_else(|error| (error.to_string(), false, 0)),
            Node::Bookmark { content, .. } => (content.url.clone(), true, 0),
            Node::File { base, blob_ref, .. } => self
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

    /// Creates a bookmark in the selected folder, falling back to the inbox
    /// when nothing is selected so bookmarks never hang off the home root.
    pub fn create_bookmark_with_inbox_fallback(
        &mut self,
        parent_id: Option<&str>,
        title: impl Into<String>,
        url: &str,
    ) -> Result<Node, ApplicationError> {
        let parent_id = match parent_id {
            Some(parent_id) => parent_id.to_owned(),
            None => self.ensure_inbox()?,
        };
        self.create_bookmark(Some(&parent_id), title, url)
    }

    /// Reopens the workspace at `path` and runs one debounced search.
    /// Background search tasks use this so the UI thread never touches the
    /// database.
    pub fn search_at(
        path: impl AsRef<Path>,
        query: &str,
        limit: usize,
        kind: Option<NodeKind>,
    ) -> Result<Vec<NodeSummary>, ApplicationError> {
        Self::open(path)?.search_with_kind(query, limit, kind)
    }

    /// Reopens the workspace at `path` and saves one draft. Background
    /// autosave tasks use this so the UI thread never touches the database.
    pub fn save_draft_at(
        path: impl AsRef<Path>,
        id: &str,
        title: String,
        body: Option<&str>,
    ) -> Result<NodeSummary, ApplicationError> {
        let node = Self::open(path)?.save_edits(id, title, body)?;
        Ok(NodeSummary::from(&node))
    }

    /// Persists the public sync settings and stores any newly entered
    /// credentials in the platform secure store.
    pub fn persist_sync_settings(
        &mut self,
        draft: &SyncSettingsDraft,
        credentials: &dyn CredentialStore,
    ) -> Result<PersistedSyncSettings, String> {
        let settings = self
            .save_sync_settings(&SyncSettings {
                server_base_url: draft.server_base_url.clone(),
            })
            .map_err(|error| error.to_string())?;

        let mut persisted = PersistedSyncSettings {
            settings,
            sync_code_saved: false,
            bearer_token_saved: false,
        };
        if !draft.sync_code.trim().is_empty() {
            if !is_valid_sync_code(&draft.sync_code) {
                return Err(INVALID_SYNC_CODE_STATUS.into());
            }
            credentials.set(
                CredentialKey::SyncCode,
                &normalize_sync_code(&draft.sync_code),
            )?;
            persisted.sync_code_saved = true;
        }
        let token = draft.bearer_token.trim();
        if !token.is_empty() {
            if !token.bytes().all(|byte| byte.is_ascii_graphic()) {
                return Err(INVALID_BEARER_TOKEN_STATUS.into());
            }
            credentials.set(CredentialKey::SyncBearerToken, token)?;
            persisted.bearer_token_saved = true;
        }
        Ok(persisted)
    }

    /// Persists the public provider coordinates and stores a newly entered
    /// API key after validating it against the provider requirements.
    pub fn persist_agent_model_settings(
        &mut self,
        draft: &AgentModelSettingsDraft,
        credentials: &dyn CredentialStore,
    ) -> Result<PersistedAgentModelSettings, String> {
        let settings = self
            .save_agent_model_settings(&AgentModelSettings {
                base_url: draft.base_url.clone(),
                model: draft.model.clone(),
            })
            .map_err(|error| error.to_string())?;

        let mut persisted = PersistedAgentModelSettings {
            settings,
            api_key_saved: false,
        };
        let api_key = draft.api_key.trim();
        if !api_key.is_empty() {
            OpenAiCompatibleClient::new(
                &persisted.settings.base_url,
                &persisted.settings.model,
                api_key,
            )
            .map_err(|error| error.to_string())?;
            credentials.set(CredentialKey::AgentCredential, api_key)?;
            persisted.api_key_saved = true;
        }
        Ok(persisted)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::*;

    #[derive(Default)]
    struct MemoryCredentials {
        values: RefCell<HashMap<CredentialKey, String>>,
    }

    impl MemoryCredentials {
        fn stored(&self, key: CredentialKey) -> Option<String> {
            self.values.borrow().get(&key).cloned()
        }
    }

    impl CredentialStore for MemoryCredentials {
        fn get(&self, key: CredentialKey) -> Result<Option<String>, String> {
            Ok(self.stored(key))
        }

        fn set(&self, key: CredentialKey, value: &str) -> Result<(), String> {
            self.values.borrow_mut().insert(key, value.to_owned());
            Ok(())
        }

        fn delete(&self, key: CredentialKey) -> Result<(), String> {
            self.values.borrow_mut().remove(&key);
            Ok(())
        }
    }

    fn sync_draft() -> SyncSettingsDraft {
        SyncSettingsDraft {
            server_base_url: "https://sync.example.com/".into(),
            sync_code: String::new(),
            bearer_token: String::new(),
        }
    }

    #[test]
    fn persist_sync_settings_stores_canonical_settings_and_new_credentials() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let credentials = MemoryCredentials::default();
        let draft = SyncSettingsDraft {
            sync_code: "  ABCDEF0123456789ABCDEF0123456789 ".into(),
            bearer_token: "  token-123  ".into(),
            ..sync_draft()
        };
        let persisted = workspace
            .persist_sync_settings(&draft, &credentials)
            .unwrap();
        assert_eq!(
            persisted.settings.server_base_url,
            "https://sync.example.com"
        );
        assert!(persisted.sync_code_saved);
        assert!(persisted.bearer_token_saved);
        assert_eq!(
            credentials.stored(CredentialKey::SyncCode).as_deref(),
            Some("abcdef0123456789abcdef0123456789")
        );
        assert_eq!(
            credentials
                .stored(CredentialKey::SyncBearerToken)
                .as_deref(),
            Some("token-123")
        );
    }

    #[test]
    fn persist_sync_settings_keeps_credentials_when_drafts_are_empty() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let credentials = MemoryCredentials::default();
        let persisted = workspace
            .persist_sync_settings(&sync_draft(), &credentials)
            .unwrap();
        assert!(!persisted.sync_code_saved);
        assert!(!persisted.bearer_token_saved);
        assert_eq!(credentials.stored(CredentialKey::SyncCode), None);
        assert_eq!(credentials.stored(CredentialKey::SyncBearerToken), None);
    }

    #[test]
    fn persist_sync_settings_rejects_an_invalid_sync_code_before_storing() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let credentials = MemoryCredentials::default();
        let draft = SyncSettingsDraft {
            sync_code: "not-hex".into(),
            ..sync_draft()
        };
        let error = workspace
            .persist_sync_settings(&draft, &credentials)
            .unwrap_err();
        assert_eq!(error, "同步码必须恰好包含 32 位十六进制字符");
        assert_eq!(credentials.stored(CredentialKey::SyncCode), None);
    }

    #[test]
    fn persist_sync_settings_rejects_a_non_ascii_bearer_token() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let credentials = MemoryCredentials::default();
        let draft = SyncSettingsDraft {
            bearer_token: "令牌 with spaces".into(),
            ..sync_draft()
        };
        let error = workspace
            .persist_sync_settings(&draft, &credentials)
            .unwrap_err();
        assert_eq!(error, "Bearer Token 只能包含可打印 ASCII 字符");
        assert_eq!(credentials.stored(CredentialKey::SyncBearerToken), None);
        // The settings save precedes credential validation, matching the
        // ordering both apps always had.
        assert_eq!(
            workspace.load_sync_settings().unwrap().server_base_url,
            "https://sync.example.com"
        );
    }

    #[test]
    fn persist_agent_model_settings_stores_coordinates_and_validated_key() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let credentials = MemoryCredentials::default();
        let draft = AgentModelSettingsDraft {
            base_url: " https://example.com/compatible/v1 ".into(),
            model: " custom-model ".into(),
            api_key: " sk-test ".into(),
        };
        let persisted = workspace
            .persist_agent_model_settings(&draft, &credentials)
            .unwrap();
        assert_eq!(
            persisted.settings.base_url,
            "https://example.com/compatible/v1/"
        );
        assert_eq!(persisted.settings.model, "custom-model");
        assert!(persisted.api_key_saved);
        assert_eq!(
            credentials
                .stored(CredentialKey::AgentCredential)
                .as_deref(),
            Some("sk-test")
        );
    }

    #[test]
    fn persist_agent_model_settings_rejects_an_invalid_key_without_storing() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let credentials = MemoryCredentials::default();
        let draft = AgentModelSettingsDraft {
            api_key: "not a key".into(),
            ..AgentModelSettingsDraft::default()
        };
        assert!(
            workspace
                .persist_agent_model_settings(&draft, &credentials)
                .is_err()
        );
        assert_eq!(credentials.stored(CredentialKey::AgentCredential), None);
    }

    #[test]
    fn persist_agent_model_settings_rejects_an_invalid_base_url() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let credentials = MemoryCredentials::default();
        let draft = AgentModelSettingsDraft {
            base_url: "https://secret@example.com/v1".into(),
            model: "m".into(),
            api_key: "sk-test".into(),
        };
        assert!(
            workspace
                .persist_agent_model_settings(&draft, &credentials)
                .is_err()
        );
        assert_eq!(credentials.stored(CredentialKey::AgentCredential), None);
    }

    #[test]
    fn require_credential_reads_values_or_reports_the_missing_one() {
        let credentials = MemoryCredentials::default();
        credentials.set(CredentialKey::SyncCode, "code").unwrap();
        assert_eq!(
            require_credential(&credentials, CredentialKey::SyncCode).unwrap(),
            "code"
        );
        assert_eq!(
            require_credential(&credentials, CredentialKey::SyncBearerToken).unwrap_err(),
            "请先保存登录 Bearer Token"
        );
        assert_eq!(
            require_credential(&credentials, CredentialKey::AgentCredential).unwrap_err(),
            "请先安全保存模型 API Key"
        );
    }

    #[test]
    fn sync_secret_status_and_clear_track_the_secure_store() {
        let credentials = MemoryCredentials::default();
        assert_eq!(
            sync_secret_status(&credentials).unwrap(),
            SyncSecretStatus::default()
        );
        credentials.set(CredentialKey::SyncCode, "code").unwrap();
        credentials
            .set(CredentialKey::SyncBearerToken, "token")
            .unwrap();
        assert_eq!(
            sync_secret_status(&credentials).unwrap(),
            SyncSecretStatus {
                sync_code_configured: true,
                sync_token_configured: true,
            }
        );
        clear_sync_credentials(&credentials).unwrap();
        assert_eq!(
            sync_secret_status(&credentials).unwrap(),
            SyncSecretStatus::default()
        );
    }

    #[derive(Default)]
    struct RecordingTransport {
        committed: Vec<String>,
    }

    impl SyncTransport for RecordingTransport {
        fn get_manifest(
            &mut self,
            _storage_id: &str,
        ) -> Result<Option<ideall_sync::SyncManifest>, ideall_sync::TransportError> {
            Ok(None)
        }

        fn get_part(
            &mut self,
            _storage_id: &str,
            _generation: &str,
            _part_index: usize,
        ) -> Result<ideall_sync::SyncGenerationPart, ideall_sync::TransportError> {
            Err(ideall_sync::TransportError::new(Some(404), "not committed"))
        }

        fn put_part(
            &mut self,
            _storage_id: &str,
            _generation: &str,
            _part: &ideall_sync::SyncGenerationPart,
        ) -> Result<(), ideall_sync::TransportError> {
            Ok(())
        }

        fn commit_manifest(
            &mut self,
            storage_id: &str,
            generation: &str,
            part_count: usize,
            expected_version: u64,
        ) -> Result<ideall_sync::SyncManifest, ideall_sync::TransportError> {
            self.committed.push(storage_id.to_owned());
            Ok(ideall_sync::SyncManifest {
                generation: generation.into(),
                part_count,
                total_ciphertext_chars: 0,
                parts_sha256: "0".repeat(64),
                version: expected_version + 1,
                updated_at_ms: 1,
            })
        }

        fn discard_generation(
            &mut self,
            _storage_id: &str,
            _generation: &str,
        ) -> Result<(), ideall_sync::TransportError> {
            Ok(())
        }
    }

    #[test]
    fn sync_all_domains_runs_the_three_domains_in_order() {
        let code = "00112233445566778899aabbccddeeff";
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        workspace.create_note(None, "跨端笔记").unwrap();
        workspace
            .create_feed(
                "example.com",
                ideall_protocol::SubscriptionType::Publisher,
                "example.com",
            )
            .unwrap();
        let folder = workspace.create_folder(None, "目录").unwrap();
        workspace
            .create_bookmark(Some(&folder.base().id), "书签", "https://example.com/")
            .unwrap();
        let mut transport = RecordingTransport::default();
        let summary = workspace.sync_all_domains(code, &mut transport).unwrap();
        assert_eq!(summary.subscriptions.total, 1);
        assert_eq!(summary.notes.total, 1);
        // The folder is normalized into the bookmark domain so nested
        // bookmarks keep their structure on the wire.
        assert_eq!(summary.bookmarks.total, 2);
        assert_eq!(transport.committed.len(), 3);
        assert!(
            summary
                .status_line()
                .starts_with("同步完成：关注 1、笔记 1、书签 2 条；新增 ")
        );
    }

    #[test]
    fn notes_sync_status_line_formats_totals_and_additions() {
        let result = |total, added| SyncRunResult {
            total,
            added,
            attempts: 1,
        };
        let summary = NotesSyncSummary {
            subscriptions: result(4, 1),
            notes: result(10, 2),
            bookmarks: result(6, 3),
        };
        assert_eq!(
            summary.status_line(),
            "同步完成：关注 4、笔记 10、书签 6 条；新增 6 条"
        );
    }

    #[test]
    fn agent_activity_and_latest_thread_track_durable_threads() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        assert_eq!(workspace.latest_agent_thread_id().unwrap(), None);
        let thread_id = workspace.begin_external_agent_turn(None, "你好").unwrap();
        let activity = workspace.agent_activity().unwrap();
        assert!(activity.audits.is_empty());
        assert_eq!(activity.threads.len(), 1);
        assert_eq!(activity.threads[0].id, thread_id);
        assert_eq!(
            workspace.latest_agent_thread_id().unwrap().as_deref(),
            Some(thread_id.as_str())
        );
    }

    #[test]
    fn run_openai_agent_turn_rejects_invalid_settings_before_any_network() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let settings = AgentModelSettings {
            base_url: "https://secret@example.com/v1".into(),
            model: "m".into(),
        };
        assert!(matches!(
            workspace.run_openai_agent_turn(&settings, "sk-test", None, "你好", None),
            Err(ApplicationError::AgentConfig(_))
        ));
    }

    #[test]
    fn agent_turn_status_line_summarizes_tool_runs() {
        let result = |tools: Vec<crate::workspace::AgentToolRun>| AgentTurnResult {
            thread_id: "thread".into(),
            content: "答复".into(),
            tools,
        };
        assert_eq!(
            result(Vec::new()).status_line(),
            "Agent 已回复；对话保存在本地 SQLite"
        );
        let tool = |name: &str, ok: bool| crate::workspace::AgentToolRun {
            name: name.into(),
            ok,
            summary: String::new(),
        };
        assert_eq!(
            result(vec![tool("a", true), tool("b", false)]).status_line(),
            "Agent 已回复；执行 2 个工具，1 个失败；写操作均已审计"
        );
    }

    #[test]
    fn node_body_projects_each_node_kind() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let note = workspace.create_note(None, "笔记").unwrap();
        let (text, editable, protected) = workspace.node_body(&note);
        assert!(text.is_empty() && editable && protected == 0);

        let bookmark = workspace
            .create_bookmark(None, "书签", "https://example.com/")
            .unwrap();
        assert_eq!(
            workspace.node_body(&bookmark),
            ("https://example.com/".to_owned(), true, 0)
        );

        let folder = workspace.create_folder(None, "目录").unwrap();
        assert_eq!(
            workspace.node_body(&folder),
            ("文件夹中的项目将在这里显示。".to_owned(), false, 0)
        );

        let feed = workspace
            .create_feed(
                "example.com",
                ideall_protocol::SubscriptionType::Publisher,
                "example.com",
            )
            .unwrap();
        assert_eq!(
            workspace.node_body(&feed),
            ("关注源：example.com".to_owned(), false, 0)
        );

        let thread_id = workspace.begin_external_agent_turn(None, "你好").unwrap();
        let thread = workspace.node(&thread_id).unwrap();
        assert_eq!(
            workspace.node_body(&thread),
            ("1 条本地会话消息".to_owned(), false, 0)
        );

        let binary = workspace
            .create_file(None, "data.bin", "application/octet-stream", vec![0, 159])
            .unwrap();
        let (text, editable, _) = workspace.node_body(&binary);
        assert!(!editable);
        assert!(text.starts_with("文件类型：application/octet-stream\n大小：2 字节\nBlob："));
    }

    #[test]
    fn create_bookmark_with_inbox_fallback_uses_the_selected_folder_or_inbox() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let folder = workspace.create_folder(None, "目录").unwrap();
        let in_folder = workspace
            .create_bookmark_with_inbox_fallback(
                Some(&folder.base().id),
                "书签",
                "https://example.com/",
            )
            .unwrap();
        assert_eq!(
            in_folder.base().parent_id.as_deref(),
            Some(folder.base().id.as_str())
        );

        let fallback = workspace
            .create_bookmark_with_inbox_fallback(None, "书签", "https://example.org/")
            .unwrap();
        assert_eq!(
            fallback.base().parent_id.as_deref(),
            Some(crate::workspace::INBOX_ROOT_ID)
        );
    }

    #[test]
    fn search_at_and_save_draft_at_reopen_the_workspace_from_disk() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ideall.db");
        let note_id = {
            let mut workspace = LocalWorkspace::open(&path).unwrap();
            workspace
                .create_note(None, "会议纪要")
                .unwrap()
                .base()
                .id
                .clone()
        };

        let hits = LocalWorkspace::search_at(&path, "会议", 10, None).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(
            LocalWorkspace::search_at(&path, "会议", 10, Some(NodeKind::Folder))
                .unwrap()
                .is_empty()
        );

        let summary =
            LocalWorkspace::save_draft_at(&path, &note_id, "会议纪要 v2".to_owned(), Some("正文"))
                .unwrap();
        assert_eq!(summary.title, "会议纪要 v2");
        let workspace = LocalWorkspace::open(&path).unwrap();
        assert_eq!(
            workspace.node(&note_id).unwrap().base().title,
            "会议纪要 v2"
        );
    }
}
