use std::collections::{BTreeMap, BTreeSet};

use ideall_protocol::{
    EngineAccess, EngineDescriptor, EngineLayout, EngineMatcher, EngineSuspension, FileKind,
    IdeallFile,
};
use serde::{Deserialize, Serialize};

use crate::{engine_match_specificity, media_type_ancestors};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnginePreferences {
    pub version: u8,
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    #[serde(default)]
    pub media_types: BTreeMap<String, String>,
    #[serde(default)]
    pub removed: BTreeMap<String, BTreeSet<String>>,
}

impl Default for EnginePreferences {
    fn default() -> Self {
        Self {
            version: 2,
            files: BTreeMap::new(),
            media_types: BTreeMap::new(),
            removed: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EngineResolutionSource {
    FilePreference,
    MediaTypePreference,
    Priority,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EngineCandidate {
    pub descriptor: EngineDescriptor,
    pub specificity: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EngineResolution {
    pub candidate: EngineCandidate,
    pub source: EngineResolutionSource,
}

pub fn list_matching_engines(
    descriptors: &[EngineDescriptor],
    file: &IdeallFile,
) -> Vec<EngineCandidate> {
    let mut candidates = descriptors
        .iter()
        .filter_map(|descriptor| {
            let specificity = descriptor
                .r#match
                .as_ref()
                .map_or(Some(0), |matcher| engine_match_specificity(matcher, file))?;
            Some(EngineCandidate {
                descriptor: descriptor.clone(),
                specificity,
            })
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .descriptor
            .priority
            .cmp(&left.descriptor.priority)
            .then_with(|| right.specificity.cmp(&left.specificity))
            .then_with(|| left.descriptor.engine_id.cmp(&right.descriptor.engine_id))
    });
    candidates
}

pub fn resolve_default_engine(
    descriptors: &[EngineDescriptor],
    file: &IdeallFile,
    preferences: &EnginePreferences,
) -> Option<EngineResolution> {
    let matching = list_matching_engines(descriptors, file);
    let file_key = file.r#ref.to_key().ok();
    if let Some(preferred) = file_key
        .as_ref()
        .and_then(|key| preferences.files.get(key))
        .and_then(|id| {
            matching
                .iter()
                .find(|candidate| candidate.descriptor.engine_id == *id)
        })
    {
        return Some(EngineResolution {
            candidate: preferred.clone(),
            source: EngineResolutionSource::FilePreference,
        });
    }

    let filtered = matching
        .iter()
        .filter(|candidate| {
            !association_removed(
                preferences,
                &file.media_type,
                &candidate.descriptor.engine_id,
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    let candidates = if filtered.is_empty() {
        &matching
    } else {
        &filtered
    };
    if let Some(preferred_id) = media_type_preference(preferences, &file.media_type)
        && let Some(preferred) = candidates
            .iter()
            .find(|candidate| candidate.descriptor.engine_id == preferred_id)
    {
        return Some(EngineResolution {
            candidate: preferred.clone(),
            source: EngineResolutionSource::MediaTypePreference,
        });
    }
    candidates
        .first()
        .cloned()
        .map(|candidate| EngineResolution {
            candidate,
            source: EngineResolutionSource::Priority,
        })
}

fn media_type_preference<'a>(
    preferences: &'a EnginePreferences,
    media_type: &str,
) -> Option<&'a str> {
    let normalized = normalize_media_type(media_type);
    preferences
        .media_types
        .get(&normalized)
        .map(String::as_str)
        .or_else(|| {
            media_type_ancestors(&normalized)
                .into_iter()
                .find_map(|ancestor| preferences.media_types.get(ancestor).map(String::as_str))
        })
}

fn association_removed(preferences: &EnginePreferences, media_type: &str, engine_id: &str) -> bool {
    let normalized = normalize_media_type(media_type);
    preferences
        .removed
        .get(&normalized)
        .is_some_and(|removed| removed.contains(engine_id))
        || media_type_ancestors(&normalized)
            .into_iter()
            .any(|ancestor| {
                preferences
                    .removed
                    .get(ancestor)
                    .is_some_and(|removed| removed.contains(engine_id))
            })
}

fn normalize_media_type(value: &str) -> String {
    value
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

pub fn builtin_engines() -> Vec<EngineDescriptor> {
    vec![
        descriptor(
            "ideall.note",
            "页面",
            1_000,
            &["application/vnd.ideall.note+json"],
        ),
        descriptor(
            "ideall.bookmark",
            "书签",
            950,
            &["application/vnd.ideall.bookmark+json"],
        ),
        descriptor(
            "ideall.feed",
            "关注",
            950,
            &["application/vnd.ideall.feed+json"],
        ),
        descriptor(
            "ideall.thread",
            "对话",
            950,
            &["application/vnd.ideall.thread+json"],
        ),
        read_only_descriptor("ideall.audio", "音频", 900, &["audio/*"]),
        read_only_descriptor("ideall.database", "数据库", 880, &["application/x-sqlite3"]),
        descriptor(
            "ideall.browser",
            "浏览器",
            750,
            &["text/uri-list", "application/vnd.ideall.bookmark+json"],
        ),
        descriptor(
            "ideall.code",
            "开发",
            700,
            &[
                "text/*",
                "application/json",
                "application/javascript",
                "application/typescript",
                "application/xml",
                "image/svg+xml",
            ],
        ),
        EngineDescriptor {
            engine_id: "ideall.directory".into(),
            label: "文件树".into(),
            r#match: Some(EngineMatcher {
                kinds: vec![FileKind::Directory],
                ..EngineMatcher::default()
            }),
            priority: 600,
            layout: EngineLayout::Padded,
            access: EngineAccess::ReadOnly,
            suspension: None,
            supports_standalone_window: true,
            icon_hint: Some("folder".into()),
        },
        EngineDescriptor {
            engine_id: "ideall.preview".into(),
            label: "通用预览".into(),
            r#match: Some(EngineMatcher {
                kinds: vec![FileKind::File, FileKind::Directory],
                ..EngineMatcher::default()
            }),
            priority: -1_000,
            layout: EngineLayout::Fill,
            access: EngineAccess::ReadOnly,
            suspension: Some(EngineSuspension::Serializable),
            supports_standalone_window: true,
            icon_hint: Some("file".into()),
        },
    ]
}

fn descriptor(id: &str, label: &str, priority: i32, media_types: &[&str]) -> EngineDescriptor {
    EngineDescriptor {
        engine_id: id.into(),
        label: label.into(),
        r#match: Some(EngineMatcher {
            media_types: media_types.iter().map(|value| (*value).into()).collect(),
            ..EngineMatcher::default()
        }),
        priority,
        layout: EngineLayout::Fill,
        access: EngineAccess::ReadWrite,
        suspension: None,
        supports_standalone_window: true,
        icon_hint: None,
    }
}

fn read_only_descriptor(
    id: &str,
    label: &str,
    priority: i32,
    media_types: &[&str],
) -> EngineDescriptor {
    let mut descriptor = descriptor(id, label, priority, media_types);
    descriptor.access = EngineAccess::ReadOnly;
    descriptor
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ideall_protocol::{FileRef, FileSource, FileSourceKind};

    use super::*;

    fn file(media_type: &str) -> IdeallFile {
        IdeallFile {
            r#ref: FileRef::new("local.nodes", "n1"),
            kind: FileKind::File,
            name: "README.md".into(),
            media_type: media_type.into(),
            capabilities: vec!["read".into(), "write".into()],
            source: FileSource {
                kind: FileSourceKind::Local,
                id: "local.nodes".into(),
                label: None,
                read_only: Some(false),
            },
            size: None,
            created_at: None,
            updated_at: None,
            version: None,
            properties: BTreeMap::new(),
        }
    }

    #[test]
    fn file_preference_then_media_preference_then_priority() {
        let descriptors = builtin_engines();
        let markdown = file("text/markdown");
        let mut preferences = EnginePreferences::default();
        assert_eq!(
            resolve_default_engine(&descriptors, &markdown, &preferences)
                .unwrap()
                .candidate
                .descriptor
                .engine_id,
            "ideall.code"
        );
        preferences
            .media_types
            .insert("text/plain".into(), "ideall.preview".into());
        assert_eq!(
            resolve_default_engine(&descriptors, &markdown, &preferences)
                .unwrap()
                .source,
            EngineResolutionSource::MediaTypePreference
        );
        preferences
            .files
            .insert(markdown.r#ref.to_key().unwrap(), "ideall.code".into());
        assert_eq!(
            resolve_default_engine(&descriptors, &markdown, &preferences)
                .unwrap()
                .source,
            EngineResolutionSource::FilePreference
        );
    }

    #[test]
    fn removed_associations_cannot_remove_every_candidate() {
        let descriptors = builtin_engines();
        let markdown = file("text/markdown");
        let mut preferences = EnginePreferences::default();
        preferences.removed.insert(
            "text/markdown".into(),
            BTreeSet::from(["ideall.code".into(), "ideall.preview".into()]),
        );
        assert!(resolve_default_engine(&descriptors, &markdown, &preferences).is_some());
    }

    #[test]
    fn unfinished_professional_engines_are_explicitly_read_only() {
        let descriptors = builtin_engines();

        for engine_id in ["ideall.audio", "ideall.database"] {
            let descriptor = descriptors
                .iter()
                .find(|descriptor| descriptor.engine_id == engine_id)
                .unwrap_or_else(|| panic!("missing built-in engine {engine_id}"));
            assert_eq!(descriptor.access, EngineAccess::ReadOnly);
        }
    }
}
