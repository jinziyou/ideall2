//! Verified native desktop update discovery and download.
//!
//! The updater never performs an unattended install. It authenticates a small
//! release manifest with the existing ideall minisign trust root, selects an
//! installer for the current platform, downloads it into an application-owned
//! directory with exact size/SHA-256 checks, and returns the path for an
//! explicit user action.

use std::{
    collections::HashSet,
    fs,
    io::{Read, Write as _},
    path::{Path, PathBuf},
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use minisign_verify::{PublicKey, Signature};
use percent_encoding::percent_decode_str;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use tempfile::NamedTempFile;
use thiserror::Error;
use url::Url;

pub const DEFAULT_PREVIEW_MANIFEST_URL: &str =
    "https://github.com/jinziyou/ideall/releases/download/native-preview/native-preview.json";
pub const DEFAULT_STABLE_MANIFEST_URL: &str =
    "https://github.com/jinziyou/ideall/releases/download/native-stable/native-stable.json";
pub const OFFICIAL_MINISIGN_KEY: &str = "RWRJZd+yMqKmCi+f6bpzu532c25RXBp3NT8jgkTQ2PWbQYvbv2doyAv5";

const MANIFEST_LIMIT: u64 = 512 * 1024;
const SIGNATURE_LIMIT: u64 = 16 * 1024;
const MAX_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ARTIFACTS: usize = 24;
const MAX_NOTES_CHARS: usize = 8 * 1024;

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateManifest {
    pub schema_version: u32,
    pub channel: String,
    pub version: Version,
    pub pub_date: String,
    pub notes: String,
    pub artifacts: Vec<UpdateArtifact>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateArtifact {
    pub target: String,
    pub kind: String,
    pub file: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum UpdateStatus {
    Current { version: Version },
    Available(AvailableUpdate),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AvailableUpdate {
    pub version: Version,
    pub notes: String,
    pub artifact: UpdateArtifact,
}

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("update endpoint must be an HTTPS URL without credentials, query or fragment")]
    InvalidEndpoint,
    #[error("update network request failed")]
    Network,
    #[error("update endpoint returned HTTP {0}")]
    Http(u16),
    #[error("update response exceeded its size budget")]
    ResponseTooLarge,
    #[error("update manifest signature is invalid")]
    InvalidSignature,
    #[error("update manifest is invalid")]
    InvalidManifest,
    #[error("update manifest does not contain an installer for this platform")]
    UnsupportedPlatform,
    #[error("update download did not match the signed size or SHA-256")]
    ArtifactMismatch,
    #[error("update destination is unavailable")]
    Destination,
}

pub struct NativeUpdater {
    agent: ureq::Agent,
    manifest_url: Url,
    channel: String,
}

impl NativeUpdater {
    pub fn new(manifest_url: &str, channel: &str) -> Result<Self, UpdateError> {
        let manifest_url = canonical_https_url(manifest_url)?;
        if !valid_channel(channel) {
            return Err(UpdateError::InvalidEndpoint);
        }
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(45)))
            .https_only(true)
            .max_redirects(5)
            .http_status_as_error(false)
            .build();
        Ok(Self {
            agent: config.into(),
            manifest_url,
            channel: channel.to_owned(),
        })
    }

    pub fn check(&self, current_version: &str) -> Result<UpdateStatus, UpdateError> {
        let current_version =
            Version::parse(current_version).map_err(|_| UpdateError::InvalidManifest)?;
        let manifest_bytes = self.fetch_bounded(self.manifest_url.as_str(), MANIFEST_LIMIT)?;
        let signature_url = signature_url(&self.manifest_url)?;
        let signature = self.fetch_bounded(signature_url.as_str(), SIGNATURE_LIMIT)?;
        let manifest = parse_verified_manifest(&manifest_bytes, &signature, &self.channel)?;
        if manifest.version <= current_version {
            return Ok(UpdateStatus::Current {
                version: current_version,
            });
        }
        let artifact = select_current_artifact(&manifest)?;
        Ok(UpdateStatus::Available(AvailableUpdate {
            version: manifest.version,
            notes: manifest.notes,
            artifact,
        }))
    }

    pub fn download(
        &self,
        update: &AvailableUpdate,
        destination_root: &Path,
    ) -> Result<PathBuf, UpdateError> {
        validate_artifact(&update.artifact)?;
        let directory = destination_root.join(update.version.to_string());
        fs::create_dir_all(&directory).map_err(|_| UpdateError::Destination)?;
        if !fs::symlink_metadata(&directory)
            .map_err(|_| UpdateError::Destination)?
            .file_type()
            .is_dir()
        {
            return Err(UpdateError::Destination);
        }
        let destination = directory.join(&update.artifact.file);
        if destination.is_file() && file_matches(&destination, &update.artifact)? {
            return Ok(destination);
        }

        let artifact_url = canonical_https_url(&update.artifact.url)?;
        let mut response = self
            .agent
            .get(artifact_url.as_str())
            .header("Accept", "application/octet-stream")
            .call()
            .map_err(|_| UpdateError::Network)?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(UpdateError::Http(status));
        }

        write_verified_artifact(
            response.body_mut().as_reader(),
            &update.artifact,
            &directory,
        )
    }

    fn fetch_bounded(&self, url: &str, limit: u64) -> Result<Vec<u8>, UpdateError> {
        let mut response = self
            .agent
            .get(url)
            .header("Accept", "application/json, text/plain")
            .call()
            .map_err(|_| UpdateError::Network)?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(UpdateError::Http(status));
        }
        let mut bytes = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| UpdateError::Network)?;
        if bytes.len() as u64 > limit {
            return Err(UpdateError::ResponseTooLarge);
        }
        Ok(bytes)
    }
}

