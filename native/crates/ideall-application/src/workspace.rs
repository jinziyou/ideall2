use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use ideall_agent::{
    AgentAuditCompletion, AgentAuditInput, AgentAuditRecord, AuditError, AuditSource, AuditStatus,
    AuditTarget, CompletionProvider, Intent, LocalMcpServer, McpTool, ModelMessage, ModelRole,
    ModelTool, OpenAiConfigError, OpenAiError, ToolEffect, ToolHandler, ToolRisk, agent_grant,
    canonical_base_url, complete_audit, new_audit, validate_model_name,
};
use ideall_domain::{EnginePreferences, WorkspaceState};
use ideall_protocol::{
    BaseNode, BlobRef, BookmarkContent, FeedContent, FileKind, FileRef, FileSource, FileSourceKind,
    IdeallFile, Node, NodeKind, SubscriptionType, SyncNote, SyncSubscription, ThreadContent,
};
use ideall_storage::{ArchiveBlob, Database, NodeProjection, StorageError};
use ideall_sync::{
    LocalStoreError, SyncBlockBudget, SyncLocalStore, SyncRunError, SyncRunResult, SyncScope,
    SyncTransport, TOMBSTONE_TTL_MS, gc_sync_notes, is_sane_sync_timestamp, merge_sync_notes,
    prune_expired_tombstones, run_sync, run_sync_with,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use url::Url;
use uuid::Uuid;

use crate::note_document::{NoteDocumentError, apply_note_text, project_note};

pub const HOME_ROOT_ID: &str = "ideall-home";
const ENGINE_PREFERENCES_KEY: &str = "engine-preferences:files";
const SYNC_SETTINGS_KEY: &str = "sync-settings:v1";
const AGENT_MODEL_SETTINGS_KEY: &str = "agent-model-settings:v1";
const EXTERNAL_ACP_SETTINGS_KEY: &str = "external-acp-settings:v1";
const DEVICE_ID_KEY: &str = "native-device-id:v1";
const DEFAULT_SYNC_SERVER: &str = "https://api.wonita.link";
const MAX_AGENT_PROMPT_BYTES: usize = 64 * 1024;
const MAX_AGENT_CONTEXT_MESSAGES: usize = 192;
const MAX_AGENT_TOOL_RESULT_BYTES: usize = 256 * 1024;
const MAX_AGENT_TOOL_ROUNDS: usize = 8;
const AGENT_SYSTEM_PROMPT: &str = "你是 ideall 的本地优先个人信息助手。只使用当前提供的工具；工具列表只含用户已授予的能力。不要声称读取了未提供的正文或文件内容。执行工具后，用简洁中文说明实际结果。";

#[derive(Debug, Error)]
pub enum ApplicationError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("node `{0}` was not found")]
    NotFound(String),
    #[error("node `{id}` is {actual:?}, expected {expected:?}")]
    WrongKind {
        id: String,
        actual: NodeKind,
        expected: NodeKind,
    },
    #[error("note document cannot be saved: {0}")]
    InvalidNoteDocument(String),
    #[error("bookmark URL must use http or https")]
    InvalidBookmarkUrl,
    #[error("subscription key must contain 1 to 2048 printable characters")]
    InvalidSubscriptionKey,
    #[error("sync server must be an http(s) base URL without credentials, query, or fragment")]
    InvalidSyncServer,
    #[error("folder `{0}` must be empty before it can be moved to trash")]
    FolderNotEmpty(String),
    #[error("engine `{engine_id}` does not support `{media_type}`")]
    EngineNotSupported {
        engine_id: String,
        media_type: String,
    },
    #[error("invalid file reference: {0}")]
    InvalidFileRef(String),
    #[error("file is larger than the {limit} byte import limit ({actual} bytes)")]
    FileTooLarge { actual: u64, limit: u64 },
    #[error("node title must contain 1 to 1024 bytes")]
    InvalidNodeTitle,
    #[error("node cannot be moved into itself or one of its descendants")]
    InvalidNodeMove,
    #[error("local file operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("binary file `{0}` is read-only in the text editor")]
    BinaryFileReadOnly(String),
    #[error("text file `{0}` is not valid UTF-8")]
    InvalidTextEncoding(String),
    #[error("system clock is before the Unix epoch")]
    InvalidClock,
    #[error(transparent)]
    Sync(#[from] SyncRunError),
    #[error(transparent)]
    Audit(#[from] AuditError),
    #[error(transparent)]
    AgentConfig(#[from] OpenAiConfigError),
    #[error(transparent)]
    AgentModel(#[from] OpenAiError),
    #[error("Agent audit intent `{0}` was not found")]
    AgentAuditNotFound(String),
    #[error("local Agent tool failed: {0}")]
    AgentTool(String),
    #[error("Agent prompt must contain 1 to 65536 bytes of text")]
    InvalidAgentPrompt,
    #[error("external ACP settings contain an invalid program, argv, or working directory")]
    InvalidExternalAcpSettings,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeSummary {
    pub id: String,
    pub parent_id: Option<String>,
    pub kind: NodeKind,
    pub title: String,
    pub updated_at: i64,
    pub deleted: bool,
    pub depth: usize,
}

impl From<&Node> for NodeSummary {
    fn from(node: &Node) -> Self {
        Self {
            id: node.base().id.clone(),
            parent_id: node.base().parent_id.clone(),
            kind: node.kind(),
            title: node.base().title.clone(),
            updated_at: node.base().updated_at,
            deleted: node.base().deleted_at.is_some(),
            depth: 0,
        }
    }
}

impl From<&NodeProjection> for NodeSummary {
    fn from(node: &NodeProjection) -> Self {
        Self {
            id: node.id.clone(),
            parent_id: node.parent_id.clone(),
            kind: node.kind,
            title: node.title.clone(),
            updated_at: node.updated_at,
            deleted: node.deleted,
            depth: 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlainNoteDocument {
    pub text: String,
    pub editable: bool,
    pub protected_blocks: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextFileDocument {
    pub text: String,
    pub media_type: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SyncSettings {
    pub server_base_url: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelSettings {
    pub base_url: String,
    pub model: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalAcpSettings {
    pub program: String,
    pub args: String,
    pub cwd: String,
}

impl Default for AgentModelSettings {
    fn default() -> Self {
        Self {
            base_url: "https://api.deepseek.com/v1/".into(),
            model: "deepseek-chat".into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentToolRun {
    pub name: String,
    pub ok: bool,
    pub summary: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentTurnResult {
    pub thread_id: String,
    pub content: String,
    pub tools: Vec<AgentToolRun>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentTranscriptMessage {
    pub role: ModelRole,
    pub content: String,
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self {
            server_base_url: DEFAULT_SYNC_SERVER.into(),
        }
    }
}

pub struct LocalWorkspace {
    database: Database,
    device_id: String,
}

impl LocalWorkspace {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ApplicationError> {
        let mut database = Database::open(path)?;
        let device_id = ensure_device_id(&mut database)?;
        let mut workspace = Self {
            database,
            device_id,
        };
        workspace.ensure_home_root()?;
        Ok(workspace)
    }

    pub fn open_in_memory() -> Result<Self, ApplicationError> {
        let mut database = Database::open_in_memory()?;
        let device_id = ensure_device_id(&mut database)?;
        let mut workspace = Self {
            database,
            device_id,
        };
        workspace.ensure_home_root()?;
        Ok(workspace)
    }

    pub fn list_home(&self) -> Result<Vec<NodeSummary>, ApplicationError> {
        let nodes = self.database.list_node_projections(false)?;
        Ok(flatten_tree(&nodes))
    }

    pub fn list_children(&self, parent_id: &str) -> Result<Vec<NodeSummary>, ApplicationError> {
        Ok(self
            .database
            .list_children(Some(parent_id), false)?
            .iter()
            .map(NodeSummary::from)
            .collect())
    }

    pub fn list_trash(&self) -> Result<Vec<NodeSummary>, ApplicationError> {
        Ok(self
            .database
            .list_node_projections(true)?
            .iter()
            .filter(|node| node.deleted)
            .map(NodeSummary::from)
            .collect())
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<NodeSummary>, ApplicationError> {
        Ok(self
            .database
            .search_nodes(query, limit)?
            .iter()
            .map(NodeSummary::from)
            .collect())
    }

    pub fn node(&self, id: &str) -> Result<Node, ApplicationError> {
        self.database
            .get_node(id)?
            .ok_or_else(|| ApplicationError::NotFound(id.into()))
    }

    pub fn file_metadata(&self, id: &str) -> Result<IdeallFile, ApplicationError> {
        let node = self.node(id)?;
        let base = node.base();
        let (kind, media_type, size) = match &node {
            Node::Folder { .. } => (FileKind::Directory, "inode/directory".into(), None),
            Node::Note { .. } => (
                FileKind::File,
                "application/vnd.ideall.note+json".into(),
                None,
            ),
            Node::Bookmark { .. } => (
                FileKind::File,
                "application/vnd.ideall.bookmark+json".into(),
                None,
            ),
            Node::File { blob_ref, .. } => {
                (FileKind::File, blob_ref.mime.clone(), Some(blob_ref.size))
            }
            Node::Feed { .. } => (
                FileKind::File,
                "application/vnd.ideall.feed+json".into(),
                None,
            ),
            Node::Thread { .. } => (
                FileKind::File,
                "application/vnd.ideall.thread+json".into(),
                None,
            ),
        };
        Ok(IdeallFile {
            r#ref: FileRef::new("local.nodes", id),
            kind,
            name: base.title.clone(),
            media_type,
            capabilities: if base.deleted_at.is_some() {
                vec!["read".into(), "restore".into()]
            } else {
                vec!["read".into(), "write".into(), "delete".into()]
            },
            source: FileSource {
                kind: FileSourceKind::Local,
                id: "local.nodes".into(),
                label: Some("ideall SQLite".into()),
                read_only: Some(false),
            },
            size,
            created_at: Some(base.created_at),
            updated_at: Some(base.updated_at),
            version: Some(base.updated_at.to_string()),
            properties: BTreeMap::from([(
                "nodeKind".into(),
                Value::String(node_kind_name(node.kind()).into()),
            )]),
        })
    }

    pub fn create_folder(
        &mut self,
        parent_id: Option<&str>,
        title: impl Into<String>,
    ) -> Result<Node, ApplicationError> {
        let node = Node::Folder {
            base: new_base(parent_id, title.into())?,
            content: None,
        };
        self.database.put_node(&node)?;
        Ok(node)
    }

    pub fn create_note(
        &mut self,
        parent_id: Option<&str>,
        title: impl Into<String>,
    ) -> Result<Node, ApplicationError> {
        let mut base = new_base(parent_id, title.into())?;
        let block_id = format!("blk_{}", Uuid::new_v4().simple());
        let content = vec![json!({
            "id": block_id,
            "type": "p",
            "children": [{"text": ""}]
        })];
        base.meta.insert(
            "blockMeta".into(),
            Value::Object(serde_json::Map::from_iter([(
                block_id,
                json!({
                    "v": 1,
                    "by": self.device_id,
                    "sk": native_block_sort_key(0)
                }),
            )])),
        );
        let node = Node::Note { base, content };
        self.database.put_node(&node)?;
        Ok(node)
    }

    pub fn create_bookmark(
        &mut self,
        parent_id: Option<&str>,
        title: impl Into<String>,
        url: &str,
    ) -> Result<Node, ApplicationError> {
        let parsed = validated_bookmark_url(url)?;
        let node = Node::Bookmark {
            base: new_base(parent_id, title.into())?,
            content: BookmarkContent {
                url: parsed.into(),
                description: String::new(),
                favicon: String::new(),
            },
        };
        self.database.put_node(&node)?;
        Ok(node)
    }

    pub fn create_feed(
        &mut self,
        title: impl Into<String>,
        r#type: SubscriptionType,
        key: &str,
    ) -> Result<Node, ApplicationError> {
        let key = validated_subscription_key(key)?;
        if r#type == SubscriptionType::Tool {
            validated_bookmark_url(&key)?;
        }
        let id = format!("{}:{key}", subscription_type_name(r#type));
        if let Some(existing) = self.database.get_node(&id)? {
            if existing.kind() != NodeKind::Feed {
                return Err(ApplicationError::WrongKind {
                    id,
                    actual: existing.kind(),
                    expected: NodeKind::Feed,
                });
            }
            if existing.base().deleted_at.is_some() {
                self.database.restore_from_trash(&id, now_millis()?)?;
            }
            return self.node(&id);
        }
        let timestamp = now_millis()?;
        let title = title.into();
        let node = Node::Feed {
            base: BaseNode {
                id: id.clone(),
                parent_id: Some(HOME_ROOT_ID.into()),
                sort_key: format!("{timestamp:020}-{id}"),
                title: if title.trim().is_empty() {
                    key.clone()
                } else {
                    title
                },
                tags: Vec::new(),
                created_at: timestamp,
                updated_at: timestamp,
                deleted_at: None,
                meta: BTreeMap::new(),
            },
            content: FeedContent {
                r#type,
                key,
                favicon: String::new(),
                entity_label: None,
                entity_name: None,
                search_keyword: None,
                search_domain: None,
            },
        };
        self.database.put_node(&node)?;
        Ok(node)
    }

    pub fn create_file(
        &mut self,
        parent_id: Option<&str>,
        title: impl Into<String>,
        mime: impl Into<String>,
        data: Vec<u8>,
    ) -> Result<Node, ApplicationError> {
        let mime = mime.into();
        let blob_key = Uuid::new_v4().to_string();
        let node = Node::File {
            base: new_base(parent_id, title.into())?,
            blob_ref: BlobRef {
                store: "blobs".into(),
                key: blob_key.clone(),
                size: data.len() as u64,
                mime: mime.clone(),
            },
            content: None,
        };
        self.database.put_file(
            &node,
            &ArchiveBlob {
                key: blob_key,
                mime,
                data,
            },
        )?;
        Ok(node)
    }

    pub fn import_file(
        &mut self,
        parent_id: Option<&str>,
        path: impl AsRef<Path>,
        max_bytes: u64,
    ) -> Result<Node, ApplicationError> {
        let path = path.as_ref();
        let metadata = std::fs::metadata(path)?;
        if metadata.len() > max_bytes {
            return Err(ApplicationError::FileTooLarge {
                actual: metadata.len(),
                limit: max_bytes,
            });
        }
        let title = path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("导入文件")
            .to_owned();
        let data = std::fs::read(path)?;
        self.create_file(parent_id, title, media_type_for_path(path), data)
    }

    /// Imports a user-selected mobile document. iOS selectors return file
    /// URLs, while Android's app helper materializes content URIs into a
    /// private cache path before this boundary is called.
    pub fn import_external_file(
        &mut self,
        parent_id: Option<&str>,
        path_or_file_url: &str,
        display_name: &str,
        max_bytes: u64,
    ) -> Result<Node, ApplicationError> {
        let path = if path_or_file_url.starts_with("file:") {
            Url::parse(path_or_file_url)
                .ok()
                .and_then(|url| url.to_file_path().ok())
                .ok_or_else(|| ApplicationError::InvalidFileRef("invalid file URL".into()))?
        } else {
            Path::new(path_or_file_url).to_owned()
        };
        let metadata = std::fs::metadata(&path)?;
        if metadata.len() > max_bytes {
            return Err(ApplicationError::FileTooLarge {
                actual: metadata.len(),
                limit: max_bytes,
            });
        }
        let title = Path::new(display_name)
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .or_else(|| path.file_name().and_then(|name| name.to_str()))
            .unwrap_or("导入文件")
            .to_owned();
        let data = std::fs::read(&path)?;
        let media_type = media_type_for_path(Path::new(&title));
        self.create_file(parent_id, title, media_type, data)
    }

    pub fn update_bookmark_url(&mut self, id: &str, url: &str) -> Result<Node, ApplicationError> {
        let parsed = validated_bookmark_url(url)?;
        let mut node = self.node(id)?;
        let actual = node.kind();
        let Node::Bookmark { content, .. } = &mut node else {
            return Err(ApplicationError::WrongKind {
                id: id.into(),
                actual,
                expected: NodeKind::Bookmark,
            });
        };
        content.url = parsed.into();
        node.base_mut().updated_at = now_millis()?;
        self.database.put_node(&node)?;
        Ok(node)
    }

    pub fn rename(&mut self, id: &str, title: impl Into<String>) -> Result<Node, ApplicationError> {
        let mut node = self.node(id)?;
        let title = validated_node_title(title.into())?;
        node.base_mut().title = title;
        node.base_mut().updated_at = now_millis()?;
        self.database.put_node(&node)?;
        Ok(node)
    }

    pub fn move_node(
        &mut self,
        id: &str,
        parent_id: Option<&str>,
    ) -> Result<Node, ApplicationError> {
        if id == HOME_ROOT_ID || parent_id == Some(id) {
            return Err(ApplicationError::InvalidNodeMove);
        }
        if let Some(parent_id) = parent_id {
            let parent = self.node(parent_id)?;
            if parent.kind() != NodeKind::Folder {
                return Err(ApplicationError::WrongKind {
                    id: parent_id.into(),
                    actual: parent.kind(),
                    expected: NodeKind::Folder,
                });
            }
            let mut ancestor = parent.base().parent_id.clone();
            while let Some(ancestor_id) = ancestor {
                if ancestor_id == id {
                    return Err(ApplicationError::InvalidNodeMove);
                }
                let node = self.node(&ancestor_id)?;
                ancestor = node.base().parent_id.clone();
            }
        }
        let mut node = self.node(id)?;
        node.base_mut().parent_id = Some(parent_id.unwrap_or(HOME_ROOT_ID).to_owned());
        node.base_mut().updated_at = now_millis()?;
        self.database.put_node(&node)?;
        Ok(node)
    }

    pub fn save_edits(
        &mut self,
        id: &str,
        title: impl Into<String>,
        body: Option<&str>,
    ) -> Result<Node, ApplicationError> {
        let mut node = self.node(id)?;
        let title = validated_node_title(title.into())?;
        let mut replacement_blob = None;
        let timestamp = now_millis()?;
        match &mut node {
            Node::Note { base, content } => {
                if let Some(body) = body {
                    apply_note_edit(base, content, body, &self.device_id, timestamp)?;
                }
            }
            Node::Bookmark { content, .. } => {
                if let Some(body) = body {
                    content.url = validated_bookmark_url(body)?.into();
                }
            }
            Node::File { blob_ref, .. } => {
                if let Some(body) = body {
                    if !is_text_media_type(&blob_ref.mime) {
                        return Err(ApplicationError::BinaryFileReadOnly(id.into()));
                    }
                    let data = body.as_bytes().to_vec();
                    blob_ref.size = data.len() as u64;
                    replacement_blob = Some(ArchiveBlob {
                        key: blob_ref.key.clone(),
                        mime: blob_ref.mime.clone(),
                        data,
                    });
                }
            }
            Node::Folder { .. } | Node::Feed { .. } | Node::Thread { .. } => {}
        }
        node.base_mut().title = title;
        node.base_mut().updated_at = timestamp;
        if let Some(blob) = replacement_blob {
            self.database.put_file(&node, &blob)?;
        } else {
            self.database.put_node(&node)?;
        }
        Ok(node)
    }

    pub fn plain_note(&self, id: &str) -> Result<PlainNoteDocument, ApplicationError> {
        let node = self.node(id)?;
        let Node::Note { content, .. } = node else {
            return Err(ApplicationError::WrongKind {
                id: id.into(),
                actual: node.kind(),
                expected: NodeKind::Note,
            });
        };
        Ok(read_plain_note(id, &content))
    }

    pub fn update_plain_note(&mut self, id: &str, text: &str) -> Result<Node, ApplicationError> {
        let mut node = self.node(id)?;
        let actual = node.kind();
        let Node::Note { base, content } = &mut node else {
            return Err(ApplicationError::WrongKind {
                id: id.into(),
                actual,
                expected: NodeKind::Note,
            });
        };
        let timestamp = now_millis()?;
        apply_note_edit(base, content, text, &self.device_id, timestamp)?;
        node.base_mut().updated_at = timestamp;
        self.database.put_node(&node)?;
        Ok(node)
    }

    pub fn text_file(&self, id: &str) -> Result<TextFileDocument, ApplicationError> {
        let node = self.node(id)?;
        let actual = node.kind();
        let Node::File { base, blob_ref, .. } = node else {
            return Err(ApplicationError::WrongKind {
                id: id.into(),
                actual,
                expected: NodeKind::File,
            });
        };
        if !is_text_media_type(&blob_ref.mime) {
            return Err(ApplicationError::BinaryFileReadOnly(id.into()));
        }
        let (_, data) =
            self.database
                .get_blob(&blob_ref.key)?
                .ok_or_else(|| StorageError::MissingBlob {
                    node_id: base.id.clone(),
                    blob_key: blob_ref.key.clone(),
                })?;
        let text = String::from_utf8(data)
            .map_err(|_| ApplicationError::InvalidTextEncoding(id.into()))?;
        Ok(TextFileDocument {
            text,
            media_type: blob_ref.mime,
        })
    }

    pub fn update_text_file(&mut self, id: &str, text: &str) -> Result<Node, ApplicationError> {
        let mut node = self.node(id)?;
        let actual = node.kind();
        let Node::File { blob_ref, .. } = &mut node else {
            return Err(ApplicationError::WrongKind {
                id: id.into(),
                actual,
                expected: NodeKind::File,
            });
        };
        if !is_text_media_type(&blob_ref.mime) {
            return Err(ApplicationError::BinaryFileReadOnly(id.into()));
        }
        let data = text.as_bytes().to_vec();
        blob_ref.size = data.len() as u64;
        let blob = ArchiveBlob {
            key: blob_ref.key.clone(),
            mime: blob_ref.mime.clone(),
            data,
        };
        node.base_mut().updated_at = now_millis()?;
        self.database.put_file(&node, &blob)?;
        Ok(node)
    }

    /// Materializes an internal Blob at a caller-selected platform path. The
    /// SQLite Blob remains authoritative; this copy is suitable for a system
    /// viewer or an explicit export destination.
    pub fn export_file(&self, id: &str, target: impl AsRef<Path>) -> Result<u64, ApplicationError> {
        let node = self.node(id)?;
        let actual = node.kind();
        let Node::File { base, blob_ref, .. } = node else {
            return Err(ApplicationError::WrongKind {
                id: id.into(),
                actual,
                expected: NodeKind::File,
            });
        };
        let (_, data) =
            self.database
                .get_blob(&blob_ref.key)?
                .ok_or_else(|| StorageError::MissingBlob {
                    node_id: base.id,
                    blob_key: blob_ref.key,
                })?;
        let target = target.as_ref();
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(target, &data)?;
        Ok(data.len() as u64)
    }

    pub fn move_to_trash(&mut self, id: &str) -> Result<bool, ApplicationError> {
        let node = self.node(id)?;
        if node.kind() == NodeKind::Folder
            && !self.database.list_children(Some(id), false)?.is_empty()
        {
            return Err(ApplicationError::FolderNotEmpty(id.into()));
        }
        Ok(self.database.move_to_trash(id, now_millis()?)?)
    }

    pub fn restore(&mut self, id: &str) -> Result<bool, ApplicationError> {
        Ok(self.database.restore_from_trash(id, now_millis()?)?)
    }

    pub fn purge(&mut self, id: &str) -> Result<bool, ApplicationError> {
        Ok(self.database.remove_node_permanently(id)?)
    }

    pub fn load_workspace_state(&self) -> Result<WorkspaceState, ApplicationError> {
        let Some(value) = self.database.workspace_state()? else {
            return Ok(WorkspaceState::default());
        };
        Ok(serde_json::from_value(value).unwrap_or_default())
    }

    pub fn save_workspace_state(
        &mut self,
        workspace: &WorkspaceState,
    ) -> Result<(), ApplicationError> {
        self.database.set_workspace_state(workspace)?;
        Ok(())
    }

    pub fn load_engine_preferences(&self) -> Result<EnginePreferences, ApplicationError> {
        Ok(self
            .database
            .setting(ENGINE_PREFERENCES_KEY)?
            .unwrap_or_default())
    }

    pub fn save_engine_preferences(
        &mut self,
        preferences: &EnginePreferences,
    ) -> Result<(), ApplicationError> {
        self.database
            .set_setting(ENGINE_PREFERENCES_KEY, preferences)?;
        Ok(())
    }

    pub fn load_sync_settings(&self) -> Result<SyncSettings, ApplicationError> {
        Ok(self
            .database
            .setting(SYNC_SETTINGS_KEY)?
            .unwrap_or_default())
    }

    pub fn save_sync_settings(
        &mut self,
        settings: &SyncSettings,
    ) -> Result<SyncSettings, ApplicationError> {
        let canonical = canonical_sync_server(&settings.server_base_url)?;
        let settings = SyncSettings {
            server_base_url: canonical,
        };
        self.database.set_setting(SYNC_SETTINGS_KEY, &settings)?;
        Ok(settings)
    }

    pub fn load_agent_model_settings(&self) -> Result<AgentModelSettings, ApplicationError> {
        Ok(self
            .database
            .setting(AGENT_MODEL_SETTINGS_KEY)?
            .unwrap_or_default())
    }

    /// Persists only public provider coordinates. The API key belongs in the
    /// platform secure store and never enters SQLite or thread JSON.
    pub fn save_agent_model_settings(
        &mut self,
        settings: &AgentModelSettings,
    ) -> Result<AgentModelSettings, ApplicationError> {
        let base_url = canonical_base_url(&settings.base_url)?.to_string();
        let model = validate_model_name(&settings.model)?;
        let settings = AgentModelSettings { base_url, model };
        self.database
            .set_setting(AGENT_MODEL_SETTINGS_KEY, &settings)?;
        Ok(settings)
    }

    pub fn load_external_acp_settings(&self) -> Result<ExternalAcpSettings, ApplicationError> {
        Ok(self
            .database
            .setting(EXTERNAL_ACP_SETTINGS_KEY)?
            .unwrap_or_default())
    }

    pub fn save_external_acp_settings(
        &mut self,
        settings: &ExternalAcpSettings,
    ) -> Result<ExternalAcpSettings, ApplicationError> {
        let program = settings.program.trim();
        let args = settings.args.trim();
        let cwd = settings.cwd.trim();
        if program.chars().count() > 512
            || args.chars().count() > 8 * 1024
            || cwd.chars().count() > 4 * 1024
            || [program, args, cwd]
                .into_iter()
                .any(|value| value.contains(['\0', '\r', '\n']))
            || (!cwd.is_empty() && !Path::new(cwd).is_absolute())
        {
            return Err(ApplicationError::InvalidExternalAcpSettings);
        }
        let settings = ExternalAcpSettings {
            program: program.into(),
            args: args.into(),
            cwd: cwd.into(),
        };
        self.database
            .set_setting(EXTERNAL_ACP_SETTINGS_KEY, &settings)?;
        Ok(settings)
    }

    pub fn list_agent_threads(&self, limit: usize) -> Result<Vec<NodeSummary>, ApplicationError> {
        let mut threads = self
            .database
            .list_nodes(false)?
            .into_iter()
            .filter(|node| node.kind() == NodeKind::Thread)
            .map(|node| NodeSummary::from(&node))
            .collect::<Vec<_>>();
        threads.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        threads.truncate(limit.min(200));
        Ok(threads)
    }

    pub fn agent_transcript(
        &self,
        thread_id: &str,
    ) -> Result<Vec<AgentTranscriptMessage>, ApplicationError> {
        let node = self.node(thread_id)?;
        let actual = node.kind();
        let Node::Thread { content, .. } = node else {
            return Err(ApplicationError::WrongKind {
                id: thread_id.into(),
                actual,
                expected: NodeKind::Thread,
            });
        };
        Ok(content
            .messages
            .into_iter()
            .filter_map(agent_transcript_message)
            .collect())
    }

    pub fn begin_external_agent_turn(
        &mut self,
        thread_id: Option<&str>,
        prompt: &str,
    ) -> Result<String, ApplicationError> {
        let prompt = validate_agent_prompt(prompt)?;
        let (thread_id, mut records) = self.agent_thread_records(thread_id, prompt)?;
        records.push(
            serde_json::to_value(ModelMessage::text(ModelRole::User, prompt)).map_err(|_| {
                ApplicationError::AgentTool("failed to encode ACP user message".into())
            })?,
        );
        self.persist_agent_thread_records(&thread_id, &records)?;
        Ok(thread_id)
    }

    pub fn complete_external_agent_turn(
        &mut self,
        thread_id: &str,
        content: &str,
        tools: &[AgentToolRun],
    ) -> Result<(), ApplicationError> {
        let (_, mut records) = self.agent_thread_records(Some(thread_id), "existing")?;
        for tool in tools.iter().take(128) {
            records.push(json!({
                "role": "event",
                "event": "acpTool",
                "name": bounded_agent_summary(&tool.name, "external-acp.tool"),
                "ok": tool.ok,
                "summary": bounded_agent_summary(&tool.summary, "外部 Agent 工具")
            }));
        }
        records.push(
            serde_json::to_value(ModelMessage::text(ModelRole::Assistant, content))
                .map_err(|_| ApplicationError::AgentTool("failed to encode ACP response".into()))?,
        );
        self.persist_agent_thread_records(thread_id, &records)
    }

    /// Runs one durable native Agent turn. User, assistant, and tool messages
    /// are committed incrementally, so an HTTP failure cannot erase an
    /// already-executed local tool or its audit record.
    pub fn run_agent_turn<P: CompletionProvider>(
        &mut self,
        provider: &mut P,
        thread_id: Option<&str>,
        prompt: &str,
    ) -> Result<AgentTurnResult, ApplicationError> {
        let prompt = validate_agent_prompt(prompt)?;
        let (thread_id, mut records) = self.agent_thread_records(thread_id, prompt)?;
        let user = ModelMessage::text(ModelRole::User, prompt);
        records.push(
            serde_json::to_value(&user).map_err(|_| {
                ApplicationError::AgentTool("failed to encode Agent message".into())
            })?,
        );
        self.persist_agent_thread_records(&thread_id, &records)?;

        let tools = local_agent_model_tools();
        let mut tool_runs = Vec::new();
        for _ in 0..MAX_AGENT_TOOL_ROUNDS {
            let context = agent_model_context(&records);
            let assistant = provider.complete(&context, &tools)?;
            records.push(serde_json::to_value(&assistant).map_err(|_| {
                ApplicationError::AgentTool("failed to encode Agent response".into())
            })?);
            self.persist_agent_thread_records(&thread_id, &records)?;

            if assistant.tool_calls.is_empty() {
                let content = assistant.content.unwrap_or_default();
                return Ok(AgentTurnResult {
                    thread_id,
                    content,
                    tools: tool_runs,
                });
            }

            for call in assistant.tool_calls {
                let (message, run) = self.execute_agent_model_tool(&call);
                records.push(serde_json::to_value(message).map_err(|_| {
                    ApplicationError::AgentTool("failed to encode Agent tool result".into())
                })?);
                self.persist_agent_thread_records(&thread_id, &records)?;
                tool_runs.push(run);
            }
        }

        let mut context = agent_model_context(&records);
        context.push(ModelMessage::text(
            ModelRole::System,
            "工具轮数已达上限。请只基于已有结果给出最终答复，不要再调用工具。",
        ));
        let final_message = provider.complete(&context, &[])?;
        if !final_message.tool_calls.is_empty() {
            return Err(ApplicationError::AgentModel(OpenAiError::InvalidResponse));
        }
        let content = final_message.content.clone().unwrap_or_default();
        records.push(serde_json::to_value(final_message).map_err(|_| {
            ApplicationError::AgentTool("failed to encode final Agent response".into())
        })?);
        self.persist_agent_thread_records(&thread_id, &records)?;
        Ok(AgentTurnResult {
            thread_id,
            content,
            tools: tool_runs,
        })
    }

    fn agent_thread_records(
        &mut self,
        thread_id: Option<&str>,
        first_prompt: &str,
    ) -> Result<(String, Vec<Value>), ApplicationError> {
        if let Some(thread_id) = thread_id {
            let node = self.node(thread_id)?;
            let actual = node.kind();
            let Node::Thread { content, .. } = node else {
                return Err(ApplicationError::WrongKind {
                    id: thread_id.into(),
                    actual,
                    expected: NodeKind::Thread,
                });
            };
            return Ok((thread_id.into(), content.messages));
        }

        let title = agent_thread_title(first_prompt);
        let node = Node::Thread {
            base: new_base(None, title)?,
            content: ThreadContent {
                messages: Vec::new(),
            },
        };
        let id = node.base().id.clone();
        self.database.put_node(&node)?;
        Ok((id, Vec::new()))
    }

    fn persist_agent_thread_records(
        &mut self,
        thread_id: &str,
        records: &[Value],
    ) -> Result<(), ApplicationError> {
        let mut node = self.node(thread_id)?;
        let actual = node.kind();
        let Node::Thread { base, content } = &mut node else {
            return Err(ApplicationError::WrongKind {
                id: thread_id.into(),
                actual,
                expected: NodeKind::Thread,
            });
        };
        content.messages = records.to_vec();
        base.updated_at = now_millis()?;
        self.database.put_node(&node)?;
        Ok(())
    }

    fn execute_agent_model_tool(
        &mut self,
        call: &ideall_agent::ModelToolCall,
    ) -> (ModelMessage, AgentToolRun) {
        let arguments = serde_json::from_str::<Value>(&call.function.arguments)
            .ok()
            .filter(Value::is_object);
        let response = arguments.map_or_else(
            || {
                json!({
                    "jsonrpc": "2.0",
                    "id": call.id,
                    "error": {"code": -32602, "message": "Invalid tool arguments"}
                })
            },
            |arguments| {
                self.handle_local_agent_mcp(json!({
                    "jsonrpc": "2.0",
                    "id": call.id,
                    "method": "tools/call",
                    "params": {"name": call.function.name, "arguments": arguments}
                }))
            },
        );
        let (ok, summary) = agent_tool_response_summary(&response);
        let payload = bounded_agent_tool_payload(&response, ok, &summary);
        (
            ModelMessage::tool(&call.id, payload),
            AgentToolRun {
                name: call.function.name.clone(),
                ok,
                summary,
            },
        )
    }

    /// Runs the 0.2-compatible encrypted notes domain without persisting the
    /// synchronization code. The caller owns authenticated transport and any
    /// platform secure-storage policy.
    pub fn sync_notes<T: SyncTransport>(
        &mut self,
        code: &str,
        transport: &mut T,
    ) -> Result<SyncRunResult, ApplicationError> {
        let now = now_millis()?;
        let mut store = NoteSyncStore {
            database: &mut self.database,
        };
        Ok(run_sync_with(
            code,
            SyncScope::Notes,
            SyncBlockBudget::NOTES,
            now,
            &mut store,
            transport,
            valid_remote_note,
            merge_sync_notes,
            gc_sync_notes,
        )?)
    }

    pub fn sync_subscriptions<T: SyncTransport>(
        &mut self,
        code: &str,
        transport: &mut T,
    ) -> Result<SyncRunResult, ApplicationError> {
        let now = now_millis()?;
        let mut store = SubscriptionSyncStore {
            database: &mut self.database,
        };
        Ok(run_sync(
            code,
            SyncScope::Subscriptions,
            SyncBlockBudget::SUBSCRIPTIONS,
            now,
            &mut store,
            transport,
            valid_remote_subscription,
        )?)
    }

    pub fn sync_bookmarks<T: SyncTransport>(
        &mut self,
        code: &str,
        transport: &mut T,
    ) -> Result<SyncRunResult, ApplicationError> {
        let now = now_millis()?;
        let mut store = BookmarkSyncStore {
            database: &mut self.database,
        };
        Ok(run_sync_with(
            code,
            SyncScope::Bookmarks,
            SyncBlockBudget::BOOKMARKS,
            now,
            &mut store,
            transport,
            valid_remote_bookmark_node,
            ideall_sync::union_merge,
            gc_bookmarks,
        )?)
    }

    pub fn append_agent_audit(
        &mut self,
        input: AgentAuditInput,
    ) -> Result<AgentAuditRecord, ApplicationError> {
        let record = new_audit(input, now_millis()?)?;
        self.database
            .append_agent_audit(&record.id, record.created_at, &record, 1_000)?;
        Ok(record)
    }

    pub fn complete_agent_audit(
        &mut self,
        completion: AgentAuditCompletion,
    ) -> Result<AgentAuditRecord, ApplicationError> {
        let current = self
            .database
            .agent_audit::<AgentAuditRecord>(&completion.id)?
            .ok_or_else(|| ApplicationError::AgentAuditNotFound(completion.id.clone()))?;
        let completed = complete_audit(&current, completion, now_millis()?)?;
        self.database
            .compare_and_swap_agent_audit(&current.id, &current, &completed)?;
        Ok(completed)
    }

    pub fn list_agent_audits(
        &self,
        limit: usize,
    ) -> Result<Vec<AgentAuditRecord>, ApplicationError> {
        Ok(self.database.list_agent_audits(limit)?)
    }

    /// Handles one request against ideall's in-process MCP boundary.
    ///
    /// The server is rebuilt for every request so authorization is evaluated
    /// against a fresh grant at execution time. The default grant deliberately
    /// exposes file metadata and note creation, but not existing note content
    /// or blob bytes.
    pub fn handle_local_agent_mcp(&mut self, request: Value) -> Value {
        let request_id = request.get("id").cloned().unwrap_or(Value::Null);
        let now = match now_millis() {
            Ok(now) => now,
            Err(_) => {
                return json!({
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "error": {"code": -32603, "message": "Local Agent clock is unavailable"}
                });
            }
        };
        let handler = WorkspaceToolHandler { workspace: self };
        let server = LocalMcpServer::new(
            agent_grant(now, None),
            "ideall-agent",
            "loopback",
            local_agent_tools(),
            handler,
        );
        match server {
            Ok(mut server) => server.handle(request, now),
            Err(_) => json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "error": {"code": -32603, "message": "Local Agent initialization failed"}
            }),
        }
    }

    /// Creates a blank note through the same MCP path used by the native Agent
    /// workspace. This is intentionally a narrow, content-free smoke action.
    pub fn create_agent_note_via_mcp(&mut self, title: &str) -> Result<Node, ApplicationError> {
        let response = self.handle_local_agent_mcp(json!({
            "jsonrpc": "2.0",
            "id": "native-ui-create-note",
            "method": "tools/call",
            "params": {
                "name": "fs.create-note",
                "arguments": {"title": title}
            }
        }));
        if let Some(message) = response
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
        {
            return Err(ApplicationError::AgentTool(message.to_owned()));
        }
        let result = response
            .get("result")
            .ok_or_else(|| ApplicationError::AgentTool("invalid MCP response".into()))?;
        if result.get("isError").and_then(Value::as_bool) == Some(true) {
            let message = result
                .get("content")
                .and_then(Value::as_array)
                .and_then(|content| content.first())
                .and_then(|entry| entry.get("text"))
                .and_then(Value::as_str)
                .unwrap_or("Agent tool rejected the request");
            return Err(ApplicationError::AgentTool(message.to_owned()));
        }
        let node_id = result
            .get("structuredContent")
            .and_then(|content| content.get("nodeId"))
            .and_then(Value::as_str)
            .ok_or_else(|| ApplicationError::AgentTool("missing Agent result id".into()))?;
        self.node(node_id)
    }

    fn create_note_from_agent(&mut self, title: &str) -> Result<Node, ApplicationError> {
        let pending = self.append_agent_audit(AgentAuditInput {
            source: AuditSource::Tool,
            operation: "fs.create-note".into(),
            title: "Agent 创建笔记".into(),
            summary: "等待本地持久化".into(),
            status: AuditStatus::Pending,
            effect: ToolEffect::Write,
            risk: ToolRisk::Medium,
            target: Some(AuditTarget {
                kind: Some("note".into()),
                id: None,
                label: "新笔记".into(),
            }),
            thread_id: None,
            message_id: None,
        })?;
        match self.create_note(None, title) {
            Ok(node) => {
                self.complete_agent_audit(AgentAuditCompletion {
                    id: pending.id,
                    status: AuditStatus::Committed,
                    summary: "已创建 1 条本地笔记".into(),
                })?;
                Ok(node)
            }
            Err(error) => {
                let _ = self.complete_agent_audit(AgentAuditCompletion {
                    id: pending.id,
                    status: AuditStatus::Failed,
                    summary: "Agent 创建笔记失败".into(),
                });
                Err(error)
            }
        }
    }

    fn ensure_home_root(&mut self) -> Result<(), ApplicationError> {
        match self.database.get_node(HOME_ROOT_ID)? {
            Some(Node::Folder { .. }) => Ok(()),
            Some(node) => Err(ApplicationError::WrongKind {
                id: HOME_ROOT_ID.into(),
                actual: node.kind(),
                expected: NodeKind::Folder,
            }),
            None => {
                let timestamp = now_millis()?;
                self.database.put_node(&Node::Folder {
                    base: BaseNode {
                        id: HOME_ROOT_ID.into(),
                        parent_id: None,
                        sort_key: "00000000000000000000-home".into(),
                        title: "我的".into(),
                        tags: Vec::new(),
                        created_at: timestamp,
                        updated_at: timestamp,
                        deleted_at: None,
                        meta: BTreeMap::new(),
                    },
                    content: None,
                })?;
                Ok(())
            }
        }
    }
}

struct WorkspaceToolHandler<'a> {
    workspace: &'a mut LocalWorkspace,
}

impl ToolHandler for WorkspaceToolHandler<'_> {
    fn call(&mut self, name: &str, arguments: Value) -> Result<Value, String> {
        match name {
            "fs.list" => self
                .workspace
                .list_home()
                .map(|nodes| {
                    Value::Array(
                        nodes
                            .into_iter()
                            .map(|node| {
                                json!({
                                    "id": node.id,
                                    "parentId": node.parent_id,
                                    "kind": node_kind_name(node.kind),
                                    "title": node.title,
                                    "updatedAt": node.updated_at
                                })
                            })
                            .collect(),
                    )
                })
                .map_err(|error| error.to_string()),
            "fs.create-note" => {
                let title = arguments
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("Agent 草稿")
                    .trim();
                if title.is_empty()
                    || title.chars().count() > 120
                    || title.chars().any(char::is_control)
                {
                    return Err("title must contain 1 to 120 printable characters".into());
                }
                self.workspace
                    .create_note_from_agent(title)
                    .map(|node| {
                        json!({
                            "nodeId": node.base().id,
                            "kind": "note",
                            "created": true
                        })
                    })
                    .map_err(|error| error.to_string())
            }
            _ => Err("tool is not implemented".into()),
        }
    }
}

fn local_agent_tools() -> Vec<McpTool> {
    vec![
        McpTool {
            name: "fs.list".into(),
            description: "List local node metadata without reading note or blob content".into(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
            intent: Intent::ListFiles,
        },
        McpTool {
            name: "fs.create-note".into(),
            description: "Create a blank local note and persist a redacted write audit".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "title": {"type": "string", "minLength": 1, "maxLength": 120}
                },
                "additionalProperties": false
            }),
            intent: Intent::Create(NodeKind::Note),
        },
    ]
}

fn local_agent_model_tools() -> Vec<ModelTool> {
    local_agent_tools()
        .into_iter()
        .map(|tool| ModelTool {
            name: tool.name,
            description: tool.description,
            input_schema: tool.input_schema,
        })
        .collect()
}

fn validate_agent_prompt(value: &str) -> Result<&str, ApplicationError> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > MAX_AGENT_PROMPT_BYTES
        || value
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(ApplicationError::InvalidAgentPrompt);
    }
    Ok(value)
}

fn agent_thread_title(prompt: &str) -> String {
    let title = prompt
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(48)
        .collect::<String>();
    if prompt.chars().count() > 48 {
        format!("{title}…")
    } else {
        title
    }
}

fn agent_model_context(records: &[Value]) -> Vec<ModelMessage> {
    let mut messages = records
        .iter()
        .filter_map(|value| serde_json::from_value::<ModelMessage>(value.clone()).ok())
        .collect::<Vec<_>>();
    if messages.len() > MAX_AGENT_CONTEXT_MESSAGES {
        messages.drain(..messages.len() - MAX_AGENT_CONTEXT_MESSAGES);
        while messages
            .first()
            .is_some_and(|message| message.role != ModelRole::User)
        {
            messages.remove(0);
        }
    }
    messages.insert(
        0,
        ModelMessage::text(ModelRole::System, AGENT_SYSTEM_PROMPT),
    );
    messages
}

fn agent_transcript_message(value: Value) -> Option<AgentTranscriptMessage> {
    if value.get("role").and_then(Value::as_str) == Some("event")
        && value.get("event").and_then(Value::as_str) == Some("acpTool")
    {
        let name = value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("external-acp.tool");
        let summary = value
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or("外部 Agent 工具");
        let status = if value.get("ok").and_then(Value::as_bool) == Some(true) {
            "完成"
        } else {
            "未完成"
        };
        return Some(AgentTranscriptMessage {
            role: ModelRole::Tool,
            content: format!("{name} · {status} · {summary}"),
        });
    }
    serde_json::from_value::<ModelMessage>(value)
        .ok()
        .and_then(|message| {
            message.content.map(|content| AgentTranscriptMessage {
                role: message.role,
                content,
            })
        })
}

fn agent_tool_response_summary(response: &Value) -> (bool, String) {
    if let Some(message) = response
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
    {
        return (false, bounded_agent_summary(message, "工具调用失败"));
    }
    let result = response.get("result");
    let ok = result
        .and_then(|result| result.get("isError"))
        .and_then(Value::as_bool)
        != Some(true)
        && result.is_some();
    let summary = result
        .and_then(|result| result.get("content"))
        .and_then(Value::as_array)
        .and_then(|content| content.first())
        .and_then(|entry| entry.get("text"))
        .and_then(Value::as_str)
        .unwrap_or(if ok {
            "工具调用完成"
        } else {
            "工具调用失败"
        });
    (ok, bounded_agent_summary(summary, "工具调用完成"))
}

fn bounded_agent_summary(value: &str, fallback: &str) -> String {
    let value = value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(240)
        .collect::<String>();
    if value.trim().is_empty() {
        fallback.into()
    } else {
        value
    }
}

fn bounded_agent_tool_payload(response: &Value, ok: bool, summary: &str) -> String {
    let data = response
        .get("result")
        .and_then(|result| result.get("structuredContent"))
        .cloned()
        .unwrap_or(Value::Null);
    let payload = json!({"ok": ok, "summary": summary, "data": data});
    let encoded = serde_json::to_string(&payload)
        .unwrap_or_else(|_| r#"{"ok":false,"summary":"tool result encoding failed"}"#.to_owned());
    if encoded.len() <= MAX_AGENT_TOOL_RESULT_BYTES {
        encoded
    } else {
        serde_json::to_string(&json!({
            "ok": ok,
            "summary": summary,
            "truncated": true,
            "reason": "tool result exceeded the local model context budget"
        }))
        .unwrap_or_else(|_| "{\"ok\":false}".into())
    }
}

struct NoteSyncStore<'a> {
    database: &'a mut Database,
}

struct SubscriptionSyncStore<'a> {
    database: &'a mut Database,
}

impl SyncLocalStore<SyncSubscription> for SubscriptionSyncStore<'_> {
    fn list_all(&mut self) -> Result<Vec<SyncSubscription>, LocalStoreError> {
        self.database
            .list_nodes(true)
            .map_err(local_store_error)
            .map(|nodes| nodes.iter().filter_map(node_to_sync_subscription).collect())
    }

    fn compare_and_swap(
        &mut self,
        records: &[SyncSubscription],
        expected: &[SyncSubscription],
    ) -> Result<Vec<SyncSubscription>, LocalStoreError> {
        let records = records
            .iter()
            .map(sync_subscription_to_node)
            .collect::<Vec<_>>();
        let expected = expected
            .iter()
            .map(sync_subscription_to_node)
            .collect::<Vec<_>>();
        self.database
            .compare_and_swap_nodes_projected(
                &records,
                &expected,
                |node| node.kind() == NodeKind::Feed,
                canonical_feed_node,
            )
            .map_err(local_store_error)
            .map(|nodes| nodes.iter().filter_map(node_to_sync_subscription).collect())
    }
}

struct BookmarkSyncStore<'a> {
    database: &'a mut Database,
}

impl SyncLocalStore<Node> for BookmarkSyncStore<'_> {
    fn list_all(&mut self) -> Result<Vec<Node>, LocalStoreError> {
        self.database
            .list_nodes(true)
            .map_err(local_store_error)
            .map(|nodes| {
                nodes
                    .iter()
                    .filter_map(node_to_sync_bookmark_node)
                    .collect()
            })
    }

    fn compare_and_swap(
        &mut self,
        records: &[Node],
        expected: &[Node],
    ) -> Result<Vec<Node>, LocalStoreError> {
        let records = records
            .iter()
            .map(sync_bookmark_node_to_native)
            .collect::<Vec<_>>();
        let expected = expected
            .iter()
            .map(sync_bookmark_node_to_native)
            .collect::<Vec<_>>();
        self.database
            .compare_and_swap_nodes(&records, &expected, bookmark_node_in_scope)
            .map_err(local_store_error)
            .map(|nodes| {
                nodes
                    .iter()
                    .filter_map(node_to_sync_bookmark_node)
                    .collect()
            })
    }
}

impl SyncLocalStore<SyncNote> for NoteSyncStore<'_> {
    fn list_all(&mut self) -> Result<Vec<SyncNote>, LocalStoreError> {
        self.database
            .list_nodes(true)
            .map_err(local_store_error)
            .map(|nodes| nodes.iter().filter_map(node_to_sync_note).collect())
    }

    fn compare_and_swap(
        &mut self,
        records: &[SyncNote],
        expected: &[SyncNote],
    ) -> Result<Vec<SyncNote>, LocalStoreError> {
        let records = records.iter().map(sync_note_to_node).collect::<Vec<_>>();
        let expected = expected.iter().map(sync_note_to_node).collect::<Vec<_>>();
        self.database
            .compare_and_swap_nodes(&records, &expected, |node| node.kind() == NodeKind::Note)
            .map_err(local_store_error)
            .map(|nodes| nodes.iter().filter_map(node_to_sync_note).collect())
    }
}

fn local_store_error(error: StorageError) -> LocalStoreError {
    match error {
        StorageError::SnapshotConflict => LocalStoreError::Conflict,
        error => LocalStoreError::Other(error.to_string()),
    }
}

fn node_to_sync_note(node: &Node) -> Option<SyncNote> {
    let Node::Note { base, content } = node else {
        return None;
    };
    let block_meta = base
        .meta
        .get("blockMeta")
        .and_then(Value::as_object)
        .map(|object| {
            object
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        });
    Some(SyncNote {
        id: base.id.clone(),
        title: base.title.clone(),
        content: content.clone(),
        parent_id: base
            .parent_id
            .as_ref()
            .filter(|parent| parent.as_str() != HOME_ROOT_ID)
            .cloned(),
        sort_key: base.sort_key.clone(),
        tags: base.tags.clone(),
        created_at: base.created_at,
        updated_at: base.updated_at,
        deleted_at: base.deleted_at,
        block_meta,
    })
}

fn sync_note_to_node(note: &SyncNote) -> Node {
    let mut meta = BTreeMap::new();
    if let Some(block_meta) = &note.block_meta {
        meta.insert(
            "blockMeta".into(),
            Value::Object(block_meta.clone().into_iter().collect()),
        );
    }
    Node::Note {
        base: BaseNode {
            id: note.id.clone(),
            parent_id: Some(
                note.parent_id
                    .clone()
                    .unwrap_or_else(|| HOME_ROOT_ID.into()),
            ),
            sort_key: note.sort_key.clone(),
            title: note.title.clone(),
            tags: note.tags.clone(),
            created_at: note.created_at,
            updated_at: note.updated_at,
            deleted_at: note.deleted_at,
            meta,
        },
        content: note.content.clone(),
    }
}

fn node_to_sync_subscription(node: &Node) -> Option<SyncSubscription> {
    let Node::Feed { base, content } = node else {
        return None;
    };
    Some(SyncSubscription {
        id: base.id.clone(),
        r#type: content.r#type,
        key: content.key.clone(),
        title: base.title.clone(),
        favicon: content.favicon.clone(),
        entity_label: content.entity_label.clone(),
        entity_name: content.entity_name.clone(),
        search_keyword: content.search_keyword.clone(),
        search_domain: content.search_domain.clone(),
        created_at: base.created_at,
        updated_at: base.updated_at,
        deleted_at: base.deleted_at,
    })
}

fn sync_subscription_to_node(subscription: &SyncSubscription) -> Node {
    Node::Feed {
        base: BaseNode {
            id: subscription.id.clone(),
            parent_id: Some(HOME_ROOT_ID.into()),
            sort_key: format!("{:020}-{}", subscription.created_at.max(0), subscription.id),
            title: subscription.title.clone(),
            tags: Vec::new(),
            created_at: subscription.created_at,
            updated_at: subscription.updated_at,
            deleted_at: subscription.deleted_at,
            meta: BTreeMap::new(),
        },
        content: FeedContent {
            r#type: subscription.r#type,
            key: subscription.key.clone(),
            favicon: subscription.favicon.clone(),
            entity_label: subscription.entity_label.clone(),
            entity_name: subscription.entity_name.clone(),
            search_keyword: subscription.search_keyword.clone(),
            search_domain: subscription.search_domain.clone(),
        },
    }
}

fn canonical_feed_node(node: &Node) -> Node {
    node_to_sync_subscription(node)
        .as_ref()
        .map(sync_subscription_to_node)
        .unwrap_or_else(|| node.clone())
}

fn valid_remote_subscription(subscription: &SyncSubscription, now: i64) -> bool {
    !subscription.key.is_empty()
        && subscription.key.trim() == subscription.key
        && subscription.id
            == format!(
                "{}:{}",
                subscription_type_name(subscription.r#type),
                subscription.key
            )
        && (subscription.r#type != SubscriptionType::Tool
            || validated_bookmark_url(&subscription.key).is_ok())
        && is_sane_sync_timestamp(subscription.created_at, now)
        && is_sane_sync_timestamp(subscription.updated_at, now)
        && subscription
            .deleted_at
            .is_none_or(|timestamp| is_sane_sync_timestamp(timestamp, now))
}

const fn subscription_type_name(value: SubscriptionType) -> &'static str {
    match value {
        SubscriptionType::Publisher => "publisher",
        SubscriptionType::Entity => "entity",
        SubscriptionType::Tool => "tool",
        SubscriptionType::Search => "search",
        SubscriptionType::Peer => "peer",
    }
}

fn bookmark_node_in_scope(node: &Node) -> bool {
    match node {
        Node::Folder { base, .. } => base.id != HOME_ROOT_ID,
        Node::Bookmark { .. } => true,
        _ => false,
    }
}

fn node_to_sync_bookmark_node(node: &Node) -> Option<Node> {
    if !bookmark_node_in_scope(node) {
        return None;
    }
    let mut node = node.clone();
    if node.base().parent_id.as_deref() == Some(HOME_ROOT_ID) {
        node.base_mut().parent_id = None;
    }
    Some(node)
}

fn sync_bookmark_node_to_native(node: &Node) -> Node {
    let mut node = node.clone();
    if node.base().parent_id.is_none() {
        node.base_mut().parent_id = Some(HOME_ROOT_ID.into());
    }
    node
}

fn valid_remote_bookmark_node(node: &Node, now: i64) -> bool {
    let base = node.base();
    if base.id.is_empty()
        || base.id == HOME_ROOT_ID
        || base.sort_key.is_empty()
        || !is_sane_sync_timestamp(base.created_at, now)
        || !is_sane_sync_timestamp(base.updated_at, now)
        || base
            .deleted_at
            .is_some_and(|timestamp| !is_sane_sync_timestamp(timestamp, now))
    {
        return false;
    }
    match node {
        Node::Folder { base, .. } => base.parent_id.is_none(),
        Node::Bookmark { base, content } => {
            base.parent_id
                .as_ref()
                .is_none_or(|parent| !parent.is_empty())
                && validated_bookmark_url(&content.url).is_ok()
        }
        _ => false,
    }
}

fn gc_bookmarks(nodes: &[Node], now: i64) -> Vec<Node> {
    let mut kept = prune_expired_tombstones(nodes, now, TOMBSTONE_TTL_MS);
    let mut folder_versions = HashMap::new();
    for node in &mut kept {
        if let Node::Folder { base, .. } = node {
            folder_versions.insert(base.id.clone(), base.updated_at);
            if base.parent_id.is_some() {
                base.parent_id = None;
                base.updated_at = base.updated_at.saturating_add(1);
            }
        }
    }
    let active_folders = kept
        .iter()
        .filter(|node| node.kind() == NodeKind::Folder && node.base().deleted_at.is_none())
        .map(|node| node.base().id.clone())
        .collect::<HashSet<_>>();
    for node in &mut kept {
        let Node::Bookmark { base, .. } = node else {
            continue;
        };
        let Some(parent) = base.parent_id.clone() else {
            continue;
        };
        if base.deleted_at.is_none() && !active_folders.contains(&parent) {
            base.parent_id = None;
            base.updated_at = base
                .updated_at
                .max(
                    folder_versions
                        .get(&parent)
                        .copied()
                        .unwrap_or(base.updated_at),
                )
                .saturating_add(1);
        }
    }
    kept
}

fn valid_remote_note(note: &SyncNote, now: i64) -> bool {
    !note.id.is_empty()
        && !note.sort_key.is_empty()
        && note
            .parent_id
            .as_ref()
            .is_none_or(|parent| !parent.is_empty() && parent != HOME_ROOT_ID)
        && note.content.iter().all(Value::is_object)
        && note.block_meta.as_ref().is_none_or(|metadata| {
            metadata.values().all(|value| {
                let Some(value) = value.as_object() else {
                    return false;
                };
                value.get("v").is_some_and(Value::is_number)
                    && value.get("by").is_some_and(Value::is_string)
                    && value.get("sk").is_some_and(Value::is_string)
                    && value.get("del").is_none_or(|deleted| {
                        deleted.as_i64().is_some_and(|timestamp| {
                            timestamp >= 0 && timestamp <= now + 86_400_000
                        })
                    })
            })
        })
}

fn new_base(parent_id: Option<&str>, title: String) -> Result<BaseNode, ApplicationError> {
    let timestamp = now_millis()?;
    let id = Uuid::new_v4().to_string();
    Ok(BaseNode {
        id: id.clone(),
        parent_id: Some(parent_id.unwrap_or(HOME_ROOT_ID).to_owned()),
        sort_key: format!("{timestamp:020}-{id}"),
        title: validated_node_title(title)?,
        tags: Vec::new(),
        created_at: timestamp,
        updated_at: timestamp,
        deleted_at: None,
        meta: BTreeMap::new(),
    })
}

fn validated_node_title(title: String) -> Result<String, ApplicationError> {
    if title.trim().is_empty() || title.len() > 1024 {
        return Err(ApplicationError::InvalidNodeTitle);
    }
    Ok(title)
}

fn validated_bookmark_url(url: &str) -> Result<Url, ApplicationError> {
    let parsed = Url::parse(url).map_err(|_| ApplicationError::InvalidBookmarkUrl)?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(ApplicationError::InvalidBookmarkUrl);
    }
    Ok(parsed)
}

fn validated_subscription_key(value: &str) -> Result<String, ApplicationError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 2_048 || value.chars().any(char::is_control) {
        return Err(ApplicationError::InvalidSubscriptionKey);
    }
    Ok(value.to_owned())
}

fn canonical_sync_server(value: &str) -> Result<String, ApplicationError> {
    if value.len() > 2_048 {
        return Err(ApplicationError::InvalidSyncServer);
    }
    let mut url = Url::parse(value.trim()).map_err(|_| ApplicationError::InvalidSyncServer)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ApplicationError::InvalidSyncServer);
    }
    url.set_query(None);
    url.set_fragment(None);
    let canonical = url.to_string();
    Ok(canonical.trim_end_matches('/').to_owned())
}

