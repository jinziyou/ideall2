//! Pure, serializable contracts shared by every ideall platform.
//!
//! This crate deliberately has no UI, storage, networking, or operating-system dependency.

mod engine;
mod file_system;
mod node;
mod sync;

pub use engine::{EngineAccess, EngineDescriptor, EngineLayout, EngineMatcher, EngineSuspension};
pub use file_system::{
    DIRECTORY_MEDIA_TYPE, DirectoryEntry, DirectoryEntryKind, FileKind, FileRef, FileRefKeyError,
    FileSource, FileSourceKind, IdeallFile,
};
pub use node::{
    BaseNode, BlobRef, BookmarkContent, FeedContent, Node, NodeKind, SubscriptionType,
    ThreadContent,
};
pub use sync::{SyncNote, SyncSubscription};
