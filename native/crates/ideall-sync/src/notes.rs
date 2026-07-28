use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use ideall_protocol::SyncNote;
use serde_json::Value;

use crate::{TOMBSTONE_TTL_MS, prune_expired_tombstones};

#[derive(Clone, Copy)]
enum Side {
    Local,
    Remote,
}

struct ParsedMeta<'a> {
    version: f64,
    by: &'a str,
    sort_key: &'a str,
    deleted_at: Option<i64>,
}

pub fn merge_sync_notes(local: &[SyncNote], remote: &[SyncNote]) -> Vec<SyncNote> {
    let mut records = HashMap::with_capacity(local.len().saturating_add(remote.len()));
    for note in remote {
        records.insert(note.id.clone(), note.clone());
    }
    for note in local {
        match records.remove(&note.id) {
            Some(other) => {
                records.insert(note.id.clone(), merge_two_notes(note, &other));
            }
            None => {
                records.insert(note.id.clone(), note.clone());
            }
        }
    }
    let mut records = records.into_values().collect::<Vec<_>>();
    records.sort_by(|left, right| left.id.cmp(&right.id));
    records
}

pub fn gc_sync_notes(notes: &[SyncNote], now: i64) -> Vec<SyncNote> {
    prune_expired_tombstones(notes, now, TOMBSTONE_TTL_MS)
        .into_iter()
        .map(|mut note| {
            if let Some(metadata) = note.block_meta.take() {
                note.block_meta = Some(
                    metadata
                        .into_iter()
                        .filter(|(_, value)| {
                            parsed_meta(value).is_none_or(|meta| {
                                meta.deleted_at.is_none_or(|deleted_at| {
                                    now.saturating_sub(deleted_at) <= TOMBSTONE_TTL_MS
                                })
                            })
                        })
                        .collect(),
                );
            }
            note
        })
        .collect()
}