fn node_kind_name(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Folder => "folder",
        NodeKind::Note => "note",
        NodeKind::Bookmark => "bookmark",
        NodeKind::File => "file",
        NodeKind::Feed => "feed",
        NodeKind::Thread => "thread",
    }
}

fn media_type_for_path(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("md" | "markdown") => "text/markdown",
        Some("txt" | "log") => "text/plain",
        Some("json") => "application/json",
        Some("js" | "mjs" | "cjs") => "application/javascript",
        Some("ts" | "tsx") => "application/typescript",
        Some("html" | "htm") => "text/html",
        Some("css") => "text/css",
        Some("xml") => "application/xml",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("pdf") => "application/pdf",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("mp4") => "video/mp4",
        Some("sqlite" | "sqlite3" | "db") => "application/x-sqlite3",
        _ => "application/octet-stream",
    }
}

fn is_text_media_type(media_type: &str) -> bool {
    let media_type = media_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    media_type.starts_with("text/")
        || matches!(
            media_type.as_str(),
            "application/json"
                | "application/javascript"
                | "application/typescript"
                | "application/xml"
                | "application/yaml"
                | "application/toml"
                | "image/svg+xml"
        )
}

fn flatten_tree(nodes: &[NodeProjection]) -> Vec<NodeSummary> {
    let mut children: HashMap<Option<&str>, Vec<&NodeProjection>> = HashMap::new();
    for node in nodes {
        if node.id != HOME_ROOT_ID {
            children
                .entry(node.parent_id.as_deref())
                .or_default()
                .push(node);
        }
    }
    for group in children.values_mut() {
        group.sort_by(|left, right| {
            left.sort_key
                .cmp(&right.sort_key)
                .then_with(|| left.id.cmp(&right.id))
        });
    }
    let mut result = Vec::with_capacity(nodes.len().saturating_sub(1));
    let mut visited = HashSet::new();
    append_tree(Some(HOME_ROOT_ID), 0, &children, &mut visited, &mut result);
    append_tree(None, 0, &children, &mut visited, &mut result);
    for node in nodes {
        if node.id != HOME_ROOT_ID && visited.insert(node.id.as_str()) {
            result.push(NodeSummary::from(node));
        }
    }
    result
}

