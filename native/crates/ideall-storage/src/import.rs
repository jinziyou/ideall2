use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;
use uuid::Uuid;

use crate::{
    ArchiveLimits, Database, DatabaseCounts, StorageError, WorkspaceArchiveError,
    decrypt_workspace_archive, is_encrypted_workspace_archive, parse_workspace_archive,
};

#[derive(Debug, Error)]
pub enum AtomicImportError {
    #[error("encrypted workspace archive requires a passphrase")]
    PassphraseRequired,
    #[error(transparent)]
    Archive(#[from] WorkspaceArchiveError),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("workspace database path must have a parent directory")]
    MissingParent,
    #[error("workspace database file operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("new database could not replace the old database; the old database remains intact")]
    ReplaceFailed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AtomicImportResult {
    pub database_path: PathBuf,
    pub backup_path: Option<PathBuf>,
    pub counts: DatabaseCounts,
    pub encrypted: bool,
}

/// Imports through a complete sibling database and switches it into place only
/// after schema, reference, transaction, and SQLite integrity checks succeed.
///
/// The application service must close its live `Database` before calling this
/// function. Desktop and mobile apps never import while a write actor is active.
pub fn import_workspace_archive_atomic(
    database_path: impl AsRef<Path>,
    raw: &str,
    passphrase: Option<&str>,
    limits: ArchiveLimits,
) -> Result<AtomicImportResult, AtomicImportError> {
    let database_path = database_path.as_ref();
    let parent = database_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or(AtomicImportError::MissingParent)?;
    fs::create_dir_all(parent)?;

    let encrypted = is_encrypted_workspace_archive(raw);
    let plaintext = if encrypted {
        let passphrase = passphrase.ok_or(AtomicImportError::PassphraseRequired)?;
        decrypt_workspace_archive(raw, passphrase, limits)?
    } else {
        raw.to_owned()
    };
    let archive = parse_workspace_archive(&plaintext, limits)?;

    let token = Uuid::new_v4().simple().to_string();
    let temporary_path = sibling_path(database_path, &format!("import-{token}.tmp"));
    let backup_path = sibling_path(database_path, &format!("pre-import-{token}.bak"));
    cleanup_database_files(&temporary_path);

    let build_result = (|| -> Result<DatabaseCounts, AtomicImportError> {
        let mut temporary = Database::open(&temporary_path)?;
        temporary.replace_from_archive(&archive)?;
        temporary.quick_check()?;
        let counts = temporary.counts()?;
        temporary.checkpoint()?;
        drop(temporary);
        cleanup_sidecars(&temporary_path);
        Ok(counts)
    })();
    let counts = match build_result {
        Ok(counts) => counts,
        Err(error) => {
            cleanup_database_files(&temporary_path);
            return Err(error);
        }
    };

    let had_existing_database = database_path.exists();
    if had_existing_database {
        let existing = Database::open(database_path)?;
        existing.quick_check()?;
        existing.checkpoint()?;
        drop(existing);
        cleanup_sidecars(database_path);
        fs::copy(database_path, &backup_path)?;
        sync_file(&backup_path)?;
    }

    if replace_database_file(&temporary_path, database_path).is_err() {
        cleanup_database_files(&temporary_path);
        return Err(AtomicImportError::ReplaceFailed);
    }
    sync_parent(parent)?;

    let installed = Database::open(database_path)?;
    installed.quick_check()?;
    let installed_counts = installed.counts()?;
    drop(installed);
    if installed_counts != counts {
        return Err(AtomicImportError::Storage(StorageError::Integrity(
            "post-install row counts changed".into(),
        )));
    }

    Ok(AtomicImportResult {
        database_path: database_path.to_owned(),
        backup_path: had_existing_database.then_some(backup_path),
        counts,
        encrypted,
    })
}

fn sibling_path(database_path: &Path, suffix: &str) -> PathBuf {
    let name = database_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("ideall.db");
    database_path.with_file_name(format!(".{name}.{suffix}"))
}

#[cfg(unix)]
fn replace_database_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    // POSIX rename replaces a file in the same directory atomically.
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_database_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;

    const REPLACEFILE_WRITE_THROUGH: u32 = 0x0000_0001;
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn ReplaceFileW(
            replaced: *const u16,
            replacement: *const u16,
            backup: *const u16,
            flags: u32,
            exclude: *const core::ffi::c_void,
            reserved: *const core::ffi::c_void,
        ) -> i32;
        fn MoveFileExW(existing: *const u16, new_name: *const u16, flags: u32) -> i32;
    }

