//! UI-independent use cases for the native clients.

mod note_document;
mod note_edit;
mod workspace;

pub use ideall_agent::{
    AgentAuditRecord, AuditStatus, ModelRole, OpenAiCompatibleClient, ToolEffect, ToolRisk,
};
pub use note_edit::{
    NoteBlockIndent, NoteBlockMove, NoteBlockStyle, NoteEditError, NoteInlineStyle,
    NoteSlashCommand, apply_note_block_style, apply_note_slash_command, delete_note_block,
    duplicate_note_block, indent_note_block, move_note_block, note_slash_commands,
    note_slash_query, toggle_note_inline_style, toggle_note_todo,
};
pub use workspace::{
    AgentModelSettings, AgentToolRun, AgentTranscriptMessage, AgentTurnResult, ApplicationError,
    ExternalAcpSettings, HOME_ROOT_ID, LocalWorkspace, NodeSummary, PlainNoteDocument,
    SyncSettings, TextFileDocument,
};