fn append_tree<'a>(
    parent: Option<&'a str>,
    depth: usize,
    children: &HashMap<Option<&'a str>, Vec<&'a NodeProjection>>,
    visited: &mut HashSet<&'a str>,
    result: &mut Vec<NodeSummary>,
) {
    let Some(group) = children.get(&parent) else {
        return;
    };
    for node in group {
        if !visited.insert(node.id.as_str()) {
            continue;
        }
        let mut summary = NodeSummary::from(*node);
        summary.depth = depth;
        result.push(summary);
        append_tree(Some(node.id.as_str()), depth + 1, children, visited, result);
    }
}

fn now_millis() -> Result<i64, ApplicationError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ApplicationError::InvalidClock)?;
    Ok(duration.as_millis().min(i64::MAX as u128) as i64)
}

fn ensure_device_id(database: &mut Database) -> Result<String, ApplicationError> {
    if let Some(device_id) = database.setting::<String>(DEVICE_ID_KEY)?
        && !device_id.trim().is_empty()
        && device_id.len() <= 128
    {
        return Ok(device_id);
    }
    let device_id = format!("native-{}", Uuid::new_v4().simple());
    database.set_setting(DEVICE_ID_KEY, &device_id)?;
    Ok(device_id)
}

fn read_plain_note(note_id: &str, content: &[Value]) -> PlainNoteDocument {
    let normalized = normalize_block_ids(note_id, content);
    let projection = project_note(&normalized);
    PlainNoteDocument {
        text: projection.text,
        editable: true,
        protected_blocks: projection.protected_blocks,
    }
}

