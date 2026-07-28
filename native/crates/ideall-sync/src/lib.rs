//! UI- and transport-independent pieces of ideall's encrypted sync protocol.

mod notes;
mod runner;

pub use notes::{gc_sync_notes, merge_sync_notes};
pub use runner::{
    LocalStoreError, SyncLocalStore, SyncRunError, SyncRunResult, SyncTransport, TransportError,
    run_sync, run_sync_with,
};

use std::collections::{BTreeMap, HashMap, HashSet};

use aes_gcm::{Aes256Gcm, KeyInit as _, aead::Aead as _};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use hkdf::Hkdf;
use ideall_protocol::{Node, SyncNote, SyncSubscription};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest as _, Sha256};
use thiserror::Error;

const SALT: &[u8] = b"wonita-sync";
pub const SYNC_PART_MAX_CIPHERTEXT_CHARS: usize = 262_144;
pub const SYNC_PART_MAX_PLAINTEXT_BYTES: usize = 196_592;
pub const SYNC_MAX_PARTITION: u16 = 1_023;
pub const MAX_FUTURE_SKEW_MS: i64 = 24 * 60 * 60 * 1_000;
pub const TOMBSTONE_TTL_MS: i64 = 90 * 24 * 60 * 60 * 1_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyncBlockBudget {
    pub max_records: usize,
    pub max_plaintext_bytes: usize,
}