fn write_verified_artifact(
    mut reader: impl Read,
    artifact: &UpdateArtifact,
    directory: &Path,
) -> Result<PathBuf, UpdateError> {
    let destination = directory.join(&artifact.file);
    let mut temporary = NamedTempFile::new_in(directory).map_err(|_| UpdateError::Destination)?;
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer).map_err(|_| UpdateError::Network)?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or(UpdateError::ArtifactMismatch)?;
        if total > artifact.size || total > MAX_ARTIFACT_BYTES {
            return Err(UpdateError::ArtifactMismatch);
        }
        hasher.update(&buffer[..read]);
        temporary
            .write_all(&buffer[..read])
            .map_err(|_| UpdateError::Destination)?;
    }
    if total != artifact.size || format!("{:x}", hasher.finalize()) != artifact.sha256 {
        return Err(UpdateError::ArtifactMismatch);
    }
    temporary
        .as_file()
        .sync_all()
        .map_err(|_| UpdateError::Destination)?;
    if destination.exists() {
        fs::remove_file(&destination).map_err(|_| UpdateError::Destination)?;
    }
    temporary
        .persist(&destination)
        .map_err(|_| UpdateError::Destination)?;
    if !file_matches(&destination, artifact)? {
        return Err(UpdateError::ArtifactMismatch);
    }
    Ok(destination)
}

pub fn updater_from_environment() -> Result<NativeUpdater, UpdateError> {
    let default_channel = option_env!("IDEALL_NATIVE_DEFAULT_UPDATE_CHANNEL").unwrap_or("preview");
    let channel = std::env::var("IDEALL_NATIVE_UPDATE_CHANNEL")
        .unwrap_or_else(|_| default_channel.to_owned());
    let default_endpoint = if channel == "stable" {
        DEFAULT_STABLE_MANIFEST_URL
    } else {
        DEFAULT_PREVIEW_MANIFEST_URL
    };
    let endpoint =
        std::env::var("IDEALL_NATIVE_UPDATE_URL").unwrap_or_else(|_| default_endpoint.to_owned());
    NativeUpdater::new(&endpoint, &channel)
}

