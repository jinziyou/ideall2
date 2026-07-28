use std::collections::HashSet;

use serde::{Serialize, de::DeserializeOwned};
use thiserror::Error;

use crate::{
    SyncBlockBudget, SyncCryptoError, SyncGenerationPart, SyncManifest, SyncRecord, SyncScope,
    TOMBSTONE_TTL_MS, canonical_records_equal, decrypt_snapshot, is_sane_sync_timestamp,
    prepare_snapshot, prune_expired_tombstones, union_merge,
};

pub const SYNC_MAX_ATTEMPTS: usize = 4;

#[derive(Clone, Debug, Eq, PartialEq, Error)]
#[error("{message}")]
pub struct TransportError {
    pub status: Option<u16>,
    pub message: String,
}

impl TransportError {
    pub fn new(status: Option<u16>, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    fn is_conflict(&self) -> bool {
        self.status == Some(409)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum LocalStoreError {
    #[error("local data changed during synchronization")]
    Conflict,
    #[error("local synchronization storage failed: {0}")]
    Other(String),
}

pub trait SyncTransport {
    fn get_manifest(&mut self, storage_id: &str) -> Result<Option<SyncManifest>, TransportError>;

    fn get_part(
        &mut self,
        storage_id: &str,
        generation: &str,
        part_index: usize,
    ) -> Result<SyncGenerationPart, TransportError>;

    fn put_part(
        &mut self,
        storage_id: &str,
        generation: &str,
        part: &SyncGenerationPart,
    ) -> Result<(), TransportError>;

    fn commit_manifest(
        &mut self,
        storage_id: &str,
        generation: &str,
        part_count: usize,
        expected_version: u64,
    ) -> Result<SyncManifest, TransportError>;

    fn discard_generation(
        &mut self,
        storage_id: &str,
        generation: &str,
    ) -> Result<(), TransportError>;
}

pub trait SyncLocalStore<T> {
    fn list_all(&mut self) -> Result<Vec<T>, LocalStoreError>;

    /// Writes only when the current logical snapshot still equals `expected`.
    fn compare_and_swap(
        &mut self,
        records: &[T],
        expected: &[T],
    ) -> Result<Vec<T>, LocalStoreError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyncRunResult {
    pub total: usize,
    pub added: usize,
    pub attempts: usize,
}

#[derive(Debug, Error)]
pub enum SyncRunError {
    #[error(transparent)]
    Crypto(#[from] SyncCryptoError),
    #[error(transparent)]
    Transport(#[from] TransportError),
    #[error(transparent)]
    Local(#[from] LocalStoreError),
    #[error("remote synchronization records failed validation")]
    InvalidRemote,
    #[error("synchronization kept conflicting after {SYNC_MAX_ATTEMPTS} attempts")]
    ConflictExhausted,
}

struct RemoteSnapshot<T> {
    records: Vec<T>,
    version: u64,
    dirty: bool,
}

pub fn run_sync<T, S, R, V>(
    code: &str,
    scope: SyncScope,
    budget: SyncBlockBudget,
    now_ms: i64,
    store: &mut S,
    transport: &mut R,
    is_valid_remote: V,
) -> Result<SyncRunResult, SyncRunError>
where
    T: SyncRecord + Clone + Serialize + DeserializeOwned,
    S: SyncLocalStore<T>,
    R: SyncTransport,
    V: Fn(&T, i64) -> bool,
{
    run_sync_with(
        code,
        scope,
        budget,
        now_ms,
        store,
        transport,
        is_valid_remote,
        union_merge,
        |records, now| prune_expired_tombstones(records, now, TOMBSTONE_TTL_MS),
    )
}

#[allow(clippy::too_many_arguments)]
pub fn run_sync_with<T, S, R, V, M, G>(
    code: &str,
    scope: SyncScope,
    budget: SyncBlockBudget,
    now_ms: i64,
    store: &mut S,
    transport: &mut R,
    is_valid_remote: V,
    merge: M,
    gc: G,
) -> Result<SyncRunResult, SyncRunError>
where
    T: SyncRecord + Clone + Serialize + DeserializeOwned,
    S: SyncLocalStore<T>,
    R: SyncTransport,
    V: Fn(&T, i64) -> bool,
    M: Fn(&[T], &[T]) -> Vec<T>,
    G: Fn(&[T], i64) -> Vec<T>,
{
    let root_keys = crate::derive_keys(code, scope, 0)?;
    let storage_id = root_keys.storage_id;
    let local_all = store.list_all()?;
    validate_snapshot_budget(&local_all, budget)?;
    let initial_live = local_all
        .iter()
        .filter(|record| record.deleted_at().is_none())
        .map(|record| record.sync_id().to_owned())
        .collect::<HashSet<_>>();
    let mut local_snapshot = local_all.clone();
    let mut accumulated = local_all.clone();

    for attempt in 1..=SYNC_MAX_ATTEMPTS {
        let remote = match load_remote::<T, _, _>(
            code,
            scope,
            budget,
            &storage_id,
            transport,
            now_ms,
            &is_valid_remote,
        ) {
            Ok(remote) => remote,
            Err(SyncRunError::Transport(error)) if error.status == Some(404) => {
                if attempt == SYNC_MAX_ATTEMPTS {
                    return Err(SyncRunError::ConflictExhausted);
                }
                continue;
            }
            Err(error) => return Err(error),
        };
        let merged = merge(&accumulated, &remote.records);
        let kept = gc(&merged, now_ms);
        validate_snapshot_budget(&kept, budget)?;

        if !records_equal(&kept, &local_snapshot)? {
            match store.compare_and_swap(&kept, &local_snapshot) {
                Ok(committed) => local_snapshot = committed,
                Err(LocalStoreError::Conflict) => {
                    if attempt == SYNC_MAX_ATTEMPTS {
                        return Err(SyncRunError::ConflictExhausted);
                    }
                    let current = store.list_all()?;
                    accumulated = merge(&current, &kept);
                    local_snapshot = current;
                    continue;
                }
                Err(error) => return Err(error.into()),
            }
        }

        if !remote.dirty && records_equal(&local_snapshot, &remote.records)? {
            return Ok(count_result(&initial_live, &local_snapshot, attempt));
        }

        match publish_snapshot(
            code,
            scope,
            budget,
            &storage_id,
            &local_snapshot,
            remote.version,
            transport,
        ) {
            Ok(()) => return Ok(count_result(&initial_live, &local_snapshot, attempt)),
            Err(SyncRunError::Transport(error)) if error.is_conflict() => {
                if attempt == SYNC_MAX_ATTEMPTS {
                    return Err(SyncRunError::ConflictExhausted);
                }
                accumulated = local_snapshot.clone();
            }
            Err(error) => return Err(error),
        }
    }
    Err(SyncRunError::ConflictExhausted)
}

fn load_remote<T, R, V>(
    code: &str,
    scope: SyncScope,
    budget: SyncBlockBudget,
    storage_id: &str,
    transport: &mut R,
    now_ms: i64,
    is_valid_remote: &V,
) -> Result<RemoteSnapshot<T>, SyncRunError>
where
    T: SyncRecord + Clone + DeserializeOwned,
    R: SyncTransport,
    V: Fn(&T, i64) -> bool,
{
    let Some(manifest) = transport.get_manifest(storage_id)? else {
        return Ok(RemoteSnapshot {
            records: Vec::new(),
            version: 0,
            dirty: false,
        });
    };
    let mut parts = Vec::with_capacity(manifest.part_count);
    for part_index in 0..manifest.part_count {
        parts.push(transport.get_part(storage_id, &manifest.generation, part_index)?);
    }
    let decoded = decrypt_snapshot(code, scope, &manifest, &parts, budget)?;
    let decoded_len = decoded.len();
    let records = decoded
        .into_iter()
        .filter(|record| {
            is_valid_remote(record, now_ms)
                && is_sane_sync_timestamp(record.updated_at(), now_ms)
                && record
                    .deleted_at()
                    .is_none_or(|timestamp| is_sane_sync_timestamp(timestamp, now_ms))
        })
        .collect::<Vec<_>>();
    Ok(RemoteSnapshot {
        dirty: records.len() != decoded_len,
        records,
        version: manifest.version,
    })
}

fn publish_snapshot<T, R>(
    code: &str,
    scope: SyncScope,
    budget: SyncBlockBudget,
    storage_id: &str,
    records: &[T],
    expected_version: u64,
    transport: &mut R,
) -> Result<(), SyncRunError>
where
    T: Serialize,
    R: SyncTransport,
{
    let prepared = prepare_snapshot(code, scope, records, budget)?;
    for part in &prepared.parts {
        if let Err(error) = transport.put_part(storage_id, &prepared.generation, part) {
            let _ = transport.discard_generation(storage_id, &prepared.generation);
            return Err(error.into());
        }
    }
    match transport.commit_manifest(
        storage_id,
        &prepared.generation,
        prepared.parts.len(),
        expected_version,
    ) {
        Ok(manifest)
            if manifest.generation == prepared.generation
                && manifest.part_count == prepared.parts.len()
                && manifest.version > expected_version =>
        {
            Ok(())
        }
        Ok(_) => {
            Err(TransportError::new(None, "server returned an invalid committed manifest").into())
        }
        Err(error)
            if error
                .status
                .is_some_and(|status| (400..500).contains(&status)) =>
        {
            let _ = transport.discard_generation(storage_id, &prepared.generation);
            Err(error.into())
        }
        Err(error) => {
            let observed = transport.get_manifest(storage_id);
            match observed {
                Ok(Some(manifest)) if manifest.generation == prepared.generation => {
                    if manifest.part_count == prepared.parts.len()
                        && manifest.version > expected_version
                    {
                        Ok(())
                    } else {
                        Err(error.into())
                    }
                }
                Ok(Some(manifest)) if manifest.version != expected_version => {
                    let _ = transport.discard_generation(storage_id, &prepared.generation);
                    Err(TransportError::new(Some(409), "sync manifest changed").into())
                }
                Ok(_) => {
                    let _ = transport.discard_generation(storage_id, &prepared.generation);
                    Err(error.into())
                }
                Err(_) => Err(error.into()),
            }
        }
    }
}

fn validate_snapshot_budget<T: Serialize>(
    records: &[T],
    budget: SyncBlockBudget,
) -> Result<(), SyncRunError> {
    if records.len() > budget.max_records {
        return Err(SyncCryptoError::RecordLimit.into());
    }
    if serde_json::to_vec(records)
        .map_err(SyncCryptoError::from)?
        .len()
        > budget.max_plaintext_bytes
    {
        return Err(SyncCryptoError::BlockLimit.into());
    }
    Ok(())
}

fn records_equal<T: SyncRecord + Serialize>(left: &[T], right: &[T]) -> Result<bool, SyncRunError> {
    canonical_records_equal(left, right)
        .map_err(SyncCryptoError::from)
        .map_err(Into::into)
}

fn count_result<T: SyncRecord>(
    initial_live: &HashSet<String>,
    records: &[T],
    attempts: usize,
) -> SyncRunResult {
    let live = records
        .iter()
        .filter(|record| record.deleted_at().is_none())
        .collect::<Vec<_>>();
    SyncRunResult {
        total: live.len(),
        added: live
            .iter()
            .filter(|record| !initial_live.contains(record.sync_id()))
            .count(),
        attempts,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::{PreparedSnapshot, prepare_snapshot_with_generation};

    const CODE: &str = "00112233445566778899aabbccddeeff";
    const BUDGET: SyncBlockBudget = SyncBlockBudget {
        max_records: 100,
        max_plaintext_bytes: 128 * 1024,
    };

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

    #[derive(Default)]
    struct MemoryStore {
        records: Vec<Record>,
        conflict_once: Option<Record>,
    }

    impl SyncLocalStore<Record> for MemoryStore {
        fn list_all(&mut self) -> Result<Vec<Record>, LocalStoreError> {
            Ok(self.records.clone())
        }

        fn compare_and_swap(
            &mut self,
            records: &[Record],
            expected: &[Record],
        ) -> Result<Vec<Record>, LocalStoreError> {
            if let Some(concurrent) = self.conflict_once.take() {
                self.records.push(concurrent);
                return Err(LocalStoreError::Conflict);
            }
            if !records_equal(&self.records, expected)
                .map_err(|error| LocalStoreError::Other(error.to_string()))?
            {
                return Err(LocalStoreError::Conflict);
            }
            self.records = records.to_vec();
            Ok(self.records.clone())
        }
    }

    #[derive(Default)]
    struct MemoryTransport {
        manifest: Option<SyncManifest>,
        parts: BTreeMap<(String, usize), SyncGenerationPart>,
        commit_conflict_once: bool,
        ambiguous_commit_once: bool,
        commit_calls: usize,
    }

    impl MemoryTransport {
        fn with_snapshot(records: &[Record]) -> Self {
            let prepared = prepare_snapshot_with_generation(
                CODE,
                SyncScope::Notes,
                records,
                BUDGET,
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            )
            .unwrap();
            let manifest = prepared.manifest(1, 10);
            let parts = prepared
                .parts
                .into_iter()
                .map(|part| ((part.generation.clone(), part.part_index), part))
                .collect();
            Self {
                manifest: Some(manifest),
                parts,
                ..Self::default()
            }
        }

        fn commit_prepared(
            &mut self,
            generation: &str,
            part_count: usize,
            expected_version: u64,
        ) -> Result<SyncManifest, TransportError> {
            let parts = (0..part_count)
                .map(|index| {
                    self.parts
                        .get(&(generation.to_owned(), index))
                        .cloned()
                        .ok_or_else(|| TransportError::new(Some(422), "missing staged part"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let prepared = PreparedSnapshot {
                generation: generation.into(),
                total_ciphertext_chars: parts.iter().map(|part| part.ciphertext.len()).sum(),
                parts_sha256: crate::parts_digest(&parts),
                parts,
            };
            let manifest = prepared.manifest(expected_version + 1, 20);
            self.manifest = Some(manifest.clone());
            Ok(manifest)
        }
    }

    impl SyncTransport for MemoryTransport {
        fn get_manifest(
            &mut self,
            _storage_id: &str,
        ) -> Result<Option<SyncManifest>, TransportError> {
            Ok(self.manifest.clone())
        }

        fn get_part(
            &mut self,
            _storage_id: &str,
            generation: &str,
            part_index: usize,
        ) -> Result<SyncGenerationPart, TransportError> {
            self.parts
                .get(&(generation.to_owned(), part_index))
                .cloned()
                .ok_or_else(|| TransportError::new(Some(404), "part disappeared"))
        }

        fn put_part(
            &mut self,
            _storage_id: &str,
            generation: &str,
            part: &SyncGenerationPart,
        ) -> Result<(), TransportError> {
            self.parts
                .insert((generation.to_owned(), part.part_index), part.clone());
            Ok(())
        }

        fn commit_manifest(
            &mut self,
            _storage_id: &str,
            generation: &str,
            part_count: usize,
            expected_version: u64,
        ) -> Result<SyncManifest, TransportError> {
            self.commit_calls += 1;
            if self.commit_conflict_once {
                self.commit_conflict_once = false;
                return Err(TransportError::new(Some(409), "concurrent device"));
            }
            let current = self
                .manifest
                .as_ref()
                .map_or(0, |manifest| manifest.version);
            if current != expected_version {
                return Err(TransportError::new(Some(409), "stale manifest"));
            }
            let manifest = self.commit_prepared(generation, part_count, expected_version)?;
            if self.ambiguous_commit_once {
                self.ambiguous_commit_once = false;
                return Err(TransportError::new(None, "response was lost"));
            }
            Ok(manifest)
        }

        fn discard_generation(
            &mut self,
            _storage_id: &str,
            generation: &str,
        ) -> Result<(), TransportError> {
            if self
                .manifest
                .as_ref()
                .map(|manifest| manifest.generation.as_str())
                != Some(generation)
            {
                self.parts
                    .retain(|(stored_generation, _), _| stored_generation != generation);
            }
            Ok(())
        }
    }

    fn valid(_: &Record, _: i64) -> bool {
        true
    }

    #[test]
    fn uploads_local_snapshot_then_becomes_a_read_only_noop() {
        let mut store = MemoryStore {
            records: vec![record("local", 1, "draft")],
            ..MemoryStore::default()
        };
        let mut transport = MemoryTransport::default();
        let first = run_sync(
            CODE,
            SyncScope::Notes,
            BUDGET,
            100,
            &mut store,
            &mut transport,
            valid,
        )
        .unwrap();
        assert_eq!(
            first,
            SyncRunResult {
                total: 1,
                added: 0,
                attempts: 1
            }
        );
        assert_eq!(transport.commit_calls, 1);

        let second = run_sync(
            CODE,
            SyncScope::Notes,
            BUDGET,
            100,
            &mut store,
            &mut transport,
            valid,
        )
        .unwrap();
        assert_eq!(second.total, 1);
        assert_eq!(transport.commit_calls, 1);
    }

    #[test]
    fn downloads_remote_records_through_local_cas() {
        let remote = record("remote", 2, "from phone");
        let mut store = MemoryStore::default();
        let mut transport = MemoryTransport::with_snapshot(std::slice::from_ref(&remote));
        let result = run_sync(
            CODE,
            SyncScope::Notes,
            BUDGET,
            100,
            &mut store,
            &mut transport,
            valid,
        )
        .unwrap();
        assert_eq!(result.added, 1);
        assert_eq!(store.records, vec![remote]);
        assert_eq!(transport.commit_calls, 0);
    }

    #[test]
    fn retries_remote_cas_and_preserves_a_concurrent_local_write() {
        let mut store = MemoryStore {
            records: vec![record("first", 1, "one")],
            conflict_once: Some(record("concurrent", 2, "two")),
        };
        let mut transport = MemoryTransport::with_snapshot(&[record("remote", 1, "three")]);
        transport.commit_conflict_once = true;
        let result = run_sync(
            CODE,
            SyncScope::Notes,
            BUDGET,
            100,
            &mut store,
            &mut transport,
            valid,
        )
        .unwrap();
        assert!(result.attempts >= 2);
        assert_eq!(store.records.len(), 3);
        assert!(store.records.iter().any(|record| record.id == "concurrent"));
    }

    #[test]
    fn recognizes_a_commit_whose_success_response_was_lost() {
        let mut store = MemoryStore {
            records: vec![record("local", 1, "draft")],
            ..MemoryStore::default()
        };
        let mut transport = MemoryTransport {
            ambiguous_commit_once: true,
            ..MemoryTransport::default()
        };
        let result = run_sync(
            CODE,
            SyncScope::Notes,
            BUDGET,
            100,
            &mut store,
            &mut transport,
            valid,
        )
        .unwrap();
        assert_eq!(result.attempts, 1);
        assert!(transport.manifest.is_some());
    }
}