impl SyncBlockBudget {
    pub const SUBSCRIPTIONS: Self = Self {
        max_records: 50_000,
        max_plaintext_bytes: 4 * 1024 * 1024,
    };
    pub const NOTES: Self = Self {
        max_records: 100_000,
        max_plaintext_bytes: 32 * 1024 * 1024,
    };
    pub const BOOKMARKS: Self = Self {
        max_records: 100_000,
        max_plaintext_bytes: 16 * 1024 * 1024,
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncScope {
    Subscriptions,
    Notes,
    Bookmarks,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DerivedKeys {
    pub storage_id: String,
    encryption_key: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Encrypted {
    pub iv: String,
    pub ciphertext: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SyncManifest {
    pub generation: String,
    pub part_count: usize,
    pub total_ciphertext_chars: usize,
    pub parts_sha256: String,
    pub version: u64,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SyncGenerationPart {
    pub generation: String,
    pub part_index: usize,
    pub iv: String,
    pub ciphertext: String,
    pub content_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedSnapshot {
    pub generation: String,
    pub parts: Vec<SyncGenerationPart>,
    pub total_ciphertext_chars: usize,
    pub parts_sha256: String,
}

impl PreparedSnapshot {
    pub fn manifest(&self, version: u64, updated_at_ms: i64) -> SyncManifest {
        SyncManifest {
            generation: self.generation.clone(),
            part_count: self.parts.len(),
            total_ciphertext_chars: self.total_ciphertext_chars,
            parts_sha256: self.parts_sha256.clone(),
            version,
            updated_at_ms,
        }
    }
}

#[derive(Debug, Error)]
pub enum SyncCryptoError {
    #[error("sync code must contain exactly 32 hexadecimal digits")]
    InvalidCode,
    #[error("sync partition must be between 0 and {SYNC_MAX_PARTITION}")]
    InvalidPartition,
    #[error("sync payload exceeds its transfer budget")]
    BlockLimit,
    #[error("sync ciphertext is malformed")]
    MalformedCiphertext,
    #[error("sync ciphertext failed authentication")]
    Authentication,
    #[error("sync JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("sync manifest is invalid")]
    InvalidManifest,
    #[error("sync generation is incomplete or inconsistent")]
    GenerationChanged,
    #[error("sync part digest does not match its encrypted content")]
    DigestMismatch,
    #[error("sync snapshot contains too many records")]
    RecordLimit,
    #[error("the operating system random source failed")]
    Random,
}

pub fn normalize_sync_code(code: &str) -> String {
    code.chars()
        .filter(char::is_ascii_hexdigit)
        .flat_map(char::to_lowercase)
        .collect()
}

pub fn is_valid_sync_code(code: &str) -> bool {
    normalize_sync_code(code).len() == 32
}

pub fn derive_keys(
    code: &str,
    scope: SyncScope,
    partition: u16,
) -> Result<DerivedKeys, SyncCryptoError> {
    let code = normalize_sync_code(code);
    if code.len() != 32 {
        return Err(SyncCryptoError::InvalidCode);
    }
    if partition > SYNC_MAX_PARTITION {
        return Err(SyncCryptoError::InvalidPartition);
    }
    let (id_info, encryption_info) = scope_info(scope, partition);
    let hkdf = Hkdf::<Sha256>::new(Some(SALT), code.as_bytes());
    let mut storage_id = [0_u8; 16];
    hkdf.expand(id_info.as_bytes(), &mut storage_id)
        .map_err(|_| SyncCryptoError::InvalidCode)?;
    let mut encryption_key = [0_u8; 32];
    hkdf.expand(encryption_info.as_bytes(), &mut encryption_key)
        .map_err(|_| SyncCryptoError::InvalidCode)?;
    Ok(DerivedKeys {
        storage_id: hex(&storage_id),
        encryption_key,
    })
}

impl DerivedKeys {
    pub fn encrypt_bytes(
        &self,
        plaintext: &[u8],
        max_ciphertext_chars: Option<usize>,
    ) -> Result<Encrypted, SyncCryptoError> {
        let mut iv = [0_u8; 12];
        getrandom::fill(&mut iv).map_err(|_| SyncCryptoError::Random)?;
        let cipher = Aes256Gcm::new_from_slice(&self.encryption_key)
            .map_err(|_| SyncCryptoError::Authentication)?;
        let ciphertext = cipher
            .encrypt((&iv).into(), plaintext)
            .map_err(|_| SyncCryptoError::Authentication)?;
        let ciphertext = BASE64.encode(ciphertext);
        if max_ciphertext_chars.is_some_and(|limit| ciphertext.len() > limit) {
            return Err(SyncCryptoError::BlockLimit);
        }
        Ok(Encrypted {
            iv: BASE64.encode(iv),
            ciphertext,
        })
    }

    pub fn decrypt_bytes(
        &self,
        encrypted: &Encrypted,
        max_ciphertext_chars: Option<usize>,
    ) -> Result<Vec<u8>, SyncCryptoError> {
        if max_ciphertext_chars.is_some_and(|limit| encrypted.ciphertext.len() > limit) {
            return Err(SyncCryptoError::BlockLimit);
        }
        let iv = BASE64
            .decode(&encrypted.iv)
            .map_err(|_| SyncCryptoError::MalformedCiphertext)?;
        let ciphertext = BASE64
            .decode(&encrypted.ciphertext)
            .map_err(|_| SyncCryptoError::MalformedCiphertext)?;
        if iv.len() != 12 || ciphertext.len() < 16 {
            return Err(SyncCryptoError::MalformedCiphertext);
        }
        let cipher = Aes256Gcm::new_from_slice(&self.encryption_key)
            .map_err(|_| SyncCryptoError::Authentication)?;
        cipher
            .decrypt(iv.as_slice().into(), ciphertext.as_slice())
            .map_err(|_| SyncCryptoError::Authentication)
    }

    pub fn encrypt_json<T: Serialize>(
        &self,
        value: &T,
        max_plaintext_bytes: usize,
    ) -> Result<Encrypted, SyncCryptoError> {
        let plaintext = serde_json::to_vec(value)?;
        if plaintext.len() > max_plaintext_bytes {
            return Err(SyncCryptoError::BlockLimit);
        }
        self.encrypt_bytes(&plaintext, None)
    }

    pub fn decrypt_json<T: DeserializeOwned>(
        &self,
        encrypted: &Encrypted,
        max_plaintext_bytes: usize,
        max_ciphertext_chars: usize,
    ) -> Result<T, SyncCryptoError> {
        let plaintext = self.decrypt_bytes(encrypted, Some(max_ciphertext_chars))?;
        if plaintext.len() > max_plaintext_bytes {
            return Err(SyncCryptoError::BlockLimit);
        }
        Ok(serde_json::from_slice(&plaintext)?)
    }
}

pub fn new_generation() -> Result<String, SyncCryptoError> {
    let mut generation = [0_u8; 16];
    getrandom::fill(&mut generation).map_err(|_| SyncCryptoError::Random)?;
    Ok(hex(&generation))
}

pub fn prepare_snapshot<T: Serialize>(
    code: &str,
    scope: SyncScope,
    records: &[T],
    budget: SyncBlockBudget,
) -> Result<PreparedSnapshot, SyncCryptoError> {
    prepare_snapshot_with_generation(code, scope, records, budget, new_generation()?)
}

pub fn prepare_snapshot_with_generation<T: Serialize>(
    code: &str,
    scope: SyncScope,
    records: &[T],
    budget: SyncBlockBudget,
    generation: String,
) -> Result<PreparedSnapshot, SyncCryptoError> {
    if !is_lower_hex(&generation, 32) {
        return Err(SyncCryptoError::InvalidManifest);
    }
    if records.len() > budget.max_records {
        return Err(SyncCryptoError::RecordLimit);
    }
    let plaintext = serde_json::to_vec(records)?;
    if plaintext.len() > budget.max_plaintext_bytes {
        return Err(SyncCryptoError::BlockLimit);
    }
    let mut parts = Vec::new();
    for (part_index, chunk) in plaintext.chunks(SYNC_PART_MAX_PLAINTEXT_BYTES).enumerate() {
        if part_index > SYNC_MAX_PARTITION as usize {
            return Err(SyncCryptoError::BlockLimit);
        }
        let keys = derive_keys(code, scope, part_index as u16)?;
        let encrypted = keys.encrypt_bytes(chunk, Some(SYNC_PART_MAX_CIPHERTEXT_CHARS))?;
        let content_sha256 = part_content_digest(&encrypted.iv, &encrypted.ciphertext);
        parts.push(SyncGenerationPart {
            generation: generation.clone(),
            part_index,
            iv: encrypted.iv,
            ciphertext: encrypted.ciphertext,
            content_sha256,
        });
    }
    if parts.is_empty() {
        return Err(SyncCryptoError::BlockLimit);
    }
    let total_ciphertext_chars = parts.iter().map(|part| part.ciphertext.len()).sum();
    let parts_sha256 = parts_digest(&parts);
    Ok(PreparedSnapshot {
        generation,
        parts,
        total_ciphertext_chars,
        parts_sha256,
    })
}

pub fn decrypt_snapshot<T: DeserializeOwned>(
    code: &str,
    scope: SyncScope,
    manifest: &SyncManifest,
    parts: &[SyncGenerationPart],
    budget: SyncBlockBudget,
) -> Result<Vec<T>, SyncCryptoError> {
    validate_manifest(manifest)?;
    if parts.len() != manifest.part_count {
        return Err(SyncCryptoError::GenerationChanged);
    }
    let mut ordered = BTreeMap::new();
    for part in parts {
        if part.generation != manifest.generation
            || part.part_index >= manifest.part_count
            || !is_lower_hex(&part.content_sha256, 64)
            || ordered.insert(part.part_index, part).is_some()
        {
            return Err(SyncCryptoError::GenerationChanged);
        }
    }
    if ordered.len() != manifest.part_count {
        return Err(SyncCryptoError::GenerationChanged);
    }
    let ordered = ordered.into_values().collect::<Vec<_>>();
    let total_ciphertext_chars: usize = ordered.iter().map(|part| part.ciphertext.len()).sum();
    if total_ciphertext_chars != manifest.total_ciphertext_chars {
        return Err(SyncCryptoError::InvalidManifest);
    }
    for part in &ordered {
        if part_content_digest(&part.iv, &part.ciphertext) != part.content_sha256 {
            return Err(SyncCryptoError::DigestMismatch);
        }
    }
    if parts_digest_refs(&ordered) != manifest.parts_sha256 {
        return Err(SyncCryptoError::DigestMismatch);
    }
    let mut plaintext = Vec::new();
    for part in ordered {
        let keys = derive_keys(code, scope, part.part_index as u16)?;
        let encrypted = part.encrypted_value();
        let decrypted = keys.decrypt_bytes(&encrypted, Some(SYNC_PART_MAX_CIPHERTEXT_CHARS))?;
        if decrypted.len() > SYNC_PART_MAX_PLAINTEXT_BYTES
            || plaintext.len().saturating_add(decrypted.len()) > budget.max_plaintext_bytes
        {
            return Err(SyncCryptoError::BlockLimit);
        }
        plaintext.extend_from_slice(&decrypted);
    }
    let records: Vec<T> = serde_json::from_slice(&plaintext)?;
    if records.len() > budget.max_records {
        return Err(SyncCryptoError::RecordLimit);
    }
    Ok(records)
}

impl SyncGenerationPart {
    fn encrypted_value(&self) -> Encrypted {
        Encrypted {
            iv: self.iv.clone(),
            ciphertext: self.ciphertext.clone(),
        }
    }
}

fn validate_manifest(manifest: &SyncManifest) -> Result<(), SyncCryptoError> {
    if !is_lower_hex(&manifest.generation, 32)
        || manifest.part_count == 0
        || manifest.part_count > SYNC_MAX_PARTITION as usize + 1
        || manifest.total_ciphertext_chars
            > manifest
                .part_count
                .saturating_mul(SYNC_PART_MAX_CIPHERTEXT_CHARS)
        || !is_lower_hex(&manifest.parts_sha256, 64)
        || manifest.version == 0
        || manifest.updated_at_ms < 0
    {
        return Err(SyncCryptoError::InvalidManifest);
    }
    Ok(())
}

fn part_content_digest(iv: &str, ciphertext: &str) -> String {
    let mut value = String::with_capacity(iv.len() + ciphertext.len() + 1);
    value.push_str(iv);
    value.push('\0');
    value.push_str(ciphertext);
    sha256_hex(value.as_bytes())
}

fn parts_digest(parts: &[SyncGenerationPart]) -> String {
    parts_digest_refs(&parts.iter().collect::<Vec<_>>())
}

fn parts_digest_refs(parts: &[&SyncGenerationPart]) -> String {
    let mut value = String::new();
    for (index, part) in parts.iter().enumerate() {
        use std::fmt::Write as _;
        writeln!(&mut value, "{index}:{}", part.content_sha256)
            .expect("writing to a String cannot fail");
    }
    sha256_hex(value.as_bytes())
}

fn sha256_hex(value: &[u8]) -> String {
    hex(&Sha256::digest(value))
}

fn is_lower_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub trait SyncRecord {
    fn sync_id(&self) -> &str;
    fn updated_at(&self) -> i64;
    fn deleted_at(&self) -> Option<i64>;
}

impl SyncRecord for Node {
    fn sync_id(&self) -> &str {
        &self.base().id
    }

    fn updated_at(&self) -> i64 {
        self.base().updated_at
    }

    fn deleted_at(&self) -> Option<i64> {
        self.base().deleted_at
    }
}

impl SyncRecord for SyncNote {
    fn sync_id(&self) -> &str {
        &self.id
    }

    fn updated_at(&self) -> i64 {
        self.updated_at
    }

    fn deleted_at(&self) -> Option<i64> {
        self.deleted_at
    }
}

impl SyncRecord for SyncSubscription {
    fn sync_id(&self) -> &str {
        &self.id
    }

    fn updated_at(&self) -> i64 {
        self.updated_at
    }

    fn deleted_at(&self) -> Option<i64> {
        self.deleted_at
    }
}

pub fn union_merge<T: SyncRecord + Clone>(local: &[T], remote: &[T]) -> Vec<T> {
    let mut records = HashMap::with_capacity(local.len().saturating_add(remote.len()));
    for record in remote {
        records.insert(record.sync_id().to_owned(), record.clone());
    }
    for record in local {
        match records.get(record.sync_id()) {
            Some(remote) if remote.updated_at() > record.updated_at() => {}
            _ => {
                records.insert(record.sync_id().to_owned(), record.clone());
            }
        }
    }
    let mut records = records.into_values().collect::<Vec<_>>();
    records.sort_by(|left, right| left.sync_id().cmp(right.sync_id()));
    records
}

pub fn is_sane_sync_timestamp(timestamp: i64, now: i64) -> bool {
    timestamp >= 0 && timestamp <= now.saturating_add(MAX_FUTURE_SKEW_MS)
}

pub fn prune_expired_tombstones<T: SyncRecord + Clone>(
    records: &[T],
    now: i64,
    ttl_ms: i64,
) -> Vec<T> {
    records
        .iter()
        .filter(|record| {
            record
                .deleted_at()
                .is_none_or(|deleted_at| now.saturating_sub(deleted_at) <= ttl_ms)
        })
        .cloned()
        .collect()
}

pub fn expired_tombstone_ids_to_delete<T: SyncRecord>(
    existing: &[T],
    keep_ids: &HashSet<String>,
    now: i64,
    ttl_ms: i64,
) -> Vec<String> {
    existing
        .iter()
        .filter(|record| !keep_ids.contains(record.sync_id()))
        .filter(|record| {
            record
                .deleted_at()
                .is_some_and(|deleted_at| now.saturating_sub(deleted_at) > ttl_ms)
        })
        .map(|record| record.sync_id().to_owned())
        .collect()
}

pub fn canonical_records_equal<T: SyncRecord + Serialize>(
    left: &[T],
    right: &[T],
) -> Result<bool, serde_json::Error> {
    Ok(canonical_records(left)? == canonical_records(right)?)
}

fn canonical_records<T: SyncRecord + Serialize>(
    records: &[T],
) -> Result<BTreeMap<String, serde_json::Value>, serde_json::Error> {
    records
        .iter()
        .map(|record| Ok((record.sync_id().to_owned(), serde_json::to_value(record)?)))
        .collect()
}

fn scope_info(scope: SyncScope, partition: u16) -> (String, String) {
    let (id, encryption) = match scope {
        SyncScope::Subscriptions => ("wonita-sync-id-v1", "wonita-sync-enc-v1"),
        SyncScope::Notes => ("wonita-sync-notes-id-v1", "wonita-sync-notes-enc-v1"),
        SyncScope::Bookmarks => (
            "wonita-sync-bookmarks-id-v1",
            "wonita-sync-bookmarks-enc-v1",
        ),
    };
    if partition == 0 {
        (id.into(), encryption.into())
    } else {
        (
            format!("{id}:partition:{partition}"),
            format!("{encryption}:partition:{partition}"),
        )
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut result, "{byte:02x}").expect("writing to a String cannot fail");
    }
    result
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ideall_protocol::{BaseNode, Node};
    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
    struct Record {
        id: String,
        updated_at: i64,
        deleted_at: Option<i64>,
        value: String,
    }

    impl SyncRecord for Record {
        fn sync_id(&self) -> &str {
            &self.id
        }
        fn updated_at(&self) -> i64 {
            self.updated_at
        }
        fn deleted_at(&self) -> Option<i64> {
            self.deleted_at
        }
    }

    fn record(id: &str, updated_at: i64, value: &str) -> Record {
        Record {
            id: id.into(),
            updated_at,
            deleted_at: None,
            value: value.into(),
        }
    }

    #[test]
    fn key_derivation_matches_web_crypto_protocol() {
        let code = "00112233-44556677-8899aabb-ccddeeff";
        let expected = [
            (SyncScope::Subscriptions, "1c5c452365ba1ab0f24cfacc49849ea7"),
            (SyncScope::Notes, "c9291d90b6b75be17a6ef13b5dabcddb"),
            (SyncScope::Bookmarks, "d6f27c7fe22a93c9c8efb03f62e510f0"),
        ];
        for (scope, storage_id) in expected {
            assert_eq!(derive_keys(code, scope, 0).unwrap().storage_id, storage_id);
        }
    }

    #[test]
    fn encryption_round_trips_and_rejects_tampering() {
        let keys = derive_keys("00112233445566778899aabbccddeeff", SyncScope::Notes, 0).unwrap();
        let encrypted = keys.encrypt_json(&vec!["秘密"], 1_024).unwrap();
        assert!(!encrypted.ciphertext.contains("秘密"));
        assert_eq!(
            keys.decrypt_json::<Vec<String>>(&encrypted, 1_024, 2_048)
                .unwrap(),
            vec!["秘密"]
        );
        let mut tampered = encrypted;
        let replacement = if tampered.ciphertext.starts_with('A') {
            "B"
        } else {
            "A"
        };
        tampered.ciphertext.replace_range(0..1, replacement);
        assert!(keys.decrypt_bytes(&tampered, None).is_err());
    }

    #[test]
    fn partitioned_snapshot_round_trips_with_v2_manifest() {
        let code = "00112233445566778899aabbccddeeff";
        let records = vec![record(
            "large",
            2,
            &"跨端正文".repeat(SYNC_PART_MAX_PLAINTEXT_BYTES / 8),
        )];
        let budget = SyncBlockBudget {
            max_records: 10,
            max_plaintext_bytes: 512 * 1024,
        };
        let prepared = prepare_snapshot_with_generation(
            code,
            SyncScope::Notes,
            &records,
            budget,
            "00112233445566778899aabbccddeeff".into(),
        )
        .unwrap();
        assert!(prepared.parts.len() > 1);
        let manifest = prepared.manifest(7, 42);
        assert_eq!(
            serde_json::to_value(&manifest).unwrap()["part_count"],
            prepared.parts.len()
        );
        let decoded: Vec<Record> =
            decrypt_snapshot(code, SyncScope::Notes, &manifest, &prepared.parts, budget).unwrap();
        assert_eq!(decoded, records);
    }

    #[test]
    fn partitioned_snapshot_rejects_tampering_and_generation_changes() {
        let code = "00112233445566778899aabbccddeeff";
        let budget = SyncBlockBudget {
            max_records: 10,
            max_plaintext_bytes: 1_024,
        };
        let prepared = prepare_snapshot_with_generation(
            code,
            SyncScope::Bookmarks,
            &[record("bookmark", 1, "https://example.com")],
            budget,
            "00112233445566778899aabbccddeeff".into(),
        )
        .unwrap();
        let manifest = prepared.manifest(1, 1);

        let mut tampered = prepared.parts.clone();
        tampered[0].ciphertext.replace_range(0..1, "A");
        assert!(matches!(
            decrypt_snapshot::<Record>(code, SyncScope::Bookmarks, &manifest, &tampered, budget),
            Err(SyncCryptoError::DigestMismatch)
        ));

        assert!(matches!(
            decrypt_snapshot::<Record>(
                code,
                SyncScope::Bookmarks,
                &manifest,
                &prepared.parts[..0],
                budget
            ),
            Err(SyncCryptoError::GenerationChanged)
        ));
    }

    #[test]
    fn part_digests_match_the_web_sha256_construction() {
        let part = SyncGenerationPart {
            generation: "00112233445566778899aabbccddeeff".into(),
            part_index: 0,
            iv: "abc".into(),
            ciphertext: "xyz".into(),
            content_sha256: "06ad82869dec2100e17aa84a8e20a3473ea0760703ca78b1f193d139fec0711d"
                .into(),
        };
        assert_eq!(
            part_content_digest(&part.iv, &part.ciphertext),
            part.content_sha256
        );
        assert_eq!(
            parts_digest(&[part]),
            "acbd6733571e795a3e8c63610b9d6a6e9969454f0d6ea1f6e605ee8cee3a7b2f"
        );
    }

    #[test]
    fn merge_is_lww_with_local_winning_ties() {
        let local = vec![record("same", 2, "local"), record("local", 1, "only")];
        let remote = vec![record("same", 2, "remote"), record("remote", 3, "only")];
        let merged = union_merge(&local, &remote);
        assert_eq!(merged.len(), 3);
        assert_eq!(
            merged.iter().find(|item| item.id == "same").unwrap().value,
            "local"
        );
    }

    #[test]
    fn tombstone_gc_keeps_live_and_recent_records() {
        let now = TOMBSTONE_TTL_MS + 10;
        let mut expired = record("expired", 1, "x");
        expired.deleted_at = Some(1);
        let mut recent = record("recent", now, "x");
        recent.deleted_at = Some(now);
        let kept = prune_expired_tombstones(
            &[expired, recent.clone(), record("live", 1, "x")],
            now,
            TOMBSTONE_TTL_MS,
        );
        assert_eq!(kept.len(), 2);
        assert!(kept.contains(&recent));
    }

    #[test]
    fn protocol_nodes_participate_without_ui_or_storage_types() {
        let node = Node::Folder {
            base: BaseNode {
                id: "folder".into(),
                parent_id: None,
                sort_key: "a".into(),
                title: "Folder".into(),
                tags: Vec::new(),
                created_at: 1,
                updated_at: 2,
                deleted_at: None,
                meta: BTreeMap::new(),
            },
            content: None,
        };
        assert_eq!(node.sync_id(), "folder");
    }
}