pub fn parse_verified_manifest(
    manifest: &[u8],
    signature: &[u8],
    expected_channel: &str,
) -> Result<UpdateManifest, UpdateError> {
    verify_official(manifest, signature)?;
    parse_manifest_with_verifier(manifest, expected_channel, |_| Ok(()))
}

fn parse_manifest_with_verifier<F>(
    manifest: &[u8],
    expected_channel: &str,
    verifier: F,
) -> Result<UpdateManifest, UpdateError>
where
    F: FnOnce(&[u8]) -> Result<(), UpdateError>,
{
    verifier(manifest)?;
    let manifest: UpdateManifest =
        serde_json::from_slice(manifest).map_err(|_| UpdateError::InvalidManifest)?;
    validate_manifest(&manifest, expected_channel)?;
    Ok(manifest)
}

fn verify_official(content: &[u8], signature: &[u8]) -> Result<(), UpdateError> {
    verify_with_key(OFFICIAL_MINISIGN_KEY, content, signature)
}

fn verify_with_key(key: &str, content: &[u8], signature: &[u8]) -> Result<(), UpdateError> {
    let key = PublicKey::from_base64(key).map_err(|_| UpdateError::InvalidSignature)?;
    let text = std::str::from_utf8(signature).map_err(|_| UpdateError::InvalidSignature)?;
    let signature = if let Ok(signature) = Signature::decode(text.trim()) {
        signature
    } else {
        let decoded = STANDARD
            .decode(text.trim())
            .map_err(|_| UpdateError::InvalidSignature)?;
        let decoded = std::str::from_utf8(&decoded).map_err(|_| UpdateError::InvalidSignature)?;
        Signature::decode(decoded.trim()).map_err(|_| UpdateError::InvalidSignature)?
    };
    key.verify(content, &signature, false)
        .map_err(|_| UpdateError::InvalidSignature)
}

fn validate_manifest(manifest: &UpdateManifest, expected_channel: &str) -> Result<(), UpdateError> {
    if manifest.schema_version != 1
        || !valid_channel(expected_channel)
        || manifest.channel != expected_channel
        || manifest.pub_date.is_empty()
        || manifest.pub_date.len() > 64
        || manifest.pub_date.chars().any(char::is_control)
        || manifest.notes.chars().count() > MAX_NOTES_CHARS
        || manifest.artifacts.is_empty()
        || manifest.artifacts.len() > MAX_ARTIFACTS
    {
        return Err(UpdateError::InvalidManifest);
    }
    let mut identities = HashSet::new();
    for artifact in &manifest.artifacts {
        validate_artifact(artifact)?;
        if !identities.insert((&artifact.target, &artifact.kind)) {
            return Err(UpdateError::InvalidManifest);
        }
    }
    Ok(())
}

fn validate_artifact(artifact: &UpdateArtifact) -> Result<(), UpdateError> {
    if !matches!(
        artifact.target.as_str(),
        "linux-x86_64" | "linux-aarch64" | "darwin-aarch64" | "darwin-x86_64" | "windows-x86_64"
    ) || !matches!(
        artifact.kind.as_str(),
        "tar.gz" | "deb" | "rpm" | "zip" | "dmg" | "msi" | "nsis"
    ) || !safe_file_name(&artifact.file)
        || artifact.size == 0
        || artifact.size > MAX_ARTIFACT_BYTES
        || artifact.sha256.len() != 64
        || !artifact
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(UpdateError::InvalidManifest);
    }
    let url = canonical_https_url(&artifact.url)?;
    if !url_path_file_matches(&url, &artifact.file) {
        return Err(UpdateError::InvalidManifest);
    }
    Ok(())
}

fn url_path_file_matches(url: &Url, file: &str) -> bool {
    let Some(segment) = url.path().rsplit('/').next() else {
        return false;
    };
    percent_decode_str(segment)
        .decode_utf8()
        .is_ok_and(|decoded| decoded == file)
}

