use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    Folder,
    Note,
    Bookmark,
    File,
    Feed,
    Thread,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseNode {
    pub id: String,
    pub parent_id: Option<String>,
    pub sort_key: String,
    pub title: String,
    pub tags: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<i64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub meta: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobRef {
    pub store: String,
    pub key: String,
    pub size: u64,
    pub mime: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkContent {
    pub url: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub favicon: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubscriptionType {
    Publisher,
    Entity,
    Tool,
    Search,
    Peer,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedContent {
    pub r#type: SubscriptionType,
    pub key: String,
    #[serde(default)]
    pub favicon: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_keyword: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_domain: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThreadContent {
    pub messages: Vec<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Node {
    Folder {
        #[serde(flatten)]
        base: BaseNode,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<()>,
    },
    Note {
        #[serde(flatten)]
        base: BaseNode,
        content: Vec<Value>,
    },
    Bookmark {
        #[serde(flatten)]
        base: BaseNode,
        content: BookmarkContent,
    },
    File {
        #[serde(flatten)]
        base: BaseNode,
        #[serde(rename = "blobRef")]
        blob_ref: BlobRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<()>,
    },
    Feed {
        #[serde(flatten)]
        base: BaseNode,
        content: FeedContent,
    },
    Thread {
        #[serde(flatten)]
        base: BaseNode,
        content: ThreadContent,
    },
}

impl Node {
    pub fn kind(&self) -> NodeKind {
        match self {
            Self::Folder { .. } => NodeKind::Folder,
            Self::Note { .. } => NodeKind::Note,
            Self::Bookmark { .. } => NodeKind::Bookmark,
            Self::File { .. } => NodeKind::File,
            Self::Feed { .. } => NodeKind::Feed,
            Self::Thread { .. } => NodeKind::Thread,
        }
    }

    pub fn base(&self) -> &BaseNode {
        match self {
            Self::Folder { base, .. }
            | Self::Note { base, .. }
            | Self::Bookmark { base, .. }
            | Self::File { base, .. }
            | Self::Feed { base, .. }
            | Self::Thread { base, .. } => base,
        }
    }

    pub fn base_mut(&mut self) -> &mut BaseNode {
        match self {
            Self::Folder { base, .. }
            | Self::Note { base, .. }
            | Self::Bookmark { base, .. }
            | Self::File { base, .. }
            | Self::Feed { base, .. }
            | Self::Thread { base, .. } => base,
        }
    }

    /// Privacy-preserving projection used for bulk listings.
    pub fn strip_for_listing(&self) -> Self {
        match self {
            Self::Note { base, .. } => Self::Note {
                base: base.clone(),
                content: Vec::new(),
            },
            Self::Thread { base, .. } => Self::Thread {
                base: base.clone(),
                content: ThreadContent {
                    messages: Vec::new(),
                },
            },
            other => other.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> BaseNode {
        BaseNode {
            id: "n1".into(),
            parent_id: None,
            sort_key: "a0".into(),
            title: "测试".into(),
            tags: vec!["private".into()],
            created_at: 10,
            updated_at: 20,
            deleted_at: None,
            meta: BTreeMap::new(),
        }
    }

    #[test]
    fn note_serde_matches_typescript_wire() {
        let raw = serde_json::json!({
            "id": "n1",
            "kind": "note",
            "parentId": null,
            "sortKey": "a0",
            "title": "测试",
            "tags": ["private"],
            "createdAt": 10,
            "updatedAt": 20,
            "content": [{"type": "p", "children": [{"text": "secret"}]}]
        });
        let node: Node = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(node.kind(), NodeKind::Note);
        assert_eq!(serde_json::to_value(&node).unwrap(), raw);

        let stripped = node.strip_for_listing();
        assert_eq!(
            serde_json::to_value(stripped).unwrap()["content"],
            serde_json::json!([])
        );
    }

    #[test]
    fn thread_listing_removes_messages() {
        let node = Node::Thread {
            base: base(),
            content: ThreadContent {
                messages: vec![serde_json::json!({"role": "user", "content": "secret"})],
            },
        };
        let stripped = serde_json::to_value(node.strip_for_listing()).unwrap();
        assert_eq!(stripped["content"]["messages"], serde_json::json!([]));
    }
}
