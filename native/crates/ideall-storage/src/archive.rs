use aes_gcm::{
    Aes256Gcm, KeyInit,
    aead::{Aead, Payload},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ideall_protocol::Node;
use pbkdf2::pbkdf2_hmac;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::Sha256;
use thiserror::Error;

const ARCHIVE_KIND: &str = "ideall.workspace-archive";
const ARCHIVE_VERSION: u64 = 2;
const ENCRYPTED_KIND: &str = "ideall.workspace-archive-encrypted";
const ENCRYPTED_VERSION: u64 = 1;
const WORKSPACE_BACKUP_KIND: &str = "ideall.workspace-backup";
const WORKSPACE_BACKUP_VERSION: u64 = 1;
const PLUGIN_DATA_KIND: &str = "ideall.plugin-data";
const PLUGIN_DATA_VERSION: u64 = 1;
const PBKDF2_ITERATIONS: u32 = 600_000;
const SALT_BYTES: usize = 16;
const IV_BYTES: usize = 12;
const AES_GCM_TAG_BYTES: usize = 16;
const MIB: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArchiveLimits {
    pub max_plaintext_bytes: usize,
    pub max_envelope_bytes: usize,
    pub max_nodes: usize,
    pub max_blobs: usize,
    pub max_single_blob_bytes: usize,
    pub max_total_blob_bytes: usize,
    pub max_trash_snapshots: usize,
    pub max_plugins: usize,
    pub max_tabs: usize,
}

impl Default for ArchiveLimits {
    fn default() -> Self {
        Self {
            max_plaintext_bytes: 256 * MIB,
            max_envelope_bytes: 352 * MIB,
            max_nodes: 250_000,
            max_blobs: 25_000,
            max_single_blob_bytes: 64 * MIB,
            max_total_blob_bytes: 160 * MIB,
            max_trash_snapshots: 100_000,
            max_plugins: 128,
            max_tabs: 1_000,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveManifest {
    pub checksum: String,
    pub node_count: usize,
    pub blob_count: usize,
    pub blob_bytes: usize,
    pub trash_snapshot_count: usize,
    pub plugin_count: usize,
    pub tab_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveBlob {
    pub key: String,
    pub mime: String,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ArchiveTrashSnapshot {
    pub id: String,
    pub node: Node,
    pub blob: Option<ArchiveBlob>,
    pub captured_at: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveWorkspace {
    pub tabs: Vec<Value>,
    pub active_id: Option<String>,
    pub transient_id: Option<String>,
    pub active_module: String,
    pub workspace_kind: String,
    pub development_tool: String,
    pub sidebar_collapsed: bool,
    pub right_panel_open: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchivePluginDescriptor {
    pub id: String,
    pub label: String,
    pub data_kind: String,
    pub data_version: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ArchivePluginPackage {
    pub kind: String,
    pub version: u64,
    pub plugin: ArchivePluginDescriptor,
    #[serde(rename = "exportedAt")]
    pub exported_at: String,
    pub payload: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedArchive {
    pub exported_at: String,
    pub manifest: ArchiveManifest,
    pub nodes: Vec<Node>,
    pub blobs: Vec<ArchiveBlob>,
    pub trash_snapshots: Vec<ArchiveTrashSnapshot>,
    pub workspace: Option<ArchiveWorkspace>,
    pub plugins_exported_at: String,
    pub plugins: Vec<ArchivePluginPackage>,
}

#[derive(Debug, Error)]
pub enum WorkspaceArchiveError {
    #[error("{label} exceeds its archive limit ({actual} > {maximum})")]
    Limit {
        label: &'static str,
        actual: usize,
        maximum: usize,
    },
    #[error("workspace archive JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("workspace archive field `{0}` is missing or invalid")]
    InvalidField(String),
    #[error("unsupported workspace archive kind or version")]
    UnsupportedVersion,
    #[error("workspace archive manifest `{0}` does not match its payload")]
    ManifestMismatch(&'static str),
    #[error("workspace archive base64 is invalid")]
    InvalidBase64,
    #[error("archive passphrase must contain 12 to 1024 UTF-16 code units")]
    InvalidPassphrase,
    #[error("archive passphrase is incorrect or encrypted data is damaged")]
    DecryptionFailed,
    #[error("encrypted workspace archive is invalid")]
    InvalidEnvelope,
    #[error("decrypted workspace archive is not valid UTF-8")]
    InvalidUtf8,
}

pub fn is_encrypted_workspace_archive(raw: &str) -> bool {
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|value| value.get("kind").and_then(Value::as_str).map(str::to_owned))
        .as_deref()
        == Some(ENCRYPTED_KIND)
}

pub fn decrypt_workspace_archive(
    raw: &str,
    passphrase: &str,
    limits: ArchiveLimits,
) -> Result<String, WorkspaceArchiveError> {
    validate_passphrase(passphrase)?;
    ensure_limit("encrypted archive", raw.len(), limits.max_envelope_bytes)?;
    let envelope: EncryptedEnvelope =
        serde_json::from_str(raw).map_err(|_| WorkspaceArchiveError::InvalidEnvelope)?;
    envelope.validate(limits)?;

    let salt = decode_base64(&envelope.kdf.salt)?;
    let iv = decode_base64(&envelope.cipher.iv)?;
    let ciphertext = decode_base64(&envelope.ciphertext)?;
    if salt.len() != SALT_BYTES
        || iv.len() != IV_BYTES
        || ciphertext.len() != envelope.plaintext_bytes + AES_GCM_TAG_BYTES
    {
        return Err(WorkspaceArchiveError::InvalidEnvelope);
    }

    let mut key = [0_u8; 32];
    pbkdf2_hmac::<Sha256>(passphrase.as_bytes(), &salt, PBKDF2_ITERATIONS, &mut key);
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|_| WorkspaceArchiveError::DecryptionFailed)?;
    let aad = envelope.additional_data()?;
    let plaintext = cipher
        .decrypt(
            iv.as_slice().into(),
            Payload {
                msg: &ciphertext,
                aad: &aad,
            },
        )
        .map_err(|_| WorkspaceArchiveError::DecryptionFailed)?;
    if plaintext.len() != envelope.plaintext_bytes {
        return Err(WorkspaceArchiveError::DecryptionFailed);
    }
    String::from_utf8(plaintext).map_err(|_| WorkspaceArchiveError::InvalidUtf8)
}

pub fn parse_workspace_archive(
    raw: &str,
    limits: ArchiveLimits,
) -> Result<ParsedArchive, WorkspaceArchiveError> {
    ensure_limit("workspace archive", raw.len(), limits.max_plaintext_bytes)?;
    let root: Value = serde_json::from_str(raw)?;
    let object = record(&root, "root")?;
    if string(object, "kind")? != ARCHIVE_KIND || integer(object, "version")? != ARCHIVE_VERSION {
        return Err(WorkspaceArchiveError::UnsupportedVersion);
    }
    let exported_at = non_empty_string(object, "exportedAt")?.to_owned();
    let core_value = field(object, "core")?;
    let core = record(core_value, "core")?;
    let plugins_value = field(object, "plugins")?;
    let plugins_object = record(plugins_value, "plugins")?;

    let node_values = array(core, "nodes")?;
    let blob_values = array(core, "blobs")?;
    let trash_values = array(core, "trashSnapshots")?;
    let plugin_values = array(plugins_object, "plugins")?;
    ensure_limit("nodes", node_values.len(), limits.max_nodes)?;
    ensure_limit("blobs", blob_values.len(), limits.max_blobs)?;
    ensure_limit(
        "trash snapshots",
        trash_values.len(),
        limits.max_trash_snapshots,
    )?;
    ensure_limit("plugins", plugin_values.len(), limits.max_plugins)?;

    let workspace = normalize_workspace(core.get("workspace"), limits)?;
    let nodes = node_values
        .iter()
        .enumerate()
        .map(|(index, value)| parse_node(value, &format!("nodes[{index}]")))
        .collect::<Result<Vec<_>, _>>()?;
    let blobs = blob_values
        .iter()
        .enumerate()
        .map(|(index, value)| parse_blob(value, &format!("blobs[{index}]"), limits))
        .collect::<Result<Vec<_>, _>>()?;
    let trash_snapshots = trash_values
        .iter()
        .enumerate()
        .map(|(index, value)| parse_trash_snapshot(value, index, limits))
        .collect::<Result<Vec<_>, _>>()?;
    let nested_blob_count = trash_snapshots
        .iter()
        .filter(|snapshot| snapshot.blob.is_some())
        .count();
    ensure_limit(
        "all blobs",
        blobs.len() + nested_blob_count,
        limits.max_blobs,
    )?;
    let total_blob_bytes = blobs
        .iter()
        .map(|blob| blob.data.len())
        .chain(
            trash_snapshots
                .iter()
                .filter_map(|snapshot| snapshot.blob.as_ref().map(|blob| blob.data.len())),
        )
        .try_fold(0_usize, |total, size| total.checked_add(size))
        .ok_or(WorkspaceArchiveError::Limit {
            label: "total blob bytes",
            actual: usize::MAX,
            maximum: limits.max_total_blob_bytes,
        })?;
    ensure_limit(
        "total blob bytes",
        total_blob_bytes,
        limits.max_total_blob_bytes,
    )?;

    if string(plugins_object, "kind")? != WORKSPACE_BACKUP_KIND
        || integer(plugins_object, "version")? != WORKSPACE_BACKUP_VERSION
    {
        return Err(WorkspaceArchiveError::InvalidField("plugins".into()));
    }
    let plugins_exported_at = non_empty_string(plugins_object, "exportedAt")?.to_owned();
    let plugins = plugin_values
        .iter()
        .enumerate()
        .map(|(index, value)| parse_plugin(value, index))
        .collect::<Result<Vec<_>, _>>()?;

    let checksum_value = json!({
        "exportedAt": exported_at,
        "core": core_value,
        "plugins": plugins_value,
    });
    let checksum_raw = serde_json::to_vec(&checksum_value)?;
    let checksum = format!("{:08x}", crc32fast::hash(&checksum_raw));
    let manifest = parse_manifest(field(object, "manifest")?)?;
    validate_manifest(
        &manifest,
        &checksum,
        nodes.len(),
        blobs.len(),
        blobs.iter().map(|blob| blob.data.len()).sum(),
        trash_snapshots.len(),
        plugins.len(),
        workspace.as_ref().map_or(0, |state| state.tabs.len()),
    )?;

    Ok(ParsedArchive {
        exported_at,
        manifest,
        nodes,
        blobs,
        trash_snapshots,
        workspace,
        plugins_exported_at,
        plugins,
    })
}

fn validate_passphrase(passphrase: &str) -> Result<(), WorkspaceArchiveError> {
    let len = passphrase.encode_utf16().count();
    if !(12..=1024).contains(&len) {
        return Err(WorkspaceArchiveError::InvalidPassphrase);
    }
    Ok(())
}

fn ensure_limit(
    label: &'static str,
    actual: usize,
    maximum: usize,
) -> Result<(), WorkspaceArchiveError> {
    if actual > maximum {
        return Err(WorkspaceArchiveError::Limit {
            label,
            actual,
            maximum,
        });
    }
    Ok(())
}

fn record<'a>(
    value: &'a Value,
    label: &str,
) -> Result<&'a Map<String, Value>, WorkspaceArchiveError> {
    value
        .as_object()
        .ok_or_else(|| WorkspaceArchiveError::InvalidField(label.into()))
}

fn field<'a>(
    object: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a Value, WorkspaceArchiveError> {
    object
        .get(key)
        .ok_or_else(|| WorkspaceArchiveError::InvalidField(key.into()))
}

fn string<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str, WorkspaceArchiveError> {
    field(object, key)?
        .as_str()
        .ok_or_else(|| WorkspaceArchiveError::InvalidField(key.into()))
}

fn non_empty_string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a str, WorkspaceArchiveError> {
    let value = string(object, key)?;
    if value.is_empty() {
        return Err(WorkspaceArchiveError::InvalidField(key.into()));
    }
    Ok(value)
}

fn integer(object: &Map<String, Value>, key: &str) -> Result<u64, WorkspaceArchiveError> {
    field(object, key)?
        .as_u64()
        .ok_or_else(|| WorkspaceArchiveError::InvalidField(key.into()))
}

fn array<'a>(
    object: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a Vec<Value>, WorkspaceArchiveError> {
    field(object, key)?
        .as_array()
        .ok_or_else(|| WorkspaceArchiveError::InvalidField(key.into()))
}

fn parse_node(value: &Value, label: &str) -> Result<Node, WorkspaceArchiveError> {
    let node: Node = serde_json::from_value(value.clone())
        .map_err(|_| WorkspaceArchiveError::InvalidField(label.into()))?;
    let base = node.base();
    if base.id.is_empty()
        || base.sort_key.is_empty()
        || base.parent_id.as_ref().is_some_and(String::is_empty)
    {
        return Err(WorkspaceArchiveError::InvalidField(label.into()));
    }
    match &node {
        Node::Bookmark { content, .. } if content.url.is_empty() => {
            return Err(WorkspaceArchiveError::InvalidField(label.into()));
        }
        Node::File { blob_ref, .. } if blob_ref.store != "blobs" || blob_ref.key.is_empty() => {
            return Err(WorkspaceArchiveError::InvalidField(label.into()));
        }
        Node::Feed { content, .. } if content.key.is_empty() => {
            return Err(WorkspaceArchiveError::InvalidField(label.into()));
        }
        _ => {}
    }
    Ok(node)
}

fn parse_blob(
    value: &Value,
    label: &str,
    limits: ArchiveLimits,
) -> Result<ArchiveBlob, WorkspaceArchiveError> {
    let object = record(value, label)?;
    let key = non_empty_string(object, "key")?.to_owned();
    let mime = object
        .get("mime")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let size = integer(object, "size")? as usize;
    ensure_limit("single blob", size, limits.max_single_blob_bytes)?;
    let encoded = string(object, "dataBase64")?;
    let data = decode_base64(encoded)?;
    if data.len() != size {
        return Err(WorkspaceArchiveError::InvalidField(format!("{label}.size")));
    }
    Ok(ArchiveBlob { key, mime, data })
}

fn parse_trash_snapshot(
    value: &Value,
    index: usize,
    limits: ArchiveLimits,
) -> Result<ArchiveTrashSnapshot, WorkspaceArchiveError> {
    let label = format!("trashSnapshots[{index}]");
    let object = record(value, &label)?;
    let id = non_empty_string(object, "id")?.to_owned();
    let node = parse_node(field(object, "node")?, &format!("{label}.node"))?;
    let blob = object
        .get("blob")
        .map(|value| parse_blob(value, "trash blob", limits))
        .transpose()?;
    let captured_at = field(object, "capturedAt")?
        .as_i64()
        .ok_or_else(|| WorkspaceArchiveError::InvalidField(format!("{label}.capturedAt")))?;
    Ok(ArchiveTrashSnapshot {
        id,
        node,
        blob,
        captured_at,
    })
}

fn normalize_workspace(
    value: Option<&Value>,
    limits: ArchiveLimits,
) -> Result<Option<ArchiveWorkspace>, WorkspaceArchiveError> {
    let Some(object) = value.and_then(Value::as_object) else {
        return Ok(None);
    };
    let Some(tabs) = object.get("tabs").and_then(Value::as_array) else {
        return Ok(None);
    };
    ensure_limit("workspace tabs", tabs.len(), limits.max_tabs)?;
    let tabs = tabs.iter().filter(|tab| tab.is_object()).cloned().collect();
    let nullable_string = |key: &str| object.get(key).and_then(Value::as_str).map(str::to_owned);
    let workspace_kind = match object.get("workspaceKind").and_then(Value::as_str) {
        Some("audio") => "audio",
        Some("development") => "development",
        _ => "files",
    };
    let development_tool = match object.get("developmentTool").and_then(Value::as_str) {
        Some("shell") => "shell",
        _ => "git",
    };
    Ok(Some(ArchiveWorkspace {
        tabs,
        active_id: nullable_string("activeId"),
        transient_id: nullable_string("transientId"),
        active_module: object
            .get("activeModule")
            .and_then(Value::as_str)
            .unwrap_or("home")
            .to_owned(),
        workspace_kind: workspace_kind.to_owned(),
        development_tool: development_tool.to_owned(),
        sidebar_collapsed: object.get("sidebarCollapsed") == Some(&Value::Bool(true)),
        right_panel_open: object.get("rightPanelOpen") == Some(&Value::Bool(true)),
    }))
}

fn parse_plugin(
    value: &Value,
    index: usize,
) -> Result<ArchivePluginPackage, WorkspaceArchiveError> {
    let label = format!("plugins[{index}]");
    let object = record(value, &label)?;
    if string(object, "kind")? != PLUGIN_DATA_KIND
        || integer(object, "version")? != PLUGIN_DATA_VERSION
    {
        return Err(WorkspaceArchiveError::InvalidField(label));
    }
    let descriptor = record(field(object, "plugin")?, "plugin")?;
    let data_version = integer(descriptor, "dataVersion")?;
    if data_version == 0 {
        return Err(WorkspaceArchiveError::InvalidField(format!(
            "{label}.plugin.dataVersion"
        )));
    }
    Ok(ArchivePluginPackage {
        kind: PLUGIN_DATA_KIND.into(),
        version: PLUGIN_DATA_VERSION,
        plugin: ArchivePluginDescriptor {
            id: non_empty_string(descriptor, "id")?.to_owned(),
            label: non_empty_string(descriptor, "label")?.to_owned(),
            data_kind: non_empty_string(descriptor, "dataKind")?.to_owned(),
            data_version,
        },
        exported_at: non_empty_string(object, "exportedAt")?.to_owned(),
        payload: object.get("payload").cloned().unwrap_or(Value::Null),
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawManifest {
    algorithm: String,
    checksum: String,
    node_count: usize,
    blob_count: usize,
    blob_bytes: usize,
    trash_snapshot_count: usize,
    plugin_count: usize,
    tab_count: usize,
}

fn parse_manifest(value: &Value) -> Result<ArchiveManifest, WorkspaceArchiveError> {
    let raw: RawManifest = serde_json::from_value(value.clone())
        .map_err(|_| WorkspaceArchiveError::InvalidField("manifest".into()))?;
    if raw.algorithm != "crc32"
        || raw.checksum.len() != 8
        || !raw
            .checksum
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(WorkspaceArchiveError::InvalidField("manifest".into()));
    }
    Ok(ArchiveManifest {
        checksum: raw.checksum,
        node_count: raw.node_count,
        blob_count: raw.blob_count,
        blob_bytes: raw.blob_bytes,
        trash_snapshot_count: raw.trash_snapshot_count,
        plugin_count: raw.plugin_count,
        tab_count: raw.tab_count,
    })
}

#[allow(clippy::too_many_arguments)]
fn validate_manifest(
    manifest: &ArchiveManifest,
    checksum: &str,
    node_count: usize,
    blob_count: usize,
    blob_bytes: usize,
    trash_snapshot_count: usize,
    plugin_count: usize,
    tab_count: usize,
) -> Result<(), WorkspaceArchiveError> {
    let fields = [
        (manifest.checksum == checksum, "checksum"),
        (manifest.node_count == node_count, "nodeCount"),
        (manifest.blob_count == blob_count, "blobCount"),
        (manifest.blob_bytes == blob_bytes, "blobBytes"),
        (
            manifest.trash_snapshot_count == trash_snapshot_count,
            "trashSnapshotCount",
        ),
        (manifest.plugin_count == plugin_count, "pluginCount"),
        (manifest.tab_count == tab_count, "tabCount"),
    ];
    for (valid, field) in fields {
        if !valid {
            return Err(WorkspaceArchiveError::ManifestMismatch(field));
        }
    }
    Ok(())
}

fn decode_base64(value: &str) -> Result<Vec<u8>, WorkspaceArchiveError> {
    STANDARD
        .decode(value)
        .map_err(|_| WorkspaceArchiveError::InvalidBase64)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EncryptedEnvelope {
    kind: String,
    version: u64,
    created_at: String,
    plaintext_bytes: usize,
    kdf: EnvelopeKdf,
    cipher: EnvelopeCipher,
    ciphertext: String,
}

#[derive(Deserialize)]
struct EnvelopeKdf {
    name: String,
    hash: String,
    iterations: u32,
    salt: String,
}

#[derive(Deserialize)]
struct EnvelopeCipher {
    name: String,
    iv: String,
}

impl EncryptedEnvelope {
    fn validate(&self, limits: ArchiveLimits) -> Result<(), WorkspaceArchiveError> {
        if self.kind != ENCRYPTED_KIND
            || self.version != ENCRYPTED_VERSION
            || self.created_at.is_empty()
            || self.plaintext_bytes > limits.max_plaintext_bytes
            || self.kdf.name != "PBKDF2"
            || self.kdf.hash != "SHA-256"
            || self.kdf.iterations != PBKDF2_ITERATIONS
            || self.cipher.name != "AES-GCM"
            || self.kdf.salt.len() != 4 * SALT_BYTES.div_ceil(3)
            || self.cipher.iv.len() != 4 * IV_BYTES.div_ceil(3)
            || self.ciphertext.len() != 4 * (self.plaintext_bytes + AES_GCM_TAG_BYTES).div_ceil(3)
        {
            return Err(WorkspaceArchiveError::InvalidEnvelope);
        }
        Ok(())
    }

    fn additional_data(&self) -> Result<Vec<u8>, WorkspaceArchiveError> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct AdditionalData<'a> {
            kind: &'a str,
            version: u64,
            created_at: &'a str,
            plaintext_bytes: usize,
            iterations: u32,
        }
        serde_json::to_vec(&AdditionalData {
            kind: &self.kind,
            version: self.version,
            created_at: &self.created_at,
            plaintext_bytes: self.plaintext_bytes,
            iterations: PBKDF2_ITERATIONS,
        })
        .map_err(WorkspaceArchiveError::Json)
    }
}

#[cfg(test)]
mod tests {
    use aes_gcm::aead::Aead;
    use ideall_protocol::NodeKind;

    use super::*;

    const EXPORTED_AT: &str = "2026-01-01T00:00:00.000Z";

    fn archive_value() -> Value {
        let core = json!({
            "nodes": [{
                "id": "n1",
                "kind": "note",
                "title": "Archive Note",
                "parentId": null,
                "sortKey": "a0",
                "tags": ["archive"],
                "createdAt": 1,
                "updatedAt": 2,
                "content": [{"type": "p", "children": [{"text": "body"}]}]
            }],
            "blobs": [{"key": "f1", "mime": "text/plain", "size": 3, "dataBase64": "YWJj"}],
            "trashSnapshots": [],
            "workspace": null
        });
        let plugins = json!({
            "kind": "ideall.workspace-backup",
            "version": 1,
            "exportedAt": EXPORTED_AT,
            "plugins": []
        });
        let checksum_raw = serde_json::to_vec(&json!({
            "exportedAt": EXPORTED_AT,
            "core": core,
            "plugins": plugins,
        }))
        .unwrap();
        let checksum = format!("{:08x}", crc32fast::hash(&checksum_raw));
        json!({
            "kind": ARCHIVE_KIND,
            "version": 2,
            "exportedAt": EXPORTED_AT,
            "core": core,
            "plugins": plugins,
            "manifest": {
                "algorithm": "crc32",
                "checksum": checksum,
                "nodeCount": 1,
                "blobCount": 1,
                "blobBytes": 3,
                "trashSnapshotCount": 0,
                "pluginCount": 0,
                "tabCount": 0
            }
        })
    }

    #[test]
    fn parses_v2_and_rejects_tampering() {
        let mut value = archive_value();
        let parsed = parse_workspace_archive(
            &serde_json::to_string_pretty(&value).unwrap(),
            ArchiveLimits::default(),
        )
        .unwrap();
        assert_eq!(parsed.nodes.len(), 1);
        assert_eq!(parsed.blobs[0].data, b"abc");

        value["core"]["nodes"][0]["title"] = Value::String("tampered".into());
        let error = parse_workspace_archive(
            &serde_json::to_string(&value).unwrap(),
            ArchiveLimits::default(),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            WorkspaceArchiveError::ManifestMismatch("checksum")
        ));
    }

    #[test]
    fn decrypts_web_crypto_compatible_envelope() {
        let plaintext = serde_json::to_string(&archive_value()).unwrap();
        let passphrase = "correct horse battery staple";
        let salt = [7_u8; SALT_BYTES];
        let iv = [9_u8; IV_BYTES];
        let created_at = "2026-02-03T04:05:06.000Z";
        let mut key = [0_u8; 32];
        pbkdf2_hmac::<Sha256>(passphrase.as_bytes(), &salt, PBKDF2_ITERATIONS, &mut key);
        let header = EncryptedEnvelope {
            kind: ENCRYPTED_KIND.into(),
            version: 1,
            created_at: created_at.into(),
            plaintext_bytes: plaintext.len(),
            kdf: EnvelopeKdf {
                name: "PBKDF2".into(),
                hash: "SHA-256".into(),
                iterations: PBKDF2_ITERATIONS,
                salt: STANDARD.encode(salt),
            },
            cipher: EnvelopeCipher {
                name: "AES-GCM".into(),
                iv: STANDARD.encode(iv),
            },
            ciphertext: String::new(),
        };
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let ciphertext = cipher
            .encrypt(
                (&iv).into(),
                Payload {
                    msg: plaintext.as_bytes(),
                    aad: &header.additional_data().unwrap(),
                },
            )
            .unwrap();
        let envelope = json!({
            "kind": ENCRYPTED_KIND,
            "version": 1,
            "createdAt": created_at,
            "plaintextBytes": plaintext.len(),
            "kdf": {
                "name": "PBKDF2",
                "hash": "SHA-256",
                "iterations": PBKDF2_ITERATIONS,
                "salt": STANDARD.encode(salt)
            },
            "cipher": {"name": "AES-GCM", "iv": STANDARD.encode(iv)},
            "ciphertext": STANDARD.encode(ciphertext)
        });
        let decrypted = decrypt_workspace_archive(
            &serde_json::to_string_pretty(&envelope).unwrap(),
            passphrase,
            ArchiveLimits::default(),
        )
        .unwrap();
        assert_eq!(decrypted, plaintext);
        assert!(parse_workspace_archive(&decrypted, ArchiveLimits::default()).is_ok());
    }

    #[test]
    fn enforces_limits_before_decoding_blobs() {
        let raw = serde_json::to_string(&archive_value()).unwrap();
        let limits = ArchiveLimits {
            max_nodes: 0,
            ..ArchiveLimits::default()
        };
        assert!(matches!(
            parse_workspace_archive(&raw, limits),
            Err(WorkspaceArchiveError::Limit { label: "nodes", .. })
        ));
    }

    #[test]
    fn node_kind_is_available_for_database_projection() {
        let parsed = parse_workspace_archive(
            &serde_json::to_string(&archive_value()).unwrap(),
            ArchiveLimits::default(),
        )
        .unwrap();
        assert_eq!(parsed.nodes[0].kind(), NodeKind::Note);
    }
}
