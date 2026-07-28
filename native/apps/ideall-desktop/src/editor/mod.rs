use super::*;
use std::collections::HashMap;

use gpui_component::input::Position;

mod actions;
mod commands;
mod view;

pub(crate) use view::{EditorMode, engine_icon, kind_color_for_engine};

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
    pub(super) find_visible: bool,
    pub(super) replace_visible: bool,
    pub(super) find_cursor: usize,
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
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct DocumentSession {
    cursor: Position,
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
            find_visible: false,
            replace_visible: false,
            find_cursor: 0,
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
        let body = if self.active_engine_id.as_deref() == Some("ideall.text") {
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
        self.suppress_events = false;
        self.selected_kind = Some(document.kind);
        self.selected_id = Some(document.id);
        self.body_editable = document.editable;
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
        let cursor = self.active_input().read(cx).cursor_position();
        self.sessions.insert(id, DocumentSession { cursor });
    }

    pub(super) fn restore_active_session(
        &mut self,
        window: &mut Window,
        cx: &mut Context<IdeallDesktop>,
    ) {
        let Some(cursor) = self.pending_cursor_restore.take() else {
            return;
        };
        self.active_input().update(cx, |input, cx| {
            input.set_cursor_position(cursor, window, cx)
        });
    }

    pub(super) fn remove_session(&mut self, id: &str) {
        self.sessions.remove(id);
    }

    pub(super) fn active_input(&self) -> &Entity<InputState> {
        if self.active_engine_id.as_deref() == Some("ideall.text") {
            &self.text_input
        } else {
            &self.body_input
        }
    }

    pub(super) fn active_text(&self, cx: &Context<IdeallDesktop>) -> String {
        self.active_input().read(cx).value().to_string()
    }

    pub(super) fn find_match_count(&self, cx: &Context<IdeallDesktop>) -> usize {
        let query = self.find_input.read(cx).value().to_string();
        if query.is_empty() {
            return 0;
        }
        self.active_text(cx).match_indices(&query).count()
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
}
