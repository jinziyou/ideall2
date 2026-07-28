//! UI- and transport-independent Agent authority boundary.
//!
//! MCP and ACP adapters must turn every operation into an [`Intent`] and pass
//! it through a live [`Grant`]. Tool registration may additionally omit tools
//! that are not effective for the grant, but omission is not a substitute for
//! this execution-time check.

mod mcp;
mod model;

pub use mcp::{LocalMcpServer, McpTool, ToolHandler};
pub use model::{
    CompletionProvider, ModelMessage, ModelRole, ModelTool, ModelToolCall, ModelToolFunction,
    OpenAiCompatibleClient, OpenAiConfigError, OpenAiError, canonical_base_url,
    validate_model_name,
};

use std::collections::BTreeSet;

use ideall_protocol::NodeKind;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

const MAX_AUDIT_TEXT_CHARS: usize = 240;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum Permission {
    #[serde(rename = "identity:read")]
    IdentityRead,
    #[serde(rename = "identity.publish")]
    IdentityPublish,
    #[serde(rename = "hub.subscriptions:read")]
    HubSubscriptionsRead,
    #[serde(rename = "hub.subscriptions:write")]
    HubSubscriptionsWrite,
    #[serde(rename = "hub.bookmarks:read")]
    HubBookmarksRead,
    #[serde(rename = "hub.bookmarks:write")]
    HubBookmarksWrite,
    #[serde(rename = "host.external")]
    HostExternal,
    #[serde(rename = "host.nav")]
    HostNavigation,
    #[serde(rename = "fs:read")]
    FilesRead,
    #[serde(rename = "fs:write")]
    FilesWrite,
    #[serde(rename = "fs.notes:read")]
    NotesRead,
    #[serde(rename = "fs.notes:write")]
    NotesWrite,
    #[serde(rename = "fs.blobs:read")]
    BlobsRead,
    #[serde(rename = "agent.config:read")]
    AgentConfigRead,
    #[serde(rename = "ui.tabs")]
    UiTabs,
    #[serde(rename = "web:search")]
    WebSearch,
    #[serde(rename = "web:fetch")]
    WebFetch,
    #[serde(rename = "browser:read")]
    BrowserRead,
    #[serde(rename = "browser:control")]
    BrowserControl,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GrantTier {
    FirstParty,
    Verified,
    AnyOrigin,
}

impl GrantTier {
    fn rank(self) -> u8 {
        match self {
            Self::FirstParty => 2,
            Self::Verified => 1,
            Self::AnyOrigin => 0,
        }
    }

    fn allows(self, required: Self) -> bool {
        self.rank() >= required.rank()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Grant {
    pub consumer_id: String,
    pub origin: String,
    pub tier: GrantTier,
    pub permissions: BTreeSet<Permission>,
    pub granted_at: i64,
    pub expiry: Option<i64>,
    pub revocable: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Intent {
    ListFiles,
    ReadMetadata(NodeKind),
    ReadContent(NodeKind),
    ReadBlob,
    Create(NodeKind),
    Write(NodeKind),
    Delete(NodeKind),
    ReadAgentConfig,
    OpenTab,
    NavigateHost,
    OpenExternal,
    SearchWeb,
    FetchWeb,
    ReadBrowser,
    ControlBrowser,
    PublishIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum AuthorizationError {
    #[error("grant consumer does not match the authenticated consumer")]
    ConsumerMismatch,
    #[error("grant origin does not match the authenticated origin")]
    OriginMismatch,
    #[error("grant is not active")]
    Inactive,
    #[error("grant does not provide effective permission `{0:?}`")]
    MissingPermission(Permission),
}

impl Grant {
    pub fn is_active(&self, now: i64) -> bool {
        self.expiry.is_none_or(|expiry| now < expiry)
    }

    pub fn effective_permissions(&self) -> BTreeSet<Permission> {
        self.permissions
            .iter()
            .copied()
            .filter(|permission| self.tier.allows(minimum_tier(*permission)))
            .collect()
    }

    pub fn authorize(
        &self,
        consumer_id: &str,
        origin: &str,
        intent: Intent,
        now: i64,
    ) -> Result<(), AuthorizationError> {
        if self.consumer_id != consumer_id {
            return Err(AuthorizationError::ConsumerMismatch);
        }
        if self.origin != origin {
            return Err(AuthorizationError::OriginMismatch);
        }
        if !self.is_active(now) {
            return Err(AuthorizationError::Inactive);
        }
        let effective = self.effective_permissions();
        for permission in permissions_for_intent(intent) {
            if !effective.contains(permission) {
                return Err(AuthorizationError::MissingPermission(*permission));
            }
        }
        Ok(())
    }
}

pub fn agent_grant(now: i64, requested: Option<&BTreeSet<Permission>>) -> Grant {
    let defaults = default_agent_permissions();
    let configurable = configurable_agent_permissions();
    let permissions = requested.map_or(defaults, |requested| {
        requested.intersection(&configurable).copied().collect()
    });
    Grant {
        consumer_id: "ideall-agent".into(),
        origin: "loopback".into(),
        tier: GrantTier::FirstParty,
        permissions,
        granted_at: now,
        expiry: None,
        revocable: false,
    }
}

pub fn default_agent_permissions() -> BTreeSet<Permission> {
    [
        Permission::FilesRead,
        Permission::FilesWrite,
        Permission::NotesWrite,
        Permission::UiTabs,
        Permission::WebSearch,
        Permission::WebFetch,
        Permission::BrowserRead,
        Permission::BrowserControl,
    ]
    .into_iter()
    .collect()
}

pub fn configurable_agent_permissions() -> BTreeSet<Permission> {
    default_agent_permissions()
        .into_iter()
        .chain([Permission::AgentConfigRead])
        .collect()
}

fn minimum_tier(permission: Permission) -> GrantTier {
    match permission {
        Permission::FilesWrite
        | Permission::NotesRead
        | Permission::NotesWrite
        | Permission::BlobsRead
        | Permission::AgentConfigRead
        | Permission::IdentityPublish
        | Permission::WebSearch
        | Permission::WebFetch
        | Permission::BrowserRead
        | Permission::BrowserControl => GrantTier::FirstParty,
        _ => GrantTier::AnyOrigin,
    }
}

fn permissions_for_intent(intent: Intent) -> &'static [Permission] {
    match intent {
        Intent::ListFiles | Intent::ReadMetadata(_) => &[Permission::FilesRead],
        Intent::ReadContent(NodeKind::Note) => &[Permission::FilesRead, Permission::NotesRead],
        Intent::ReadContent(_) => &[Permission::FilesRead],
        Intent::ReadBlob => &[Permission::FilesRead, Permission::BlobsRead],
        Intent::Create(NodeKind::Note)
        | Intent::Write(NodeKind::Note)
        | Intent::Delete(NodeKind::Note) => &[Permission::FilesWrite, Permission::NotesWrite],
        Intent::Create(_) | Intent::Write(_) | Intent::Delete(_) => &[Permission::FilesWrite],
        Intent::ReadAgentConfig => &[Permission::AgentConfigRead],
        Intent::OpenTab => &[Permission::UiTabs],
        Intent::NavigateHost => &[Permission::HostNavigation],
        Intent::OpenExternal => &[Permission::HostExternal],
        Intent::SearchWeb => &[Permission::WebSearch],
        Intent::FetchWeb => &[Permission::WebFetch],
        Intent::ReadBrowser => &[Permission::BrowserRead],
        Intent::ControlBrowser => &[Permission::BrowserControl],
        Intent::PublishIdentity => &[Permission::IdentityPublish],
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditSource {
    Artifact,
    Tool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditStatus {
    Pending,
    Committed,
    Failed,
    Rejected,
    Undone,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolEffect {
    Read,
    Write,
    Delete,
    Navigation,
    External,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolRisk {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditTarget {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAuditRecord {
    pub id: String,
    pub version: u8,
    pub source: AuditSource,
    pub operation: String,
    pub title: String,
    pub summary: String,
    pub status: AuditStatus,
    pub effect: ToolEffect,
    pub risk: ToolRisk,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<AuditTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentAuditInput {
    pub source: AuditSource,
    pub operation: String,
    pub title: String,
    pub summary: String,
    pub status: AuditStatus,
    pub effect: ToolEffect,
    pub risk: ToolRisk,
    pub target: Option<AuditTarget>,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentAuditCompletion {
    pub id: String,
    pub status: AuditStatus,
    pub summary: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum AuditError {
    #[error("only tool audit records may start pending")]
    InvalidPendingSource,
    #[error("audit completion must be committed or failed")]
    InvalidCompletion,
    #[error("audit intent id does not match")]
    IdMismatch,
    #[error("audit intent is already finalized")]
    AlreadyFinalized,
}

pub fn new_audit(input: AgentAuditInput, now: i64) -> Result<AgentAuditRecord, AuditError> {
    if input.status == AuditStatus::Pending && input.source != AuditSource::Tool {
        return Err(AuditError::InvalidPendingSource);
    }
    Ok(AgentAuditRecord {
        id: format!("audit-{}", Uuid::new_v4()),
        version: 1,
        source: input.source,
        operation: bounded(input.operation, "unknown"),
        title: bounded(input.title, "Agent 写操作"),
        summary: bounded(input.summary, "已记录写操作结果"),
        status: input.status,
        effect: input.effect,
        risk: input.risk,
        target: input.target.map(|target| AuditTarget {
            kind: target.kind.map(|value| bounded(value, "unknown")),
            id: target.id.map(|value| bounded(value, "unknown")),
            label: bounded(target.label, "未命名目标"),
        }),
        thread_id: input.thread_id.map(|value| bounded(value, "unknown")),
        message_id: input.message_id.map(|value| bounded(value, "unknown")),
        created_at: now,
        updated_at: now,
    })
}

pub fn complete_audit(
    current: &AgentAuditRecord,
    completion: AgentAuditCompletion,
    now: i64,
) -> Result<AgentAuditRecord, AuditError> {
    if completion.id != current.id {
        return Err(AuditError::IdMismatch);
    }
    if !matches!(
        completion.status,
        AuditStatus::Committed | AuditStatus::Failed
    ) {
        return Err(AuditError::InvalidCompletion);
    }
    if current.source != AuditSource::Tool || current.status != AuditStatus::Pending {
        return Err(AuditError::AlreadyFinalized);
    }
    let mut completed = current.clone();
    completed.status = completion.status;
    completed.summary = bounded(completion.summary, "已记录工具执行结果");
    completed.updated_at = now;
    Ok(completed)
}

fn bounded(value: String, fallback: &str) -> String {
    let normalized = value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let normalized = normalized.trim();
    if normalized.is_empty() {
        return fallback.into();
    }
    if normalized.chars().count() <= MAX_AUDIT_TEXT_CHARS {
        return normalized.into();
    }
    normalized
        .chars()
        .take(MAX_AUDIT_TEXT_CHARS - 1)
        .chain(['…'])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_agent_cannot_read_existing_note_content_or_blobs() {
        let grant = agent_grant(10, None);
        assert!(
            grant
                .authorize("ideall-agent", "loopback", Intent::ListFiles, 11)
                .is_ok()
        );
        assert!(matches!(
            grant.authorize(
                "ideall-agent",
                "loopback",
                Intent::ReadContent(NodeKind::Note),
                11
            ),
            Err(AuthorizationError::MissingPermission(Permission::NotesRead))
        ));
        assert!(matches!(
            grant.authorize("ideall-agent", "loopback", Intent::ReadBlob, 11),
            Err(AuthorizationError::MissingPermission(Permission::BlobsRead))
        ));
    }

    #[test]
    fn low_trust_grant_cannot_smuggle_sensitive_permissions() {
        let grant = Grant {
            consumer_id: "embed".into(),
            origin: "https://example.com".into(),
            tier: GrantTier::Verified,
            permissions: [Permission::FilesRead, Permission::FilesWrite]
                .into_iter()
                .collect(),
            granted_at: 1,
            expiry: Some(100),
            revocable: true,
        };
        assert_eq!(
            grant.effective_permissions(),
            [Permission::FilesRead].into_iter().collect()
        );
        assert!(matches!(
            grant.authorize(
                "embed",
                "https://example.com",
                Intent::Write(NodeKind::Bookmark),
                2
            ),
            Err(AuthorizationError::MissingPermission(
                Permission::FilesWrite
            ))
        ));
    }

    #[test]
    fn authorization_binds_consumer_origin_and_expiry() {
        let mut grant = agent_grant(1, None);
        grant.expiry = Some(5);
        assert!(matches!(
            grant.authorize("other", "loopback", Intent::ListFiles, 2),
            Err(AuthorizationError::ConsumerMismatch)
        ));
        assert!(matches!(
            grant.authorize("ideall-agent", "https://evil.invalid", Intent::ListFiles, 2),
            Err(AuthorizationError::OriginMismatch)
        ));
        assert!(matches!(
            grant.authorize("ideall-agent", "loopback", Intent::ListFiles, 5),
            Err(AuthorizationError::Inactive)
        ));
    }

    #[test]
    fn audit_is_bounded_redacted_shape_and_settles_only_once() {
        let pending = new_audit(
            AgentAuditInput {
                source: AuditSource::Tool,
                operation: "fs.write\0".repeat(100),
                title: "Write".into(),
                summary: "正文和密钥不应进入这里".into(),
                status: AuditStatus::Pending,
                effect: ToolEffect::Write,
                risk: ToolRisk::Medium,
                target: Some(AuditTarget {
                    kind: Some("note".into()),
                    id: Some("n1".into()),
                    label: "会议纪要".into(),
                }),
                thread_id: None,
                message_id: None,
            },
            10,
        )
        .unwrap();
        assert_eq!(pending.operation.chars().count(), MAX_AUDIT_TEXT_CHARS);
        assert!(!pending.operation.contains('\0'));
        let completed = complete_audit(
            &pending,
            AgentAuditCompletion {
                id: pending.id.clone(),
                status: AuditStatus::Committed,
                summary: "已保存".into(),
            },
            11,
        )
        .unwrap();
        assert_eq!(completed.status, AuditStatus::Committed);
        assert!(matches!(
            complete_audit(
                &completed,
                AgentAuditCompletion {
                    id: completed.id.clone(),
                    status: AuditStatus::Failed,
                    summary: "late".into()
                },
                12
            ),
            Err(AuditError::AlreadyFinalized)
        ));
    }
}