fn apply_note_edit(
    base: &mut BaseNode,
    content: &mut Vec<Value>,
    text: &str,
    device_id: &str,
    timestamp: i64,
) -> Result<(), ApplicationError> {
    let original = normalize_block_ids(&base.id, content);
    let mut edited = apply_note_text(&original, text).map_err(note_document_error)?;
    for block in &mut edited {
        let Some(object) = block.as_object_mut() else {
            continue;
        };
        let has_id = object
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| !id.is_empty());
        if !has_id {
            object.insert(
                "id".into(),
                Value::String(format!("blk_{}", Uuid::new_v4().simple())),
            );
        }
    }
    update_block_metadata(base, &original, &edited, device_id, timestamp);
    *content = edited;
    Ok(())
}

fn note_document_error(error: NoteDocumentError) -> ApplicationError {
    ApplicationError::InvalidNoteDocument(error.to_string())
}

fn normalize_block_ids(note_id: &str, content: &[Value]) -> Vec<Value> {
    content
        .iter()
        .enumerate()
        .map(|(index, block)| {
            let Some(object) = block.as_object() else {
                return block.clone();
            };
            if object
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| !id.is_empty())
            {
                return block.clone();
            }
            let mut normalized = object.clone();
            normalized.insert(
                "id".into(),
                Value::String(deterministic_block_id(note_id, index, block)),
            );
            Value::Object(normalized)
        })
        .collect()
}