fn safe_file_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 180
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-'))
}

fn canonical_https_url(value: &str) -> Result<Url, UpdateError> {
    Url::parse(value.trim())
        .ok()
        .filter(|url| url.scheme() == "https")
        .filter(|url| url.host_str().is_some())
        .filter(|url| url.username().is_empty() && url.password().is_none())
        .filter(|url| url.query().is_none() && url.fragment().is_none())
        .ok_or(UpdateError::InvalidEndpoint)
}

fn signature_url(manifest_url: &Url) -> Result<Url, UpdateError> {
    let mut signature_url = manifest_url.clone();
    let path = format!("{}.sig", signature_url.path());
    signature_url.set_path(&path);
    canonical_https_url(signature_url.as_str())
}

fn valid_channel(channel: &str) -> bool {
    matches!(channel, "preview" | "stable")
}

fn select_current_artifact(manifest: &UpdateManifest) -> Result<UpdateArtifact, UpdateError> {
    let target = current_target().ok_or(UpdateError::UnsupportedPlatform)?;
    let preferences = current_kind_preferences();
    preferences
        .iter()
        .find_map(|kind| {
            manifest
                .artifacts
                .iter()
                .find(|artifact| artifact.target == target && artifact.kind == *kind)
        })
        .cloned()
        .ok_or(UpdateError::UnsupportedPlatform)
}

fn current_target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("linux-x86_64"),
        ("linux", "aarch64") => Some("linux-aarch64"),
        ("macos", "aarch64") => Some("darwin-aarch64"),
        ("macos", "x86_64") => Some("darwin-x86_64"),
        ("windows", "x86_64") => Some("windows-x86_64"),
        _ => None,
    }
}

fn current_kind_preferences() -> Vec<&'static str> {
    #[cfg(target_os = "windows")]
    return vec!["nsis", "msi", "zip"];
    #[cfg(target_os = "macos")]
    return vec!["dmg", "zip"];
    #[cfg(target_os = "linux")]
    {
        if Path::new("/etc/debian_version").exists() {
            return vec!["deb", "tar.gz", "rpm"];
        }
        if Path::new("/etc/redhat-release").exists() {
            return vec!["rpm", "tar.gz", "deb"];
        }
        vec!["tar.gz", "deb", "rpm"]
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    Vec::new()
}

