use super::*;
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
};

use gpui_component::input::Position;

mod commands;
mod rich_input;
mod view;

use rich_input::{RichInputEvent, RichInputMode, RichInputState, RichNavigation};
pub(crate) use view::{EditorMode, engine_icon};

pub(super) struct NoteEditorBlock {
    pub(super) id: u64,
    pub(super) kind: NoteBlockKind,
    pub(in crate::editor) input: Entity<RichInputState>,
    pub(super) indent: usize,
    pub(super) checked: bool,
}

impl NoteEditorBlock {
    pub(super) fn to_spec(&self, cx: &Context<IdeallDesktop>) -> NoteBlockSpec {
        NoteBlockSpec {
            kind: self.kind.clone(),
            text: self.input.read(cx).markdown(),
            indent: self.indent,
            checked: self.checked,
        }
    }

    pub(super) fn markdown(&self, cx: &Context<IdeallDesktop>) -> String {
        render_note_block(&self.to_spec(cx))
    }
}

#[derive(Clone)]
pub(super) struct DraggedNoteBlocks {
    pub(super) ids: Vec<u64>,
}

impl Render for DraggedNoteBlocks {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .py_2()
            .rounded_md()
            .border_1()
            .border_color(rgb(BORDER_STRONG))
            .bg(rgb(SURFACE_RAISED))
            .text_sm()
            .text_color(rgb(INK))
            .child(if self.ids.len() == 1 {
                "移动 1 个内容块".to_owned()
            } else {
                format!("移动 {} 个内容块", self.ids.len())
            })
    }
}

pub(super) fn note_block_placeholder(kind: &NoteBlockKind) -> &'static str {
    match kind {
        NoteBlockKind::Code { .. } => "输入代码；Enter 换行",
        NoteBlockKind::Callout { .. } => "输入需要强调的内容",
        NoteBlockKind::Toggle { .. } => "第一行作为摘要，其余内容可折叠",
        NoteBlockKind::Equation => "输入 LaTeX 公式",
        NoteBlockKind::Bookmark => "输入完整的 https:// 地址",
        _ => "输入内容，键入 / 选择块类型",
    }
}

