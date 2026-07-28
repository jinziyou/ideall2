//! Durable local storage and one-way migration adapters.
//!
//! SQLite owns native data. IndexedDB is only accepted through the validated V2
//! workspace archive format so the old and new applications never share a live store.

mod archive;
mod database;
mod import;

pub use archive::{
    ArchiveBlob, ArchiveLimits, ArchiveManifest, ArchivePluginDescriptor, ArchivePluginPackage,
    ArchiveTrashSnapshot, ArchiveWorkspace, ParsedArchive, WorkspaceArchiveError,
    decrypt_workspace_archive, is_encrypted_workspace_archive, parse_workspace_archive,
};
pub use database::{Database, DatabaseCounts, NodeProjection, StorageError};
pub use import::{AtomicImportError, AtomicImportResult, import_workspace_archive_atomic};