fn file_matches(path: &Path, artifact: &UpdateArtifact) -> Result<bool, UpdateError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| UpdateError::Destination)?;
    if !metadata.file_type().is_file() || metadata.len() != artifact.size {
        return Ok(false);
    }
    let mut file = fs::File::open(path).map_err(|_| UpdateError::Destination)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| UpdateError::Destination)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()) == artifact.sha256)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Vec<u8> {
        br#"{
          "schemaVersion": 1,
          "channel": "preview",
          "version": "1.2.3",
          "pubDate": "2026-07-22T00:00:00Z",
          "notes": "Native preview",
          "artifacts": [{
            "target": "linux-x86_64",
            "kind": "deb",
            "file": "ideall-native-1.2.3-linux-x86_64.deb",
            "url": "https://github.com/jinziyou/ideall/releases/download/native-v1.2.3/ideall-native-1.2.3-linux-x86_64.deb",
            "size": 123,
            "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
          }]
        }"#
        .to_vec()
    }

    #[test]
    fn accepts_a_strict_manifest_after_verification() {
        let parsed = parse_manifest_with_verifier(&manifest(), "preview", |_| Ok(())).unwrap();
        assert_eq!(parsed.version, Version::new(1, 2, 3));
        assert_eq!(parsed.artifacts[0].kind, "deb");
        let updater = NativeUpdater::new(DEFAULT_PREVIEW_MANIFEST_URL, "preview").unwrap();
        assert!(updater.agent.config().https_only());
    }

    #[test]
    fn accepts_percent_encoded_build_metadata_in_asset_name() {
        let with_build = String::from_utf8(manifest())
            .unwrap()
            .replace("1.2.3", "1.2.3+build.4")
            .replace(
                "/ideall-native-1.2.3+build.4-linux",
                "/ideall-native-1.2.3%2Bbuild.4-linux",
            );
        let parsed =
            parse_manifest_with_verifier(with_build.as_bytes(), "preview", |_| Ok(())).unwrap();
        assert_eq!(parsed.version, Version::parse("1.2.3+build.4").unwrap());
        assert_eq!(
            parsed.artifacts[0].file,
            "ideall-native-1.2.3+build.4-linux-x86_64.deb"
        );
    }

    #[test]
    fn rejects_unknown_fields_channels_and_insecure_urls() {
        let unknown = String::from_utf8(manifest())
            .unwrap()
            .replace("\"notes\":", "\"unknown\": true, \"notes\":");
        assert!(matches!(
            parse_manifest_with_verifier(unknown.as_bytes(), "preview", |_| Ok(())),
            Err(UpdateError::InvalidManifest)
        ));
        assert!(matches!(
            parse_manifest_with_verifier(&manifest(), "stable", |_| Ok(())),
            Err(UpdateError::InvalidManifest)
        ));
        let insecure = String::from_utf8(manifest())
            .unwrap()
            .replace("https://github.com", "http://github.com");
        assert!(parse_manifest_with_verifier(insecure.as_bytes(), "preview", |_| Ok(())).is_err());
    }

    #[test]
    fn official_minisign_vector_verifies_and_mutation_fails() {
        let key = "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
        let signature = b"untrusted comment: signature from minisign secret key\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: timestamp:1633700835\tfile:test\tprehashed\nwLMDjy9FLAuxZ3q4NlEvkgtyhrr0gtTu6KC4KBJdITbbOeAi1zBIYo0v4iTgt8jJpIidRJnp94ABQkJAgAooBQ==";
        assert!(verify_with_key(key, b"test", signature).is_ok());
        assert!(verify_with_key(key, b"changed", signature).is_err());
        let encoded = STANDARD.encode(signature);
        assert!(verify_with_key(key, b"test", encoded.as_bytes()).is_ok());
        assert!(PublicKey::from_base64(OFFICIAL_MINISIGN_KEY).is_ok());
    }

    #[test]
    fn duplicate_target_kind_is_rejected() {
        let duplicate = String::from_utf8(manifest())
            .unwrap()
            .replace("}]", "}, {\"target\":\"linux-x86_64\",\"kind\":\"deb\",\"file\":\"other.deb\",\"url\":\"https://example.com/other.deb\",\"size\":1,\"sha256\":\"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"}]");
        assert!(parse_manifest_with_verifier(duplicate.as_bytes(), "preview", |_| Ok(())).is_err());
    }

    #[test]
    fn download_writer_enforces_exact_size_and_digest() {
        let directory = tempfile::tempdir().unwrap();
        let bytes = b"verified installer bytes";
        let artifact = UpdateArtifact {
            target: "linux-x86_64".into(),
            kind: "deb".into(),
            file: "ideall.deb".into(),
            url: "https://example.com/ideall.deb".into(),
            size: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(bytes)),
        };
        let path = write_verified_artifact(bytes.as_slice(), &artifact, directory.path()).unwrap();
        assert_eq!(fs::read(path).unwrap(), bytes);

        let mut too_short = artifact.clone();
        too_short.size -= 1;
        assert!(matches!(
            write_verified_artifact(bytes.as_slice(), &too_short, directory.path()),
            Err(UpdateError::ArtifactMismatch)
        ));
        let mut wrong_hash = artifact;
        wrong_hash.sha256 = "b".repeat(64);
        assert!(matches!(
            write_verified_artifact(bytes.as_slice(), &wrong_hash, directory.path()),
            Err(UpdateError::ArtifactMismatch)
        ));
    }
}