    let destination_exists = destination.exists();
    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let changed = if destination_exists {
        // SAFETY: both paths are owned, NUL-terminated UTF-16 buffers that remain
        // alive for the call; optional pointers are null as required by Win32.
        unsafe {
            ReplaceFileW(
                destination.as_ptr(),
                source.as_ptr(),
                std::ptr::null(),
                REPLACEFILE_WRITE_THROUGH,
                std::ptr::null(),
                std::ptr::null(),
            )
        }
    } else {
        // SAFETY: both path buffers are valid and NUL-terminated for this call.
        unsafe {
            MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        }
    };
    if changed == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(all(not(unix), not(windows)))]
fn replace_database_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
}

fn sidecar_path(database_path: &Path, suffix: &str) -> PathBuf {
    let mut value = database_path.as_os_str().to_owned();
    value.push(suffix);
    PathBuf::from(value)
}

fn cleanup_sidecars(database_path: &Path) {
    for suffix in ["-wal", "-shm"] {
        let _ = fs::remove_file(sidecar_path(database_path, suffix));
    }
}

fn cleanup_database_files(database_path: &Path) {
    let _ = fs::remove_file(database_path);
    cleanup_sidecars(database_path);
}

fn sync_file(path: &Path) -> std::io::Result<()> {
    fs::File::open(path)?.sync_all()
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> std::io::Result<()> {
    fs::File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::*;

    fn archive_raw(id: &str, title: &str) -> String {
        let exported_at = "2026-01-01T00:00:00.000Z";
        let core = json!({
            "nodes": [{
                "id": id, "kind": "note", "parentId": null, "sortKey": "a0",
                "title": title, "tags": [], "createdAt": 1, "updatedAt": 2,
                "content": [{"type": "p", "children": [{"text": title}]}]
            }],
            "blobs": [], "trashSnapshots": [], "workspace": null
        });
        let plugins = json!({
            "kind": "ideall.workspace-backup", "version": 1,
            "exportedAt": exported_at, "plugins": []
        });
        let checksum = format!(
            "{:08x}",
            crc32fast::hash(
                &serde_json::to_vec(&json!({
                    "exportedAt": exported_at, "core": core, "plugins": plugins
                }))
                .unwrap()
            )
        );
        serde_json::to_string(&json!({
            "kind": "ideall.workspace-archive", "version": 2,
            "exportedAt": exported_at, "core": core, "plugins": plugins,
            "manifest": {
                "algorithm": "crc32", "checksum": checksum,
                "nodeCount": 1, "blobCount": 0, "blobBytes": 0,
                "trashSnapshotCount": 0, "pluginCount": 0, "tabCount": 0
            }
        }))
        .unwrap()
    }

    #[test]
    fn replaces_database_and_keeps_a_readable_backup() {
        let directory = tempdir().unwrap();
        let database_path = directory.path().join("ideall.db");

        let first = import_workspace_archive_atomic(
            &database_path,
            &archive_raw("old", "Old"),
            None,
            ArchiveLimits::default(),
        )
        .unwrap();
        assert_eq!(first.backup_path, None);

        let second = import_workspace_archive_atomic(
            &database_path,
            &archive_raw("new", "New"),
            None,
            ArchiveLimits::default(),
        )
        .unwrap();
        let installed = Database::open(&database_path).unwrap();
        assert!(installed.get_node("new").unwrap().is_some());
        assert!(installed.get_node("old").unwrap().is_none());

        let backup = Database::open(second.backup_path.unwrap()).unwrap();
        assert!(backup.get_node("old").unwrap().is_some());
        assert!(backup.get_node("new").unwrap().is_none());
    }

    #[test]
    fn rejected_archive_leaves_existing_database_unchanged() {
        let directory = tempdir().unwrap();
        let database_path = directory.path().join("ideall.db");
        import_workspace_archive_atomic(
            &database_path,
            &archive_raw("old", "Old"),
            None,
            ArchiveLimits::default(),
        )
        .unwrap();

        let mut tampered: Value = serde_json::from_str(&archive_raw("new", "New")).unwrap();
        tampered["core"]["nodes"][0]["title"] = Value::String("tampered".into());
        assert!(
            import_workspace_archive_atomic(
                &database_path,
                &serde_json::to_string(&tampered).unwrap(),
                None,
                ArchiveLimits::default(),
            )
            .is_err()
        );

        let database = Database::open(&database_path).unwrap();
        assert!(database.get_node("old").unwrap().is_some());
        assert!(database.get_node("new").unwrap().is_none());
    }
}
