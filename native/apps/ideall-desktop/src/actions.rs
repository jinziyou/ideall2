use super::*;
use gpui::{KeyBinding, actions};

actions!(
    ideall,
    [
        FocusSearch,
        NewNote,
        SaveDocument,
        CloseTab,
        NextTab,
        PreviousTab
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
        self.show_files(window, cx);
        self.create_note(window, cx);
    }

    pub(super) fn save_from_shortcut(
        &mut self,
        _: &SaveDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dirty {
            self.save(window, cx);
        }
    }

    pub(super) fn close_active_tab(
        &mut self,
        _: &CloseTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(active_id) = self.workspace.active_id.clone() {
            self.close_tab(active_id, window, cx);
        }
    }

    pub(super) fn activate_next_tab(
        &mut self,
        _: &NextTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.activate_next()
            && let Some(active_id) = self.workspace.active_id.clone()
        {
            self.activate_tab(active_id, window, cx);
        }
    }

    pub(super) fn activate_previous_tab(
        &mut self,
        _: &PreviousTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.activate_previous()
            && let Some(active_id) = self.workspace.active_id.clone()
        {
            self.activate_tab(active_id, window, cx);
        }
    }
}
