//! UI-independent use cases for the native clients.

mod note_document;
mod note_edit;
mod note_rich_text;
mod usecases;
mod workspace;

pub use note_document::{NoteBlockKind, NoteBlockSpec, parse_note_blocks, render_note_block};
pub use note_edit::{
    NoteBlockIndent, NoteBlockMove, NoteBlockStyle, NoteEditError, NoteInlineStyle,
    NoteSlashCommand, apply_note_block_style, apply_note_slash_command, delete_note_block,
    duplicate_note_block, indent_note_block, move_note_block, note_slash_commands,
    note_slash_query, toggle_note_inline_style, toggle_note_todo,
};
pub use note_rich_text::{
    NoteColor, NoteInlineDocument, NoteInlineMark, NoteInlineSpan, note_mention_query,
    note_node_id_from_url, note_node_url,
};
pub use usecases::{
    AGENT_MODEL_SETTINGS_SAVED_STATUS, AGENT_PROMPT_EMPTY_STATUS, AGENT_THREAD_OPENED_STATUS,
    AGENT_TURN_IN_PROGRESS_STATUS, AgentActivity, AgentModelSettingsDraft, CredentialKey,
    CredentialStore, MAX_IMPORT_BYTES, NEW_AGENT_THREAD_STATUS, NotesSyncSummary,
    PersistedAgentModelSettings, PersistedSyncSettings, SEARCH_IN_PROGRESS_STATUS,
    SYNC_CREDENTIALS_CLEARED_STATUS, SYNC_IN_PROGRESS_STATUS, SYNC_SETTINGS_SAVED_STATUS,
    SyncSecretStatus, SyncSettingsDraft, clear_sync_credentials, credential_configured,
    require_credential, search_failed_status, search_results_status, sync_secret_status,
};
pub use workspace::{
    AgentDocumentContext, AgentModelSettings, AgentToolRun, AgentTranscriptMessage,
    AgentTurnResult, ApplicationError, BookmarkMetadata, ExternalAcpSettings, HOME_ROOT_ID,
    INBOX_ROOT_ID, LocalWorkspace, NodeSummary, PlainNoteDocument, SyncSettings, TextFileDocument,
};
