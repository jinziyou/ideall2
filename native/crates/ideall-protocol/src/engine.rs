use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::FileKind;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineLayout {
    Padded,
    Fill,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EngineAccess {
    ReadOnly,
    ReadWrite,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EngineSuspension {
    Serializable,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineMatcher {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<FileKind>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media_types: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineDescriptor {
    pub engine_id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#match: Option<EngineMatcher>,
    #[serde(default)]
    pub priority: i32,
    pub layout: EngineLayout,
    pub access: EngineAccess,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suspension: Option<EngineSuspension>,
    #[serde(default)]
    pub supports_standalone_window: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_hint: Option<String>,
}