pub(super) fn note_blocks_markdown(
    blocks: &[NoteEditorBlock],
    cx: &Context<IdeallDesktop>,
) -> String {
    blocks
        .iter()
        .map(|block| block.markdown(cx))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn note_blocks_plain_text(
    blocks: &[NoteEditorBlock],
    cx: &Context<IdeallDesktop>,
) -> String {
    blocks
        .iter()
        .map(|block| block.input.read(cx).text().to_owned())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Owns the complete state of the active desktop editor workspace.
///
/// Persistence stays in the application shell so this UI state never leaks
/// GPUI types into shared crates.
pub(super) struct EditorWorkspace {
    pub(super) engines: Vec<EngineDescriptor>,
    pub(super) engine_preferences: EnginePreferences,
    pub(super) workspace: WorkspaceState,
    pub(super) selected_id: Option<String>,
    pub(super) selected_kind: Option<NodeKind>,
    pub(super) title_input: Entity<InputState>,
    pub(super) body_input: Entity<InputState>,
    pub(super) text_input: Entity<InputState>,
    pub(super) find_input: Entity<InputState>,
    pub(super) replace_input: Entity<InputState>,
    pub(super) link_input: Entity<InputState>,
    pub(super) find_visible: bool,
    pub(super) replace_visible: bool,
    pub(super) find_cursor: usize,
    pub(super) note_preview: bool,
    note_blocks: Vec<NoteEditorBlock>,
    active_note_block: usize,
    selected_note_blocks: HashSet<u64>,
    note_selection_anchor: Option<usize>,
    note_block_subscriptions: Vec<Subscription>,
    next_note_block_id: u64,
    note_undo: Vec<Vec<NoteBlockSpec>>,
    note_redo: Vec<Vec<NoteBlockSpec>>,
    note_snapshot: Vec<NoteBlockSpec>,
    note_history_suspended: bool,
    pub(super) link_editor: Option<NoteLinkEditor>,
    sessions: HashMap<String, DocumentSession>,
    pending_cursor_restore: Option<Position>,
    pub(super) active_engine_id: Option<String>,
    pub(super) body_editable: bool,
    pub(super) suppress_events: bool,
    pub(super) draft_revision: u64,
    pub(super) autosave_task: Option<Task<()>>,
    pub(super) dirty: bool,
}

pub(super) struct EditorDraft {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) body: String,
    pub(super) body_editable: bool,
}

pub(super) struct LoadedDocument {
    pub(super) id: String,
    pub(super) kind: NodeKind,
    pub(super) title: String,
    pub(super) body: String,
    pub(super) language: &'static str,
    pub(super) editable: bool,
}

pub(super) struct EditorInputs {
    pub(super) title: Entity<InputState>,
    pub(super) body: Entity<InputState>,
    pub(super) text: Entity<InputState>,
    pub(super) find: Entity<InputState>,
    pub(super) replace: Entity<InputState>,
    pub(super) link: Entity<InputState>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct NoteLinkEditor {
    pub(super) block_id: u64,
    pub(super) range: Range<usize>,
    pub(super) has_existing_link: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct DocumentSession {
    cursor: Position,
    note_block: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct EditorMetrics {
    pub(super) lines: usize,
    pub(super) characters: usize,
    pub(super) cursor_line: u32,
    pub(super) cursor_column: u32,
}

impl EditorWorkspace {
    pub(super) fn new(
        engines: Vec<EngineDescriptor>,
        engine_preferences: EnginePreferences,
        workspace: WorkspaceState,
        inputs: EditorInputs,
    ) -> Self {
        Self {
            engines,
            engine_preferences,
            workspace,
            selected_id: None,
            selected_kind: None,
            title_input: inputs.title,
            body_input: inputs.body,
            text_input: inputs.text,
            find_input: inputs.find,
            replace_input: inputs.replace,
            link_input: inputs.link,
            find_visible: false,
            replace_visible: false,
            find_cursor: 0,
            note_preview: false,
            note_blocks: Vec::new(),
            active_note_block: 0,
            selected_note_blocks: HashSet::new(),
            note_selection_anchor: None,
            note_block_subscriptions: Vec::new(),
            next_note_block_id: 1,
            note_undo: Vec::new(),
            note_redo: Vec::new(),
            note_snapshot: Vec::new(),
            note_history_suspended: false,
            link_editor: None,
            sessions: HashMap::new(),
            pending_cursor_restore: None,
            active_engine_id: None,
            body_editable: false,
            suppress_events: false,
            draft_revision: 0,
            autosave_task: None,
            dirty: false,
        }
    }

    pub(super) fn has_selection(&self) -> bool {
        self.selected_id.is_some()
    }

    pub(super) fn draft(&self, cx: &Context<IdeallDesktop>) -> Option<EditorDraft> {
        let id = self.selected_id.clone()?;
        let title = self.title_input.read(cx).value().to_string();
        let body = if self.selected_kind == Some(NodeKind::Note) && !self.note_blocks.is_empty() {
            note_blocks_markdown(&self.note_blocks, cx)
        } else if self.uses_text_input() {
            self.text_input.read(cx).value().to_string()
        } else {
            self.body_input.read(cx).value().to_string()
        };
        Some(EditorDraft {
            id,
            title,
            body,
            body_editable: self.body_editable,
        })
    }

    pub(super) fn load_document(
        &mut self,
        document: LoadedDocument,
        window: &mut Window,
        cx: &mut Context<IdeallDesktop>,
    ) {
        let note_preview = document.kind == NodeKind::Note && !document.body.trim().is_empty();
        self.capture_active_session(cx);
        self.pending_cursor_restore = self
            .sessions
            .get(&document.id)
            .map(|session| session.cursor);
        self.suppress_events = true;
        self.title_input
            .update(cx, |input, cx| input.set_value(document.title, window, cx));
        self.body_input.update(cx, |input, cx| {
            input.set_value(document.body.clone(), window, cx)
        });
        self.text_input.update(cx, |input, cx| {
            input.set_highlighter(document.language, cx);
            input.set_value(document.body, window, cx);
        });
        self.note_blocks.clear();
        self.link_editor = None;
        self.selected_note_blocks.clear();
        self.note_selection_anchor = None;
        self.note_block_subscriptions.clear();
        self.active_note_block = 0;
        if document.kind == NodeKind::Note {
            for block in parse_note_blocks(self.body_input.read(cx).value().as_ref()) {
                self.push_note_block(block, window, cx);
            }
        }
        self.note_undo.clear();
        self.note_redo.clear();
        self.note_snapshot = self.note_snapshot(cx);
        self.note_history_suspended = false;
        self.suppress_events = false;
        self.selected_kind = Some(document.kind);
        self.selected_id = Some(document.id);
        self.body_editable = document.editable;
        self.note_preview = note_preview;
        self.set_note_rich_inputs_read_only(note_preview, cx);
        self.dirty = false;
    }

    pub(super) fn clear(&mut self, window: &mut Window, cx: &mut Context<IdeallDesktop>) {
        self.capture_active_session(cx);
        self.suppress_events = true;
        self.selected_id = None;
        self.selected_kind = None;
        self.body_editable = false;
        self.dirty = false;
        self.title_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.body_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.text_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.suppress_events = false;
        self.active_engine_id = None;
        self.find_visible = false;
        self.replace_visible = false;
        self.find_cursor = 0;
        self.note_preview = false;
        self.note_blocks.clear();
        self.link_editor = None;
        self.selected_note_blocks.clear();
        self.note_selection_anchor = None;
        self.note_block_subscriptions.clear();
        self.active_note_block = 0;
        self.note_undo.clear();
        self.note_redo.clear();
        self.note_snapshot.clear();
        self.note_history_suspended = false;
    }

    pub(super) fn mark_dirty(&mut self) -> Option<u64> {
        if self.suppress_events || self.selected_id.is_none() {
            return None;
        }
        self.dirty = true;
        self.draft_revision = self.draft_revision.wrapping_add(1);
        Some(self.draft_revision)
    }

    pub(super) fn capture_active_session(&mut self, cx: &Context<IdeallDesktop>) {
        let Some(id) = self.selected_id.clone() else {
            return;
        };
        let cursor = self
            .active_note_input()
            .map(|input| {
                let input = input.read(cx);
                position_for_byte_offset(input.text(), input.cursor_offset())
            })
            .unwrap_or_else(|| self.active_input().read(cx).cursor_position());
        self.sessions.insert(
            id,
            DocumentSession {
                cursor,
                note_block: self.active_note_block,
            },
        );
    }

    pub(super) fn restore_active_session(
        &mut self,
        window: &mut Window,
        cx: &mut Context<IdeallDesktop>,
    ) {
        let Some(cursor) = self.pending_cursor_restore.take() else {
            return;
        };
        if let Some(id) = self.selected_id.as_deref()
            && let Some(session) = self.sessions.get(id)
        {
            self.active_note_block = session
                .note_block
                .min(self.note_blocks.len().saturating_sub(1));
        }
        if let Some(input) = self.active_note_input() {
            let offset = byte_offset_for_position(input.read(cx).text(), cursor);
            input.update(cx, |input, cx| input.set_cursor_offset(offset, cx));
        } else {
            self.active_input().update(cx, |input, cx| {
                input.set_cursor_position(cursor, window, cx)
            });
        }
    }

    pub(super) fn remove_session(&mut self, id: &str) {
        self.sessions.remove(id);
    }

    pub(super) fn active_input(&self) -> &Entity<InputState> {
        if self.uses_text_input() {
            &self.text_input
        } else {
            &self.body_input
        }
    }

    /// The single input of the active note block, if a note with blocks is
    /// loaded. Prose blocks hold a rich-mode input; all other blocks hold a
    /// plain-mode one.
    pub(in crate::editor) fn active_note_input(&self) -> Option<&Entity<RichInputState>> {
        if self.selected_kind != Some(NodeKind::Note) || self.note_blocks.is_empty() {
            return None;
        }
        Some(
            &self.note_blocks[self
                .active_note_block
                .min(self.note_blocks.len().saturating_sub(1))]
            .input,
        )
    }

    /// The active note block input when the block edits inline rich text.
    fn active_rich_input(&self) -> Option<&Entity<RichInputState>> {
        if self.selected_kind != Some(NodeKind::Note) || self.note_blocks.is_empty() {
            return None;
        }
        let block = &self.note_blocks[self
            .active_note_block
            .min(self.note_blocks.len().saturating_sub(1))];
        block
            .kind
            .supports_inline_rich_text()
            .then_some(&block.input)
    }

    pub(super) fn active_note_query_context(
        &self,
        cx: &Context<IdeallDesktop>,
    ) -> Option<(String, usize)> {
        let input = self.active_note_input()?.read(cx);
        Some((input.text().to_owned(), input.cursor_offset()))
    }

    pub(super) fn active_note_mention_context(
        &self,
        cx: &Context<IdeallDesktop>,
    ) -> Option<(Range<usize>, String)> {
        let input = self.active_note_input()?.read(cx);
        if input.mode() == RichInputMode::Rich {
            return input.mention_query();
        }
        note_mention_query(input.text(), input.cursor_offset())
            .map(|(range, query)| (range, query.to_owned()))
    }

    pub(super) fn focus_active_editor(&self, window: &mut Window, cx: &mut Context<IdeallDesktop>) {
        if let Some(input) = self.active_note_input() {
            input.update(cx, |input, _| input.focus(window));
        } else {
            self.active_input()
                .update(cx, |input, cx| input.focus(window, cx));
        }
    }

    pub(super) fn uses_text_input(&self) -> bool {
        self.selected_kind == Some(NodeKind::File)
            && matches!(
                self.active_engine_id.as_deref(),
                Some("ideall.text" | "ideall.preview")
            )
    }

    pub(super) fn active_text(&self, cx: &Context<IdeallDesktop>) -> String {
        if self.selected_kind == Some(NodeKind::Note) && !self.note_blocks.is_empty() {
            note_blocks_plain_text(&self.note_blocks, cx)
        } else {
            self.active_input().read(cx).value().to_string()
        }
    }

    pub(super) fn active_cursor_position(&self, cx: &Context<IdeallDesktop>) -> Position {
        if self.selected_kind != Some(NodeKind::Note) || self.note_blocks.is_empty() {
            return self.active_input().read(cx).cursor_position();
        }
        let active = self
            .active_note_block
            .min(self.note_blocks.len().saturating_sub(1));
        let cursor = {
            let input = self.note_blocks[active].input.read(cx);
            position_for_byte_offset(input.text(), input.cursor_offset())
        };
        let preceding_lines = self.note_blocks[..active]
            .iter()
            .map(|block| {
                block
                    .input
                    .read(cx)
                    .text()
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count()
                    + 1
            })
            .sum::<usize>();
        Position::new(
            cursor.line.saturating_add(preceding_lines as u32),
            cursor.character,
        )
    }

    pub(super) fn has_note_blocks(&self) -> bool {
        !self.note_blocks.is_empty()
    }

    pub(super) fn find_match_count(&self, cx: &Context<IdeallDesktop>) -> usize {
        let query = self.find_input.read(cx).value().to_string();
        if query.is_empty() {
            return 0;
        }
        self.active_text(cx).match_indices(&query).count()
    }

    pub(super) fn focus_note_match(
        &mut self,
        mut offset: usize,
        window: &mut Window,
        cx: &mut Context<IdeallDesktop>,
    ) -> Option<Position> {
        for (index, block) in self.note_blocks.iter().enumerate() {
            let text = block.input.read(cx).text().to_owned();
            if offset <= text.len() {
                let position = position_for_byte_offset(&text, offset);
                self.active_note_block = index;
                block.input.update(cx, |input, cx| {
                    input.set_cursor_offset(offset, cx);
                    input.focus(window);
                });
                return Some(position);
            }
            offset = offset.checked_sub(text.len() + 1)?;
        }
        None
    }

    pub(super) fn replace_all_note_blocks(
        &mut self,
        query: &str,
        replacement: &str,
        window: &mut Window,
        cx: &mut Context<IdeallDesktop>,
    ) -> usize {
        let mut count = 0;
        self.suppress_events = true;
        for block in &self.note_blocks {
            if matches!(block.kind, NoteBlockKind::Protected | NoteBlockKind::Rule) {
                continue;
            }
            count += block
                .input
                .update(cx, |input, cx| input.replace_all(query, replacement, cx));
        }
        if count > 0 {
            self.commit_note_history(cx);
            let markdown = note_blocks_markdown(&self.note_blocks, cx);
            self.body_input
                .update(cx, |input, cx| input.set_value(markdown, window, cx));
        }
        self.suppress_events = false;
        count
    }

    fn note_snapshot(&self, cx: &Context<IdeallDesktop>) -> Vec<NoteBlockSpec> {
        self.note_blocks
            .iter()
            .map(|block| block.to_spec(cx))
            .collect()
    }

    pub(super) fn commit_note_history(&mut self, cx: &Context<IdeallDesktop>) {
        if self.note_history_suspended {
            return;
        }
        let current = self.note_snapshot(cx);
        if current == self.note_snapshot {
            return;
        }
        let previous = std::mem::replace(&mut self.note_snapshot, current);
        self.note_undo.push(previous);
        if self.note_undo.len() > 32 {
            self.note_undo.remove(0);
        }
        self.note_redo.clear();
    }

    pub(super) fn undo_note_document(
        &mut self,
        window: &mut Window,
        cx: &mut Context<IdeallDesktop>,
    ) -> bool {
        let Some(previous) = self.note_undo.pop() else {
            return false;
        };
        let current = self.note_snapshot(cx);
        self.note_redo.push(current);
        self.restore_note_snapshot(previous, window, cx);
        true
    }

    pub(super) fn redo_note_document(
        &mut self,
        window: &mut Window,
        cx: &mut Context<IdeallDesktop>,
    ) -> bool {
        let Some(next) = self.note_redo.pop() else {
            return false;
        };
        let current = self.note_snapshot(cx);
        self.note_undo.push(current);
        self.restore_note_snapshot(next, window, cx);
        true
    }

    fn restore_note_snapshot(
        &mut self,
        snapshot: Vec<NoteBlockSpec>,
        window: &mut Window,
        cx: &mut Context<IdeallDesktop>,
    ) {
        let active = self.active_note_block.min(snapshot.len().saturating_sub(1));
        self.note_history_suspended = true;
        self.suppress_events = true;
        self.note_blocks.clear();
        self.link_editor = None;
        self.selected_note_blocks.clear();
        self.note_selection_anchor = None;
        self.note_block_subscriptions.clear();
        for block in snapshot.iter().cloned() {
            self.push_note_block(block, window, cx);
        }
        self.active_note_block = active.min(self.note_blocks.len().saturating_sub(1));
        let markdown = note_blocks_markdown(&self.note_blocks, cx);
        self.body_input
            .update(cx, |input, cx| input.set_value(markdown, window, cx));
        self.note_snapshot = snapshot;
        self.suppress_events = false;
        self.note_history_suspended = false;
    }

    pub(super) fn note_block_index(&self, id: u64) -> Option<usize> {
        self.note_blocks.iter().position(|block| block.id == id)
    }

    pub(super) fn selected_note_block_ids(&self) -> Vec<u64> {
        if self.selected_note_blocks.is_empty() {
            return self
                .note_blocks
                .get(
                    self.active_note_block
                        .min(self.note_blocks.len().saturating_sub(1)),
                )
                .map(|block| vec![block.id])
                .unwrap_or_default();
        }
        self.note_blocks
            .iter()
            .filter(|block| self.selected_note_blocks.contains(&block.id))
            .map(|block| block.id)
            .collect()
    }

    pub(super) fn selected_note_block_indices(&self) -> Vec<usize> {
        let ids = self
            .selected_note_block_ids()
            .into_iter()
            .collect::<HashSet<_>>();
        self.note_blocks
            .iter()
            .enumerate()
            .filter_map(|(index, block)| ids.contains(&block.id).then_some(index))
            .collect()
    }

    fn push_note_block(
        &mut self,
        block: NoteBlockSpec,
        window: &mut Window,
        cx: &mut Context<IdeallDesktop>,
    ) -> u64 {
        self.insert_note_block(self.note_blocks.len(), block, window, cx)
    }

    fn insert_note_block(
        &mut self,
        index: usize,
        block: NoteBlockSpec,
        window: &mut Window,
        cx: &mut Context<IdeallDesktop>,
    ) -> u64 {
        let id = self.next_note_block_id;
        self.next_note_block_id = self.next_note_block_id.wrapping_add(1).max(1);
        let placeholder = note_block_placeholder(&block.kind);
        let input = cx.new(|cx| {
            if block.kind.supports_inline_rich_text() {
                RichInputState::new(&block.text, placeholder, cx)
            } else {
                RichInputState::plain(&block.text, placeholder, cx)
            }
        });
        let subscription = cx.subscribe_in(
            &input,
            window,
            move |this, _, event: &RichInputEvent, window, cx| {
                this.handle_note_rich_input(id, event, window, cx);
            },
        );
        self.note_block_subscriptions.push(subscription);
        self.note_blocks.insert(
            index.min(self.note_blocks.len()),
            NoteEditorBlock {
                id,
                kind: block.kind,
                input,
                indent: block.indent,
                checked: block.checked,
            },
        );
        id
    }

    pub(super) fn set_note_rich_inputs_read_only(
        &self,
        read_only: bool,
        cx: &mut Context<IdeallDesktop>,
    ) {
        for block in &self.note_blocks {
            block
                .input
                .update(cx, |input, cx| input.set_read_only(read_only, cx));
        }
    }

    /// Re-aligns the block input mode and placeholder with the block kind
    /// after a conversion. The Markdown source stays the block body.
    pub(super) fn refresh_note_block_input(
        &mut self,
        index: usize,
        cx: &mut Context<IdeallDesktop>,
    ) {
        let Some(block) = self.note_blocks.get(index) else {
            return;
        };
        let placeholder = note_block_placeholder(&block.kind);
        let mode = if block.kind.supports_inline_rich_text() {
            RichInputMode::Rich
        } else {
            RichInputMode::Plain
        };
        block.input.update(cx, |input, cx| {
            input.set_placeholder(placeholder, cx);
            input.set_mode(mode, cx);
        });
    }
}

pub(super) fn editor_metrics(text: &str, cursor: Position) -> EditorMetrics {
    EditorMetrics {
        lines: text.bytes().filter(|byte| *byte == b'\n').count() + 1,
        characters: text.chars().count(),
        cursor_line: cursor.line.saturating_add(1),
        cursor_column: cursor.character.saturating_add(1),
    }
}

pub(super) fn text_highlight_for_node(service: &LocalWorkspace, id: &str) -> &'static str {
    let Ok(file) = service.file_metadata(id) else {
        return "text";
    };
    text_highlight_language(&file.media_type, &file.name)
}

fn text_highlight_language(media_type: &str, name: &str) -> &'static str {
    let extension = name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase());
    match (media_type, extension.as_deref()) {
        (_, Some("rs")) => "rust",
        (_, Some("ts")) => "typescript",
        (_, Some("tsx")) => "tsx",
        (_, Some("js" | "jsx" | "mjs" | "cjs")) => "javascript",
        ("application/json", _) | (_, Some("json" | "jsonc")) => "json",
        ("text/markdown", _) | (_, Some("md" | "mdx")) => "markdown",
        (_, Some("toml")) => "toml",
        (_, Some("yaml" | "yml")) => "yaml",
        ("text/html", _) | (_, Some("html" | "htm")) => "html",
        ("text/css", _) | (_, Some("css" | "scss")) => "css",
        (_, Some("py")) => "python",
        (_, Some("go")) => "go",
        (_, Some("c" | "h")) => "c",
        (_, Some("cc" | "cpp" | "cxx" | "hpp")) => "cpp",
        (_, Some("sh" | "bash" | "zsh" | "fish")) => "bash",
        (_, Some("java")) => "java",
        (_, Some("cs")) => "csharp",
        (_, Some("rb")) => "ruby",
        (_, Some("swift")) => "swift",
        (_, Some("zig")) => "zig",
        (_, Some("proto")) => "proto",
        (_, Some("graphql" | "gql")) => "graphql",
        (_, Some("cmake")) => "cmake",
        (_, Some("diff" | "patch")) => "diff",
        _ if name.eq_ignore_ascii_case("makefile") => "make",
        _ => "text",
    }
}

pub(super) fn position_for_byte_offset(text: &str, byte_offset: usize) -> Position {
    let mut byte_offset = byte_offset.min(text.len());
    while !text.is_char_boundary(byte_offset) {
        byte_offset -= 1;
    }
    let prefix = &text[..byte_offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() as u32;
    let current_line = prefix.rsplit_once('\n').map_or(prefix, |(_, line)| line);
    let character = current_line.encode_utf16().count() as u32;
    Position::new(line, character)
}

pub(super) fn byte_offset_for_position(text: &str, position: Position) -> usize {
    let line = position.line as usize;
    let mut line_start = 0;
    for _ in 0..line {
        let Some(offset) = text[line_start..].find('\n') else {
            return text.len();
        };
        line_start += offset + 1;
    }

    let line_end = text[line_start..]
        .find('\n')
        .map_or(text.len(), |offset| line_start + offset);
    let target_utf16 = position.character as usize;
    let mut consumed_utf16 = 0;
    for (offset, character) in text[line_start..line_end].char_indices() {
        let next = consumed_utf16 + character.len_utf16();
        if next > target_utf16 {
            return line_start + offset;
        }
        consumed_utf16 = next;
        if consumed_utf16 == target_utf16 {
            return line_start + offset + character.len_utf8();
        }
    }
    line_end
}

pub(super) fn byte_offset_for_utf16(text: &str, target: usize) -> usize {
    let mut consumed = 0;
    for (offset, character) in text.char_indices() {
        let next = consumed + character.len_utf16();
        if next > target {
            return offset;
        }
        consumed = next;
        if consumed == target {
            return offset + character.len_utf8();
        }
    }
    text.len()
}

pub(super) fn utf16_offset_for_byte(text: &str, byte_offset: usize) -> usize {
    let mut byte_offset = byte_offset.min(text.len());
    while byte_offset > 0 && !text.is_char_boundary(byte_offset) {
        byte_offset -= 1;
    }
    text[..byte_offset].encode_utf16().count()
}

impl EditorWorkspace {
    pub(crate) fn active_tab_id(&self) -> Option<String> {
        self.workspace.active_id.clone()
    }

    pub(crate) fn activate_tab(&mut self, id: &str) -> Option<String> {
        self.workspace.activate(id).then(|| {
            self.workspace
                .tabs
                .iter()
                .find(|tab| tab_key(tab) == id)
                .map(|tab| tab.file.file_id.clone())
        })?
    }

    pub(crate) fn activate_next_tab(&mut self) -> Option<String> {
        self.workspace.activate_next().then(|| {
            self.workspace.active_id.as_deref().and_then(|active_id| {
                self.workspace
                    .tabs
                    .iter()
                    .find(|tab| tab_key(tab) == active_id)
                    .map(|tab| tab.file.file_id.clone())
            })
        })?
    }

    pub(crate) fn activate_previous_tab(&mut self) -> Option<String> {
        self.workspace.activate_previous().then(|| {
            self.workspace.active_id.as_deref().and_then(|active_id| {
                self.workspace
                    .tabs
                    .iter()
                    .find(|tab| tab_key(tab) == active_id)
                    .map(|tab| tab.file.file_id.clone())
            })
        })?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_offsets_become_utf16_editor_positions() {
        let text = "第一行\n😀ab";
        let offset = text.find('a').unwrap();

        assert_eq!(position_for_byte_offset(text, offset), Position::new(1, 2));
        assert_eq!(position_for_byte_offset(text, 0), Position::new(0, 0));
        assert_eq!(
            position_for_byte_offset(text, text.find('😀').unwrap() + 1),
            Position::new(1, 0)
        );
    }

    #[test]
    fn editor_positions_and_utf16_offsets_round_trip_unicode() {
        let text = "第一行\n😀ab";
        let offset = text.find('a').unwrap();
        let position = position_for_byte_offset(text, offset);

        assert_eq!(byte_offset_for_position(text, position), offset);
        assert_eq!(
            byte_offset_for_utf16(text, utf16_offset_for_byte(text, offset)),
            offset
        );
        assert_eq!(
            byte_offset_for_position(text, Position::new(99, 0)),
            text.len()
        );
    }

    #[test]
    fn text_highlight_prefers_known_extensions_and_falls_back_to_plain_text() {
        assert_eq!(text_highlight_language("text/plain", "main.rs"), "rust");
        assert_eq!(
            text_highlight_language("application/json", "settings"),
            "json"
        );
        assert_eq!(
            text_highlight_language("application/octet-stream", "unknown"),
            "text"
        );
    }

    #[test]
    fn editor_metrics_are_one_based_and_unicode_aware() {
        assert_eq!(
            editor_metrics("你好\nabc", Position::new(1, 2)),
            EditorMetrics {
                lines: 2,
                characters: 6,
                cursor_line: 2,
                cursor_column: 3,
            }
        );
        assert_eq!(
            editor_metrics("", Position::new(0, 0)),
            EditorMetrics {
                lines: 1,
                characters: 0,
                cursor_line: 1,
                cursor_column: 1,
            }
        );
    }
}
