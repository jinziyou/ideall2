use std::collections::HashSet;
use std::path::Path;

use ideall_protocol::{Node, NodeKind};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use thiserror::Error;

use crate::{ArchiveBlob, ArchivePluginPackage, ArchiveTrashSnapshot, ParsedArchive};

const SCHEMA_VERSION: i64 = 3;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("stored JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported native database schema version {0}")]
    UnsupportedSchema(i64),
    #[error("archive contains duplicate {kind} id `{id}`")]
    DuplicateId { kind: &'static str, id: String },
    #[error("file node `{node_id}` refers to missing blob `{blob_key}`")]
    MissingBlob { node_id: String, blob_key: String },
    #[error("trash snapshot `{snapshot_id}` does not match node `{node_id}`")]
    InvalidTrashSnapshot {
        snapshot_id: String,
        node_id: String,
    },
    #[error("SQLite integrity check failed: {0}")]
    Integrity(String),
    #[error("local synchronization snapshot changed before commit")]
    SnapshotConflict,
    #[error("Agent audit outbox is full of pending intents")]
    AgentAuditFull,
    #[error("Agent audit record changed before completion")]
    AgentAuditConflict,
}

pub struct Database {
    connection: Connection,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeProjection {
    pub id: String,
    pub parent_id: Option<String>,
    pub sort_key: String,
    pub kind: NodeKind,
    pub title: String,
    pub updated_at: i64,
    pub deleted: bool,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let connection = Connection::open(path)?;
        Self::from_connection(connection, true)
    }

    pub fn open_in_memory() -> Result<Self, StorageError> {
        let connection = Connection::open_in_memory()?;
        Self::from_connection(connection, false)
    }

    fn from_connection(mut connection: Connection, persistent: bool) -> Result<Self, StorageError> {
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        if persistent {
            connection.pragma_update(None, "journal_mode", "WAL")?;
        }
        migrate(&mut connection)?;
        Ok(Self { connection })
    }

