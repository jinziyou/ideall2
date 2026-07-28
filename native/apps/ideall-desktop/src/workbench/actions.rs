use crate::*;
use gpui::{KeyBinding, actions};

actions!(
    ideall,
    [
        FocusSearch,
        NewNote,
        SaveDocument,
        CloseTab,
        NextTab,
        PreviousTab,
        ToggleProjectPanel,
        SelectRecentSection,
        SelectBrowserSection,
        SelectAppsSection,
        SelectSettingsSection,
        NewTextFile,
        NewFolder,
        ImportFiles,
        FindInEditor,
        ReplaceInEditor,
        OpenSelectedNode,
        RenameSelectedNode,
        TrashSelectedNode,
        SelectPreviousNode,
        SelectNextNode,
        FormatNoteBold,
        FormatNoteItalic,
        FormatNoteStrikethrough,
        FormatNoteCode,
        MoveNoteBlockUp,
        MoveNoteBlockDown,
        DuplicateNoteBlock
    ]
);

const CONTEXT: &str = "Ideall";

pub(super) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-k", FocusSearch, Some(CONTEXT)),
        KeyBinding::new("ctrl-k", FocusSearch, Some(CONTEXT)),
        KeyBinding::new("cmd-p", FocusSearch, Some(CONTEXT)),
        KeyBinding::new("ctrl-p", FocusSearch, Some(CONTEXT)),
        KeyBinding::new("cmd-n", NewNote, Some(CONTEXT)),
        KeyBinding::new("ctrl-n", NewNote, Some(CONTEXT)),
        KeyBinding::new("cmd-s", SaveDocument, Some(CONTEXT)),
        KeyBinding::new("ctrl-s", SaveDocument, Some(CONTEXT)),
        KeyBinding::new("cmd-w", CloseTab, Some(CONTEXT)),
        KeyBinding::new("ctrl-w", CloseTab, Some(CONTEXT)),
        KeyBinding::new("ctrl-tab", NextTab, Some(CONTEXT)),
        KeyBinding::new("ctrl-shift-tab", PreviousTab, Some(CONTEXT)),
        KeyBinding::new("alt-1", ToggleProjectPanel, Some(CONTEXT)),
        KeyBinding::new("alt-2", SelectRecentSection, Some(CONTEXT)),
        KeyBinding::new("alt-3", SelectBrowserSection, Some(CONTEXT)),
        KeyBinding::new("alt-4", SelectAppsSection, Some(CONTEXT)),
        KeyBinding::new("alt-5", SelectSettingsSection, Some(CONTEXT)),
        KeyBinding::new("cmd-shift-n", NewTextFile, Some(CONTEXT)),
        KeyBinding::new("ctrl-shift-n", NewTextFile, Some(CONTEXT)),
        KeyBinding::new("cmd-f", FindInEditor, Some(CONTEXT)),
        KeyBinding::new("ctrl-f", FindInEditor, Some(CONTEXT)),
        KeyBinding::new("cmd-alt-f", ReplaceInEditor, Some(CONTEXT)),
        KeyBinding::new("ctrl-alt-f", ReplaceInEditor, Some(CONTEXT)),
        KeyBinding::new("enter", OpenSelectedNode, Some(CONTEXT)),
        KeyBinding::new("f2", RenameSelectedNode, Some(CONTEXT)),
        KeyBinding::new("delete", TrashSelectedNode, Some(CONTEXT)),
        KeyBinding::new("up", SelectPreviousNode, Some(CONTEXT)),
        KeyBinding::new("down", SelectNextNode, Some(CONTEXT)),
        KeyBinding::new("cmd-b", FormatNoteBold, Some(CONTEXT)),
        KeyBinding::new("ctrl-b", FormatNoteBold, Some(CONTEXT)),
        KeyBinding::new("cmd-i", FormatNoteItalic, Some(CONTEXT)),
        KeyBinding::new("ctrl-i", FormatNoteItalic, Some(CONTEXT)),
        KeyBinding::new("cmd-shift-s", FormatNoteStrikethrough, Some(CONTEXT)),
        KeyBinding::new("ctrl-shift-s", FormatNoteStrikethrough, Some(CONTEXT)),
        KeyBinding::new("cmd-e", FormatNoteCode, Some(CONTEXT)),
        KeyBinding::new("ctrl-alt-e", FormatNoteCode, Some(CONTEXT)),
        KeyBinding::new("cmd-alt-up", MoveNoteBlockUp, Some(CONTEXT)),
        KeyBinding::new("ctrl-alt-up", MoveNoteBlockUp, Some(CONTEXT)),
        KeyBinding::new("cmd-alt-down", MoveNoteBlockDown, Some(CONTEXT)),
        KeyBinding::new("ctrl-alt-down", MoveNoteBlockDown, Some(CONTEXT)),
        KeyBinding::new("cmd-shift-d", DuplicateNoteBlock, Some(CONTEXT)),
        KeyBinding::new("ctrl-shift-d", DuplicateNoteBlock, Some(CONTEXT)),
    ]);
}

impl IdeallDesktop {
    pub(super) fn focus_search(
        &mut self,
        _: &FocusSearch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.search_input
            .update(cx, |input, cx| input.focus(window, cx));
    }