fn merge_two_notes(local: &SyncNote, remote: &SyncNote) -> SyncNote {
    let mut winner = if local.updated_at >= remote.updated_at {
        local.clone()
    } else {
        remote.clone()
    };
    if winner.deleted_at.is_some() {
        return winner;
    }
    let (Some(local_meta), Some(remote_meta)) = (&local.block_meta, &remote.block_meta) else {
        return winner;
    };

    let local_blocks = blocks_by_id(&local.content);
    let remote_blocks = blocks_by_id(&remote.content);
    let ids = local_meta
        .keys()
        .chain(remote_meta.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut metadata = BTreeMap::new();
    let mut blocks = Vec::new();
    for id in ids {
        let local_value = local_meta.get(&id);
        let remote_value = remote_meta.get(&id);
        let side = pick_side(local_value, remote_value);
        let winning_value = match side {
            Side::Local => local_value.or(remote_value),
            Side::Remote => remote_value.or(local_value),
        };
        let Some(winning_value) = winning_value else {
            continue;
        };
        metadata.insert(id.clone(), winning_value.clone());
        if parsed_meta(winning_value).is_some_and(|meta| meta.deleted_at.is_some()) {
            continue;
        }
        let block = match side {
            Side::Local => local_blocks
                .get(id.as_str())
                .or_else(|| remote_blocks.get(id.as_str())),
            Side::Remote => remote_blocks
                .get(id.as_str())
                .or_else(|| local_blocks.get(id.as_str())),
        };
        if let Some(block) = block {
            blocks.push((id, (*block).clone()));
        }
    }
    blocks.sort_by(|(left_id, _), (right_id, _)| {
        let left = metadata.get(left_id).and_then(parsed_meta);
        let right = metadata.get(right_id).and_then(parsed_meta);
        match (left, right) {
            (Some(left), Some(right)) => left
                .sort_key
                .cmp(right.sort_key)
                .then_with(|| left_id.cmp(right_id)),
            _ => left_id.cmp(right_id),
        }
    });
    winner.content = blocks.into_iter().map(|(_, block)| block).collect();
    winner.block_meta = Some(metadata);
    winner.created_at = local.created_at.min(remote.created_at);
    winner.updated_at = local.updated_at.max(remote.updated_at);
    winner.deleted_at = None;
    winner
}

fn blocks_by_id(content: &[Value]) -> HashMap<&str, &Value> {
    content
        .iter()
        .filter_map(|value| {
            value
                .as_object()
                .and_then(|block| block.get("id"))
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .map(|id| (id, value))
        })
        .collect()
}

fn pick_side(local: Option<&Value>, remote: Option<&Value>) -> Side {
    match (local.and_then(parsed_meta), remote.and_then(parsed_meta)) {
        (Some(_), None) => Side::Local,
        (None, Some(_)) => Side::Remote,
        (None, None) => Side::Local,
        (Some(local), Some(remote)) => match local
            .version
            .partial_cmp(&remote.version)
            .unwrap_or(Ordering::Equal)
            .then_with(|| remote.by.cmp(local.by))
            .then_with(|| remote.sort_key.cmp(local.sort_key))
        {
            Ordering::Greater | Ordering::Equal => Side::Local,
            Ordering::Less => Side::Remote,
        },
    }
}

fn parsed_meta(value: &Value) -> Option<ParsedMeta<'_>> {
    let object = value.as_object()?;
    Some(ParsedMeta {
        version: object.get("v")?.as_f64()?,
        by: object.get("by")?.as_str()?,
        sort_key: object.get("sk")?.as_str()?,
        deleted_at: object.get("del").and_then(Value::as_i64),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(id: &str, updated_at: i64, blocks: Vec<Value>, metadata: Value) -> SyncNote {
        SyncNote {
            id: id.into(),
            title: format!("note-{updated_at}"),
            content: blocks,
            parent_id: None,
            sort_key: "a0".into(),
            tags: Vec::new(),
            created_at: updated_at,
            updated_at,
            deleted_at: None,
            block_meta: Some(
                metadata
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
            ),
        }
    }

    #[test]
    fn block_merge_keeps_concurrent_blocks_and_newer_block_versions() {
        let local = note(
            "n1",
            20,
            vec![serde_json::json!({"id": "a", "text": "local"})],
            serde_json::json!({"a": {"v": 2, "by": "desktop", "sk": "b"}}),
        );
        let remote = note(
            "n1",
            10,
            vec![
                serde_json::json!({"id": "a", "text": "stale"}),
                serde_json::json!({"id": "b", "text": "remote"}),
            ],
            serde_json::json!({
                "a": {"v": 1, "by": "phone", "sk": "b"},
                "b": {"v": 1, "by": "phone", "sk": "a"}
            }),
        );
        let merged = merge_sync_notes(std::slice::from_ref(&local), std::slice::from_ref(&remote));
        assert_eq!(merged[0].content[0]["id"], "b");
        assert_eq!(merged[0].content[1]["text"], "local");
        assert_eq!(merged[0].created_at, 10);
        assert_eq!(
            merge_sync_notes(&[remote], &[local]),
            merged,
            "block-ready merge must be commutative"
        );
    }

    #[test]
    fn newer_note_tombstone_wins_before_block_merge() {
        let live = note(
            "n1",
            10,
            vec![serde_json::json!({"id": "a", "text": "live"})],
            serde_json::json!({"a": {"v": 1, "by": "a", "sk": "a"}}),
        );
        let mut deleted = live.clone();
        deleted.updated_at = 20;
        deleted.deleted_at = Some(20);
        let merged = merge_sync_notes(&[live], std::slice::from_ref(&deleted));
        assert_eq!(merged, vec![deleted]);
    }

    #[test]
    fn block_tombstones_are_pruned_only_after_the_shared_ttl() {
        let now = TOMBSTONE_TTL_MS + 100;
        let note = note(
            "n1",
            now,
            Vec::new(),
            serde_json::json!({
                "expired": {"v": 1, "by": "a", "sk": "a", "del": 1},
                "recent": {"v": 1, "by": "a", "sk": "b", "del": 100}
            }),
        );
        let collected = gc_sync_notes(&[note], now);
        let metadata = collected[0].block_meta.as_ref().unwrap();
        assert!(!metadata.contains_key("expired"));
        assert!(metadata.contains_key("recent"));
    }
}
