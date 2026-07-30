//! UI-independent use cases for the native clients.

mod note_document;
mod note_edit;
mod note_rich_text;
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
pub use note_rich_text::{
    NoteColor, NoteInlineDocument, NoteInlineMark, NoteInlineSpan, note_mention_query,
    note_node_id_from_url, note_node_url,
};
pub use workspace::{
    AgentDocumentContext, AgentModelSettings, AgentToolRun, AgentTranscriptMessage,
    AgentTurnResult, ApplicationError, ExternalAcpSettings, HOME_ROOT_ID, INBOX_ROOT_ID,
    LocalWorkspace, NodeSummary, PlainNoteDocument, SyncSettings, TextFileDocument,
};