    pub(super) fn create_note_from_shortcut(
        &mut self,
        _: &NewNote,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_section.as_ref() != "我的" {
            self.show_files(window, cx);
        }
        self.create_note(window, cx);
    }

    pub(super) fn save_from_shortcut(
        &mut self,
        _: &SaveDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editor.dirty {
            self.save(window, cx);
        }
    }

    pub(super) fn close_active_tab(
        &mut self,
        _: &CloseTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(active_id) = self.editor.active_tab_id() {
            self.close_tab(active_id, window, cx);
        }
    }

    pub(super) fn activate_next_tab(
        &mut self,
        _: &NextTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(file_id) = self.editor.activate_next_tab() {
            self.open_node(file_id, window, cx);
            self.persist_workspace();
        }
    }

    pub(super) fn activate_previous_tab(
        &mut self,
        _: &PreviousTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(file_id) = self.editor.activate_previous_tab() {
            self.open_node(file_id, window, cx);
            self.persist_workspace();
        }
    }

    pub(super) fn toggle_project_panel_from_shortcut(
        &mut self,
        _: &ToggleProjectPanel,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_project_panel(cx);
    }

    pub(super) fn select_recent_section_from_shortcut(
        &mut self,
        _: &SelectRecentSection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_section("活动", window, cx);
    }

    pub(super) fn select_browser_section_from_shortcut(
        &mut self,
        _: &SelectBrowserSection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_section("浏览", window, cx);
    }

    pub(super) fn select_apps_section_from_shortcut(
        &mut self,
        _: &SelectAppsSection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_section("应用", window, cx);
    }

    pub(super) fn select_settings_section_from_shortcut(
        &mut self,
        _: &SelectSettingsSection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_section("设置", window, cx);
    }

    pub(super) fn create_text_file_from_shortcut(
        &mut self,
        _: &NewTextFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_section.as_ref() != "我的" {
            self.show_files(window, cx);
        }
        self.create_text_file(window, cx);
    }

    pub(super) fn create_folder_from_tree(
        &mut self,
        _: &NewFolder,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_section.as_ref() != "我的" {
            self.show_files(window, cx);
        }
        self.create_folder(window, cx);
    }

    pub(super) fn import_files_from_tree(
        &mut self,
        _: &ImportFiles,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_section.as_ref() != "我的" {
            self.show_files(window, cx);
        }
        self.import_files(cx);
    }

    pub(super) fn find_in_editor(
        &mut self,
        _: &FindInEditor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_editor_find(false, window, cx);
    }

    pub(super) fn replace_in_editor(
        &mut self,
        _: &ReplaceInEditor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_editor_find(true, window, cx);
    }

    pub(super) fn open_selected_node(
        &mut self,
        _: &OpenSelectedNode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_selected_explorer_node(window, cx);
    }

    pub(super) fn rename_selected_node(
        &mut self,
        _: &RenameSelectedNode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.begin_selected_explorer_rename(window, cx);
    }

    pub(super) fn trash_selected_node(
        &mut self,
        _: &TrashSelectedNode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.show_trash {
            self.move_explorer_selection_to_trash(window, cx);
        }
    }

    pub(super) fn select_previous_node(
        &mut self,
        _: &SelectPreviousNode,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_relative_explorer_node(-1, cx);
    }

    pub(super) fn select_next_node(
        &mut self,
        _: &SelectNextNode,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_relative_explorer_node(1, cx);
    }

    pub(super) fn format_note_bold_from_shortcut(
        &mut self,
        _: &FormatNoteBold,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.note_editor_focused(window, cx) {
            self.format_note_inline(NoteInlineStyle::Bold, window, cx);
        }
    }

    pub(super) fn format_note_italic_from_shortcut(
        &mut self,
        _: &FormatNoteItalic,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.note_editor_focused(window, cx) {
            self.format_note_inline(NoteInlineStyle::Italic, window, cx);
        }
    }

    pub(super) fn format_note_strikethrough_from_shortcut(
        &mut self,
        _: &FormatNoteStrikethrough,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.note_editor_focused(window, cx) {
            self.format_note_inline(NoteInlineStyle::Strikethrough, window, cx);
        }
    }

    pub(super) fn format_note_code_from_shortcut(
        &mut self,
        _: &FormatNoteCode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.note_editor_focused(window, cx) {
            self.format_note_inline(NoteInlineStyle::Code, window, cx);
        }
    }

    pub(super) fn move_note_block_up_from_shortcut(
        &mut self,
        _: &MoveNoteBlockUp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.note_editor_focused(window, cx) {
            self.move_current_note_block(NoteBlockMove::Up, window, cx);
        }
    }

    pub(super) fn move_note_block_down_from_shortcut(
        &mut self,
        _: &MoveNoteBlockDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.note_editor_focused(window, cx) {
            self.move_current_note_block(NoteBlockMove::Down, window, cx);
        }
    }

    pub(super) fn duplicate_note_block_from_shortcut(
        &mut self,
        _: &DuplicateNoteBlock,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.note_editor_focused(window, cx) {
            self.duplicate_current_note_block(window, cx);
        }
    }
}