fn deterministic_block_id(note_id: &str, index: usize, block: &Value) -> String {
    let serialized = serde_json::to_string(block).unwrap_or_else(|_| "null".into());
    format!(
        "blk_{}_{}_{}",
        base36(djb2(note_id)),
        index,
        base36(djb2(&serialized))
    )
}

fn djb2(value: &str) -> u32 {
    value.encode_utf16().fold(5_381_u32, |hash, unit| {
        hash.wrapping_mul(33).wrapping_add(u32::from(unit))
    })
}

fn base36(mut value: u32) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if value == 0 {
        return "0".into();
    }
    let mut output = Vec::new();
    while value > 0 {
        output.push(DIGITS[(value % 36) as usize]);
        value /= 36;
    }
    output.reverse();
    String::from_utf8(output).expect("base36 alphabet is UTF-8")
}

fn update_block_metadata(
    base: &mut BaseNode,
    original: &[Value],
    edited: &[Value],
    device_id: &str,
    timestamp: i64,
) {
    let Some(original_ids) = block_ids(original) else {
        base.meta.remove("blockMeta");
        return;
    };
    let Some(edited_ids) = block_ids(edited) else {
        base.meta.remove("blockMeta");
        return;
    };
    let original_by_id = original
        .iter()
        .filter_map(|block| block_id(block).map(|id| (id, block)))
        .collect::<HashMap<_, _>>();
    let existing = base
        .meta
        .get("blockMeta")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let desired_sort_keys = desired_block_sort_keys(&original_ids, &edited_ids, &existing);
    let mut metadata = existing;

    for (index, block) in edited.iter().enumerate() {
        let id = &edited_ids[index];
        let desired_sort_key = &desired_sort_keys[index];
        let old_meta = metadata.get(id);
        let unchanged = original_by_id
            .get(id.as_str())
            .is_some_and(|old| *old == block);
        let metadata_unchanged = old_meta.is_some_and(|value| {
            value.get("del").is_none()
                && value.get("sk").and_then(Value::as_str) == Some(desired_sort_key)
                && value.get("v").and_then(Value::as_f64).is_some()
                && value.get("by").and_then(Value::as_str).is_some()
        });
        if unchanged && metadata_unchanged {
            continue;
        }
        let version = old_meta
            .and_then(|value| value.get("v"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
            .floor()
            .max(0.0) as u64
            + 1;
        metadata.insert(
            id.clone(),
            json!({
                "v": version,
                "by": device_id,
                "sk": desired_sort_key
            }),
        );
    }

    let edited_set = edited_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    for (index, id) in original_ids.iter().enumerate() {
        if edited_set.contains(id.as_str()) {
            continue;
        }
        let old_meta = metadata.get(id);
        let version = old_meta
            .and_then(|value| value.get("v"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
            .floor()
            .max(0.0) as u64
            + 1;
        let sort_key = old_meta
            .and_then(|value| value.get("sk"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| native_block_sort_key(index));
        metadata.insert(
            id.clone(),
            json!({
                "v": version,
                "by": device_id,
                "sk": sort_key,
                "del": timestamp
            }),
        );
    }
    base.meta
        .insert("blockMeta".into(), Value::Object(metadata));
}

fn block_ids(content: &[Value]) -> Option<Vec<String>> {
    content
        .iter()
        .map(|block| block_id(block).map(str::to_owned))
        .collect()
}

fn block_id(block: &Value) -> Option<&str> {
    block
        .as_object()?
        .get("id")?
        .as_str()
        .filter(|id| !id.is_empty())
}

fn desired_block_sort_keys(
    original_ids: &[String],
    edited_ids: &[String],
    existing: &serde_json::Map<String, Value>,
) -> Vec<String> {
    let edited_set = edited_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let original_remaining = original_ids
        .iter()
        .filter(|id| edited_set.contains(id.as_str()))
        .map(String::as_str)
        .collect::<Vec<_>>();
    let original_set = original_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let edited_existing = edited_ids
        .iter()
        .filter(|id| original_set.contains(id.as_str()))
        .map(String::as_str)
        .collect::<Vec<_>>();
    let first_new = edited_ids
        .iter()
        .position(|id| !original_set.contains(id.as_str()));
    let new_only_at_end = first_new.is_none_or(|index| {
        edited_ids[index..]
            .iter()
            .all(|id| !original_set.contains(id.as_str()))
    });
    let mut previous = None::<String>;
    let existing_keys_are_monotonic = original_remaining.iter().all(|id| {
        let Some(sort_key) = existing
            .get(*id)
            .and_then(|value| value.get("sk"))
            .and_then(Value::as_str)
        else {
            return false;
        };
        let monotonic = previous.as_deref().is_none_or(|value| value < sort_key);
        previous = Some(sort_key.to_owned());
        monotonic
    });
    if original_remaining == edited_existing && new_only_at_end && existing_keys_are_monotonic {
        let mut last = None::<String>;
        return edited_ids
            .iter()
            .map(|id| {
                let sort_key = existing
                    .get(id)
                    .and_then(|value| value.get("sk"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        last.as_ref()
                            .map_or_else(|| native_block_sort_key(0), |value| format!("{value}z"))
                    });
                last = Some(sort_key.clone());
                sort_key
            })
            .collect();
    }
    (0..edited_ids.len()).map(native_block_sort_key).collect()
}

fn native_block_sort_key(index: usize) -> String {
    format!("native-{index:016x}")
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use ideall_agent::{ModelToolCall, ModelToolFunction};
    use ideall_sync::{Encrypted, SyncGenerationPart, SyncManifest, TransportError, derive_keys};

    #[derive(Default)]
    struct CaptureTransport {
        parts: Vec<SyncGenerationPart>,
    }

    struct ScriptedProvider {
        responses: VecDeque<Result<ModelMessage, OpenAiError>>,
        requests: Vec<(Vec<ModelMessage>, Vec<String>)>,
    }

    impl ScriptedProvider {
        fn new(responses: impl IntoIterator<Item = Result<ModelMessage, OpenAiError>>) -> Self {
            Self {
                responses: responses.into_iter().collect(),
                requests: Vec::new(),
            }
        }
    }

    impl CompletionProvider for ScriptedProvider {
        fn complete(
            &mut self,
            messages: &[ModelMessage],
            tools: &[ModelTool],
        ) -> Result<ModelMessage, OpenAiError> {
            self.requests.push((
                messages.to_vec(),
                tools.iter().map(|tool| tool.name.clone()).collect(),
            ));
            self.responses
                .pop_front()
                .unwrap_or(Err(OpenAiError::InvalidResponse))
        }
    }

    fn assistant(content: &str) -> ModelMessage {
        ModelMessage::text(ModelRole::Assistant, content)
    }

    fn tool_assistant(name: &str, arguments: &str) -> ModelMessage {
        ModelMessage {
            role: ModelRole::Assistant,
            content: None,
            tool_calls: vec![ModelToolCall {
                id: "tool-call-1".into(),
                kind: "function".into(),
                function: ModelToolFunction {
                    name: name.into(),
                    arguments: arguments.into(),
                },
            }],
            tool_call_id: None,
        }
    }

    impl SyncTransport for CaptureTransport {
        fn get_manifest(
            &mut self,
            _storage_id: &str,
        ) -> Result<Option<SyncManifest>, TransportError> {
            Ok(None)
        }

        fn get_part(
            &mut self,
            _storage_id: &str,
            _generation: &str,
            _part_index: usize,
        ) -> Result<SyncGenerationPart, TransportError> {
            Err(TransportError::new(Some(404), "not committed"))
        }

        fn put_part(
            &mut self,
            _storage_id: &str,
            _generation: &str,
            part: &SyncGenerationPart,
        ) -> Result<(), TransportError> {
            self.parts.push(part.clone());
            Ok(())
        }

        fn commit_manifest(
            &mut self,
            _storage_id: &str,
            generation: &str,
            part_count: usize,
            expected_version: u64,
        ) -> Result<SyncManifest, TransportError> {
            Ok(SyncManifest {
                generation: generation.into(),
                part_count,
                total_ciphertext_chars: self.parts.iter().map(|part| part.ciphertext.len()).sum(),
                parts_sha256: "0".repeat(64),
                version: expected_version + 1,
                updated_at_ms: 1,
            })
        }

        fn discard_generation(
            &mut self,
            _storage_id: &str,
            _generation: &str,
        ) -> Result<(), TransportError> {
            Ok(())
        }
    }

    #[test]
    fn note_lifecycle_persists_and_restores_content() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let note = workspace.create_note(None, "第一篇").unwrap();
        let id = note.base().id.clone();
        workspace.update_plain_note(&id, "本地正文").unwrap();
        assert_eq!(
            workspace.plain_note(&id).unwrap(),
            PlainNoteDocument {
                text: "本地正文".into(),
                editable: true,
                protected_blocks: 0,
            }
        );

        assert!(workspace.move_to_trash(&id).unwrap());
        assert!(workspace.list_home().unwrap().is_empty());
        assert_eq!(workspace.list_trash().unwrap()[0].id, id);
        assert!(workspace.restore(&id).unwrap());
        assert_eq!(workspace.plain_note(&id).unwrap().text, "本地正文");
    }

    #[test]
    fn rich_note_projects_common_blocks_without_losing_marks() {
        let content = vec![json!({
            "type": "h1",
            "children": [{"text": "Heading", "bold": true}]
        })];
        let document = read_plain_note("note-rich", &content);
        assert_eq!(document.text, "# **Heading**");
        assert!(document.editable);
        assert_eq!(document.protected_blocks, 0);
    }

    #[test]
    fn deterministic_block_id_matches_the_typescript_utf16_hash() {
        let block = json!({"type": "p", "children": [{"text": "中文😀"}]});
        assert_eq!(
            deterministic_block_id("note1", 0, &block),
            "blk_4fznf0_0_1bgepf0"
        );
    }

    #[test]
    fn native_rich_edit_preserves_unknown_blocks_and_updates_block_metadata() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let mut note = workspace.create_note(None, "rich").unwrap();
        let id = note.base().id.clone();
        let Node::Note { base, content } = &mut note else {
            unreachable!();
        };
        base.meta.clear();
        *content = vec![
            json!({
                "type": "h1",
                "children": [{"text": "Old", "bold": true}]
            }),
            json!({
                "type": "future-card",
                "payload": {"opaque": [1, 2, 3]},
                "children": [{"text": "preview"}]
            }),
            json!({"type": "p", "children": [{"text": "Body"}]}),
        ];
        workspace.database.put_node(&note).unwrap();

        let projection = workspace.plain_note(&id).unwrap();
        assert_eq!(projection.protected_blocks, 1);
        let edited = projection.text.replacen("Old", "New", 1);
        workspace.save_edits(&id, "rich", Some(&edited)).unwrap();

        let saved = workspace.node(&id).unwrap();
        let Node::Note { base, content } = saved else {
            unreachable!();
        };
        assert_eq!(content[0]["children"][0]["text"], "New");
        assert_eq!(content[0]["children"][0]["bold"], true);
        assert_eq!(content[1]["type"], "future-card");
        assert_eq!(content[1]["payload"], json!({"opaque": [1, 2, 3]}));
        assert_eq!(content[2]["children"][0]["text"], "Body");
        assert!(content.iter().all(|block| block["id"].is_string()));
        let metadata = base.meta["blockMeta"].as_object().unwrap();
        assert_eq!(metadata.len(), 3);
        for block in &content {
            assert!(metadata.contains_key(block["id"].as_str().unwrap()));
        }
    }

    #[test]
    fn tampered_protected_marker_cannot_partially_write_a_note() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let mut note = workspace.create_note(None, "before").unwrap();
        let id = note.base().id.clone();
        let Node::Note { content, .. } = &mut note else {
            unreachable!();
        };
        *content = vec![json!({"type": "future-card", "payload": "keep"})];
        workspace.database.put_node(&note).unwrap();

        let error = workspace
            .save_edits(&id, "after", Some("marker removed"))
            .unwrap_err();
        assert!(matches!(error, ApplicationError::InvalidNoteDocument(_)));
        let saved = workspace.node(&id).unwrap();
        assert_eq!(saved.base().title, "before");
        let Node::Note { content, .. } = saved else {
            unreachable!();
        };
        assert_eq!(
            content,
            vec![json!({"type": "future-card", "payload": "keep"})]
        );
    }

    #[test]
    fn creates_bookmarks_and_binary_files() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let bookmark = workspace
            .create_bookmark(None, "ideall", "https://example.com/path")
            .unwrap();
        assert_eq!(bookmark.kind(), NodeKind::Bookmark);
        assert!(
            workspace
                .create_bookmark(None, "bad", "javascript:alert(1)")
                .is_err()
        );
        let file = workspace
            .create_file(None, "hello.txt", "text/plain", b"hello".to_vec())
            .unwrap();
        assert_eq!(file.kind(), NodeKind::File);
        assert_eq!(workspace.list_home().unwrap().len(), 2);
    }

    #[test]
    fn flattens_nested_folders_in_preorder_with_depth() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let folder = workspace.create_folder(None, "folder").unwrap();
        let child = workspace
            .create_note(Some(&folder.base().id), "child")
            .unwrap();
        let sibling = workspace.create_note(None, "sibling").unwrap();

        let tree = workspace.list_home().unwrap();
        let folder_index = tree
            .iter()
            .position(|node| node.id == folder.base().id)
            .unwrap();
        let child_index = tree
            .iter()
            .position(|node| node.id == child.base().id)
            .unwrap();
        let sibling_index = tree
            .iter()
            .position(|node| node.id == sibling.base().id)
            .unwrap();

        assert_eq!(tree[folder_index].depth, 0);
        assert_eq!(tree[child_index].depth, 1);
        assert_eq!(tree[sibling_index].depth, 0);
        assert_eq!(child_index, folder_index + 1);
    }

    #[test]
    fn rename_validates_title_before_persisting() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let note = workspace.create_note(None, "before").unwrap();
        let id = note.base().id.clone();

        assert!(matches!(
            workspace.rename(&id, "   "),
            Err(ApplicationError::InvalidNodeTitle)
        ));
        assert!(matches!(
            workspace.save_edits(&id, "", Some("must not persist")),
            Err(ApplicationError::InvalidNodeTitle)
        ));
        assert_eq!(workspace.node(&id).unwrap().base().title, "before");
        assert_eq!(workspace.plain_note(&id).unwrap().text, "");

        workspace.rename(&id, "after.md").unwrap();
        assert_eq!(workspace.node(&id).unwrap().base().title, "after.md");
    }

    #[test]
    fn move_node_changes_parent_and_rejects_cycles_or_file_parents() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let source = workspace.create_folder(None, "source").unwrap();
        let nested = workspace
            .create_folder(Some(&source.base().id), "nested")
            .unwrap();
        let target = workspace.create_folder(None, "target").unwrap();
        let note = workspace
            .create_note(Some(&source.base().id), "note")
            .unwrap();

        workspace
            .move_node(&note.base().id, Some(&target.base().id))
            .unwrap();
        assert_eq!(
            workspace.node(&note.base().id).unwrap().base().parent_id,
            Some(target.base().id.clone())
        );
        assert!(matches!(
            workspace.move_node(&source.base().id, Some(&nested.base().id)),
            Err(ApplicationError::InvalidNodeMove)
        ));
        assert!(matches!(
            workspace.move_node(&target.base().id, Some(&note.base().id)),
            Err(ApplicationError::WrongKind { .. })
        ));

        workspace.move_node(&note.base().id, None).unwrap();
        assert_eq!(
            workspace
                .node(&note.base().id)
                .unwrap()
                .base()
                .parent_id
                .as_deref(),
            Some(HOME_ROOT_ID)
        );
    }

    #[test]
    fn protects_nonempty_folders_from_trash() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let folder = workspace.create_folder(None, "folder").unwrap();
        workspace
            .create_note(Some(&folder.base().id), "child")
            .unwrap();

        assert!(matches!(
            workspace.move_to_trash(&folder.base().id),
            Err(ApplicationError::FolderNotEmpty(_))
        ));
    }

    #[test]
    fn bookmark_update_validates_before_persisting() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let bookmark = workspace
            .create_bookmark(None, "safe", "https://example.com")
            .unwrap();
        let id = bookmark.base().id.clone();

        assert!(matches!(
            workspace.update_bookmark_url(&id, "javascript:alert(1)"),
            Err(ApplicationError::InvalidBookmarkUrl)
        ));
        let Node::Bookmark { content, .. } = workspace.node(&id).unwrap() else {
            panic!("bookmark changed kind");
        };
        assert_eq!(content.url, "https://example.com/");

        assert!(
            workspace
                .save_edits(&id, "changed", Some("data:bad"))
                .is_err()
        );
        let saved = workspace.node(&id).unwrap();
        assert_eq!(saved.base().title, "safe");
    }

    #[test]
    fn workspace_state_round_trips_through_sqlite() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let mut state = WorkspaceState::default();
        state.open(ideall_domain::TabDescriptor {
            file: ideall_protocol::FileRef::new("local.nodes", "n1"),
            engine_id: "ideall.note".into(),
            title: "Note".into(),
            root_id: Some(HOME_ROOT_ID.into()),
            navigation_path: Some("/home/n1".into()),
        });
        state.explorer = ideall_domain::ExplorerLayoutState {
            width: 344,
            expanded_folders: vec!["folder-b".into(), "folder-a".into()],
            visible: false,
        };

        workspace.save_workspace_state(&state).unwrap();
        assert_eq!(workspace.load_workspace_state().unwrap(), state);
    }

    #[test]
    fn searches_titles_and_note_content_without_returning_trash() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let first = workspace.create_note(None, "会议记录").unwrap();
        workspace
            .update_plain_note(&first.base().id, "讨论离线优先架构")
            .unwrap();
        let second = workspace.create_note(None, "离线草稿").unwrap();

        let hits = workspace.search("离线", 20).unwrap();
        assert_eq!(hits.len(), 2);
        workspace.move_to_trash(&second.base().id).unwrap();
        assert_eq!(workspace.search("离线", 20).unwrap().len(), 1);
    }

    #[test]
    fn projects_nodes_to_stable_file_metadata() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let note = workspace.create_note(None, "Note").unwrap();
        let metadata = workspace.file_metadata(&note.base().id).unwrap();
        assert_eq!(metadata.r#ref, FileRef::new("local.nodes", &note.base().id));
        assert_eq!(metadata.media_type, "application/vnd.ideall.note+json");
        assert!(metadata.capabilities.contains(&"write".into()));
    }

    #[test]
    fn engine_preferences_round_trip_without_entering_workspace_tabs() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let mut preferences = EnginePreferences::default();
        preferences
            .media_types
            .insert("text/plain".into(), "ideall.code".into());
        workspace.save_engine_preferences(&preferences).unwrap();
        assert_eq!(workspace.load_engine_preferences().unwrap(), preferences);
        assert_eq!(
            workspace.load_workspace_state().unwrap(),
            WorkspaceState::default()
        );
    }

    #[test]
    fn sync_settings_store_only_a_canonical_non_secret_server_base() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        assert_eq!(
            workspace.load_sync_settings().unwrap(),
            SyncSettings::default()
        );
        let saved = workspace
            .save_sync_settings(&SyncSettings {
                server_base_url: " https://example.com/deployment/ ".into(),
            })
            .unwrap();
        assert_eq!(saved.server_base_url, "https://example.com/deployment");
        assert_eq!(workspace.load_sync_settings().unwrap(), saved);
        assert!(matches!(
            workspace.save_sync_settings(&SyncSettings {
                server_base_url: "https://token@example.com/?secret=1".into(),
            }),
            Err(ApplicationError::InvalidSyncServer)
        ));
    }

    #[test]
    fn agent_model_settings_store_only_canonical_public_coordinates() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        assert_eq!(
            workspace.load_agent_model_settings().unwrap(),
            AgentModelSettings::default()
        );
        let saved = workspace
            .save_agent_model_settings(&AgentModelSettings {
                base_url: " https://example.com/compatible/v1 ".into(),
                model: " custom-model ".into(),
            })
            .unwrap();
        assert_eq!(saved.base_url, "https://example.com/compatible/v1/");
        assert_eq!(saved.model, "custom-model");
        assert_eq!(workspace.load_agent_model_settings().unwrap(), saved);
        assert!(matches!(
            workspace.save_agent_model_settings(&AgentModelSettings {
                base_url: "https://secret@example.com/v1".into(),
                model: "m".into(),
            }),
            Err(ApplicationError::AgentConfig(_))
        ));
    }

    #[test]
    fn external_acp_settings_and_redacted_events_round_trip_locally() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let cwd = std::env::current_dir().unwrap();
        let saved = workspace
            .save_external_acp_settings(&ExternalAcpSettings {
                program: " node ".into(),
                args: " script.mjs --acp ".into(),
                cwd: format!(" {} ", cwd.display()),
            })
            .unwrap();
        assert_eq!(saved.program, "node");
        assert_eq!(saved.args, "script.mjs --acp");
        assert_eq!(workspace.load_external_acp_settings().unwrap(), saved);

        let thread_id = workspace
            .begin_external_agent_turn(None, "运行外部 Agent")
            .unwrap();
        workspace
            .complete_external_agent_turn(
                &thread_id,
                "已完成",
                &[AgentToolRun {
                    name: "external-acp.execute".into(),
                    ok: false,
                    summary: "权限请求已拒绝".into(),
                }],
            )
            .unwrap();
        assert_eq!(
            workspace.agent_transcript(&thread_id).unwrap(),
            vec![
                AgentTranscriptMessage {
                    role: ModelRole::User,
                    content: "运行外部 Agent".into(),
                },
                AgentTranscriptMessage {
                    role: ModelRole::Tool,
                    content: "external-acp.execute · 未完成 · 权限请求已拒绝".into(),
                },
                AgentTranscriptMessage {
                    role: ModelRole::Assistant,
                    content: "已完成".into(),
                },
            ]
        );
        let Node::Thread { content, .. } = workspace.node(&thread_id).unwrap() else {
            panic!("expected thread");
        };
        assert!(content.messages[1].get("rawInput").is_none());
        assert!(content.messages[1].get("rawOutput").is_none());
    }

    #[test]
    fn agent_turn_runs_mcp_tools_and_persists_a_replayable_thread() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let mut provider = ScriptedProvider::new([
            Ok(tool_assistant("fs.create-note", r#"{"title":"模型草稿"}"#)),
            Ok(assistant("已创建模型草稿。")),
        ]);

        let result = workspace
            .run_agent_turn(&mut provider, None, "请创建一条草稿")
            .unwrap();
        assert_eq!(result.content, "已创建模型草稿。");
        assert_eq!(result.tools.len(), 1);
        assert_eq!(result.tools[0].name, "fs.create-note");
        assert!(result.tools[0].ok);
        assert_eq!(provider.requests.len(), 2);
        assert_eq!(
            provider.requests[0].1,
            vec!["fs.list".to_owned(), "fs.create-note".to_owned()]
        );
        assert_eq!(provider.requests[1].0.last().unwrap().role, ModelRole::Tool);

        let threads = workspace.list_agent_threads(20).unwrap();
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].id, result.thread_id);
        let transcript = workspace.agent_transcript(&result.thread_id).unwrap();
        assert_eq!(transcript.first().unwrap().role, ModelRole::User);
        assert_eq!(transcript.last().unwrap().content, "已创建模型草稿。");
        assert!(
            workspace
                .list_home()
                .unwrap()
                .iter()
                .any(|node| node.kind == NodeKind::Note && node.title == "模型草稿")
        );
        assert_eq!(workspace.list_agent_audits(10).unwrap().len(), 1);
    }

    #[test]
    fn agent_network_failure_keeps_the_user_message_in_a_durable_thread() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let mut provider = ScriptedProvider::new([Err(OpenAiError::Network)]);
        assert!(matches!(
            workspace.run_agent_turn(&mut provider, None, "不要丢失这条消息"),
            Err(ApplicationError::AgentModel(OpenAiError::Network))
        ));
        let thread = workspace.list_agent_threads(1).unwrap().pop().unwrap();
        assert_eq!(
            workspace.agent_transcript(&thread.id).unwrap(),
            vec![AgentTranscriptMessage {
                role: ModelRole::User,
                content: "不要丢失这条消息".into(),
            }]
        );
    }

    #[test]
    fn malformed_model_tool_arguments_are_reported_without_execution() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let mut provider = ScriptedProvider::new([
            Ok(tool_assistant("fs.create-note", "not-json")),
            Ok(assistant("参数无效，未创建笔记。")),
        ]);
        let result = workspace
            .run_agent_turn(&mut provider, None, "测试无效参数")
            .unwrap();
        assert_eq!(result.tools.len(), 1);
        assert!(!result.tools[0].ok);
        assert_eq!(workspace.list_agent_audits(10).unwrap().len(), 0);
        assert!(
            !workspace
                .list_home()
                .unwrap()
                .iter()
                .any(|node| node.kind == NodeKind::Note)
        );
    }

    #[test]
    fn text_files_are_editable_but_binary_files_are_protected() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let text = workspace
            .create_file(None, "hello.txt", "text/plain", b"hello".to_vec())
            .unwrap();
        workspace.update_text_file(&text.base().id, "你好").unwrap();
        assert_eq!(workspace.text_file(&text.base().id).unwrap().text, "你好");

        let binary = workspace
            .create_file(None, "image.png", "image/png", vec![0, 1, 2])
            .unwrap();
        assert!(matches!(
            workspace.text_file(&binary.base().id),
            Err(ApplicationError::BinaryFileReadOnly(_))
        ));
    }

    #[test]
    fn note_sync_uploads_the_legacy_wire_without_persisting_the_code() {
        let code = "00112233445566778899aabbccddeeff";
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let note = workspace.create_note(None, "跨端笔记").unwrap();
        workspace
            .update_plain_note(&note.base().id, "encrypted body")
            .unwrap();
        let mut transport = CaptureTransport::default();

        let result = workspace.sync_notes(code, &mut transport).unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(transport.parts.len(), 1);
        let keys = derive_keys(code, SyncScope::Notes, 0).unwrap();
        let plaintext = keys
            .decrypt_bytes(
                &Encrypted {
                    iv: transport.parts[0].iv.clone(),
                    ciphertext: transport.parts[0].ciphertext.clone(),
                },
                None,
            )
            .unwrap();
        let value: Value = serde_json::from_slice(&plaintext).unwrap();
        assert_eq!(value[0]["title"], "跨端笔记");
        assert!(value[0].get("kind").is_none());
        assert_eq!(value[0]["parentId"], Value::Null);
        assert!(
            workspace
                .database
                .setting::<String>("sync-code")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn subscription_sync_uploads_the_exact_legacy_wire() {
        let code = "00112233445566778899aabbccddeeff";
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let timestamp = now_millis().unwrap();
        workspace
            .database
            .put_node(&Node::Feed {
                base: BaseNode {
                    id: "publisher:example.com".into(),
                    parent_id: Some(HOME_ROOT_ID.into()),
                    sort_key: "local-only-sort".into(),
                    title: "Example".into(),
                    tags: vec!["local-only".into()],
                    created_at: timestamp,
                    updated_at: timestamp,
                    deleted_at: None,
                    meta: BTreeMap::from([("local".into(), Value::Bool(true))]),
                },
                content: FeedContent {
                    r#type: SubscriptionType::Publisher,
                    key: "example.com".into(),
                    favicon: "https://example.com/favicon.ico".into(),
                    entity_label: None,
                    entity_name: None,
                    search_keyword: None,
                    search_domain: None,
                },
            })
            .unwrap();
        let mut transport = CaptureTransport::default();
        workspace.sync_subscriptions(code, &mut transport).unwrap();

        let keys = derive_keys(code, SyncScope::Subscriptions, 0).unwrap();
        let plaintext = keys
            .decrypt_bytes(
                &Encrypted {
                    iv: transport.parts[0].iv.clone(),
                    ciphertext: transport.parts[0].ciphertext.clone(),
                },
                None,
            )
            .unwrap();
        let value: Value = serde_json::from_slice(&plaintext).unwrap();
        assert_eq!(value[0]["id"], "publisher:example.com");
        assert_eq!(value[0]["type"], "publisher");
        assert!(value[0].get("kind").is_none());
        assert!(value[0].get("sortKey").is_none());
        assert!(value[0].get("meta").is_none());
    }

    #[test]
    fn creates_idempotent_feeds_with_legacy_canonical_identity() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let first = workspace
            .create_feed("Example", SubscriptionType::Publisher, "example.com")
            .unwrap();
        let second = workspace
            .create_feed(
                "Ignored duplicate",
                SubscriptionType::Publisher,
                "example.com",
            )
            .unwrap();
        assert_eq!(first.base().id, "publisher:example.com");
        assert_eq!(second, first);
        assert_eq!(workspace.list_home().unwrap().len(), 1);
        assert!(matches!(
            workspace.create_feed("Unsafe tool", SubscriptionType::Tool, "javascript:alert(1)"),
            Err(ApplicationError::InvalidBookmarkUrl)
        ));
    }

    #[test]
    fn bookmark_sync_excludes_home_and_normalizes_nested_folders() {
        let code = "00112233445566778899aabbccddeeff";
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let root_folder = workspace.create_folder(None, "收藏夹").unwrap();
        let nested = workspace
            .create_folder(Some(&root_folder.base().id), "嵌套")
            .unwrap();
        let bookmark = workspace
            .create_bookmark(Some(&nested.base().id), "Example", "https://example.com")
            .unwrap();
        let mut transport = CaptureTransport::default();
        workspace.sync_bookmarks(code, &mut transport).unwrap();

        let keys = derive_keys(code, SyncScope::Bookmarks, 0).unwrap();
        let plaintext = keys
            .decrypt_bytes(
                &Encrypted {
                    iv: transport.parts[0].iv.clone(),
                    ciphertext: transport.parts[0].ciphertext.clone(),
                },
                None,
            )
            .unwrap();
        let value: Value = serde_json::from_slice(&plaintext).unwrap();
        let records = value.as_array().unwrap();
        assert!(!records.iter().any(|node| node["id"] == HOME_ROOT_ID));
        assert!(
            records
                .iter()
                .filter(|node| node["kind"] == "folder")
                .all(|node| node["parentId"].is_null())
        );
        assert_eq!(
            records
                .iter()
                .find(|node| node["id"] == bookmark.base().id)
                .unwrap()["parentId"],
            nested.base().id
        );
        assert_eq!(
            workspace
                .node(&nested.base().id)
                .unwrap()
                .base()
                .parent_id
                .as_deref(),
            Some(HOME_ROOT_ID)
        );
    }

    #[test]
    fn agent_audit_lifecycle_is_durable_and_one_way() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let pending = workspace
            .append_agent_audit(AgentAuditInput {
                source: ideall_agent::AuditSource::Tool,
                operation: "fs.write".into(),
                title: "更新笔记".into(),
                summary: "仅记录脱敏摘要".into(),
                status: ideall_agent::AuditStatus::Pending,
                effect: ideall_agent::ToolEffect::Write,
                risk: ideall_agent::ToolRisk::Medium,
                target: None,
                thread_id: None,
                message_id: None,
            })
            .unwrap();
        assert_eq!(
            workspace.list_agent_audits(10).unwrap(),
            vec![pending.clone()]
        );

        let completed = workspace
            .complete_agent_audit(AgentAuditCompletion {
                id: pending.id.clone(),
                status: ideall_agent::AuditStatus::Committed,
                summary: "已写入".into(),
            })
            .unwrap();
        assert_eq!(completed.status, ideall_agent::AuditStatus::Committed);
        assert!(matches!(
            workspace.complete_agent_audit(AgentAuditCompletion {
                id: pending.id,
                status: ideall_agent::AuditStatus::Failed,
                summary: "迟到回执".into()
            }),
            Err(ApplicationError::Audit(AuditError::AlreadyFinalized))
        ));
    }

    #[test]
    fn local_agent_mcp_exposes_only_default_content_safe_tools() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let response = workspace.handle_local_agent_mcp(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list"
        }));
        let names = response["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["fs.list", "fs.create-note"]);
        assert!(!names.iter().any(|name| name.contains("read-note")));
        assert!(!names.iter().any(|name| name.contains("blob")));
    }

    #[test]
    fn agent_note_creation_uses_mcp_and_commits_redacted_audit() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let node = workspace.create_agent_note_via_mcp("Agent 草稿").unwrap();
        assert_eq!(node.kind(), NodeKind::Note);
        assert_eq!(node.base().title, "Agent 草稿");
        assert_eq!(node.base().parent_id.as_deref(), Some(HOME_ROOT_ID));
        assert!(
            workspace
                .plain_note(&node.base().id)
                .unwrap()
                .text
                .is_empty()
        );

        let audits = workspace.list_agent_audits(10).unwrap();
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].operation, "fs.create-note");
        assert_eq!(audits[0].status, AuditStatus::Committed);
        assert_eq!(audits[0].summary, "已创建 1 条本地笔记");
        assert_eq!(audits[0].target.as_ref().unwrap().label, "新笔记");
        assert!(!audits[0].summary.contains("Agent 草稿"));
    }

    #[test]
    fn rejected_agent_arguments_do_not_create_an_audit_or_node() {
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let response = workspace.handle_local_agent_mcp(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {"name": "fs.create-note", "arguments": {"title": "\n"}}
        }));
        assert_eq!(response["result"]["isError"], true);
        assert!(workspace.list_agent_audits(10).unwrap().is_empty());
        assert!(workspace.list_home().unwrap().is_empty());
    }

    #[test]
    fn imports_percent_encoded_mobile_file_urls_with_a_safe_display_name() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("外部 文档.md");
        std::fs::write(&path, b"# mobile").unwrap();
        let file_url = Url::from_file_path(&path).unwrap().to_string();
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let node = workspace
            .import_external_file(None, &file_url, "../选择的文档.md", 1024)
            .unwrap();
        assert_eq!(node.base().title, "选择的文档.md");
        let Node::File { blob_ref, .. } = node else {
            panic!("expected imported file");
        };
        assert_eq!(blob_ref.mime, "text/markdown");
    }

    #[test]
    fn exports_internal_blobs_for_platform_viewers() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("exported.bin");
        let mut workspace = LocalWorkspace::open_in_memory().unwrap();
        let node = workspace
            .create_file(
                None,
                "source.bin",
                "application/octet-stream",
                vec![0, 1, 2],
            )
            .unwrap();
        assert_eq!(workspace.export_file(&node.base().id, &target).unwrap(), 3);
        assert_eq!(std::fs::read(target).unwrap(), vec![0, 1, 2]);
    }
}
