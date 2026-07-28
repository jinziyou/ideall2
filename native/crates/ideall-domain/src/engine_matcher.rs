use ideall_protocol::{EngineMatcher, IdeallFile};

pub fn engine_matches(matcher: &EngineMatcher, file: &IdeallFile) -> bool {
    engine_match_specificity(matcher, file).is_some()
}

pub fn engine_match_specificity(matcher: &EngineMatcher, file: &IdeallFile) -> Option<i32> {
    let mut specificity = 0;
    if !matcher.kinds.is_empty() && !matcher.kinds.contains(&file.kind) {
        return None;
    }
    if !matcher.kinds.is_empty() {
        specificity += 40;
    }
    if !matcher.media_types.is_empty() {
        let best = matcher
            .media_types
            .iter()
            .filter_map(|pattern| media_type_match_score(pattern, &file.media_type))
            .max()?;
        specificity += best;
    }
    if matcher
        .required_capabilities
        .iter()
        .any(|required| !file.capabilities.contains(required))
    {
        return None;
    }
    specificity += matcher.required_capabilities.len() as i32 * 5;
    if !matcher
        .properties
        .iter()
        .all(|(key, expected)| file.properties.get(key) == Some(expected))
    {
        return None;
    }
    specificity += matcher.properties.len() as i32 * 10;
    Some(specificity)
}

fn media_type_match_score(pattern: &str, actual: &str) -> Option<i32> {
    let pattern = pattern.trim().to_ascii_lowercase();
    let actual = normalize_media_type(actual);
    direct_media_type_score(&pattern, &actual).or_else(|| {
        media_type_ancestors(&actual)
            .into_iter()
            .enumerate()
            .filter_map(|(index, ancestor)| {
                direct_media_type_score(&pattern, ancestor)
                    .map(|score| score - 150 * (index as i32 + 1))
                    .filter(|score| *score > 0)
            })
            .max()
    })
}

fn direct_media_type_score(pattern: &str, actual: &str) -> Option<i32> {
    if pattern == "*" || pattern == "*/*" {
        return Some(1);
    }
    if !pattern.contains('*') {
        return (pattern == actual).then_some(400);
    }
    if !wildcard_matches(pattern, actual) {
        return None;
    }
    let literal_length = pattern.replace('*', "").len() as i32;
    Some(if pattern.ends_with("/*") {
        200 + literal_length
    } else {
        300 + literal_length
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

pub fn media_type_ancestors(media_type: &str) -> Vec<&'static str> {
    match normalize_media_type(media_type).as_str() {
        "text/markdown"
        | "text/csv"
        | "text/uri-list"
        | "application/json"
        | "application/javascript"
        | "application/typescript"
        | "application/xml"
        | "application/yaml"
        | "application/toml" => vec!["text/plain"],
        "image/svg+xml" => vec!["application/xml", "text/plain"],
        "application/ld+json" => vec!["application/json", "text/plain"],
        _ => Vec::new(),
    }
}

fn wildcard_matches(pattern: &str, value: &str) -> bool {
    if pattern == "*" || pattern == "*/*" {
        return true;
    }
    let parts: Vec<_> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == value;
    }

    let mut cursor = 0;
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        let Some(found) = value[cursor..].find(part) else {
            return false;
        };
        if index == 0 && found != 0 {
            return false;
        }
        cursor += found + part.len();
    }
    pattern.ends_with('*') || parts.last().is_some_and(|last| value.ends_with(last))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ideall_protocol::{FileKind, FileRef, FileSource, FileSourceKind};
    use serde_json::json;

    use super::*;

    fn file() -> IdeallFile {
        IdeallFile {
            r#ref: FileRef::new("local", "1"),
            kind: FileKind::File,
            name: "config.json".into(),
            media_type: "application/vnd.ideall.settings+json; charset=utf-8".into(),
            capabilities: vec!["read".into(), "write".into()],
            source: FileSource {
                kind: FileSourceKind::Local,
                id: "local".into(),
                label: None,
                read_only: None,
            },
            size: None,
            created_at: None,
            updated_at: None,
            version: None,
            properties: BTreeMap::from([("section".into(), json!("appearance"))]),
        }
    }

    #[test]
    fn matches_kind_suffix_media_type_capability_and_property() {
        let matcher = EngineMatcher {
            kinds: vec![FileKind::File],
            media_types: vec!["application/*+json".into()],
            required_capabilities: vec!["read".into()],
            properties: BTreeMap::from([("section".into(), json!("appearance"))]),
        };
        assert!(engine_matches(&matcher, &file()));
    }

    #[test]
    fn rejects_missing_capability() {
        let matcher = EngineMatcher {
            required_capabilities: vec!["delete".into()],
            ..EngineMatcher::default()
        };
        assert!(!engine_matches(&matcher, &file()));
    }

    #[test]
    fn exact_media_type_is_more_specific_than_wildcard_and_parent() {
        let exact = EngineMatcher {
            media_types: vec!["text/markdown".into()],
            ..EngineMatcher::default()
        };
        let wildcard = EngineMatcher {
            media_types: vec!["text/*".into()],
            ..EngineMatcher::default()
        };
        let parent = EngineMatcher {
            media_types: vec!["text/plain".into()],
            ..EngineMatcher::default()
        };
        let mut markdown = file();
        markdown.media_type = "text/markdown".into();
        assert!(
            engine_match_specificity(&exact, &markdown)
                > engine_match_specificity(&wildcard, &markdown)
        );
        assert!(engine_match_specificity(&parent, &markdown).is_some());
    }
}