    pub fn schema_version(&self) -> Result<i64, StorageError> {
        Ok(self
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))?)
    }

    pub fn replace_from_archive(&mut self, archive: &ParsedArchive) -> Result<(), StorageError> {
        validate_archive_references(archive)?;
        let transaction = self.connection.transaction()?;
        for table in [
            "node_search",
            "nodes",
            "blobs",
            "trash_snapshots",
            "workspace_state",
            "plugin_data",
            "agent_tasks",
            "agent_write_audit",
            "local_search_index",
            "local_semantic_index",
        ] {
            transaction.execute(&format!("DELETE FROM {table}"), [])?;
        }

        for node in &archive.nodes {
            insert_node(&transaction, node)?;
        }
        for blob in &archive.blobs {
            insert_blob(&transaction, blob)?;
        }
        for snapshot in &archive.trash_snapshots {
            insert_trash_snapshot(&transaction, snapshot)?;
        }
        if let Some(workspace) = &archive.workspace {
            transaction.execute(
                "INSERT INTO workspace_state (id, state_json) VALUES (1, ?1)",
                [serde_json::to_string(workspace)?],
            )?;
        }
        for plugin in &archive.plugins {
            insert_plugin(&transaction, plugin)?;
        }
        transaction.commit()?;
        self.quick_check()
    }

    pub fn get_node(&self, id: &str) -> Result<Option<Node>, StorageError> {
        let raw = self
            .connection
            .query_row(
                "SELECT document_json FROM nodes WHERE id = ?1",
                [id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        raw.map(|raw| serde_json::from_str(&raw).map_err(StorageError::from))
            .transpose()
    }

    pub fn put_node(&mut self, node: &Node) -> Result<(), StorageError> {
        upsert_node(&self.connection, node)
    }

    pub fn put_file(&mut self, node: &Node, blob: &ArchiveBlob) -> Result<(), StorageError> {
        let Node::File { blob_ref, .. } = node else {
            return Err(StorageError::Integrity(
                "put_file requires a file node".into(),
            ));
        };
        if blob_ref.key != blob.key || blob_ref.size != blob.data.len() as u64 {
            return Err(StorageError::Integrity(
                "file node and blob metadata do not match".into(),
            ));
        }
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO blobs (key, mime, size, data) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(key) DO UPDATE SET
                mime = excluded.mime, size = excluded.size, data = excluded.data",
            params![blob.key, blob.mime, blob.data.len() as i64, blob.data],
        )?;
        upsert_node(&transaction, node)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn list_children(
        &self,
        parent_id: Option<&str>,
        include_deleted: bool,
    ) -> Result<Vec<Node>, StorageError> {
        let mut sql = String::from("SELECT document_json FROM nodes WHERE parent_id IS ?1");
        if !include_deleted {
            sql.push_str(" AND deleted_at IS NULL");
        }
        sql.push_str(" ORDER BY sort_key, id");
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map([parent_id], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            let raw = row?;
            serde_json::from_str(&raw).map_err(StorageError::from)
        })
        .collect()
    }

    pub fn remove_node_permanently(&mut self, id: &str) -> Result<bool, StorageError> {
        let transaction = self.connection.transaction()?;
        let blob_key = transaction
            .query_row(
                "SELECT json_extract(document_json, '$.blobRef.key')
                 FROM nodes WHERE id = ?1 AND kind = 'file'",
                [id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
        let changed = transaction.execute("DELETE FROM nodes WHERE id = ?1", [id])? > 0;
        transaction.execute("DELETE FROM node_search WHERE node_id = ?1", [id])?;
        transaction.execute("DELETE FROM trash_snapshots WHERE id = ?1", [id])?;
        if let Some(blob_key) = blob_key {
            transaction.execute("DELETE FROM blobs WHERE key = ?1", [blob_key])?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    pub fn move_to_trash(&mut self, id: &str, deleted_at: i64) -> Result<bool, StorageError> {
        let transaction = self.connection.transaction()?;
        let Some(raw) = transaction
            .query_row(
                "SELECT document_json FROM nodes WHERE id = ?1 AND deleted_at IS NULL",
                [id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        else {
            return Ok(false);
        };
        let mut node: Node = serde_json::from_str(&raw)?;
        let (blob_key, blob_mime, blob_data) = if let Node::File { blob_ref, .. } = &node {
            transaction
                .query_row(
                    "SELECT key, mime, data FROM blobs WHERE key = ?1",
                    [&blob_ref.key],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Vec<u8>>(2)?,
                        ))
                    },
                )
                .optional()?
                .map_or((None, None, None), |(key, mime, data)| {
                    (Some(key), Some(mime), Some(data))
                })
        } else {
            (None, None, None)
        };
        transaction.execute(
            "INSERT INTO trash_snapshots (
                id, node_json, blob_key, blob_mime, blob_data, captured_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                node_json = excluded.node_json,
                blob_key = excluded.blob_key,
                blob_mime = excluded.blob_mime,
                blob_data = excluded.blob_data,
                captured_at = excluded.captured_at",
            params![id, raw, blob_key, blob_mime, blob_data, deleted_at],
        )?;
        node.base_mut().deleted_at = Some(deleted_at);
        node.base_mut().updated_at = deleted_at;
        upsert_node(&transaction, &node)?;
        transaction.commit()?;
        Ok(true)
    }

    pub fn restore_from_trash(&mut self, id: &str, restored_at: i64) -> Result<bool, StorageError> {
        let transaction = self.connection.transaction()?;
        let snapshot = transaction
            .query_row(
                "SELECT node_json, blob_key, blob_mime, blob_data
                 FROM trash_snapshots WHERE id = ?1",
                [id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<Vec<u8>>>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((raw, blob_key, blob_mime, blob_data)) = snapshot else {
            return Ok(false);
        };
        let mut node: Node = serde_json::from_str(&raw)?;
        node.base_mut().deleted_at = None;
        node.base_mut().updated_at = restored_at;
        if let (Some(key), Some(mime), Some(data)) = (blob_key, blob_mime, blob_data) {
            transaction.execute(
                "INSERT INTO blobs (key, mime, size, data) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(key) DO UPDATE SET
                    mime = excluded.mime, size = excluded.size, data = excluded.data",
                params![key, mime, data.len() as i64, data],
            )?;
        }
        upsert_node(&transaction, &node)?;
        transaction.execute("DELETE FROM trash_snapshots WHERE id = ?1", [id])?;
        transaction.commit()?;
        Ok(true)
    }

    pub fn list_nodes(&self, include_deleted: bool) -> Result<Vec<Node>, StorageError> {
        let sql = if include_deleted {
            "SELECT document_json FROM nodes ORDER BY parent_id, sort_key, id"
        } else {
            "SELECT document_json FROM nodes WHERE deleted_at IS NULL ORDER BY parent_id, sort_key, id"
        };
        let mut statement = self.connection.prepare(sql)?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            let raw = row?;
            serde_json::from_str(&raw).map_err(StorageError::from)
        })
        .collect()
    }

    /// Reads only the columns needed by navigation and tree rendering.
    ///
    /// Listing a large workspace must not deserialize note bodies, file
    /// metadata, or Agent thread documents merely to draw the sidebar.
    pub fn list_node_projections(
        &self,
        include_deleted: bool,
    ) -> Result<Vec<NodeProjection>, StorageError> {
        let sql = if include_deleted {
            "SELECT id, parent_id, sort_key, kind, title, updated_at, deleted_at
             FROM nodes ORDER BY parent_id, sort_key, id"
        } else {
            "SELECT id, parent_id, sort_key, kind, title, updated_at, deleted_at
             FROM nodes WHERE deleted_at IS NULL ORDER BY parent_id, sort_key, id"
        };
        let mut statement = self.connection.prepare(sql)?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, Option<i64>>(6)?,
            ))
        })?;
        rows.map(|row| {
            let (id, parent_id, sort_key, kind, title, updated_at, deleted_at) = row?;
            Ok(NodeProjection {
                id,
                parent_id,
                sort_key,
                kind: parse_node_kind(&kind)?,
                title,
                updated_at,
                deleted: deleted_at.is_some(),
            })
        })
        .collect()
    }

    /// Atomically replaces one logical synchronization domain when its current
    /// snapshot still equals `expected`. The predicate must be deterministic
    /// and may not capture mutable state.
    pub fn compare_and_swap_nodes<F>(
        &mut self,
        records: &[Node],
        expected: &[Node],
        in_scope: F,
    ) -> Result<Vec<Node>, StorageError>
    where
        F: Fn(&Node) -> bool,
    {
        self.compare_and_swap_nodes_projected(records, expected, in_scope, Clone::clone)
    }

    /// Variant for legacy wire domains whose local representation contains
    /// additional fields. `project` must retain node identity and kind and is
    /// used only for the optimistic snapshot comparison.
    pub fn compare_and_swap_nodes_projected<F, P>(
        &mut self,
        records: &[Node],
        expected: &[Node],
        in_scope: F,
        project: P,
    ) -> Result<Vec<Node>, StorageError>
    where
        F: Fn(&Node) -> bool,
        P: Fn(&Node) -> Node,
    {
        if records.iter().any(|node| !in_scope(node)) || expected.iter().any(|node| !in_scope(node))
        {
            return Err(StorageError::Integrity(
                "synchronization batch escaped its node scope".into(),
            ));
        }
        validate_unique_nodes(records)?;
        validate_unique_nodes(expected)?;

        let transaction = self.connection.transaction()?;
        let all_nodes = read_all_nodes(&transaction)?;
        let current_nodes = all_nodes
            .iter()
            .filter(|node| in_scope(node))
            .cloned()
            .collect::<Vec<_>>();
        let mut current = current_nodes.iter().map(&project).collect::<Vec<_>>();
        if current.iter().any(|node| !in_scope(node))
            || current
                .iter()
                .zip(&current_nodes)
                .any(|(projected, source)| {
                    projected.base().id != source.base().id || projected.kind() != source.kind()
                })
        {
            return Err(StorageError::Integrity(
                "synchronization snapshot projection changed node identity or scope".into(),
            ));
        }
        let mut expected = expected.to_vec();
        sort_nodes_by_id(&mut current);
        sort_nodes_by_id(&mut expected);
        if current != expected {
            return Err(StorageError::SnapshotConflict);
        }

        let current_by_id = current_nodes
            .iter()
            .map(|node| (node.base().id.as_str(), node))
            .collect::<std::collections::HashMap<_, _>>();
        let all_by_id = all_nodes
            .iter()
            .map(|node| (node.base().id.as_str(), node))
            .collect::<std::collections::HashMap<_, _>>();
        let keep_ids = records
            .iter()
            .map(|node| node.base().id.as_str())
            .collect::<HashSet<_>>();

        for current in &current_nodes {
            if !keep_ids.contains(current.base().id.as_str()) {
                transaction.execute("DELETE FROM nodes WHERE id = ?1", [&current.base().id])?;
                transaction.execute(
                    "DELETE FROM node_search WHERE node_id = ?1",
                    [&current.base().id],
                )?;
                transaction.execute(
                    "DELETE FROM trash_snapshots WHERE id = ?1",
                    [&current.base().id],
                )?;
            }
        }

        for node in records {
            if let Some(existing) = all_by_id.get(node.base().id.as_str())
                && existing.kind() != node.kind()
            {
                return Err(StorageError::Integrity(format!(
                    "synchronization cannot change node kind for `{}`",
                    node.base().id
                )));
            }
            upsert_node(&transaction, node)?;
            if node.base().deleted_at.is_none() {
                transaction.execute(
                    "DELETE FROM trash_snapshots WHERE id = ?1",
                    [&node.base().id],
                )?;
            } else if current_by_id
                .get(node.base().id.as_str())
                .is_some_and(|current| current.base().deleted_at.is_none())
            {
                let current = current_by_id[node.base().id.as_str()];
                transaction.execute(
                    "INSERT INTO trash_snapshots (
                        id, node_json, blob_key, blob_mime, blob_data, captured_at
                     ) VALUES (?1, ?2, NULL, NULL, NULL, ?3)
                     ON CONFLICT(id) DO UPDATE SET
                        node_json = excluded.node_json,
                        blob_key = NULL,
                        blob_mime = NULL,
                        blob_data = NULL,
                        captured_at = excluded.captured_at",
                    params![
                        node.base().id,
                        serde_json::to_string(current)?,
                        node.base().deleted_at
                    ],
                )?;
            }
        }
        transaction.commit()?;
        let mut committed = records.to_vec();
        sort_nodes_by_id(&mut committed);
        Ok(committed)
    }

    pub fn search_nodes(&self, query: &str, limit: usize) -> Result<Vec<Node>, StorageError> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let limit = limit.clamp(1, 200);
        let mut result = Vec::new();
        let mut seen = HashSet::new();

        if let Some(fts_query) = fts_literal_query(query) {
            let mut statement = self.connection.prepare(
                "SELECT n.document_json
                 FROM node_search AS search
                 JOIN nodes AS n ON n.id = search.node_id
                 WHERE node_search MATCH ?1 AND n.deleted_at IS NULL
                 ORDER BY bm25(node_search), n.updated_at DESC, n.id
                 LIMIT ?2",
            )?;
            let rows = statement.query_map(params![fts_query, limit as i64], |row| {
                row.get::<_, String>(0)
            })?;
            for row in rows {
                let raw = row?;
                let node: Node = serde_json::from_str(&raw)?;
                seen.insert(node.base().id.clone());
                result.push(node);
            }
        }

        if result.len() < limit {
            let pattern = format!("%{}%", escape_like_pattern(query));
            let remaining = limit - result.len();
            let mut statement = self.connection.prepare(
                "SELECT n.document_json
                 FROM node_search AS search
                 JOIN nodes AS n ON n.id = search.node_id
                 WHERE n.deleted_at IS NULL
                   AND (search.title LIKE ?1 ESCAPE '\\' COLLATE NOCASE
                     OR search.body LIKE ?1 ESCAPE '\\' COLLATE NOCASE
                     OR search.tags LIKE ?1 ESCAPE '\\' COLLATE NOCASE)
                 ORDER BY n.updated_at DESC, n.id
                 LIMIT ?2",
            )?;
            let rows = statement
                .query_map(params![pattern, (remaining + seen.len()) as i64], |row| {
                    row.get::<_, String>(0)
                })?;
            for row in rows {
                let raw = row?;
                let node: Node = serde_json::from_str(&raw)?;
                if seen.insert(node.base().id.clone()) {
                    result.push(node);
                    if result.len() == limit {
                        break;
                    }
                }
            }
        }

        Ok(result)
    }

    pub fn get_blob(&self, key: &str) -> Result<Option<(String, Vec<u8>)>, StorageError> {
        Ok(self
            .connection
            .query_row(
                "SELECT mime, data FROM blobs WHERE key = ?1",
                [key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?)
    }

    pub fn workspace_state(&self) -> Result<Option<Value>, StorageError> {
        let raw = self
            .connection
            .query_row(
                "SELECT state_json FROM workspace_state WHERE id = 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        raw.map(|raw| serde_json::from_str(&raw).map_err(StorageError::from))
            .transpose()
    }

    pub fn set_workspace_state<T: Serialize>(&mut self, state: &T) -> Result<(), StorageError> {
        self.connection.execute(
            "INSERT INTO workspace_state (id, state_json) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET state_json = excluded.state_json",
            [serde_json::to_string(state)?],
        )?;
        Ok(())
    }

    pub fn setting<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, StorageError> {
        let raw = self
            .connection
            .query_row(
                "SELECT value_json FROM app_settings WHERE key = ?1",
                [key],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        raw.map(|raw| serde_json::from_str(&raw).map_err(StorageError::from))
            .transpose()
    }

    pub fn set_setting<T: Serialize>(&mut self, key: &str, value: &T) -> Result<(), StorageError> {
        self.connection.execute(
            "INSERT INTO app_settings (key, value_json) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
            params![key, serde_json::to_string(value)?],
        )?;
        Ok(())
    }

    pub fn append_agent_audit<T: Serialize>(
        &mut self,
        id: &str,
        created_at: i64,
        value: &T,
        max_records: usize,
    ) -> Result<(), StorageError> {
        let max_records = max_records.max(1);
        let transaction = self.connection.transaction()?;
        let count: i64 =
            transaction.query_row("SELECT count(*) FROM agent_write_audit", [], |row| {
                row.get(0)
            })?;
        let count = usize::try_from(count).unwrap_or(usize::MAX);
        for _ in max_records..=count {
            let removed = transaction.execute(
                "DELETE FROM agent_write_audit
                 WHERE id = (
                    SELECT id FROM agent_write_audit
                    WHERE json_extract(value_json, '$.status') != 'pending'
                    ORDER BY json_extract(value_json, '$.updatedAt'), id
                    LIMIT 1
                 )",
                [],
            )?;
            if removed == 0 {
                return Err(StorageError::AgentAuditFull);
            }
        }
        transaction.execute(
            "INSERT INTO agent_write_audit (id, created_at, value_json) VALUES (?1, ?2, ?3)",
            params![id, created_at, serde_json::to_string(value)?],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn agent_audit<T: DeserializeOwned>(&self, id: &str) -> Result<Option<T>, StorageError> {
        let raw = self
            .connection
            .query_row(
                "SELECT value_json FROM agent_write_audit WHERE id = ?1",
                [id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        raw.map(|raw| serde_json::from_str(&raw).map_err(StorageError::from))
            .transpose()
    }

    pub fn list_agent_audits<T: DeserializeOwned>(
        &self,
        limit: usize,
    ) -> Result<Vec<T>, StorageError> {
        let limit = limit.clamp(1, 1_000);
        let mut statement = self.connection.prepare(
            "SELECT value_json FROM agent_write_audit
             ORDER BY json_extract(value_json, '$.updatedAt') DESC, id DESC
             LIMIT ?1",
        )?;
        let rows = statement.query_map([limit as i64], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            let raw = row?;
            serde_json::from_str(&raw).map_err(StorageError::from)
        })
        .collect()
    }

    pub fn compare_and_swap_agent_audit<T>(
        &mut self,
        id: &str,
        expected: &T,
        replacement: &T,
    ) -> Result<(), StorageError>
    where
        T: DeserializeOwned + PartialEq + Serialize,
    {
        let transaction = self.connection.transaction()?;
        let raw = transaction
            .query_row(
                "SELECT value_json FROM agent_write_audit WHERE id = ?1",
                [id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(raw) = raw else {
            return Err(StorageError::AgentAuditConflict);
        };
        let current: T = serde_json::from_str(&raw)?;
        if &current != expected {
            return Err(StorageError::AgentAuditConflict);
        }
        transaction.execute(
            "UPDATE agent_write_audit SET value_json = ?2 WHERE id = ?1",
            params![id, serde_json::to_string(replacement)?],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn counts(&self) -> Result<DatabaseCounts, StorageError> {
        Ok(DatabaseCounts {
            nodes: count(&self.connection, "nodes")?,
            blobs: count(&self.connection, "blobs")?,
            trash_snapshots: count(&self.connection, "trash_snapshots")?,
            plugins: count(&self.connection, "plugin_data")?,
        })
    }

    pub fn quick_check(&self) -> Result<(), StorageError> {
        let result: String = self
            .connection
            .query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        if result != "ok" {
            return Err(StorageError::Integrity(result));
        }
        Ok(())
    }

    pub fn checkpoint(&self) -> Result<(), StorageError> {
        self.connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DatabaseCounts {
    pub nodes: u64,
    pub blobs: u64,
    pub trash_snapshots: u64,
    pub plugins: u64,
}

fn migrate(connection: &mut Connection) -> Result<(), StorageError> {
    let mut current: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if current > SCHEMA_VERSION {
        return Err(StorageError::UnsupportedSchema(current));
    }
    if current == 0 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(
            "
            CREATE TABLE schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at INTEGER NOT NULL
            ) STRICT;
            CREATE TABLE nodes (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL CHECK (kind IN ('folder','note','bookmark','file','feed','thread')),
                parent_id TEXT,
                sort_key TEXT NOT NULL,
                title TEXT NOT NULL,
                tags_json TEXT NOT NULL CHECK (json_valid(tags_json)),
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                deleted_at INTEGER,
                meta_json TEXT NOT NULL CHECK (json_valid(meta_json)),
                document_json TEXT NOT NULL CHECK (json_valid(document_json))
            ) STRICT;
            CREATE INDEX nodes_parent_sort ON nodes(parent_id, sort_key, id);
            CREATE INDEX nodes_kind_updated ON nodes(kind, updated_at DESC);
            CREATE INDEX nodes_deleted ON nodes(deleted_at) WHERE deleted_at IS NOT NULL;
            CREATE TABLE blobs (
                key TEXT PRIMARY KEY,
                mime TEXT NOT NULL,
                size INTEGER NOT NULL CHECK (size >= 0),
                data BLOB NOT NULL,
                CHECK (length(data) = size)
            ) STRICT;
            CREATE TABLE trash_snapshots (
                id TEXT PRIMARY KEY,
                node_json TEXT NOT NULL CHECK (json_valid(node_json)),
                blob_key TEXT,
                blob_mime TEXT,
                blob_data BLOB,
                captured_at INTEGER NOT NULL,
                CHECK ((blob_key IS NULL AND blob_mime IS NULL AND blob_data IS NULL)
                    OR (blob_key IS NOT NULL AND blob_mime IS NOT NULL AND blob_data IS NOT NULL))
            ) STRICT;
            CREATE TABLE workspace_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                state_json TEXT NOT NULL CHECK (json_valid(state_json))
            ) STRICT;
            CREATE TABLE plugin_data (
                plugin_id TEXT PRIMARY KEY,
                data_kind TEXT NOT NULL,
                data_version INTEGER NOT NULL CHECK (data_version >= 1),
                package_json TEXT NOT NULL CHECK (json_valid(package_json))
            ) STRICT;
            CREATE TABLE agent_tasks (
                key TEXT PRIMARY KEY,
                value_json TEXT NOT NULL CHECK (json_valid(value_json))
            ) STRICT;
            CREATE TABLE agent_write_audit (
                id TEXT PRIMARY KEY,
                created_at INTEGER NOT NULL,
                value_json TEXT NOT NULL CHECK (json_valid(value_json))
            ) STRICT;
            CREATE TABLE local_search_index (
                key TEXT PRIMARY KEY,
                value_json TEXT NOT NULL CHECK (json_valid(value_json))
            ) STRICT;
            CREATE TABLE local_semantic_index (
                key TEXT PRIMARY KEY,
                value BLOB NOT NULL
            ) STRICT;
            ",
        )?;
        transaction.execute(
            "INSERT INTO schema_migrations (version, applied_at)
             VALUES (1, CAST(strftime('%s','now') AS INTEGER) * 1000)",
            [],
        )?;
        transaction.pragma_update(None, "user_version", 1)?;
        transaction.commit()?;
        current = 1;
    }
    if current < 2 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(
            "CREATE VIRTUAL TABLE node_search USING fts5(
                node_id UNINDEXED,
                title,
                body,
                tags,
                tokenize = 'unicode61 remove_diacritics 2'
            );",
        )?;
        let raw_nodes = {
            let mut statement = transaction.prepare("SELECT document_json FROM nodes")?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        for raw in raw_nodes {
            let node: Node = serde_json::from_str(&raw)?;
            index_node(&transaction, &node)?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations (version, applied_at)
             VALUES (2, CAST(strftime('%s','now') AS INTEGER) * 1000)",
            [],
        )?;
        transaction.pragma_update(None, "user_version", 2)?;
        transaction.commit()?;
        current = 2;
    }
    if current < 3 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(
            "CREATE TABLE app_settings (
                key TEXT PRIMARY KEY,
                value_json TEXT NOT NULL CHECK (json_valid(value_json))
            ) STRICT;",
        )?;
        transaction.execute(
            "INSERT INTO schema_migrations (version, applied_at)
             VALUES (3, CAST(strftime('%s','now') AS INTEGER) * 1000)",
            [],
        )?;
        transaction.pragma_update(None, "user_version", 3)?;
        transaction.commit()?;
    }
    Ok(())
}

fn read_all_nodes(connection: &Connection) -> Result<Vec<Node>, StorageError> {
    let mut statement = connection.prepare("SELECT document_json FROM nodes ORDER BY id")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    rows.map(|row| {
        let raw = row?;
        serde_json::from_str(&raw).map_err(StorageError::from)
    })
    .collect()
}

fn validate_unique_nodes(nodes: &[Node]) -> Result<(), StorageError> {
    let mut ids = HashSet::with_capacity(nodes.len());
    for node in nodes {
        if !ids.insert(node.base().id.as_str()) {
            return Err(StorageError::DuplicateId {
                kind: "node",
                id: node.base().id.clone(),
            });
        }
    }
    Ok(())
}

fn sort_nodes_by_id(nodes: &mut [Node]) {
    nodes.sort_by(|left, right| left.base().id.cmp(&right.base().id));
}

fn validate_archive_references(archive: &ParsedArchive) -> Result<(), StorageError> {
    let mut node_ids = HashSet::with_capacity(archive.nodes.len());
    for node in &archive.nodes {
        let id = &node.base().id;
        if !node_ids.insert(id) {
            return Err(StorageError::DuplicateId {
                kind: "node",
                id: id.clone(),
            });
        }
    }
    let mut blob_ids = HashSet::with_capacity(archive.blobs.len());
    for blob in &archive.blobs {
        if !blob_ids.insert(&blob.key) {
            return Err(StorageError::DuplicateId {
                kind: "blob",
                id: blob.key.clone(),
            });
        }
    }
    for node in &archive.nodes {
        if let Node::File { base, blob_ref, .. } = node
            && !blob_ids.contains(&blob_ref.key)
        {
            return Err(StorageError::MissingBlob {
                node_id: base.id.clone(),
                blob_key: blob_ref.key.clone(),
            });
        }
    }
    let mut snapshot_ids = HashSet::with_capacity(archive.trash_snapshots.len());
    for snapshot in &archive.trash_snapshots {
        if !snapshot_ids.insert(&snapshot.id) {
            return Err(StorageError::DuplicateId {
                kind: "trash snapshot",
                id: snapshot.id.clone(),
            });
        }
        if snapshot.id != snapshot.node.base().id {
            return Err(StorageError::InvalidTrashSnapshot {
                snapshot_id: snapshot.id.clone(),
                node_id: snapshot.node.base().id.clone(),
            });
        }
    }
    let mut plugin_ids = HashSet::with_capacity(archive.plugins.len());
    for plugin in &archive.plugins {
        if !plugin_ids.insert(&plugin.plugin.id) {
            return Err(StorageError::DuplicateId {
                kind: "plugin",
                id: plugin.plugin.id.clone(),
            });
        }
    }
    Ok(())
}

fn insert_node(transaction: &Transaction<'_>, node: &Node) -> Result<(), StorageError> {
    let base = node.base();
    transaction.execute(
        "INSERT INTO nodes (
            id, kind, parent_id, sort_key, title, tags_json, created_at,
            updated_at, deleted_at, meta_json, document_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            base.id,
            node_kind(node.kind()),
            base.parent_id,
            base.sort_key,
            base.title,
            serde_json::to_string(&base.tags)?,
            base.created_at,
            base.updated_at,
            base.deleted_at,
            serde_json::to_string(&base.meta)?,
            serde_json::to_string(node)?,
        ],
    )?;
    index_node(transaction, node)?;
    Ok(())
}

fn upsert_node(connection: &Connection, node: &Node) -> Result<(), StorageError> {
    let base = node.base();
    connection.execute(
        "INSERT INTO nodes (
            id, kind, parent_id, sort_key, title, tags_json, created_at,
            updated_at, deleted_at, meta_json, document_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(id) DO UPDATE SET
            kind = excluded.kind,
            parent_id = excluded.parent_id,
            sort_key = excluded.sort_key,
            title = excluded.title,
            tags_json = excluded.tags_json,
            created_at = excluded.created_at,
            updated_at = excluded.updated_at,
            deleted_at = excluded.deleted_at,
            meta_json = excluded.meta_json,
            document_json = excluded.document_json",
        params![
            base.id,
            node_kind(node.kind()),
            base.parent_id,
            base.sort_key,
            base.title,
            serde_json::to_string(&base.tags)?,
            base.created_at,
            base.updated_at,
            base.deleted_at,
            serde_json::to_string(&base.meta)?,
            serde_json::to_string(node)?,
        ],
    )?;
    index_node(connection, node)?;
    Ok(())
}

fn index_node(connection: &Connection, node: &Node) -> Result<(), StorageError> {
    connection.execute(
        "DELETE FROM node_search WHERE node_id = ?1",
        [&node.base().id],
    )?;
    if node.base().deleted_at.is_some() {
        return Ok(());
    }
    connection.execute(
        "INSERT INTO node_search (node_id, title, body, tags) VALUES (?1, ?2, ?3, ?4)",
        params![
            node.base().id,
            node.base().title,
            searchable_body(node),
            node.base().tags.join(" "),
        ],
    )?;
    Ok(())
}

fn searchable_body(node: &Node) -> String {
    match node {
        Node::Folder { .. } => String::new(),
        Node::Note { content, .. } => {
            let mut text = Vec::new();
            for value in content {
                collect_json_text(value, &mut text);
            }
            text.join(" ")
        }
        Node::Bookmark { content, .. } => {
            format!("{} {}", content.url, content.description)
        }
        Node::File { blob_ref, .. } => format!("{} {}", blob_ref.mime, blob_ref.key),
        Node::Feed { content, .. } => format!(
            "{} {} {} {}",
            content.key,
            content.entity_label.as_deref().unwrap_or_default(),
            content.entity_name.as_deref().unwrap_or_default(),
            content.search_keyword.as_deref().unwrap_or_default()
        ),
        Node::Thread { content, .. } => {
            let mut text = Vec::new();
            for value in &content.messages {
                collect_json_text(value, &mut text);
            }
            text.join(" ")
        }
    }
}

fn collect_json_text<'a>(value: &'a Value, output: &mut Vec<&'a str>) {
    match value {
        Value::String(value) => output.push(value),
        Value::Array(values) => {
            for value in values {
                collect_json_text(value, output);
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                collect_json_text(value, output);
            }
        }
        _ => {}
    }
}

fn fts_literal_query(query: &str) -> Option<String> {
    let query = query
        .split_whitespace()
        .filter(|token| token.chars().any(char::is_alphanumeric))
        .map(|token| format!("\"{}\"*", token.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ");
    (!query.is_empty()).then_some(query)
}

fn escape_like_pattern(query: &str) -> String {
    query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn insert_blob(transaction: &Transaction<'_>, blob: &ArchiveBlob) -> Result<(), StorageError> {
    transaction.execute(
        "INSERT INTO blobs (key, mime, size, data) VALUES (?1, ?2, ?3, ?4)",
        params![blob.key, blob.mime, blob.data.len() as i64, blob.data],
    )?;
    Ok(())
}

fn insert_trash_snapshot(
    transaction: &Transaction<'_>,
    snapshot: &ArchiveTrashSnapshot,
) -> Result<(), StorageError> {
    let (key, mime, data) = snapshot.blob.as_ref().map_or((None, None, None), |blob| {
        (
            Some(blob.key.as_str()),
            Some(blob.mime.as_str()),
            Some(blob.data.as_slice()),
        )
    });
    transaction.execute(
        "INSERT INTO trash_snapshots (
            id, node_json, blob_key, blob_mime, blob_data, captured_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            snapshot.id,
            serde_json::to_string(&snapshot.node)?,
            key,
            mime,
            data,
            snapshot.captured_at,
        ],
    )?;
    Ok(())
}

fn insert_plugin(
    transaction: &Transaction<'_>,
    plugin: &ArchivePluginPackage,
) -> Result<(), StorageError> {
    transaction.execute(
        "INSERT INTO plugin_data (plugin_id, data_kind, data_version, package_json)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            plugin.plugin.id,
            plugin.plugin.data_kind,
            plugin.plugin.data_version as i64,
            serde_json::to_string(plugin)?,
        ],
    )?;
    Ok(())
}

fn node_kind(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Folder => "folder",
        NodeKind::Note => "note",
        NodeKind::Bookmark => "bookmark",
        NodeKind::File => "file",
        NodeKind::Feed => "feed",
        NodeKind::Thread => "thread",
    }
}

fn parse_node_kind(kind: &str) -> Result<NodeKind, StorageError> {
    match kind {
        "folder" => Ok(NodeKind::Folder),
        "note" => Ok(NodeKind::Note),
        "bookmark" => Ok(NodeKind::Bookmark),
        "file" => Ok(NodeKind::File),
        "feed" => Ok(NodeKind::Feed),
        "thread" => Ok(NodeKind::Thread),
        _ => Err(StorageError::Integrity(format!(
            "stored node kind `{kind}` is invalid"
        ))),
    }
}

fn count(connection: &Connection, table: &str) -> Result<u64, StorageError> {
    let value: i64 = connection.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
        row.get(0)
    })?;
    Ok(value as u64)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ideall_protocol::{BaseNode, Node};

    use super::*;
    use crate::{ArchiveLimits, parse_workspace_archive};

    fn note(id: &str, updated_at: i64, text: &str) -> Node {
        Node::Note {
            base: BaseNode {
                id: id.into(),
                parent_id: None,
                sort_key: id.into(),
                title: id.into(),
                tags: Vec::new(),
                created_at: 1,
                updated_at,
                deleted_at: None,
                meta: BTreeMap::new(),
            },
            content: vec![serde_json::json!({
                "type": "p",
                "children": [{"text": text}]
            })],
        }
    }

    fn archive() -> ParsedArchive {
        let exported_at = "2026-01-01T00:00:00.000Z";
        let core = serde_json::json!({
            "nodes": [{
                "id": "n1", "kind": "note", "parentId": null, "sortKey": "a0",
                "title": "Note", "tags": [], "createdAt": 1, "updatedAt": 2,
                "content": [{"type": "p", "children": [{"text": "body"}]}]
            }],
            "blobs": [], "trashSnapshots": [], "workspace": null
        });
        let plugins = serde_json::json!({
            "kind": "ideall.workspace-backup", "version": 1,
            "exportedAt": exported_at, "plugins": []
        });
        let checksum = format!(
            "{:08x}",
            crc32fast::hash(
                &serde_json::to_vec(&serde_json::json!({
                    "exportedAt": exported_at, "core": core, "plugins": plugins
                }))
                .unwrap()
            )
        );
        let raw = serde_json::json!({
            "kind": "ideall.workspace-archive", "version": 2,
            "exportedAt": exported_at, "core": core, "plugins": plugins,
            "manifest": {
                "algorithm": "crc32", "checksum": checksum, "nodeCount": 1,
                "blobCount": 0, "blobBytes": 0, "trashSnapshotCount": 0,
                "pluginCount": 0, "tabCount": 0
            }
        });
        parse_workspace_archive(
            &serde_json::to_string(&raw).unwrap(),
            ArchiveLimits::default(),
        )
        .unwrap()
    }

    #[test]
    fn migrates_schema_and_replaces_archive_in_one_transaction() {
        let mut database = Database::open_in_memory().unwrap();
        assert_eq!(database.schema_version().unwrap(), 3);
        database.replace_from_archive(&archive()).unwrap();
        assert_eq!(
            database.counts().unwrap(),
            DatabaseCounts {
                nodes: 1,
                blobs: 0,
                trash_snapshots: 0,
                plugins: 0,
            }
        );
        let node = database.get_node("n1").unwrap().unwrap();
        assert_eq!(node.base().title, "Note");
        database.quick_check().unwrap();
    }

    #[test]
    fn navigation_projection_does_not_deserialize_full_documents() {
        let mut database = Database::open_in_memory().unwrap();
        database.replace_from_archive(&archive()).unwrap();
        database
            .connection
            .execute("UPDATE nodes SET document_json = '{}' WHERE id = 'n1'", [])
            .unwrap();

        assert!(database.list_nodes(false).is_err());
        assert_eq!(
            database.list_node_projections(false).unwrap(),
            vec![NodeProjection {
                id: "n1".into(),
                parent_id: None,
                sort_key: "a0".into(),
                kind: NodeKind::Note,
                title: "Note".into(),
                updated_at: 2,
                deleted: false,
            }]
        );
    }

    #[test]
    fn rejects_file_nodes_with_missing_blob_without_changing_data() {
        let mut database = Database::open_in_memory().unwrap();
        let initial = archive();
        database.replace_from_archive(&initial).unwrap();
        let mut invalid = archive();
        invalid.nodes.push(Node::File {
            base: BaseNode {
                id: "file-1".into(),
                parent_id: None,
                sort_key: "b0".into(),
                title: "missing.txt".into(),
                tags: Vec::new(),
                created_at: 1,
                updated_at: 1,
                deleted_at: None,
                meta: BTreeMap::new(),
            },
            blob_ref: ideall_protocol::BlobRef {
                store: "blobs".into(),
                key: "missing".into(),
                size: 1,
                mime: "text/plain".into(),
            },
            content: None,
        });
        assert!(matches!(
            database.replace_from_archive(&invalid),
            Err(StorageError::MissingBlob { .. })
        ));
        assert_eq!(database.counts().unwrap().nodes, 1);
    }

    #[test]
    fn full_text_index_tracks_updates_trash_restore_and_purge() {
        let mut database = Database::open_in_memory().unwrap();
        database.replace_from_archive(&archive()).unwrap();
        assert_eq!(database.search_nodes("body", 20).unwrap().len(), 1);

        let mut node = database.get_node("n1").unwrap().unwrap();
        node.base_mut().title = "中文知识库".into();
        database.put_node(&node).unwrap();
        assert_eq!(database.search_nodes("知识", 20).unwrap().len(), 1);
        assert!(database.search_nodes("100%", 20).unwrap().is_empty());
        for punctuation in ["\"", "*", "%", "' OR 1=1 --"] {
            assert!(database.search_nodes(punctuation, 20).unwrap().is_empty());
        }

        database.move_to_trash("n1", 10).unwrap();
        assert!(database.search_nodes("知识", 20).unwrap().is_empty());
        database.restore_from_trash("n1", 11).unwrap();
        assert_eq!(database.search_nodes("知识", 20).unwrap().len(), 1);
        database.remove_node_permanently("n1").unwrap();
        assert!(database.search_nodes("知识", 20).unwrap().is_empty());
    }

    #[test]
    fn application_settings_round_trip_as_typed_json() {
        let mut database = Database::open_in_memory().unwrap();
        let value = BTreeMap::from([("theme", "dark")]);
        database.set_setting("appearance", &value).unwrap();
        assert_eq!(
            database
                .setting::<BTreeMap<String, String>>("appearance")
                .unwrap(),
            Some(BTreeMap::from([("theme".into(), "dark".into())]))
        );
    }

    #[test]
    fn synchronization_batch_uses_snapshot_cas_and_updates_search() {
        let mut database = Database::open_in_memory().unwrap();
        let original = note("sync-note", 1, "old body");
        database.put_node(&original).unwrap();
        let mut updated = note("sync-note", 2, "new body");
        updated.base_mut().title = "remote title".into();

        let committed = database
            .compare_and_swap_nodes(
                std::slice::from_ref(&updated),
                std::slice::from_ref(&original),
                |node| node.kind() == NodeKind::Note,
            )
            .unwrap();
        assert_eq!(committed, vec![updated.clone()]);
        assert_eq!(
            database.search_nodes("new body", 20).unwrap(),
            vec![updated]
        );

        assert!(matches!(
            database.compare_and_swap_nodes(&[], std::slice::from_ref(&original), |node| {
                node.kind() == NodeKind::Note
            }),
            Err(StorageError::SnapshotConflict)
        ));
        assert!(database.get_node("sync-note").unwrap().is_some());
    }

    #[test]
    fn synchronized_tombstone_preserves_a_restorable_local_snapshot() {
        let mut database = Database::open_in_memory().unwrap();
        let original = note("sync-delete", 1, "recover me");
        database.put_node(&original).unwrap();
        let mut tombstone = original.clone();
        tombstone.base_mut().updated_at = 10;
        tombstone.base_mut().deleted_at = Some(10);

        database
            .compare_and_swap_nodes(
                std::slice::from_ref(&tombstone),
                std::slice::from_ref(&original),
                |node| node.kind() == NodeKind::Note,
            )
            .unwrap();
        assert!(database.search_nodes("recover", 20).unwrap().is_empty());
        assert!(database.restore_from_trash("sync-delete", 11).unwrap());
        let restored = database.get_node("sync-delete").unwrap().unwrap();
        assert_eq!(restored.base().deleted_at, None);
        assert_eq!(database.search_nodes("recover", 20).unwrap().len(), 1);
    }

    #[test]
    fn agent_audit_is_bounded_and_completion_uses_cas() {
        let mut database = Database::open_in_memory().unwrap();
        let committed = serde_json::json!({
            "id": "old", "status": "committed", "updatedAt": 1
        });
        let pending = serde_json::json!({
            "id": "pending", "status": "pending", "updatedAt": 2
        });
        database
            .append_agent_audit("old", 1, &committed, 2)
            .unwrap();
        database
            .append_agent_audit("pending", 2, &pending, 2)
            .unwrap();
        let newest = serde_json::json!({
            "id": "new", "status": "failed", "updatedAt": 3
        });
        database.append_agent_audit("new", 3, &newest, 2).unwrap();
        assert!(database.agent_audit::<Value>("old").unwrap().is_none());
        assert_eq!(database.list_agent_audits::<Value>(10).unwrap().len(), 2);

        let completed = serde_json::json!({
            "id": "pending", "status": "committed", "updatedAt": 4
        });
        database
            .compare_and_swap_agent_audit("pending", &pending, &completed)
            .unwrap();
        assert!(matches!(
            database.compare_and_swap_agent_audit("pending", &pending, &newest),
            Err(StorageError::AgentAuditConflict)
        ));
    }

    #[test]
    fn pending_agent_audits_are_never_evicted_for_capacity() {
        let mut database = Database::open_in_memory().unwrap();
        for id in ["p1", "p2"] {
            database
                .append_agent_audit(
                    id,
                    1,
                    &serde_json::json!({"id": id, "status": "pending", "updatedAt": 1}),
                    2,
                )
                .unwrap();
        }
        assert!(matches!(
            database.append_agent_audit(
                "p3",
                2,
                &serde_json::json!({"id": "p3", "status": "pending", "updatedAt": 2}),
                2
            ),
            Err(StorageError::AgentAuditFull)
        ));
        assert_eq!(database.list_agent_audits::<Value>(10).unwrap().len(), 2);
    }
}
