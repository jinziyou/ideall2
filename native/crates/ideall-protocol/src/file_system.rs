use std::collections::BTreeMap;
use std::fmt;

use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub const DIRECTORY_MEDIA_TYPE: &str = "inode/directory";

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRef {
    pub file_system_id: String,
    pub file_id: String,
}

impl FileRef {
    pub fn new(file_system_id: impl Into<String>, file_id: impl Into<String>) -> Self {
        Self {
            file_system_id: file_system_id.into(),
            file_id: file_id.into(),
        }
    }

    pub fn validate(&self) -> Result<(), FileRefKeyError> {
        if self.file_system_id.is_empty() || self.file_id.is_empty() {
            return Err(FileRefKeyError::EmptyComponent);
        }
        Ok(())
    }

    /// Stable key compatible with `src/protocol/file-system.ts:fileRefKey`.
    pub fn to_key(&self) -> Result<String, FileRefKeyError> {
        self.validate()?;
        Ok(format!(
            "{}:{}",
            encode_uri_component(&self.file_system_id),
            encode_uri_component(&self.file_id)
        ))
    }

    pub fn from_key(raw: &str) -> Result<Self, FileRefKeyError> {
        let mut components = raw.split(':');
        let Some(file_system_id) = components.next() else {
            return Err(FileRefKeyError::MalformedKey);
        };
        let Some(file_id) = components.next() else {
            return Err(FileRefKeyError::MalformedKey);
        };
        if components.next().is_some() || file_system_id.is_empty() || file_id.is_empty() {
            return Err(FileRefKeyError::MalformedKey);
        }

        let value = Self {
            file_system_id: decode_uri_component(file_system_id)?,
            file_id: decode_uri_component(file_id)?,
        };
        value.validate()?;
        Ok(value)
    }
}

impl fmt::Display for FileRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.to_key() {
            Ok(key) => formatter.write_str(&key),
            Err(_) => formatter.write_str("<invalid-file-ref>"),
        }
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum FileRefKeyError {
    #[error("FileRef requires non-empty fileSystemId and fileId")]
    EmptyComponent,
    #[error("malformed FileRef key")]
    MalformedKey,
    #[error("FileRef key contains invalid UTF-8")]
    InvalidUtf8,
}

fn encode_uri_component(raw: &str) -> String {
    let mut encoded = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
            )
        {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            write!(&mut encoded, "%{byte:02X}").expect("writing to a String cannot fail");
        }
    }
    encoded
}

fn decode_uri_component(raw: &str) -> Result<String, FileRefKeyError> {
    percent_decode_str(raw)
        .decode_utf8()
        .map(|value| value.into_owned())
        .map_err(|_| FileRefKeyError::InvalidUtf8)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileKind {
    File,
    Directory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileSourceKind {
    Local,
    Remote,
    App,
    ThirdParty,
    System,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSource {
    pub kind: FileSourceKind,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_only: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdeallFile {
    pub r#ref: FileRef,
    pub kind: FileKind,
    pub name: String,
    pub media_type: String,
    pub capabilities: Vec<String>,
    pub source: FileSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, Value>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DirectoryEntryKind {
    Child,
    Link,
    Mount,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryEntry {
    pub entry_id: String,
    pub parent: FileRef,
    pub target: FileRef,
    pub name: String,
    pub kind: DirectoryEntryKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<IdeallFile>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_ref_key_matches_javascript_encode_uri_component() {
        let reference = FileRef::new("node:本地", "note/a?x=1 & y=2");
        let key = reference.to_key().unwrap();
        assert_eq!(
            key,
            "node%3A%E6%9C%AC%E5%9C%B0:note%2Fa%3Fx%3D1%20%26%20y%3D2"
        );
        assert_eq!(FileRef::from_key(&key).unwrap(), reference);
    }

    #[test]
    fn file_ref_key_rejects_empty_or_ambiguous_values() {
        assert_eq!(
            FileRef::from_key("a:b:c"),
            Err(FileRefKeyError::MalformedKey)
        );
        assert_eq!(FileRef::from_key(":"), Err(FileRefKeyError::MalformedKey));
        assert_eq!(
            FileRef::new("", "id").to_key(),
            Err(FileRefKeyError::EmptyComponent)
        );
    }

    #[test]
    fn file_ref_serde_uses_wire_field_names() {
        let reference = FileRef::new("local.nodes", "n1");
        assert_eq!(
            serde_json::to_value(reference).unwrap(),
            serde_json::json!({"fileSystemId": "local.nodes", "fileId": "n1"})
        );
    }
}
