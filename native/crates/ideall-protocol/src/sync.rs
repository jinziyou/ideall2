use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::SubscriptionType;

/// Exact 0.2 note synchronization wire shape. It intentionally does not carry
/// the unified Node `kind` discriminator so native and web clients can coexist.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncNote {
    pub id: String,
    pub title: String,
    pub content: Vec<Value>,
    pub parent_id: Option<String>,
    pub sort_key: String,
    pub tags: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_meta: Option<BTreeMap<String, Value>>,
}

/// Exact 0.2 subscription synchronization wire shape. Native storage projects
/// this to and from a unified `feed` Node at its adapter boundary.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncSubscription {
    pub id: String,
    pub r#type: SubscriptionType,
    pub key: String,
    pub title: String,
    pub favicon: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_keyword: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_domain: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_note_uses_legacy_wire_without_node_discriminator() {
        let note = SyncNote {
            id: "n1".into(),
            title: "Note".into(),
            content: vec![serde_json::json!({"type": "p"})],
            parent_id: None,
            sort_key: "a0".into(),
            tags: Vec::new(),
            created_at: 1,
            updated_at: 2,
            deleted_at: None,
            block_meta: None,
        };
        let value = serde_json::to_value(note).unwrap();
        assert_eq!(value["parentId"], Value::Null);
        assert!(value.get("kind").is_none());
        assert!(value.get("deletedAt").is_none());
    }

    #[test]
    fn subscription_identity_fields_match_web_wire() {
        let value = serde_json::to_value(SyncSubscription {
            id: "publisher:example.com".into(),
            r#type: SubscriptionType::Publisher,
            key: "example.com".into(),
            title: "Example".into(),
            favicon: String::new(),
            entity_label: None,
            entity_name: None,
            search_keyword: None,
            search_domain: None,
            created_at: 1,
            updated_at: 1,
            deleted_at: None,
        })
        .unwrap();
        assert_eq!(value["type"], "publisher");
        assert_eq!(value["createdAt"], 1);
    }
}
